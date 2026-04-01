//! ZIP-backed implementation of `VirtualFsProvider`.
//!
//! Uses the `zip` crate to read ZIP archives. Builds an in-memory inode index
//! at open time for O(1) lookups, synthesizing directory inodes from file paths
//! since ZIP archives don't always have explicit directory entries.

use crate::provider::VirtualFsProvider;
use crate::types::*;
use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

/// Default page size for directory listing pagination.
const DIR_PAGE_SIZE: usize = 64;

/// An entry in the ZIP inode index.
#[derive(Debug, Clone)]
struct IndexEntry {
    /// Parent inode (ROOT_INODE for the root's own entry).
    parent: InodeId,
    /// Name of this entry (basename, empty string for root).
    name: String,
    /// Cached attributes.
    attributes: EntryAttributes,
    /// For directories: ordered list of child inode IDs (sorted by name).
    children: Vec<InodeId>,
    /// For directories: O(1) child lookup by name.
    children_by_name: HashMap<String, InodeId>,
    /// Index into the zip archive (None for synthesized directories).
    zip_index: Option<usize>,
}

/// The in-memory inode index built from the ZIP directory tree.
#[derive(Debug)]
struct InodeIndex {
    entries: HashMap<InodeId, IndexEntry>,
}

impl InodeIndex {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    fn get(&self, inode: InodeId) -> CoreResult<&IndexEntry> {
        self.entries
            .get(&inode)
            .ok_or_else(|| CoreError::NotFound(format!("inode {inode}")))
    }

    fn insert(&mut self, inode: InodeId, entry: IndexEntry) {
        self.entries.insert(inode, entry);
    }

    fn len(&self) -> usize {
        self.entries.len()
    }
}

/// A ZIP-backed implementation of `VirtualFsProvider`.
///
/// Thread-safe: the `zip::ZipArchive` is behind a `Mutex` since it requires
/// `&mut self` for reading entries. The `InodeIndex` is immutable after
/// construction.
pub struct ZipFsProvider {
    /// The raw archive bytes (must outlive `archive`).
    _archive_data: Vec<u8>,
    /// The zip archive reader, behind a Mutex for thread safety.
    archive: Mutex<zip::ZipArchive<Cursor<&'static [u8]>>>,
    /// Precomputed inode index (immutable after construction).
    index: InodeIndex,
    /// Total uncompressed size of all files.
    total_size: u64,
}

// SAFETY: ZipFsProvider is conceptually immutable after construction.
// The only mutable state is inside the Mutex-protected ZipArchive.
unsafe impl Send for ZipFsProvider {}
unsafe impl Sync for ZipFsProvider {}

impl std::fmt::Debug for ZipFsProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZipFsProvider")
            .field("inodes", &self.index.len())
            .finish()
    }
}

impl ZipFsProvider {
    /// Open a ZIP archive file and build the inode index.
    pub fn open(archive_path: &Path) -> CoreResult<Self> {
        let archive_data = std::fs::read(archive_path).map_err(|e| {
            CoreError::Io(format!("failed to read {}: {e}", archive_path.display()))
        })?;

        Self::from_bytes(archive_data)
    }

    /// Create a provider from raw archive bytes.
    pub fn from_bytes(archive_data: Vec<u8>) -> CoreResult<Self> {
        // SAFETY: Same self-referencing pattern as SquashFsProvider.
        // We create a slice that borrows from `archive_data` and transmute
        // the lifetime to 'static. This is safe because:
        // - `_archive_data` is stored in the same struct and never moved
        // - The archive is dropped before (or with) the data
        let static_slice: &'static [u8] = unsafe {
            std::slice::from_raw_parts(archive_data.as_ptr(), archive_data.len())
        };
        let cursor = Cursor::new(static_slice);

        let mut archive = zip::ZipArchive::new(cursor).map_err(|e| {
            CoreError::Io(format!("failed to parse ZIP archive: {e}"))
        })?;

        let (index, total_size) = Self::build_index(&mut archive)?;

