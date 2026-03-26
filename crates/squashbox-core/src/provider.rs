//! The `VirtualFsProvider` trait — the central abstraction of Squashbox.
//!
//! Both the Windows (ProjFS) and macOS (FSKit/UniFFI) drivers call into
//! implementations of this trait. It is intentionally synchronous — each
//! driver layer handles async/threading concerns itself.

use crate::types::*;
use std::path::Path;

/// Platform-agnostic virtual filesystem provider.
///
/// Implementors provide read-only access to a filesystem image (e.g., SquashFS).
///
/// # Thread Safety
///
/// Implementations must be `Send + Sync` because:
/// - ProjFS delivers callbacks on multiple OS threads concurrently
/// - FSKit may dispatch operations from multiple dispatch queues
///
/// # Path vs. Inode
///
/// This trait exposes both path-based and inode-based APIs:
/// - **ProjFS** is path-based — use `resolve_path()` then inode methods
/// - **FSKit** is inode-based — call inode methods directly
///
/// `resolve_path()` internally walks the tree using `lookup()`, so there's
/// no duplication of logic.
pub trait VirtualFsProvider: Send + Sync {
    // ── Path resolution ──

    /// Resolve a relative path (e.g., `"dir/subdir/file.txt"`) to an inode ID.
    ///
    /// Returns `Ok(None)` if the path does not exist.
    /// Returns `Err` on I/O or internal errors.
    fn resolve_path(&self, path: &Path) -> CoreResult<Option<InodeId>>;

    // ── Inode-based operations ──

    /// Get attributes for a given inode.
    ///
    /// Returns `Err(CoreError::NotFound)` if the inode doesn't exist.
    fn get_attributes(&self, inode: InodeId) -> CoreResult<EntryAttributes>;

    /// List directory entries with pagination.
    ///
    /// `cookie` is 0 for the first batch. Use `DirEntryBatch::next_cookie`
    /// from the previous response for subsequent batches. A `next_cookie`
    /// of 0 in the response means no more entries.
    ///
    /// Returns `Err(CoreError::NotADirectory)` if the inode is not a directory.
    fn list_directory(&self, inode: InodeId, cookie: u64) -> CoreResult<DirEntryBatch>;

    /// Look up a single entry by name within a directory.
    ///
    /// Returns `Ok(None)` if no entry with that name exists in the directory.
    /// Returns `Err(CoreError::NotADirectory)` if `parent_inode` is not a directory.
    fn lookup(&self, parent_inode: InodeId, name: &str) -> CoreResult<Option<DirEntry>>;

    /// Read file content at a byte range.
    ///
    /// Returns the decompressed data as a byte vector.
    /// Returns `Err(CoreError::NotAFile)` if the inode is not a regular file.
    fn read_file(
        &self,
        inode: InodeId,
        offset: u64,
        length: u64,
    ) -> CoreResult<Vec<u8>>;

    /// Read the target of a symbolic link.
    ///
    /// Returns the symlink target as a string.
    /// Returns `Err(CoreError::NotASymlink)` if the inode is not a symlink.
    fn read_symlink(&self, inode: InodeId) -> CoreResult<String>;

    // ── Extended attributes ──

    /// List xattr names for a given inode.
    fn list_xattrs(&self, inode: InodeId) -> CoreResult<Vec<String>>;

    /// Get the value of a specific xattr.
    ///
    /// Returns `Err(CoreError::NotFound)` if the xattr doesn't exist.
    fn get_xattr(&self, inode: InodeId, name: &str) -> CoreResult<Vec<u8>>;

    // ── Access control ──

    /// Check if access is allowed for the given POSIX access mask.
    ///
    /// `mask` uses POSIX constants: `R_OK=4`, `W_OK=2`, `X_OK=1`.
    /// Since SquashFS is read-only, `W_OK` should always return `false`.
    fn check_access(&self, inode: InodeId, mask: u32) -> CoreResult<bool>;

    // ── Volume info ──

    /// Get filesystem-level statistics.
    fn volume_stats(&self) -> CoreResult<VolumeStats>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// A mock implementation for testing trait ergonomics and object safety.
    struct MockProvider;

    impl VirtualFsProvider for MockProvider {
        fn resolve_path(&self, path: &Path) -> CoreResult<Option<InodeId>> {
            match path.to_str() {
                Some("") => Ok(Some(ROOT_INODE)),
                Some("existing.txt") => Ok(Some(10)),
                Some("subdir") => Ok(Some(20)),
                Some("subdir/nested.txt") => Ok(Some(30)),
                _ => Ok(None),
            }
        }

