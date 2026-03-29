/// FragmentTable.swift — Fragment block lookup table.
///
/// The fragment table is a two-level lookup: u64 offsets point to metadata
/// blocks that contain arrays of 16-byte fragment entries.
import Foundation

/// A single fragment block entry (16 bytes).
public struct FragmentEntry: Sendable {
    /// Absolute offset of the fragment block in the image.
    public let start: UInt64
    /// Size + compression flag (same encoding as data block sizes).
    public let size: UInt32
    /// Unused field (always 0).
    public let unused: UInt32
}

/// Read the fragment table from the image.
///
/// - Parameters:
///   - imageData: The full image bytes.
///   - superblock: The parsed superblock.
///   - compressor: Compressor for metadata block decompression.
/// - Returns: Array of fragment entries, or nil if no fragments.
public func readFragmentTable(
    from imageData: Data,
    superblock: Superblock,
    compressor: SquashFsCompressor
) throws -> [FragmentEntry]? {
    guard superblock.fragmentTableStart != squashfsNotPresent else { return nil }
    guard superblock.fragmentEntryCount > 0 else { return nil }

    let entryCount = Int(superblock.fragmentEntryCount)
    // Each metadata block stores 512 entries (16 bytes each, 8KiB block)
    let blockCount = (entryCount + 511) / 512

    // Read the u64 offset array at fragmentTableStart
    let offsetArrayStart = Int(superblock.fragmentTableStart)
    let offsetArraySize = blockCount * 8
    guard offsetArrayStart + offsetArraySize <= imageData.count else {
        throw SquashFsFormatError.truncatedImage("fragment table offset array")
    }

    let offsetReader = BinaryReader(
        data: imageData[imageData.startIndex + offsetArrayStart
                        ..< imageData.startIndex + offsetArrayStart + offsetArraySize]
    )

    var blockOffsets: [UInt64] = []
    for _ in 0..<blockCount {
        blockOffsets.append(try offsetReader.readU64())
    }

    // Read each metadata block and extract fragment entries
    var entries: [FragmentEntry] = []
    entries.reserveCapacity(entryCount)

    for blockOffset in blockOffsets {
        let (blockData, _) = try readMetadataBlock(
            from: imageData,
            at: Int(blockOffset),
            compressor: compressor
        )
        let blockReader = BinaryReader(data: blockData)
        while !blockReader.isAtEnd && entries.count < entryCount {
            let start = try blockReader.readU64()
            let size = try blockReader.readU32()
            let unused = try blockReader.readU32()
            entries.append(FragmentEntry(start: start, size: size, unused: unused))
        }
    }

    return entries
}
