# Squashbox Architecture

This document defines the architecture through skeletal implementations — traits, structs,
method signatures, and their interactions with the parent OS libraries (`windows-projfs`
and `fskit-rs`).

No business logic is implemented. The goal is to establish the contract between layers.

---

## Crate Structure

```
squashbox/
├── crates/
│   ├── squashbox-core/          # Platform-agnostic SquashFS abstraction
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── provider.rs      # VirtualFsProvider trait
│   │   │   ├── types.rs         # Shared types (entries, attributes, errors)
│   │   │   └── squashfs.rs      # backhand-backed implementation
│   │   └── Cargo.toml
│   │
│   ├── squashbox-windows/       # Windows ProjFS driver
│   │   ├── src/
│   │   │   ├── main.rs          # CLI entry point
│   │   │   └── projfs_source.rs # ProjectedFileSystemSource impl
│   │   └── Cargo.toml
│   │
│   ├── squashbox-macos/         # macOS FSKit driver (Rust side)
│   │   ├── src/
│   │   │   ├── main.rs          # Tokio entry point
│   │   │   └── fskit_fs.rs      # Filesystem trait impl
│   │   └── Cargo.toml
│   │
│   ├── windows-projfs/          # Local fork (git subtree)
│   └── fskit-rs/                # Local fork (git subtree)
│
├── docs/
└── Cargo.toml                   # Workspace root
```

---

## Layer 1: Core Abstraction (`squashbox-core`)

### `types.rs` — Shared Types

```rust
use std::time::SystemTime;

/// Unique identifier for a filesystem item.
/// On ProjFS this maps from a path lookup; on FSKit this is the inode number.
pub type InodeId = u64;

/// Root inode is always 1 (matches SquashFS convention).
pub const ROOT_INODE: InodeId = 1;

/// File type classification.
#[derive(Debug, Clone, PartialEq)]
pub enum EntryType {
    File,
    Directory,
    Symlink,
    // BlockDevice, CharDevice, Fifo, Socket — if needed later
}

/// Metadata for a single filesystem entry.
#[derive(Debug, Clone)]
pub struct EntryAttributes {
    pub inode: InodeId,
    pub entry_type: EntryType,
    pub size: u64,
    pub mode: u32,          // POSIX mode bits (e.g., 0o755)
    pub uid: u32,
    pub gid: u32,
    pub mtime: SystemTime,
    pub atime: SystemTime,
    pub ctime: SystemTime,
    pub nlink: u32,
}

/// A single directory entry (name + attributes).
#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub attributes: EntryAttributes,
}

/// Extended attribute.
#[derive(Debug, Clone)]
pub struct Xattr {
    pub name: String,
    pub value: Vec<u8>,
}

/// Volume-level statistics.
#[derive(Debug, Clone)]
pub struct VolumeStats {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub total_inodes: u64,
    pub used_inodes: u64,
    pub block_size: u32,
}

/// Core error type.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("entry not found: {0}")]
    NotFound(String),

    #[error("not a directory: inode {0}")]
    NotADirectory(InodeId),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("squashfs error: {0}")]
    SquashFs(String),

    #[error("operation not supported")]
    NotSupported,
}

pub type CoreResult<T> = Result<T, CoreError>;
```

---

### `provider.rs` — The VirtualFsProvider Trait

This is the **central abstraction**. Both platform drivers call into this trait.
It is intentionally synchronous — the async boundary lives in the driver layer.

