import Foundation
import SquashboxCore
import CProjFS

/// Windows ProjFS driver that projects a `VirtualFsProvider` as a native directory.
///
/// This adapter translates ProjFS callback requests (path-based) into
/// `VirtualFsProvider` calls (inode-based). It bridges the gap between
/// the Windows Projected File System API and our platform-agnostic core.
///
/// ## Thread Safety
/// ProjFS dispatches callbacks on multiple threads. The `VirtualFsProvider`
/// is `Sendable`, and all mutable state (enum sessions) is protected by a lock.
public final class ProjFsDriver: @unchecked Sendable {
    /// The underlying filesystem provider.
    let provider: any VirtualFsProvider

    /// Active enumeration sessions (GUID → already-enumerated flag).
    private var enumSessions: [Foundation.UUID: Bool] = [:]
    private let lock = NSLock()

    /// The opaque CProjFS handle.
    private var handle: UnsafeMutableRawPointer?

    /// The root directory path.
    public let rootPath: String

    public init(provider: any VirtualFsProvider, rootPath: String) {
        self.provider = provider
        self.rootPath = rootPath
    }

    /// Start ProjFS virtualization.
    /// This call blocks the calling thread — ProjFS callbacks are dispatched
    /// on internal worker threads.
    public func start() throws {
        // Create the directory if it doesn't exist
        if !FileManager.default.fileExists(atPath: rootPath) {
            try FileManager.default.createDirectory(atPath: rootPath,
                                                     withIntermediateDirectories: true)
        }

        // Store self as unmanaged pointer for C callback context
        let ctx = Unmanaged.passRetained(self).toOpaque()

        var callbacks = cprojfs_callbacks(
            startEnum: projfsStartEnum,
            endEnum: projfsEndEnum,
            getEnum: projfsGetEnum,
            getPlaceholder: projfsGetPlaceholder,
            getFileData: projfsGetFileData,
            notification: projfsNotification
        )

        guard let h = cprojfs_start(rootPath, &callbacks, ctx) else {
            Unmanaged<ProjFsDriver>.fromOpaque(ctx).release()
            throw SquashboxError.io("ProjFS start failed for '\(rootPath)'")
        }
        self.handle = h
    }

    /// Stop ProjFS virtualization and clean up.
    public func stop() {
        if let h = handle {
            cprojfs_stop(h)
            handle = nil
            // Release the retained self from the C context
            // (the handle held a reference to us)
        }
    }

    deinit {
        stop()
    }

    // MARK: - Enum Session Management

    func startEnumSession(_ guid: Foundation.UUID) {
        lock.lock()
        enumSessions[guid] = false
        lock.unlock()
    }

    func endEnumSession(_ guid: Foundation.UUID) {
        lock.lock()
        enumSessions.removeValue(forKey: guid)
        lock.unlock()
    }

    func isEnumDone(_ guid: Foundation.UUID) -> Bool {
        lock.lock()
        defer { lock.unlock() }
        return enumSessions[guid] ?? true
    }

    func markEnumDone(_ guid: Foundation.UUID) {
        lock.lock()
        enumSessions[guid] = true
        lock.unlock()
    }

    // MARK: - Reparse Point Cleanup

    /// Remove stale ProjFS reparse points from a directory.
    /// Idempotent — no-op if the directory is already clean.
    public static func cleanupReparsePoint(at path: String) throws {
        let hr = cprojfs_delete_reparse_point(path)
        if hr != 0 { // S_OK = 0
            throw SquashboxError.io("failed to remove reparse point (HRESULT: 0x\(String(hr, radix: 16)))")
        }
    }

    /// Check if a directory has a ProjFS reparse point.
    public static func hasReparsePoint(at path: String) -> Bool {
        cprojfs_has_reparse_point(path) != 0
    }
}

// MARK: - Win32 HRESULT Constants

/// Common Win32 HRESULT values returned by ProjFS callbacks.
///
/// These are COM-style error codes: the high bit indicates failure,
/// facility `0x0007` means "Win32", and the low 16 bits are the
/// underlying Win32 error code.
private enum HResult {
    /// Operation succeeded.
    static let ok: Int32            =  0          // 0x00000000
    /// Operation succeeded, but there is no more data.
    static let sFalse: Int32        =  1          // 0x00000001
    /// One or more arguments are invalid.
    static let invalidArg: Int32    = -2147024809 // 0x80070057  E_INVALIDARG
    /// The system cannot find the file specified.
    static let fileNotFound: Int32  = -2147024894 // 0x80070002  HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND)
    /// Access is denied.
    static let accessDenied: Int32  = -2147024891 // 0x80070005  E_ACCESSDENIED
}

// MARK: - C Callback Trampolines (@convention(c))