        fn get_attributes(&self, inode: InodeId) -> CoreResult<EntryAttributes> {
            match inode {
                ROOT_INODE => Ok(EntryAttributes {
                    inode: ROOT_INODE,
                    entry_type: EntryType::Directory,
                    size: 0,
                    mode: 0o755,
                    uid: 0,
                    gid: 0,
                    mtime_secs: 1700000000,
                    nlink: 3,
                }),
                10 => Ok(EntryAttributes {
                    inode: 10,
                    entry_type: EntryType::File,
                    size: 100,
                    mode: 0o644,
                    uid: 1000,
                    gid: 1000,
                    mtime_secs: 1700000000,
                    nlink: 1,
                }),
                20 => Ok(EntryAttributes {
                    inode: 20,
                    entry_type: EntryType::Directory,
                    size: 0,
                    mode: 0o755,
                    uid: 1000,
                    gid: 1000,
                    mtime_secs: 1700000000,
                    nlink: 2,
                }),
                _ => Err(CoreError::NotFound(format!("inode {inode}"))),
            }
        }

        fn list_directory(&self, inode: InodeId, _cookie: u64) -> CoreResult<DirEntryBatch> {
            match inode {
                ROOT_INODE => Ok(DirEntryBatch {
                    entries: vec![
                        DirEntry {
                            name: "existing.txt".to_string(),
                            attributes: self.get_attributes(10)?,
                        },
                        DirEntry {
                            name: "subdir".to_string(),
                            attributes: self.get_attributes(20)?,
                        },
                    ],
                    next_cookie: 0,
                }),
                20 => Ok(DirEntryBatch {
                    entries: vec![DirEntry {
                        name: "nested.txt".to_string(),
                        attributes: EntryAttributes {
                            inode: 30,
                            entry_type: EntryType::File,
                            size: 50,
                            mode: 0o644,
                            uid: 1000,
                            gid: 1000,
                            mtime_secs: 1700000000,
                            nlink: 1,
                        },
                    }],
                    next_cookie: 0,
                }),
                _ => Err(CoreError::NotADirectory(inode)),
            }
        }

        fn lookup(&self, parent_inode: InodeId, name: &str) -> CoreResult<Option<DirEntry>> {
            let batch = self.list_directory(parent_inode, 0)?;
            Ok(batch.entries.into_iter().find(|e| e.name == name))
        }

        fn read_file(&self, inode: InodeId, offset: u64, length: u64) -> CoreResult<Vec<u8>> {
            if inode != 10 && inode != 30 {
                return Err(CoreError::NotAFile(inode));
            }
            let data = b"Hello, World! This is test content for the mock provider.";
            let start = offset as usize;
            let end = (start + length as usize).min(data.len());
            if start >= data.len() {
                return Ok(vec![]);
            }
            Ok(data[start..end].to_vec())
        }

        fn read_symlink(&self, _inode: InodeId) -> CoreResult<String> {
            Err(CoreError::NotASymlink(0))
        }

        fn list_xattrs(&self, _inode: InodeId) -> CoreResult<Vec<String>> {
            Ok(vec![])
        }

        fn get_xattr(&self, _inode: InodeId, name: &str) -> CoreResult<Vec<u8>> {
            Err(CoreError::NotFound(format!("xattr {name}")))
        }

        fn check_access(&self, _inode: InodeId, mask: u32) -> CoreResult<bool> {
            // Read-only: deny W_OK (2)
            Ok(mask & 2 == 0)
        }

        fn volume_stats(&self) -> CoreResult<VolumeStats> {
            Ok(VolumeStats {
                total_bytes: 1_000_000,
                used_bytes: 500_000,
                total_inodes: 100,
                used_inodes: 4,
                block_size: 131072,
            })
        }
    }

    // ── Trait object safety ──

    #[test]
    fn provider_is_object_safe() {
        // Must compile: VirtualFsProvider can be used as a trait object.
        let _: Box<dyn VirtualFsProvider> = Box::new(MockProvider);
    }

    #[test]
    fn provider_is_arc_compatible() {
        // Must compile: can wrap in Arc (needed for ProjFS multi-thread).
        let provider: Arc<dyn VirtualFsProvider> = Arc::new(MockProvider);
        let _clone = Arc::clone(&provider);
    }

    // ── resolve_path ──

    #[test]
    fn resolve_path_root() {
        let p = MockProvider;
        assert_eq!(p.resolve_path(Path::new("")).unwrap(), Some(ROOT_INODE));
    }

    #[test]
    fn resolve_path_existing_file() {
        let p = MockProvider;
        assert_eq!(p.resolve_path(Path::new("existing.txt")).unwrap(), Some(10));
    }

    #[test]
    fn resolve_path_existing_dir() {
        let p = MockProvider;
        assert_eq!(p.resolve_path(Path::new("subdir")).unwrap(), Some(20));
    }

    #[test]
    fn resolve_path_nested() {
        let p = MockProvider;
        assert_eq!(
            p.resolve_path(Path::new("subdir/nested.txt")).unwrap(),
            Some(30)
        );
    }

    #[test]
    fn resolve_path_nonexistent() {
        let p = MockProvider;
        assert_eq!(p.resolve_path(Path::new("nope.txt")).unwrap(), None);
    }