        Ok(Self {
            _archive_data: archive_data,
            archive: Mutex::new(archive),
            index,
            total_size,
        })
    }

    /// Build the inode index by walking all entries in the ZIP central directory.
    ///
    /// ZIP archives don't always have explicit directory entries, so we
    /// synthesize directory inodes by walking file paths and creating
    /// intermediate directories as needed.
    fn build_index(
        archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    ) -> CoreResult<(InodeIndex, u64)> {
        let mut index = InodeIndex::new();
        let mut path_to_inode: HashMap<PathBuf, InodeId> = HashMap::new();
        let mut inode_counter: InodeId = ROOT_INODE;
        let mut total_size: u64 = 0;

        // Create root entry
        let root_inode = inode_counter;
        inode_counter += 1;
        index.insert(
            root_inode,
            IndexEntry {
                parent: ROOT_INODE,
                name: String::new(),
                attributes: EntryAttributes {
                    inode: root_inode,
                    entry_type: EntryType::Directory,
                    size: 0,
                    mode: 0o755,
                    uid: 0,
                    gid: 0,
                    mtime_secs: 0,
                    nlink: 2,
                },
                children: Vec::new(),
                children_by_name: HashMap::new(),
                zip_index: None,
            },
        );
        path_to_inode.insert(PathBuf::from(""), ROOT_INODE);

        for i in 0..archive.len() {
            let file = archive.by_index_raw(i).map_err(|e| {
                CoreError::Io(format!("failed to read ZIP entry {i}: {e}"))
            })?;

            let raw_name = file.name().to_string();

            // Skip macOS resource fork entries
            if raw_name.starts_with("__MACOSX/") || raw_name.contains("/.DS_Store") {
                continue;
            }

            let is_dir = raw_name.ends_with('/');
            let clean_name = raw_name.trim_end_matches('/');
            if clean_name.is_empty() {
                continue; // Skip root-level "/" entries
            }

            let entry_path = PathBuf::from(clean_name);

            // Ensure all ancestor directories exist
            let mut ancestors: Vec<PathBuf> = Vec::new();
            let mut current = entry_path.parent();
            while let Some(p) = current {
                if p.as_os_str().is_empty() {
                    break;
                }
                if !path_to_inode.contains_key(p) {
                    ancestors.push(p.to_path_buf());
                }
                current = p.parent();
            }
            // Process ancestors from shallowest to deepest
            ancestors.reverse();
            for ancestor in &ancestors {
                let dir_name = ancestor
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let parent_path = ancestor.parent().unwrap_or(Path::new(""));
                let parent_inode = path_to_inode
                    .get(parent_path)
                    .copied()
                    .unwrap_or(ROOT_INODE);

                let dir_inode = inode_counter;
                inode_counter += 1;

                index.insert(
                    dir_inode,
                    IndexEntry {
                        parent: parent_inode,
                        name: dir_name.clone(),
                        attributes: EntryAttributes {
                            inode: dir_inode,
                            entry_type: EntryType::Directory,
                            size: 0,
                            mode: 0o755,
                            uid: 0,
                            gid: 0,
                            mtime_secs: 0,
                            nlink: 2,
                        },
                        children: Vec::new(),
                        children_by_name: HashMap::new(),
                        zip_index: None,
                    },
                );

                // Register as child of parent
                if let Some(parent) = index.entries.get_mut(&parent_inode) {
                    parent.children.push(dir_inode);
                    parent.children_by_name.insert(dir_name, dir_inode);
                }

                path_to_inode.insert(ancestor.clone(), dir_inode);
            }

            // Skip if this is a directory entry we've already synthesized
            if is_dir && path_to_inode.contains_key(&entry_path) {
                // Update the mtime of the existing dir entry if the ZIP has explicit timestamps
                if let Some(&existing_inode) = path_to_inode.get(&entry_path) {
                    if let Some(entry) = index.entries.get_mut(&existing_inode) {
                        if let Some(dt) = file.last_modified() {
                            entry.attributes.mtime_secs =
                                datetime_to_unix(dt);
                        }
                    }
                }
                continue;
            }

            if is_dir {
                // Explicit directory entry not yet created
                let dir_name = entry_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let parent_path = entry_path.parent().unwrap_or(Path::new(""));
                let parent_inode = path_to_inode
                    .get(parent_path)
                    .copied()
                    .unwrap_or(ROOT_INODE);

                let dir_inode = inode_counter;
                inode_counter += 1;

                let mtime = file
                    .last_modified()
                    .map(datetime_to_unix)
                    .unwrap_or(0);

                index.insert(
                    dir_inode,
                    IndexEntry {
                        parent: parent_inode,
                        name: dir_name.clone(),
                        attributes: EntryAttributes {
                            inode: dir_inode,
                            entry_type: EntryType::Directory,
                            size: 0,
                            mode: 0o755,
                            uid: 0,
                            gid: 0,
                            mtime_secs: mtime,
                            nlink: 2,
                        },
                        children: Vec::new(),
                        children_by_name: HashMap::new(),
                        zip_index: Some(i),
                    },
                );

                if let Some(parent) = index.entries.get_mut(&parent_inode) {
                    parent.children.push(dir_inode);
                    parent.children_by_name.insert(dir_name, dir_inode);
                }

                path_to_inode.insert(entry_path, dir_inode);
            } else {
                // Regular file entry
                let file_name = entry_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let parent_path = entry_path.parent().unwrap_or(Path::new(""));
                let parent_inode = path_to_inode
                    .get(parent_path)
                    .copied()
                    .unwrap_or(ROOT_INODE);

                let file_inode = inode_counter;
                inode_counter += 1;

                let size = file.size();
                total_size += size;

                let mtime = file
                    .last_modified()
                    .map(datetime_to_unix)
                    .unwrap_or(0);

                // Extract Unix mode if available, otherwise default
                let mode = file.unix_mode().unwrap_or(0o644) as u32;

                index.insert(
                    file_inode,
                    IndexEntry {
                        parent: parent_inode,
                        name: file_name.clone(),
                        attributes: EntryAttributes {
                            inode: file_inode,
                            entry_type: EntryType::File,
                            size,
                            mode,
                            uid: 0,
                            gid: 0,
                            mtime_secs: mtime,
                            nlink: 1,
                        },
                        children: Vec::new(),
                        children_by_name: HashMap::new(),
                        zip_index: Some(i),
                    },
                );

                if let Some(parent) = index.entries.get_mut(&parent_inode) {
                    parent.children.push(file_inode);
                    parent.children_by_name.insert(file_name, file_inode);
                }

                path_to_inode.insert(entry_path, file_inode);
            }
        }

        // Sort children by name for consistent enumeration order
        let name_map: HashMap<InodeId, String> = index
            .entries
            .iter()
            .map(|(&id, e)| (id, e.name.clone()))
            .collect();

        for entry in index.entries.values_mut() {
            entry.children.sort_by(|a, b| name_map[a].cmp(&name_map[b]));
        }

        Ok((index, total_size))
    }
}

