//! Shared CLI logic for the `sqb` tool.
//!
//! This module contains all platform-agnostic CLI commands so that the
//! Windows (`squashbox-windows`) and macOS (`squashbox-macos`) CLIs share
//! a single implementation. This eliminates any risk of the two CLIs
//! diverging in output format, command structure, or behavior.
//!
//! # Architecture
//!
//! Each platform crate defines its own `main()` and adds platform-specific
//! commands (e.g., `mount --force` on Windows, FSKit activation on macOS),
//! but delegates shared commands here:
//!
//! ```text
//! squashbox-windows/main.rs     squashbox-macos/main.rs
//!         │                              │
//!         ├─ image ──────→ cli::cmd_image()  ←── image
//!         ├─ mount ──────→ (platform-specific)
//!         └─ umount ─────→ (platform-specific)
//! ```
//!
//! # Adding New Commands
//!
//! To add a new shared command:
//! 1. Add it here with a `pub fn cmd_foo(...)` function
//! 2. Add matching subcommand entries to both platform CLIs
//!
//! To add a platform-specific command:
//! 1. Add it only in the relevant platform crate's `main.rs`

use crate::fmt::Table;
use crate::provider::VirtualFsProvider;
use crate::types::*;
use crate::SquashFsProvider;
use std::path::Path;

/// Table width used by all CLI output.
const TABLE_WIDTH: usize = 52;

