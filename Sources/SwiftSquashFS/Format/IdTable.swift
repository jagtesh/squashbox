/// IdTable.swift — UID/GID lookup table.
///
/// Same two-level structure as the fragment table: u64 offsets point to
/// metadata blocks containing packed u32 IDs.
import Foundation

/// Read the UID/GID lookup table.
///
/// - Parameters:
///   - imageData: The full image bytes.
///   - superblock: The parsed superblock.
///   - compressor: Compressor for metadata block decompression.
/// - Returns: Array of u32 IDs (UIDs and GIDs share the same table).
public func readIdTable(
    from imageData: Data,
    superblock: Superblock,
    compressor: SquashFsCompressor
) throws -> [UInt32] {
    let idCount = Int(superblock.idCount)
    guard idCount > 0 else { return [] }

    // Each metadata block stores 2048 IDs (4 bytes each, 8KiB)
    let blockCount = (idCount + 2047) / 2048

    // Read the u64 offset array at idTableStart
    let offsetArrayStart = Int(superblock.idTableStart)
    let offsetArraySize = blockCount * 8
    guard offsetArrayStart + offsetArraySize <= imageData.count else {
        throw SquashFsFormatError.truncatedImage("ID table offset array")
    }

    let offsetReader = BinaryReader(
        data: imageData[imageData.startIndex + offsetArrayStart
                        ..< imageData.startIndex + offsetArrayStart + offsetArraySize]
    )

    var blockOffsets: [UInt64] = []
    for _ in 0..<blockCount {
        blockOffsets.append(try offsetReader.readU64())
    }

    // Read each metadata block and extract IDs
    let forceUncompressed = superblock.flags.contains(.uncompressedIds)
    var ids: [UInt32] = []
    ids.reserveCapacity(idCount)

    for blockOffset in blockOffsets {
        let (blockData, _) = try readMetadataBlock(
            from: imageData,
            at: Int(blockOffset),
            compressor: compressor,
            isUncompressedOverride: forceUncompressed
        )
        let blockReader = BinaryReader(data: blockData)
        while !blockReader.isAtEnd && ids.count < idCount {
            ids.append(try blockReader.readU32())
        }
    }

    return ids
}
