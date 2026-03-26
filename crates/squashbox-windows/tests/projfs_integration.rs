//! Integration tests for the Windows ProjFS driver.
//!
//! These tests require:
//! 1. Windows with ProjFS feature enabled
//! 2. A test SquashFS image at `../squashbox-core/tests/fixtures/test.sqsh`
//!
//! To enable ProjFS:
//!   Enable-WindowsOptionalFeature -Online -FeatureName Client-ProjFS -NoRestart
//!
//! Most tests here are marked `#[ignore]` because they require ProjFS runtime.
//! Run them with: cargo test --test projfs_integration -- --ignored

use squashbox_core::SquashFsProvider;
use squashbox_windows::projfs_source::SquashboxProjFsSource;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn core_fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("squashbox-core")
        .join("tests")
        .join("fixtures")
}

fn test_image_path() -> PathBuf {
    core_fixtures_dir().join("test.sqsh")
}

fn has_fixture() -> bool {
    test_image_path().exists()
}

fn make_source() -> Option<SquashboxProjFsSource> {
    if !has_fixture() {
        eprintln!("SKIP: test fixture not found at {:?}", test_image_path());
        return None;
    }
    let provider = Arc::new(SquashFsProvider::open(&test_image_path()).unwrap());
    Some(SquashboxProjFsSource::new(provider))
}

// ═══════════════════════════════════════════════════════════
// Non-ProjFS tests (no ProjFS runtime needed — unit-level integration)
// ═══════════════════════════════════════════════════════════

#[test]
fn source_lists_root_from_real_image() {
    let Some(source) = make_source() else { return };
    use windows_projfs::ProjectedFileSystemSource;

    let entries = source.list_directory(Path::new(""));
    assert!(!entries.is_empty(), "root should have entries");
}

#[test]
fn source_gets_file_entry_from_real_image() {
    let Some(source) = make_source() else { return };
    use windows_projfs::ProjectedFileSystemSource;

    let entry = source.get_directory_entry(Path::new("hello.txt"));
    assert!(entry.is_some(), "hello.txt should exist");
}

#[test]
fn source_streams_file_from_real_image() {
    let Some(source) = make_source() else { return };
    use std::io::Read;
    use windows_projfs::ProjectedFileSystemSource;

    let mut reader = source
        .stream_file_content(Path::new("hello.txt"), 0, 1000)
        .unwrap();
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).unwrap();
    let content = String::from_utf8_lossy(&buf);
    assert!(
        content.contains("Hello"),
        "expected 'Hello' in content: {content}"
    );
}

// ═══════════════════════════════════════════════════════════
// ProjFS runtime tests (require ProjFS enabled + mount)
// ═══════════════════════════════════════════════════════════

#[test]
#[ignore] // Requires ProjFS enabled on Windows
fn projfs_mount_and_list() {
    let Some(source) = make_source() else { return };

    let mount_dir = tempfile::tempdir().unwrap();
    let mount_path = mount_dir.path();

    // This would actually start ProjFS virtualization.
    // Keeping as placeholder — real test would:
    // 1. pfs = ProjectedFileSystem::new(mount_path, source)
    // 2. pfs.start()
    // 3. std::fs::read_dir(mount_path) and verify entries
    // 4. Drop pfs to stop
    let _ = source;
    let _ = mount_path;
    eprintln!("TODO: Full ProjFS mount test requires ProjFS runtime");
}

#[test]
#[ignore] // Requires ProjFS enabled on Windows
fn projfs_mount_and_read_file() {
    let Some(source) = make_source() else { return };

    let mount_dir = tempfile::tempdir().unwrap();
    let mount_path = mount_dir.path();

    // Placeholder for real ProjFS test:
    // 1. Mount
    // 2. std::fs::read(mount_path.join("hello.txt"))
    // 3. Assert content matches
    // 4. Unmount
    let _ = source;
    let _ = mount_path;
    eprintln!("TODO: Full ProjFS read test requires ProjFS runtime");
}

#[test]
#[ignore] // Requires ProjFS enabled on Windows
fn projfs_write_denied() {
    let Some(source) = make_source() else { return };

    let mount_dir = tempfile::tempdir().unwrap();
    let mount_path = mount_dir.path();

    // Placeholder for real ProjFS test:
    // 1. Mount
    // 2. std::fs::write(mount_path.join("new.txt"), "data")
    // 3. Assert it fails with access denied
    // 4. Unmount
    let _ = source;
    let _ = mount_path;
    eprintln!("TODO: Full ProjFS write-deny test requires ProjFS runtime");
}
