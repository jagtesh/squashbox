# Squashbox Architecture

This document defines the architecture through skeletal implementations — traits, structs,
method signatures, and their interactions with the parent OS libraries (`windows-projfs`
on Windows, FSKit + UniFFI on macOS).

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
│   │   │   ├── squashfs.rs      # backhand-backed implementation
│   │   │   └── ffi.rs           # UniFFI-exported functions for macOS
│   │   ├── src/squashbox_core.udl  # UniFFI interface definition
│   │   └── Cargo.toml           # crate-type = ["lib", "staticlib"]
│   │
│   ├── squashbox-windows/       # Windows ProjFS driver
│   │   ├── src/
│   │   │   ├── main.rs          # CLI entry point
│   │   │   └── projfs_source.rs # ProjectedFileSystemSource impl
│   │   └── Cargo.toml
│   │
│   ├── squashbox-macos/         # macOS FSKit driver (Swift appex)
│   │   ├── SquashboxFS/
│   │   │   ├── SquashboxFS.swift           # FSUnaryFileSystem subclass
│   │   │   ├── Info.plist
│   │   │   └── SquashboxFS.entitlements
│   │   ├── Generated/
│   │   │   ├── SquashboxCore.swift         # UniFFI-generated Swift bindings
│   │   │   └── SquashboxCoreFFI.h          # UniFFI-generated C header
│   │   └── SquashboxFS.xcodeproj
│   │
│   └── windows-projfs/          # Local fork (git subtree)
│
├── docs/
└── Cargo.toml                   # Workspace root
```

**Key change from previous architecture:** `fskit-rs` is removed entirely. The macOS
driver is a native Swift FSKit app extension that links the Rust `squashbox-core` crate
as a static library via UniFFI. No TCP, no Protobuf, no bridge process.

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
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum EntryType {
    File,
    Directory,
    Symlink,
}

/// Metadata for a single filesystem entry.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct EntryAttributes {
    pub inode: InodeId,
    pub entry_type: EntryType,
    pub size: u64,
    pub mode: u32,          // POSIX mode bits (e.g., 0o755)
    pub uid: u32,
    pub gid: u32,
    pub mtime_secs: i64,    // Unix timestamp (UniFFI-friendly, no SystemTime)
    pub atime_secs: i64,
    pub ctime_secs: i64,
    pub nlink: u32,
}

/// A single directory entry (name + attributes).
#[derive(Debug, Clone)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct DirEntry {
    pub name: String,
    pub attributes: EntryAttributes,
}

/// Extended attribute.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Xattr {
    pub name: String,
    pub value: Vec<u8>,
}

/// A paginated batch of directory entries.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct DirEntryBatch {
    pub entries: Vec<DirEntry>,
    pub next_cookie: u64,   // 0 = no more entries
}

/// Volume-level statistics.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct VolumeStats {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub total_inodes: u64,
    pub used_inodes: u64,
    pub block_size: u32,
}

/// Core error type.
#[derive(Debug, thiserror::Error)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Error))]
pub enum CoreError {
    #[error("entry not found: {0}")]
    NotFound(String),

    #[error("not a directory: inode {0}")]
    NotADirectory(InodeId),

    #[error("I/O error: {0}")]
    IoError(String),

    #[error("squashfs error: {0}")]
    SquashFs(String),

    #[error("operation not supported")]
    NotSupported,

    #[error("read-only filesystem")]
    ReadOnly,
}

pub type CoreResult<T> = Result<T, CoreError>;
```

> **Note on timestamps:** `SystemTime` is replaced with `i64` Unix timestamps.
> UniFFI cannot marshal `SystemTime` across FFI; Unix seconds are universally understood
> by both Rust and Swift (`Date(timeIntervalSince1970:)`).

> **Note on `Io` variant:** The `Io(#[from] std::io::Error)` variant is replaced with
> `IoError(String)` because `std::io::Error` is not FFI-safe. Conversion happens at
> the boundary.

---

### `provider.rs` — The VirtualFsProvider Trait

This is the **central abstraction**. Both platform drivers call into this trait.
It is intentionally synchronous — the async boundary lives in the driver layer.

