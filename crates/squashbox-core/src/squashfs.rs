//! SquashFS-backed implementation of `VirtualFsProvider`.
//!
//! Uses the `backhand` crate to read SquashFS images. Builds an in-memory
//! inode index at mount time for O(1) lookups.

use crate::provider::VirtualFsProvider;
use crate::types::*;
use std::collections::HashMap;
use std::io::{BufReader, Read, Seek};
use std::path::Path;
use std::sync::Arc;

/// Default page size for directory listing pagination.
const DIR_PAGE_SIZE: usize = 64;

/// An entry in the inode index, representing a single node in the FS tree.
#[derive(Debug, Clone)]
struct IndexEntry {
    /// Parent inode (ROOT_INODE for the root's own entry).
    parent: InodeId,
    /// Name of this entry (empty string for root).
    name: String,
    /// Cached attributes.
    attributes: EntryAttributes,
    /// For symlinks: the target path.
    symlink_target: Option<String>,
    /// For directories: sorted list of child inode IDs.
    children: Vec<InodeId>,
}

/// The in-memory inode index built from the SquashFS directory tree.
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

/// A SquashFS-backed implementation of `VirtualFsProvider`.
///
/// Thread-safe: the backhand `FilesystemReader` and the `InodeIndex` are
/// both behind `Arc` and immutable after construction. Multiple ProjFS
/// callback threads or FSKit dispatch queues can call into this concurrently.
pub struct SquashFsProvider {
    /// The backhand filesystem reader (immutable after open).
    reader: Arc<backhand::FilesystemReader>,
    /// The raw image bytes (kept alive for reader's lifetime).
    _image_data: Arc<Vec<u8>>,
    /// Precomputed inode index.
    index: InodeIndex,
}

// Safety: SquashFsProvider is immutable after construction.
// backhand::FilesystemReader is Send + Sync.
unsafe impl Send for SquashFsProvider {}
unsafe impl Sync for SquashFsProvider {}

impl SquashFsProvider {
    /// Open a SquashFS image file and build the inode index.
    ///
    /// This reads the entire image into memory and walks the directory tree
    /// to build an O(1) inode lookup table.
    ///
    /// # Errors
    ///
    /// Returns `CoreError::Io` if the file cannot be read.
    /// Returns `CoreError::SquashFs` if the image is not valid SquashFS.
    pub fn open(image_path: &Path) -> CoreResult<Self> {
        // 1. Read the image file into memory
        let image_data = std::fs::read(image_path).map_err(|e| {
            CoreError::Io(format!("failed to read {}: {e}", image_path.display()))
        })?;
        let image_data = Arc::new(image_data);

        // 2. Parse with backhand
        let reader = backhand::FilesystemReader::from_reader(
            std::io::Cursor::new(image_data.as_ref().as_slice()),
        )?;
        let reader = Arc::new(reader);

        // 3. Build inode index by walking the tree
        let index = Self::build_index(&reader)?;

        Ok(Self {
            reader,
            _image_data: image_data,
            index,
        })
    }

