/// FileDataReader.swift — Read file data blocks from a SquashFS image.
///
/// Handles block-list iteration, decompression, fragment reads, and
/// slicing to the requested [offset..<offset+length] range.
import Foundation

/// Read file data for a given inode at the specified offset and length.
///
/// This handles both regular data blocks and fragment blocks:
/// - Regular blocks are stored sequentially starting at `blocksStart`
/// - The final sub-block-size chunk may be a fragment stored in a shared
///   fragment block referenced by `fragmentIndex` + `blockOffset`
///
/// - Parameters:
///   - imageData: The full image bytes.
///   - inode: The parsed file inode.
///   - offset: Byte offset into the file to start reading.
///   - length: Number of bytes to read.
///   - blockSize: The image's block size.
///   - compressor: Compressor for decompression.
///   - fragmentTable: Fragment table (may be nil if no fragments).
/// - Returns: The requested file data.
public func readFileData(
    from imageData: Data,
    inode: ParsedInode,
    offset: UInt64,
    length: UInt64,
    blockSize: UInt32,
    compressor: SquashFsCompressor,
    fragmentTable: [FragmentEntry]?
) throws -> Data {
    let fileSize = inode.fileSize
    guard offset < fileSize else { return Data() }

    let actualLength = min(length, fileSize - offset)
    guard actualLength > 0 else { return Data() }

    // Extract block-level fields from the inode
    let blocksStart: UInt64
    let blockSizes: [UInt32]
    let fragmentIndex: UInt32
    let fragmentOffset: UInt32

    switch inode.body {
    case .basicFile(let f):
        blocksStart = UInt64(f.blocksStart)
        blockSizes = f.blockSizes
        fragmentIndex = f.fragmentIndex
        fragmentOffset = f.blockOffset
    case .extendedFile(let f):
        blocksStart = f.blocksStart
        blockSizes = f.blockSizes
        fragmentIndex = f.fragmentIndex
        fragmentOffset = f.blockOffset
    default:
        throw SquashFsFormatError.invalidInode("readFileData called on non-file inode")
    }

    let hasFragment = fragmentIndex != BasicFile.noFragment
    let bs = UInt64(blockSize)

    // Build the complete file data by decompressing blocks + fragment
    // We only decompress what's needed based on offset/length
    var result = Data()
    result.reserveCapacity(Int(actualLength))

    var filePosition: UInt64 = 0
    var diskPosition = blocksStart
    let endOffset = offset + actualLength

    // Process data blocks
    for rawBlockSize in blockSizes {
        let blockEnd = filePosition + bs

        if rawBlockSize == 0 {
            // Sparse block — all zeros
            if blockEnd > offset && filePosition < endOffset {
                let sliceStart = max(offset, filePosition) - filePosition
                let sliceEnd = min(endOffset, blockEnd) - filePosition
                result.append(Data(count: Int(sliceEnd - sliceStart)))
            }
            filePosition = blockEnd
            continue
        }

        let onDiskSize = dataBlockOnDiskSize(rawBlockSize)
        let isUncompressed = dataBlockIsUncompressed(rawBlockSize)

        // Only decompress if this block overlaps our read range
        if blockEnd > offset && filePosition < endOffset {
            guard Int(diskPosition) + Int(onDiskSize) <= imageData.count else {
                throw SquashFsFormatError.truncatedImage(
                    "data block at \(diskPosition) extends past image")
            }

            let blockPayload = imageData[
                imageData.startIndex + Int(diskPosition)
                ..< imageData.startIndex + Int(diskPosition) + Int(onDiskSize)
            ]

            let decompressed: Data
            if isUncompressed {
                decompressed = Data(blockPayload)
            } else {
                decompressed = try compressor.decompress(
                    Data(blockPayload), maxOutputSize: Int(blockSize))
            }

            // Slice the decompressed block to the requested range
            let sliceStart = Int(max(offset, filePosition) - filePosition)
            let sliceEnd = Int(min(endOffset, filePosition + UInt64(decompressed.count)) - filePosition)
            if sliceStart < sliceEnd && sliceEnd <= decompressed.count {
                result.append(decompressed[decompressed.startIndex + sliceStart
                                           ..< decompressed.startIndex + sliceEnd])
            }
        }

        diskPosition += UInt64(onDiskSize)
        filePosition = blockEnd
    }

    // Process fragment (if present and overlaps our range)
    if hasFragment && filePosition < fileSize && filePosition < endOffset {
        guard let fragments = fragmentTable,
              Int(fragmentIndex) < fragments.count else {
            throw SquashFsFormatError.invalidInode(
                "fragment index \(fragmentIndex) out of range")
        }

        let fragEntry = fragments[Int(fragmentIndex)]
        let fragOnDiskSize = dataBlockOnDiskSize(fragEntry.size)
        let fragIsUncompressed = dataBlockIsUncompressed(fragEntry.size)

        guard Int(fragEntry.start) + Int(fragOnDiskSize) <= imageData.count else {
            throw SquashFsFormatError.truncatedImage(
                "fragment block at \(fragEntry.start) extends past image")
        }

        let fragPayload = imageData[
            imageData.startIndex + Int(fragEntry.start)
            ..< imageData.startIndex + Int(fragEntry.start) + Int(fragOnDiskSize)
        ]

        let decompressedFrag: Data
        if fragIsUncompressed {
            decompressedFrag = Data(fragPayload)
        } else {
            decompressedFrag = try compressor.decompress(
                Data(fragPayload), maxOutputSize: Int(blockSize))
        }

        // Our fragment starts at fragmentOffset within the decompressed block
        let fragDataSize = fileSize - filePosition
        let fragStart = Int(fragmentOffset)
        let fragEnd = fragStart + Int(fragDataSize)

        guard fragEnd <= decompressedFrag.count else {
            throw SquashFsFormatError.corruptedMetadata(
                "fragment data extends past decompressed fragment block")
        }

        let fragData = decompressedFrag[
            decompressedFrag.startIndex + fragStart
            ..< decompressedFrag.startIndex + fragEnd
        ]

        // Slice the fragment data to the requested range
        if filePosition < endOffset {
            let sliceStart = Int(max(offset, filePosition) - filePosition)
            let sliceEnd = Int(min(endOffset, filePosition + fragDataSize) - filePosition)
            if sliceStart < sliceEnd && sliceEnd <= fragData.count {
                result.append(fragData[fragData.startIndex + sliceStart
                                       ..< fragData.startIndex + sliceEnd])
            }
        }
    }

    return result
}