```rust
use crate::types::*;
use std::path::Path;

/// Platform-agnostic virtual filesystem provider.
///
/// Implementors provide read-only access to a filesystem image.
/// The ProjFS driver calls these methods directly.
/// The macOS driver calls these methods via UniFFI-exported wrapper functions.
pub trait VirtualFsProvider: Send + Sync {
    // ── Path-based lookups (used primarily by ProjFS) ──

    /// Resolve a relative path to an inode ID.
    /// Returns None if the path does not exist.
    fn resolve_path(&self, path: &Path) -> CoreResult<Option<InodeId>>;

    // ── Inode-based operations (used by both drivers) ──

    /// Get attributes for a given inode.
    fn get_attributes(&self, inode: InodeId) -> CoreResult<EntryAttributes>;

    /// List directory entries with pagination.
    /// `cookie` is 0 for the first batch, then `next_cookie` from previous batch.
    fn list_directory(&self, inode: InodeId, cookie: u64) -> CoreResult<DirEntryBatch>;

    /// Look up a single entry by name within a directory.
    fn lookup(&self, parent_inode: InodeId, name: &str) -> CoreResult<Option<DirEntry>>;

    /// Read file content at a byte range.
    /// Returns the data as a byte vector.
    fn read_file(
        &self,
        inode: InodeId,
        offset: u64,
        length: u64,
    ) -> CoreResult<Vec<u8>>;

    /// Read the target of a symbolic link.
    fn read_symlink(&self, inode: InodeId) -> CoreResult<String>;

    // ── Extended attributes ──

    /// List xattr names for a given inode.
    fn list_xattrs(&self, inode: InodeId) -> CoreResult<Vec<String>>;

    /// Get the value of a specific xattr.
    fn get_xattr(&self, inode: InodeId, name: &str) -> CoreResult<Vec<u8>>;

    // ── Access control ──

    /// Check if access is allowed for the given mode bits.
    /// `mask` is a POSIX access mask (R_OK, W_OK, X_OK).
    fn check_access(&self, inode: InodeId, mask: u32) -> CoreResult<bool>;

    // ── Volume info ──

    /// Get filesystem-level statistics.
    fn volume_stats(&self) -> CoreResult<VolumeStats>;
}
```

> **Change from previous version:** `read_file()` returns `Vec<u8>` instead of
> `Box<dyn Read>`. This is necessary because UniFFI cannot marshal Rust trait objects
> across FFI. The `Vec<u8>` is the natural FFI-safe return type — Swift receives it
> as `Data`. For ProjFS, the Windows driver wraps the `Vec<u8>` in a `Cursor<Vec<u8>>`
> to satisfy the `Read` trait if needed.

---

### `squashfs.rs` — backhand Implementation

```rust
use crate::provider::VirtualFsProvider;
use crate::types::*;
use std::path::Path;

/// A SquashFS-backed implementation of VirtualFsProvider.
///
/// Thread-safe: multiple ProjFS callback threads or FSKit dispatch
/// queues can call into this concurrently.
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

    fn list_directory(&self, inode: InodeId, cookie: u64) -> CoreResult<DirEntryBatch> {
        // Verify inode is a directory
        // Collect all children from InodeIndex where parent == inode
        // Sort by name, paginate starting at cookie offset
        // Return DirEntryBatch { entries, next_cookie }
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
    ) -> CoreResult<Vec<u8>> {
        // 1. Get the backhand SquashfsFileReader for this inode
        // 2. Seek to offset (may need to decompress blocks up to that point)
        // 3. Read `length` bytes into Vec<u8>
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

    fn check_access(&self, inode: InodeId, mask: u32) -> CoreResult<bool> {
        // Read mode bits, compare against mask
        // For read-only FS, always deny W_OK
        todo!()
    }

    fn volume_stats(&self) -> CoreResult<VolumeStats> {
        // Read superblock for total size, inode count, block size
        todo!()
    }
}
```

---

### `ffi.rs` — UniFFI-Exported Functions (macOS boundary)

This module exposes the `VirtualFsProvider` interface as flat, FFI-safe functions
that UniFFI can generate Swift bindings for. The `SquashboxHandle` is an opaque
object that Swift holds a reference to.

