/// BinaryWriter — a cursor for writing little-endian binary data.
///
/// Counterpart to `BinaryReader`. Accumulates bytes into a `Data` buffer.
import Foundation

public final class BinaryWriter {
    public private(set) var data: Data

    public init(capacity: Int = 256) {
        self.data = Data()
        self.data.reserveCapacity(capacity)
    }

    /// Current write position (always at end).
    public var position: Int { data.count }

    // MARK: - Integer Writes (Little-Endian)

    public func writeU8(_ value: UInt8) {
        data.append(value)
    }

    public func writeU16(_ value: UInt16) {
        data.append(UInt8(value & 0xFF))
        data.append(UInt8((value >> 8) & 0xFF))
    }

    public func writeU32(_ value: UInt32) {
        data.append(UInt8(value & 0xFF))
        data.append(UInt8((value >> 8) & 0xFF))
        data.append(UInt8((value >> 16) & 0xFF))
        data.append(UInt8((value >> 24) & 0xFF))
    }

    public func writeU64(_ value: UInt64) {
        data.append(UInt8(value & 0xFF))
        data.append(UInt8((value >> 8) & 0xFF))
        data.append(UInt8((value >> 16) & 0xFF))
        data.append(UInt8((value >> 24) & 0xFF))
        data.append(UInt8((value >> 32) & 0xFF))
        data.append(UInt8((value >> 40) & 0xFF))
        data.append(UInt8((value >> 48) & 0xFF))
        data.append(UInt8((value >> 56) & 0xFF))
    }

    public func writeI16(_ value: Int16) {
        writeU16(UInt16(bitPattern: value))
    }

    public func writeI32(_ value: Int32) {
        writeU32(UInt32(bitPattern: value))
    }

    // MARK: - Raw Bytes

    public func writeBytes(_ bytes: Data) {
        data.append(bytes)
    }

    public func writeBytes(_ bytes: [UInt8]) {
        data.append(contentsOf: bytes)
    }

    /// Write `count` zero bytes.
    public func writeZeros(_ count: Int) {
        data.append(contentsOf: [UInt8](repeating: 0, count: count))
    }

    /// Write a string as raw UTF-8 bytes (no null terminator).
    public func writeString(_ string: String) {
        data.append(contentsOf: Array(string.utf8))
    }

    /// Overwrite bytes at a specific position without advancing.
    public func patchU32(at offset: Int, value: UInt32) {
        data[offset] = UInt8(value & 0xFF)
        data[offset + 1] = UInt8((value >> 8) & 0xFF)
        data[offset + 2] = UInt8((value >> 16) & 0xFF)
        data[offset + 3] = UInt8((value >> 24) & 0xFF)
    }

    public func patchU64(at offset: Int, value: UInt64) {
        data[offset] = UInt8(value & 0xFF)
        data[offset + 1] = UInt8((value >> 8) & 0xFF)
        data[offset + 2] = UInt8((value >> 16) & 0xFF)
        data[offset + 3] = UInt8((value >> 24) & 0xFF)
        data[offset + 4] = UInt8((value >> 32) & 0xFF)
        data[offset + 5] = UInt8((value >> 40) & 0xFF)
        data[offset + 6] = UInt8((value >> 48) & 0xFF)
        data[offset + 7] = UInt8((value >> 56) & 0xFF)
    }
}
