//! Test fixture creation script.
//!
//! This test creates a SquashFS test image using backhand's writer API.
//! Run with: cargo test --test create_fixture -- --ignored
//!
//! The fixture contains:
//! - hello.txt (text file)
//! - data.bin (binary file)
//! - empty.txt (empty file)
//! - subdir/ (directory)
//!   - nested.txt (text file)
//!   - deep/ (directory)
//!     - level2.txt (text file)
//! - link.txt (symlink → hello.txt)

use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

#[test]
#[ignore] // Only run manually to create the fixture
fn create_test_fixture() {
    use backhand::{FilesystemWriter, NodeHeader};
    use std::io::Cursor;

    let fixtures = fixtures_dir();
    std::fs::create_dir_all(&fixtures).unwrap();

    let output_path = fixtures.join("test.sqsh");

    let mut writer = FilesystemWriter::default();

    let header = NodeHeader {
        permissions: 0o644,
        uid: 1000,
        gid: 1000,
        mtime: 1700000000,
    };

    let dir_header = NodeHeader {
        permissions: 0o755,
        uid: 1000,
        gid: 1000,
        mtime: 1700000000,
    };

    // Add files
    writer
        .push_file(
            Cursor::new(b"Hello, World!\n"),
            "hello.txt",
            header,
        )
        .unwrap();

    writer
        .push_file(
            Cursor::new(vec![0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x01, 0x02, 0x03]),
            "data.bin",
            header,
        )
        .unwrap();

    writer
        .push_file(Cursor::new(b""), "empty.txt", header)
        .unwrap();

    // Add directory with nested files
    writer.push_dir("subdir", dir_header).unwrap();

    writer
        .push_file(
            Cursor::new(b"Nested file content\n"),
            "subdir/nested.txt",
            header,
        )
        .unwrap();

    writer.push_dir("subdir/deep", dir_header).unwrap();

    writer
        .push_file(
            Cursor::new(b"Level 2 deep\n"),
            "subdir/deep/level2.txt",
            header,
        )
        .unwrap();

    // Add symlink
    writer
        .push_symlink("hello.txt", "link.txt", header)
        .unwrap();

    // Write the image
    let mut output = std::fs::File::create(&output_path).unwrap();
    writer.write(&mut output).unwrap();

    println!("Created test fixture at {}", output_path.display());
    assert!(output_path.exists());
}