```rust
use crate::types::*;
use std::io::Read;
use std::path::Path;

/// Platform-agnostic virtual filesystem provider.
///
/// Implementors provide read-only access to a filesystem image.
/// Both the ProjFS and FSKit drivers call these methods.
pub trait VirtualFsProvider: Send + Sync {
    // ── Path-based lookups (used primarily by ProjFS) ──

    /// Resolve a relative path to an inode ID.
    /// Returns None if the path does not exist.
    fn resolve_path(&self, path: &Path) -> CoreResult<Option<InodeId>>;

    // ── Inode-based operations (used by both drivers) ──

    /// Get attributes for a given inode.
    fn get_attributes(&self, inode: InodeId) -> CoreResult<EntryAttributes>;

    /// List all entries in a directory.
    /// Returns entries sorted by name.
    fn list_directory(&self, inode: InodeId) -> CoreResult<Vec<DirEntry>>;

    /// Look up a single entry by name within a directory.
    fn lookup(&self, parent_inode: InodeId, name: &str) -> CoreResult<Option<DirEntry>>;

    /// Read file content at a byte range.
    /// Returns a Read impl positioned at `offset`, returning up to `length` bytes.
    fn read_file(
        &self,
        inode: InodeId,
        offset: u64,
        length: u64,
    ) -> CoreResult<Box<dyn Read + Send + '_>>;

    /// Read the target of a symbolic link.
    fn read_symlink(&self, inode: InodeId) -> CoreResult<String>;

    // ── Extended attributes ──

    /// List xattr names for a given inode.
    fn list_xattrs(&self, inode: InodeId) -> CoreResult<Vec<String>>;

    /// Get the value of a specific xattr.
    fn get_xattr(&self, inode: InodeId, name: &str) -> CoreResult<Vec<u8>>;

    // ── Volume info ──

    /// Get filesystem-level statistics.
    fn volume_stats(&self) -> CoreResult<VolumeStats>;
}
```

---

### `squashfs.rs` — backhand Implementation

```rust
use crate::provider::VirtualFsProvider;
use crate::types::*;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

/// A SquashFS-backed implementation of VirtualFsProvider.
///
/// Thread-safe via interior Arc: multiple ProjFS callback threads
/// can call into this concurrently.
pub struct SquashFsProvider {
    // backhand::FilesystemReader is the main handle.
    // It is Send + Sync, so multiple threads can read concurrently.
    reader: backhand::FilesystemReader,
    // Precomputed inode-to-path index for fast lookups.
    // Built once at open() time by traversing the SquashFS directory tree.
    inode_index: InodeIndex,
}

/// Maps inode IDs to their directory entries and parent relationships.
struct InodeIndex {
    // inode → (parent_inode, name, entry_type, size, ...)
    // Built by walking the SquashFS tree at mount time.
    entries: std::collections::HashMap<InodeId, IndexEntry>,
}

struct IndexEntry {
    parent: InodeId,
    name: String,
    attributes: EntryAttributes,
}

impl SquashFsProvider {
    /// Open a SquashFS image file and build the inode index.
    pub fn open(image_path: &Path) -> CoreResult<Self> {
        // 1. Open the .sqsh file with backhand::FilesystemReader::from_reader()
        // 2. Walk the directory tree, building InodeIndex
        // 3. Return SquashFsProvider { reader, inode_index }
        todo!()
    }
}

impl VirtualFsProvider for SquashFsProvider {
    fn resolve_path(&self, path: &Path) -> CoreResult<Option<InodeId>> {
        // Walk path components through InodeIndex:
        // start at ROOT_INODE, for each component call lookup()
        todo!()
    }

    fn get_attributes(&self, inode: InodeId) -> CoreResult<EntryAttributes> {
        // Look up inode in index, return its attributes
        todo!()
    }

    fn list_directory(&self, inode: InodeId) -> CoreResult<Vec<DirEntry>> {
        // Verify inode is a directory
        // Collect all children from InodeIndex where parent == inode
        // Sort by name
        todo!()
    }

    fn lookup(&self, parent_inode: InodeId, name: &str) -> CoreResult<Option<DirEntry>> {
        // Search children of parent_inode for matching name
        todo!()
    }

    fn read_file(
        &self,
        inode: InodeId,
        offset: u64,
        length: u64,
    ) -> CoreResult<Box<dyn Read + Send + '_>> {
        // 1. Get the backhand SquashfsFileReader for this inode
        // 2. Seek to offset (may need to decompress blocks up to that point)
        // 3. Return a Read adapter limited to `length` bytes
        todo!()
    }

    fn read_symlink(&self, inode: InodeId) -> CoreResult<String> {
        // Read symlink target from the SquashFS inode
        todo!()
    }

    fn list_xattrs(&self, inode: InodeId) -> CoreResult<Vec<String>> {
        // Read xattr names from SquashFS inode metadata
        todo!()
    }

    fn get_xattr(&self, inode: InodeId, name: &str) -> CoreResult<Vec<u8>> {
        // Read specific xattr value
        todo!()
    }

    fn volume_stats(&self) -> CoreResult<VolumeStats> {
        // Read superblock for total size, inode count, block size
        todo!()
    }
}
```

