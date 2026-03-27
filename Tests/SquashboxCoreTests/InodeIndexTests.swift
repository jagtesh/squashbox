import Testing
@testable import SquashboxCore

@Suite("InodeIndex")
struct InodeIndexTests {

    // MARK: - Builder Tests

    @Test("builder creates root")
    func builderCreatesRoot() throws {
        var builder = InodeIndex.Builder()
        let rootId = builder.insertRoot(attributes: dirAttrs())
        let index = builder.build()

        #expect(rootId == rootInodeId)
        #expect(index.count == 1)

        let entry = try index.get(rootId)
        #expect(entry.name == "")
        #expect(entry.attributes.isDirectory)
        #expect(entry.parent == rootId)
    }

    @Test("builder assigns sequential inode IDs")
    func sequentialIds() throws {
        var builder = InodeIndex.Builder()
        let root = builder.insertRoot(attributes: dirAttrs())
        let child1 = builder.insertEntry(parent: root, name: "a", attributes: fileAttrs())
        let child2 = builder.insertEntry(parent: root, name: "b", attributes: fileAttrs())

        #expect(root == 1)
        #expect(child1 == 2)
        #expect(child2 == 3)
    }

    @Test("builder links children to parent")
    func childrenLinked() throws {
        var builder = InodeIndex.Builder()
        let root = builder.insertRoot(attributes: dirAttrs())
        let _ = builder.insertEntry(parent: root, name: "etc", attributes: dirAttrs())
        let _ = builder.insertEntry(parent: root, name: "usr", attributes: dirAttrs())
        let index = builder.build()

        let rootEntry = try index.get(root)
        #expect(rootEntry.children.count == 2)
        #expect(rootEntry.childrenByName.count == 2)
        #expect(rootEntry.childrenByName["etc"] != nil)
        #expect(rootEntry.childrenByName["usr"] != nil)
    }

    // MARK: - Case-Collision Resolution

    @Test("case-collision resolution mangles names")
    func caseCollision() throws {
        var builder = InodeIndex.Builder()
        let root = builder.insertRoot(attributes: dirAttrs())
        let id1 = builder.insertEntry(parent: root, name: "README", attributes: fileAttrs())
        let id2 = builder.insertEntry(parent: root, name: "readme", attributes: fileAttrs())
        let id3 = builder.insertEntry(parent: root, name: "Readme", attributes: fileAttrs())
        let index = builder.build()

        let entry1 = try index.get(id1)
        let entry2 = try index.get(id2)
        let entry3 = try index.get(id3)

        #expect(entry1.name == "README")
        #expect(entry2.name == "readme (1)")
        #expect(entry3.name == "Readme (2)")
    }

    // MARK: - Lookup Tests

    @Test("get existing inode")
    func getExisting() throws {
        let index = buildSimpleIndex()
        let entry = try index.get(rootInodeId)
        #expect(entry.name == "")
    }

