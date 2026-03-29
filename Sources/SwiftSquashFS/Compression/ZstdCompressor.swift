/// ZstdCompressor.swift — Zstandard compression for SquashFS.
///
/// Uses the C zstd library via the CZstd bridge module.
import Foundation
import CZstd

/// Zstd compressor using system or vendored libzstd.
public struct ZstdCompressor: SquashFsCompressor {
    /// Default compression level for writing.
    public var compressionLevel: Int32 = 3

    public init() {}

    public func decompress(_ input: Data, maxOutputSize: Int) throws -> Data {
        return try input.withUnsafeBytes { (srcPtr: UnsafeRawBufferPointer) -> Data in
            guard let srcBase = srcPtr.baseAddress else {
                throw CompressionError.invalidInput
            }

            // Get the decompressed size if it's in the frame header
            let frameSize = ZSTD_getFrameContentSize(srcBase, srcPtr.count)

            let outputCapacity: Int
            if frameSize != ZSTD_CONTENTSIZE_UNKNOWN && frameSize != ZSTD_CONTENTSIZE_ERROR {
                outputCapacity = Int(frameSize)
            } else {
                outputCapacity = maxOutputSize
            }

            var output = [UInt8](repeating: 0, count: outputCapacity)

            let result = output.withUnsafeMutableBufferPointer { dstPtr -> Int in
                return ZSTD_decompress(
                    dstPtr.baseAddress!,
                    dstPtr.count,
                    srcBase,
                    srcPtr.count
                )
            }

            if ZSTD_isError(result) != 0 {
                let errName = String(cString: ZSTD_getErrorName(result))
                throw CompressionError.zstdError("decompression failed: \(errName)")
            }

            return Data(output[0..<result])
        }
    }

    public func compress(_ input: Data) throws -> Data {
        return try input.withUnsafeBytes { (srcPtr: UnsafeRawBufferPointer) -> Data in
            guard let srcBase = srcPtr.baseAddress else {
                throw CompressionError.invalidInput
            }

            let bound = ZSTD_compressBound(srcPtr.count)
            var output = [UInt8](repeating: 0, count: bound)

            let result = output.withUnsafeMutableBufferPointer { dstPtr -> Int in
                return ZSTD_compress(
                    dstPtr.baseAddress!,
                    dstPtr.count,
                    srcBase,
                    srcPtr.count,
                    Int32(compressionLevel)
                )
            }

            if ZSTD_isError(result) != 0 {
                let errName = String(cString: ZSTD_getErrorName(result))
                throw CompressionError.zstdError("compression failed: \(errName)")
            }

            return Data(output[0..<result])
        }
    }
}
