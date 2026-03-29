/// SquashFsReader.swift — Read-only access to SquashFS images.
///
/// This is the main entry point for the SwiftSquashFS library. It reads
/// a SquashFS image, parses all metadata, and provides tree-walking and
/// file-reading APIs.
import Foundation

/// A node in the directory tree, yielded during `walkTree`.
public struct TreeNode: Sendable {
    /// Name of this entry (empty string for root).
    public let name: String
    /// The parsed inode for this entry.
    public let inode: ParsedInode
    /// Resolved UID from the ID table.
    public let uid: UInt32
    /// Resolved GID from the ID table.
    public let gid: UInt32
}

/// Read-only SquashFS image reader.
///
/// Loads the entire image into memory, parses the superblock and all metadata
/// tables, and provides methods to walk the directory tree and read file data.
public final class SquashFsReader: @unchecked Sendable {
    /// The parsed superblock.
    public let superblock: Superblock

    /// The compressor used by this image.
    public let compressor: SquashFsCompressor

    // ── Parsed tables ──

    /// UID/GID lookup table.
    private let idTable: [UInt32]

    /// Fragment block entries (nil if no fragments).
    private let fragmentTable: [FragmentEntry]?

    /// Decompressed inode table and its offset map.
    private let inodeData: Data
    private let inodeOffsetMap: [UInt64: UInt64]

    /// Decompressed directory table and its offset map.
    private let dirData: Data
    private let dirOffsetMap: [UInt64: UInt64]

    /// The raw image data (retained for data block reads).
    private let imageData: Data

    // MARK: - Initialization

    /// Open a SquashFS image from a file path.
    public convenience init(imagePath: String) throws {
        let url = URL(fileURLWithPath: imagePath)
        let data = try Data(contentsOf: url, options: .mappedIfSafe)
        try self.init(data: data)
    }

    /// Open a SquashFS image from in-memory data.
    public init(data: Data) throws {
        self.imageData = data

        // 1. Parse superblock
        let sbReader = BinaryReader(data: data)
        self.superblock = try Superblock.read(from: sbReader)

        // 2. Setup compressor
        self.compressor = try makeCompressor(for: superblock.compressionId)

        // 3. Read ID table
        self.idTable = try readIdTable(
            from: data, superblock: superblock, compressor: compressor)

        // 4. Read fragment table
        self.fragmentTable = try readFragmentTable(
            from: data, superblock: superblock, compressor: compressor)

        // 5. Read and decompress inode table
        let forceUncompressedInodes = superblock.flags.contains(.uncompressedInodes)
        let inodeSection = try readMetadataSection(
            from: data,
            start: Int(superblock.inodeTableStart),
            end: Int(superblock.directoryTableStart),
            compressor: compressor,
            forceUncompressed: forceUncompressedInodes
        )
        self.inodeData = inodeSection.data
        self.inodeOffsetMap = inodeSection.offsetMap

        // 6. Read and decompress directory table
        // Directory table ends at fragment table, export table, or ID table
        let dirEnd: Int
        if superblock.fragmentTableStart != squashfsNotPresent {
            dirEnd = Int(superblock.fragmentTableStart)
        } else if superblock.exportTableStart != squashfsNotPresent {
            dirEnd = Int(superblock.exportTableStart)
        } else {
            dirEnd = Int(superblock.idTableStart)
        }

        let dirSection = try readMetadataSection(
            from: data,
            start: Int(superblock.directoryTableStart),
            end: dirEnd,
            compressor: compressor
        )
        self.dirData = dirSection.data
        self.dirOffsetMap = dirSection.offsetMap
    }

    // MARK: - Inode Resolution

    /// Resolve an inode reference (64-bit value from superblock or directory entry)
    /// to a parsed inode.
    ///
    /// The reference format: upper 32 bits = offset of compressed metadata block
    /// (relative to inode table start), lower 16 bits = offset within decompressed block.
    public func resolveInodeRef(_ ref: UInt64) throws -> ParsedInode {
        let blockOffset = UInt64((ref >> 16) & 0xFFFF_FFFF)
        let intraOffset = UInt16(ref & 0xFFFF)

        guard let decompressedBlockStart = inodeOffsetMap[blockOffset] else {
            throw SquashFsFormatError.invalidInode(
                "inode block offset 0x\(String(blockOffset, radix: 16)) not in offset map")
        }

        let absoluteOffset = Int(decompressedBlockStart) + Int(intraOffset)
        guard absoluteOffset < inodeData.count else {
            throw SquashFsFormatError.invalidInode(
                "inode offset \(absoluteOffset) past decompressed inode table (\(inodeData.count) bytes)")
        }

        let sliceStart = inodeData.startIndex + absoluteOffset
        let reader = BinaryReader(data: Data(inodeData[sliceStart..<inodeData.endIndex]))
        return try parseInode(
            from: reader,
            blockSize: superblock.blockSize,
            blockLog: superblock.blockLog
        )
    }

