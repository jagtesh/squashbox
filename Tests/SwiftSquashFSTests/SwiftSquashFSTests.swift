import Testing
@testable import SwiftSquashFS

@Suite("SwiftSquashFS")
struct SwiftSquashFSTests {
    @Test("BinaryReader reads little-endian integers")
    func binaryReaderIntegers() throws {
        // 0x04030201 in little-endian
        let data = Data([0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08])
        let reader = BinaryReader(data: data)

        let u16val = try reader.readU16()
        #expect(u16val == 0x0201)

        let u16val2 = try reader.readU16()
        #expect(u16val2 == 0x0403)

        let u32val = try reader.readU32()
        #expect(u32val == 0x08070605)

        #expect(reader.isAtEnd)
    }

    @Test("BinaryReader reads u64")
    func binaryReaderU64() throws {
        let data = Data([0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08])
        let reader = BinaryReader(data: data)
        let val = try reader.readU64()
        #expect(val == 0x0807060504030201)
    }

    @Test("BinaryReader bounds checking")
    func binaryReaderBounds() throws {
        let data = Data([0x01, 0x02])
        let reader = BinaryReader(data: data)
        #expect(throws: BinaryReaderError.self) {
            _ = try reader.readU32()
        }
    }

    @Test("BinaryWriter produces little-endian bytes")
    func binaryWriterIntegers() throws {
        let writer = BinaryWriter()
        writer.writeU16(0x0201)
        writer.writeU32(0x06050403)
        #expect(writer.data == Data([0x01, 0x02, 0x03, 0x04, 0x05, 0x06]))
    }

    @Test("Superblock magic validation")
    func superblockMagic() throws {
        let data = Data([0x00, 0x00, 0x00, 0x00]) + Data(repeating: 0, count: 92)
        let reader = BinaryReader(data: data)
        #expect(throws: SquashFsFormatError.self) {
            _ = try Superblock.read(from: reader)
        }
    }

    @Test("Superblock round-trip")
    func superblockRoundTrip() throws {
        let sb = Superblock(
            magic: squashfsMagic,
            inodeCount: 100,
            modificationTime: 1700000000,
            blockSize: 131072,
            fragmentEntryCount: 10,
            compressionId: .gzip,
            blockLog: 17,
            flags: SuperblockFlags(),
            idCount: 2,
            versionMajor: 4,
            versionMinor: 0,
            rootInodeRef: 0x0000_0000_0100_0020,
            bytesUsed: 50000,
            idTableStart: 40000,
            xattrTableStart: squashfsNotPresent,
            inodeTableStart: 10000,
            directoryTableStart: 20000,
            fragmentTableStart: 30000,
            exportTableStart: squashfsNotPresent
        )

        // Write
        let writer = BinaryWriter()
        sb.write(to: writer)
        #expect(writer.data.count == 96)

        // Read back
        let reader = BinaryReader(data: writer.data)
        let sb2 = try Superblock.read(from: reader)

        #expect(sb2.inodeCount == 100)
        #expect(sb2.blockSize == 131072)
        #expect(sb2.compressionId == .gzip)
        #expect(sb2.rootInodeRef == 0x0000_0000_0100_0020)
        #expect(sb2.fragmentTableStart == 30000)
        #expect(sb2.xattrTableStart == squashfsNotPresent)
    }

    @Test("DataBlock size encoding")
    func dataBlockEncoding() throws {
        // Compressed block of 1000 bytes
        let compressed: UInt32 = 1000
        #expect(dataBlockOnDiskSize(compressed) == 1000)
        #expect(!dataBlockIsUncompressed(compressed))
        #expect(!dataBlockIsSparse(compressed))

        // Uncompressed block of 1000 bytes
        let uncompressed: UInt32 = 1000 | dataBlockUncompressedFlag
        #expect(dataBlockOnDiskSize(uncompressed) == 1000)
        #expect(dataBlockIsUncompressed(uncompressed))

        // Sparse block
        #expect(dataBlockIsSparse(0))
    }

    @Test("InodeType discriminator values match spec")
    func inodeTypeValues() throws {
        #expect(InodeType.basicDirectory.rawValue == 1)
        #expect(InodeType.basicFile.rawValue == 2)
        #expect(InodeType.basicSymlink.rawValue == 3)
        #expect(InodeType.extendedDirectory.rawValue == 8)
        #expect(InodeType.extendedFile.rawValue == 9)
        #expect(InodeType.extendedSocket.rawValue == 14)
    }

    @Test("GzipCompressor round-trip")
    func gzipRoundTrip() throws {
        let compressor = GzipCompressor()
        let original = Data("Hello, SquashFS world! This is a test of zlib compression.".utf8)

        let compressed = try compressor.compress(original)
        let decompressed = try compressor.decompress(compressed, maxOutputSize: 1024)

        #expect(decompressed == original)
    }

    @Test("ZstdCompressor round-trip")
    func zstdRoundTrip() throws {
        let compressor = ZstdCompressor()
        let original = Data("Hello, SquashFS world! This is a test of zstd compression.".utf8)

        let compressed = try compressor.compress(original)
        let decompressed = try compressor.decompress(compressed, maxOutputSize: 1024)

        #expect(decompressed == original)
    }
}
