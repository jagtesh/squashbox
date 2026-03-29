/// MetadataBlock.swift — Reading and writing SquashFS 8KiB metadata blocks.
///
/// Metadata blocks (inodes, directories, IDs, fragments) are stored as
/// compressed 8KiB blocks with a 2-byte header. The header's MSB indicates
/// whether the block is stored uncompressed.
import Foundation

/// Maximum uncompressed size of a metadata block.
public let metadataMaxSize: Int = 8192 // 0x2000

/// Bit flag in the u16 metadata header indicating uncompressed storage.
private let metadataUncompressedFlag: UInt16 = 1 << 15

/// Read a single metadata block from raw image data at `offset`.
///
/// Returns the decompressed data and the number of bytes consumed from the image
/// (header + on-disk payload size).
public func readMetadataBlock(
    from imageData: Data,
    at offset: Int,
    compressor: SquashFsCompressor,
    isUncompressedOverride: Bool = false
) throws -> (data: Data, consumed: Int) {
    guard offset + 2 <= imageData.count else {
        throw SquashFsFormatError.truncatedImage("metadata block header at offset \(offset)")
    }

    // Read the 2-byte header
    let headerLow = UInt16(imageData[imageData.startIndex + offset])
    let headerHigh = UInt16(imageData[imageData.startIndex + offset + 1])
    let header = headerLow | (headerHigh << 8)

    let isUncompressed = isUncompressedOverride || (header & metadataUncompressedFlag != 0)
    let onDiskSize = Int(header & ~metadataUncompressedFlag)

    guard onDiskSize > 0 && onDiskSize <= metadataMaxSize else {
        throw SquashFsFormatError.corruptedMetadata(
            "metadata block at \(offset) has invalid size: \(onDiskSize)")
    }

    let payloadStart = offset + 2
    guard payloadStart + onDiskSize <= imageData.count else {
        throw SquashFsFormatError.truncatedImage(
            "metadata block at \(offset) extends past image (need \(onDiskSize) bytes)")
    }

    let payload = imageData[imageData.startIndex + payloadStart
                            ..< imageData.startIndex + payloadStart + onDiskSize]

    let decompressed: Data
    if isUncompressed {
        decompressed = Data(payload)
    } else {
        decompressed = try compressor.decompress(Data(payload), maxOutputSize: metadataMaxSize)
    }

    guard decompressed.count <= metadataMaxSize else {
        throw SquashFsFormatError.corruptedMetadata(
            "metadata block decompressed to \(decompressed.count) bytes (max \(metadataMaxSize))")
    }

    return (data: decompressed, consumed: 2 + onDiskSize)
}

/// Read all consecutive metadata blocks in a section, returning a single
/// contiguous decompressed buffer and a map from on-disk block offset → 
/// offset in the decompressed buffer.
///
/// - Parameters:
///   - imageData: The full image bytes.
///   - start: Absolute offset of the first metadata block.
///   - end: Absolute offset of the byte past the last metadata block.
///   - compressor: The compressor to use for decompression.
///   - forceUncompressed: If true, treat all blocks as uncompressed.
/// - Returns: The concatenated decompressed data and a map from on-disk
///   block offsets (relative to `start`) to decompressed offsets.
public func readMetadataSection(
    from imageData: Data,
    start: Int,
    end: Int,
    compressor: SquashFsCompressor,
    forceUncompressed: Bool = false
) throws -> (data: Data, offsetMap: [UInt64: UInt64]) {
    var result = Data()
    var offsetMap: [UInt64: UInt64] = [:]
    var cursor = start

    while cursor < end {
        let relativeOffset = UInt64(cursor - start)
        let decompressedOffset = UInt64(result.count)
        offsetMap[relativeOffset] = decompressedOffset

        let (block, consumed) = try readMetadataBlock(
            from: imageData,
            at: cursor,
            compressor: compressor,
            isUncompressedOverride: forceUncompressed
        )
        result.append(block)
        cursor += consumed
    }

    return (data: result, offsetMap: offsetMap)
}
