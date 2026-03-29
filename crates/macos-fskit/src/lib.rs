//! macos-fskit: Rust bindings for Apple's FSKit framework.
//!
//! This crate wraps FSKit's ObjC APIs using `objc2` and exposes a Rust-native
//! trait (`FsKitFileSystemSource`) for implementing filesystem extensions.
//!
//! This is the macOS equivalent of the `windows-projfs` crate — it provides
//! the platform abstraction layer that a Squashbox driver implements against.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────┐
//! │  Your code                      │
//! │  impl FsKitFileSystemSource     │
//! └───────────┬─────────────────────┘
//!             │ Rust trait
//! ┌───────────▼─────────────────────┐
//! │  macos-fskit runtime            │
//! │  ObjC class registration        │
//! │  (define_class! subclass of     │
//! │   FSUnaryFileSystem / FSVolume) │
//! └───────────┬─────────────────────┘
//!             │ objc2 FFI
//! ┌───────────▼─────────────────────┐
//! │  FSKit.framework (macOS)        │
//! │  Kernel VFS ↔ User-space XPC    │
//! └─────────────────────────────────┘
//! ```

pub mod objc_bindings;
pub mod runtime;
pub mod types;

pub use types::*;

/// The core trait for implementing an FSKit filesystem in Rust.
///
/// This is the macOS equivalent of `windows_projfs::ProjectedFileSystemSource`.
/// Implement this trait to provide filesystem operations, then pass your
/// implementation to the FSKit runtime for registration.
///
/// # Thread Safety
///
/// Implementations must be `Send + Sync` — FSKit dispatches callbacks on
/// multiple dispatch queues concurrently.
///
/// # Inode Numbering
///
/// FSKit reserves inode IDs 0-2:
/// - `0` = invalid
/// - `1` = parent of root
/// - `2` = root directory
///
/// Your implementation must use `2` as the root directory ID. The adapter
/// layer in `squashbox-macos` handles translation between FSKit's inode
/// numbering and squashbox-core's `ROOT_INODE = 1`.
pub trait FsKitFileSystemSource: Send + Sync {
    /// Probe whether this source can handle the given resource.
    ///
    /// Called when the system wants to determine if this filesystem
    /// recognizes the given image/resource. Return `Ok(true)` if
    /// the resource is a valid SquashFS image.
    fn probe(&self, resource_path: &str) -> Result<bool, FsKitError>;

    /// Load a resource (e.g., a SquashFS image file).
    ///
    /// Opens the image, parses the superblock, builds the inode index.
    /// Returns volume metadata on success.
    fn load_resource(&self, resource_path: &str) -> Result<VolumeInfo, FsKitError>;

    /// Activate the volume.
    ///
    /// Called after `load_resource` but before any filesystem operations.
    /// Returns attributes of the root directory item.
    fn activate(&self) -> Result<ItemAttributes, FsKitError>;

    /// Look up an entry by name within a parent directory.
    ///
    /// Returns `(item_id, name_as_stored, attributes)` on success.
    /// Returns `Err(FsKitError::NotFound)` if the name doesn't exist.
    fn lookup(
        &self,
        parent_id: u64,
        name: &str,
    ) -> Result<(u64, String, ItemAttributes), FsKitError>;

    /// Enumerate directory contents.
    ///
    /// Returns a batch of entries starting from `cookie`.
    /// Use `COOKIE_INITIAL` (0) for the first call.
    fn enumerate_directory(
        &self,
        item_id: u64,
        cookie: u64,
    ) -> Result<Vec<DirEntry>, FsKitError>;

    /// Get attributes for an item.
    fn get_attributes(&self, item_id: u64) -> Result<ItemAttributes, FsKitError>;

    /// Read file data.
    ///
    /// Returns the bytes read from `[offset..offset+length]`.
    fn read_file(
        &self,
        item_id: u64,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, FsKitError>;

    /// Read symbolic link target.
    fn read_symlink(&self, item_id: u64) -> Result<String, FsKitError>;

    /// Get volume statistics.
    fn volume_statistics(&self) -> Result<StatFSResult, FsKitError>;

    /// List extended attribute names for an item.
    fn list_xattrs(&self, item_id: u64) -> Result<Vec<String>, FsKitError>;

    /// Get the value of a specific extended attribute.
    fn get_xattr(&self, item_id: u64, name: &str) -> Result<Vec<u8>, FsKitError>;

    /// Check access permissions.
    ///
    /// `mask` uses the constants from the `access` module.
    fn check_access(&self, item_id: u64, mask: u32) -> Result<bool, FsKitError>;

    /// Reclaim an item (release resources).
    ///
    /// Called when FSKit no longer needs the item. Default implementation
    /// is a no-op, which is fine for read-only filesystems.
    fn reclaim(&self, _item_id: u64) -> Result<(), FsKitError> {
        Ok(())
    }

    /// Unload the resource / close the image.
    fn unload(&self);
}
