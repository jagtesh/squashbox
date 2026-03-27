import ArgumentParser
import SquashboxCore
import SquashFsSource
import Foundation

@main
struct Sqb: ParsableCommand {
    static var configuration: CommandConfiguration {
        var subcommands: [ParsableCommand.Type] = [ImageCommand.self]
        // TODO: Add MountCommand and UmountCommand when ProjFsDriver is ready
        // #if os(Windows)
        // subcommands += [MountCommand.self, UmountCommand.self]
        // #endif
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
        var allEntries: [DirEntry] = []
        var cookie: UInt64 = 0
        repeat {
            let batch = try source.listDirectory(rootInode, cookie: cookie)
            allEntries.append(contentsOf: batch.entries)
            if batch.isEmpty { break }
            cookie = batch.cookie
        } while true

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
                var subCookie: UInt64 = 0
                repeat {
                    let batch = try source.listDirectory(entry.inode, cookie: subCookie)
                    for child in batch.entries {
                        typeCounts[child.entryType, default: 0] += 1
                        totalTwoLevels += 1
                    }
                    if batch.isEmpty { break }
                    subCookie = batch.cookie
                } while true
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
                    var count = 0
                    var subCookie: UInt64 = 0
                    repeat {
                        let batch = try source.listDirectory(inode, cookie: subCookie)
                        count += batch.entries.count
                        if batch.isEmpty { break }
                        subCookie = batch.cookie
                    } while true
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