```rust
use crate::provider::VirtualFsProvider;
use crate::squashfs::SquashFsProvider;
use crate::types::*;
use std::sync::Arc;

/// Opaque handle to an opened SquashFS image.
/// Swift holds an `Arc` to this via UniFFI's object support.
#[cfg_attr(feature = "uniffi", derive(uniffi::Object))]
pub struct SquashboxHandle {
    provider: Arc<SquashFsProvider>,
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
impl SquashboxHandle {
    /// Open a SquashFS image and return a handle.
    #[cfg_attr(feature = "uniffi", uniffi::constructor)]
    pub fn open(image_path: String) -> Result<Arc<Self>, CoreError> {
        let provider = SquashFsProvider::open(std::path::Path::new(&image_path))?;
        Ok(Arc::new(Self {
            provider: Arc::new(provider),
        }))
    }

    /// Get attributes for an inode.
    pub fn get_attributes(&self, inode_id: u64) -> Result<EntryAttributes, CoreError> {
        self.provider.get_attributes(inode_id)
    }

    /// Look up an entry by name within a parent directory.
    pub fn lookup(&self, parent_inode: u64, name: String) -> Result<Option<DirEntry>, CoreError> {
        self.provider.lookup(parent_inode, &name)
    }

    /// List directory entries with pagination.
    pub fn list_directory(&self, inode_id: u64, cookie: u64) -> Result<DirEntryBatch, CoreError> {
        self.provider.list_directory(inode_id, cookie)
    }

    /// Read file content at a byte range.
    pub fn read_file(&self, inode_id: u64, offset: u64, length: u64) -> Result<Vec<u8>, CoreError> {
        self.provider.read_file(inode_id, offset, length)
    }

    /// Read symlink target.
    pub fn read_symlink(&self, inode_id: u64) -> Result<String, CoreError> {
        self.provider.read_symlink(inode_id)
    }

    /// List xattr names.
    pub fn list_xattrs(&self, inode_id: u64) -> Result<Vec<String>, CoreError> {
        self.provider.list_xattrs(inode_id)
    }

    /// Get a specific xattr value.
    pub fn get_xattr(&self, inode_id: u64, name: String) -> Result<Vec<u8>, CoreError> {
        self.provider.get_xattr(inode_id, &name)
    }

    /// Check access permissions.
    pub fn check_access(&self, inode_id: u64, mask: u32) -> Result<bool, CoreError> {
        self.provider.check_access(inode_id, mask)
    }

    /// Get volume statistics.
    pub fn volume_stats(&self) -> Result<VolumeStats, CoreError> {
        self.provider.volume_stats()
    }

    /// Close the handle (explicit cleanup before Drop).
    pub fn close(&self) {
        // Explicit cleanup if needed; Arc handles the rest
    }
}
```

This generates Swift code that looks like:

```swift
// Generated by UniFFI — SquashboxCore.swift (DO NOT EDIT)

public class SquashboxHandle {
    public convenience init(imagePath: String) throws { ... }
    public func getAttributes(inodeId: UInt64) throws -> EntryAttributes { ... }
    public func lookup(parentInode: UInt64, name: String) throws -> DirEntry? { ... }
    public func listDirectory(inodeId: UInt64, cookie: UInt64) throws -> DirEntryBatch { ... }
    public func readFile(inodeId: UInt64, offset: UInt64, length: UInt64) throws -> Data { ... }
    public func readSymlink(inodeId: UInt64) throws -> String { ... }
    public func listXattrs(inodeId: UInt64) throws -> [String] { ... }
    public func getXattr(inodeId: UInt64, name: String) throws -> Data { ... }
    public func checkAccess(inodeId: UInt64, mask: UInt32) throws -> Bool { ... }
    public func volumeStats() throws -> VolumeStats { ... }
    public func close() { ... }
}

public struct EntryAttributes {
    public var inode: UInt64
    public var entryType: EntryType
    public var size: UInt64
    public var mode: UInt32
    public var uid: UInt32
    public var gid: UInt32
    public var mtimeSecs: Int64
    public var atimeSecs: Int64
    public var ctimeSecs: Int64
    public var nlink: UInt32
}

public enum EntryType { case file, directory, symlink }
public enum CoreError: Error { case notFound(String), notADirectory(UInt64), ... }
```

