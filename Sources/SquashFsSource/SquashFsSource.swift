import Foundation
import SquashboxCore
import SquashboxUniFFI

/// A SquashFS-backed implementation of `VirtualFsProvider`.
///
/// Uses backhand (Rust) via UniFFI-generated bindings for all SquashFS parsing.
/// The Rust side handles compression (gzip, xz, zstd, lz4, lzo) natively.
///
/// UniFFI types live in the `SquashboxUniFFI` module. SquashboxCore types
/// are used via their fully qualified names where collisions exist.
public final class SquashFsSource: @unchecked Sendable {
    /// The UniFFI-generated Rust provider (thread-safe, Arc'd internally).
    private let provider: SquashboxUniFFI.SquashFsProvider

    /// Open a SquashFS image at the given path.
    public init(imagePath: String) throws {
        self.provider = try SquashboxUniFFI.SquashFsProvider(imagePath: imagePath)
    }
}

// MARK: - VirtualFsProvider Conformance

extension SquashFsSource: VirtualFsProvider {
    public func resolvePath(_ path: String) throws -> InodeId? {
        do {
            return try provider.resolvePath(path: path)
        } catch let error as SquashboxUniFFI.CoreError {
            throw mapCoreError(error)
        }
    }

    public func getAttributes(_ inode: InodeId) throws -> SquashboxCore.EntryAttributes {
        do {
            let attrs = try provider.getAttributes(inode: inode)
            return SquashboxCore.EntryAttributes(
                inode: attrs.inode,
                entryType: mapEntryType(attrs.entryType),
                size: attrs.size,
                mode: attrs.mode,
                uid: attrs.uid,
                gid: attrs.gid,
                mtimeSecs: UInt32(truncatingIfNeeded: attrs.mtimeSecs),
                nlink: attrs.nlink
            )
        } catch let error as SquashboxUniFFI.CoreError {
            throw mapCoreError(error)
        }
    }

    public func listDirectory(_ inode: InodeId, cookie: UInt64) throws -> SquashboxCore.DirEntryBatch {
        do {
            let batch = try provider.listDirectory(inode: inode, cookie: cookie)
            let entries = batch.entries.map { entry in
                SquashboxCore.DirEntry(
                    name: entry.name,
                    inode: entry.attributes.inode,
                    entryType: mapEntryType(entry.attributes.entryType)
                )
            }
            return SquashboxCore.DirEntryBatch(entries: entries, cookie: batch.nextCookie)
        } catch let error as SquashboxUniFFI.CoreError {
            throw mapCoreError(error)
        }
    }

    public func lookup(parent: InodeId, name: String) throws -> SquashboxCore.DirEntry? {
        do {
            guard let entry = try provider.lookup(parentInode: parent, name: name) else {
                return nil
            }
            return SquashboxCore.DirEntry(
                name: entry.name,
                inode: entry.attributes.inode,
                entryType: mapEntryType(entry.attributes.entryType)
            )
        } catch let error as SquashboxUniFFI.CoreError {
            throw mapCoreError(error)
        }
    }

    public func readFile(_ inode: InodeId, offset: UInt64, length: UInt64) throws -> Data {
        do {
            let bytes = try provider.readFile(inode: inode, offset: offset, length: length)
            return Data(bytes)
        } catch let error as SquashboxUniFFI.CoreError {
            throw mapCoreError(error)
        }
    }

    public func readSymlink(_ inode: InodeId) throws -> String {
        do {
            return try provider.readSymlink(inode: inode)
        } catch let error as SquashboxUniFFI.CoreError {
            throw mapCoreError(error)
        }
    }

    public func volumeStats() throws -> SquashboxCore.VolumeStats {
        do {
            let stats = try provider.volumeStats()
            return SquashboxCore.VolumeStats(
                totalBytes: stats.totalBytes,
                totalInodes: stats.totalInodes,
                blockSize: stats.blockSize,
                creationTime: 0  // Not directly available from Rust VolumeStats
            )
        } catch let error as SquashboxUniFFI.CoreError {
            throw mapCoreError(error)
        }
    }
}

// MARK: - Type Mapping Helpers

/// Map UniFFI-generated EntryType → SquashboxCore.EntryType
private func mapEntryType(_ et: SquashboxUniFFI.EntryType) -> SquashboxCore.EntryType {
    switch et {
    case .file: return .file
    case .directory: return .directory
    case .symlink: return .symlink
    case .blockDevice: return .blockDevice
    case .charDevice: return .charDevice
    }
}

/// Map UniFFI-generated CoreError → SquashboxError
private func mapCoreError(_ error: SquashboxUniFFI.CoreError) -> SquashboxError {
    switch error {
    case .NotFound(let msg):
        return .notFound(msg)
    case .NotADirectory(let inode):
        return .notADirectory("inode \(inode)")
    case .NotAFile(let inode):
        return .notAFile("inode \(inode)")
    case .NotASymlink(let inode):
        return .notASymlink("inode \(inode)")
    case .Io(let msg):
        return .io(msg)
    case .SquashFs(let msg):
        return .formatError(msg)
    case .NotSupported:
        return .notSupported("operation")
    case .ReadOnly:
        return .readOnly
    }
}
