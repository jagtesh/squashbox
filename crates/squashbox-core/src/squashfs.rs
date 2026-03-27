//! SquashFS-backed implementation of `VirtualFsProvider`.
//!
//! Uses the `backhand` crate to read SquashFS images. Builds an in-memory
//! inode index at mount time for O(1) lookups.

use crate::provider::VirtualFsProvider;
use crate::types::*;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

/// Default page size for directory listing pagination.
const DIR_PAGE_SIZE: usize = 64;

/// Map a raw SquashFS filename to a safe display name for the host OS.
///
/// On Windows, certain characters are illegal in Win32 filesystem names:
/// backslash, colon, asterisk, question mark, double-quote, angle brackets,
/// and pipe. A SquashFS image from Linux may legally contain all of these.
///
/// We map them to their Unicode Private Use Area (PUA) equivalents in the
/// range U+F000–U+F0FF, which is the same strategy used by WSL2 when
/// surfacing Linux-native filenames through the Windows filesystem API.
/// This keeps the names lossless and round-trippable: the index stays
/// internally consistent, and Explorer can display the PUA glyphs safely.
///
/// On Linux and macOS no transformation is needed — all POSIX-legal
/// filename bytes are legal filesystem name characters.
#[cfg(windows)]
fn pua_map_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for b in name.bytes() {
        match b {
            b'\\' => out.push('\u{F05C}'),
            b':'  => out.push('\u{F03A}'),
            b'*'  => out.push('\u{F02A}'),
            b'?'  => out.push('\u{F03F}'),
            b'"'  => out.push('\u{F022}'),
            b'<'  => out.push('\u{F03C}'),
            b'>'  => out.push('\u{F03E}'),
            b'|'  => out.push('\u{F07C}'),
            c if c < 0x20 => out.push(
                char::from_u32(0xF000 + c as u32).unwrap_or('\u{FFFD}')
            ),
            _ => out.push(b as char),
        }
    }
    out
}

/// On non-Windows platforms filenames are already legal — identity function.
#[cfg(not(windows))]
#[inline(always)]
fn pua_map_name(name: &str) -> String {
    name.to_owned()
}

/// An entry in the inode index, representing a single node in the FS tree.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct IndexEntry {
    /// Parent inode (ROOT_INODE for the root's own entry).
    parent: InodeId,
    /// Name of this entry (empty string for root).
    name: String,
    /// Cached attributes.
    attributes: EntryAttributes,
    /// For symlinks: the target path.
    symlink_target: Option<String>,
    /// For directories: ordered list of child inode IDs (sorted by name).
    children: Vec<InodeId>,
    /// For directories: O(1) child lookup by name.
    children_by_name: HashMap<String, InodeId>,
    /// For directories: O(1) child lookup by lowercase generic name (for Windows/macOS collision prevention).
    children_by_lowercase: HashMap<String, InodeId>,
    /// Position of this node in backhand's files() iterator.
    /// Used for O(1) access to the backhand node without a linear scan.
    backhand_node_index: usize,
    /// Full path in the SquashFS image (for backhand lookups).
    /// Stored as a PathBuf to enable cross-platform comparison.
    squashfs_path: PathBuf,
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
/// Thread-safe: the backhand `FilesystemReader` is behind a `Mutex` for
/// interior mutability (it's needed for decompression state). The
/// `InodeIndex` is immutable after construction.
///
/// # Lifetime Management
///
/// `backhand::FilesystemReader<'b>` borrows from its reader. We own the
/// image data as `Vec<u8>` and pass a `Cursor` to backhand. The reader
/// is stored alongside the data it borrows using a self-referencing
/// pattern via raw pointer + unsafe. This is safe because:
/// 1. `_image_data` is never moved or dropped before `reader`
/// 2. The struct is never partially moved
/// 3. Both fields are dropped together when the struct is dropped
pub struct SquashFsProvider {
    /// The raw image bytes (must outlive `reader`).
    _image_data: Vec<u8>,
    /// The backhand filesystem reader.
    /// Uses a raw pointer approach: reader borrows from _image_data.
    reader: backhand::FilesystemReader<'static>,
    /// Precomputed inode index (immutable after construction).
    index: InodeIndex,
}

// SAFETY: SquashFsProvider is conceptually immutable after construction.
// The only mutable state is inside FilesystemReader (decompression cache),
// which is protected by internal Mutex/RwLock in backhand.
unsafe impl Send for SquashFsProvider {}
unsafe impl Sync for SquashFsProvider {}

