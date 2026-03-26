# Squashbox Driver Lifecycles

This document maps the complete lifecycle of every filesystem action through both the
Windows (ProjFS via `windows-projfs`) and macOS (FSKit via UniFFI + Swift appex) drivers.

---

## How to Read This Document

Each action follows the same format:

1. **Trigger** — what the user/OS does to initiate the action
2. **OS ➜ Driver** — the OS callback or protocol message that arrives at our code
3. **Driver ➜ Core** — how our driver translates the OS request into a call on the core abstraction
4. **Core ➜ Driver** — what the core returns
5. **Driver ➜ OS** — how the driver satisfies the original OS request

---

## 1. Initialization / Mount

### Windows (ProjFS)

```
User runs: squashbox mount image.sqsh C:\mount\point

1. TRIGGER:  CLI parses args, calls SquashboxCore::open("image.sqsh")
2. CORE:     backhand opens the .sqsh file, parses superblock, returns SquashFs handle
3. DRIVER:   Creates virtualization root directory at C:\mount\point
             Calls PrjMarkDirectoryAsPlaceholder() on root
             Constructs SquashboxProjFsSource { core: SquashFs }
             Calls ProjectedFileSystem::new(root_path, source).start()
4. OS:       PrjStartVirtualizing() is called internally
             ProjFS is now intercepting I/O to the root directory
5. STATE:    The driver enters a blocking wait (the ProjFS event loop runs on
             internal threads managed by windows-projfs)
```

### macOS (FSKit via UniFFI)

```
User runs: squashbox mount image.sqsh /tmp/mount_point

1. TRIGGER:  CLI invokes the FSKit app extension (SquashboxFS.appex)
2. APPEX:    macOS activates the app extension via XPC (launchd-managed)
             Swift FSUnaryFileSystem subclass receives loadResource() callback
3. SWIFT:    Calls SquashboxCore.open(imagePath) via UniFFI-generated Swift bindings
             → This is an in-process FFI call into the Rust static library
             → Rust opens .sqsh, builds inode index, returns handle
4. FSKIT:    FSKit calls didFinishLoading(), then volume query methods:
             → volumeName()           → reads from Rust core
             → volumeStatistics()     → calls provider.volume_stats() via FFI
             → volumeCapabilities()   → returns read-only capability set
5. OS:       Volume appears in Finder / mount table
6. STATE:    The app extension stays alive, serving FSKit callbacks.
             Each callback maps to an in-process FFI call into Rust.
```

---

## 2. Directory Listing (`ls`, Explorer browse)

### Windows (ProjFS)

```
1. TRIGGER:  User opens C:\mount\point\some\dir in Explorer
2. OS:       ProjFS fires GetDirectoryEnumeration callback
3. DRIVER:   ProjectedFileSystemSource::list_directory(&self, path: &Path)
             → path = "some\dir" (relative to virtualization root)
4. CORE:     Translates path to SquashFS inode
             Reads directory entries from the .sqsh image
             Returns Vec<(name, type, size, timestamps)>
5. DRIVER:   Maps each entry to DirectoryEntry::File { name, size } or
             DirectoryEntry::Directory { name }
             Returns Vec<DirectoryEntry>
6. OS:       ProjFS sends placeholder info back to Explorer
             Explorer renders the file listing
```

### macOS (FSKit via UniFFI)

```
1. TRIGGER:  User opens /tmp/mount_point/some/dir in Finder
2. OS:       VFS calls readdir → FSKit → Swift appex (in-process)
3. SWIFT:    FSUnaryFileSystem.enumerateDirectory(identifier:cookie:)
             calls SquashboxCore.listDirectory(inodeId:cookie:) via FFI
4. CORE:     Reads directory entries from inode, returns paginated list
5. SWIFT:    Maps each returned entry to FSItemAttributes for FSKit
             Returns FSDirectoryEntryBatch to FSKit
6. OS:       FSKit sends entries back to kernel via XPC
             Finder renders the file listing
```

---

## 3. File Metadata / Stat

### Windows (ProjFS)

```
1. TRIGGER:  Application calls GetFileAttributes() or stat() on a file
2. OS:       ProjFS fires GetPlaceholderInfo callback
3. DRIVER:   ProjectedFileSystemSource::get_directory_entry(&self, path: &Path)
             → path = "some\dir\file.txt"
4. CORE:     Looks up inode for the path
             Returns metadata (size, timestamps, type)
5. DRIVER:   Returns Some(DirectoryEntry::File { name, size })
             or None if path doesn't exist
6. OS:       ProjFS populates the placeholder with file metadata
             Returns to the calling application
```

### macOS (FSKit via UniFFI)

```
1. TRIGGER:  Application calls stat() on a file
2. OS:       VFS calls getattr → FSKit → Swift appex (in-process)
3. SWIFT:    Two possible paths:

             a) FSUnaryFileSystem.lookUp(name:inDirectory:)
                calls SquashboxCore.lookup(parentInode:name:) via FFI
                → Resolves name within parent directory
                → Returns item with (inode, type, size, mode, timestamps)

             b) FSUnaryFileSystem.getAttributes(of:)
                calls SquashboxCore.getAttributes(inodeId:) via FFI
                → Retrieves attributes for a known inode

4. CORE:     Reads inode metadata from .sqsh (in-process, zero serialization)
5. SWIFT:    Maps Rust EntryAttributes → FSItemAttributes
6. OS:       VFS populates stat buffer, returns to application
```