---

## Layer 2a: Windows Driver (`squashbox-windows`)

### `projfs_source.rs` — Bridging VirtualFsProvider → windows-projfs

```rust
use squashbox_core::provider::VirtualFsProvider;
use squashbox_core::types::*;
use std::io::Read;
use std::ops::ControlFlow;
use std::path::Path;
use std::sync::Arc;

use windows_projfs::{
    DirectoryEntry, DirectoryInfo, FileInfo, Notification,
    ProjectedFileSystem, ProjectedFileSystemSource, ProjectionContext,
};

/// Adapts a VirtualFsProvider into a windows-projfs ProjectedFileSystemSource.
///
/// This struct holds a reference to the core provider and translates
/// between the path-based ProjFS callbacks and our inode-based core.
pub struct SquashboxProjFsSource {
    provider: Arc<dyn VirtualFsProvider>,
}

impl SquashboxProjFsSource {
    pub fn new(provider: Arc<dyn VirtualFsProvider>) -> Self {
        Self { provider }
    }
}

impl ProjectedFileSystemSource for SquashboxProjFsSource {
    /// Called by ProjFS when Explorer or an application enumerates a directory.
    ///
    /// Flow:
    ///   ProjFS callback → resolve path → list_directory() → map to DirectoryEntry
    fn list_directory(&self, path: &Path) -> Vec<DirectoryEntry> {
        // 1. Resolve relative path to inode
        let inode = match self.provider.resolve_path(path) {
            Ok(Some(id)) => id,
            _ => return vec![], // Empty = directory not found
        };

        // 2. List directory contents from core
        let entries = match self.provider.list_directory(inode) {
            Ok(entries) => entries,
            Err(_) => return vec![],
        };

        // 3. Map core DirEntry → windows_projfs DirectoryEntry
        entries
            .into_iter()
            .map(|e| match e.attributes.entry_type {
                EntryType::Directory => DirectoryEntry::Directory(DirectoryInfo {
                    name: e.name.into(),
                }),
                EntryType::File | EntryType::Symlink => DirectoryEntry::File(FileInfo {
                    name: e.name.into(),
                    size: e.attributes.size,
                    // Map timestamps...
                }),
            })
            .collect()
    }

    /// Called by ProjFS when a file needs to be hydrated (first read).
    ///
    /// Flow:
    ///   ProjFS callback → resolve path → read_file(offset, length) → return Read
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
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::NotFound, e))?
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "file not found")
            })?;

        // 2. Delegate to core read_file
        self.provider
            .read_file(inode, byte_offset as u64, length as u64)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    }

    /// Called by ProjFS for stat-like operations (get placeholder info).
    ///
    /// Flow:
    ///   ProjFS callback → resolve path → get_attributes() → Some(DirectoryEntry)
    fn get_directory_entry(&self, path: &Path) -> Option<DirectoryEntry> {
        let inode = self.provider.resolve_path(path).ok()??;
        let attrs = self.provider.get_attributes(inode).ok()?;

        Some(match attrs.entry_type {
            EntryType::Directory => DirectoryEntry::Directory(DirectoryInfo {
                name: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into(),
            }),
            EntryType::File | EntryType::Symlink => DirectoryEntry::File(FileInfo {
                name: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into(),
                size: attrs.size,
            }),
        })
    }

    /// Called for pre-operation notifications (file create, delete, rename, etc.)
    ///
    /// We deny all write operations — SquashFS is read-only.
    fn handle_notification(&self, _notification: &Notification) -> ControlFlow<()> {
        // Break = deny the operation
        ControlFlow::Break(())
    }
}
```

