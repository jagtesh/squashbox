/// XzCompressor.swift — XZ/LZMA decompression using vendored liblzma.
///
/// Compression ID 4 in SquashFS. This is the most commonly used compression
/// for Linux distribution SquashFS images (Ubuntu, Debian, Arch, etc.).
import Foundation
import CLzma

/// XZ/LZMA compressor using vendored liblzma (0BSD license).
public struct XzCompressor: SquashFsCompressor {
    public init() {}

    public func decompress(_ input: Data, maxOutputSize: Int) throws -> Data {
        var output = [UInt8](repeating: 0, count: maxOutputSize)
        var outPos: Int = 0

        // Use lzma_stream_buffer_decode for single-shot XZ stream decompression.
        // SquashFS stores each metadata/data block as a complete XZ stream.
        var memlimit: UInt64 = UInt64.max  // no memory limit
        var inPos: Int = 0

        let ret = input.withUnsafeBytes { (srcPtr: UnsafeRawBufferPointer) -> lzma_ret in
            guard let srcBase = srcPtr.baseAddress?.assumingMemoryBound(to: UInt8.self) else {
                return LZMA_DATA_ERROR
            }
            return output.withUnsafeMutableBufferPointer { dstPtr -> lzma_ret in
                return lzma_stream_buffer_decode(
                    &memlimit,     // memlimit
                    0,             // flags
                    nil,           // allocator (use default)
                    srcBase,       // in
                    &inPos,        // in_pos
                    srcPtr.count,  // in_size
                    dstPtr.baseAddress!, // out
                    &outPos,       // out_pos
                    maxOutputSize  // out_size
                )
            }
        }

        guard ret == LZMA_OK || ret == LZMA_STREAM_END else {
            throw CompressionError.lzmaError("decompression failed: \(lzmaRetDescription(ret))")
        }

        return Data(output[..<outPos])
    }

    public func compress(_ input: Data) throws -> Data {
        // Use LZMA2 preset 6 (default for mksquashfs)
        let preset: UInt32 = 6

        // Calculate worst-case output size
        let maxSize = lzma_stream_buffer_bound(input.count)
        var output = [UInt8](repeating: 0, count: maxSize)
        var outPos: Int = 0

        let ret = input.withUnsafeBytes { (srcPtr: UnsafeRawBufferPointer) -> lzma_ret in
            guard let srcBase = srcPtr.baseAddress?.assumingMemoryBound(to: UInt8.self) else {
                return LZMA_DATA_ERROR
            }
            return output.withUnsafeMutableBufferPointer { dstPtr -> lzma_ret in
                return lzma_easy_buffer_encode(
                    preset,        // preset
                    LZMA_CHECK_CRC32, // check
                    nil,           // allocator
                    srcBase,       // in
                    srcPtr.count,  // in_size
                    dstPtr.baseAddress!, // out
                    &outPos,       // out_pos
                    maxSize        // out_size
                )
            }
        }

        guard ret == LZMA_OK || ret == LZMA_STREAM_END else {
            throw CompressionError.lzmaError("compression failed: \(lzmaRetDescription(ret))")
        }

        return Data(output[..<outPos])
    }

    /// Human-readable description for lzma_ret error codes.
    private func lzmaRetDescription(_ ret: lzma_ret) -> String {
        switch ret {
        case LZMA_OK: return "OK"
        case LZMA_STREAM_END: return "STREAM_END"
        case LZMA_NO_CHECK: return "NO_CHECK"
        case LZMA_UNSUPPORTED_CHECK: return "UNSUPPORTED_CHECK"
        case LZMA_GET_CHECK: return "GET_CHECK"
        case LZMA_MEM_ERROR: return "MEM_ERROR"
        case LZMA_MEMLIMIT_ERROR: return "MEMLIMIT_ERROR"
        case LZMA_FORMAT_ERROR: return "FORMAT_ERROR"
        case LZMA_OPTIONS_ERROR: return "OPTIONS_ERROR"
        case LZMA_DATA_ERROR: return "DATA_ERROR"
        case LZMA_BUF_ERROR: return "BUF_ERROR"
        case LZMA_PROG_ERROR: return "PROG_ERROR"
        default: return "UNKNOWN(\(ret.rawValue))"
        }
    }
}
