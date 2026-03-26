//! Stress tests using a real Ubuntu SquashFS filesystem image.
//!
//! These tests exercise the full `SquashFsProvider` and `SquashboxProjFsSource`
//! against a production-sized image (hundreds of MB, thousands of inodes).
//!
//! # Setup
//!
//! Set the `SQUASHBOX_UBUNTU_IMAGE` environment variable to the path of a
//! SquashFS image before running:
//!
//! ```text
//! $env:SQUASHBOX_UBUNTU_IMAGE = "C:\path\to\filesystem.squashfs"
//! cargo test -p squashbox-core --features ubuntu_stress
//! ```
//!
//! If the variable is not set, all tests in this file are silently skipped.
#![cfg(feature = "ubuntu_stress")]

use squashbox_core::provider::VirtualFsProvider;
use squashbox_core::types::*;
use squashbox_core::SquashFsProvider;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

/// Locate the Ubuntu squashfs image via `SQUASHBOX_UBUNTU_IMAGE` env var.
/// Returns `None` (skip) if the variable is unset or the path doesn't exist.
fn ubuntu_image_path() -> Option<PathBuf> {
    let path = std::env::var("SQUASHBOX_UBUNTU_IMAGE").ok()?;
    let p = PathBuf::from(&path);
    if p.exists() {
        Some(p)
    } else {
        eprintln!(
            "SKIP: SQUASHBOX_UBUNTU_IMAGE is set to {:?} but the file does not exist.",
            path
        );
        None
    }
}

/// Open the Ubuntu image or skip the test.
fn open_ubuntu_image() -> Option<SquashFsProvider> {
    let path = ubuntu_image_path()?;
    eprintln!("Using Ubuntu image at: {}", path.display());
    let start = Instant::now();
    let provider = SquashFsProvider::open(&path).expect("Failed to open Ubuntu image! ❌");
    eprintln!("Image opened and indexed in {:?}", start.elapsed());
    Some(provider)
}

// ═══════════════════════════════════════════════════════════
// Core provider tests on a real Ubuntu filesystem
// ═══════════════════════════════════════════════════════════

#[test]
fn ubuntu_open_and_index() {
    let Some(p) = open_ubuntu_image() else { return };
    let stats = p.volume_stats().unwrap();

    eprintln!("  Total inodes: {}", stats.total_inodes);
    eprintln!("  Total bytes:  {} ({:.1} MB)", stats.total_bytes, stats.total_bytes as f64 / 1_048_576.0);
    eprintln!("  Block size:   {}", stats.block_size);

    // Ubuntu images have thousands of files
    assert!(stats.total_inodes > 1000, "expected many inodes, got {}", stats.total_inodes);
    assert!(stats.block_size > 0);
}

#[test]
fn ubuntu_root_is_directory() {
    let Some(p) = open_ubuntu_image() else { return };
    let attrs = p.get_attributes(ROOT_INODE).unwrap();
    assert!(attrs.is_dir());
    assert_eq!(attrs.entry_type, EntryType::Directory);
}

#[test]
fn ubuntu_root_has_standard_directories() {
    let Some(p) = open_ubuntu_image() else { return };

    // Every Ubuntu root should have these
    let expected_dirs = ["bin", "etc", "usr", "var", "lib"];
    let batch = p.list_directory(ROOT_INODE, 0).unwrap();
    let root_names: Vec<&str> = batch.entries.iter().map(|e| e.name.as_str()).collect();

    for dir in &expected_dirs {
        assert!(
            root_names.contains(dir),
            "root should contain '{dir}', found: {root_names:?}"
        );
    }
}

#[test]
fn ubuntu_resolve_deep_paths() {
    let Some(p) = open_ubuntu_image() else { return };

    // Standard paths that should exist in any Ubuntu rootfs
    let test_paths = [
        "etc",
        "usr",
        "usr/bin",
        "usr/lib",
        "usr/share",
        "etc/passwd",
        "etc/hostname",
    ];

    for path_str in &test_paths {
        let result = p.resolve_path(Path::new(path_str)).unwrap();
        assert!(
            result.is_some(),
            "expected '{}' to resolve, got None",
            path_str
        );

        // Verify the resolved inode's attributes are accessible
        let inode = result.unwrap();
        let attrs = p.get_attributes(inode).unwrap();
        assert_eq!(attrs.inode, inode);
    }
}

#[test]
fn ubuntu_resolve_with_platform_separators() {
    let Some(p) = open_ubuntu_image() else { return };

    // Test that both forward and backslash separators work
    let forward = p.resolve_path(Path::new("usr/bin")).unwrap();
    let backward = p.resolve_path(Path::new("usr\\bin")).unwrap();
    assert_eq!(forward, backward, "path separator normalization should make these equivalent");
    assert!(forward.is_some());
}

