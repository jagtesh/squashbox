import Foundation

/// The contract that every source provider must fulfill.
///
/// A `VirtualFsProvider` reads from an archive format (SquashFS, ISO, ZIP, etc.)
/// and exposes its contents as a virtual filesystem tree. Platform drivers
/// (`ProjFsDriver`, `FSKitDriver`, etc.) consume this protocol to project
/// the virtual tree onto the OS.
///
/// All methods are synchronous and may be called from any thread.
/// Implementors must be thread-safe (hence the `Sendable` requirement).
///
/// ## Relationship to the Rust implementation
/// This protocol maps to the `VirtualFsProvider` trait in `provider.rs`.
/// The key difference: Swift protocols are inherently object-safe, so there's
/// no need for the `Arc<dyn VirtualFsProvider>` pattern from Rust.
public protocol VirtualFsProvider: Sendable {

    /// Resolve a path string (e.g. "usr/bin/ls") to an inode ID.
    /// Returns `nil` if the path does not exist.
    /// Path components are separated by "/" (never "\\").
    func resolvePath(_ path: String) throws -> InodeId?

    /// Get the attributes (metadata) for a given inode.
    func getAttributes(_ inode: InodeId) throws -> EntryAttributes

    /// List directory contents starting from the given `cookie`.
    /// Pass `cookie: 0` for the first batch.
    /// Returns a `DirEntryBatch` with entries and a new cookie for the next batch.
    /// An empty batch signals end-of-directory.
    func listDirectory(_ inode: InodeId, cookie: UInt64) throws -> DirEntryBatch

    /// Look up a single child by name within a parent directory.
    /// Returns `nil` if no child with that name exists.
    func lookup(parent: InodeId, name: String) throws -> DirEntry?

    /// Read file data at the given byte offset and length.
    /// Returns the data read (may be shorter than `length` near EOF).
    func readFile(_ inode: InodeId, offset: UInt64, length: UInt64) throws -> Data

    /// Read the target of a symbolic link.
    func readSymlink(_ inode: InodeId) throws -> String

    /// List extended attribute names for an inode.
    /// Returns an empty array if no xattrs are present or if unsupported.
    func listXattrs(_ inode: InodeId) throws -> [String]

    /// Get the value of a named extended attribute.
    func getXattr(_ inode: InodeId, name: String) throws -> Data

    /// Check whether the given access mask is permitted for an inode.
    /// For read-only filesystems, write/execute checks may always fail.
    func checkAccess(_ inode: InodeId, mask: UInt32) throws -> Bool

    /// Get aggregate volume statistics.
    func volumeStats() throws -> VolumeStats
}

// MARK: - Default Implementations

/// Default implementations for optional protocol methods.
/// Sources that don't support xattrs or access checks get reasonable defaults.
extension VirtualFsProvider {
    /// Collect all directory entries for an inode, handling pagination.
    ///
    /// The Rust `list_directory` returns `cookie = 0` to signal "no more entries".
    /// This helper drains all pages into a single array.
    public func allEntries(_ inode: InodeId) throws -> [DirEntry] {
        var result: [DirEntry] = []
        var cookie: UInt64 = 0
        repeat {
            let batch = try listDirectory(inode, cookie: cookie)
            result.append(contentsOf: batch.entries)
            cookie = batch.cookie
        } while cookie != 0
        return result
    }

    public func listXattrs(_ inode: InodeId) throws -> [String] {
        return []
    }

    public func getXattr(_ inode: InodeId, name: String) throws -> Data {
        throw SquashboxError.notSupported("extended attributes")
    }

    public func checkAccess(_ inode: InodeId, mask: UInt32) throws -> Bool {
        // Read-only filesystem: allow read, deny write
        let writeFlag: UInt32 = 0o222
        return (mask & writeFlag) == 0
    }
}