    /// Build the inode index from the parsed SquashFS filesystem.
    fn build_index(reader: &backhand::FilesystemReader) -> CoreResult<InodeIndex> {
        let mut index = InodeIndex::new();

        // We need to assign stable inode IDs. backhand uses InodeId internally.
        // We'll use a counter-based approach, walking the tree breadth-first.

        // First, insert the root entry
        let root_inode = ROOT_INODE;
        index.insert(
            root_inode,
            IndexEntry {
                parent: root_inode, // Root is its own parent
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
                symlink_target: None,
                children: Vec::new(),
            },
        );

        // Walk backhand nodes and populate the index
        let mut inode_counter: InodeId = 2; // Start after root
        let mut path_to_inode: HashMap<String, InodeId> = HashMap::new();
        path_to_inode.insert(String::new(), root_inode);

        for node in reader.nodes() {
            let fullpath = node.fullpath.to_string_lossy().to_string();
            // Strip leading "/" if present
            let relative = fullpath.trim_start_matches('/');

            // Skip root (already inserted)
            if relative.is_empty() {
                continue;
            }

            let inode_id = inode_counter;
            inode_counter += 1;

            // Determine parent path and name
            let (parent_path, name) = match relative.rsplit_once('/') {
                Some((parent, name)) => (parent.to_string(), name.to_string()),
                None => (String::new(), relative.to_string()),
            };

            let parent_inode = *path_to_inode
                .get(&parent_path)
                .unwrap_or(&root_inode);

            // Determine entry type and attributes from backhand node
            let (entry_type, size, mode, symlink_target) =
                Self::extract_node_info(node);

            let mtime_secs = node.header.mtime as i64;
            let uid = node.header.uid as u32;
            let gid = node.header.gid as u32;

            let attributes = EntryAttributes {
                inode: inode_id,
                entry_type,
                size,
                mode,
                uid,
                gid,
                mtime_secs,
                nlink: if entry_type == EntryType::Directory {
                    2
                } else {
                    1
                },
            };

            index.insert(
                inode_id,
                IndexEntry {
                    parent: parent_inode,
                    name: name.clone(),
                    attributes,
                    symlink_target,
                    children: Vec::new(),
                },
            );

            // Register this path for child resolution
            path_to_inode.insert(relative.to_string(), inode_id);

            // Add this inode as a child of its parent
            if let Some(parent_entry) = index.entries.get_mut(&parent_inode) {
                parent_entry.children.push(inode_id);
            }
        }

        // Sort children by name for consistent enumeration
        for entry in index.entries.values_mut() {
            // We need to sort children by their names, but we only have inode IDs.
            // We'll defer sorting until we have all entries populated.
        }
        // Now sort children by name
        let name_map: HashMap<InodeId, String> = index
            .entries
            .iter()
            .map(|(&id, e)| (id, e.name.clone()))
            .collect();

        for entry in index.entries.values_mut() {
            entry
                .children
                .sort_by(|a, b| name_map[a].cmp(&name_map[b]));
        }

        Ok(index)
    }

    /// Extract type, size, mode, and symlink target from a backhand node.
    fn extract_node_info(
        node: &backhand::Node<backhand::SquashfsFileReader>,
    ) -> (EntryType, u64, u32, Option<String>) {
        use backhand::InnerNode;

        match &node.inner {
            InnerNode::File(file) => {
                let size = file.basic.file_size as u64;
                (EntryType::File, size, node.header.permissions as u32, None)
            }
            InnerNode::Dir(_) => (
                EntryType::Directory,
                0,
                node.header.permissions as u32,
                None,
            ),
            InnerNode::Symlink(link) => {
                let target = link.link.to_string_lossy().to_string();
                let size = target.len() as u64;
                (
                    EntryType::Symlink,
                    size,
                    0o777, // Symlinks are always 0777
                    Some(target),
                )
            }
            InnerNode::CharacterDevice(_) => (
                EntryType::CharDevice,
                0,
                node.header.permissions as u32,
                None,
            ),
            InnerNode::BlockDevice(_) => (
                EntryType::BlockDevice,
                0,
                node.header.permissions as u32,
                None,
            ),
        }
    }

    /// Find the backhand node matching a given inode's full path.
    fn find_backhand_node(
        &self,
        inode: InodeId,
    ) -> CoreResult<&backhand::Node<backhand::SquashfsFileReader>> {
        // Reconstruct the full path from the inode index
        let fullpath = self.reconstruct_path(inode)?;

        // Find the matching node in backhand
        for node in self.reader.nodes() {
            let node_path = node.fullpath.to_string_lossy();
            let node_relative = node_path.trim_start_matches('/');
            if node_relative == fullpath {
                return Ok(node);
            }
        }

        Err(CoreError::NotFound(format!("backhand node for inode {inode}")))
    }

    /// Reconstruct the full relative path from root for a given inode.
    fn reconstruct_path(&self, inode: InodeId) -> CoreResult<String> {
        if inode == ROOT_INODE {
            return Ok(String::new());
        }

        let mut components = Vec::new();
        let mut current = inode;

        loop {
            let entry = self.index.get(current)?;
            if current == ROOT_INODE {
                break;
            }
            components.push(entry.name.clone());
            current = entry.parent;
        }

        components.reverse();
        Ok(components.join("/"))
    }
}

