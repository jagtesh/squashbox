/// `OutputFormatting` — Reusable terminal output formatting for Squashbox CLI.
///
/// Provides box-drawing, alignment, and color helpers that can be used
/// across all CLI commands (image, mount, umount, etc.).

// MARK: - Box Drawing

/// A text box with Unicode box-drawing borders.
///
/// Usage:
/// ```swift
/// let box = TextBox(title: "Volume Stats", width: 48)
/// box.printHeader()
/// box.printRow("Total inodes:", "52271")
/// box.printFooter()
/// ```
public struct TextBox {
    public let title: String
    public let width: Int

    /// Content area width (interior between "  │ " and " │")
    public var contentWidth: Int { width - 6 }

    public init(title: String, width: Int = 48) {
        self.title = title
        self.width = width
    }

    /// Print the top border with title.
    public func printHeader() {
        let titlePart = "─ \(title) "
        let remainingDashes = width - 4 - titlePart.count
        let dashes = String(repeating: "─", count: max(0, remainingDashes))
        print("  ┌\(titlePart)\(dashes)┐")
    }

    /// Print a row with a label and value.
    public func printRow(_ label: String, _ value: String) {
        let content = "\(label) \(value)"
        let padding = contentWidth - displayWidth(content)
        if padding >= 0 {
            print("  │ \(content)\(String(repeating: " ", count: padding)) │")
        } else {
            print("  │ \(content) │")
        }
    }

    /// Print a row with a label and right-justified numeric value.
    public func printAlignedRow(_ label: String, _ value: String, valueWidth: Int = 10) {
        let paddedValue = value.count >= valueWidth
            ? value
            : String(repeating: " ", count: valueWidth - value.count) + value
        printRow(label, paddedValue)
    }

    /// Print a free-form content line (left-aligned).
    public func printContent(_ text: String) {
        let padding = contentWidth - displayWidth(text)
        if padding >= 0 {
            print("  │ \(text)\(String(repeating: " ", count: padding)) │")
        } else {
            print("  │ \(text) │")
        }
    }

    /// Print the bottom border.
    public func printFooter() {
        let dashes = String(repeating: "─", count: width - 4)
        print("  └\(dashes)┘")
    }
}

// MARK: - Banner

/// Print the Squashbox banner with a title.
public func printBanner(_ title: String, width: Int = 50) {
    let bar = String(repeating: "═", count: width)
    let padding = width - title.count
    let leftPad = padding / 2
    let rightPad = padding - leftPad
    print("╔\(bar)╗")
    print("║\(String(repeating: " ", count: leftPad))\(title)\(String(repeating: " ", count: rightPad))║")
    print("╚\(bar)╝")
}

// MARK: - Entry Type Icons

/// Get the display icon for a filesystem entry type.
public func entryIcon(for type: EntryType) -> String {
    switch type {
    case .directory:   return "📁"
    case .file:        return "📄"
    case .symlink:     return "🔗"
    case .charDevice:  return "🔌"
    case .blockDevice: return "💾"
    case .fifo:        return "🔧"
    case .socket:      return "🔌"
    }
}

/// Get the short letter indicator for a filesystem entry type.
public func entryLetter(for type: EntryType) -> String {
    switch type {
    case .directory:   return "d"
    case .file:        return "-"
    case .symlink:     return "l"
    case .charDevice:  return "c"
    case .blockDevice: return "b"
    case .fifo:        return "p"
    case .socket:      return "s"
    }
}

// MARK: - Size Formatting

/// Format a byte count into a human-readable string (e.g. "123.4 MB").
public func formatSize(_ bytes: UInt64) -> String {
    let mb = Double(bytes) / 1_048_576.0
    if mb >= 1024 {
        return String(format: "%.1f GB", mb / 1024.0)
    } else if mb >= 1 {
        return String(format: "%.1f MB", mb)
    } else {
        let kb = Double(bytes) / 1024.0
        if kb >= 1 {
            return String(format: "%.1f KB", kb)
        } else {
            return "\(bytes) B"
        }
    }
}

/// Format bytes as "123456 (123.4 MB)".
public func formatBytesDetailed(_ bytes: UInt64) -> String {
    "\(bytes) (\(formatSize(bytes)))"
}

// MARK: - Timing

/// Format an elapsed duration as a human-readable string.
public func formatDuration(_ seconds: Double) -> String {
    if seconds < 0.001 {
        return String(format: "%.0f µs", seconds * 1_000_000)
    } else if seconds < 1 {
        return String(format: "%.1f ms", seconds * 1_000)
    } else {
        return String(format: "%.2f s", seconds)
    }
}

// MARK: - Display Width

/// Calculate the terminal display width of a string.
/// Emoji and other wide characters count as 2 columns.
public func displayWidth(_ string: String) -> Int {
    var width = 0
    for scalar in string.unicodeScalars {
        if scalar.value > 0x1F000 ||  // Emoji & symbols
           (scalar.value >= 0x2600 && scalar.value <= 0x27BF) ||  // Misc symbols
           (scalar.value >= 0x1F300 && scalar.value <= 0x1FAFF) { // Extended emoji
            width += 2
        } else {
            width += 1
        }
    }
    // Emoji sequences (ZWJ, variation selectors) are part of multi-scalar
    // grapheme clusters — subtract their extra scalars.
    let scalarCount = string.unicodeScalars.count
    let charCount = string.count
    if scalarCount > charCount {
        // Each grapheme cluster with extra scalars (ZWJ, VS16) adds
        // phantom width. Subtract the invisible joining scalars.
        width -= (scalarCount - charCount)
    }
    return width
}
