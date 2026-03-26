//! Integration tests for squashbox-core.
//!
//! These tests require a test fixture SquashFS image at
//! `tests/fixtures/test.sqsh`. If the fixture doesn't exist,
//! tests skip gracefully.
//!
//! To create the fixture, run:
//!   cd crates/squashbox-core && cargo test --test create_fixture
//! Or manually create it with mksquashfs.

use squashbox_core::*;
use std::path::{Path, PathBuf};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

fn test_image_path() -> PathBuf {
    fixtures_dir().join("test.sqsh")
}

fn has_fixture() -> bool {
    test_image_path().exists()
}

/// Helper to get a provider or skip.
fn provider() -> Option<SquashFsProvider> {
    if !has_fixture() {
        eprintln!("SKIP: test fixture not found at {:?}", test_image_path());
        return None;
    }
    Some(SquashFsProvider::open(&test_image_path()).unwrap())
}

// ═══════════════════════════════════════════════════════════
// Integration: End-to-end lifecycle
// ═══════════════════════════════════════════════════════════

#[test]
fn integration_full_lifecycle() {
    let Some(p) = provider() else { return };

    // 1. Mount: open the image (already done)

    // 2. List root
    let root_batch = p.list_directory(ROOT_INODE, 0).unwrap();
    assert!(!root_batch.entries.is_empty(), "root should have entries");

    // 3. For each entry in root, verify we can get_attributes
    for entry in &root_batch.entries {
        let attrs = p.get_attributes(entry.attributes.inode).unwrap();
        assert_eq!(attrs.inode, entry.attributes.inode);
        assert_eq!(attrs.entry_type, entry.attributes.entry_type);
    }

    // 4. Lookup a known file
    let hello = p.lookup(ROOT_INODE, "hello.txt").unwrap();
    assert!(hello.is_some(), "hello.txt should exist");
    let hello = hello.unwrap();
    assert!(hello.attributes.is_file());

    // 5. Read the file
    let data = p.read_file(hello.attributes.inode, 0, 1_000_000).unwrap();
    assert!(!data.is_empty());
    let content = String::from_utf8_lossy(&data);
    assert!(
        content.contains("Hello"),
        "file should contain 'Hello', got: {content}"
    );

    // 6. Resolve the same file via path
    let inode = p.resolve_path(Path::new("hello.txt")).unwrap();
    assert_eq!(inode, Some(hello.attributes.inode));

    // 7. Volume stats
    let stats = p.volume_stats().unwrap();
    assert!(stats.total_inodes > 0);
    assert!(stats.block_size > 0);
}

#[test]
fn integration_resolve_path_traversal() {
    let Some(p) = provider() else { return };

    // List root to find all top-level entries
    let root_batch = p.list_directory(ROOT_INODE, 0).unwrap();

    for entry in &root_batch.entries {
        // Each root entry should be resolvable by name
        let inode = p.resolve_path(Path::new(&entry.name)).unwrap();
        assert_eq!(
            inode,
            Some(entry.attributes.inode),
            "resolve_path('{}') should return inode {}",
            entry.name,
            entry.attributes.inode
        );
    }
}

#[test]
fn integration_directory_pagination() {
    let Some(p) = provider() else { return };

    // Get all entries by paginating
    let mut all_entries = Vec::new();
    let mut cookie = 0u64;
    loop {
        let batch = p.list_directory(ROOT_INODE, cookie).unwrap();
        all_entries.extend(batch.entries);
        if batch.next_cookie == 0 {
            break;
        }
        cookie = batch.next_cookie;
    }

    // Should match a single non-paginated fetch (if small enough)
    let single_batch = p.list_directory(ROOT_INODE, 0).unwrap();
    if single_batch.next_cookie == 0 {
        // All entries fit in one page
        assert_eq!(all_entries.len(), single_batch.entries.len());
    }
}