---

### `squashbox_core.udl` — UniFFI Interface Definition (alternative to proc-macros)

```webidl
// If using UDL instead of proc-macro attributes:

namespace squashbox_core {};

[Error]
enum CoreError {
    "NotFound",
    "NotADirectory",
    "IoError",
    "SquashFs",
    "NotSupported",
    "ReadOnly",
};

enum EntryType {
    "File",
    "Directory",
    "Symlink",
};

dictionary EntryAttributes {
    u64 inode;
    EntryType entry_type;
    u64 size;
    u32 mode;
    u32 uid;
    u32 gid;
    i64 mtime_secs;
    i64 atime_secs;
    i64 ctime_secs;
    u32 nlink;
};

dictionary DirEntry {
    string name;
    EntryAttributes attributes;
};

dictionary DirEntryBatch {
    sequence<DirEntry> entries;
    u64 next_cookie;
};

dictionary VolumeStats {
    u64 total_bytes;
    u64 used_bytes;
    u64 total_inodes;
    u64 used_inodes;
    u32 block_size;
};

interface SquashboxHandle {
    [Throws=CoreError]
    constructor(string image_path);

    [Throws=CoreError]
    EntryAttributes get_attributes(u64 inode_id);

    [Throws=CoreError]
    DirEntry? lookup(u64 parent_inode, string name);

    [Throws=CoreError]
    DirEntryBatch list_directory(u64 inode_id, u64 cookie);

    [Throws=CoreError]
    bytes read_file(u64 inode_id, u64 offset, u64 length);

    [Throws=CoreError]
    string read_symlink(u64 inode_id);

    [Throws=CoreError]
    sequence<string> list_xattrs(u64 inode_id);

    [Throws=CoreError]
    bytes get_xattr(u64 inode_id, string name);

    [Throws=CoreError]
    boolean check_access(u64 inode_id, u32 mask);

    [Throws=CoreError]
    VolumeStats volume_stats();

    void close();
};
```

---

## Layer 2a: Windows Driver (`squashbox-windows`)

### `projfs_source.rs` — Bridging VirtualFsProvider → windows-projfs

```rust
use squashbox_core::provider::VirtualFsProvider;
use squashbox_core::types::*;
use std::io::{Cursor, Read};
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

        // 2. List ALL directory contents (ProjFS expects full list)
        let mut all_entries = Vec::new();
        let mut cookie = 0u64;
        loop {
            match self.provider.list_directory(inode, cookie) {
                Ok(batch) => {
                    all_entries.extend(batch.entries);
                    if batch.next_cookie == 0 { break; }
                    cookie = batch.next_cookie;
                }
                Err(_) => return vec![],
            }
        }

        // 3. Map core DirEntry → windows_projfs DirectoryEntry
        all_entries
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
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::NotFound, e.to_string()))?
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "file not found")
            })?;

        // 2. Read data from core (returns Vec<u8>)
        let data = self
            .provider
            .read_file(inode, byte_offset as u64, length as u64)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        // 3. Wrap in Cursor to satisfy Box<dyn Read>
        Ok(Box::new(Cursor::new(data)))
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

### Architecture Overview

The macOS driver is a Swift FSKit app extension (`.appex`) that links the Rust
`squashbox-core` static library directly into its process space via UniFFI.

```
┌───────────────────────────────────────────────────────┐
│ SquashboxFS.appex  (FSKit App Extension)              │
│                                                       │
│  ┌──────────────────────┐    ┌──────────────────────┐ │
│  │ SquashboxFS.swift     │    │ libsquashbox_core.a   │ │
│  │                       │    │ (Rust static library) │ │
│  │ FSUnaryFileSystem     │    │                       │ │
│  │ subclass (~200 lines) │───►│ SquashboxHandle       │ │
│  │                       │ FFI│ (UniFFI object)       │ │
│  │ Maps FSKit callbacks  │◄───│                       │ │
│  │ to Rust core calls    │    │ VirtualFsProvider     │ │
│  └──────────────────────┘    │ SquashFsProvider      │ │
│                               │ backhand              │ │
│                               └──────────────────────┘ │
└───────────────────────────────────────────────────────┘
         ▲                              │
         │ XPC (managed by FSKit)       │ Direct file I/O
         ▼                              ▼
    macOS kernel                   .sqsh image file
    (VFS layer)
