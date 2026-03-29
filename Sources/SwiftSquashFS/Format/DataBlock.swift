/// DataBlock.swift — Data block size representation.
///
/// Data blocks and fragment blocks use a u32 where bit 24 indicates
/// whether the block is stored uncompressed.
import Foundation

/// Bit flag for uncompressed data blocks.
public let dataBlockUncompressedFlag: UInt32 = 1 << 24

/// Decode the on-disk size from a raw data block size value.
public func dataBlockOnDiskSize(_ raw: UInt32) -> UInt32 {
    raw & ~dataBlockUncompressedFlag
}

/// Check if a data block is stored uncompressed.
public func dataBlockIsUncompressed(_ raw: UInt32) -> Bool {
    raw & dataBlockUncompressedFlag != 0
}

/// A sparse block (all zeros) has a size of 0.
public func dataBlockIsSparse(_ raw: UInt32) -> Bool {
    raw == 0
}
