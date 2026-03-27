import Testing
@testable import SquashFsSource
@testable import SquashboxCore

@Suite("SquashFsSource")
struct SquashFsSourceTests {
    // TODO: Integration tests with real SquashFS images once libsqfs is integrated.
    // These will use the same test.sqsh fixture as the Rust tests.

    @Test("stub provider resolves root")
    func stubResolvesRoot() throws {
        // This tests the stub implementation — will be replaced with real tests
        let source = try SquashFsSource(imagePath: "dummy")
        let inode = try source.resolvePath("")
        #expect(inode == rootInodeId)
    }
}
