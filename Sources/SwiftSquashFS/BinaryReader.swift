/// BinaryReader — a lightweight cursor for reading little-endian binary data.
///
/// All SquashFS on-disk structures are little-endian. This reader wraps a
/// `Data` buffer and provides typed read methods that handle endianness,
/// bounds checking, and cursor advancement.
import Foundation

public final class BinaryReader {
    public let data: Data
    public private(set) var position: Int

    public init(data: Data) {
        self.data = data
        self.position = 0
    }

    /// Bytes remaining from the current position to the end.
    public var remaining: Int { data.count - position }

    /// Whether there are no more bytes to read.
    public var isAtEnd: Bool { position >= data.count }

    /// Seek to an absolute position.
    public func seek(to offset: Int) throws {
        guard offset >= 0 && offset <= data.count else {
            throw BinaryReaderError.seekOutOfBounds(offset: offset, size: data.count)
        }
        position = offset
    }

    /// Skip forward by `count` bytes.
    public func skip(_ count: Int) throws {
        try seek(to: position + count)
    }

    // MARK: - Integer Reads (Little-Endian)

    public func readU8() throws -> UInt8 {
        guard remaining >= 1 else { throw BinaryReaderError.unexpectedEOF(needed: 1, available: remaining) }
        let value = data[data.startIndex + position]
        position += 1
        return value
    }

    public func readU16() throws -> UInt16 {
        let bytes = try readBytes(count: 2)
        return UInt16(bytes[bytes.startIndex]) | UInt16(bytes[bytes.startIndex + 1]) << 8
    }

    public func readU32() throws -> UInt32 {
        let bytes = try readBytes(count: 4)
        let base = bytes.startIndex
        return UInt32(bytes[base])
             | UInt32(bytes[base + 1]) << 8
             | UInt32(bytes[base + 2]) << 16
             | UInt32(bytes[base + 3]) << 24
    }

    public func readU64() throws -> UInt64 {
        let bytes = try readBytes(count: 8)
        let base = bytes.startIndex
        return UInt64(bytes[base])
             | UInt64(bytes[base + 1]) << 8
             | UInt64(bytes[base + 2]) << 16
             | UInt64(bytes[base + 3]) << 24
             | UInt64(bytes[base + 4]) << 32
             | UInt64(bytes[base + 5]) << 40
             | UInt64(bytes[base + 6]) << 48
             | UInt64(bytes[base + 7]) << 56
    }

    public func readI16() throws -> Int16 {
        return Int16(bitPattern: try readU16())
    }

    public func readI32() throws -> Int32 {
        return Int32(bitPattern: try readU32())
    }

    // MARK: - Raw Bytes

    /// Read exactly `count` bytes, advancing the cursor.
    public func readBytes(count: Int) throws -> Data {
        guard count >= 0 else { throw BinaryReaderError.invalidCount(count) }
        guard remaining >= count else {
            throw BinaryReaderError.unexpectedEOF(needed: count, available: remaining)
        }
        let start = data.startIndex + position
        let slice = data[start..<start + count]
        position += count
        return slice
    }

    /// Read a null-terminated UTF-8 string (does NOT include the null in the returned string).
    public func readCString() throws -> String {
        let start = data.startIndex + position
        guard let nullIndex = data[start...].firstIndex(of: 0) else {
            throw BinaryReaderError.unterminatedString
        }
        let stringData = data[start..<nullIndex]
        position = nullIndex - data.startIndex + 1 // skip past the null
        guard let string = String(data: stringData, encoding: .utf8) else {
            throw BinaryReaderError.invalidUTF8
        }
        return string
    }

    /// Read `count` bytes and interpret as a UTF-8 string (no null terminator expected).
    public func readString(count: Int) throws -> String {
        let bytes = try readBytes(count: count)
        guard let string = String(data: bytes, encoding: .utf8) else {
            throw BinaryReaderError.invalidUTF8
        }
        return string
    }

    /// Create a sub-reader over a slice of the data, starting at the current position.
    public func subReader(count: Int) throws -> BinaryReader {
        let bytes = try readBytes(count: count)
        return BinaryReader(data: Data(bytes))
    }
}

// MARK: - Errors

public enum BinaryReaderError: Error, CustomStringConvertible {
    case unexpectedEOF(needed: Int, available: Int)
    case seekOutOfBounds(offset: Int, size: Int)
    case invalidCount(Int)
    case unterminatedString
    case invalidUTF8

    public var description: String {
        switch self {
        case .unexpectedEOF(let needed, let available):
            return "unexpected EOF: needed \(needed) bytes, only \(available) available"
        case .seekOutOfBounds(let offset, let size):
            return "seek to \(offset) out of bounds (size \(size))"
        case .invalidCount(let count):
            return "invalid byte count: \(count)"
        case .unterminatedString:
            return "unterminated C string"
        case .invalidUTF8:
            return "invalid UTF-8 data"
        }
    }
}
