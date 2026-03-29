/// DirectoryTable.swift — SquashFS directory header and entry parsing.
///
/// The directory table stores sorted lists of entries for each directory,
/// using delta-encoded inode numbers and off-by-one name sizes.
import Foundation

/// A directory header precedes a run of 1–256 entries.
public struct DirHeader: Sendable {
    /// Number of entries following (actual count = count + 1).
    public let count: UInt32
    /// Start offset of the metadata block containing the inodes
    /// (relative to inode table start).
    public let start: UInt32
    /// Base inode number; entries store deltas from this.
    public let inodeNumber: UInt32

    public static func read(from reader: BinaryReader) throws -> DirHeader {
        return DirHeader(
            count: try reader.readU32(),
            start: try reader.readU32(),
            inodeNumber: try reader.readU32()
        )
    }
}

/// A single directory entry.
public struct DirEntryRaw: Sendable {
    /// Offset into the uncompressed inode metadata block.
    public let offset: UInt16
    /// Delta from the header's base inode number (signed).
    public let inodeOffset: Int16
    /// Inode type (basic type, even for extended inodes).
    public let type: UInt16
    /// Name size minus 1 (i.e., actual name length = nameSize + 1).
    public let nameSize: UInt16
    /// Raw filename bytes (length = nameSize + 1).
    public let nameBytes: Data

    /// The filename as a UTF-8 string.
    public var name: String {
        String(data: nameBytes, encoding: .utf8) ?? ""
    }

    public static func read(from reader: BinaryReader) throws -> DirEntryRaw {
        let offset = try reader.readU16()
        let inodeOffset = try reader.readI16()
        let type = try reader.readU16()
        let nameSize = try reader.readU16()
        let nameBytes = try reader.readBytes(count: Int(nameSize) + 1)
        return DirEntryRaw(
            offset: offset,
            inodeOffset: inodeOffset,
            type: type,
            nameSize: nameSize,
            nameBytes: nameBytes
        )
    }
}

/// Read all directory entries for a directory inode from the decompressed
/// directory table.
///
/// - Parameters:
///   - dirData: The full decompressed directory table.
///   - offsetMap: Map from on-disk block offset → decompressed offset
///     (from `readMetadataSection`).
///   - blockIndex: The `block_index` field from the directory inode.
///   - blockOffset: The `block_offset` field from the directory inode.
///   - fileSize: The `file_size` field from the directory inode.
///     Note: actual data size is `fileSize - 3` (spec quirk).
/// - Returns: Array of `(DirHeader, [DirEntryRaw])` pairs.
public func readDirectoryEntries(
    dirData: Data,
    offsetMap: [UInt64: UInt64],
    blockIndex: UInt32,
    blockOffset: UInt16,
    fileSize: UInt32
) throws -> [(header: DirHeader, entries: [DirEntryRaw])] {
    guard fileSize >= 3 else { return [] }

    // Resolve the starting position in the decompressed directory data
    guard let decompressedBlockStart = offsetMap[UInt64(blockIndex)] else {
        throw SquashFsFormatError.invalidDirectory(
            "block_index \(blockIndex) not found in directory offset map")
    }

    let startOffset = Int(decompressedBlockStart) + Int(blockOffset)
    let dataSize = Int(fileSize) - 3  // spec: file_size includes 3-byte overhead

    guard startOffset + dataSize <= dirData.count else {
        throw SquashFsFormatError.invalidDirectory(
            "directory data extends past table (start=\(startOffset), size=\(dataSize), table=\(dirData.count))")
    }

    let slice = dirData[dirData.startIndex + startOffset
                        ..< dirData.startIndex + startOffset + dataSize]
    let reader = BinaryReader(data: Data(slice))

    var results: [(header: DirHeader, entries: [DirEntryRaw])] = []

    while !reader.isAtEnd {
        let header = try DirHeader.read(from: reader)
        let entryCount = Int(header.count) + 1  // off-by-one encoding

        var entries: [DirEntryRaw] = []
        entries.reserveCapacity(entryCount)
        for _ in 0..<entryCount {
            entries.append(try DirEntryRaw.read(from: reader))
        }
        results.append((header: header, entries: entries))
    }

    return results
}
