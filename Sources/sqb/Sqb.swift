import ArgumentParser
import SquashboxCore
import SquashFsSource

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
        let source = try SquashFsSource(imagePath: imagePath)
        let stats = try source.volumeStats()

        print("Image: \(imagePath)")
        print("Inodes: \(stats.totalInodes)")
        print("Block size: \(stats.blockSize)")

        // List root directory entries
        let rootInode = rootInodeId
        let attrs = try source.getAttributes(rootInode)
        print("Root inode: \(attrs.inode) (type: \(attrs.entryType))")

        var cookie: UInt64 = 0
        var entryCount = 0
        repeat {
            let batch = try source.listDirectory(rootInode, cookie: cookie)
            for entry in batch.entries {
                let typeIndicator: String
                switch entry.entryType {
                case .directory: typeIndicator = "d"
                case .symlink:   typeIndicator = "l"
                case .file:      typeIndicator = "-"
                default:         typeIndicator = "?"
                }
                print("  \(typeIndicator) \(entry.name)")
                entryCount += 1
            }
            cookie = batch.cookie
            if batch.isEmpty { break }
        } while true

        print("Total root entries: \(entryCount)")
    }
}