/// Convert a `zip::DateTime` to a Unix timestamp (seconds since epoch).
fn datetime_to_unix(dt: zip::DateTime) -> i64 {
    // zip::DateTime stores year/month/day/hour/minute/second
    // Use a simple calculation (not leapsecond-precise, but good enough)
    let year = dt.year() as i64;
    let month = dt.month() as i64;
    let day = dt.day() as i64;
    let hour = dt.hour() as i64;
    let minute = dt.minute() as i64;
    let second = dt.second() as i64;

    // Days from epoch (1970-01-01) using a simplified formula
    let mut y = year;
    let mut m = month;
    if m <= 2 {
        y -= 1;
        m += 12;
    }
    let days = 365 * y + y / 4 - y / 100 + y / 400 + (153 * (m - 3) + 2) / 5 + day - 719469;
    days * 86400 + hour * 3600 + minute * 60 + second
}

impl VirtualFsProvider for ZipFsProvider {
    fn resolve_path(&self, path: &Path) -> CoreResult<Option<InodeId>> {
        let components: Vec<_> = path
            .components()
            .filter_map(|c| match c {
                Component::Normal(name) => Some(name.to_string_lossy().to_string()),
                Component::CurDir => None,
                Component::RootDir => None,
                Component::Prefix(_) => None,
                Component::ParentDir => None,
            })
            .collect();

        if components.is_empty() {
            return Ok(Some(ROOT_INODE));
        }

        let mut current_inode = ROOT_INODE;
        for component in &components {
            match self.lookup(current_inode, component) {
                Ok(Some(entry)) => current_inode = entry.attributes.inode,
                Ok(None) => return Ok(None),
                Err(CoreError::NotADirectory(_)) => return Ok(None),
                Err(e) => return Err(e),
            }
        }

        Ok(Some(current_inode))
    }

    fn get_attributes(&self, inode: InodeId) -> CoreResult<EntryAttributes> {
        Ok(self.index.get(inode)?.attributes.clone())
    }

