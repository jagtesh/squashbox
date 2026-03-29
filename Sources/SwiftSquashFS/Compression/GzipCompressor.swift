/// GzipCompressor.swift — zlib (deflate) compression for SquashFS.
///
/// SquashFS uses raw zlib/deflate, not gzip envelope. We call the C zlib
/// functions directly via the CZlib bridge module.
import Foundation
import CZlib

/// Gzip/zlib compressor using system zlib.
public struct GzipCompressor: SquashFsCompressor {
    /// Default compression level for writing.
    public var compressionLevel: Int32 = 9 // Z_BEST_COMPRESSION

    public init() {}

    public func decompress(_ input: Data, maxOutputSize: Int) throws -> Data {
        return try input.withUnsafeBytes { (srcPtr: UnsafeRawBufferPointer) -> Data in
            guard let srcBase = srcPtr.baseAddress else {
                throw CompressionError.invalidInput
            }

            var outputSize = UInt32(maxOutputSize)
            var output = [UInt8](repeating: 0, count: maxOutputSize)

            let result = output.withUnsafeMutableBufferPointer { dstPtr -> Int32 in
                return uncompress(
                    dstPtr.baseAddress!,
                    &outputSize,
                    srcBase.assumingMemoryBound(to: UInt8.self),
                    UInt32(srcPtr.count)
                )
            }

            guard result == Z_OK else {
                throw CompressionError.decompressionFailed(zlibError: result)
            }

            return Data(output[0..<Int(outputSize)])
        }
    }

    public func compress(_ input: Data) throws -> Data {
        return try input.withUnsafeBytes { (srcPtr: UnsafeRawBufferPointer) -> Data in
            guard let srcBase = srcPtr.baseAddress else {
                throw CompressionError.invalidInput
            }

            let bound = compressBound(UInt32(srcPtr.count))
            var outputSize = bound
            var output = [UInt8](repeating: 0, count: Int(bound))

            let result = output.withUnsafeMutableBufferPointer { dstPtr -> Int32 in
                return compress2(
                    dstPtr.baseAddress!,
                    &outputSize,
                    srcBase.assumingMemoryBound(to: UInt8.self),
                    UInt32(srcPtr.count),
                    compressionLevel
                )
            }

            guard result == Z_OK else {
                throw CompressionError.compressionFailed(zlibError: result)
            }

            return Data(output[0..<Int(outputSize)])
        }
    }
}

/// Compression errors.
public enum CompressionError: Error, CustomStringConvertible {
    case invalidInput
    case decompressionFailed(zlibError: Int32)
    case compressionFailed(zlibError: Int32)
    case zstdError(String)
    case lzmaError(String)

    public var description: String {
        switch self {
        case .invalidInput:
            return "invalid compression input"
        case .decompressionFailed(let code):
            return "zlib decompression failed (error \(code))"
        case .compressionFailed(let code):
            return "zlib compression failed (error \(code))"
        case .zstdError(let msg):
            return "zstd error: \(msg)"
        case .lzmaError(let msg):
            return "lzma error: \(msg)"
        }
    }
}
