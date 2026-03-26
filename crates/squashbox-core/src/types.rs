//! Shared types used across all Squashbox layers.
//!
//! These types are FFI-safe (for UniFFI on macOS) and serialization-free.
//! Both the ProjFS and FSKit drivers consume these types.

use std::fmt;

/// Unique identifier for a filesystem item.
/// On ProjFS this maps from a path lookup; on FSKit this is the inode number.
pub type InodeId = u64;

/// Root inode is always 1 (matches SquashFS convention).
pub const ROOT_INODE: InodeId = 1;

/// File type classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryType {
    File,
    Directory,
    Symlink,
    /// Block device (preserved from SquashFS but not projected)
    BlockDevice,
    /// Character device (preserved from SquashFS but not projected)
    CharDevice,
}

impl fmt::Display for EntryType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EntryType::File => write!(f, "file"),
            EntryType::Directory => write!(f, "directory"),
            EntryType::Symlink => write!(f, "symlink"),
            EntryType::BlockDevice => write!(f, "block_device"),
            EntryType::CharDevice => write!(f, "char_device"),
        }
    }
}

/// Metadata for a single filesystem entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryAttributes {
    /// Inode number.
    pub inode: InodeId,
    /// Type of this entry.
    pub entry_type: EntryType,
    /// File size in bytes (0 for directories).
    pub size: u64,
    /// POSIX mode bits (e.g., 0o755).
    pub mode: u32,
    /// Owner user ID.
    pub uid: u32,
    /// Owner group ID.
    pub gid: u32,
    /// Last modification time as Unix timestamp (seconds since epoch).
    pub mtime_secs: i64,
    /// Hard link count.
    pub nlink: u32,
}

impl EntryAttributes {
    /// Returns `true` if this entry is a regular file.
    pub fn is_file(&self) -> bool {
        self.entry_type == EntryType::File
    }

    /// Returns `true` if this entry is a directory.
    pub fn is_dir(&self) -> bool {
        self.entry_type == EntryType::Directory
    }

    /// Returns `true` if this entry is a symbolic link.
    pub fn is_symlink(&self) -> bool {
        self.entry_type == EntryType::Symlink
    }
}

/// A single directory entry (name + attributes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// Name of this entry (file/directory name, not full path).
    pub name: String,
    /// Metadata for this entry.
    pub attributes: EntryAttributes,
}

/// A paginated batch of directory entries.
#[derive(Debug, Clone)]
pub struct DirEntryBatch {
    /// Entries in this batch.
    pub entries: Vec<DirEntry>,
    /// Cookie for the next batch. 0 means no more entries.
    pub next_cookie: u64,
}

/// Volume-level statistics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeStats {
    /// Total size of the filesystem image in bytes.
    pub total_bytes: u64,
    /// Used (decompressed) size in bytes.
    pub used_bytes: u64,
    /// Total number of inodes.
    pub total_inodes: u64,
    /// Number of used inodes.
    pub used_inodes: u64,
    /// Block size in bytes.
    pub block_size: u32,
}

/// Core error type for all squashbox-core operations.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// The requested path or inode was not found.
    #[error("not found: {0}")]
    NotFound(String),

    /// The inode is not a directory when a directory was expected.
    #[error("not a directory: inode {0}")]
    NotADirectory(InodeId),

    /// The inode is not a file when a file was expected.
    #[error("not a file: inode {0}")]
    NotAFile(InodeId),

    /// The inode is not a symlink when a symlink was expected.
    #[error("not a symlink: inode {0}")]
    NotASymlink(InodeId),

    /// An I/O error occurred.
    #[error("I/O error: {0}")]
    Io(String),

    /// A SquashFS-specific error occurred.
    #[error("squashfs error: {0}")]
    SquashFs(String),

    /// The requested operation is not supported.
    #[error("operation not supported")]
    NotSupported,

    /// The filesystem is read-only (write operation attempted).
    #[error("read-only filesystem")]
    ReadOnly,
}

impl From<std::io::Error> for CoreError {
    fn from(e: std::io::Error) -> Self {
        CoreError::Io(e.to_string())
    }
}

impl From<backhand::BackhandError> for CoreError {
    fn from(e: backhand::BackhandError) -> Self {
        CoreError::SquashFs(e.to_string())
    }
}

/// Convenience type alias for `Result<T, CoreError>`.
pub type CoreResult<T> = Result<T, CoreError>;

#[cfg(test)]
mod tests {
    use super::*;

    // ── EntryType tests ──

    #[test]
    fn entry_type_display() {
        assert_eq!(EntryType::File.to_string(), "file");
        assert_eq!(EntryType::Directory.to_string(), "directory");
        assert_eq!(EntryType::Symlink.to_string(), "symlink");
        assert_eq!(EntryType::BlockDevice.to_string(), "block_device");
        assert_eq!(EntryType::CharDevice.to_string(), "char_device");
    }

    #[test]
    fn entry_type_equality() {
        assert_eq!(EntryType::File, EntryType::File);
        assert_ne!(EntryType::File, EntryType::Directory);
    }

    #[test]
    fn entry_type_clone() {
        let t = EntryType::Symlink;
        let t2 = t;
        assert_eq!(t, t2);
    }

    // ── EntryAttributes tests ──

    fn sample_file_attrs() -> EntryAttributes {
        EntryAttributes {
            inode: 42,
            entry_type: EntryType::File,
            size: 1024,
            mode: 0o644,
            uid: 1000,
            gid: 1000,
            mtime_secs: 1700000000,
            nlink: 1,
        }
    }