---

## 4. File Read

### Windows (ProjFS)

```
1. TRIGGER:  Application calls ReadFile() / opens file for reading
2. OS:       ProjFS fires GetFileData callback
             This happens when ProjFS needs to hydrate a placeholder into a full file
3. DRIVER:   ProjectedFileSystemSource::stream_file_content(
                &self, path: &Path, byte_offset: usize, length: usize
             ) → Result<Box<dyn Read>>
4. CORE:     Opens file within .sqsh at given inode
             Decompresses the relevant data blocks
             Returns a Read impl starting at byte_offset, limited to length
5. DRIVER:   Returns the Box<dyn Read>
             windows-projfs reads from it and writes data into the ProjFS scratch area
6. OS:       ProjFS caches the hydrated file locally
             Subsequent reads hit the local cache (no more callbacks)
             Application gets file content

NOTE: ProjFS hydrates entire files (or large chunks). Once hydrated, the
file is served from the local filesystem until the placeholder is reset.
```

### macOS (FSKit via UniFFI)

```
1. TRIGGER:  Application calls read() on an open file descriptor
2. OS:       VFS calls read → FSKit → Swift appex (in-process)
3. SWIFT:    FSUnaryFileSystem.read(from:offset:length:into:)
             calls SquashboxCore.readFile(inodeId:offset:length:) via FFI
             → Direct function call across FFI boundary — no serialization,
               no TCP, no Protobuf. Just a C-ABI call into the Rust static library.
4. CORE:     Seeks to offset within SquashFS file data
             Decompresses requested blocks
             Returns byte buffer (Data / UnsafeBufferPointer across FFI)
5. SWIFT:    Writes bytes directly into FSKit's provided buffer
6. OS:       FSKit returns data to kernel via XPC
             VFS copies to user buffer, returns to application

NOTE: FSKit is request-per-read — no local hydration cache like ProjFS.
Each read() goes through the full path. The critical difference from fskit-rs
is that the Rust core runs in-process with the Swift appex — no TCP/Protobuf
overhead per read(). Consider implementing a block cache in the core layer
for SquashFS decompression performance.
```

---

## 5. Symlink Resolution

### Windows (ProjFS)

```
ProjFS has no native symlink support in its projection model.

OPTIONS:
a) Project symlinks as regular files containing the target path
b) Project symlinks as NTFS reparse points (requires custom notification handling)
c) Resolve symlinks transparently — project the target content at the link's path

For Squashbox, option (c) is simplest: the core resolves symlinks during
path traversal before returning entries/content to the ProjFS driver.
```

### macOS (FSKit via UniFFI)

```
1. TRIGGER:  Application accesses a symlink (readlink or follows it)
2. OS:       VFS calls readlink → FSKit → Swift appex (in-process)
3. SWIFT:    FSUnaryFileSystem.readSymbolicLink(of:)
             calls SquashboxCore.readSymlink(inodeId:) via FFI
4. CORE:     Reads symlink target from SquashFS inode
             Returns target path as string
5. SWIFT:    Converts String → Data, returns to FSKit
6. OS:       VFS follows the link or returns the target to the caller
```

---

## 6. Extended Attributes (xattr)

### Windows (ProjFS)

```
ProjFS does not support extended attributes.
Windows uses Alternate Data Streams (ADS) for similar purposes.

For Squashbox: xattrs from the SquashFS image are not surfaced on Windows.
This is acceptable — most Windows applications don't expect xattrs.
```

### macOS (FSKit via UniFFI)

```
1. TRIGGER:  Application calls getxattr() / listxattr()
2. OS:       VFS calls getxattr → FSKit → Swift appex (in-process)
3. SWIFT:    FSUnaryFileSystem.xattrNames(of:) / getXattr(named:of:)
             calls SquashboxCore.listXattrs(inodeId:) via FFI
             or    SquashboxCore.getXattr(inodeId:name:) via FFI
4. CORE:     Reads xattr data from SquashFS inode metadata
5. SWIFT:    Returns xattr names and/or values to FSKit
6. OS:       VFS returns to application
```

---

## 7. Access Control / Permissions

### Windows (ProjFS)

```
ProjFS projections inherit the ACL of the virtualization root.
Individual file permissions from SquashFS are not mapped.

For Squashbox: all projected files inherit the root directory's
permissions. This is the expected ProjFS behavior.
```

### macOS (FSKit via UniFFI)

```
1. TRIGGER:  Application calls access() or VFS checks permissions
2. OS:       VFS calls access → FSKit → Swift appex (in-process)
3. SWIFT:    FSUnaryFileSystem.checkAccess(to:operations:)
             calls SquashboxCore.checkAccess(inodeId:mask:) via FFI
4. CORE:     Reads mode bits from SquashFS inode
             Compares against requested access mask
5. SWIFT:    Returns allowed/denied to FSKit
6. OS:       VFS allows or denies the operation
```

