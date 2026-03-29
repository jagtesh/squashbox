//! UniFFI bindings for squashbox-core.
//!
//! This crate wraps the `squashbox_core::SquashFsProvider` with UniFFI
//! annotations so that `uniffi-bindgen` can generate Swift (and other
//! language) bindings.

use squashbox_core::{self as core, VirtualFsProvider};
use std::path::Path;
use std::sync::Arc;

// ── Error type ──────────────────────────────────────────────────────

/// FFI-safe error type matching the UDL `CoreError` enum.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CoreError {
    #[error("not found: {msg}")]
    NotFound { msg: String },
    #[error("not a directory: inode {inode}")]
    NotADirectory { inode: u64 },
    #[error("not a file: inode {inode}")]
    NotAFile { inode: u64 },
    #[error("not a symlink: inode {inode}")]
    NotASymlink { inode: u64 },
    #[error("I/O error: {msg}")]
    Io { msg: String },
    #[error("squashfs error: {msg}")]
    SquashFs { msg: String },
    #[error("operation not supported")]
    NotSupported,
    #[error("read-only filesystem")]
    ReadOnly,
}

impl From<core::CoreError> for CoreError {
    fn from(e: core::CoreError) -> Self {
        match e {
            core::CoreError::NotFound(s) => CoreError::NotFound { msg: s },
            core::CoreError::NotADirectory(id) => CoreError::NotADirectory { inode: id },
            core::CoreError::NotAFile(id) => CoreError::NotAFile { inode: id },
            core::CoreError::NotASymlink(id) => CoreError::NotASymlink { inode: id },
            core::CoreError::Io(s) => CoreError::Io { msg: s },
            core::CoreError::SquashFs(s) => CoreError::SquashFs { msg: s },
            core::CoreError::NotSupported => CoreError::NotSupported,
            core::CoreError::ReadOnly => CoreError::ReadOnly,
        }
    }
}

// ── Value types ─────────────────────────────────────────────────────

#[derive(Debug, Clone, uniffi::Enum)]
pub enum EntryType {
    File,
    Directory,
    Symlink,
    BlockDevice,
    CharDevice,
}

impl From<core::EntryType> for EntryType {
    fn from(et: core::EntryType) -> Self {
        match et {
            core::EntryType::File => EntryType::File,
            core::EntryType::Directory => EntryType::Directory,
            core::EntryType::Symlink => EntryType::Symlink,
            core::EntryType::BlockDevice => EntryType::BlockDevice,
            core::EntryType::CharDevice => EntryType::CharDevice,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct EntryAttributes {
    pub inode: u64,
    pub entry_type: EntryType,
    pub size: u64,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub mtime_secs: i64,
    pub nlink: u32,
}

impl From<core::EntryAttributes> for EntryAttributes {
    fn from(a: core::EntryAttributes) -> Self {
        EntryAttributes {
            inode: a.inode,
            entry_type: a.entry_type.into(),
            size: a.size,
            mode: a.mode,
            uid: a.uid,
            gid: a.gid,
            mtime_secs: a.mtime_secs,
            nlink: a.nlink,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct DirEntry {
    pub name: String,
    pub attributes: EntryAttributes,
}

impl From<core::DirEntry> for DirEntry {
    fn from(e: core::DirEntry) -> Self {
        DirEntry {
            name: e.name,
            attributes: e.attributes.into(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct DirEntryBatch {
    pub entries: Vec<DirEntry>,
    pub next_cookie: u64,
}

impl From<core::DirEntryBatch> for DirEntryBatch {
    fn from(b: core::DirEntryBatch) -> Self {
        DirEntryBatch {
            entries: b.entries.into_iter().map(Into::into).collect(),
            next_cookie: b.next_cookie,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct VolumeStats {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub total_inodes: u64,
    pub used_inodes: u64,
    pub block_size: u32,
}

impl From<core::VolumeStats> for VolumeStats {
    fn from(s: core::VolumeStats) -> Self {
        VolumeStats {
            total_bytes: s.total_bytes,
            used_bytes: s.used_bytes,
            total_inodes: s.total_inodes,
            used_inodes: s.used_inodes,
            block_size: s.block_size,
        }
    }
}

// ── Provider object ─────────────────────────────────────────────────

/// FFI-exported SquashFS provider. Wraps `squashbox_core::SquashFsProvider`.
#[derive(uniffi::Object)]
pub struct SquashFsProvider {
    inner: Arc<core::SquashFsProvider>,
}

#[uniffi::export]
impl SquashFsProvider {
    /// Open a SquashFS image at the given path.
    #[uniffi::constructor]
    pub fn new(image_path: String) -> Result<Arc<Self>, CoreError> {
        let provider = core::SquashFsProvider::open(Path::new(&image_path))
            .map_err(CoreError::from)?;
        Ok(Arc::new(SquashFsProvider {
            inner: Arc::new(provider),
        }))
    }

    /// Get attributes for a given inode.
    pub fn get_attributes(&self, inode: u64) -> Result<EntryAttributes, CoreError> {
        self.inner.get_attributes(inode).map(Into::into).map_err(Into::into)
    }

    /// List directory entries with pagination.
    pub fn list_directory(&self, inode: u64, cookie: u64) -> Result<DirEntryBatch, CoreError> {
        self.inner.list_directory(inode, cookie).map(Into::into).map_err(Into::into)
    }

    /// Look up a single entry by name within a directory.
    pub fn lookup(&self, parent_inode: u64, name: String) -> Result<Option<DirEntry>, CoreError> {
        self.inner
            .lookup(parent_inode, &name)
            .map(|opt| opt.map(Into::into))
            .map_err(Into::into)
    }

    /// Read file content at a byte range.
    pub fn read_file(&self, inode: u64, offset: u64, length: u64) -> Result<Vec<u8>, CoreError> {
        self.inner.read_file(inode, offset, length).map_err(Into::into)
    }

    /// Read the target of a symbolic link.
    pub fn read_symlink(&self, inode: u64) -> Result<String, CoreError> {
        self.inner.read_symlink(inode).map_err(Into::into)
    }

    /// Get filesystem-level statistics.
    pub fn volume_stats(&self) -> Result<VolumeStats, CoreError> {
        self.inner.volume_stats().map(Into::into).map_err(Into::into)
    }

    /// Resolve a relative path to an inode ID.
    pub fn resolve_path(&self, path: String) -> Result<Option<u64>, CoreError> {
        self.inner.resolve_path(Path::new(&path)).map_err(Into::into)
    }
}

uniffi::setup_scaffolding!();