#[test]
fn integration_read_file_byte_accuracy() {
    let Some(p) = provider() else { return };

    let inode = match p.resolve_path(Path::new("hello.txt")).unwrap() {
        Some(id) => id,
        None => return,
    };

    // Read full file
    let full = p.read_file(inode, 0, 1_000_000).unwrap();

    // Read byte by byte and reconstruct
    let mut reconstructed = Vec::new();
    for i in 0..full.len() {
        let byte = p.read_file(inode, i as u64, 1).unwrap();
        assert_eq!(byte.len(), 1, "single byte read at offset {i}");
        reconstructed.push(byte[0]);
    }

    assert_eq!(full, reconstructed, "byte-by-byte reconstruction should match");
}

#[test]
fn integration_read_file_past_eof() {
    let Some(p) = provider() else { return };

    let inode = match p.resolve_path(Path::new("hello.txt")).unwrap() {
        Some(id) => id,
        None => return,
    };

    let full = p.read_file(inode, 0, 1_000_000).unwrap();

    // Read starting past EOF
    let empty = p.read_file(inode, full.len() as u64 + 100, 10).unwrap();
    assert!(empty.is_empty());

    // Read overlapping EOF
    if full.len() > 5 {
        let partial = p
            .read_file(inode, (full.len() - 3) as u64, 100)
            .unwrap();
        assert_eq!(partial.len(), 3);
        assert_eq!(partial, &full[full.len() - 3..]);
    }
}

#[test]
fn integration_nonexistent_paths() {
    let Some(p) = provider() else { return };

    assert_eq!(p.resolve_path(Path::new("does_not_exist")).unwrap(), None);
    assert_eq!(
        p.resolve_path(Path::new("a/b/c/d/e/f")).unwrap(),
        None
    );
    assert_eq!(
        p.resolve_path(Path::new("hello.txt/child")).unwrap(),
        None
    );
}

#[test]
fn integration_error_types() {
    let Some(p) = provider() else { return };

    // NotFound for nonexistent inode
    assert!(matches!(
        p.get_attributes(999999),
        Err(CoreError::NotFound(_))
    ));

    // NotADirectory for file
    if let Some(inode) = p.resolve_path(Path::new("hello.txt")).unwrap() {
        assert!(matches!(
            p.list_directory(inode, 0),
            Err(CoreError::NotADirectory(_))
        ));
        assert!(matches!(
            p.lookup(inode, "anything"),
            Err(CoreError::NotADirectory(_))
        ));
    }

    // NotAFile for directory
    assert!(matches!(
        p.read_file(ROOT_INODE, 0, 10),
        Err(CoreError::NotAFile(_))
    ));

    // NotASymlink for directory
    assert!(matches!(
        p.read_symlink(ROOT_INODE),
        Err(CoreError::NotASymlink(_))
    ));
}

#[test]
fn integration_access_control() {
    let Some(p) = provider() else { return };

    // Read-only FS: write always denied
    assert!(!p.check_access(ROOT_INODE, 2).unwrap()); // W_OK
    assert!(!p.check_access(ROOT_INODE, 6).unwrap()); // R_OK | W_OK
    assert!(!p.check_access(ROOT_INODE, 7).unwrap()); // R_OK | W_OK | X_OK

    // Read should be allowed for root (mode 0755 → other has read+execute)
    assert!(p.check_access(ROOT_INODE, 4).unwrap()); // R_OK
}

#[test]
fn integration_multiple_files() {
    let Some(p) = provider() else { return };

    let batch = p.list_directory(ROOT_INODE, 0).unwrap();

    // Read all files in root
    for entry in &batch.entries {
        if entry.attributes.is_file() {
            let data = p
                .read_file(entry.attributes.inode, 0, entry.attributes.size)
                .unwrap();
            assert_eq!(
                data.len() as u64,
                entry.attributes.size,
                "file '{}' size mismatch: got {} bytes, expected {}",
                entry.name,
                data.len(),
                entry.attributes.size
            );
        }
    }
}

#[test]
fn integration_concurrent_reads() {
    let Some(p) = provider() else { return };

    // Verify thread safety by wrapping in Arc and reading from multiple threads
    let p = std::sync::Arc::new(p);
    let mut handles = Vec::new();

    for _ in 0..4 {
        let p = std::sync::Arc::clone(&p);
        handles.push(std::thread::spawn(move || {
            let batch = p.list_directory(ROOT_INODE, 0).unwrap();
            for entry in &batch.entries {
                let _ = p.get_attributes(entry.attributes.inode);
            }
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }
}