---

## 8. Notifications / Write Attempts (Read-Only FS)

### Windows (ProjFS)

```
1. TRIGGER:  User tries to create/delete/rename a file in the mount
2. OS:       ProjFS fires a pre-operation notification
3. DRIVER:   ProjectedFileSystemSource::handle_notification(&self, notification: &Notification)
             → Matches on Notification variant (FileCreated, FileRenamed, etc.)
4. DRIVER:   Returns ControlFlow::Break(()) to deny the operation
5. OS:       ProjFS returns STATUS_ACCESS_DENIED to the application
```

### macOS (FSKit via UniFFI)

```
1. TRIGGER:  User tries to write/create/delete
2. OS:       VFS calls write/create/remove → FSKit → Swift appex (in-process)
3. SWIFT:    Write-related FSUnaryFileSystem methods
             → Do NOT call into Rust core at all
             → Immediately return NSError with POSIX EROFS
4. OS:       VFS returns EROFS to application
```

---

## 9. Unmount / Shutdown

### Windows (ProjFS)

```
1. TRIGGER:  User runs: squashbox unmount C:\mount\point
             Or process receives SIGTERM/Ctrl+C
2. DRIVER:   Calls ProjectedFileSystem::stop()
             → Internally calls PrjStopVirtualizing()
3. OS:       ProjFS stops intercepting I/O on the root
4. DRIVER:   Drops the SquashFs handle (closes .sqsh file)
5. CLEANUP:  Optionally removes placeholder metadata from root directory
             (ProjFS leaves .git-style metadata files)
```

### macOS (FSKit via UniFFI)

```
1. TRIGGER:  User runs: squashbox unmount /tmp/mount_point
             Or: diskutil unmount /tmp/mount_point
2. OS:       VFS calls unmount → FSKit → Swift appex (in-process)
3. SWIFT:    FSUnaryFileSystem.unmount()
             calls SquashboxCore.close() via FFI
4. CORE:     Drops SquashFs handle (closes .sqsh file, frees inode index)
5. APPEX:    App extension may be terminated by launchd (lifecycle-managed)
6. OS:       Volume disappears from mount table / Finder
```

---

## Lifecycle Summary Matrix

| Action | Windows (ProjFS) | macOS (FSKit via UniFFI) |
|--------|-----------------|--------------------------|
| **Mount** | `ProjectedFileSystem::new().start()` | `loadResource()` → FFI `open()` → volume queries |
| **List dir** | `list_directory(path)` | `enumerateDirectory()` → FFI `listDirectory()` |
| **Stat file** | `get_directory_entry(path)` | `lookUp()` → FFI `lookup()` / `getAttributes()` |
| **Read file** | `stream_file_content(path, offset, len)` | `read()` → FFI `readFile()` |
| **Symlink** | Resolved transparently by core | `readSymbolicLink()` → FFI `readSymlink()` |
| **Xattr** | Not supported | `getXattr()` → FFI `getXattr()` / `listXattrs()` |
| **Permissions** | Inherited from root ACL | `checkAccess()` → FFI `checkAccess()` |
| **Write (deny)** | `handle_notification() → Break` | Return `EROFS` directly in Swift |
| **Unmount** | `ProjectedFileSystem::stop()` | `unmount()` → FFI `close()` |

---

## Key Architectural Differences

### Addressing Model
- **ProjFS**: Path-based — every callback receives a relative `&Path` from the virtualization root
- **FSKit**: Inode-based — operations use `u64` item IDs (inode numbers), with `lookUp()` resolving names to IDs

### Caching Model
- **ProjFS**: Hydration-based — files are fully materialized to local disk on first access. Subsequent reads bypass the driver entirely
- **FSKit**: Pass-through — every `read()` goes through the driver. No OS-level caching of file data by default

### Concurrency Model
- **ProjFS** (`windows-projfs`): Synchronous `&self` callbacks — the crate manages internal thread pool. Source must be `Sync`
- **FSKit** (UniFFI): Synchronous FFI calls from Swift into Rust — FSKit manages callback dispatch on its own queues. The Rust `VirtualFsProvider` is `Send + Sync` behind an `Arc`, so concurrent calls are safe

### Interop Model
- **ProjFS**: Pure Rust — `windows-projfs` wraps `Win32_Storage_ProjectedFileSystem` APIs via the `windows` crate
- **FSKit**: Swift appex → Rust via UniFFI — the Rust core compiles as a `staticlib`, UniFFI generates idiomatic Swift bindings, the static library links directly into the FSKit app extension. **No IPC, no serialization, no bridge process**

### API Surface
- **ProjFS**: **4 methods** (2 required + 2 optional). Very focused on "project this tree"
- **FSKit**: **~15 FSUnaryFileSystem override methods** in Swift, each mapping 1:1 to a UniFFI-exported Rust function. Write methods return `EROFS` without calling into Rust