#[test]
fn ubuntu_read_etc_passwd() {
    let Some(p) = open_ubuntu_image() else { return };

    let inode = match p.resolve_path(Path::new("etc/passwd")).unwrap() {
        Some(id) => id,
        None => {
            eprintln!("SKIP: /etc/passwd not found");
            return;
        }
    };

    let attrs = p.get_attributes(inode).unwrap();
    assert!(attrs.is_file());
    assert!(attrs.size > 0, "/etc/passwd should not be empty");

    // Read the full file
    let data = p.read_file(inode, 0, attrs.size).unwrap();
    assert_eq!(data.len(), attrs.size as usize);

    // It should be valid UTF-8 text containing "root"
    let content = String::from_utf8_lossy(&data);
    assert!(
        content.contains("root"),
        "/etc/passwd should contain 'root'"
    );
}

#[test]
fn ubuntu_read_file_with_offset_accuracy() {
    let Some(p) = open_ubuntu_image() else { return };

    let inode = match p.resolve_path(Path::new("etc/passwd")).unwrap() {
        Some(id) => id,
        None => return,
    };

    let full = p.read_file(inode, 0, 1_000_000).unwrap();
    assert!(!full.is_empty());

    // Read a chunk from the middle and verify it matches
    let offset = full.len() / 2;
    let length = 64.min(full.len() - offset);
    let chunk = p.read_file(inode, offset as u64, length as u64).unwrap();

    assert_eq!(
        chunk,
        &full[offset..offset + length],
        "partial read at offset {} should match full read slice",
        offset
    );
}

#[test]
fn ubuntu_list_directory_all_entries() {
    let Some(p) = open_ubuntu_image() else { return };

    // List /usr/bin — should have many entries (hundreds of binaries)
    let usr_bin = match p.resolve_path(Path::new("usr/bin")).unwrap() {
        Some(id) => id,
        None => {
            eprintln!("SKIP: /usr/bin not found");
            return;
        }
    };

    // Paginate through all entries
    let mut all_entries = Vec::new();
    let mut cookie = 0u64;
    let mut pages = 0u32;
    loop {
        let batch = p.list_directory(usr_bin, cookie).unwrap();
        all_entries.extend(batch.entries);
        pages += 1;
        if batch.next_cookie == 0 {
            break;
        }
        cookie = batch.next_cookie;
    }

    eprintln!("  /usr/bin: {} entries across {} pages", all_entries.len(), pages);
    assert!(
        all_entries.len() > 50,
        "/usr/bin should have many binaries, got {}",
        all_entries.len()
    );

    // Entries should be sorted by name
    for window in all_entries.windows(2) {
        assert!(
            window[0].name <= window[1].name,
            "entries should be sorted: '{}' should come before '{}'",
            window[0].name,
            window[1].name
        );
    }
}

#[test]
fn ubuntu_symlinks() {
    let Some(p) = open_ubuntu_image() else { return };

    // Ubuntu typically has symlinks in the root (e.g., bin -> usr/bin)
    let batch = p.list_directory(ROOT_INODE, 0).unwrap();

    let symlinks: Vec<_> = batch
        .entries
        .iter()
        .filter(|e| e.attributes.entry_type == EntryType::Symlink)
        .collect();

    if symlinks.is_empty() {
        eprintln!("SKIP: no symlinks in root directory");
        return;
    }

    eprintln!("  Found {} symlinks in root", symlinks.len());

    for link_entry in &symlinks {
        let target = p.read_symlink(link_entry.attributes.inode).unwrap();
        assert!(
            !target.is_empty(),
            "symlink '{}' should have a non-empty target",
            link_entry.name
        );
        eprintln!("  {} -> {}", link_entry.name, target);
    }
}

#[test]
fn ubuntu_all_inodes_have_valid_attributes() {
    let Some(p) = open_ubuntu_image() else { return };

    // Walk every entry in root and verify all children have valid attributes
    let root_batch = p.list_directory(ROOT_INODE, 0).unwrap();
    let mut checked = 0u32;

    for entry in &root_batch.entries {
        let attrs = p.get_attributes(entry.attributes.inode).unwrap();
        assert_eq!(attrs.inode, entry.attributes.inode);
        checked += 1;

        // If it's a directory, list it and check those children too
        if attrs.is_dir() {
            let mut cookie = 0u64;
            loop {
                let batch = p.list_directory(entry.attributes.inode, cookie).unwrap();
                for child in &batch.entries {
                    let child_attrs = p.get_attributes(child.attributes.inode).unwrap();
                    assert_eq!(child_attrs.inode, child.attributes.inode);
                    checked += 1;
                }
                if batch.next_cookie == 0 {
                    break;
                }
                cookie = batch.next_cookie;
            }
        }
    }

    eprintln!("  Verified attributes for {} inodes (2 levels deep)", checked);
    assert!(checked > 100, "should check many inodes, got {}", checked);
}