### `main.rs` — Windows Entry Point

```rust
use squashbox_core::squashfs::SquashFsProvider;
use std::path::PathBuf;
use std::sync::Arc;

mod projfs_source;
use projfs_source::SquashboxProjFsSource;

fn main() -> anyhow::Result<()> {
    // 1. Parse CLI arguments
    let image_path = PathBuf::from(std::env::args().nth(1).expect("usage: squashbox <image> <mount>"));
    let mount_point = PathBuf::from(std::env::args().nth(2).expect("usage: squashbox <image> <mount>"));

    // 2. Open SquashFS image
    let provider = Arc::new(SquashFsProvider::open(&image_path)?);

    // 3. Create ProjFS source adapter
    let source = SquashboxProjFsSource::new(provider);

    // 4. Start projected file system
    //    Internally calls PrjStartVirtualizing()
    let pfs = windows_projfs::ProjectedFileSystem::new(&mount_point, source)?;
    pfs.start()?;

    println!("Mounted {} at {}", image_path.display(), mount_point.display());
    println!("Press Ctrl+C to unmount...");

    // 5. Block until Ctrl+C
    let (tx, rx) = std::sync::mpsc::channel();
    ctrlc::set_handler(move || tx.send(()).unwrap())?;
    rx.recv()?;

    // 6. Stop projection (calls PrjStopVirtualizing)
    // pfs is dropped here, which stops the virtualization
    println!("Unmounting...");
    Ok(())
}
```

---

## Layer 2b: macOS Driver (`squashbox-macos`)

### `fskit_fs.rs` — Bridging VirtualFsProvider → fskit-rs

