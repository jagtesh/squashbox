//! FSKit source adapter: bridges `VirtualFsProvider` to `FsKitFileSystemSource`.
//!
//! This is the macOS equivalent of `projfs_source.rs`. It adapts the
//! platform-agnostic `VirtualFsProvider` into the FSKit-specific
//! `FsKitFileSystemSource` trait.
//!
//! # Inode Translation
//!
//! SquashFS (via squashbox-core) uses `ROOT_INODE = 1`.
//! FSKit uses `FSItemIDRootDirectory = 2` (with 0 = invalid, 1 = parent-of-root).
//!
//! This adapter translates between the two numbering schemes:
//! - FSKit inode 2 → core inode 1 (root)
//! - All other FSKit inodes → core inodes offset by +1
//! - Core inodes → FSKit inodes offset by -1 (reverse mapping)

use macos_fskit::types::*;
use macos_fskit::FsKitFileSystemSource;
use squashbox_core::provider::VirtualFsProvider;
use squashbox_core::types::*;
use std::sync::{Arc, Mutex};

/// Adapts a `VirtualFsProvider` into an `FsKitFileSystemSource`.
///
/// This struct holds an `Arc` reference to the core provider and translates
/// between the inode-based FSKit callbacks and our core provider.
///
/// # Thread Safety
///
/// `FsKitFileSystemSource` requires `Send + Sync`. Our provider is
/// `Send + Sync` via `Arc`, so concurrent FSKit dispatch queue access is safe.
pub struct SquashboxFsKitSource {
    /// The provider is set during `load_resource()`.
    provider: Mutex<Option<Arc<dyn VirtualFsProvider>>>,
    /// Path to the image, remembered from `load_resource()`.
    image_path: Mutex<Option<String>>,
}

impl SquashboxFsKitSource {
    /// Create a new FSKit source (provider not yet loaded).
    pub fn new() -> Self {
        Self {
            provider: Mutex::new(None),
            image_path: Mutex::new(None),
        }
    }

    /// Create a new FSKit source with a pre-loaded provider.
    ///
    /// This is useful for testing or when the provider is opened externally
    /// (e.g., by the CLI's `image` command).
    pub fn with_provider(provider: Arc<dyn VirtualFsProvider>) -> Self {
        Self {
            provider: Mutex::new(Some(provider)),
            image_path: Mutex::new(None),
        }
    }

    /// Get a reference to the underlying provider (for testing).
    pub fn provider(&self) -> Option<Arc<dyn VirtualFsProvider>> {
        self.provider.lock().unwrap().clone()
    }

    // ── Inode translation ──

    /// Convert a core inode ID to an FSKit inode ID.
    ///
    /// Core `ROOT_INODE` (1) → FSKit `ROOT_DIRECTORY` (2).
    /// All others: core_inode + 1.
    fn core_to_fskit_inode(core_inode: InodeId) -> u64 {
        core_inode + 1
    }

    /// Convert an FSKit inode ID to a core inode ID.
    ///
    /// FSKit `ROOT_DIRECTORY` (2) → Core `ROOT_INODE` (1).
    /// All others: fskit_inode - 1.
    fn fskit_to_core_inode(fskit_inode: u64) -> InodeId {
        if fskit_inode <= 1 {
            // 0 = invalid, 1 = parent-of-root → treat as root
            ROOT_INODE
        } else {
            fskit_inode - 1
        }
    }

    /// Convert a core `EntryType` to an FSKit `ItemType`.
    fn entry_type_to_item_type(et: EntryType) -> ItemType {
        match et {
            EntryType::File => ItemType::File,
            EntryType::Directory => ItemType::Directory,
            EntryType::Symlink => ItemType::Symlink,
            EntryType::CharDevice => ItemType::CharDevice,
            EntryType::BlockDevice => ItemType::BlockDevice,
        }
    }

    /// Convert core `EntryAttributes` to FSKit `ItemAttributes`.
    fn core_attrs_to_fskit(
        attrs: &EntryAttributes,
        parent_inode: Option<InodeId>,
    ) -> ItemAttributes {
        ItemAttributes {
            item_type: Self::entry_type_to_item_type(attrs.entry_type),
            mode: attrs.mode,
            uid: attrs.uid,
            gid: attrs.gid,
            link_count: attrs.nlink,
            size: attrs.size,
            alloc_size: attrs.size, // SquashFS: allocated = decompressed
            file_id: Self::core_to_fskit_inode(attrs.inode),
            parent_id: parent_inode
                .map(Self::core_to_fskit_inode)
                .unwrap_or(item_id::PARENT_OF_ROOT),
            mtime: Timespec::from_secs(attrs.mtime_secs),
            atime: Timespec::from_secs(attrs.mtime_secs), // SquashFS only has mtime
            ctime: Timespec::from_secs(attrs.mtime_secs),
            btime: Timespec::from_secs(attrs.mtime_secs),
        }
    }

