import Foundation
import SquashboxCore
import CSquashFS

/// A SquashFS-backed implementation of `VirtualFsProvider`.
///
/// Uses libsqfs (from squashfs-tools-ng) via the CSquashFS bridge.
/// All libsqfs state is encapsulated behind an opaque `void*` handle.
public final class SquashFsSource: @unchecked Sendable {
    /// The built inode index (immutable after init).
    private let index: InodeIndex

    /// The opaque C handle (owns all libsqfs state).
    /// Swift sees void* as UnsafeMutableRawPointer.
    private let handle: UnsafeMutableRawPointer

    /// Cached superblock info.
    private let superBlock: sqfs_super_t

    /// Mapping from our InodeIndex IDs → libsqfs inode pointers (for data reads).
    /// Valid for the lifetime of the handle (tree is owned by it).
    private let inodeRefs: [InodeId: UnsafeMutablePointer<sqfs_inode_generic_t>]

    /// Open a SquashFS image and build the inode index.
    public init(imagePath: String) throws {
        // 1. Open image via C handle
        guard let h = csqfs_open(imagePath) else {
            throw SquashboxError.io("failed to open image '\(imagePath)'")
        }
        self.handle = h

        // 2. Get superblock
        self.superBlock = csqfs_get_super(h).pointee

        // 3. Load the full directory tree
        guard let root = csqfs_get_tree(h) else {
            csqfs_close(h)
            throw SquashboxError.formatError("failed to read directory tree")
        }

        // 4. Build InodeIndex from the tree
        var builder = InodeIndex.Builder()
        var refs: [InodeId: UnsafeMutablePointer<sqfs_inode_generic_t>] = [:]

        let rootInode = root.pointee.inode!
        let rootId = builder.insertRoot(attributes: EntryAttributes(
            inode: 0,
            entryType: .directory,
            size: 0,
            mode: UInt32(rootInode.pointee.base.mode),
            uid: root.pointee.uid,
            gid: root.pointee.gid,
            mtimeSecs: rootInode.pointee.base.mod_time,
            nlink: 2
        ))
        refs[rootId] = rootInode

        Self.walkTree(node: root, parentInodeId: rootId,
                      builder: &builder, refs: &refs)

        self.index = builder.build()
        self.inodeRefs = refs
    }

    deinit {
        csqfs_close(handle)
    }

    // MARK: - Tree Walking

    private static func walkTree(
        node: UnsafeMutablePointer<sqfs_tree_node_t>,
        parentInodeId: InodeId,
        builder: inout InodeIndex.Builder,
        refs: inout [InodeId: UnsafeMutablePointer<sqfs_inode_generic_t>]
    ) {
        var child = node.pointee.children
        while let c = child {
            let inode = c.pointee.inode!
            let base = inode.pointee.base

            let rawName = String(cString: csqfs_tree_node_get_name(c))
            let safeName = FilenameMapping.toPlatformSafe(rawName)

            let entryType = entryTypeFromInode(base.type)
            let size: UInt64 = (entryType == .file) ? csqfs_inode_get_file_size_val(inode) : 0

            var symlinkTarget: String? = nil
            if entryType == .symlink {
                let targetSize = Int(csqfs_inode_get_symlink_size(inode))
                if targetSize > 0, let targetPtr = csqfs_inode_get_symlink_target(inode) {
                    symlinkTarget = targetPtr.withMemoryRebound(to: UInt8.self, capacity: targetSize) { ptr in
                        String(bytes: UnsafeBufferPointer(start: ptr, count: targetSize), encoding: .utf8)
                    } ?? String(cString: targetPtr)
                }
            }

            let childId = builder.insertEntry(
                parent: parentInodeId,
                name: safeName,
                attributes: EntryAttributes(
                    inode: 0,
                    entryType: entryType,
                    size: size,
                    mode: UInt32(base.mode),
                    uid: c.pointee.uid,
                    gid: c.pointee.gid,
                    mtimeSecs: base.mod_time,
                    nlink: (entryType == .directory) ? 2 : 1
                ),
                symlinkTarget: symlinkTarget
            )
            refs[childId] = inode

            if entryType == .directory {
                walkTree(node: c, parentInodeId: childId,
                         builder: &builder, refs: &refs)
            }

            child = c.pointee.next
        }
    }

    private static func entryTypeFromInode(_ type: UInt16) -> EntryType {
        switch Int32(type) {
        case SQFS_INODE_DIR.rawValue, SQFS_INODE_EXT_DIR.rawValue:
            return .directory
        case SQFS_INODE_FILE.rawValue, SQFS_INODE_EXT_FILE.rawValue:
            return .file
        case SQFS_INODE_SLINK.rawValue, SQFS_INODE_EXT_SLINK.rawValue:
            return .symlink
        case SQFS_INODE_BDEV.rawValue, SQFS_INODE_EXT_BDEV.rawValue:
            return .blockDevice
        case SQFS_INODE_CDEV.rawValue, SQFS_INODE_EXT_CDEV.rawValue:
            return .charDevice
        case SQFS_INODE_FIFO.rawValue, SQFS_INODE_EXT_FIFO.rawValue:
            return .fifo
        case SQFS_INODE_SOCKET.rawValue, SQFS_INODE_EXT_SOCKET.rawValue:
            return .socket
        default:
            return .file
        }
    }
}

// MARK: - VirtualFsProvider Conformance

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
        let entry = try index.get(inode)
        guard entry.attributes.isFile else {
            throw SquashboxError.notAFile("inode \(inode)")
        }
        guard let sqfsInode = inodeRefs[inode] else {
            throw SquashboxError.notFound("no libsqfs inode ref for inode \(inode)")
        }

        let fileSize = entry.attributes.size
        guard offset < fileSize else { return Data() }

        let readSize = min(length, fileSize - offset)
        var buffer = [UInt8](repeating: 0, count: Int(readSize))

        let err = buffer.withUnsafeMutableBufferPointer { bufPtr in
            csqfs_read_file(handle, sqfsInode, offset,
                            bufPtr.baseAddress, UInt32(readSize))
        }
        guard err == 0 else {
            throw SquashboxError.io("failed to read file data (error \(err))")
        }

        return Data(buffer)
    }

    public func readSymlink(_ inode: InodeId) throws -> String {
        let entry = try index.get(inode)
        guard let target = entry.symlinkTarget else {
            throw SquashboxError.notASymlink("inode \(inode)")
        }
        return target
    }

    public func volumeStats() throws -> VolumeStats {
        VolumeStats(
            totalBytes: superBlock.bytes_used,
            totalInodes: UInt64(superBlock.inode_count),
            blockSize: superBlock.block_size,
            creationTime: superBlock.modification_time
        )
    }
}
