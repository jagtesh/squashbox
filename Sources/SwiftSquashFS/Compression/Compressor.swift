/// Compressor.swift — Protocol and dispatch for SquashFS compression.
import Foundation

/// Protocol for SquashFS block compression/decompression.
public protocol SquashFsCompressor: Sendable {
    /// Decompress data. `maxOutputSize` is a hint for buffer allocation.
    func decompress(_ input: Data, maxOutputSize: Int) throws -> Data

    /// Compress data. Returns compressed bytes (may be larger than input
    /// if data is incompressible).
    func compress(_ input: Data) throws -> Data
}

/// Create the appropriate compressor for a given compression ID.
public func makeCompressor(for id: CompressionId) throws -> SquashFsCompressor {
    switch id {
    case .none:
        return NoCompressor()
    case .gzip:
        return GzipCompressor()
    case .xz:
        return XzCompressor()
    case .zstd:
        return ZstdCompressor()
    default:
        throw SquashFsFormatError.unsupportedCompression(id.rawValue)
    }
}

/// Pass-through "compressor" for uncompressed data.
struct NoCompressor: SquashFsCompressor {
    func decompress(_ input: Data, maxOutputSize: Int) throws -> Data {
        return input
    }

    func compress(_ input: Data) throws -> Data {
        return input
    }
}
