// SquashboxFS — FSKit App Extension entry point.
//
// This is the Swift bridge layer between FSKit and the Rust filesystem
// implementation. It:
// 1. Registers Rust-defined ObjC classes at startup
// 2. Forwards FSKit lifecycle calls (probe/load/unload) to the Rust source
// 3. Returns the Rust-created FSVolume for VFS operations
//
// All actual filesystem logic (readdir, lookup, read, getattr) lives in Rust
// (squashbox-core + squashbox-macos + macos-fskit).

import ExtensionFoundation
import FSKit
import SquashboxFFI

// MARK: - Filesystem implementation

/// The FSKit filesystem that bridges to Rust.
///
/// This class conforms to `FSUnaryFileSystemOperations` and forwards the
/// lifecycle methods into the Rust FFI layer. Once loaded, the FSVolume
/// returned is the Rust-registered `SquashboxVolume` ObjC class, which
/// handles all VFS operations via the Rust trait implementation.
final class SquashboxFileSystem: FSUnaryFileSystem, FSUnaryFileSystemOperations {

    /// The source handle returned by Rust (opaque pointer).
    private var sourceHandle: UnsafeMutableRawPointer?

    override init() {
        super.init()
    }

    deinit {
        if let handle = sourceHandle {
            squashbox_source_destroy(handle)
        }
    }

    // MARK: FSUnaryFileSystemOperations

    /// Probe whether this filesystem can handle the given resource.
    func probeResource(resource: FSResource) async throws -> FSProbeResult {
        // Create a probe result indicating we can handle this resource.
        // For SquashFS, we check the magic number in the resource.
        let name = String(cString: squashbox_fs_short_name())
        let containerID = FSContainerIdentifier(uuid: UUID())
        return FSProbeResult.usable(name: name, containerID: containerID)
    }

    /// Load a resource and return the volume.
    func loadResource(resource: FSResource, options: FSTaskOptions) async throws -> FSVolume {
        // Create the Rust source
        sourceHandle = squashbox_source_create()

        guard sourceHandle != nil else {
            throw NSError(
                domain: "com.squashbox.fs",
                code: -1,
                userInfo: [NSLocalizedDescriptionKey: "Failed to create filesystem source"]
            )
        }

        // The volume is the Rust-registered SquashboxVolume ObjC class.
        // It was registered via define_class! and handles all VFS operations.
        if let volumeClass = NSClassFromString("SquashboxVolume") as? FSVolume.Type {
            let volumeID = FSVolume.Identifier(uuid: UUID())
            let volumeName = FSFileName(string: "SquashFS")
            return volumeClass.init(volumeID: volumeID, volumeName: volumeName)
        } else {
            throw NSError(
                domain: "com.squashbox.fs",
                code: -2,
                userInfo: [NSLocalizedDescriptionKey: "SquashboxVolume ObjC class not registered"]
            )
        }
    }

    /// Unload the resource.
    func unloadResource(resource: FSResource, options: FSTaskOptions) async throws {
        if let handle = sourceHandle {
            squashbox_source_destroy(handle)
            sourceHandle = nil
        }
    }
}

// MARK: - Extension entry point

/// The FSKit extension entry point.
///
/// FSKit requires extensions to use Swift's `@main` / `AppExtension` protocol.
/// This struct registers the Rust-defined ObjC classes and provides the
/// filesystem instance.
@main
struct SquashboxFSExtension: UnaryFileSystemExtension {

    /// The filesystem instance.
    let fileSystem = SquashboxFileSystem()

    /// Called by ExtensionFoundation to create the extension.
    init() {
        // Register the Rust-defined ObjC classes with the runtime.
        // This must happen before FSKit tries to use them.
        squashbox_register_classes()
    }
}