/// Convert a GUID to a Swift UUID.
private func guidToUUID(_ guid: UnsafePointer<GUID>) -> Foundation.UUID {
    let g = guid.pointee
    return Foundation.UUID(uuid: (
        UInt8((g.Data1 >> 24) & 0xFF), UInt8((g.Data1 >> 16) & 0xFF),
        UInt8((g.Data1 >> 8) & 0xFF),  UInt8(g.Data1 & 0xFF),
        UInt8((g.Data2 >> 8) & 0xFF),  UInt8(g.Data2 & 0xFF),
        UInt8((g.Data3 >> 8) & 0xFF),  UInt8(g.Data3 & 0xFF),
        g.Data4.0, g.Data4.1, g.Data4.2, g.Data4.3,
        g.Data4.4, g.Data4.5, g.Data4.6, g.Data4.7
    ))
}

/// Start directory enumeration
private let projfsStartEnum: cprojfs_start_enum_cb = { ctx, path, enumId in
    guard let ctx = ctx, let enumId = enumId else { return HResult.invalidArg }
    let driver = Unmanaged<ProjFsDriver>.fromOpaque(ctx).takeUnretainedValue()
    let uuid = guidToUUID(enumId)
    driver.startEnumSession(uuid)
    return HResult.ok
}

/// End directory enumeration
private let projfsEndEnum: cprojfs_end_enum_cb = { ctx, path, enumId in
    guard let ctx = ctx, let enumId = enumId else { return HResult.invalidArg }
    let driver = Unmanaged<ProjFsDriver>.fromOpaque(ctx).takeUnretainedValue()
    let uuid = guidToUUID(enumId)
    driver.endEnumSession(uuid)
    return HResult.ok
}

/// Get directory entries
private let projfsGetEnum: cprojfs_get_enum_cb = { ctx, pathPtr, enumId, searchExpr, entryBuffer in
    guard let ctx = ctx, let pathPtr = pathPtr, let enumId = enumId, let entryBuffer = entryBuffer else {
        return HResult.invalidArg
    }
    let driver = Unmanaged<ProjFsDriver>.fromOpaque(ctx).takeUnretainedValue()
    let uuid = guidToUUID(enumId)

    // If we already sent all entries for this session, done
    if driver.isEnumDone(uuid) {
        return HResult.sFalse
    }

    let path = String(cString: pathPtr)
    let searchPattern = searchExpr.map { String(cString: $0) }

    do {
        // Resolve path to inode
        let inode: InodeId
        if path.isEmpty || path == "." {
            inode = rootInodeId
        } else {
            guard let id = try driver.provider.resolvePath(path) else {
                return HResult.fileNotFound
            }
            inode = id
        }

        // List all entries
        let allEntries = try driver.provider.allEntries(inode)

        // Fill the buffer
        for entry in allEntries {
            // Apply search filter if present
            if let pattern = searchPattern {
                // Simple wildcard match — ProjFS uses DOS-style patterns
                // For now, skip filtering (ProjFS handles it)
                _ = pattern
            }

            let isDir: Int32 = (entry.entryType == .directory) ? 1 : 0
            var fileSize: Int64 = 0
            if entry.entryType == .file {
                let attrs = try driver.provider.getAttributes(entry.inode)
                fileSize = Int64(attrs.size)
            }

            let hr = cprojfs_add_entry(entryBuffer, entry.name, isDir, fileSize)
            if hr != HResult.ok {
                // Buffer full — mark as done (ProjFS will call again)
                break
            }
        }

        driver.markEnumDone(uuid)
        return HResult.ok
    } catch {
        return HResult.accessDenied
    }
}

/// Get placeholder info for a single file/directory
private let projfsGetPlaceholder: cprojfs_get_placeholder_cb = { ctx, pathPtr, nsCtx in
    guard let ctx = ctx, let pathPtr = pathPtr, let nsCtx = nsCtx else {
        return HResult.invalidArg
    }
    let driver = Unmanaged<ProjFsDriver>.fromOpaque(ctx).takeUnretainedValue()
    let path = String(cString: pathPtr)

    do {
        guard let inode = try driver.provider.resolvePath(path) else {
            return HResult.fileNotFound
        }
        let attrs = try driver.provider.getAttributes(inode)
        let isDir: Int32 = attrs.isDirectory ? 1 : 0
        let size = Int64(attrs.size)

        return cprojfs_write_placeholder(nsCtx, path, isDir, size)
    } catch {
        return HResult.fileNotFound
    }
}

/// Get file data (hydration)
private let projfsGetFileData: cprojfs_get_file_data_cb = { ctx, pathPtr, nsCtx, streamId, offset, length in
    guard let ctx = ctx, let pathPtr = pathPtr, let nsCtx = nsCtx, let streamId = streamId else {
        return HResult.invalidArg
    }
    let driver = Unmanaged<ProjFsDriver>.fromOpaque(ctx).takeUnretainedValue()
    let path = String(cString: pathPtr)

    do {
        guard let inode = try driver.provider.resolvePath(path) else {
            return HResult.fileNotFound
        }

        let data = try driver.provider.readFile(inode, offset: offset, length: UInt64(length))

        return data.withUnsafeBytes { bufPtr in
            cprojfs_write_file_data(nsCtx, streamId, bufPtr.baseAddress, offset, UInt32(data.count))
        }
    } catch {
        return HResult.accessDenied
    }
}

/// Notification — deny all writes (read-only filesystem)
private let projfsNotification: cprojfs_notification_cb = { _, _, _, _ in
    return HResult.accessDenied
}

