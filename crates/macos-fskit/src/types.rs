//! Rust-native types mirroring FSKit's ObjC types.
//!
//! These provide a clean Rust interface that the `FsKitFileSystemSource` trait
//! uses, isolating consumers from raw ObjC types.

use std::fmt;

/// File type classification matching FSKit's `FSItemType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i64)]
pub enum ItemType {
    Unknown = 0,
    File = 1,
    Directory = 2,
    Symlink = 3,
    Fifo = 4,
    CharDevice = 5,
    BlockDevice = 6,
    Socket = 7,
}

impl fmt::Display for ItemType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ItemType::Unknown => write!(f, "unknown"),
            ItemType::File => write!(f, "file"),
            ItemType::Directory => write!(f, "directory"),
            ItemType::Symlink => write!(f, "symlink"),
            ItemType::Fifo => write!(f, "fifo"),
            ItemType::CharDevice => write!(f, "char_device"),
            ItemType::BlockDevice => write!(f, "block_device"),
            ItemType::Socket => write!(f, "socket"),
        }
    }
}

/// Item attributes, equivalent to FSKit's `FSItemAttributes`.
#[derive(Debug, Clone)]
pub struct ItemAttributes {
    /// Item type (file, directory, symlink, etc.)
    pub item_type: ItemType,
    /// POSIX mode bits (e.g., 0o755).
    pub mode: u32,
    /// Owner user ID.
    pub uid: u32,
    /// Owner group ID.
    pub gid: u32,
    /// Hard link count.
    pub link_count: u32,
    /// File size in bytes.
    pub size: u64,
    /// Allocated size in bytes.
    pub alloc_size: u64,
    /// File ID / inode number.
    pub file_id: u64,
    /// Parent item ID.
    pub parent_id: u64,
    /// Modification time (seconds since epoch, nanoseconds).
    pub mtime: Timespec,
    /// Access time.
    pub atime: Timespec,
    /// Change time.
    pub ctime: Timespec,
    /// Birth (creation) time.
    pub btime: Timespec,
}

/// A timespec matching POSIX `struct timespec`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Timespec {
    pub secs: i64,
    pub nsecs: i64,
}

impl Timespec {
    pub fn from_secs(secs: i64) -> Self {
        Self { secs, nsecs: 0 }
    }
}

/// A directory entry for enumeration.
#[derive(Debug, Clone)]
pub struct DirEntry {
    /// Entry name.
    pub name: String,
    /// Entry type.
    pub item_type: ItemType,
    /// Item ID (inode number).
    pub item_id: u64,
    /// Cookie for this entry (used for pagination).
    pub next_cookie: u64,
    /// Optional attributes (if requested during enumeration).
    pub attributes: Option<ItemAttributes>,
}

/// Volume-level filesystem statistics, matching FSKit's `FSStatFSResult`.
#[derive(Debug, Clone)]
pub struct StatFSResult {
    /// Block size in bytes.
    pub block_size: i64,
    /// Optimal I/O block size.
    pub io_size: i64,
    /// Total data blocks.
    pub total_blocks: u64,
    /// Free blocks available to non-superuser.
    pub available_blocks: u64,
    /// Free blocks.
    pub free_blocks: u64,
    /// Used blocks.
    pub used_blocks: u64,
    /// Total size in bytes.
    pub total_bytes: u64,
    /// Available space in bytes.
    pub available_bytes: u64,
    /// Free space in bytes.
    pub free_bytes: u64,
    /// Used space in bytes.
    pub used_bytes: u64,
    /// Total file slots.
    pub total_files: u64,
    /// Free file slots.
    pub free_files: u64,
    /// Filesystem type name (e.g., "squashfs").
    pub fs_type_name: String,
}

/// Volume information returned when loading a resource.
#[derive(Debug, Clone)]
pub struct VolumeInfo {
    /// A unique identifier for the volume (UUID-like, often from the image).
    pub volume_id: [u8; 16],
    /// Display name for the volume.
    pub volume_name: String,
}

/// Error type for FSKit operations.
#[derive(Debug, thiserror::Error)]
pub enum FsKitError {
    /// POSIX error with errno code.
    #[error("POSIX error {0}: {1}")]
    Posix(i32, String),

    /// Not found (ENOENT).
    #[error("not found: {0}")]
    NotFound(String),

    /// Not a directory (ENOTDIR).
    #[error("not a directory: item {0}")]
    NotADirectory(u64),

    /// Not a file (EISDIR or similar).
    #[error("not a file: item {0}")]
    NotAFile(u64),

    /// Read-only filesystem (EROFS).
    #[error("read-only filesystem")]
    ReadOnly,

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(String),

    /// Internal / unexpected error.
    #[error("internal error: {0}")]
    Internal(String),
}

impl FsKitError {
    /// Convert to a POSIX errno code.
    pub fn to_errno(&self) -> i32 {
        match self {
            FsKitError::Posix(code, _) => *code,
            FsKitError::NotFound(_) => libc::ENOENT,
            FsKitError::NotADirectory(_) => libc::ENOTDIR,
            FsKitError::NotAFile(_) => libc::EISDIR,
            FsKitError::ReadOnly => libc::EROFS,
            FsKitError::Io(_) => libc::EIO,
            FsKitError::Internal(_) => libc::EIO,
        }
    }
}

/// FSKit access mask flags, matching `FSAccessMask`.
#[allow(dead_code)]
pub mod access {
    pub const READ_DATA: u32 = 1 << 1;
    pub const LIST_DIRECTORY: u32 = READ_DATA;
    pub const WRITE_DATA: u32 = 1 << 2;
    pub const EXECUTE: u32 = 1 << 3;
    pub const SEARCH: u32 = EXECUTE;
    pub const DELETE: u32 = 1 << 4;
    pub const APPEND_DATA: u32 = 1 << 5;
    pub const READ_ATTRIBUTES: u32 = 1 << 7;
    pub const WRITE_ATTRIBUTES: u32 = 1 << 8;
    pub const READ_XATTR: u32 = 1 << 9;
    pub const WRITE_XATTR: u32 = 1 << 10;
    pub const READ_SECURITY: u32 = 1 << 11;
    pub const WRITE_SECURITY: u32 = 1 << 12;
    pub const TAKE_OWNERSHIP: u32 = 1 << 13;
}

/// FSKit item ID constants.
pub mod item_id {
    /// Invalid item.
    pub const INVALID: u64 = 0;
    /// Parent of root directory.
    pub const PARENT_OF_ROOT: u64 = 1;
    /// Root directory.
    pub const ROOT_DIRECTORY: u64 = 2;
}

/// Cookie value indicating the start of directory enumeration.
pub const COOKIE_INITIAL: u64 = 0;
