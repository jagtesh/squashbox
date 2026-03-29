/// Superblock.swift — The 96-byte SquashFS superblock.
///
/// This is the first structure in every SquashFS image and contains
/// critical metadata including section offsets and archive properties.
import Foundation

/// The magic number "hsqs" in little-endian.
public let squashfsMagic: UInt32 = 0x7371_7368

/// 96-byte superblock structure.
public struct Superblock: Sendable {
    public var magic: UInt32
    public var inodeCount: UInt32
    public var modificationTime: UInt32
    public var blockSize: UInt32
    public var fragmentEntryCount: UInt32
    public var compressionId: CompressionId
    public var blockLog: UInt16
    public var flags: SuperblockFlags
    public var idCount: UInt16
    public var versionMajor: UInt16
    public var versionMinor: UInt16
    public var rootInodeRef: UInt64
    public var bytesUsed: UInt64
    public var idTableStart: UInt64
    public var xattrTableStart: UInt64
    public var inodeTableStart: UInt64
    public var directoryTableStart: UInt64
    public var fragmentTableStart: UInt64
    public var exportTableStart: UInt64

    /// Size of the superblock in bytes (fixed).
    public static let size: Int = 96

    /// Parse a superblock from a BinaryReader.
    public static func read(from reader: BinaryReader) throws -> Superblock {
        let magic = try reader.readU32()
        guard magic == squashfsMagic else {
            throw SquashFsFormatError.invalidMagic(magic)
        }

        let inodeCount = try reader.readU32()
        let modificationTime = try reader.readU32()
        let blockSize = try reader.readU32()
        let fragmentEntryCount = try reader.readU32()

        let compressionRaw = try reader.readU16()
        guard let compressionId = CompressionId(rawValue: compressionRaw) else {
            throw SquashFsFormatError.unsupportedCompression(compressionRaw)
        }

        let blockLog = try reader.readU16()
        let flagsRaw = try reader.readU16()
        let flags = SuperblockFlags(rawValue: flagsRaw)
        let idCount = try reader.readU16()

        let versionMajor = try reader.readU16()
        let versionMinor = try reader.readU16()
        guard versionMajor == 4 && versionMinor == 0 else {
            throw SquashFsFormatError.unsupportedVersion(major: versionMajor, minor: versionMinor)
        }

        let rootInodeRef = try reader.readU64()
        let bytesUsed = try reader.readU64()
        let idTableStart = try reader.readU64()
        let xattrTableStart = try reader.readU64()
        let inodeTableStart = try reader.readU64()
        let directoryTableStart = try reader.readU64()
        let fragmentTableStart = try reader.readU64()
        let exportTableStart = try reader.readU64()

        let sb = Superblock(
            magic: magic,
            inodeCount: inodeCount,
            modificationTime: modificationTime,
            blockSize: blockSize,
            fragmentEntryCount: fragmentEntryCount,
            compressionId: compressionId,
            blockLog: blockLog,
            flags: flags,
            idCount: idCount,
            versionMajor: versionMajor,
            versionMinor: versionMinor,
            rootInodeRef: rootInodeRef,
            bytesUsed: bytesUsed,
            idTableStart: idTableStart,
            xattrTableStart: xattrTableStart,
            inodeTableStart: inodeTableStart,
            directoryTableStart: directoryTableStart,
            fragmentTableStart: fragmentTableStart,
            exportTableStart: exportTableStart
        )

        // Validate block size
        guard sb.blockSize >= 4096 && sb.blockSize <= 1_048_576 else {
            throw SquashFsFormatError.invalidBlockSize(sb.blockSize)
        }
        guard sb.blockSize & (sb.blockSize - 1) == 0 else {
            throw SquashFsFormatError.invalidBlockSize(sb.blockSize)  // must be power of 2
        }

        return sb
    }

    /// Write the superblock to a BinaryWriter.
    public func write(to writer: BinaryWriter) {
        writer.writeU32(magic)
        writer.writeU32(inodeCount)
        writer.writeU32(modificationTime)
        writer.writeU32(blockSize)
        writer.writeU32(fragmentEntryCount)
        writer.writeU16(compressionId.rawValue)
        writer.writeU16(blockLog)
        writer.writeU16(flags.rawValue)
        writer.writeU16(idCount)
        writer.writeU16(versionMajor)
        writer.writeU16(versionMinor)
        writer.writeU64(rootInodeRef)
        writer.writeU64(bytesUsed)
        writer.writeU64(idTableStart)
        writer.writeU64(xattrTableStart)
        writer.writeU64(inodeTableStart)
        writer.writeU64(directoryTableStart)
        writer.writeU64(fragmentTableStart)
        writer.writeU64(exportTableStart)
    }
}

// MARK: - Format Errors

public enum SquashFsFormatError: Error, CustomStringConvertible {
    case invalidMagic(UInt32)
    case unsupportedCompression(UInt16)
    case unsupportedVersion(major: UInt16, minor: UInt16)
    case invalidBlockSize(UInt32)
    case corruptedMetadata(String)
    case invalidInode(String)
    case invalidDirectory(String)
    case truncatedImage(String)

    public var description: String {
        switch self {
        case .invalidMagic(let m):
            return "invalid SquashFS magic: 0x\(String(m, radix: 16)) (expected 0x73717368)"
        case .unsupportedCompression(let c):
            return "unsupported compression id: \(c)"
        case .unsupportedVersion(let major, let minor):
            return "unsupported SquashFS version: \(major).\(minor) (expected 4.0)"
        case .invalidBlockSize(let s):
            return "invalid block size: \(s) (must be power of 2, 4K-1M)"
        case .corruptedMetadata(let msg):
            return "corrupted metadata: \(msg)"
        case .invalidInode(let msg):
            return "invalid inode: \(msg)"
        case .invalidDirectory(let msg):
            return "invalid directory: \(msg)"
        case .truncatedImage(let msg):
            return "truncated image: \(msg)"
        }
    }
}
