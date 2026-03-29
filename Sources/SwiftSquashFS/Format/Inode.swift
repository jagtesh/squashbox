/// Inode.swift — SquashFS inode types and parsing.
///
/// All 14 inode types (7 basic + 7 extended) as defined in the SquashFS v4 spec.
import Foundation

/// Inode type discriminator (from the inode header).
public enum InodeType: UInt16, Sendable {
    case basicDirectory       = 1
    case basicFile            = 2
    case basicSymlink         = 3
    case basicBlockDevice     = 4
    case basicCharDevice      = 5
    case basicFifo            = 6
    case basicSocket          = 7
    case extendedDirectory    = 8
    case extendedFile         = 9
    case extendedSymlink      = 10
    case extendedBlockDevice  = 11
    case extendedCharDevice   = 12
    case extendedFifo         = 13
    case extendedSocket       = 14
}

/// Common inode header (shared by all types).
public struct InodeHeader: Sendable {
    public let type: InodeType
    public let permissions: UInt16
    public let uidIndex: UInt16
    public let gidIndex: UInt16
    public let modifiedTime: UInt32
    public let inodeNumber: UInt32

    static let size: Int = 16 // 2+2+2+2+4+4

    public static func read(from reader: BinaryReader) throws -> InodeHeader {
        let typeRaw = try reader.readU16()
        guard let type = InodeType(rawValue: typeRaw) else {
            throw SquashFsFormatError.invalidInode("unknown inode type: \(typeRaw)")
        }
        return InodeHeader(
            type: type,
            permissions: try reader.readU16(),
            uidIndex: try reader.readU16(),
            gidIndex: try reader.readU16(),
            modifiedTime: try reader.readU32(),
            inodeNumber: try reader.readU32()
        )
    }
}

// MARK: - Inode Body Types

public struct BasicDirectory: Sendable {
    public let blockIndex: UInt32
    public let linkCount: UInt32
    public let fileSize: UInt16
    public let blockOffset: UInt16
    public let parentInode: UInt32

    public static func read(from reader: BinaryReader) throws -> BasicDirectory {
        return BasicDirectory(
            blockIndex: try reader.readU32(),
            linkCount: try reader.readU32(),
            fileSize: try reader.readU16(),
            blockOffset: try reader.readU16(),
            parentInode: try reader.readU32()
        )
    }
}

public struct ExtendedDirectory: Sendable {
    public let linkCount: UInt32
    public let fileSize: UInt32
    public let blockIndex: UInt32
    public let parentInode: UInt32
    public let indexCount: UInt16
    public let blockOffset: UInt16
    public let xattrIndex: UInt32

    public static func read(from reader: BinaryReader) throws -> ExtendedDirectory {
        let ed = ExtendedDirectory(
            linkCount: try reader.readU32(),
            fileSize: try reader.readU32(),
            blockIndex: try reader.readU32(),
            parentInode: try reader.readU32(),
            indexCount: try reader.readU16(),
            blockOffset: try reader.readU16(),
            xattrIndex: try reader.readU32()
        )
        // Skip directory index entries if present
        if ed.indexCount > 0 {
            for _ in 0..<ed.indexCount {
                try reader.skip(4) // index
                try reader.skip(4) // start
                let nameSize = try reader.readU32()
                try reader.skip(Int(nameSize) + 1) // name (off-by-one encoding)
            }
        }
        return ed
    }
}

public struct BasicFile: Sendable {
    public let blocksStart: UInt32
    public let fragmentIndex: UInt32
    public let blockOffset: UInt32
    public let fileSize: UInt32
    public let blockSizes: [UInt32]

    public static let noFragment: UInt32 = 0xFFFF_FFFF

    public static func read(from reader: BinaryReader, blockSize: UInt32, blockLog: UInt16) throws -> BasicFile {
        let blocksStart = try reader.readU32()
        let fragmentIndex = try reader.readU32()
        let blockOffset = try reader.readU32()
        let fileSize = try reader.readU32()

        let blockCount: Int
        if fragmentIndex == noFragment {
            blockCount = Int((UInt64(fileSize) + UInt64(blockSize) - 1) >> blockLog)
        } else {
            blockCount = Int(UInt64(fileSize) >> blockLog)
        }

        var blockSizes: [UInt32] = []
        blockSizes.reserveCapacity(blockCount)
        for _ in 0..<blockCount {
            blockSizes.append(try reader.readU32())
        }

        return BasicFile(
            blocksStart: blocksStart,
            fragmentIndex: fragmentIndex,
            blockOffset: blockOffset,
            fileSize: fileSize,
            blockSizes: blockSizes
        )
    }
}

