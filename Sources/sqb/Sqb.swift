import ArgumentParser
import SquashboxCore
import SquashFsSource
import Foundation

#if os(Windows)
import ProjFsDriver
#endif

@main
struct Sqb: ParsableCommand {
    static var configuration: CommandConfiguration {
        var subcommands: [ParsableCommand.Type] = [ImageCommand.self]
        #if os(Windows)
        subcommands += [MountCommand.self, UmountCommand.self]
        #endif
        return CommandConfiguration(
            commandName: "sqb",
            abstract: "Squashbox — native SquashFS tools for Windows, macOS, and Linux",
            subcommands: subcommands
        )
    }
}

// MARK: - Image Command (cross-platform)

struct ImageCommand: ParsableCommand {
    static let configuration = CommandConfiguration(
        commandName: "image",
        abstract: "Inspect a SquashFS image"
    )

    @Argument(help: "Path to the SquashFS image file")
    var imagePath: String

    func run() throws {
        // ── Banner ──
        printBanner("Squashbox Image Inspector")
        print()

        // ── File info ──
        let fileURL = URL(fileURLWithPath: imagePath)
        let fileAttrs = try FileManager.default.attributesOfItem(atPath: imagePath)
        let fileSize = (fileAttrs[.size] as? UInt64) ?? 0
        print("  File:       \(fileURL.path)")
        print("  File size:  \(formatBytesDetailed(fileSize))")
        print()

        // ── Open image ──
        print("  Opening image... ", terminator: "")
        let startTime = Date()
        let source = try SquashFsSource(imagePath: imagePath)
        let elapsed = Date().timeIntervalSince(startTime)
        print("done in \(formatDuration(elapsed))")
        print()

        // ── Volume Stats ──
        let stats = try source.volumeStats()
        let volBox = TextBox(title: "Volume Stats")
        volBox.printHeader()
        volBox.printAlignedRow("Total inodes:", "\(stats.totalInodes)")
        volBox.printAlignedRow("Total bytes:", formatBytesDetailed(stats.totalBytes))
        volBox.printAlignedRow("Block size:", "\(stats.blockSize)")
        volBox.printFooter()
        print()

        // ── Root Directory ──
        let rootInode = rootInodeId
        let rootAttrs = try source.getAttributes(rootInode)
        let rootBox = TextBox(title: "Root Directory")
        rootBox.printHeader()
        rootBox.printRow("Mode:", String(format: "0o%o", rootAttrs.mode))
        rootBox.printRow("UID:", "\(rootAttrs.uid)")
        rootBox.printRow("GID:", "\(rootAttrs.gid)")
        rootBox.printRow("Nlink:", "\(rootAttrs.nlink)")
        rootBox.printFooter()
        print()

        // ── Root Entries ──
        let allEntries = try source.allEntries(rootInode)

        let entryBox = TextBox(title: "Root Entries (\(allEntries.count))")
        entryBox.printHeader()
        for entry in allEntries {
            let icon = entryIcon(for: entry.entryType)
            var extra = ""
            if entry.entryType == .symlink {
                if let target = try? source.readSymlink(entry.inode) {
                    extra = " -> \(target)"
                }
            } else if entry.entryType == .file {
                let attrs = try source.getAttributes(entry.inode)
                extra = " (\(formatSize(attrs.size)))"
            }
            let nameField = entry.name.padding(toLength: 25, withPad: " ", startingAt: 0)
            entryBox.printContent("\(icon) \(nameField)\(extra)")
        }
        entryBox.printFooter()
        print()

        // ── Entry Type Distribution (2 levels) ──
        var typeCounts: [EntryType: Int] = [:]
        var totalTwoLevels = 0
        for entry in allEntries {
            typeCounts[entry.entryType, default: 0] += 1
            totalTwoLevels += 1

            if entry.entryType == .directory {
                let children = try source.allEntries(entry.inode)
                for child in children {
                    typeCounts[child.entryType, default: 0] += 1
                    totalTwoLevels += 1
                }
            }
        }

        let typeBox = TextBox(title: "Entry Types (2 levels, \(totalTwoLevels) total)")
        typeBox.printHeader()
        for (etype, count) in typeCounts.sorted(by: { $0.value > $1.value }) {
            let icon = entryIcon(for: etype)
            let label = "\(icon) \(etype)".padding(toLength: 18, withPad: " ", startingAt: 0)
            typeBox.printRow(label, "\(count)")
        }
        typeBox.printFooter()
        print()

        // ── Notable Paths ──
        let notablePaths = ["etc/passwd", "usr/bin", "usr/share", "usr/lib"]
        let pathBox = TextBox(title: "Notable Paths")
        pathBox.printHeader()
        for pathStr in notablePaths {
            if let inode = try source.resolvePath(pathStr) {
                let attrs = try source.getAttributes(inode)
                if attrs.isDirectory {
                    let count = try source.allEntries(inode).count
                    pathBox.printContent("/\(pathStr.padding(toLength: 20, withPad: " ", startingAt: 0)) 📁 \(count) entries")
                } else {
                    pathBox.printContent("/\(pathStr.padding(toLength: 20, withPad: " ", startingAt: 0)) 📄 \(formatSize(attrs.size))")
                }
            } else {
                pathBox.printContent("/\(pathStr.padding(toLength: 20, withPad: " ", startingAt: 0)) ❌ not found")
            }
        }
        pathBox.printFooter()
    }
}