    fn list_directory(&self, inode: InodeId, cookie: u64) -> CoreResult<DirEntryBatch> {
        let entry = self.index.get(inode)?;
        if entry.attributes.entry_type != EntryType::Directory {
            return Err(CoreError::NotADirectory(inode));
        }

        let children = &entry.children;
        let start = cookie as usize;
        let end = (start + DIR_PAGE_SIZE).min(children.len());

        if start >= children.len() {
            return Ok(DirEntryBatch {
                entries: vec![],
                next_cookie: 0,
            });
        }

        let entries: CoreResult<Vec<DirEntry>> = children[start..end]
            .iter()
            .map(|&child_inode| {
                let child = self.index.get(child_inode)?;
                Ok(DirEntry {
                    name: child.name.clone(),
                    attributes: child.attributes.clone(),
                })
            })
            .collect();

        let next_cookie = if end < children.len() {
            end as u64
        } else {
            0
        };

        Ok(DirEntryBatch {
            entries: entries?,
            next_cookie,
        })
    }

    fn lookup(&self, parent_inode: InodeId, name: &str) -> CoreResult<Option<DirEntry>> {
        let parent = self.index.get(parent_inode)?;
        if parent.attributes.entry_type != EntryType::Directory {
            return Err(CoreError::NotADirectory(parent_inode));
        }

        // O(1) lookup, with case-insensitive fallback
        let child_inode_opt = parent
            .children_by_name
            .get(name)
            .or_else(|| {
                let lower = name.to_lowercase();
                parent
                    .children_by_name
                    .iter()
                    .find(|(k, _)| k.to_lowercase() == lower)
                    .map(|(_, v)| v)
            });

        match child_inode_opt {
            Some(&child_inode) => {
                let child = self.index.get(child_inode)?;
                Ok(Some(DirEntry {
                    name: child.name.clone(),
                    attributes: child.attributes.clone(),
                }))
            }
            None => Ok(None),
        }
    }

    fn read_file(&self, inode: InodeId, offset: u64, length: u64) -> CoreResult<Vec<u8>> {
        let entry = self.index.get(inode)?;
        if entry.attributes.entry_type != EntryType::File {
            return Err(CoreError::NotAFile(inode));
        }

        let zip_idx = entry.zip_index.ok_or_else(|| {
            CoreError::Io(format!("inode {inode} has no ZIP archive index"))
        })?;

        let mut archive = self.archive.lock().map_err(|e| {
            CoreError::Io(format!("failed to lock archive: {e}"))
        })?;

        let mut file = archive.by_index(zip_idx).map_err(|e| {
            CoreError::Io(format!("failed to read ZIP entry {zip_idx}: {e}"))
        })?;

        let file_size = file.size();
        if offset >= file_size {
            return Ok(Vec::new());
        }

        let actual_length = length.min(file_size - offset) as usize;

        // Read and decompress the full entry then slice
        // (ZIP doesn't support random access within compressed entries)
        let mut full_data = Vec::with_capacity(file_size as usize);
        file.read_to_end(&mut full_data).map_err(|e| {
            CoreError::Io(format!("failed to decompress ZIP entry: {e}"))
        })?;

        let start = offset as usize;
        let end = (start + actual_length).min(full_data.len());
        Ok(full_data[start..end].to_vec())
    }

    fn read_symlink(&self, _inode: InodeId) -> CoreResult<String> {
        // ZIP archives don't natively support symlinks in a cross-platform way
        Err(CoreError::NotASymlink(0))
    }

    fn list_xattrs(&self, inode: InodeId) -> CoreResult<Vec<String>> {
        let _ = self.index.get(inode)?;
        Ok(vec![])
    }

    fn get_xattr(&self, inode: InodeId, name: &str) -> CoreResult<Vec<u8>> {
        let _ = self.index.get(inode)?;
        Err(CoreError::NotFound(format!(
            "xattr '{name}' on inode {inode}"
        )))
    }

    fn check_access(&self, inode: InodeId, mask: u32) -> CoreResult<bool> {
        let _ = self.index.get(inode)?;
        // Read-only filesystem: deny W_OK (2)
        Ok(mask & 2 == 0)
    }