```

**Data flow for a read():**
```
Application → kernel VFS → FSKit XPC → Swift appex
    → FFI call into Rust static library (in-process, ~0.1μs)
    → backhand decompresses SquashFS block
    → returns Vec<u8> across FFI (zero-copy pointer handoff)
    → Swift writes to FSKit buffer → XPC → kernel → application
```

**What this eliminates vs. fskit-rs:**
- ❌ No TCP socket (was: 2 TCP hops per call)
- ❌ No Protobuf serialization (was: 2 ser/deser per call)
- ❌ No separate bridge process (was: FSKitBridge Swift project)
- ❌ No tokio runtime (was: async TCP server)
- ✅ Single process, single binary, direct function calls

---

### `SquashboxFS.swift` — FSKit App Extension

```swift
import FSKit

/// FSKit filesystem extension for SquashFS.
///
/// This is the entire macOS driver — a thin Swift shell that conforms to
/// FSUnaryFileSystem and delegates every operation to the Rust core via UniFFI.
class SquashboxFileSystem: FSUnaryFileSystem {

    /// Handle to the Rust core (opened SquashFS image)
    private var handle: SquashboxHandle?

    // MARK: - Lifecycle

    override func loadResource(
        resource: FSResource,
        options: FSTaskOptions,
        replyHandler: @escaping (FSItem?, Error?) -> Void
    ) {
        do {
            // Open the SquashFS image — this is a direct FFI call into Rust.
            // Rust parses the superblock, builds the inode index, returns a handle.
            let imagePath = resource.url.path
            self.handle = try SquashboxHandle(imagePath: imagePath)

            // Return the root directory item
            let rootAttrs = try handle!.getAttributes(inodeId: 1) // ROOT_INODE
            let rootItem = FSItem()
            rootItem.itemIdentifier = FSItemIdentifier(rawValue: 1)
            mapAttributes(from: rootAttrs, to: rootItem)
            replyHandler(rootItem, nil)
        } catch {
            replyHandler(nil, error.toNSError())
        }
    }

    override func didFinishLoading() {
        // Volume is now ready for I/O
    }

    override func unmount(replyHandler: @escaping (Error?) -> Void) {
        handle?.close()
        handle = nil
        replyHandler(nil)
    }

    // MARK: - Volume Info

    override var volumeName: String {
        return "SquashFS Volume"
    }

    override func volumeStatistics() throws -> FSStatFS {
        guard let handle = handle else { throw POSIXError(.EIO) }
        let stats = try handle.volumeStats()
        var result = FSStatFS()
        result.totalBlocks = stats.totalBytes / UInt64(stats.blockSize)
        result.freeBlocks = (stats.totalBytes - stats.usedBytes) / UInt64(stats.blockSize)
        result.blockSize = UInt32(stats.blockSize)
        result.totalFiles = stats.totalInodes
        result.freeFiles = stats.totalInodes - stats.usedInodes
        return result
    }

    // MARK: - Read Operations (the core of our FS)

    override func lookUp(
        name: String,
        inDirectory directory: FSItemIdentifier,
        replyHandler: @escaping (FSItem?, Error?) -> Void
    ) {
        do {
            guard let handle = handle else { throw POSIXError(.EIO) }
            guard let entry = try handle.lookup(
                parentInode: directory.rawValue,
                name: name
            ) else {
                throw POSIXError(.ENOENT)
            }

            let item = FSItem()
            item.itemIdentifier = FSItemIdentifier(rawValue: entry.attributes.inode)
            item.name = entry.name
            mapAttributes(from: entry.attributes, to: item)
            replyHandler(item, nil)
        } catch {
            replyHandler(nil, error.toNSError())
        }
    }

