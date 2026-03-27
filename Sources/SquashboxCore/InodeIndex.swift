import Foundation

/// Default page size for directory listing pagination.
public let defaultDirPageSize: Int = 64

// MARK: - Index Entry

/// A single node in the inode index tree.
///
/// This is the format-agnostic representation of a filesystem entry.
/// Source providers (SquashFS, ISO, etc.) populate `InodeIndex` with these
/// entries during initialization; after that, the index is immutable.
///
/// ## Relationship to the Rust implementation
/// This maps to `IndexEntry` in `squashfs.rs`, but with the `backhand_node_index`
/// and `squashfs_path` fields removed — those were format-specific and now
/// live in `SquashFsSource` as a separate lookup table.
public struct IndexEntry: Sendable {
    /// Parent inode (rootInodeId for the root's own entry).
    public let parent: InodeId
    /// Name of this entry (empty string for root).
    public let name: String
    /// Cached attributes.
    public let attributes: EntryAttributes
    /// For symlinks: the target path.
    public let symlinkTarget: String?
    /// For directories: ordered list of child inode IDs (sorted by name).
    public internal(set) var children: [InodeId]
    /// For directories: O(1) child lookup by exact name.
    public internal(set) var childrenByName: [String: InodeId]
    /// For directories: O(1) child lookup by lowercased name
    /// (for Windows/macOS case-collision prevention).
    public internal(set) var childrenByLowercase: [String: InodeId]

    public init(
        parent: InodeId,
        name: String,
        attributes: EntryAttributes,
        symlinkTarget: String? = nil
    ) {
        self.parent = parent
        self.name = name
        self.attributes = attributes
        self.symlinkTarget = symlinkTarget
        self.children = []
        self.childrenByName = [:]
        self.childrenByLowercase = [:]
    }
}

// MARK: - Inode Index

/// The in-memory inode index built from a source provider's directory tree.
///
/// This is the core data structure that enables O(1) inode lookups, O(1) path
/// resolution, and O(1) child-by-name lookups. It also handles case-collision
/// resolution for case-insensitive platforms (Windows/macOS).
///
/// ## Design Principle
/// This struct is **format-agnostic**. It doesn't know about SquashFS, ISO,
/// or any specific archive format. Source providers populate it via the
/// `Builder` pattern, then hand off an immutable `InodeIndex` to the driver.
///
/// ## Relationship to the Rust implementation
/// This extracts the indexing logic from `squashfs.rs` (lines 42-257) into
/// a reusable module. In Rust, the index was tightly coupled to backhand.
/// Here, any source provider can build and use an `InodeIndex`.
public struct InodeIndex: Sendable {
    /// The entries, keyed by inode ID.
    private var entries: [InodeId: IndexEntry]

    /// Total number of indexed inodes.
    public var count: Int { entries.count }

    /// Initialize with a pre-built entries dictionary.
    internal init(entries: [InodeId: IndexEntry]) {
        self.entries = entries
    }

    // MARK: - Lookups

    /// Get the index entry for a given inode.
    /// Throws `SquashboxError.notFound` if the inode doesn't exist.
    public func get(_ inode: InodeId) throws -> IndexEntry {
        guard let entry = entries[inode] else {
            throw SquashboxError.notFound("inode \(inode)")
        }
        return entry
    }

    /// Look up a child by exact name within a directory inode.
    /// Returns `nil` if no child with that name exists.
    public func lookupChild(parent: InodeId, name: String) throws -> DirEntry? {
        let parentEntry = try get(parent)
        guard parentEntry.attributes.isDirectory else {
            throw SquashboxError.notADirectory("inode \(parent)")
        }
        guard let childInode = parentEntry.childrenByName[name] else {
            return nil
        }
        let childEntry = try get(childInode)
        return DirEntry(
            name: childEntry.name,
            inode: childInode,
            entryType: childEntry.attributes.entryType
        )
    }

    /// List directory contents with pagination.
    ///
    /// - Parameters:
    ///   - inode: The directory inode to list.
    ///   - cookie: Pagination cursor (0 for first page).
    ///   - pageSize: Maximum entries per batch.
    /// - Returns: A `DirEntryBatch` with entries and the next cookie.
    public func listDirectory(
        _ inode: InodeId,
        cookie: UInt64,
        pageSize: Int = defaultDirPageSize
    ) throws -> DirEntryBatch {
        let entry = try get(inode)
        guard entry.attributes.isDirectory else {
            throw SquashboxError.notADirectory("inode \(inode)")
        }

        let startIndex = Int(cookie)
        guard startIndex < entry.children.count else {
            return .empty
        }

        let endIndex = min(startIndex + pageSize, entry.children.count)
        var dirEntries: [DirEntry] = []
        dirEntries.reserveCapacity(endIndex - startIndex)

        for i in startIndex..<endIndex {
            let childInode = entry.children[i]
            let childEntry = try get(childInode)
            dirEntries.append(DirEntry(
                name: childEntry.name,
                inode: childInode,
                entryType: childEntry.attributes.entryType
            ))
        }

        let nextCookie = UInt64(endIndex)
        return DirEntryBatch(entries: dirEntries, cookie: nextCookie)
    }