    fn volume_stats(&self) -> CoreResult<VolumeStats> {
        Ok(VolumeStats {
            total_bytes: self.total_size,
            used_bytes: self.total_size,
            total_inodes: self.index.len() as u64,
            used_inodes: self.index.len() as u64,
            block_size: 4096,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Helper: create a small in-memory ZIP archive for testing.
    fn create_test_zip() -> Vec<u8> {
        let buf = Vec::new();
        let cursor = Cursor::new(buf);
        let mut writer = zip::ZipWriter::new(cursor);

        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);

        // Create some files and directories
        writer.add_directory("subdir/", options).unwrap();
        writer
            .start_file("hello.txt", options)
            .unwrap();
        writer.write_all(b"Hello, World!\n").unwrap();

        writer
            .start_file("subdir/nested.txt", options)
            .unwrap();
        writer
            .write_all(b"This is a nested file inside subdir.\n")
            .unwrap();

        writer
            .start_file("subdir/another.txt", options)
            .unwrap();
        writer.write_all(b"Another file.\n").unwrap();

        let cursor = writer.finish().unwrap();
        cursor.into_inner()
    }

    #[test]
    fn open_nonexistent_returns_error() {
        let result = ZipFsProvider::open(Path::new("/nonexistent/archive.zip"));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CoreError::Io(_)));
    }

    #[test]
    fn open_invalid_data_returns_error() {
        let result = ZipFsProvider::from_bytes(b"this is not a zip file".to_vec());
        assert!(result.is_err());
    }

    #[test]
    fn open_valid_zip() {
        let data = create_test_zip();
        let provider = ZipFsProvider::from_bytes(data).unwrap();
        assert!(provider.index.len() > 0);
    }

    #[test]
    fn resolve_path_root() {
        let p = ZipFsProvider::from_bytes(create_test_zip()).unwrap();
        assert_eq!(p.resolve_path(Path::new("")).unwrap(), Some(ROOT_INODE));
        assert_eq!(p.resolve_path(Path::new(".")).unwrap(), Some(ROOT_INODE));
        assert_eq!(p.resolve_path(Path::new("/")).unwrap(), Some(ROOT_INODE));
    }

