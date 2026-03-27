import Testing
@testable import SquashFsSource
@testable import SquashboxCore

@Suite("SquashFsSource")
struct SquashFsSourceTests {
    // TODO: Integration tests with real SquashFS images once test fixture path is configured.
    // The real tests will use the same test.sqsh fixture as the Rust tests.

    @Test("opening nonexistent image throws")
    func openNonexistent() throws {
        #expect(throws: SquashboxError.self) {
            try SquashFsSource(imagePath: "nonexistent_file_that_does_not_exist.sqsh")
        }
    }
}