    /// Resolve a path string to an inode ID.
    ///
    /// Path components are separated by "/" (never "\\").
    /// Leading "/" is stripped. Empty path resolves to root.
    ///
    /// - Parameter path: The path to resolve (e.g., "usr/bin/ls").
    /// - Returns: The inode ID, or `nil` if the path doesn't exist.
    public func resolvePath(_ path: String) throws -> InodeId? {
        let normalized = path
            .replacingOccurrences(of: "\\", with: "/")
            .trimmingCharacters(in: CharacterSet(charactersIn: "/"))

        if normalized.isEmpty {
            return rootInodeId
        }

        let components = normalized.split(separator: "/")
        var currentInode = rootInodeId

        for component in components {
            let name = String(component)
            let entry = try get(currentInode)
            guard entry.attributes.isDirectory else {
                return nil
            }
            guard let childInode = entry.childrenByName[name] else {
                return nil
            }
            currentInode = childInode
        }

        return currentInode
    }
}

// MARK: - Builder

extension InodeIndex {

    /// A builder for constructing an `InodeIndex` incrementally.
    ///
    /// Source providers use this during initialization:
    /// 1. Create a `Builder`
    /// 2. Insert entries as they walk the archive's directory tree
    /// 3. Call `build()` to get an immutable `InodeIndex`
    ///
    /// ## Example
    /// ```swift
    /// var builder = InodeIndex.Builder()
    /// builder.insertRoot(attributes: rootAttrs)
    /// builder.insertEntry(inode: 2, parent: rootInodeId, name: "etc", attributes: etcAttrs)
    /// builder.insertEntry(inode: 3, parent: 2, name: "passwd", attributes: passwdAttrs)
    /// let index = builder.build()
    /// ```
    public struct Builder: Sendable {
        private var entries: [InodeId: IndexEntry] = [:]
        private var nextInode: InodeId = rootInodeId

        public init() {}

        /// The next available inode ID.
        public var nextInodeId: InodeId { nextInode }

        /// Insert the root directory entry.
        /// Must be called first, before any other `insertEntry` calls.
        @discardableResult
        public mutating func insertRoot(attributes: EntryAttributes) -> InodeId {
            let inode = nextInode
            nextInode += 1

            entries[inode] = IndexEntry(
                parent: inode, // root is its own parent
                name: "",
                attributes: attributes
            )
            return inode
        }

        /// Insert a non-root entry into the index.
        ///
        /// The `name` should already be platform-safe (call `FilenameMapping.toPlatformSafe`
        /// before inserting). Case-collision resolution is applied automatically:
        /// if a sibling with the same lowercased name already exists, the name is
        /// mangled with a numeric suffix (e.g., "README (1)").
        ///
        /// - Parameters:
        ///   - parent: The parent directory's inode ID.
        ///   - name: The entry's name (platform-safe).
        ///   - attributes: The entry's attributes.
        ///   - symlinkTarget: For symlinks, the target path.
        /// - Returns: The assigned inode ID.
        @discardableResult
        public mutating func insertEntry(
            parent: InodeId,
            name: String,
            attributes: EntryAttributes,
            symlinkTarget: String? = nil
        ) -> InodeId {
            let inode = nextInode
            nextInode += 1

            // Case-collision resolution
            var resolvedName = name
            if let parentEntry = entries[parent] {
                var lower = resolvedName.lowercased()
                var attempt = 1
                let originalName = name
                while parentEntry.childrenByLowercase[lower] != nil {
                    resolvedName = "\(originalName) (\(attempt))"
                    lower = resolvedName.lowercased()
                    attempt += 1
                }
            }

            // Create the entry with the correct inode ID in its attributes
            let adjustedAttributes = EntryAttributes(
                inode: inode,
                entryType: attributes.entryType,
                size: attributes.size,
                mode: attributes.mode,
                uid: attributes.uid,
                gid: attributes.gid,
                mtimeSecs: attributes.mtimeSecs,
                nlink: attributes.nlink
            )

            entries[inode] = IndexEntry(
                parent: parent,
                name: resolvedName,
                attributes: adjustedAttributes,
                symlinkTarget: symlinkTarget
            )

            // Add as child of parent
            if var parentEntry = entries[parent] {
                parentEntry.children.append(inode)
                parentEntry.childrenByName[resolvedName] = inode
                parentEntry.childrenByLowercase[resolvedName.lowercased()] = inode
                entries[parent] = parentEntry
            }

            return inode
        }

        /// Finalize the index: sort all children by name for consistent enumeration.
        /// Returns an immutable `InodeIndex`.
        public func build() -> InodeIndex {
            var finalEntries = entries

            // Sort children by name for consistent enumeration order
            for (inodeId, entry) in finalEntries {
                guard !entry.children.isEmpty else { continue }
                var sorted = entry
                sorted.children.sort { a, b in
                    let nameA = finalEntries[a]?.name ?? ""
                    let nameB = finalEntries[b]?.name ?? ""
                    return nameA.localizedStandardCompare(nameB) == .orderedAscending
                }
                finalEntries[inodeId] = sorted
            }

            return InodeIndex(entries: finalEntries)
        }
    }
}