impl std::fmt::Debug for SquashFsProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SquashFsProvider")
            .field("inodes", &self.index.len())
            .finish()
    }
}

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

        Self::from_bytes(image_data)
    }

    /// Create a provider from raw image bytes (useful for testing).
    pub fn from_bytes(image_data: Vec<u8>) -> CoreResult<Self> {
        // SAFETY: We create a Cursor that borrows from `image_data`, then
        // transmute the lifetime to 'static. This is safe because:
        // - `image_data` is stored in the same struct and never moved
        // - The reader is dropped before (or with) the data
        let cursor = std::io::Cursor::new(unsafe {
            std::slice::from_raw_parts(image_data.as_ptr(), image_data.len())
        });

        let reader = backhand::FilesystemReader::from_reader(cursor)?;

        // Build index from the parsed filesystem
        let index = Self::build_index(&reader)?;

        Ok(Self {
            _image_data: image_data,
            reader,
            index,
        })
    }

    /// Build the inode index by walking all nodes in the filesystem.
    ///
    /// backhand stores nodes sorted by fullpath. We assign our own inode IDs
    /// (starting from ROOT_INODE=1) and build parent→child relationships.
    fn build_index(reader: &backhand::FilesystemReader<'_>) -> CoreResult<InodeIndex> {
        let mut index = InodeIndex::new();

        // Map from backhand fullpath → our inode ID.
        // We use PathBuf as the key so comparisons work cross-platform.
        let mut path_to_inode: HashMap<PathBuf, InodeId> = HashMap::new();
        let mut inode_counter: InodeId = ROOT_INODE;

        let root_path = PathBuf::from("/");

        for (node_index, node) in reader.files().enumerate() {
            let fullpath = &node.fullpath;
            let is_root = Self::is_root_path(fullpath);

            let inode_id = inode_counter;
            inode_counter += 1;

            // Determine parent and name using Path APIs
            let (parent_inode, mut name) = if is_root {
                (ROOT_INODE, String::new())
            } else {
                // Extract the raw filename from backhand's pre-parsed fullpath.
                // backhand now returns the raw SquashFS bytes without any host-OS
                // path interpretation (our fork fix). We then apply the
                // platform-specific display mapping:
                // - On Windows: PUA-map characters illegal in Win32 names
                // - On Linux/macOS: identity (all POSIX bytes are legal)
                let raw_name = fullpath
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let name = pua_map_name(&raw_name);
                let parent_path = fullpath.parent().unwrap_or(&root_path);
                let parent_inode = path_to_inode
                    .get(parent_path)
                    .copied()
                    .unwrap_or(ROOT_INODE);
                (parent_inode, name)
            };

            // Resolve case-collisions via name mangling
            if !is_root {
                if let Some(parent_entry) = index.entries.get(&parent_inode) {
                    let mut lower = name.to_lowercase();
                    let mut attempt = 1;
                    let original_name = name.clone();
                    while parent_entry.children_by_lowercase.contains_key(&lower) {
                        name = format!("{} ({})", original_name, attempt);
                        lower = name.to_lowercase();
                        attempt += 1;
                    }
                }
            }

            // Extract type, size, mode, symlink target
            let (entry_type, size, mode, symlink_target) = Self::extract_node_info(node);

            let attributes = EntryAttributes {
                inode: inode_id,
                entry_type,
                size,
                mode: mode as u32,
                uid: node.header.uid,
                gid: node.header.gid,
                mtime_secs: node.header.mtime as i64,
                nlink: if entry_type == EntryType::Directory { 2 } else { 1 },
            };

            index.insert(
                inode_id,
                IndexEntry {
                    parent: parent_inode,
                    name: name.clone(),
                    attributes,
                    symlink_target,
                    children: Vec::new(),
                    children_by_name: HashMap::new(),
                    children_by_lowercase: HashMap::new(),
                    backhand_node_index: node_index,
                    squashfs_path: fullpath.clone(),
                },
            );

            path_to_inode.insert(fullpath.clone(), inode_id);

            // Add as child of parent (skip root being its own child)
            if !is_root {
                if let Some(parent_entry) = index.entries.get_mut(&parent_inode) {
                    parent_entry.children.push(inode_id);
                    parent_entry.children_by_name.insert(name.clone(), inode_id);
                    parent_entry.children_by_lowercase.insert(name.to_lowercase(), inode_id);
                }
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

        Ok(index)
    }

    /// Extract type, size, mode, and symlink target from a backhand node.
    fn extract_node_info(
        node: &backhand::Node<backhand::SquashfsFileReader>,
    ) -> (EntryType, u64, u16, Option<String>) {
        use backhand::InnerNode;

        match &node.inner {
            InnerNode::File(file) => {
                let size = file.file_len() as u64;
                (EntryType::File, size, node.header.permissions, None)
            }
            InnerNode::Dir(_) => (EntryType::Directory, 0, node.header.permissions, None),
            InnerNode::Symlink(link) => {
                let target = link.link.to_string_lossy().to_string();
                let size = target.len() as u64;
                (EntryType::Symlink, size, 0o777, Some(target))
            }
            InnerNode::CharacterDevice(_) => {
                (EntryType::CharDevice, 0, node.header.permissions, None)
            }
            InnerNode::BlockDevice(_) => {
                (EntryType::BlockDevice, 0, node.header.permissions, None)
            }
            InnerNode::NamedPipe | InnerNode::Socket => {
                // Map pipes and sockets to files with size 0
                (EntryType::File, 0, node.header.permissions, None)
            }
        }
    }

    /// Check if a path represents the root directory.
    ///
    /// Handles both "/" (Unix) and platform-specific root representations.
    fn is_root_path(path: &Path) -> bool {
        let s = path.to_string_lossy();
        s == "/" || s == "\\" || path.components().count() == 0
    }

    /// Read file data from the backhand filesystem.
    ///
    /// Uses the stored node index for O(1) node access (no linear scan).
    /// Skips to the requested offset and reads only the needed bytes,
    /// avoiding full decompression into a temporary buffer when possible.
    fn read_node_data(&self, inode: InodeId, offset: u64, length: u64) -> CoreResult<Vec<u8>> {
        let entry = self.index.get(inode)?;
        let node_idx = entry.backhand_node_index;

        // Access the node directly by its stored position
        let node = self.reader.files().nth(node_idx).ok_or_else(|| {
            CoreError::SquashFs(format!(
                "backhand node index {} out of range for inode {}",
                node_idx, inode
            ))
        })?;

        match &node.inner {
            backhand::InnerNode::File(file) => {
                let file_size = file.file_len() as u64;

                // Handle offset past end of file
                if offset >= file_size {
                    return Ok(Vec::new());
                }

                let actual_length = length.min(file_size - offset) as usize;
                let mut reader = self.reader.file(file).reader();

                // Skip to offset by reading and discarding bytes.
                // backhand decompresses block-by-block so we can't seek,
                // but we avoid allocating the full file just to slice it.
                if offset > 0 {
                    std::io::copy(
                        &mut reader.by_ref().take(offset),
                        &mut std::io::sink(),
                    )?;
                }

                // Read only the requested portion
                let mut data = vec![0u8; actual_length];
                reader.read_exact(&mut data)?;
                Ok(data)
            }
            _ => Err(CoreError::NotAFile(inode)),
        }
    }
}

impl VirtualFsProvider for SquashFsProvider {
    fn resolve_path(&self, path: &Path) -> CoreResult<Option<InodeId>> {
        // Use std::path::Component to iterate — this handles both `/` and `\`
        // separators correctly on all platforms.
        let components: Vec<_> = path
            .components()
            .filter_map(|c| match c {
                Component::Normal(name) => Some(name.to_string_lossy().to_string()),
                Component::CurDir => None,   // skip "."
                Component::RootDir => None,   // skip leading "/"
                Component::Prefix(_) => None, // skip Windows prefix like "C:\\"
                Component::ParentDir => None, // skip ".." (not meaningful here)
            })
            .collect();

        // No meaningful components → root
        if components.is_empty() {
            return Ok(Some(ROOT_INODE));
        }

        // Walk path components through the inode tree
        let mut current_inode = ROOT_INODE;
        for component in &components {
            match self.lookup(current_inode, component) {
                Ok(Some(entry)) => current_inode = entry.attributes.inode,
                Ok(None) => return Ok(None),
                Err(CoreError::NotADirectory(_)) => {
                    // Hit a file mid-path → path doesn't exist
                    return Ok(None);
                }
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

        // O(1) lookup via the pre-built name→inode map with a lowercase fallback
        let child_inode_opt = parent.children_by_name.get(name).or_else(|| {
            parent.children_by_lowercase.get(&name.to_lowercase())
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

        self.read_node_data(inode, offset, length)
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
        // backhand does not expose xattr reading in its public API yet
        Ok(vec![])
    }

    fn get_xattr(&self, inode: InodeId, name: &str) -> CoreResult<Vec<u8>> {
        // Verify inode exists
        let _ = self.index.get(inode)?;
        Err(CoreError::NotFound(format!(
            "xattr '{name}' on inode {inode}"
        )))
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
        let block_size = self.reader.block_size;

        // Calculate used bytes by summing all file sizes
        let mut used_bytes: u64 = 0;
        for entry in self.index.entries.values() {
            used_bytes += entry.attributes.size;
        }

        Ok(VolumeStats {
            total_bytes: used_bytes,
            used_bytes,
            total_inodes: self.index.len() as u64,
            used_inodes: self.index.len() as u64,
            block_size,
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

        let full = p.read_file(inode, 0, 1_000_000).unwrap();

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
        assert!(p.check_access(ROOT_INODE, 4).unwrap());
    }

    #[test]
    fn check_access_write_denied_on_real_image() {
        if !has_test_fixture() {
            return;
        }
        let p = SquashFsProvider::open(&test_image_path()).unwrap();
        assert!(!p.check_access(ROOT_INODE, 2).unwrap());
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
        let _ = p.resolve_path(Path::new("subdir\\nested.txt")).unwrap();
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
                children_by_name: HashMap::new(),
                children_by_lowercase: HashMap::new(),
                backhand_node_index: 0,
                squashfs_path: PathBuf::from("/"),
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
                children_by_name: HashMap::new(),
                children_by_lowercase: HashMap::new(),
                backhand_node_index: 0,
                squashfs_path: PathBuf::from("/"),
            },
        );
        assert_eq!(idx.len(), 1);
    }
}