public struct ExtendedFile: Sendable {
    public let blocksStart: UInt64
    public let fileSize: UInt64
    public let sparse: UInt64
    public let linkCount: UInt32
    public let fragmentIndex: UInt32
    public let blockOffset: UInt32
    public let xattrIndex: UInt32
    public let blockSizes: [UInt32]

    public static let noFragment: UInt32 = 0xFFFF_FFFF

    public static func read(from reader: BinaryReader, blockSize: UInt32, blockLog: UInt16) throws -> ExtendedFile {
        let blocksStart = try reader.readU64()
        let fileSize = try reader.readU64()
        let sparse = try reader.readU64()
        let linkCount = try reader.readU32()
        let fragmentIndex = try reader.readU32()
        let blockOffset = try reader.readU32()
        let xattrIndex = try reader.readU32()

        let blockCount: Int
        if fragmentIndex == ExtendedFile.noFragment {
            blockCount = Int((fileSize + UInt64(blockSize) - 1) >> blockLog)
        } else {
            blockCount = Int(fileSize >> blockLog)
        }

        var blockSizes: [UInt32] = []
        blockSizes.reserveCapacity(blockCount)
        for _ in 0..<blockCount {
            blockSizes.append(try reader.readU32())
        }

        return ExtendedFile(
            blocksStart: blocksStart,
            fileSize: fileSize,
            sparse: sparse,
            linkCount: linkCount,
            fragmentIndex: fragmentIndex,
            blockOffset: blockOffset,
            xattrIndex: xattrIndex,
            blockSizes: blockSizes
        )
    }
}

public struct BasicSymlink: Sendable {
    public let linkCount: UInt32
    public let targetSize: UInt32
    public let targetPath: Data

    public var target: String {
        String(data: targetPath, encoding: .utf8) ?? ""
    }

    public static func read(from reader: BinaryReader) throws -> BasicSymlink {
        let linkCount = try reader.readU32()
        let targetSize = try reader.readU32()
        let targetPath = try reader.readBytes(count: Int(targetSize))
        return BasicSymlink(linkCount: linkCount, targetSize: targetSize, targetPath: targetPath)
    }
}

public struct ExtendedSymlink: Sendable {
    public let linkCount: UInt32
    public let targetSize: UInt32
    public let targetPath: Data
    public let xattrIndex: UInt32

    public var target: String {
        String(data: targetPath, encoding: .utf8) ?? ""
    }

    public static func read(from reader: BinaryReader) throws -> ExtendedSymlink {
        let linkCount = try reader.readU32()
        let targetSize = try reader.readU32()
        let targetPath = try reader.readBytes(count: Int(targetSize))
        let xattrIndex = try reader.readU32()
        return ExtendedSymlink(
            linkCount: linkCount, targetSize: targetSize,
            targetPath: targetPath, xattrIndex: xattrIndex
        )
    }
}

public struct BasicDevice: Sendable {
    public let linkCount: UInt32
    public let deviceNumber: UInt32

    public static func read(from reader: BinaryReader) throws -> BasicDevice {
        return BasicDevice(
            linkCount: try reader.readU32(),
            deviceNumber: try reader.readU32()
        )
    }
}

public struct ExtendedDevice: Sendable {
    public let linkCount: UInt32
    public let deviceNumber: UInt32
    public let xattrIndex: UInt32

    public static func read(from reader: BinaryReader) throws -> ExtendedDevice {
        return ExtendedDevice(
            linkCount: try reader.readU32(),
            deviceNumber: try reader.readU32(),
            xattrIndex: try reader.readU32()
        )
    }
}

public struct BasicIPC: Sendable {
    public let linkCount: UInt32

    public static func read(from reader: BinaryReader) throws -> BasicIPC {
        return BasicIPC(linkCount: try reader.readU32())
    }
}

public struct ExtendedIPC: Sendable {
    public let linkCount: UInt32
    public let xattrIndex: UInt32

    public static func read(from reader: BinaryReader) throws -> ExtendedIPC {
        return ExtendedIPC(
            linkCount: try reader.readU32(),
            xattrIndex: try reader.readU32()
        )
    }
}

// MARK: - Parsed Inode (unified)