```rust
use async_trait::async_trait;
use squashbox_core::provider::VirtualFsProvider;
use squashbox_core::types::*;
use std::ffi::OsStr;
use std::sync::Arc;
use tokio::sync::Mutex;

use fskit_rs::{
    AccessMask, CaseFormat, DirectoryEntries, Error as FskitError, Filesystem,
    Item, ItemAttributes, ItemType, MountOptions, OpenMode, PathConfOperations,
    PreallocateFlag, ResourceIdentifier, SetXattrPolicy, StatFsResult,
    SupportedCapabilities, SyncFlags, TaskOptions, VolumeBehavior, VolumeIdentifier,
    Xattrs,
};

/// Adapts a VirtualFsProvider into an fskit-rs Filesystem.
///
/// Unlike the ProjFS driver, FSKit uses inode IDs directly —
/// no path resolution needed at this layer.
pub struct SquashboxFskitFs {
    provider: Arc<dyn VirtualFsProvider>,
    image_name: String,
}

impl SquashboxFskitFs {
    pub fn new(provider: Arc<dyn VirtualFsProvider>, image_name: String) -> Self {
        Self { provider, image_name }
    }

    /// Helper: convert CoreError to fskit-rs Error.
    fn map_err(e: CoreError) -> FskitError {
        match e {
            CoreError::NotFound(_) => FskitError::Posix(libc::ENOENT),
            CoreError::NotADirectory(_) => FskitError::Posix(libc::ENOTDIR),
            CoreError::Io(_) => FskitError::Posix(libc::EIO),
            CoreError::NotSupported => FskitError::Posix(libc::ENOSYS),
            CoreError::SquashFs(_) => FskitError::Posix(libc::EIO),
        }
    }

    /// Helper: convert core EntryAttributes to fskit-rs ItemAttributes.
    fn to_item_attributes(attrs: &EntryAttributes) -> ItemAttributes {
        ItemAttributes {
            // Map fields: size, mode, uid, gid, timestamps, nlink, etc.
            ..Default::default()
        }
    }

    /// Helper: convert core DirEntry to fskit-rs Item.
    fn to_item(entry: &DirEntry) -> Item {
        Item {
            // Map: id = entry.attributes.inode, name, attributes
            ..Default::default()
        }
    }
}

#[async_trait]
impl Filesystem for SquashboxFskitFs {
    // ── Volume Setup ──

    async fn get_resource_identifier(&mut self) -> fskit_rs::Result<ResourceIdentifier> {
        Ok(ResourceIdentifier {
            // Unique ID for this SquashFS image (could use image hash)
            ..Default::default()
        })
    }

    async fn get_volume_identifier(&mut self) -> fskit_rs::Result<VolumeIdentifier> {
        Ok(VolumeIdentifier {
            // Volume name derived from image filename
            ..Default::default()
        })
    }

    async fn get_volume_behavior(&mut self) -> fskit_rs::Result<VolumeBehavior> {
        Ok(VolumeBehavior {
            // CRITICAL: Mark as read-only
            // read_only: true,
            ..Default::default()
        })
    }

    async fn get_path_conf_operations(&mut self) -> fskit_rs::Result<PathConfOperations> {
        Ok(PathConfOperations::default())
    }

    async fn get_volume_capabilities(&mut self) -> fskit_rs::Result<SupportedCapabilities> {
        Ok(SupportedCapabilities {
            // Report: supports read, stat, readdir, readlink, xattr
            // Does not support: write, create, delete, rename
            ..Default::default()
        })
    }

    async fn get_volume_statistics(&mut self) -> fskit_rs::Result<StatFsResult> {
        let stats = self.provider.volume_stats().map_err(Self::map_err)?;
        Ok(StatFsResult {
            // Map VolumeStats → StatFsResult
            ..Default::default()
        })
    }

    // ── Lifecycle ──

    async fn activate(&mut self, _options: TaskOptions) -> fskit_rs::Result<Item> {
        // Return the root directory item
        let root_attrs = self.provider.get_attributes(ROOT_INODE).map_err(Self::map_err)?;
        Ok(Self::to_item(&DirEntry {
            name: String::new(),
            attributes: root_attrs,
        }))
    }

    async fn mount(&mut self, _options: TaskOptions) -> fskit_rs::Result<()> {
        // Volume is ready for I/O
        Ok(())
    }

    async fn unmount(&mut self) -> fskit_rs::Result<()> {
        // Clean shutdown
        Ok(())
    }

    async fn deactivate(&mut self) -> fskit_rs::Result<()> {
        // Release resources
        Ok(())
    }

    async fn synchronize(&mut self, _flags: SyncFlags) -> fskit_rs::Result<()> {
        // Read-only FS — nothing to flush
        Ok(())
    }

    // ── Read Operations (the core of our FS) ──

    async fn lookup_item(
        &mut self,
        name: &OsStr,
        directory_id: u64,
    ) -> fskit_rs::Result<Item> {
        // Flow: name + parent_inode → core.lookup() → Item
        let name_str = name.to_string_lossy();
        let entry = self
            .provider
            .lookup(directory_id, &name_str)
            .map_err(Self::map_err)?
            .ok_or(FskitError::Posix(libc::ENOENT))?;

        Ok(Self::to_item(&entry))
    }

    async fn get_attributes(&mut self, item_id: u64) -> fskit_rs::Result<ItemAttributes> {
        // Flow: inode → core.get_attributes() → ItemAttributes
        let attrs = self.provider.get_attributes(item_id).map_err(Self::map_err)?;
        Ok(Self::to_item_attributes(&attrs))
    }

    async fn enumerate_directory(
        &mut self,
        directory_id: u64,
        cookie: u64,
        _verifier: u64,
    ) -> fskit_rs::Result<DirectoryEntries> {
        // Flow: dir_inode + cookie → core.list_directory() → paginate → DirectoryEntries
        let all_entries = self.provider.list_directory(directory_id).map_err(Self::map_err)?;

        // Paginate using cookie as an offset index
        let page_size = 64; // entries per batch
        let start = cookie as usize;
        let end = (start + page_size).min(all_entries.len());
        let page = &all_entries[start..end];

        let items: Vec<Item> = page.iter().map(Self::to_item).collect();
        let next_cookie = if end < all_entries.len() { end as u64 } else { 0 };

        Ok(DirectoryEntries {
            // entries: items,
            // cookie: next_cookie,
            // verifier: 0,
            ..Default::default()
        })
    }

    async fn open_item(
        &mut self,
        item_id: u64,
        modes: Vec<OpenMode>,
    ) -> fskit_rs::Result<()> {
        // For read-only FS: allow OpenMode::Read, reject OpenMode::Write
        // No state to track — SquashFS doesn't have open handles
        Ok(())
    }

    async fn close_item(
        &mut self,
        _item_id: u64,
        _modes: Vec<OpenMode>,
    ) -> fskit_rs::Result<()> {
        // No-op for stateless reads
        Ok(())
    }

    async fn read(
        &mut self,
        item_id: u64,
        offset: i64,
        length: i64,
    ) -> fskit_rs::Result<Vec<u8>> {
        // Flow: inode + offset + length → core.read_file() → Read → Vec<u8>
        let mut reader = self
            .provider
            .read_file(item_id, offset as u64, length as u64)
            .map_err(Self::map_err)?;

        let mut buf = Vec::with_capacity(length as usize);
        reader
            .read_to_end(&mut buf)
            .map_err(|e| FskitError::Posix(libc::EIO))?;

        Ok(buf)
    }

    async fn read_symbolic_link(&mut self, item_id: u64) -> fskit_rs::Result<Vec<u8>> {
        // Flow: inode → core.read_symlink() → String → Vec<u8>
        let target = self.provider.read_symlink(item_id).map_err(Self::map_err)?;
        Ok(target.into_bytes())
    }

    // ── Extended Attributes ──

    async fn get_supported_xattr_names(
        &mut self,
        item_id: u64,
    ) -> fskit_rs::Result<Xattrs> {
        let names = self.provider.list_xattrs(item_id).map_err(Self::map_err)?;
        Ok(Xattrs {
            // Map names into Xattrs struct
            ..Default::default()
        })
    }

    async fn get_xattr(
        &mut self,
        name: &OsStr,
        item_id: u64,
    ) -> fskit_rs::Result<Vec<u8>> {
        let name_str = name.to_string_lossy();
        self.provider
            .get_xattr(item_id, &name_str)
            .map_err(Self::map_err)
    }

    async fn get_xattrs(&mut self, item_id: u64) -> fskit_rs::Result<Xattrs> {
        // Same as get_supported_xattr_names but with values
        let names = self.provider.list_xattrs(item_id).map_err(Self::map_err)?;
        Ok(Xattrs {
            ..Default::default()
        })
    }

    async fn set_xattr(
        &mut self,
        _name: &OsStr,
        _value: Option<Vec<u8>>,
        _item_id: u64,
        _policy: SetXattrPolicy,
    ) -> fskit_rs::Result<()> {
        Err(FskitError::Posix(libc::EROFS))
    }

    async fn check_access(
        &mut self,
        item_id: u64,
        _access: Vec<AccessMask>,
    ) -> fskit_rs::Result<bool> {
        // Always allow read access for now
        // Could check mode bits from core.get_attributes() against access masks
        Ok(true)
    }

    // ── Write Operations (all denied — read-only FS) ──

    async fn set_attributes(
        &mut self,
        _item_id: u64,
        _attributes: ItemAttributes,
    ) -> fskit_rs::Result<ItemAttributes> {
        Err(FskitError::Posix(libc::EROFS))
    }

    async fn create_item(
        &mut self,
        _name: &OsStr,
        _r#type: ItemType,
        _directory_id: u64,
        _attributes: ItemAttributes,
    ) -> fskit_rs::Result<Item> {
        Err(FskitError::Posix(libc::EROFS))
    }

    async fn create_symbolic_link(
        &mut self,
        _name: &OsStr,
        _directory_id: u64,
        _new_attributes: ItemAttributes,
        _contents: Vec<u8>,
    ) -> fskit_rs::Result<Item> {
        Err(FskitError::Posix(libc::EROFS))
    }

    async fn create_link(
        &mut self,
        _item_id: u64,
        _name: &OsStr,
        _directory_id: u64,
    ) -> fskit_rs::Result<Vec<u8>> {
        Err(FskitError::Posix(libc::EROFS))
    }

    async fn remove_item(
        &mut self,
        _item_id: u64,
        _name: &OsStr,
        _directory_id: u64,
    ) -> fskit_rs::Result<()> {
        Err(FskitError::Posix(libc::EROFS))
    }

    async fn rename_item(
        &mut self,
        _item_id: u64,
        _source_directory_id: u64,
        _source_name: &OsStr,
        _destination_name: &OsStr,
        _destination_directory_id: u64,
        _over_item_id: Option<u64>,
    ) -> fskit_rs::Result<Vec<u8>> {
        Err(FskitError::Posix(libc::EROFS))
    }

    async fn write(
        &mut self,
        _contents: Vec<u8>,
        _item_id: u64,
        _offset: i64,
    ) -> fskit_rs::Result<i64> {
        Err(FskitError::Posix(libc::EROFS))
    }

    async fn set_volume_name(&mut self, _name: Vec<u8>) -> fskit_rs::Result<Vec<u8>> {
        Err(FskitError::Posix(libc::EROFS))
    }

    async fn preallocate_space(
        &mut self,
        _item_id: u64,
        _offset: i64,
        _length: i64,
        _flags: Vec<PreallocateFlag>,
    ) -> fskit_rs::Result<i64> {
        Err(FskitError::Posix(libc::EROFS))
    }

    async fn reclaim_item(&mut self, _item_id: u64) -> fskit_rs::Result<()> {
        // Called when the kernel is done with an inode reference.
        // No action needed — we don't track open refs.
        Ok(())
    }

    async fn deactivate_item(&mut self, _item_id: u64) -> fskit_rs::Result<()> {
        Ok(())
    }
}
```