    /// Convert a `CoreError` to an `FsKitError`.
    fn core_error_to_fskit(err: CoreError) -> FsKitError {
        match err {
            CoreError::NotFound(msg) => FsKitError::NotFound(msg),
            CoreError::NotADirectory(inode) => {
                FsKitError::NotADirectory(Self::core_to_fskit_inode(inode))
            }
            CoreError::NotAFile(inode) => {
                FsKitError::NotAFile(Self::core_to_fskit_inode(inode))
            }
            CoreError::NotASymlink(inode) => {
                FsKitError::NotAFile(Self::core_to_fskit_inode(inode))
            }
            CoreError::Io(msg) => FsKitError::Io(msg),
            CoreError::SquashFs(msg) => FsKitError::Io(format!("squashfs: {}", msg)),
            CoreError::NotSupported => FsKitError::Posix(libc::ENOTSUP, "not supported".into()),
            CoreError::ReadOnly => FsKitError::ReadOnly,
        }
    }

    /// Get the provider, returning an error if not loaded.
    fn require_provider(&self) -> Result<Arc<dyn VirtualFsProvider>, FsKitError> {
        self.provider
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| FsKitError::Internal("Provider not loaded".into()))
    }
}

impl FsKitFileSystemSource for SquashboxFsKitSource {
    fn probe(&self, resource_path: &str) -> Result<bool, FsKitError> {
        // Try to open the image briefly to validate it's a SquashFS file.
        let path = std::path::PathBuf::from(resource_path);
        match squashbox_core::SquashFsProvider::open(&path) {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    fn load_resource(&self, resource_path: &str) -> Result<VolumeInfo, FsKitError> {
        let path = std::path::PathBuf::from(resource_path);

        log::info!("Loading SquashFS image: {}", resource_path);
        let provider = squashbox_core::SquashFsProvider::open(&path)
            .map_err(|e| FsKitError::Io(format!("Failed to open image: {}", e)))?;

        let stats = provider
            .volume_stats()
            .map_err(Self::core_error_to_fskit)?;

        log::info!(
            "Image loaded: {} inodes, {} bytes",
            stats.total_inodes,
            stats.total_bytes
        );

        *self.provider.lock().unwrap() = Some(Arc::new(provider));
        *self.image_path.lock().unwrap() = Some(resource_path.to_string());

        Ok(VolumeInfo {
            // Use a simple hash of the image path as a volume ID
            volume_id: {
                let mut id = [0u8; 16];
                let hash = resource_path.as_bytes();
                for (i, &b) in hash.iter().take(16).enumerate() {
                    id[i] = b;
                }
                id
            },
            volume_name: path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("SquashFS")
                .to_string(),
        })
    }

    fn activate(&self) -> Result<ItemAttributes, FsKitError> {
        let provider = self.require_provider()?;
        let root_attrs = provider
            .get_attributes(ROOT_INODE)
            .map_err(Self::core_error_to_fskit)?;

        Ok(Self::core_attrs_to_fskit(&root_attrs, None))
    }

    fn lookup(
        &self,
        parent_id: u64,
        name: &str,
    ) -> Result<(u64, String, ItemAttributes), FsKitError> {
        let provider = self.require_provider()?;
        let core_parent = Self::fskit_to_core_inode(parent_id);

        match provider
            .lookup(core_parent, name)
            .map_err(Self::core_error_to_fskit)?
        {
            Some(entry) => {
                let fskit_inode = Self::core_to_fskit_inode(entry.attributes.inode);
                let attrs =
                    Self::core_attrs_to_fskit(&entry.attributes, Some(core_parent));
                Ok((fskit_inode, entry.name, attrs))
            }
            None => Err(FsKitError::NotFound(format!(
                "{} in parent {}",
                name, parent_id
            ))),
        }
    }

    fn enumerate_directory(
        &self,
        item_id: u64,
        cookie: u64,
    ) -> Result<Vec<macos_fskit::DirEntry>, FsKitError> {
        let provider = self.require_provider()?;
        let core_inode = Self::fskit_to_core_inode(item_id);

        let batch = provider
            .list_directory(core_inode, cookie)
            .map_err(Self::core_error_to_fskit)?;

        let entries = batch
            .entries
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let next_cookie = if i + 1 < batch.entries.len() {
                    // Use the index within the batch as an intermediate cookie
                    cookie + (i as u64) + 1
                } else {
                    batch.next_cookie
                };

                macos_fskit::DirEntry {
                    name: entry.name.clone(),
                    item_type: Self::entry_type_to_item_type(entry.attributes.entry_type),
                    item_id: Self::core_to_fskit_inode(entry.attributes.inode),
                    next_cookie,
                    attributes: Some(Self::core_attrs_to_fskit(
                        &entry.attributes,
                        Some(core_inode),
                    )),
                }
            })
            .collect();

        Ok(entries)
    }