    override func getAttributes(
        of item: FSItemIdentifier,
        replyHandler: @escaping (FSItemAttributes?, Error?) -> Void
    ) {
        do {
            guard let handle = handle else { throw POSIXError(.EIO) }
            let attrs = try handle.getAttributes(inodeId: item.rawValue)
            let fsAttrs = FSItemAttributes()
            mapAttributes(from: attrs, to: fsAttrs)
            replyHandler(fsAttrs, nil)
        } catch {
            replyHandler(nil, error.toNSError())
        }
    }

    override func enumerateDirectory(
        identifier: FSItemIdentifier,
        startingAt cookie: UInt64,
        verifier: UInt64,
        provideItem: @escaping (FSDirectoryEntry) -> Bool,
        replyHandler: @escaping (UInt64, UInt64, Error?) -> Void
    ) {
        do {
            guard let handle = handle else { throw POSIXError(.EIO) }
            let batch = try handle.listDirectory(
                inodeId: identifier.rawValue,
                cookie: cookie
            )

            for entry in batch.entries {
                let dirEntry = FSDirectoryEntry()
                dirEntry.name = entry.name
                dirEntry.itemIdentifier = FSItemIdentifier(rawValue: entry.attributes.inode)
                dirEntry.itemType = mapEntryType(entry.attributes.entryType)

                if !provideItem(dirEntry) {
                    break  // Consumer is full
                }
            }

            replyHandler(batch.nextCookie, 0, nil)
        } catch {
            replyHandler(0, 0, error.toNSError())
        }
    }

    override func read(
        from item: FSItemIdentifier,
        offset: Int64,
        length: Int64,
        into buffer: FSMutableFileDataBuffer,
        replyHandler: @escaping (Int64, Error?) -> Void
    ) {
        do {
            guard let handle = handle else { throw POSIXError(.EIO) }

            // Direct FFI call — no serialization, no IPC.
            // Rust decompresses SquashFS blocks and returns bytes.
            let data = try handle.readFile(
                inodeId: item.rawValue,
                offset: UInt64(offset),
                length: UInt64(length)
            )

            // Copy into FSKit's provided buffer
            data.withUnsafeBytes { ptr in
                buffer.write(ptr, at: 0)
            }

            replyHandler(Int64(data.count), nil)
        } catch {
            replyHandler(0, error.toNSError())
        }
    }

    override func readSymbolicLink(
        of item: FSItemIdentifier,
        replyHandler: @escaping (String?, Error?) -> Void
    ) {
        do {
            guard let handle = handle else { throw POSIXError(.EIO) }
            let target = try handle.readSymlink(inodeId: item.rawValue)
            replyHandler(target, nil)
        } catch {
            replyHandler(nil, error.toNSError())
        }
    }

    // MARK: - Extended Attributes

    override func xattrNames(
        of item: FSItemIdentifier,
        replyHandler: @escaping ([String]?, Error?) -> Void
    ) {
        do {
            guard let handle = handle else { throw POSIXError(.EIO) }
            let names = try handle.listXattrs(inodeId: item.rawValue)
            replyHandler(names, nil)
        } catch {
            replyHandler(nil, error.toNSError())
        }
    }

    override func getXattr(
        named name: String,
        of item: FSItemIdentifier,
        replyHandler: @escaping (Data?, Error?) -> Void
    ) {
        do {
            guard let handle = handle else { throw POSIXError(.EIO) }
            let value = try handle.getXattr(inodeId: item.rawValue, name: name)
            replyHandler(value, nil)
        } catch {
            replyHandler(nil, error.toNSError())
        }
    }

    // MARK: - Access Control

    override func checkAccess(
        to item: FSItemIdentifier,
        operations: FSAccessMask,
        replyHandler: @escaping (Bool, Error?) -> Void
    ) {
        do {
            guard let handle = handle else { throw POSIXError(.EIO) }
            let allowed = try handle.checkAccess(
                inodeId: item.rawValue,
                mask: operations.rawValue
            )
            replyHandler(allowed, nil)
        } catch {
            replyHandler(false, error.toNSError())
        }
    }

    // MARK: - Write Operations (all denied — read-only FS)
    //
    // These methods do NOT call into Rust at all.
    // They return EROFS immediately in Swift.

