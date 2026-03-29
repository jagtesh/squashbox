import Foundation
import SquashboxCore
import SwiftSquashFS

/// A SquashFS-backed implementation of `VirtualFsProvider`.
///
/// Uses SwiftSquashFS (pure Swift) for all SquashFS parsing.
/// BSD-3-Clause licensed — no LGPL dependencies.
public final class SquashFsSource: @unchecked Sendable {
    /// The built inode index (immutable after init).
    private let index: InodeIndex

    /// The SwiftSquashFS reader (owns the image data).
    private let reader: SquashFsReader

    /// Mapping from our InodeIndex IDs → parsed inodes (for data reads).
    private let inodeRefs: [InodeId: ParsedInode]

    /// Open a SquashFS image and build the inode index.
    public init(imagePath: String) throws {
        // 1. Open image via SwiftSquashFS reader
        let reader = try SquashFsReader(imagePath: imagePath)
        self.reader = reader

        // 2. Build InodeIndex by walking the tree
        // We track a mapping from parentPath → InodeId so we can resolve
        // the flat (parentPath, node) visitor pattern into parent InodeIds.
        var builder = InodeIndex.Builder()
        var refs: [InodeId: ParsedInode] = [:]
        var pathToInodeId: [String: InodeId] = [:]

        try reader.walkTree { parentPath, node in
            if parentPath == "/" && node.name.isEmpty {
                // Root node
                let rootId = builder.insertRoot(attributes: EntryAttributes(
                    inode: 0,
                    entryType: .directory,
                    size: 0,
                    mode: UInt32(node.inode.header.permissions),
                    uid: node.uid,
                    gid: node.gid,
                    mtimeSecs: node.inode.header.modifiedTime,
                    nlink: node.inode.linkCount
                ))
                refs[rootId] = node.inode
                pathToInodeId["/"] = rootId
                return
            }

            // Look up the parent InodeId from the path map
            guard let parentInodeId = pathToInodeId[parentPath] else {
                throw SquashboxError.notFound("parent path '\(parentPath)' not resolved")
            }

            let entryType = Self.entryTypeFromInode(node.inode)
            let safeName = FilenameMapping.toPlatformSafe(node.name)

            var symlinkTarget: String? = nil
            if entryType == .symlink {
                symlinkTarget = node.inode.symlinkTarget
            }

            let childId = builder.insertEntry(
                parent: parentInodeId,
                name: safeName,
                attributes: EntryAttributes(
                    inode: 0,
                    entryType: entryType,
                    size: node.inode.fileSize,
                    mode: UInt32(node.inode.header.permissions),
                    uid: node.uid,
                    gid: node.gid,
                    mtimeSecs: node.inode.header.modifiedTime,
                    nlink: node.inode.linkCount
                ),
                symlinkTarget: symlinkTarget
            )
            refs[childId] = node.inode

            // Track this node's path for its children
            if entryType == .directory {
                let childPath = parentPath == "/"
                    ? "/\(safeName)"
                    : "\(parentPath)/\(safeName)"
                pathToInodeId[childPath] = childId
            }
        }

        self.index = builder.build()
        self.inodeRefs = refs
    }

    private static func entryTypeFromInode(_ inode: ParsedInode) -> EntryType {
        switch inode.header.type {
        case .basicDirectory, .extendedDirectory:
            return .directory
        case .basicFile, .extendedFile:
            return .file
        case .basicSymlink, .extendedSymlink:
            return .symlink
        case .basicBlockDevice, .extendedBlockDevice:
            return .blockDevice
        case .basicCharDevice, .extendedCharDevice:
            return .charDevice
        case .basicFifo, .extendedFifo:
            return .fifo
        case .basicSocket, .extendedSocket:
            return .socket
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
        guard let parsedInode = inodeRefs[inode] else {
            throw SquashboxError.notFound("no parsed inode for inode \(inode)")
        }

        return try reader.readFileData(
            inode: parsedInode,
            offset: offset,
            length: length
        )
    }

    public func readSymlink(_ inode: InodeId) throws -> String {
        let entry = try index.get(inode)
        guard let target = entry.symlinkTarget else {
            throw SquashboxError.notASymlink("inode \(inode)")
        }
        return target
    }

    public func volumeStats() throws -> VolumeStats {
        let stats = reader.volumeStats
        return VolumeStats(
            totalBytes: stats.bytesUsed,
            totalInodes: UInt64(stats.inodeCount),
            blockSize: stats.blockSize,
            creationTime: stats.modTime
        )
    }
}