    fn get_attributes(&self, item_id: u64) -> Result<ItemAttributes, FsKitError> {
        let provider = self.require_provider()?;
        let core_inode = Self::fskit_to_core_inode(item_id);

        let attrs = provider
            .get_attributes(core_inode)
            .map_err(Self::core_error_to_fskit)?;

        Ok(Self::core_attrs_to_fskit(&attrs, None))
    }

    fn read_file(
        &self,
        item_id: u64,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, FsKitError> {
        let provider = self.require_provider()?;
        let core_inode = Self::fskit_to_core_inode(item_id);

        provider
            .read_file(core_inode, offset, length)
            .map_err(Self::core_error_to_fskit)
    }

    fn read_symlink(&self, item_id: u64) -> Result<String, FsKitError> {
        let provider = self.require_provider()?;
        let core_inode = Self::fskit_to_core_inode(item_id);

        provider
            .read_symlink(core_inode)
            .map_err(Self::core_error_to_fskit)
    }

    fn volume_statistics(&self) -> Result<StatFSResult, FsKitError> {
        let provider = self.require_provider()?;

        let stats = provider.volume_stats().map_err(Self::core_error_to_fskit)?;

        Ok(StatFSResult {
            block_size: stats.block_size as i64,
            io_size: stats.block_size as i64,
            total_blocks: stats.total_bytes / stats.block_size as u64,
            available_blocks: 0,
            free_blocks: 0,
            used_blocks: stats.used_bytes / stats.block_size as u64,
            total_bytes: stats.total_bytes,
            available_bytes: 0,
            free_bytes: 0,
            used_bytes: stats.used_bytes,
            total_files: stats.total_inodes,
            free_files: 0,
            fs_type_name: "squashfs".to_string(),
        })
    }

    fn list_xattrs(&self, item_id: u64) -> Result<Vec<String>, FsKitError> {
        let provider = self.require_provider()?;
        let core_inode = Self::fskit_to_core_inode(item_id);

        provider
            .list_xattrs(core_inode)
            .map_err(Self::core_error_to_fskit)
    }

    fn get_xattr(&self, item_id: u64, name: &str) -> Result<Vec<u8>, FsKitError> {
        let provider = self.require_provider()?;
        let core_inode = Self::fskit_to_core_inode(item_id);

        provider
            .get_xattr(core_inode, name)
            .map_err(Self::core_error_to_fskit)
    }

    fn check_access(&self, item_id: u64, mask: u32) -> Result<bool, FsKitError> {
        let provider = self.require_provider()?;
        let core_inode = Self::fskit_to_core_inode(item_id);

        // Deny any write access (EROFS)
        if mask & (macos_fskit::access::WRITE_DATA
            | macos_fskit::access::WRITE_ATTRIBUTES
            | macos_fskit::access::WRITE_XATTR
            | macos_fskit::access::WRITE_SECURITY
            | macos_fskit::access::DELETE
            | macos_fskit::access::APPEND_DATA)
            != 0
        {
            return Ok(false);
        }

        provider
            .check_access(core_inode, mask)
            .map_err(Self::core_error_to_fskit)
    }

    fn reclaim(&self, _item_id: u64) -> Result<(), FsKitError> {
        // Read-only FS: nothing to clean up per-item
        Ok(())
    }

    fn unload(&self) {
        log::info!("Unloading SquashFS image");
        *self.provider.lock().unwrap() = None;
        *self.image_path.lock().unwrap() = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inode_translation_roundtrip() {
        // Core ROOT_INODE (1) → FSKit 2
        assert_eq!(SquashboxFsKitSource::core_to_fskit_inode(1), 2);
        // FSKit 2 → Core 1
        assert_eq!(SquashboxFsKitSource::fskit_to_core_inode(2), 1);

        // Arbitrary inode roundtrip
        for i in 1..100 {
            let fskit = SquashboxFsKitSource::core_to_fskit_inode(i);
            let core = SquashboxFsKitSource::fskit_to_core_inode(fskit);
            assert_eq!(core, i, "Roundtrip failed for core inode {}", i);
        }
    }

    #[test]
    fn test_fskit_special_inodes() {
        // FSKit 0 (invalid) → maps to root as fallback
        assert_eq!(SquashboxFsKitSource::fskit_to_core_inode(0), ROOT_INODE);
        // FSKit 1 (parent of root) → maps to root
        assert_eq!(SquashboxFsKitSource::fskit_to_core_inode(1), ROOT_INODE);
    }

    #[test]
    fn test_entry_type_mapping() {
        assert_eq!(
            SquashboxFsKitSource::entry_type_to_item_type(EntryType::File),
            macos_fskit::ItemType::File
        );
        assert_eq!(
            SquashboxFsKitSource::entry_type_to_item_type(EntryType::Directory),
            macos_fskit::ItemType::Directory
        );
        assert_eq!(
            SquashboxFsKitSource::entry_type_to_item_type(EntryType::Symlink),
            macos_fskit::ItemType::Symlink
        );
    }
}
