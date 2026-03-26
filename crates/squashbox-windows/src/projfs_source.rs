//! ProjFS source adapter: bridges `VirtualFsProvider` to `ProjectedFileSystemSource`.

use squashbox_core::provider::VirtualFsProvider;
use squashbox_core::types::*;
use std::io::{Cursor, Read};
use std::ops::ControlFlow;
use std::path::Path;
use std::sync::Arc;
use windows_projfs::{
    DirectoryEntry, DirectoryInfo, FileInfo, Notification,
    ProjectedFileSystemSource,
};

/// Adapts a `VirtualFsProvider` into a `ProjectedFileSystemSource`.
///
/// This struct holds an `Arc` reference to the core provider and translates
/// between the path-based ProjFS callbacks and our inode-based core.
///
/// # Thread Safety
///
/// `ProjectedFileSystemSource` requires `&self` (ProjFS dispatches callbacks
/// on multiple threads). Our provider is `Send + Sync` via `Arc`, so
/// concurrent access is safe.
pub struct SquashboxProjFsSource {
    provider: Arc<dyn VirtualFsProvider>,
}

impl SquashboxProjFsSource {
    /// Create a new ProjFS source backed by the given provider.
    pub fn new(provider: Arc<dyn VirtualFsProvider>) -> Self {
        Self { provider }
    }

    /// Get a reference to the underlying provider (for testing).
    pub fn provider(&self) -> &dyn VirtualFsProvider {
        self.provider.as_ref()
    }
}

impl ProjectedFileSystemSource for SquashboxProjFsSource {
    /// Called by ProjFS when Explorer or an application enumerates a directory.
    ///
    /// ProjFS delivers a relative path from the virtualization root.
    /// We resolve it to an inode, list directory contents, and map each
    /// entry to the ProjFS `DirectoryEntry` enum.
    fn list_directory(&self, path: &Path) -> Vec<DirectoryEntry> {
        // 1. Resolve the relative path to an inode
        let inode = match self.provider.resolve_path(path) {
            Ok(Some(id)) => id,
            Ok(None) => {
                log::debug!("list_directory: path not found: {}", path.display());
                return vec![];
            }
            Err(e) => {
                log::error!("list_directory: resolve_path failed: {e}");
                return vec![];
            }
        };

        // 2. Accumulate all pages of directory entries
        let mut all_entries = Vec::new();
        let mut cookie = 0u64;
        loop {
            match self.provider.list_directory(inode, cookie) {
                Ok(batch) => {
                    all_entries.extend(batch.entries);
                    if batch.next_cookie == 0 {
                        break;
                    }
                    cookie = batch.next_cookie;
                }
                Err(e) => {
                    log::error!("list_directory: failed for inode {inode}: {e}");
                    return vec![];
                }
            }
        }

        // 3. Map core entries to ProjFS entries
        all_entries
            .into_iter()
            .filter_map(|e| Self::map_entry(e))
            .collect()
    }

    /// Called by ProjFS when a file needs to be hydrated (first read).
    ///
    /// ProjFS provides the path, byte offset, and desired length.
    /// We read from the core provider and return a `Box<dyn Read>`.
    fn stream_file_content(
        &self,
        path: &Path,
        byte_offset: usize,
        length: usize,
    ) -> std::io::Result<Box<dyn Read>> {
        // 1. Resolve path to inode
        let inode = self
            .provider
            .resolve_path(path)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("file not found: {}", path.display()),
                )
            })?;

        // 2. Read data from core
        let data = self
            .provider
            .read_file(inode, byte_offset as u64, length as u64)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        // 3. Wrap in Cursor to produce Box<dyn Read>
        Ok(Box::new(Cursor::new(data)))
    }

    /// Called by ProjFS for stat-like operations (get placeholder info).
    ///
    /// Returns metadata for a single entry, or `None` if it doesn't exist.
    fn get_directory_entry(&self, path: &Path) -> Option<DirectoryEntry> {
        let inode = self.provider.resolve_path(path).ok()??;
        let attrs = self.provider.get_attributes(inode).ok()?;

        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        Some(Self::map_attributes_to_entry(&name, &attrs))
    }

    /// Called for pre-operation notifications (file create, delete, rename, etc.)
    ///
    /// We deny all write operations — SquashFS is read-only.
    fn handle_notification(&self, notification: &Notification) -> ControlFlow<()> {
        log::debug!("ProjFS notification denied: {:?}", notification);
        ControlFlow::Break(())
    }
}