    /// Resolve a directory entry's inode using the header's metadata block
    /// start and the entry's offset within that block.
    public func resolveDirectoryEntryInode(
        headerStart: UInt32,
        entryOffset: UInt16
    ) throws -> ParsedInode {
        guard let decompressedBlockStart = inodeOffsetMap[UInt64(headerStart)] else {
            throw SquashFsFormatError.invalidInode(
                "directory entry inode block 0x\(String(headerStart, radix: 16)) not in offset map")
        }

        let absoluteOffset = Int(decompressedBlockStart) + Int(entryOffset)
        guard absoluteOffset < inodeData.count else {
            throw SquashFsFormatError.invalidInode(
                "directory entry inode offset \(absoluteOffset) past inode table")
        }

        let sliceStart = inodeData.startIndex + absoluteOffset
        let reader = BinaryReader(data: Data(inodeData[sliceStart..<inodeData.endIndex]))
        return try parseInode(
            from: reader,
            blockSize: superblock.blockSize,
            blockLog: superblock.blockLog
        )
    }

    // MARK: - Tree Walking

    /// Walk the entire directory tree, calling the visitor for each node.
    ///
    /// The root is visited first, then all children recursively in sorted order.
    /// The visitor receives `(parentPath, TreeNode)` for each entry.
    public func walkTree(_ visitor: (_ parentPath: String, _ node: TreeNode) throws -> Void) throws {
        let rootInode = try resolveInodeRef(superblock.rootInodeRef)

        let rootNode = TreeNode(
            name: "",
            inode: rootInode,
            uid: resolveUid(rootInode.header.uidIndex),
            gid: resolveGid(rootInode.header.gidIndex)
        )
        try visitor("/", rootNode)

        try walkDirectory(inode: rootInode, path: "/", visitor: visitor)
    }

    private func walkDirectory(
        inode: ParsedInode,
        path: String,
        visitor: (_ parentPath: String, _ node: TreeNode) throws -> Void
    ) throws {
        let blockIndex: UInt32
        let blockOffset: UInt16
        let fileSize: UInt32

        switch inode.body {
        case .basicDirectory(let d):
            blockIndex = d.blockIndex
            blockOffset = d.blockOffset
            fileSize = UInt32(d.fileSize)
        case .extendedDirectory(let d):
            blockIndex = d.blockIndex
            blockOffset = d.blockOffset
            fileSize = d.fileSize
        default:
            return // Not a directory
        }

        let headerEntries = try readDirectoryEntries(
            dirData: dirData,
            offsetMap: dirOffsetMap,
            blockIndex: blockIndex,
            blockOffset: blockOffset,
            fileSize: fileSize
        )

        for (header, entries) in headerEntries {
            for entry in entries {
                let childInode = try resolveDirectoryEntryInode(
                    headerStart: header.start,
                    entryOffset: entry.offset
                )

                let childNode = TreeNode(
                    name: entry.name,
                    inode: childInode,
                    uid: resolveUid(childInode.header.uidIndex),
                    gid: resolveGid(childInode.header.gidIndex)
                )

                try visitor(path, childNode)

                // Recurse into subdirectories
                if childInode.isDirectory {
                    let childPath = path == "/" ? "/\(entry.name)" : "\(path)/\(entry.name)"
                    try walkDirectory(inode: childInode, path: childPath, visitor: visitor)
                }
            }
        }
    }

    // MARK: - File Reading

    /// Read file data for a file inode at the specified offset and length.
    public func readFileData(
        inode: ParsedInode,
        offset: UInt64,
        length: UInt64
    ) throws -> Data {
        return try SwiftSquashFS.readFileData(
            from: imageData,
            inode: inode,
            offset: offset,
            length: length,
            blockSize: superblock.blockSize,
            compressor: compressor,
            fragmentTable: fragmentTable
        )
    }

    // MARK: - Volume Stats

    /// Get basic volume statistics from the superblock.
    public var volumeStats: (bytesUsed: UInt64, inodeCount: UInt32, blockSize: UInt32, modTime: UInt32) {
        (superblock.bytesUsed, superblock.inodeCount, superblock.blockSize, superblock.modificationTime)
    }

    // MARK: - Private Helpers

    private func resolveUid(_ index: UInt16) -> UInt32 {
        Int(index) < idTable.count ? idTable[Int(index)] : 0
    }

    private func resolveGid(_ index: UInt16) -> UInt32 {
        Int(index) < idTable.count ? idTable[Int(index)] : 0
    }
}