impl VirtualFsProvider for SquashFsProvider {
    fn resolve_path(&self, path: &Path) -> CoreResult<Option<InodeId>> {
        let path_str = path.to_string_lossy();

        // Empty path or "." = root
        if path_str.is_empty() || path_str == "." {
            return Ok(Some(ROOT_INODE));
        }

        // Normalize: strip leading/trailing slashes, convert backslashes
        let normalized = path_str
            .replace('\\', "/")
            .trim_start_matches('/')
            .trim_end_matches('/')
            .to_string();

        if normalized.is_empty() {
            return Ok(Some(ROOT_INODE));
        }

        // Walk path components
        let mut current_inode = ROOT_INODE;
        for component in normalized.split('/') {
            match self.lookup(current_inode, component)? {
                Some(entry) => current_inode = entry.attributes.inode,
                None => return Ok(None),
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

        for &child_inode in &parent.children {
            let child = self.index.get(child_inode)?;
            if child.name == name {
                return Ok(Some(DirEntry {
                    name: child.name.clone(),
                    attributes: child.attributes.clone(),
                }));
            }
        }

        Ok(None)
    }

    fn read_file(&self, inode: InodeId, offset: u64, length: u64) -> CoreResult<Vec<u8>> {
        let entry = self.index.get(inode)?;
        if entry.attributes.entry_type != EntryType::File {
            return Err(CoreError::NotAFile(inode));
        }

        // Find the backhand node and read its data
        let node = self.find_backhand_node(inode)?;

        let mut file_reader = self.reader.file(&node.inner).reader();
        let mut all_data = Vec::new();
        file_reader.read_to_end(&mut all_data)?;

        // Apply offset and length
        let start = (offset as usize).min(all_data.len());
        let end = (start + length as usize).min(all_data.len());

        Ok(all_data[start..end].to_vec())
    }

    fn read_symlink(&self, inode: InodeId) -> CoreResult<String> {
        let entry = self.index.get(inode)?;
        if entry.attributes.entry_type != EntryType::Symlink {
            return Err(CoreError::NotASymlink(inode));
        }

        entry
            .symlink_target
            .clone()
            .ok_or_else(|| CoreError::SquashFs(format!("symlink inode {inode} has no target")))
    }

    fn list_xattrs(&self, inode: InodeId) -> CoreResult<Vec<String>> {
        // Verify inode exists
        let _ = self.index.get(inode)?;
        // TODO: Implement xattr reading from backhand when API is available
        Ok(vec![])
    }

    fn get_xattr(&self, inode: InodeId, name: &str) -> CoreResult<Vec<u8>> {
        // Verify inode exists
        let _ = self.index.get(inode)?;
        // TODO: Implement xattr reading from backhand when API is available
        Err(CoreError::NotFound(format!("xattr '{name}' on inode {inode}")))
    }

    fn check_access(&self, inode: InodeId, mask: u32) -> CoreResult<bool> {
        let entry = self.index.get(inode)?;
        // Read-only filesystem: always deny W_OK (2)
        if mask & 2 != 0 {
            return Ok(false);
        }

        // Check read permission (R_OK = 4) against "other" bits
        if mask & 4 != 0 && entry.attributes.mode & 0o004 == 0 {
            return Ok(false);
        }

        // Check execute permission (X_OK = 1) against "other" bits
        if mask & 1 != 0 && entry.attributes.mode & 0o001 == 0 {
            return Ok(false);
        }

        Ok(true)
    }

    fn volume_stats(&self) -> CoreResult<VolumeStats> {
        let block_size = self.reader.block_size();

        // Calculate used bytes by summing all file sizes
        let mut used_bytes: u64 = 0;
        for entry in self.index.entries.values() {
            used_bytes += entry.attributes.size;
        }

        Ok(VolumeStats {
            total_bytes: used_bytes, // For read-only FS, total ≈ used
            used_bytes,
            total_inodes: self.index.len() as u64,
            used_inodes: self.index.len() as u64,
            block_size: block_size as u32,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Helper: get path to test fixtures directory.
    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
    }

    /// Helper: get path to the test SquashFS image.
    /// This image is created by the `create_test_fixture.sh` script.
    fn test_image_path() -> PathBuf {
        fixtures_dir().join("test.sqsh")
    }

    /// Helper: check if the test fixture exists.
    fn has_test_fixture() -> bool {
        test_image_path().exists()
    }

    // ── Construction tests ──

    #[test]
    fn open_nonexistent_file_returns_error() {
        let result = SquashFsProvider::open(Path::new("/nonexistent/image.sqsh"));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CoreError::Io(_)));
    }

    #[test]
    fn open_invalid_file_returns_error() {
        // Create a temporary file with invalid content
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("invalid.sqsh");
        std::fs::write(&path, b"this is not a squashfs image").unwrap();

        let result = SquashFsProvider::open(&path);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CoreError::SquashFs(_)));
    }

