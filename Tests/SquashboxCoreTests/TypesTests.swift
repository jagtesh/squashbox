import Testing
@testable import SquashboxCore

// MARK: - EntryType Tests

@Suite("EntryType")
struct EntryTypeTests {

    @Test("description returns raw value")
    func description() {
        #expect(EntryType.file.description == "file")
        #expect(EntryType.directory.description == "directory")
        #expect(EntryType.symlink.description == "symlink")
        #expect(EntryType.blockDevice.description == "blockDevice")
        #expect(EntryType.charDevice.description == "charDevice")
        #expect(EntryType.fifo.description == "fifo")
        #expect(EntryType.socket.description == "socket")
    }

    @Test("equality")
    func equality() {
        #expect(EntryType.file == EntryType.file)
        #expect(EntryType.file != EntryType.directory)
    }
}

// MARK: - EntryAttributes Tests

@Suite("EntryAttributes")
struct EntryAttributesTests {

    @Test("type checkers")
    func typeCheckers() {
        let fileAttrs = makeAttrs(type: .file)
        #expect(fileAttrs.isFile)
        #expect(!fileAttrs.isDirectory)
        #expect(!fileAttrs.isSymlink)

        let dirAttrs = makeAttrs(type: .directory)
        #expect(!dirAttrs.isFile)
        #expect(dirAttrs.isDirectory)
        #expect(!dirAttrs.isSymlink)

        let symlinkAttrs = makeAttrs(type: .symlink)
        #expect(!symlinkAttrs.isFile)
        #expect(!symlinkAttrs.isDirectory)
        #expect(symlinkAttrs.isSymlink)
    }

    @Test("equality")
    func equality() {
        let a = makeAttrs(type: .file, inode: 1)
        let b = makeAttrs(type: .file, inode: 1)
        let c = makeAttrs(type: .file, inode: 2)
        #expect(a == b)
        #expect(a != c)
    }
}

// MARK: - DirEntry Tests

@Suite("DirEntry")
struct DirEntryTests {

    @Test("construction and equality")
    func constructionAndEquality() {
        let entry1 = DirEntry(name: "foo.txt", inode: 42, entryType: .file)
        let entry2 = DirEntry(name: "foo.txt", inode: 42, entryType: .file)
        let entry3 = DirEntry(name: "bar.txt", inode: 43, entryType: .file)
        #expect(entry1 == entry2)
        #expect(entry1 != entry3)
        #expect(entry1.name == "foo.txt")
        #expect(entry1.inode == 42)
        #expect(entry1.entryType == .file)
    }
}

// MARK: - DirEntryBatch Tests

@Suite("DirEntryBatch")
struct DirEntryBatchTests {

    @Test("empty batch")
    func emptyBatch() {
        let batch = DirEntryBatch.empty
        #expect(batch.isEmpty)
        #expect(batch.entries.isEmpty)
        #expect(batch.cookie == 0)
    }

    @Test("batch with entries")
    func batchWithEntries() {
        let entries = [
            DirEntry(name: "a", inode: 1, entryType: .file),
            DirEntry(name: "b", inode: 2, entryType: .directory),
        ]
        let batch = DirEntryBatch(entries: entries, cookie: 2)
        #expect(!batch.isEmpty)
        #expect(batch.entries.count == 2)
        #expect(batch.cookie == 2)
    }
}

// MARK: - VolumeStats Tests

@Suite("VolumeStats")
struct VolumeStatsTests {

    @Test("construction")
    func construction() {
        let stats = VolumeStats(
            totalBytes: 1_000_000,
            totalInodes: 500,
            blockSize: 128 * 1024,
            creationTime: 1700000000
        )
        #expect(stats.totalBytes == 1_000_000)
        #expect(stats.totalInodes == 500)
        #expect(stats.blockSize == 131072)
        #expect(stats.creationTime == 1700000000)
    }
}

// MARK: - Helpers

func makeAttrs(
    type: EntryType,
    inode: InodeId = 1,
    size: UInt64 = 0,
    mode: UInt32 = 0o644
) -> EntryAttributes {
    EntryAttributes(
        inode: inode,
        entryType: type,
        size: size,
        mode: mode,
        uid: 0,
        gid: 0,
        mtimeSecs: 0,
        nlink: 1
    )
}