    override func createItem(
        named name: String, type: FSItemType,
        inDirectory directory: FSItemIdentifier,
        attributes: FSItemAttributes,
        replyHandler: @escaping (FSItem?, Error?) -> Void
    ) {
        replyHandler(nil, POSIXError(.EROFS))
    }

    override func write(
        contents: Data, to item: FSItemIdentifier,
        at offset: Int64,
        replyHandler: @escaping (Int64, Error?) -> Void
    ) {
        replyHandler(0, POSIXError(.EROFS))
    }

    override func removeItem(
        named name: String,
        inDirectory directory: FSItemIdentifier,
        replyHandler: @escaping (Error?) -> Void
    ) {
        replyHandler(POSIXError(.EROFS))
    }

    override func rename(
        item: FSItemIdentifier, inDirectory: FSItemIdentifier,
        named: String, to: FSItemIdentifier,
        newName: String,
        replyHandler: @escaping (Error?) -> Void
    ) {
        replyHandler(POSIXError(.EROFS))
    }

    override func setXattr(
        named name: String, of item: FSItemIdentifier,
        value: Data?, policy: FSXattrPolicy,
        replyHandler: @escaping (Error?) -> Void
    ) {
        replyHandler(POSIXError(.EROFS))
    }

    override func setAttributes(
        _ attributes: FSItemAttributes,
        of item: FSItemIdentifier,
        replyHandler: @escaping (FSItemAttributes?, Error?) -> Void
    ) {
        replyHandler(nil, POSIXError(.EROFS))
    }

    // MARK: - Helpers

    private func mapAttributes(from attrs: EntryAttributes, to item: FSItem) {
        item.size = attrs.size
        item.mode = mode_t(attrs.mode)
        item.uid = attrs.uid
        item.gid = attrs.gid
        item.linkCount = UInt32(attrs.nlink)
        item.modificationDate = Date(timeIntervalSince1970: TimeInterval(attrs.mtimeSecs))
        item.accessDate = Date(timeIntervalSince1970: TimeInterval(attrs.atimeSecs))
        item.changeDate = Date(timeIntervalSince1970: TimeInterval(attrs.ctimeSecs))
    }

    private func mapAttributes(from attrs: EntryAttributes, to fsAttrs: FSItemAttributes) {
        fsAttrs.size = attrs.size
        fsAttrs.mode = mode_t(attrs.mode)
        fsAttrs.uid = attrs.uid
        fsAttrs.gid = attrs.gid
        fsAttrs.linkCount = UInt32(attrs.nlink)
    }