    #[test]
    fn open_empty_file_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.sqsh");
        std::fs::write(&path, b"").unwrap();

        let result = SquashFsProvider::open(&path);
        assert!(result.is_err());
    }

    // ── Tests that require a test fixture ──
    // These test the real backhand integration

    #[test]
    fn open_valid_image() {
        if !has_test_fixture() {
            eprintln!("SKIP: test fixture not found at {:?}", test_image_path());
            return;
        }
        let provider = SquashFsProvider::open(&test_image_path()).unwrap();
        assert!(provider.index.len() > 0);
    }

    #[test]
    fn resolve_path_root_on_real_image() {
        if !has_test_fixture() {
            return;
        }
        let p = SquashFsProvider::open(&test_image_path()).unwrap();
        assert_eq!(p.resolve_path(Path::new("")).unwrap(), Some(ROOT_INODE));
        assert_eq!(p.resolve_path(Path::new(".")).unwrap(), Some(ROOT_INODE));
        assert_eq!(p.resolve_path(Path::new("/")).unwrap(), Some(ROOT_INODE));
    }

    #[test]
    fn resolve_path_existing_file_on_real_image() {
        if !has_test_fixture() {
            return;
        }
        let p = SquashFsProvider::open(&test_image_path()).unwrap();
        // The test fixture should contain "hello.txt"
        let result = p.resolve_path(Path::new("hello.txt")).unwrap();
        assert!(result.is_some(), "expected hello.txt to exist");
    }

    #[test]
    fn resolve_path_nonexistent_on_real_image() {
        if !has_test_fixture() {
            return;
        }
        let p = SquashFsProvider::open(&test_image_path()).unwrap();
        assert_eq!(
            p.resolve_path(Path::new("does_not_exist.xyz")).unwrap(),
            None
        );
    }

    #[test]
    fn get_attributes_root_on_real_image() {
        if !has_test_fixture() {
            return;
        }
        let p = SquashFsProvider::open(&test_image_path()).unwrap();
        let attrs = p.get_attributes(ROOT_INODE).unwrap();
        assert!(attrs.is_dir());
    }

    #[test]
    fn list_directory_root_on_real_image() {
        if !has_test_fixture() {
            return;
        }
        let p = SquashFsProvider::open(&test_image_path()).unwrap();
        let batch = p.list_directory(ROOT_INODE, 0).unwrap();
        assert!(!batch.entries.is_empty(), "root should have children");
    }

    #[test]
    fn list_directory_on_file_fails() {
        if !has_test_fixture() {
            return;
        }
        let p = SquashFsProvider::open(&test_image_path()).unwrap();
        let file_inode = p
            .resolve_path(Path::new("hello.txt"))
            .unwrap()
            .expect("hello.txt should exist");
        let err = p.list_directory(file_inode, 0).unwrap_err();
        assert!(matches!(err, CoreError::NotADirectory(_)));
    }

    #[test]
    fn lookup_existing_on_real_image() {
        if !has_test_fixture() {
            return;
        }
        let p = SquashFsProvider::open(&test_image_path()).unwrap();
        let entry = p.lookup(ROOT_INODE, "hello.txt").unwrap();
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().name, "hello.txt");
    }

    #[test]
    fn lookup_nonexistent_on_real_image() {
        if !has_test_fixture() {
            return;
        }
        let p = SquashFsProvider::open(&test_image_path()).unwrap();
        let entry = p.lookup(ROOT_INODE, "nope.xyz").unwrap();
        assert!(entry.is_none());
    }

    #[test]
    fn read_file_on_real_image() {
        if !has_test_fixture() {
            return;
        }
        let p = SquashFsProvider::open(&test_image_path()).unwrap();
        let inode = p
            .resolve_path(Path::new("hello.txt"))
            .unwrap()
            .expect("hello.txt should exist");
        let data = p.read_file(inode, 0, 1024).unwrap();
        assert!(!data.is_empty());
        let content = String::from_utf8_lossy(&data);
        assert!(
            content.contains("Hello"),
            "expected 'Hello' in file content, got: {content}"
        );
    }

    #[test]
    fn read_file_with_offset_on_real_image() {
        if !has_test_fixture() {
            return;
        }
        let p = SquashFsProvider::open(&test_image_path()).unwrap();
        let inode = p
            .resolve_path(Path::new("hello.txt"))
            .unwrap()
            .expect("hello.txt should exist");

        // Read full file
        let full = p.read_file(inode, 0, 1_000_000).unwrap();

        // Read with offset
        if full.len() > 5 {
            let partial = p.read_file(inode, 5, 10).unwrap();
            let expected = &full[5..(15.min(full.len()))];
            assert_eq!(partial, expected);
        }
    }

    #[test]
    fn read_file_on_directory_fails_on_real_image() {
        if !has_test_fixture() {
            return;
        }
        let p = SquashFsProvider::open(&test_image_path()).unwrap();
        let err = p.read_file(ROOT_INODE, 0, 10).unwrap_err();
        assert!(matches!(err, CoreError::NotAFile(_)));
    }

    #[test]
    fn check_access_read_on_real_image() {
        if !has_test_fixture() {
            return;
        }
        let p = SquashFsProvider::open(&test_image_path()).unwrap();
        // Root dir should be readable
        assert!(p.check_access(ROOT_INODE, 4).unwrap()); // R_OK
    }

    #[test]
    fn check_access_write_denied_on_real_image() {
        if !has_test_fixture() {
            return;
        }
        let p = SquashFsProvider::open(&test_image_path()).unwrap();
        // Write should always be denied (read-only FS)
        assert!(!p.check_access(ROOT_INODE, 2).unwrap()); // W_OK
    }

    #[test]
    fn volume_stats_on_real_image() {
        if !has_test_fixture() {
            return;
        }
        let p = SquashFsProvider::open(&test_image_path()).unwrap();
        let stats = p.volume_stats().unwrap();
        assert!(stats.total_inodes > 0);
        assert!(stats.block_size > 0);
    }

    #[test]
    fn read_symlink_on_non_symlink_fails() {
        if !has_test_fixture() {
            return;
        }
        let p = SquashFsProvider::open(&test_image_path()).unwrap();
        let err = p.read_symlink(ROOT_INODE).unwrap_err();
        assert!(matches!(err, CoreError::NotASymlink(_)));
    }

    #[test]
    fn resolve_path_with_backslashes() {
        if !has_test_fixture() {
            return;
        }
        let p = SquashFsProvider::open(&test_image_path()).unwrap();
        // Windows-style path should work
        let result = p.resolve_path(Path::new("subdir\\nested.txt")).unwrap();
        // May or may not exist depending on the fixture, but shouldn't error
        let _ = result;
    }

    // ── InodeIndex unit tests ──

    #[test]
    fn inode_index_get_existing() {
        let mut idx = InodeIndex::new();
        idx.insert(
            1,
            IndexEntry {
                parent: 1,
                name: String::new(),
                attributes: EntryAttributes {
                    inode: 1,
                    entry_type: EntryType::Directory,
                    size: 0,
                    mode: 0o755,
                    uid: 0,
                    gid: 0,
                    mtime_secs: 0,
                    nlink: 2,
                },
                symlink_target: None,
                children: vec![],
            },
        );
        assert!(idx.get(1).is_ok());
    }

    #[test]
    fn inode_index_get_nonexistent() {
        let idx = InodeIndex::new();
        assert!(idx.get(999).is_err());
    }

    #[test]
    fn inode_index_len() {
        let mut idx = InodeIndex::new();
        assert_eq!(idx.len(), 0);
        idx.insert(
            1,
            IndexEntry {
                parent: 1,
                name: String::new(),
                attributes: EntryAttributes {
                    inode: 1,
                    entry_type: EntryType::Directory,
                    size: 0,
                    mode: 0o755,
                    uid: 0,
                    gid: 0,
                    mtime_secs: 0,
                    nlink: 2,
                },
                symlink_target: None,
                children: vec![],
            },
        );
        assert_eq!(idx.len(), 1);
    }
}
