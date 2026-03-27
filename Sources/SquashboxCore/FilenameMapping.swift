import Foundation

/// Platform-conditional filename sanitization.
///
/// Maps characters that are illegal in filenames on the current platform
/// to Unicode Private Use Area (PUA) codepoints, using the same mapping
/// as WSL2 for interoperability.
///
/// On platforms where no mapping is needed (e.g., Linux), `toPlatformSafe`
/// and `fromPlatformSafe` are identity functions.
///
/// ## Relationship to the Rust implementation
/// This was previously embedded in the `backhand` fork's `dir.rs`.
/// Moving it to Core separates the policy (which chars to map) from the
/// mechanism (SquashFS parsing). Any source provider can use it.
public enum FilenameMapping {

    // MARK: - Mapping Table

    #if os(Windows)
    /// Characters illegal in Windows NTFS filenames, mapped to PUA codepoints.
    /// The mapping matches WSL2's interop behavior:
    ///   https://learn.microsoft.com/en-us/windows/wsl/file-permissions
    private static let platformIllegalChars: [(from: Character, to: Unicode.Scalar)] = [
        ("\\", Unicode.Scalar(0xF05C)!),
        (":",  Unicode.Scalar(0xF03A)!),
        ("*",  Unicode.Scalar(0xF02A)!),
        ("?",  Unicode.Scalar(0xF03F)!),
        ("\"", Unicode.Scalar(0xF022)!),
        ("<",  Unicode.Scalar(0xF03C)!),
        (">",  Unicode.Scalar(0xF03E)!),
        ("|",  Unicode.Scalar(0xF07C)!),
    ]
    #else
    /// On non-Windows platforms, no filename character mapping is needed.
    private static let platformIllegalChars: [(from: Character, to: Unicode.Scalar)] = []
    #endif

    // MARK: - Forward Mapping (Linux → Platform-Safe)

    /// Build the forward lookup table lazily.
    private static let forwardMap: [Character: Character] = {
        var map = [Character: Character]()
        for entry in platformIllegalChars {
            map[entry.from] = Character(entry.to)
        }
        return map
    }()

    /// Build the reverse lookup table lazily.
    private static let reverseMap: [Character: Character] = {
        var map = [Character: Character]()
        for entry in platformIllegalChars {
            map[Character(entry.to)] = entry.from
        }
        return map
    }()

    /// Sanitize a filename for the current platform.
    ///
    /// Replaces characters that are illegal on this platform with their
    /// PUA equivalents. On platforms with no illegal chars (Linux),
    /// this is an identity function.
    ///
    /// - Parameter name: The original filename (e.g., from a SquashFS image).
    /// - Returns: A platform-safe filename.
    public static func toPlatformSafe(_ name: String) -> String {
        guard !forwardMap.isEmpty else { return name }

        var result = ""
        result.reserveCapacity(name.count)
        for char in name {
            result.append(forwardMap[char] ?? char)
        }
        return result
    }

    /// Reverse the platform-safe mapping back to the original filename.
    ///
    /// This is a lossless round-trip:
    /// `fromPlatformSafe(toPlatformSafe(name)) == name`
    ///
    /// - Parameter name: A platform-safe filename (possibly with PUA chars).
    /// - Returns: The original filename.
    public static func fromPlatformSafe(_ name: String) -> String {
        guard !reverseMap.isEmpty else { return name }

        var result = ""
        result.reserveCapacity(name.count)
        for char in name {
            result.append(reverseMap[char] ?? char)
        }
        return result
    }

    /// Check whether a filename contains characters that need mapping
    /// on the current platform.
    public static func needsMapping(_ name: String) -> Bool {
        guard !forwardMap.isEmpty else { return false }
        return name.contains(where: { forwardMap[$0] != nil })
    }
}