    @Test("get missing inode throws")
    func getMissing() throws {
        let index = buildSimpleIndex()
        #expect(throws: SquashboxError.self) {
            try index.get(9999)
        }
    }

    @Test("lookup child by name")
    func lookupChild() throws {
        let index = buildSimpleIndex()
        let entry = try index.lookupChild(parent: rootInodeId, name: "etc")
        #expect(entry != nil)
        #expect(entry?.name == "etc")
        #expect(entry?.entryType == .directory)
    }

    @Test("lookup nonexistent child returns nil")
    func lookupMissingChild() throws {
        let index = buildSimpleIndex()
        let entry = try index.lookupChild(parent: rootInodeId, name: "nonexistent")
        #expect(entry == nil)
    }

    @Test("lookup child on non-directory throws")
    func lookupOnFile() throws {
        let index = buildSimpleIndex()
        // "etc/passwd" is a file — looking up a child on it should fail
        let etc = try index.lookupChild(parent: rootInodeId, name: "etc")
        #expect(etc != nil)
        let passwdDirEntry = try index.lookupChild(parent: etc!.inode, name: "passwd")
        #expect(passwdDirEntry != nil)

        #expect(throws: SquashboxError.self) {
            try index.lookupChild(parent: passwdDirEntry!.inode, name: "anything")
        }
    }

    // MARK: - Path Resolution

    @Test("resolve empty path to root")
    func resolveEmptyPath() throws {
        let index = buildSimpleIndex()
        let inode = try index.resolvePath("")
        #expect(inode == rootInodeId)
    }

    @Test("resolve root slash to root")
    func resolveRootSlash() throws {
        let index = buildSimpleIndex()
        let inode = try index.resolvePath("/")
        #expect(inode == rootInodeId)
    }

    @Test("resolve existing path")
    func resolveExistingPath() throws {
        let index = buildSimpleIndex()
        let inode = try index.resolvePath("etc/passwd")
        #expect(inode != nil)

        let entry = try index.get(inode!)
        #expect(entry.name == "passwd")
        #expect(entry.attributes.isFile)
    }

    @Test("resolve nonexistent path returns nil")
    func resolveNonexistent() throws {
        let index = buildSimpleIndex()
        let inode = try index.resolvePath("etc/nonexistent")
        #expect(inode == nil)
    }

    @Test("resolve path with backslash normalizes to forward slash")
    func resolveBackslashPath() throws {
        let index = buildSimpleIndex()
        let inode = try index.resolvePath("etc\\passwd")
        #expect(inode != nil)
    }

    // MARK: - Directory Listing

    @Test("list directory returns sorted entries")
    func listDirectory() throws {
        let index = buildSimpleIndex()
        let batch = try index.listDirectory(rootInodeId, cookie: 0)
        #expect(!batch.isEmpty)

        // Children should be sorted by name
        let names = batch.entries.map(\.name)
        #expect(names == names.sorted())
    }

    @Test("list directory pagination")
    func listDirectoryPagination() throws {
        var builder = InodeIndex.Builder()
        let root = builder.insertRoot(attributes: dirAttrs())
        for i in 0..<10 {
            builder.insertEntry(parent: root, name: "file_\(i)", attributes: fileAttrs())
        }
        let index = builder.build()

        let batch1 = try index.listDirectory(root, cookie: 0, pageSize: 3)
        #expect(batch1.entries.count == 3)
        #expect(batch1.cookie == 3)

        let batch2 = try index.listDirectory(root, cookie: batch1.cookie, pageSize: 3)
        #expect(batch2.entries.count == 3)
        #expect(batch2.cookie == 6)

        // Exhaust
        let batch4 = try index.listDirectory(root, cookie: 9, pageSize: 3)
        #expect(batch4.entries.count == 1)

        let batch5 = try index.listDirectory(root, cookie: 10, pageSize: 3)
        #expect(batch5.isEmpty)
    }

    @Test("list non-directory throws")
    func listNonDirectory() throws {
        let index = buildSimpleIndex()
        let passwd = try index.resolvePath("etc/passwd")
        #expect(passwd != nil)

        #expect(throws: SquashboxError.self) {
            try index.listDirectory(passwd!, cookie: 0)
        }
    }

    // MARK: - Helpers

    /// Builds: root → etc/ → passwd, root → usr/ → bin/ → ls
    func buildSimpleIndex() -> InodeIndex {
        var builder = InodeIndex.Builder()
        let root = builder.insertRoot(attributes: dirAttrs())
        let etc = builder.insertEntry(parent: root, name: "etc", attributes: dirAttrs())
        builder.insertEntry(parent: etc, name: "passwd", attributes: fileAttrs(size: 1024))
        let usr = builder.insertEntry(parent: root, name: "usr", attributes: dirAttrs())
        let bin = builder.insertEntry(parent: usr, name: "bin", attributes: dirAttrs())
        builder.insertEntry(parent: bin, name: "ls", attributes: fileAttrs(size: 512))
        return builder.build()
    }
}

// MARK: - Test Helpers

func dirAttrs() -> EntryAttributes {
    EntryAttributes(
        inode: 0, entryType: .directory, size: 0,
        mode: 0o755, uid: 0, gid: 0, mtimeSecs: 0, nlink: 2
    )
}

func fileAttrs(size: UInt64 = 100) -> EntryAttributes {
    EntryAttributes(
        inode: 0, entryType: .file, size: size,
        mode: 0o644, uid: 0, gid: 0, mtimeSecs: 0, nlink: 1
    )
}