impl SquashboxProjFsSource {
    /// Map a core `DirEntry` to a ProjFS `DirectoryEntry`.
    fn map_entry(entry: DirEntry) -> Option<DirectoryEntry> {
        Some(Self::map_attributes_to_entry(&entry.name, &entry.attributes))
    }

    /// Map core attributes to a ProjFS entry.
    fn map_attributes_to_entry(name: &str, attrs: &EntryAttributes) -> DirectoryEntry {
        match attrs.entry_type {
            EntryType::Directory => DirectoryEntry::Directory(DirectoryInfo {
                directory_name: name.into(),
                ..Default::default()
            }),
            // Files and symlinks are projected as files.
            // Symlinks are resolved transparently by the core.
            _ => DirectoryEntry::File(FileInfo {
                file_name: name.into(),
                file_size: attrs.size,
                ..Default::default()
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use squashbox_core::provider::VirtualFsProvider;

    // ── Mock provider for unit tests ──

    struct MockProvider;

    impl VirtualFsProvider for MockProvider {
        fn resolve_path(&self, path: &Path) -> CoreResult<Option<InodeId>> {
            match path.to_str() {
                Some("") | Some(".") => Ok(Some(ROOT_INODE)),
                Some("readme.md") => Ok(Some(10)),
                Some("src") => Ok(Some(20)),
                Some("src/main.rs") => Ok(Some(30)),
                Some("link") => Ok(Some(40)),
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
                    size: 42,
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
                30 => Ok(EntryAttributes {
                    inode: 30,
                    entry_type: EntryType::File,
                    size: 256,
                    mode: 0o644,
                    uid: 1000,
                    gid: 1000,
                    mtime_secs: 1700000000,
                    nlink: 1,
                }),
                40 => Ok(EntryAttributes {
                    inode: 40,
                    entry_type: EntryType::Symlink,
                    size: 9,
                    mode: 0o777,
                    uid: 1000,
                    gid: 1000,
                    mtime_secs: 1700000000,
                    nlink: 1,
                }),
                _ => Err(CoreError::NotFound(format!("inode {inode}"))),
            }
        }

        fn list_directory(&self, inode: InodeId, _cookie: u64) -> CoreResult<DirEntryBatch> {
            match inode {
                ROOT_INODE => Ok(DirEntryBatch {
                    entries: vec![
                        DirEntry {
                            name: "readme.md".into(),
                            attributes: self.get_attributes(10)?,
                        },
                        DirEntry {
                            name: "src".into(),
                            attributes: self.get_attributes(20)?,
                        },
                        DirEntry {
                            name: "link".into(),
                            attributes: self.get_attributes(40)?,
                        },
                    ],
                    next_cookie: 0,
                }),
                20 => Ok(DirEntryBatch {
                    entries: vec![DirEntry {
                        name: "main.rs".into(),
                        attributes: self.get_attributes(30)?,
                    }],
                    next_cookie: 0,
                }),
                _ => Err(CoreError::NotADirectory(inode)),
            }
        }

        fn lookup(&self, parent: InodeId, name: &str) -> CoreResult<Option<DirEntry>> {
            let batch = self.list_directory(parent, 0)?;
            Ok(batch.entries.into_iter().find(|e| e.name == name))
        }

        fn read_file(&self, inode: InodeId, offset: u64, length: u64) -> CoreResult<Vec<u8>> {
            match inode {
                10 => {
                    let data = b"# README\n\nThis is a test project.\n";
                    let s = offset as usize;
                    let e = (s + length as usize).min(data.len());
                    if s >= data.len() {
                        Ok(vec![])
                    } else {
                        Ok(data[s..e].to_vec())
                    }
                }
                30 => {
                    let data = b"fn main() {\n    println!(\"Hello\");\n}\n";
                    let s = offset as usize;
                    let e = (s + length as usize).min(data.len());
                    if s >= data.len() {
                        Ok(vec![])
                    } else {
                        Ok(data[s..e].to_vec())
                    }
                }
                _ => Err(CoreError::NotAFile(inode)),
            }
        }

        fn read_symlink(&self, inode: InodeId) -> CoreResult<String> {
            if inode == 40 {
                Ok("readme.md".into())
            } else {
                Err(CoreError::NotASymlink(inode))
            }
        }

        fn list_xattrs(&self, _inode: InodeId) -> CoreResult<Vec<String>> {
            Ok(vec![])
        }

        fn get_xattr(&self, _inode: InodeId, name: &str) -> CoreResult<Vec<u8>> {
            Err(CoreError::NotFound(name.into()))
        }

        fn check_access(&self, _inode: InodeId, mask: u32) -> CoreResult<bool> {
            Ok(mask & 2 == 0) // Deny W_OK
        }

        fn volume_stats(&self) -> CoreResult<VolumeStats> {
            Ok(VolumeStats {
                total_bytes: 10000,
                used_bytes: 5000,
                total_inodes: 10,
                used_inodes: 5,
                block_size: 4096,
            })
        }
    }

    fn make_source() -> SquashboxProjFsSource {
        SquashboxProjFsSource::new(Arc::new(MockProvider))
    }

    // ── list_directory tests ──

    #[test]
    fn list_directory_root_returns_entries() {
        let source = make_source();
        use windows_projfs::ProjectedFileSystemSource;
        let entries = source.list_directory(Path::new(""));
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn list_directory_maps_directories_correctly() {
        let source = make_source();
        use windows_projfs::ProjectedFileSystemSource;
        let entries = source.list_directory(Path::new(""));
        let dirs: Vec<_> = entries
            .iter()
            .filter(|e| matches!(e, DirectoryEntry::Directory(_)))
            .collect();
        assert_eq!(dirs.len(), 1); // "src" is the only directory
        match &dirs[0] {
            DirectoryEntry::Directory(info) => assert_eq!(info.directory_name, "src"),
            _ => panic!("expected directory"),
        }
    }

    #[test]
    fn list_directory_maps_files_correctly() {
        let source = make_source();
        use windows_projfs::ProjectedFileSystemSource;
        let entries = source.list_directory(Path::new(""));
        let files: Vec<_> = entries
            .iter()
            .filter(|e| matches!(e, DirectoryEntry::File(_)))
            .collect();
        // "readme.md" (file) + "link" (symlink projected as file)
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn list_directory_symlink_projected_as_file() {
        let source = make_source();
        use windows_projfs::ProjectedFileSystemSource;
        let entries = source.list_directory(Path::new(""));
        let link_entry = entries
            .iter()
            .find(|e| match e {
                DirectoryEntry::File(f) => f.file_name == "link",
                _ => false,
            });
        assert!(link_entry.is_some(), "symlink should be projected as file");
    }

    #[test]
    fn list_directory_subdir() {
        let source = make_source();
        use windows_projfs::ProjectedFileSystemSource;
        let entries = source.list_directory(Path::new("src"));
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            DirectoryEntry::File(f) => {
                assert_eq!(f.file_name, "main.rs");
                assert_eq!(f.file_size, 256);
            }
            _ => panic!("expected file entry"),
        }
    }

    #[test]
    fn list_directory_nonexistent_returns_empty() {
        let source = make_source();
        use windows_projfs::ProjectedFileSystemSource;
        let entries = source.list_directory(Path::new("nonexistent"));
        assert!(entries.is_empty());
    }

    // ── stream_file_content tests ──

    #[test]
    fn stream_file_content_reads_data() {
        use windows_projfs::ProjectedFileSystemSource;
        let source = make_source();
        let mut reader = source
            .stream_file_content(Path::new("readme.md"), 0, 1000)
            .unwrap();
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).unwrap();
        let content = String::from_utf8_lossy(&buf);
        assert!(content.starts_with("# README"));
    }

    #[test]
    fn stream_file_content_with_offset() {
        use windows_projfs::ProjectedFileSystemSource;
        let source = make_source();
        let mut reader = source
            .stream_file_content(Path::new("readme.md"), 2, 6)
            .unwrap();
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).unwrap();
        assert_eq!(String::from_utf8_lossy(&buf), "README");
    }

    #[test]
    fn stream_file_content_nonexistent_returns_error() {
        use windows_projfs::ProjectedFileSystemSource;
        let source = make_source();
        let result = source.stream_file_content(Path::new("nope"), 0, 100);
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn stream_file_content_on_directory_returns_error() {
        use windows_projfs::ProjectedFileSystemSource;
        let source = make_source();
        let result = source.stream_file_content(Path::new("src"), 0, 100);
        // Should error because "src" is a directory
        assert!(result.is_err());
    }

    // ── get_directory_entry tests ──

    #[test]
    fn get_directory_entry_existing_file() {
        use windows_projfs::ProjectedFileSystemSource;
        let source = make_source();
        let entry = source.get_directory_entry(Path::new("readme.md"));
        assert!(entry.is_some());
        match entry.unwrap() {
            DirectoryEntry::File(f) => {
                assert_eq!(f.file_name, "readme.md");
                assert_eq!(f.file_size, 42);
            }
            _ => panic!("expected file"),
        }
    }

    #[test]
    fn get_directory_entry_existing_dir() {
        use windows_projfs::ProjectedFileSystemSource;
        let source = make_source();
        let entry = source.get_directory_entry(Path::new("src"));
        assert!(entry.is_some());
        match entry.unwrap() {
            DirectoryEntry::Directory(d) => assert_eq!(d.directory_name, "src"),
            _ => panic!("expected directory"),
        }
    }

    #[test]
    fn get_directory_entry_nonexistent() {
        use windows_projfs::ProjectedFileSystemSource;
        let source = make_source();
        let entry = source.get_directory_entry(Path::new("nope.txt"));
        assert!(entry.is_none());
    }

    #[test]
    fn get_directory_entry_nested() {
        use windows_projfs::ProjectedFileSystemSource;
        let source = make_source();
        let entry = source.get_directory_entry(Path::new("src/main.rs"));
        assert!(entry.is_some());
        match entry.unwrap() {
            DirectoryEntry::File(f) => {
                assert_eq!(f.file_name, "main.rs");
                assert_eq!(f.file_size, 256);
            }
            _ => panic!("expected file"),
        }
    }

    // ── handle_notification tests ──

    #[test]
    fn handle_notification_always_denies() {
        use windows_projfs::{FileRenameInfo, Notification, ProjectedFile, ProjectedFileSystemSource};
        let source = make_source();

        // FileCreated
        let result = source.handle_notification(&Notification::FileCreated(ProjectedFile {
            path: Path::new("test.txt").into(),
            ..Default::default()
        }));
        assert!(matches!(result, ControlFlow::Break(())));

        // PreFileDelete
        let result = source.handle_notification(&Notification::PreFileDelete(ProjectedFile {
            path: Path::new("test.txt").into(),
            ..Default::default()
        }));
        assert!(matches!(result, ControlFlow::Break(())));

        // FileRenamed
        let result = source.handle_notification(&Notification::FileRenamed(FileRenameInfo {
            source: Some(Path::new("test.txt").into()),
            destination: Some(Path::new("test2.txt").into()),
        }));
        assert!(matches!(result, ControlFlow::Break(())));
    }

    // ── Construction / Arc tests ──

    #[test]
    fn source_construction() {
        let provider: Arc<dyn VirtualFsProvider> = Arc::new(MockProvider);
        let source = SquashboxProjFsSource::new(provider);
        // Should be able to access provider
        let stats = source.provider().volume_stats().unwrap();
        assert_eq!(stats.total_bytes, 10000);
    }

    #[test]
    fn source_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SquashboxProjFsSource>();
    }
}