### `main.rs` — macOS Entry Point

```rust
use squashbox_core::squashfs::SquashFsProvider;
use std::path::PathBuf;
use std::sync::Arc;

mod fskit_fs;
use fskit_fs::SquashboxFskitFs;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    // 1. Parse CLI arguments
    let image_path = PathBuf::from(std::env::args().nth(1).expect("usage: squashbox <image> <mount>"));
    let mount_point = PathBuf::from(std::env::args().nth(2).expect("usage: squashbox <image> <mount>"));
    let image_name = image_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    // 2. Open SquashFS image
    let provider = Arc::new(SquashFsProvider::open(&image_path)?);

    // 3. Create FSKit filesystem adapter
    let fs = SquashboxFskitFs::new(provider, image_name);

    // 4. Configure mount options
    let opts = fskit_rs::MountOptions {
        // mount_point: mount_point.to_string_lossy().to_string(),
        // port: find_available_port(),
        ..Default::default()
    };

    // 5. Mount and serve (blocks until unmount signal)
    //    Internally: opens TCP socket, waits for FSKitBridge connection,
    //    then serves protobuf RPCs until unmount()
    fskit_rs::mount(fs, opts).await?;

    println!("Unmounted.");
    Ok(())
}
```

---

## Interaction Diagrams

### Windows: User opens a file