// MARK: - Mount Command (Windows only)

#if os(Windows)
struct MountCommand: ParsableCommand {
    static let configuration = CommandConfiguration(
        commandName: "mount",
        abstract: "Mount a SquashFS image as a virtual directory"
    )

    @Argument(help: "Path to the SquashFS image file")
    var imagePath: String

    @Argument(help: "Directory to mount the image at")
    var mountPoint: String

    @Flag(name: .long, help: "Force mount by cleaning stale reparse points")
    var force = false

    func run() throws {
        // Validate image exists
        guard FileManager.default.fileExists(atPath: imagePath) else {
            throw SquashboxError.notFound("Image file not found: \(imagePath)")
        }

        // Handle mount point
        if FileManager.default.fileExists(atPath: mountPoint) && force {
            print("  Cleaning stale mount point... ", terminator: "")
            try fixMountPoint(mountPoint)
            print("done.")
        } else if !FileManager.default.fileExists(atPath: mountPoint) {
            try FileManager.default.createDirectory(atPath: mountPoint,
                                                     withIntermediateDirectories: true)
        }

        // Open image
        print("  Opening SquashFS image: \(imagePath)")
        let source = try SquashFsSource(imagePath: imagePath)
        let stats = try source.volumeStats()
        print("  Image opened: \(stats.totalInodes) inodes, \(formatSize(stats.totalBytes))")

        // Start ProjFS
        print("  Starting ProjFS at: \(mountPoint)")
        let driver = ProjFsDriver(provider: source, rootPath: mountPoint)
        try driver.start()

        print("✓ Mounted \(imagePath) at \(mountPoint)")
        print("Press Ctrl+C to unmount...")

        // Block until Ctrl+C — use RunLoop to avoid Swift 6.2.4 compiler crash
        // with signal() + DispatchSemaphore in the SendNonSendable SIL pass.
        // Note: RunLoop.run() blocks forever; cleanup happens via driver.deinit
        // or via the `sqb umount` command.
        RunLoop.current.run()
    }
}

// MARK: - Umount Command (Windows only)

struct UmountCommand: ParsableCommand {
    static let configuration = CommandConfiguration(
        commandName: "umount",
        abstract: "Clean up a stale ProjFS mount point"
    )

    @Argument(help: "Directory to clean up")
    var mountPoint: String

    func run() throws {
        guard FileManager.default.fileExists(atPath: mountPoint) else {
            throw SquashboxError.notFound("Directory not found: \(mountPoint)")
        }
        try fixMountPoint(mountPoint)
    }
}

// MARK: - Shared Helpers

/// Remove stale ProjFS reparse points from a directory.
/// Matches the Rust `cmd_fix` function behavior.
private func fixMountPoint(_ path: String) throws {
    if !FileManager.default.fileExists(atPath: path) {
        try FileManager.default.createDirectory(atPath: path,
                                                 withIntermediateDirectories: true)
        print("✓ Created clean mount directory: \(path)")
        return
    }

    if !ProjFsDriver.hasReparsePoint(at: path) {
        print("✓ Directory is already clean: \(path)")
        return
    }

    print("Removing stale ProjFS reparse point from: \(path)")
    do {
        try ProjFsDriver.cleanupReparsePoint(at: path)
        print("✓ Reparse point removed: \(path)")
    } catch {
        // Fall back to directory removal
        fputs("Warning: reparse point removal failed: \(error)\n", stderr)
        fputs("Falling back to directory cleanup...\n", stderr)
        try FileManager.default.removeItem(atPath: path)
        try FileManager.default.createDirectory(atPath: path,
                                                 withIntermediateDirectories: true)
        print("✓ Directory cleaned and ready for mounting: \(path)")
    }
}
#endif
