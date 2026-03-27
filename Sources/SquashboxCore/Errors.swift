import Foundation

/// Errors that can occur in the Squashbox virtual filesystem.
///
/// These map to the `CoreError` variants from the Rust implementation,
/// but use Swift's native `Error` protocol instead of `thiserror` macros.
public enum SquashboxError: Error, Sendable, Equatable, CustomStringConvertible {
    /// The requested path or inode was not found.
    case notFound(String = "not found")
    /// The inode is not a directory (but a directory operation was attempted).
    case notADirectory(String = "not a directory")
    /// The inode is not a regular file.
    case notAFile(String = "not a file")
    /// The inode is not a symbolic link.
    case notASymlink(String = "not a symlink")
    /// An I/O error occurred while reading the archive.
    case io(String)
    /// The archive format is invalid or corrupt.
    case formatError(String)
    /// The requested operation is not supported by this provider.
    case notSupported(String = "operation not supported")
    /// The filesystem is read-only (all SquashFS-based providers are).
    case readOnly

    public var description: String {
        switch self {
        case .notFound(let msg):      return "not found: \(msg)"
        case .notADirectory(let msg): return "not a directory: \(msg)"
        case .notAFile(let msg):      return "not a file: \(msg)"
        case .notASymlink(let msg):   return "not a symlink: \(msg)"
        case .io(let msg):            return "I/O error: \(msg)"
        case .formatError(let msg):   return "format error: \(msg)"
        case .notSupported(let msg):  return "not supported: \(msg)"
        case .readOnly:               return "filesystem is read-only"
        }
    }
}
