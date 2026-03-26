//! Squashbox CLI (`sqb`) — SquashFS native filesystem tools for Windows.
//!
//! Usage:
//!   sqb image <FILE>        Print image info (inodes, size, block size, root entries)
//!   sqb mount <FILE> <DIR>  Mount a SquashFS image as a ProjFS projected filesystem

use clap::{Parser, Subcommand};
use squashbox_core::provider::VirtualFsProvider;
use squashbox_core::types::*;
use squashbox_core::SquashFsProvider;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "sqb", about = "Squashbox — native SquashFS tools", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Print detailed information about a SquashFS image
    Image {
        /// Path to the SquashFS image file
        file: PathBuf,
    },
    /// Mount a SquashFS image as a Windows projected filesystem
    Mount {
        /// Path to the SquashFS image file
        file: PathBuf,
        /// Directory to mount the image at (will be created if needed)
        dir: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let cli = Cli::parse();
    match cli.command {
        Commands::Image { file } => cmd_image(&file),
        Commands::Mount { file, dir } => cmd_mount(&file, &dir),
    }
}

fn cmd_image(image_path: &PathBuf) -> anyhow::Result<()> {
    if !image_path.exists() {
        anyhow::bail!("File not found: {}", image_path.display());
    }

    let file_meta = std::fs::metadata(image_path)?;
    let file_size = file_meta.len();

    println!("╔══════════════════════════════════════════════════╗");
    println!("║           Squashbox Image Inspector              ║");
    println!("╚══════════════════════════════════════════════════╝");
    println!();
    println!("  File:       {}", image_path.display());
    println!("  File size:  {} bytes ({:.1} MB)", file_size, file_size as f64 / 1_048_576.0);
    println!();

    print!("  Opening image... ");
    let start = std::time::Instant::now();
    let provider = SquashFsProvider::open(image_path)?;
    let open_time = start.elapsed();
    println!("done in {:.2?}", open_time);
    println!();

    // Volume stats
    let stats = provider.volume_stats()?;
    println!("  ┌─ Volume Stats ───────────────────────────────┐");
    println!("  │ Total inodes:  {:>10}                     │", stats.total_inodes);
    println!("  │ Total bytes:   {:>10} ({:>7.1} MB)       │", stats.total_bytes, stats.total_bytes as f64 / 1_048_576.0);
    println!("  │ Used bytes:    {:>10} ({:>7.1} MB)       │", stats.used_bytes, stats.used_bytes as f64 / 1_048_576.0);
    println!("  │ Block size:    {:>10}                     │", stats.block_size);
    println!("  └──────────────────────────────────────────────┘");
    println!();

    // Root attributes
    let root_attrs = provider.get_attributes(ROOT_INODE)?;
    println!("  ┌─ Root Directory ─────────────────────────────┐");
    println!("  │ Mode:   0o{:<5o}                             │", root_attrs.mode);
    println!("  │ UID:    {:<10}                            │", root_attrs.uid);
    println!("  │ GID:    {:<10}                            │", root_attrs.gid);
    println!("  │ Nlink:  {:<10}                            │", root_attrs.nlink);
    println!("  └──────────────────────────────────────────────┘");
    println!();

    // Root entries
    let mut all_root_entries = Vec::new();
    let mut cookie = 0u64;
    loop {
        let batch = provider.list_directory(ROOT_INODE, cookie)?;
        all_root_entries.extend(batch.entries);
        if batch.next_cookie == 0 { break; }
        cookie = batch.next_cookie;
    }

    println!("  ┌─ Root Entries ({}) ─────────────────────────┐", all_root_entries.len());
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
        println!("  │ {} {:<30}{}", type_char, entry.name, extra);
    }
    println!("  └──────────────────────────────────────────────┘");
    println!();

    // Entry type distribution (2 levels deep)
    let mut type_counts: std::collections::HashMap<EntryType, u32> = std::collections::HashMap::new();
    let mut total_two_levels = 0u32;
    for entry in &all_root_entries {
        *type_counts.entry(entry.attributes.entry_type).or_insert(0) += 1;
        total_two_levels += 1;

        if entry.attributes.is_dir() {
            let mut cookie = 0u64;
            loop {
                let batch = provider.list_directory(entry.attributes.inode, cookie)?;
                for child in &batch.entries {
                    *type_counts.entry(child.attributes.entry_type).or_insert(0) += 1;
                    total_two_levels += 1;
                }
                if batch.next_cookie == 0 { break; }
                cookie = batch.next_cookie;
            }
        }
    }

    println!("  ┌─ Entry Types (2 levels, {} total) ──────────┐", total_two_levels);
    for (etype, count) in &type_counts {
        println!("  │ {:<15} {:>8}                        │", format!("{}", etype), count);
    }
    println!("  └──────────────────────────────────────────────┘");
    println!();

    // Notable paths
    let notable_paths = ["etc/passwd", "usr/bin", "usr/share", "usr/lib"];
    println!("  ┌─ Notable Paths ──────────────────────────────┐");
    for path_str in &notable_paths {
        let path = std::path::Path::new(path_str);
        match provider.resolve_path(path)? {
            Some(inode) => {
                let attrs = provider.get_attributes(inode)?;
                if attrs.is_dir() {
                    // Count children
                    let mut count = 0u32;
                    let mut cookie = 0u64;
                    loop {
                        let batch = provider.list_directory(inode, cookie)?;
                        count += batch.entries.len() as u32;
                        if batch.next_cookie == 0 { break; }
                        cookie = batch.next_cookie;
                    }
                    println!("  │ /{:<20} 📁 {} entries             │", path_str, count);
                } else {
                    println!("  │ /{:<20} 📄 {} bytes              │", path_str, attrs.size);
                }
            }
            None => {
                println!("  │ /{:<20} ❌ not found               │", path_str);
            }
        }
    }
    println!("  └──────────────────────────────────────────────┘");

    Ok(())
}

fn cmd_mount(image_path: &PathBuf, mount_point: &PathBuf) -> anyhow::Result<()> {
    if !image_path.exists() {
        anyhow::bail!("Image file not found: {}", image_path.display());
    }

    if !mount_point.exists() {
        std::fs::create_dir_all(mount_point)?;
    }

    log::info!("Opening SquashFS image: {}", image_path.display());
    let provider: Arc<dyn VirtualFsProvider> = Arc::new(SquashFsProvider::open(image_path)?);

    let stats = provider.volume_stats()?;
    log::info!(
        "Image opened: {} inodes, {} bytes",
        stats.total_inodes,
        stats.total_bytes
    );

    let source = squashbox_windows::projfs_source::SquashboxProjFsSource::new(provider);

    log::info!("Starting ProjFS at: {}", mount_point.display());
    let _pfs = windows_projfs::ProjectedFileSystem::new(mount_point, source)?;

    println!(
        "✓ Mounted {} at {}",
        image_path.display(),
        mount_point.display()
    );
    println!("Press Ctrl+C to unmount...");

    let (tx, rx) = std::sync::mpsc::channel();
    ctrlc::set_handler(move || {
        let _ = tx.send(());
    })?;
    rx.recv()?;

    println!("Unmounting...");
    Ok(())
}