    // ── get_attributes ──

    #[test]
    fn get_attributes_root() {
        let p = MockProvider;
        let attrs = p.get_attributes(ROOT_INODE).unwrap();
        assert!(attrs.is_dir());
        assert_eq!(attrs.mode, 0o755);
    }

    #[test]
    fn get_attributes_file() {
        let p = MockProvider;
        let attrs = p.get_attributes(10).unwrap();
        assert!(attrs.is_file());
        assert_eq!(attrs.size, 100);
    }

    #[test]
    fn get_attributes_nonexistent() {
        let p = MockProvider;
        let err = p.get_attributes(9999).unwrap_err();
        assert!(matches!(err, CoreError::NotFound(_)));
    }

    // ── list_directory ──

    #[test]
    fn list_directory_root() {
        let p = MockProvider;
        let batch = p.list_directory(ROOT_INODE, 0).unwrap();
        assert_eq!(batch.entries.len(), 2);
        assert_eq!(batch.next_cookie, 0); // No more entries
        let names: Vec<&str> = batch.entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"existing.txt"));
        assert!(names.contains(&"subdir"));
    }

    #[test]
    fn list_directory_subdir() {
        let p = MockProvider;
        let batch = p.list_directory(20, 0).unwrap();
        assert_eq!(batch.entries.len(), 1);
        assert_eq!(batch.entries[0].name, "nested.txt");
    }

    #[test]
    fn list_directory_not_a_directory() {
        let p = MockProvider;
        let err = p.list_directory(10, 0).unwrap_err(); // inode 10 is a file
        assert!(matches!(err, CoreError::NotADirectory(_)));
    }

    // ── lookup ──

    #[test]
    fn lookup_existing() {
        let p = MockProvider;
        let entry = p.lookup(ROOT_INODE, "existing.txt").unwrap();
        assert!(entry.is_some());
        let entry = entry.unwrap();
        assert_eq!(entry.name, "existing.txt");
        assert!(entry.attributes.is_file());
    }

    #[test]
    fn lookup_nonexistent() {
        let p = MockProvider;
        let entry = p.lookup(ROOT_INODE, "nope").unwrap();
        assert!(entry.is_none());
    }

    #[test]
    fn lookup_in_non_directory() {
        let p = MockProvider;
        let err = p.lookup(10, "anything").unwrap_err();
        assert!(matches!(err, CoreError::NotADirectory(_)));
    }

    // ── read_file ──

    #[test]
    fn read_file_full() {
        let p = MockProvider;
        let data = p.read_file(10, 0, 1000).unwrap();
        assert!(!data.is_empty());
        assert!(String::from_utf8_lossy(&data).starts_with("Hello"));
    }

    #[test]
    fn read_file_with_offset() {
        let p = MockProvider;
        let data = p.read_file(10, 7, 5).unwrap();
        assert_eq!(String::from_utf8_lossy(&data), "World");
    }

    #[test]
    fn read_file_past_end() {
        let p = MockProvider;
        let data = p.read_file(10, 99999, 10).unwrap();
        assert!(data.is_empty());
    }

    #[test]
    fn read_file_on_directory_fails() {
        let p = MockProvider;
        let err = p.read_file(ROOT_INODE, 0, 10).unwrap_err();
        assert!(matches!(err, CoreError::NotAFile(_)));
    }

    // ── check_access ──

    #[test]
    fn check_access_read_allowed() {
        let p = MockProvider;
        assert!(p.check_access(10, 4).unwrap()); // R_OK
    }

    #[test]
    fn check_access_write_denied() {
        let p = MockProvider;
        assert!(!p.check_access(10, 2).unwrap()); // W_OK
    }

    #[test]
    fn check_access_execute_allowed() {
        let p = MockProvider;
        assert!(p.check_access(10, 1).unwrap()); // X_OK
    }

    #[test]
    fn check_access_read_write_denied() {
        let p = MockProvider;
        assert!(!p.check_access(10, 6).unwrap()); // R_OK | W_OK
    }

    // ── volume_stats ──

    #[test]
    fn volume_stats_returns_valid_data() {
        let p = MockProvider;
        let stats = p.volume_stats().unwrap();
        assert_eq!(stats.total_bytes, 1_000_000);
        assert_eq!(stats.used_bytes, 500_000);
        assert!(stats.used_inodes <= stats.total_inodes);
        assert!(stats.block_size > 0);
    }

    // ── list_xattrs / get_xattr ──

    #[test]
    fn list_xattrs_empty() {
        let p = MockProvider;
        let names = p.list_xattrs(10).unwrap();
        assert!(names.is_empty());
    }

    #[test]
    fn get_xattr_nonexistent() {
        let p = MockProvider;
        let err = p.get_xattr(10, "user.test").unwrap_err();
        assert!(matches!(err, CoreError::NotFound(_)));
    }
}