```
Explorer                    ProjFS (kernel)          windows-projfs        SquashboxProjFsSource     SquashFsProvider
   │                            │                        │                        │                       │
   │── CreateFile("foo.txt") ──►│                        │                        │                       │
   │                            │── GetPlaceholderInfo ─►│                        │                       │
   │                            │                        │── get_directory_entry ─►│                       │
   │                            │                        │                        │── resolve_path() ────►│
   │                            │                        │                        │◄── Ok(Some(inode)) ───│
   │                            │                        │                        │── get_attributes() ──►│
   │                            │                        │                        │◄── EntryAttributes ───│
   │                            │                        │◄── Some(Entry) ────────│                       │
   │                            │◄─ placeholder info ────│                        │                       │
   │                            │                        │                        │                       │
   │── ReadFile("foo.txt") ────►│                        │                        │                       │
   │                            │── GetFileData ────────►│                        │                       │
   │                            │                        │── stream_file_content ─►│                       │
   │                            │                        │                        │── read_file() ───────►│
   │                            │                        │                        │◄── Box<dyn Read> ─────│
   │                            │                        │◄── Box<dyn Read> ──────│                       │
   │                            │◄─ hydrated data ───────│                        │                       │
   │◄── file contents ─────────│                        │                        │                       │
```

### macOS: User opens a file

