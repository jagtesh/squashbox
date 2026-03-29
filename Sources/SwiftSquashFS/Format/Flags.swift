/// Flags.swift — SquashFS compression IDs and superblock flag bits.
import Foundation

/// Compression algorithm identifier stored in the superblock.
/// Values from the SquashFS specification:
///   https://dr- Adventure.github.io/squashfs/squashfs.html
public enum CompressionId: UInt16, Sendable {
    case none   = 0
    case gzip   = 1
    case lzma   = 2
    case lzo    = 3
    case xz     = 4
    case lz4    = 5
    case zstd   = 6
}

/// Superblock flags (bit field).
public struct SuperblockFlags: OptionSet, Sendable {
    public let rawValue: UInt16
    public init(rawValue: UInt16) { self.rawValue = rawValue }

    public static let uncompressedInodes     = SuperblockFlags(rawValue: 1 << 0)
    public static let uncompressedData       = SuperblockFlags(rawValue: 1 << 1)
    public static let check                  = SuperblockFlags(rawValue: 1 << 2)
    public static let uncompressedFragments  = SuperblockFlags(rawValue: 1 << 3)
    public static let noFragments            = SuperblockFlags(rawValue: 1 << 4)
    public static let alwaysFragments        = SuperblockFlags(rawValue: 1 << 5)
    public static let duplicates             = SuperblockFlags(rawValue: 1 << 6)
    public static let exportable             = SuperblockFlags(rawValue: 1 << 7)
    public static let uncompressedXattrs     = SuperblockFlags(rawValue: 1 << 8)
    public static let noXattrs               = SuperblockFlags(rawValue: 1 << 9)
    public static let compressorOptions      = SuperblockFlags(rawValue: 1 << 10)
    public static let uncompressedIds        = SuperblockFlags(rawValue: 1 << 11)
}

/// Sentinel for "not present" table offsets (all bits set).
public let squashfsNotPresent: UInt64 = 0xFFFF_FFFF_FFFF_FFFF