    fn sample_dir_attrs() -> EntryAttributes {
        EntryAttributes {
            inode: 2,
            entry_type: EntryType::Directory,
            size: 0,
            mode: 0o755,
            uid: 0,
            gid: 0,
            mtime_secs: 1700000000,
            nlink: 3,
        }
    }

    fn sample_symlink_attrs() -> EntryAttributes {
        EntryAttributes {
            inode: 99,
            entry_type: EntryType::Symlink,
            size: 11,
            mode: 0o777,
            uid: 1000,
            gid: 1000,
            mtime_secs: 1700000000,
            nlink: 1,
        }
    }

    #[test]
    fn entry_attributes_is_file() {
        assert!(sample_file_attrs().is_file());
        assert!(!sample_file_attrs().is_dir());
        assert!(!sample_file_attrs().is_symlink());
    }

    #[test]
    fn entry_attributes_is_dir() {
        assert!(sample_dir_attrs().is_dir());
        assert!(!sample_dir_attrs().is_file());
        assert!(!sample_dir_attrs().is_symlink());
    }

    #[test]
    fn entry_attributes_is_symlink() {
        assert!(sample_symlink_attrs().is_symlink());
        assert!(!sample_symlink_attrs().is_file());
        assert!(!sample_symlink_attrs().is_dir());
    }

    #[test]
    fn entry_attributes_equality() {
        let a = sample_file_attrs();
        let b = sample_file_attrs();
        assert_eq!(a, b);
    }

    #[test]
    fn entry_attributes_inequality_different_inode() {
        let a = sample_file_attrs();
        let mut b = sample_file_attrs();
        b.inode = 999;
        assert_ne!(a, b);
    }

    // ── DirEntry tests ──

    #[test]
    fn dir_entry_construction() {
        let entry = DirEntry {
            name: "hello.txt".to_string(),
            attributes: sample_file_attrs(),
        };
        assert_eq!(entry.name, "hello.txt");
        assert!(entry.attributes.is_file());
        assert_eq!(entry.attributes.size, 1024);
    }

    #[test]
    fn dir_entry_equality() {
        let a = DirEntry {
            name: "foo".to_string(),
            attributes: sample_dir_attrs(),
        };
        let b = DirEntry {
            name: "foo".to_string(),
            attributes: sample_dir_attrs(),
        };
        assert_eq!(a, b);
    }

    #[test]
    fn dir_entry_inequality_different_name() {
        let a = DirEntry {
            name: "foo".to_string(),
            attributes: sample_dir_attrs(),
        };
        let b = DirEntry {
            name: "bar".to_string(),
            attributes: sample_dir_attrs(),
        };
        assert_ne!(a, b);
    }

    // ── DirEntryBatch tests ──

    #[test]
    fn dir_entry_batch_empty() {
        let batch = DirEntryBatch {
            entries: vec![],
            next_cookie: 0,
        };
        assert!(batch.entries.is_empty());
        assert_eq!(batch.next_cookie, 0);
    }

    #[test]
    fn dir_entry_batch_with_entries() {
        let batch = DirEntryBatch {
            entries: vec![
                DirEntry {
                    name: "a.txt".to_string(),
                    attributes: sample_file_attrs(),
                },
                DirEntry {
                    name: "subdir".to_string(),
                    attributes: sample_dir_attrs(),
                },
            ],
            next_cookie: 2,
        };
        assert_eq!(batch.entries.len(), 2);
        assert_eq!(batch.next_cookie, 2);
    }

    // ── VolumeStats tests ──

    #[test]
    fn volume_stats_construction() {
        let stats = VolumeStats {
            total_bytes: 1_000_000,
            used_bytes: 500_000,
            total_inodes: 100,
            used_inodes: 50,
            block_size: 131072,
        };
        assert_eq!(stats.total_bytes, 1_000_000);
        assert_eq!(stats.block_size, 131072);
    }

    // ── CoreError tests ──

    #[test]
    fn core_error_not_found_display() {
        let e = CoreError::NotFound("foo/bar".to_string());
        assert_eq!(e.to_string(), "not found: foo/bar");
    }

    #[test]
    fn core_error_not_a_directory_display() {
        let e = CoreError::NotADirectory(42);
        assert_eq!(e.to_string(), "not a directory: inode 42");
    }

    #[test]
    fn core_error_not_a_file_display() {
        let e = CoreError::NotAFile(42);
        assert_eq!(e.to_string(), "not a file: inode 42");
    }

    #[test]
    fn core_error_not_a_symlink_display() {
        let e = CoreError::NotASymlink(42);
        assert_eq!(e.to_string(), "not a symlink: inode 42");
    }

    #[test]
    fn core_error_io_display() {
        let e = CoreError::Io("disk full".to_string());
        assert_eq!(e.to_string(), "I/O error: disk full");
    }

    #[test]
    fn core_error_read_only_display() {
        let e = CoreError::ReadOnly;
        assert_eq!(e.to_string(), "read-only filesystem");
    }

    #[test]
    fn core_error_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        let core_err: CoreError = io_err.into();
        assert!(matches!(core_err, CoreError::Io(_)));
        assert!(core_err.to_string().contains("gone"));
    }

    // ── ROOT_INODE constant ──

    #[test]
    fn root_inode_is_one() {
        assert_eq!(ROOT_INODE, 1);
    }
}