    private func mapEntryType(_ type: EntryType) -> FSItemType {
        switch type {
        case .file:      return .regular
        case .directory: return .directory
        case .symlink:   return .symbolicLink
        }
    }
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
   │                            │                        │                        │◄── Vec<u8> ────────── │
   │                            │                        │◄── Cursor<Vec<u8>> ────│                       │
   │                            │◄─ hydrated data ───────│                        │                       │
   │◄── file contents ─────────│                        │                        │                       │
```

### macOS: User opens a file

```
Application         VFS (kernel)       FSKit           SquashboxFS.swift       SquashboxHandle (Rust)
   │                     │                │                  │                        │
   │── open("/foo") ────►│                │                  │                        │
   │                     │── lookup ─────►│                  │                        │
   │                     │                │── XPC ──────────►│                        │
   │                     │                │                  │── handle.lookup() ────►│ (FFI call, ~0.1μs)
   │                     │                │                  │◄── DirEntry ───────────│
   │                     │                │                  │── map to FSItem ───────│
   │                     │                │◄── FSItem ───────│                        │
   │                     │◄── vnode ──────│                  │                        │
   │◄── fd ──────────────│                │                  │                        │
   │                     │                │                  │                        │
   │── read(fd, buf) ───►│                │                  │                        │
   │                     │── read ───────►│── XPC ──────────►│                        │
   │                     │                │                  │── handle.readFile() ──►│ (FFI call, ~0.1μs)
   │                     │                │                  │◄── Vec<u8> / Data ─────│
   │                     │                │                  │── write to buffer ─────│
   │                     │                │◄── data ─────────│                        │
   │◄── data ────────────│◄───────────────│                  │                        │
```

> **Compare with previous fskit-rs diagram:** Gone are the `FSKitBridge`, `TCP/Protobuf`,
> and `fskit-rs` columns. The data path is now 5 hops instead of 10. The critical
> `handle.readFile()` call is a direct C-ABI function call within the same process.

---

## Design Rationale

### Why `VirtualFsProvider` is synchronous

Both OS driver layers handle the concurrency boundary themselves:
- **ProjFS**: Callbacks arrive on OS-managed threads. `windows-projfs` requires `Sync` on the source. Our provider is `Send + Sync` via `Arc`.
- **FSKit**: The Swift `FSUnaryFileSystem` callbacks arrive on FSKit-managed dispatch queues. The UniFFI call into Rust is a blocking C-ABI call. Since `SquashFsProvider` is `Send + Sync`, multiple concurrent FSKit callbacks are safe.

Keeping the core synchronous avoids forcing async onto the SquashFS reader, which does file I/O that doesn't benefit from async (it's CPU-bound decompression + sequential disk reads).

### Why UniFFI instead of fskit-rs

The previous architecture used `fskit-rs`, which communicates with a Swift FSKitBridge
over localhost TCP using Protobuf serialization. This added:
- **~40-100μs per call** of IPC overhead (TCP + Protobuf ser/deser)
- A separate bridge process to manage
- A tokio async runtime for the TCP server
- Full Protobuf serialization of file data (pure waste for `read()` calls)

UniFFI eliminates all of this. The Rust core compiles as a static library that links
directly into the Swift app extension. FFI calls are **in-process C-ABI calls** with
**~0.1μs overhead** — 100-1000× faster than the TCP path. For a filesystem driver
where every `read()` must cross the boundary, this difference is critical.

### Why `read_file()` returns `Vec<u8>` instead of `Box<dyn Read>`

UniFFI cannot marshal Rust trait objects across FFI boundaries. `Vec<u8>` is the natural
FFI-safe type — UniFFI maps it to Swift `Data`, which is what FSKit expects anyway.
On the Windows side, `Vec<u8>` is wrapped in `Cursor<Vec<u8>>` to produce the
`Box<dyn Read>` that `windows-projfs` expects — a trivial adapter.

### Why path-based AND inode-based APIs exist on VirtualFsProvider

- **ProjFS** is path-based — every callback gives us a `&Path`. We need `resolve_path()` to translate.
- **FSKit** is inode-based — every callback gives us a `u64` item ID. We call `get_attributes()`, `list_directory()`, etc. directly with the inode.

Rather than forcing one model, we expose both and let each driver use what's natural. The `resolve_path()` method internally calls `lookup()` repeatedly, so there's no duplication of logic.

### Why write operations return `EROFS` / `Break`

SquashFS is inherently read-only. Rather than panicking or silently ignoring writes, we return the correct POSIX error (`EROFS` = read-only filesystem) on macOS and deny ProjFS notifications on Windows. This gives applications correct error handling.

On macOS, write operations don't even cross the FFI boundary — they return `EROFS`
immediately in Swift, avoiding any unnecessary Rust calls.

### Why the core builds an InodeIndex at mount time

SquashFS stores directory data inline with inodes. Scanning the tree once at mount time and building a HashMap gives us O(1) lookups for any inode, which is critical because:
- ProjFS `list_directory` is called on every Explorer navigation
- FSKit `lookUp` is called for every path component resolution
- Building the index is a one-time cost (~100ms for a 10GB image with 100K files)

### Build pipeline for macOS

```
1. cargo build --target aarch64-apple-darwin --release
   → produces libsquashbox_core.a

2. cargo run --bin uniffi-bindgen generate src/squashbox_core.udl --language swift
   → produces SquashboxCore.swift + SquashboxCoreFFI.h + module.modulemap

3. Xcode project links libsquashbox_core.a into SquashboxFS.appex target
   → Bridging header imports SquashboxCoreFFI.h
   → Swift code imports SquashboxCore module

4. xcodebuild builds the .appex bundle
   → Contains: Swift code + Rust static library, single binary
```