/// Print detailed information about a SquashFS image.
///
/// This is the canonical implementation of `sqb image`. It uses only
/// `VirtualFsProvider` APIs and is fully platform-agnostic.
///
/// # Output
///
/// Prints a formatted report including:
/// - File metadata (path, size)
/// - Volume statistics (inodes, bytes, block size)
/// - Root directory attributes and entries
/// - Entry type distribution (2 levels deep)
/// - Probe of notable Unix paths
pub fn cmd_image(image_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if !image_path.exists() {
        return Err(format!("File not found: {}", image_path.display()).into());
    }

    let file_meta = std::fs::metadata(image_path)?;
    let file_size = file_meta.len();

    // ── Header ──
    let header = Table::new(TABLE_WIDTH)
        .header("Squashbox Image Inspector")
        .build();
    print!("{}", header);
    println!();
    println!("  File:       {}", image_path.display());
    println!(
        "  File size:  {} bytes ({:.1} MB)",
        file_size,
        file_size as f64 / 1_048_576.0
    );
    println!();

    // ── Open image ──
    print!("  Opening image... ");
    let start = std::time::Instant::now();
    let provider = SquashFsProvider::open(image_path)?;
    let open_time = start.elapsed();
    println!("done in {:.2?}", open_time);
    println!();

    // ── Volume stats ──
    let stats = provider.volume_stats()?;
    let volume_table = Table::new(TABLE_WIDTH)
        .section("Volume Stats")
        .kv("Total inodes", &format!("{}", stats.total_inodes))
        .kv(
            "Total bytes",
            &format!(
                "{} ({:.1} MB)",
                stats.total_bytes,
                stats.total_bytes as f64 / 1_048_576.0
            ),
        )
        .kv(
            "Used bytes",
            &format!(
                "{} ({:.1} MB)",
                stats.used_bytes,
                stats.used_bytes as f64 / 1_048_576.0
            ),
        )
        .kv("Block size", &format!("{}", stats.block_size))
        .end_section()
        .build();
    print!("{}", volume_table);
    println!();

    // ── Root directory attributes ──
    let root_attrs = provider.get_attributes(ROOT_INODE)?;
    let root_table = Table::new(TABLE_WIDTH)
        .section("Root Directory")
        .kv("Mode", &format!("0o{:o}", root_attrs.mode))
        .kv("UID", &format!("{}", root_attrs.uid))
        .kv("GID", &format!("{}", root_attrs.gid))
        .kv("Nlink", &format!("{}", root_attrs.nlink))
        .end_section()
        .build();
    print!("{}", root_table);
    println!();

    // ── Root entries ──
    let mut all_root_entries = Vec::new();
    let mut cookie = 0u64;
    loop {
        let batch = provider.list_directory(ROOT_INODE, cookie)?;
        all_root_entries.extend(batch.entries);
        if batch.next_cookie == 0 {
            break;
        }
        cookie = batch.next_cookie;
    }

    let mut entries_table = Table::new(TABLE_WIDTH)
        .section(&format!("Root Entries ({})", all_root_entries.len()));

    for entry in &all_root_entries {
        let type_char = match entry.attributes.entry_type {
            EntryType::Directory => "📁",
            EntryType::File => "📄",
            EntryType::Symlink => "🔗",
            EntryType::CharDevice => "🔌",
            EntryType::BlockDevice => "💾",
        };
        let extra = if entry.attributes.entry_type == EntryType::Symlink {
            match provider.read_symlink(entry.attributes.inode) {
                Ok(target) => format!(" -> {}", target),
                Err(_) => String::new(),
            }
        } else if entry.attributes.entry_type == EntryType::File {
            format!(" ({} bytes)", entry.attributes.size)
        } else {
            String::new()
        };
        entries_table = entries_table.row(&format!("{} {}{}", type_char, entry.name, extra));
    }

    let entries_output = entries_table.end_section().build();
    print!("{}", entries_output);
    println!();

    // ── Entry type distribution (2 levels deep) ──
    let mut type_counts: std::collections::HashMap<EntryType, u32> =
        std::collections::HashMap::new();
    let mut total_two_levels = 0u32;
    for entry in &all_root_entries {
        *type_counts.entry(entry.attributes.entry_type).or_insert(0) += 1;
        total_two_levels += 1;

        if entry.attributes.is_dir() {
            let mut cookie = 0u64;
            loop {
                let batch = provider.list_directory(entry.attributes.inode, cookie)?;
                for child in &batch.entries {
                    *type_counts
                        .entry(child.attributes.entry_type)
                        .or_insert(0) += 1;
                    total_two_levels += 1;
                }
                if batch.next_cookie == 0 {
                    break;
                }
                cookie = batch.next_cookie;
            }
        }
    }

    let mut types_table = Table::new(TABLE_WIDTH).section(&format!(
        "Entry Types (2 levels, {} total)",
        total_two_levels
    ));

    for (etype, count) in &type_counts {
        types_table = types_table.kv(&format!("{}", etype), &format!("{}", count));
    }

    let types_output = types_table.end_section().build();
    print!("{}", types_output);
    println!();

    // ── Notable paths ──
    let notable_paths = ["etc/passwd", "usr/bin", "usr/share", "usr/lib"];
    let mut paths_table = Table::new(TABLE_WIDTH).section("Notable Paths");

    for path_str in &notable_paths {
        let path = std::path::Path::new(path_str);
        match provider.resolve_path(path)? {
            Some(inode) => {
                let attrs = provider.get_attributes(inode)?;
                if attrs.is_dir() {
                    let mut count = 0u32;
                    let mut cookie = 0u64;
                    loop {
                        let batch = provider.list_directory(inode, cookie)?;
                        count += batch.entries.len() as u32;
                        if batch.next_cookie == 0 {
                            break;
                        }
                        cookie = batch.next_cookie;
                    }
                    paths_table =
                        paths_table.row(&format!("/{:<20} 📁 {} entries", path_str, count));
                } else {
                    paths_table =
                        paths_table.row(&format!("/{:<20} 📄 {} bytes", path_str, attrs.size));
                }
            }
            None => {
                paths_table = paths_table.row(&format!("/{:<20} ❌ not found", path_str));
            }
        }
    }

    let paths_output = paths_table.end_section().build();
    print!("{}", paths_output);

    Ok(())
}

/// Validate that a SquashFS image can be opened, returning summary info.
///
/// This is the shared validation step used by both platform `mount` commands
/// before they proceed with platform-specific mounting logic.
///
/// Returns `(provider, volume_stats)` on success.
pub fn validate_image(
    image_path: &Path,
) -> Result<(SquashFsProvider, VolumeStats), Box<dyn std::error::Error>> {
    if !image_path.exists() {
        return Err(format!("Image file not found: {}", image_path.display()).into());
    }

    log::info!("Opening SquashFS image: {}", image_path.display());
    let provider = SquashFsProvider::open(image_path)?;

    let stats = provider.volume_stats()?;
    log::info!(
        "Image opened: {} inodes, {} bytes",
        stats.total_inodes,
        stats.total_bytes,
    );

    Ok((provider, stats))
}