    #[test]
    fn resolve_path_file() {
        let p = ZipFsProvider::from_bytes(create_test_zip()).unwrap();
        let result = p.resolve_path(Path::new("hello.txt")).unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn resolve_path_directory() {
        let p = ZipFsProvider::from_bytes(create_test_zip()).unwrap();
        let result = p.resolve_path(Path::new("subdir")).unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn resolve_path_nested() {
        let p = ZipFsProvider::from_bytes(create_test_zip()).unwrap();
        let result = p.resolve_path(Path::new("subdir/nested.txt")).unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn resolve_path_nonexistent() {
        let p = ZipFsProvider::from_bytes(create_test_zip()).unwrap();
        assert_eq!(
            p.resolve_path(Path::new("does_not_exist.xyz")).unwrap(),
            None
        );
    }

    #[test]
    fn get_attributes_root() {
        let p = ZipFsProvider::from_bytes(create_test_zip()).unwrap();
        let attrs = p.get_attributes(ROOT_INODE).unwrap();
        assert!(attrs.is_dir());
    }

    #[test]
    fn get_attributes_file() {
        let p = ZipFsProvider::from_bytes(create_test_zip()).unwrap();
        let inode = p.resolve_path(Path::new("hello.txt")).unwrap().unwrap();
        let attrs = p.get_attributes(inode).unwrap();
        assert!(attrs.is_file());
        assert_eq!(attrs.size, 14); // "Hello, World!\n"
    }

    #[test]
    fn list_directory_root() {
        let p = ZipFsProvider::from_bytes(create_test_zip()).unwrap();
        let batch = p.list_directory(ROOT_INODE, 0).unwrap();
        let names: Vec<&str> = batch.entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"hello.txt"));
        assert!(names.contains(&"subdir"));
    }

    #[test]
    fn list_directory_subdir() {
        let p = ZipFsProvider::from_bytes(create_test_zip()).unwrap();
        let inode = p.resolve_path(Path::new("subdir")).unwrap().unwrap();
        let batch = p.list_directory(inode, 0).unwrap();
        let names: Vec<&str> = batch.entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"nested.txt"));
        assert!(names.contains(&"another.txt"));
    }

    #[test]
    fn list_directory_on_file_fails() {
        let p = ZipFsProvider::from_bytes(create_test_zip()).unwrap();
        let inode = p.resolve_path(Path::new("hello.txt")).unwrap().unwrap();
        let err = p.list_directory(inode, 0).unwrap_err();
        assert!(matches!(err, CoreError::NotADirectory(_)));
    }

    #[test]
    fn lookup_existing() {
        let p = ZipFsProvider::from_bytes(create_test_zip()).unwrap();
        let entry = p.lookup(ROOT_INODE, "hello.txt").unwrap();
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().name, "hello.txt");
    }

    #[test]
    fn lookup_nonexistent() {
        let p = ZipFsProvider::from_bytes(create_test_zip()).unwrap();
        let entry = p.lookup(ROOT_INODE, "nope.xyz").unwrap();
        assert!(entry.is_none());
    }

    #[test]
    fn read_file_full() {
        let p = ZipFsProvider::from_bytes(create_test_zip()).unwrap();
        let inode = p.resolve_path(Path::new("hello.txt")).unwrap().unwrap();
        let data = p.read_file(inode, 0, 1024).unwrap();
        assert_eq!(String::from_utf8_lossy(&data), "Hello, World!\n");
    }

    #[test]
    fn read_file_with_offset() {
        let p = ZipFsProvider::from_bytes(create_test_zip()).unwrap();
        let inode = p.resolve_path(Path::new("hello.txt")).unwrap().unwrap();
        let data = p.read_file(inode, 7, 5).unwrap();
        assert_eq!(String::from_utf8_lossy(&data), "World");
    }

    #[test]
    fn read_file_past_end() {
        let p = ZipFsProvider::from_bytes(create_test_zip()).unwrap();
        let inode = p.resolve_path(Path::new("hello.txt")).unwrap().unwrap();
        let data = p.read_file(inode, 99999, 10).unwrap();
        assert!(data.is_empty());
    }

    #[test]
    fn read_file_on_directory_fails() {
        let p = ZipFsProvider::from_bytes(create_test_zip()).unwrap();
        let err = p.read_file(ROOT_INODE, 0, 10).unwrap_err();
        assert!(matches!(err, CoreError::NotAFile(_)));
    }

    #[test]
    fn check_access_read_allowed() {
        let p = ZipFsProvider::from_bytes(create_test_zip()).unwrap();
        let inode = p.resolve_path(Path::new("hello.txt")).unwrap().unwrap();
        assert!(p.check_access(inode, 4).unwrap());
    }

    #[test]
    fn check_access_write_denied() {
        let p = ZipFsProvider::from_bytes(create_test_zip()).unwrap();
        let inode = p.resolve_path(Path::new("hello.txt")).unwrap().unwrap();
        assert!(!p.check_access(inode, 2).unwrap());
    }

    #[test]
    fn volume_stats() {
        let p = ZipFsProvider::from_bytes(create_test_zip()).unwrap();
        let stats = p.volume_stats().unwrap();
        assert!(stats.total_inodes > 0);
        assert!(stats.total_bytes > 0);
        assert!(stats.block_size > 0);
    }

    #[test]
    fn nested_dir_without_explicit_entry() {
        // Test that directories are synthesized even without explicit dir entries
        let buf = Vec::new();
        let cursor = Cursor::new(buf);
        let mut writer = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default();

        // Only add a deeply nested file — no explicit directory entries
        writer.start_file("a/b/c/deep.txt", options).unwrap();
        writer.write_all(b"deep content").unwrap();

        let cursor = writer.finish().unwrap();
        let data = cursor.into_inner();

        let p = ZipFsProvider::from_bytes(data).unwrap();

        // All intermediate directories should be synthesized
        assert!(p.resolve_path(Path::new("a")).unwrap().is_some());
        assert!(p.resolve_path(Path::new("a/b")).unwrap().is_some());
        assert!(p.resolve_path(Path::new("a/b/c")).unwrap().is_some());
        assert!(p.resolve_path(Path::new("a/b/c/deep.txt")).unwrap().is_some());

        // Verify the synthesized dirs are actual directories
        let a_inode = p.resolve_path(Path::new("a")).unwrap().unwrap();
        assert!(p.get_attributes(a_inode).unwrap().is_dir());

        // And the file is a file
        let f_inode = p.resolve_path(Path::new("a/b/c/deep.txt")).unwrap().unwrap();
        let data = p.read_file(f_inode, 0, 100).unwrap();
        assert_eq!(String::from_utf8_lossy(&data), "deep content");
    }
}
