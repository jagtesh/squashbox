import Testing
@testable import SquashboxCore

@Suite("FilenameMapping")
struct FilenameMappingTests {

    // NOTE: On non-Windows platforms, toPlatformSafe/fromPlatformSafe are
    // identity functions. These tests verify the mapping logic which is
    // active on Windows.

    @Test("round-trip is lossless")
    func roundTrip() {
        let names = [
            "normal_file.txt",
            "hello world",
            "file-with-dashes",
            "UPPERCASE",
            "",
        ]
        for name in names {
            let safe = FilenameMapping.toPlatformSafe(name)
            let restored = FilenameMapping.fromPlatformSafe(safe)
            #expect(restored == name, "Round-trip failed for: \(name)")
        }
    }

    @Test("identity for names without special chars")
    func identityForSafe() {
        let safe = FilenameMapping.toPlatformSafe("hello.txt")
        #expect(safe == "hello.txt")
    }

    @Test("needsMapping returns false for safe names")
    func needsMappingSafe() {
        #expect(!FilenameMapping.needsMapping("hello.txt"))
        #expect(!FilenameMapping.needsMapping("a/b/c"))  // forward slash is fine
    }

    #if os(Windows)
    @Test("maps backslash to PUA on Windows")
    func mapsBackslash() {
        let result = FilenameMapping.toPlatformSafe("file\\name")
        #expect(result != "file\\name")
        #expect(result.contains("\u{F05C}"))
        #expect(FilenameMapping.fromPlatformSafe(result) == "file\\name")
    }

    @Test("maps colon to PUA on Windows")
    func mapsColon() {
        let result = FilenameMapping.toPlatformSafe("file:name")
        #expect(result.contains("\u{F03A}"))
        #expect(FilenameMapping.fromPlatformSafe(result) == "file:name")
    }

    @Test("maps multiple illegal chars on Windows")
    func mapsMultiple() {
        let result = FilenameMapping.toPlatformSafe("a*b?c<d>e|f")
        #expect(!result.contains("*"))
        #expect(!result.contains("?"))
        #expect(!result.contains("<"))
        #expect(!result.contains(">"))
        #expect(!result.contains("|"))
        #expect(FilenameMapping.fromPlatformSafe(result) == "a*b?c<d>e|f")
    }

    @Test("needsMapping returns true for illegal chars on Windows")
    func needsMappingIllegal() {
        #expect(FilenameMapping.needsMapping("file\\name"))
        #expect(FilenameMapping.needsMapping("file:name"))
        #expect(FilenameMapping.needsMapping("file*name"))
    }
    #endif
}
