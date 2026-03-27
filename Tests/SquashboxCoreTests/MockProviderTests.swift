import Foundation
import Testing
@testable import SquashboxCore

// MARK: - Mock Source Provider

/// A mock VirtualFsProvider for testing the protocol contract.
/// Uses InodeIndex directly — no format-specific dependencies.
final class MockSource: VirtualFsProvider, @unchecked Sendable {
    let index: InodeIndex
    let fileData: [InodeId: Data]

    init(index: InodeIndex, fileData: [InodeId: Data] = [:]) {
        self.index = index
        self.fileData = fileData
    }

    func resolvePath(_ path: String) throws -> InodeId? {
        try index.resolvePath(path)
    }

    func getAttributes(_ inode: InodeId) throws -> EntryAttributes {
        let entry = try index.get(inode)
        return entry.attributes
    }

    func listDirectory(_ inode: InodeId, cookie: UInt64) throws -> DirEntryBatch {
        try index.listDirectory(inode, cookie: cookie)
    }

    func lookup(parent: InodeId, name: String) throws -> DirEntry? {
        try index.lookupChild(parent: parent, name: name)
    }

    func readFile(_ inode: InodeId, offset: UInt64, length: UInt64) throws -> Data {
        guard let data = fileData[inode] else {
            throw SquashboxError.notAFile("inode \(inode)")
        }
        let start = min(Int(offset), data.count)
        let end = min(start + Int(length), data.count)
        return data[start..<end]
    }

    func readSymlink(_ inode: InodeId) throws -> String {
        let entry = try index.get(inode)
        guard let target = entry.symlinkTarget else {
            throw SquashboxError.notASymlink("inode \(inode)")
        }
        return target
    }

    func volumeStats() throws -> VolumeStats {
        VolumeStats(totalBytes: 1000, totalInodes: UInt64(index.count), blockSize: 4096, creationTime: 0)
    }
}

// MARK: - Mock Provider Tests

@Suite("MockProvider (protocol contract)")
struct MockProviderTests {

    @Test("protocol can be used as existential")
    func protocolExistential() throws {
        let provider: any VirtualFsProvider = buildMockSource()
        let inode = try provider.resolvePath("")
        #expect(inode == rootInodeId)
    }

    @Test("resolvePath returns root for empty path")
    func resolveRoot() throws {
        let source = buildMockSource()
        let inode = try source.resolvePath("")
        #expect(inode == rootInodeId)
    }

    @Test("resolvePath finds existing file")
    func resolveFile() throws {
        let source = buildMockSource()
        let inode = try source.resolvePath("hello.txt")
        #expect(inode != nil)
    }

    @Test("lookup finds existing child")
    func lookupExisting() throws {
        let source = buildMockSource()
        let entry = try source.lookup(parent: rootInodeId, name: "hello.txt")
        #expect(entry != nil)
        #expect(entry?.name == "hello.txt")
        #expect(entry?.entryType == .file)
    }

    @Test("lookup returns nil for missing child")
    func lookupMissing() throws {
        let source = buildMockSource()
        let entry = try source.lookup(parent: rootInodeId, name: "nonexistent")
        #expect(entry == nil)
    }

    @Test("readFile returns data")
    func readFile() throws {
        let source = buildMockSource()
        let inode = try source.resolvePath("hello.txt")!
        let data = try source.readFile(inode, offset: 0, length: 100)
        #expect(String(data: data, encoding: .utf8) == "hello, world!")
    }

    @Test("readFile with offset")
    func readFileOffset() throws {
        let source = buildMockSource()
        let inode = try source.resolvePath("hello.txt")!
        let data = try source.readFile(inode, offset: 7, length: 5)
        #expect(String(data: data, encoding: .utf8) == "world")
    }

    @Test("readFile on directory throws")
    func readFileOnDir() throws {
        let source = buildMockSource()
        #expect(throws: SquashboxError.self) {
            try source.readFile(rootInodeId, offset: 0, length: 1)
        }
    }

    @Test("listDirectory returns entries")
    func listDirectory() throws {
        let source = buildMockSource()
        let batch = try source.listDirectory(rootInodeId, cookie: 0)
        #expect(batch.entries.count == 3)  // hello.txt, link, subdir
    }

    @Test("readSymlink returns target")
    func readSymlink() throws {
        let source = buildMockSource()
        let inode = try source.resolvePath("link")!
        let target = try source.readSymlink(inode)
        #expect(target == "hello.txt")
    }

    @Test("readSymlink on non-symlink throws")
    func readSymlinkOnFile() throws {
        let source = buildMockSource()
        let inode = try source.resolvePath("hello.txt")!
        #expect(throws: SquashboxError.self) {
            try source.readSymlink(inode)
        }
    }

    @Test("checkAccess defaults: read allowed, write denied")
    func checkAccessDefaults() throws {
        let source = buildMockSource()
        let readAllowed = try source.checkAccess(rootInodeId, mask: 0o444)
        #expect(readAllowed)
        let writeAllowed = try source.checkAccess(rootInodeId, mask: 0o222)
        #expect(!writeAllowed)
    }

    @Test("volumeStats returns data")
    func volumeStats() throws {
        let source = buildMockSource()
        let stats = try source.volumeStats()
        #expect(stats.totalInodes == 4)  // root + hello.txt + subdir + link
    }

    // MARK: - Helpers

    /// root → hello.txt ("hello, world!"), root → subdir/, root → link → "hello.txt"
    func buildMockSource() -> MockSource {
        var builder = InodeIndex.Builder()
        let root = builder.insertRoot(attributes: EntryAttributes(
            inode: 0, entryType: .directory, size: 0,
            mode: 0o755, uid: 0, gid: 0, mtimeSecs: 0, nlink: 2
        ))
        let fileInode = builder.insertEntry(
            parent: root,
            name: "hello.txt",
            attributes: EntryAttributes(
                inode: 0, entryType: .file, size: 13,
                mode: 0o644, uid: 0, gid: 0, mtimeSecs: 0, nlink: 1
            )
        )
        let _ = builder.insertEntry(
            parent: root,
            name: "subdir",
            attributes: EntryAttributes(
                inode: 0, entryType: .directory, size: 0,
                mode: 0o755, uid: 0, gid: 0, mtimeSecs: 0, nlink: 2
            )
        )
        let linkInode = builder.insertEntry(
            parent: root,
            name: "link",
            attributes: EntryAttributes(
                inode: 0, entryType: .symlink, size: 0,
                mode: 0o777, uid: 0, gid: 0, mtimeSecs: 0, nlink: 1
            ),
            symlinkTarget: "hello.txt"
        )

        let index = builder.build()
        let fileData: [InodeId: Data] = [
            fileInode: "hello, world!".data(using: .utf8)!,
        ]
        return MockSource(index: index, fileData: fileData)
    }
}