```
Application          VFS (kernel)        FSKit         FSKitBridge(Swift)     TCP/Protobuf       fskit-rs          SquashboxFskitFs      SquashFsProvider
   │                     │                 │                 │                     │                 │                    │                     │
   │── open("/foo") ────►│                 │                 │                     │                 │                    │                     │
   │                     │── lookup ──────►│                 │                     │                 │                    │                     │
   │                     │                 │── XPC ─────────►│                     │                 │                    │                     │
   │                     │                 │                 │── protobuf msg ────►│                 │                    │                     │
   │                     │                 │                 │                     │── dispatch ────►│                    │                     │
   │                     │                 │                 │                     │                 │── lookup_item() ──►│                     │
   │                     │                 │                 │                     │                 │                    │── lookup() ─────────►│
   │                     │                 │                 │                     │                 │                    │◄── DirEntry ─────────│
   │                     │                 │                 │                     │                 │◄── Item ───────────│                     │
   │                     │                 │                 │                     │◄── protobuf ────│                    │                     │
   │                     │                 │                 │◄── XPC response ────│                 │                    │                     │
   │                     │                 │◄────────────────│                     │                 │                    │                     │
   │                     │◄── vnode ───────│                 │                     │                 │                    │                     │
   │◄── fd ──────────────│                 │                 │                     │                 │                    │                     │
   │                     │                 │                 │                     │                 │                    │                     │
   │── read(fd, buf) ───►│                 │                 │                     │                 │                    │                     │
   │                     │── read ────────►│── XPC ─────────►│── protobuf ────────►│── dispatch ────►│── read() ─────────►│                     │
   │                     │                 │                 │                     │                 │                    │── read_file() ──────►│
   │                     │                 │                 │                     │                 │                    │◄── Vec<u8> ──────────│
   │                     │                 │                 │                     │                 │◄── Vec<u8> ────────│                     │
   │                     │                 │                 │                     │◄── protobuf ────│                    │                     │
   │◄── data ────────────│◄────────────────│◄────────────────│◄────────────────────│                 │                    │                     │
```

---

## Design Rationale

### Why `VirtualFsProvider` is synchronous

Both OS driver layers handle the async boundary themselves:
- **ProjFS**: Callbacks arrive on OS-managed threads. `windows-projfs` requires `Sync` on the source. Our provider is `Send + Sync` via `Arc`.
- **FSKit**: The `fskit-rs` `Filesystem` trait is `async_trait`. The async adapter wraps synchronous core calls using `tokio::task::spawn_blocking` if needed.

Keeping the core synchronous avoids forcing async onto the SquashFS reader, which does file I/O that doesn't benefit from async (it's CPU-bound decompression + sequential disk reads).

### Why path-based AND inode-based APIs exist on VirtualFsProvider

- **ProjFS** is path-based — every callback gives us a `&Path`. We need `resolve_path()` to translate.
- **FSKit** is inode-based — every callback gives us a `u64` item ID. We call `get_attributes()`, `list_directory()`, etc. directly with the inode.

Rather than forcing one model, we expose both and let each driver use what's natural. The `resolve_path()` method internally calls `lookup()` repeatedly, so there's no duplication of logic.

### Why write operations return `EROFS` / `Break`

SquashFS is inherently read-only. Rather than panicking or silently ignoring writes, we return the correct POSIX error (`EROFS` = read-only filesystem) on macOS and deny ProjFS notifications on Windows. This gives applications correct error handling.

### Why the core builds an InodeIndex at mount time

SquashFS stores directory data inline with inodes. Scanning the tree once at mount time and building a HashMap gives us O(1) lookups for any inode, which is critical because:
- ProjFS `list_directory` is called on every Explorer navigation
- FSKit `lookup_item` is called for every path component resolution
- Building the index is a one-time cost (~100ms for a 10GB image with 100K files)
