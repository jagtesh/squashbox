// MARK: - Core Type Aliases

/// A unique identifier for an inode in the virtual filesystem.
/// Matches the inode number from the underlying archive format.
public typealias InodeId = UInt64

/// The root inode ID. By convention, the root directory is always inode 1.
public let rootInodeId: InodeId = 1

// MARK: - Entry Type

/// The type of a filesystem entry.
public enum EntryType: String, Sendable, Equatable, CustomStringConvertible {
    case file
    case directory
    case symlink
    case blockDevice
    case charDevice
    case fifo
    case socket

    public var description: String { rawValue }
}

// MARK: - Entry Attributes

/// Metadata attributes for a filesystem entry (file, directory, symlink, etc.).
/// Platform-agnostic — no OS-specific types leak through.
public struct EntryAttributes: Sendable, Equatable {
    public let inode: InodeId
    public let entryType: EntryType
    public let size: UInt64
    public let mode: UInt32
    public let uid: UInt32
    public let gid: UInt32
    public let mtimeSecs: UInt32
    public let nlink: UInt32

    public init(
        inode: InodeId,
        entryType: EntryType,
        size: UInt64,
        mode: UInt32,
        uid: UInt32,
        gid: UInt32,
        mtimeSecs: UInt32,
        nlink: UInt32
    ) {
        self.inode = inode
        self.entryType = entryType
        self.size = size
        self.mode = mode
        self.uid = uid
        self.gid = gid
        self.mtimeSecs = mtimeSecs
        self.nlink = nlink
    }

    /// Convenience checkers
    public var isFile: Bool { entryType == .file }
    public var isDirectory: Bool { entryType == .directory }
    public var isSymlink: Bool { entryType == .symlink }
}

// MARK: - Directory Entry

/// A single entry within a directory listing.
public struct DirEntry: Sendable, Equatable {
    public let name: String
    public let inode: InodeId
    public let entryType: EntryType

    public init(name: String, inode: InodeId, entryType: EntryType) {
        self.name = name
        self.inode = inode
        self.entryType = entryType
    }
}

// MARK: - Directory Entry Batch

/// A batch of directory entries, used for paginated directory listing.
/// `cookie` is an opaque cursor — pass it back to `listDirectory` to get the next batch.
/// When `entries` is empty, the listing is complete.
public struct DirEntryBatch: Sendable, Equatable {
    public let entries: [DirEntry]
    public let cookie: UInt64

    public init(entries: [DirEntry], cookie: UInt64) {
        self.entries = entries
        self.cookie = cookie
    }

    /// A terminal batch with no more entries.
    public static var empty: DirEntryBatch {
        DirEntryBatch(entries: [], cookie: 0)
    }

    public var isEmpty: Bool { entries.isEmpty }
}

// MARK: - Volume Stats

/// Aggregate statistics for the mounted volume.
public struct VolumeStats: Sendable, Equatable {
    /// Total size of the archive in bytes.
    public let totalBytes: UInt64
    /// Total number of inodes in the filesystem.
    public let totalInodes: UInt64
    /// Block size used by the filesystem.
    public let blockSize: UInt32
    /// Timestamp of the filesystem creation/modification.
    public let creationTime: UInt32

    public init(
        totalBytes: UInt64,
        totalInodes: UInt64,
        blockSize: UInt32,
        creationTime: UInt32
    ) {
        self.totalBytes = totalBytes
        self.totalInodes = totalInodes
        self.blockSize = blockSize
        self.creationTime = creationTime
    }
}