#[test]
fn ubuntu_concurrent_access() {
    let Some(p) = open_ubuntu_image() else { return };
    let p = Arc::new(p);

    let start = Instant::now();
    let mut handles = Vec::new();

    // 8 threads doing different operations simultaneously
    for thread_id in 0..8u32 {
        let p = Arc::clone(&p);
        handles.push(std::thread::spawn(move || {
            match thread_id % 4 {
                0 => {
                    // Thread: list root
                    let batch = p.list_directory(ROOT_INODE, 0).unwrap();
                    assert!(!batch.entries.is_empty());
                }
                1 => {
                    // Thread: resolve deep path
                    let result = p.resolve_path(Path::new("usr/bin")).unwrap();
                    assert!(result.is_some());
                }
                2 => {
                    // Thread: read /etc/passwd
                    if let Some(inode) = p.resolve_path(Path::new("etc/passwd")).unwrap() {
                        let data = p.read_file(inode, 0, 1024).unwrap();
                        assert!(!data.is_empty());
                    }
                }
                3 => {
                    // Thread: volume stats
                    let stats = p.volume_stats().unwrap();
                    assert!(stats.total_inodes > 0);
                }
                _ => unreachable!(),
            }
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    eprintln!("  8-thread concurrent access completed in {:?}", start.elapsed());
}

// ═══════════════════════════════════════════════════════════
// ProjFS adapter tests on real Ubuntu image
// ═══════════════════════════════════════════════════════════

#[test]
fn ubuntu_projfs_list_root() {
    let Some(p) = open_ubuntu_image() else { return };
    let provider: Arc<dyn VirtualFsProvider> = Arc::new(p);

    // Simulate what ProjFS would do: list_directory via the trait
    let batch = provider.list_directory(ROOT_INODE, 0).unwrap();
    assert!(!batch.entries.is_empty());

    // Every entry should be lookupable by name
    for entry in &batch.entries {
        let found = provider.lookup(ROOT_INODE, &entry.name).unwrap();
        assert!(found.is_some(), "lookup('{}') returned None", entry.name);
    }
}

#[test]
fn ubuntu_projfs_simulate_directory_walk() {
    let Some(p) = open_ubuntu_image() else { return };
    let provider: Arc<dyn VirtualFsProvider> = Arc::new(p);

    // Simulate a file manager opening /usr/share
    let usr = provider.resolve_path(Path::new("usr")).unwrap().expect("usr should exist");
    let share = provider.lookup(usr, "share").unwrap().expect("share should exist");

    let mut all_entries = Vec::new();
    let mut cookie = 0u64;
    loop {
        let batch = provider.list_directory(share.attributes.inode, cookie).unwrap();
        all_entries.extend(batch.entries);
        if batch.next_cookie == 0 {
            break;
        }
        cookie = batch.next_cookie;
    }

    eprintln!("  /usr/share: {} entries", all_entries.len());
    assert!(all_entries.len() > 20, "usr/share should have many entries");
}

#[test]
fn ubuntu_projfs_stream_file_content() {
    let Some(p) = open_ubuntu_image() else { return };
    let provider: Arc<dyn VirtualFsProvider> = Arc::new(p);

    // Simulate ProjFS requesting file data
    let inode = match provider.resolve_path(Path::new("etc/passwd")).unwrap() {
        Some(id) => id,
        None => return,
    };

    let attrs = provider.get_attributes(inode).unwrap();
    let file_size = attrs.size;

    // Read in 4KB chunks (like ProjFS would)
    let chunk_size = 4096u64;
    let mut offset = 0u64;
    let mut total_read = 0u64;

    while offset < file_size {
        let request_len = chunk_size.min(file_size - offset);
        let data = provider.read_file(inode, offset, request_len).unwrap();
        assert!(!data.is_empty() || offset >= file_size);
        total_read += data.len() as u64;
        offset += request_len;
    }

    assert_eq!(total_read, file_size, "chunked read should cover full file");
}

#[test]
fn ubuntu_entry_type_coverage() {
    let Some(p) = open_ubuntu_image() else { return };

    // Walk root + first-level dirs and count entry types
    let mut type_counts = std::collections::HashMap::new();
    let root_batch = p.list_directory(ROOT_INODE, 0).unwrap();

    for entry in &root_batch.entries {
        *type_counts.entry(entry.attributes.entry_type).or_insert(0u32) += 1;

        if entry.attributes.is_dir() {
            let mut cookie = 0u64;
            loop {
                let batch = p.list_directory(entry.attributes.inode, cookie).unwrap();
                for child in &batch.entries {
                    *type_counts.entry(child.attributes.entry_type).or_insert(0u32) += 1;
                }
                if batch.next_cookie == 0 {
                    break;
                }
                cookie = batch.next_cookie;
            }
        }
    }

    eprintln!("  Entry type distribution:");
    for (etype, count) in &type_counts {
        eprintln!("    {}: {}", etype, count);
    }

    // Ubuntu should have files and directories at minimum
    assert!(
        type_counts.get(&EntryType::File).copied().unwrap_or(0) > 0,
        "should have regular files"
    );
    assert!(
        type_counts.get(&EntryType::Directory).copied().unwrap_or(0) > 0,
        "should have directories"
    );
}
