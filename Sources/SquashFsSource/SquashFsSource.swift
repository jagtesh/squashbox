import Foundation
import SquashboxCore

/// A SquashFS-backed implementation of `VirtualFsProvider`.
///
/// Uses libsqfs (from squashfs-tools-ng) for binary parsing and decompression.
/// Builds an `InodeIndex` at initialization time for O(1) lookups.
///
/// ## Status
/// This is a stub awaiting libsqfs integration. The full implementation will:
/// 1. Open the SquashFS image via `sqfs_open_file()`
/// 2. Walk the directory tree via `sqfs_dir_reader_*()`
/// 3. Populate an `InodeIndex` via the Builder pattern
/// 4. Serve reads via `sqfs_data_reader_read()`
public final class SquashFsSource: @unchecked Sendable {
    private let index: InodeIndex

    /// Placeholder — will be replaced with libsqfs handles.
    public init(imagePath: String) throws {
        // TODO: Open image via libsqfs and build index
        // For now, create an empty index
        var builder = InodeIndex.Builder()
        builder.insertRoot(attributes: EntryAttributes(
            inode: rootInodeId,
            entryType: .directory,
            size: 0,
            mode: 0o755,
            uid: 0,
            gid: 0,
            mtimeSecs: 0,
            nlink: 2
        ))
        self.index = builder.build()
    }
}

// MARK: - VirtualFsProvider Conformance (stub)

extension SquashFsSource: VirtualFsProvider {
    public func resolvePath(_ path: String) throws -> InodeId? {
        try index.resolvePath(path)
    }

    public func getAttributes(_ inode: InodeId) throws -> EntryAttributes {
        let entry = try index.get(inode)
        return entry.attributes
    }

    public func listDirectory(_ inode: InodeId, cookie: UInt64) throws -> DirEntryBatch {
        try index.listDirectory(inode, cookie: cookie)
    }

    public func lookup(parent: InodeId, name: String) throws -> DirEntry? {
        try index.lookupChild(parent: parent, name: name)
    }

    public func readFile(_ inode: InodeId, offset: UInt64, length: UInt64) throws -> Data {
        // TODO: Implement via sqfs_data_reader_read()
        throw SquashboxError.notSupported("readFile not yet implemented")
    }

    public func readSymlink(_ inode: InodeId) throws -> String {
        let entry = try index.get(inode)
        guard let target = entry.symlinkTarget else {
            throw SquashboxError.notASymlink("inode \(inode)")
        }
        return target
    }

    public func volumeStats() throws -> VolumeStats {
        // TODO: Implement from sqfs_super_t
        return VolumeStats(totalBytes: 0, totalInodes: UInt64(index.count), blockSize: 0, creationTime: 0)
    }
}