/// A fully-parsed inode with header + type-specific body.
public struct ParsedInode: Sendable {
    public let header: InodeHeader
    public let body: InodeBody

    public var isDirectory: Bool {
        switch body {
        case .basicDirectory, .extendedDirectory: return true
        default: return false
        }
    }

    public var isFile: Bool {
        switch body {
        case .basicFile, .extendedFile: return true
        default: return false
        }
    }

    public var isSymlink: Bool {
        switch body {
        case .basicSymlink, .extendedSymlink: return true
        default: return false
        }
    }

    /// File size (0 for non-files).
    public var fileSize: UInt64 {
        switch body {
        case .basicFile(let f): return UInt64(f.fileSize)
        case .extendedFile(let f): return f.fileSize
        default: return 0
        }
    }

    /// Symlink target string, if this is a symlink.
    public var symlinkTarget: String? {
        switch body {
        case .basicSymlink(let s): return s.target
        case .extendedSymlink(let s): return s.target
        default: return nil
        }
    }

    /// Link count.
    public var linkCount: UInt32 {
        switch body {
        case .basicDirectory(let d): return d.linkCount
        case .extendedDirectory(let d): return d.linkCount
        case .basicFile(_): return 1  // basic files don't store nlink
        case .extendedFile(let f): return f.linkCount
        case .basicSymlink(let s): return s.linkCount
        case .extendedSymlink(let s): return s.linkCount
        case .basicBlockDevice(let d): return d.linkCount
        case .basicCharDevice(let d): return d.linkCount
        case .extendedBlockDevice(let d): return d.linkCount
        case .extendedCharDevice(let d): return d.linkCount
        case .basicFifo(let i): return i.linkCount
        case .basicSocket(let i): return i.linkCount
        case .extendedFifo(let i): return i.linkCount
        case .extendedSocket(let i): return i.linkCount
        }
    }
}

/// Type-specific inode body.
public enum InodeBody: Sendable {
    case basicDirectory(BasicDirectory)
    case extendedDirectory(ExtendedDirectory)
    case basicFile(BasicFile)
    case extendedFile(ExtendedFile)
    case basicSymlink(BasicSymlink)
    case extendedSymlink(ExtendedSymlink)
    case basicBlockDevice(BasicDevice)
    case extendedBlockDevice(ExtendedDevice)
    case basicCharDevice(BasicDevice)
    case extendedCharDevice(ExtendedDevice)
    case basicFifo(BasicIPC)
    case extendedFifo(ExtendedIPC)
    case basicSocket(BasicIPC)
    case extendedSocket(ExtendedIPC)
}

/// Parse a complete inode (header + body) from the given reader position.
public func parseInode(
    from reader: BinaryReader,
    blockSize: UInt32,
    blockLog: UInt16
) throws -> ParsedInode {
    let header = try InodeHeader.read(from: reader)

    let body: InodeBody
    switch header.type {
    case .basicDirectory:
        body = .basicDirectory(try BasicDirectory.read(from: reader))
    case .extendedDirectory:
        body = .extendedDirectory(try ExtendedDirectory.read(from: reader))
    case .basicFile:
        body = .basicFile(try BasicFile.read(from: reader, blockSize: blockSize, blockLog: blockLog))
    case .extendedFile:
        body = .extendedFile(try ExtendedFile.read(from: reader, blockSize: blockSize, blockLog: blockLog))
    case .basicSymlink:
        body = .basicSymlink(try BasicSymlink.read(from: reader))
    case .extendedSymlink:
        body = .extendedSymlink(try ExtendedSymlink.read(from: reader))
    case .basicBlockDevice:
        body = .basicBlockDevice(try BasicDevice.read(from: reader))
    case .extendedBlockDevice:
        body = .extendedBlockDevice(try ExtendedDevice.read(from: reader))
    case .basicCharDevice:
        body = .basicCharDevice(try BasicDevice.read(from: reader))
    case .extendedCharDevice:
        body = .extendedCharDevice(try ExtendedDevice.read(from: reader))
    case .basicFifo:
        body = .basicFifo(try BasicIPC.read(from: reader))
    case .extendedFifo:
        body = .extendedFifo(try ExtendedIPC.read(from: reader))
    case .basicSocket:
        body = .basicSocket(try BasicIPC.read(from: reader))
    case .extendedSocket:
        body = .extendedSocket(try ExtendedIPC.read(from: reader))
    }

    return ParsedInode(header: header, body: body)
}
