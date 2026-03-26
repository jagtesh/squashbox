# Squashbox Driver Lifecycles

This document maps the complete lifecycle of every filesystem action through both the
Windows (ProjFS via `windows-projfs`) and macOS (FSKit via `fskit-rs` + FSKitBridge) drivers.

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

### macOS (FSKit via fskit-rs)

```
User runs: squashbox mount image.sqsh /tmp/mount_point

1. TRIGGER:  CLI sends mount request to FSKitBridge (the Swift appex)
2. BRIDGE:   FSKitBridge receives XPC activation from launchd
             Opens TCP localhost connection to Rust backend
3. PROTOCOL: fskit-rs session::mount() is called
4. TRAIT:    Filesystem::activate(options) is called
             → Core opens .sqsh file, returns root Item { id: ROOT_INODE, ... }
5. TRAIT:    Filesystem::mount(TaskOptions) is called
             → Confirms ready for I/O
6. TRAIT:    Filesystem::get_resource_identifier() → ResourceIdentifier
             Filesystem::get_volume_identifier() → VolumeIdentifier
             Filesystem::get_volume_behavior()   → VolumeBehavior { read_only: true }
             Filesystem::get_volume_capabilities() → SupportedCapabilities
             Filesystem::get_volume_statistics() → StatFsResult
7. OS:       Volume appears in Finder / mount table
8. STATE:    Tokio runtime serves protobuf messages over TCP until unmount
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

### macOS (FSKit via fskit-rs)

```
1. TRIGGER:  User opens /tmp/mount_point/some/dir in Finder
2. OS:       VFS calls readdir → FSKit → FSKitBridge → TCP → fskit-rs
3. DRIVER:   Filesystem::enumerate_directory(directory_id, cookie, verifier)
             → directory_id = inode of "some/dir"
             → cookie = 0 for first batch, >0 for continuation
4. CORE:     Reads directory entries from inode directory_id
             Returns paginated list starting at cookie offset
5. DRIVER:   Maps each entry to DirectoryEntries { entries: [...], cookie, verifier }
             Each entry is an Item { id, name, attributes: ItemAttributes { ... } }
6. OS:       FSKit sends entries back through XPC
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

### macOS (FSKit via fskit-rs)

```
1. TRIGGER:  Application calls stat() on a file
2. OS:       VFS calls getattr → FSKit → FSKitBridge → TCP → fskit-rs
3. DRIVER:   Two possible paths:

             a) Filesystem::lookup_item(name: &OsStr, directory_id: u64)
                → Resolves name within parent directory
                → Returns Item { id, name, attributes }

             b) Filesystem::get_attributes(item_id: u64)
                → Retrieves attributes for a known inode
                → Returns ItemAttributes { size, mode, uid, gid, timestamps, ... }

4. CORE:     Reads inode metadata from .sqsh
5. DRIVER:   Returns Item or ItemAttributes with SquashFS metadata mapped to
             POSIX stat fields
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

### macOS (FSKit via fskit-rs)

```
1. TRIGGER:  Application calls read() on an open file descriptor
2. OS:       VFS calls read → FSKit → FSKitBridge → TCP → fskit-rs
3. DRIVER:   Filesystem::open_item(item_id: u64, modes: Vec<OpenMode>)
             → Called once when file is first opened
             → Returns Ok(()) for read-only access

             Filesystem::read(item_id: u64, offset: i64, length: i64)
             → Called for each read() syscall
4. CORE:     Seeks to offset within SquashFS file data
             Decompresses requested blocks
             Returns Vec<u8> of the requested range
5. DRIVER:   Returns the byte vector
6. OS:       FSKit sends data back through XPC
             VFS copies to user buffer, returns to application

NOTE: FSKit is request-per-read — no local hydration cache like ProjFS.
Each read() goes through the full path. Consider implementing a
block cache in the core layer for performance.
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

### macOS (FSKit via fskit-rs)

```
1. TRIGGER:  Application accesses a symlink (readlink or follows it)
2. OS:       VFS calls readlink → FSKit → FSKitBridge → TCP → fskit-rs
3. DRIVER:   Filesystem::read_symbolic_link(item_id: u64)
4. CORE:     Reads symlink target from SquashFS inode
             Returns target path as bytes
5. DRIVER:   Returns Ok(Vec<u8>) containing the symlink target
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

### macOS (FSKit via fskit-rs)

```
1. TRIGGER:  Application calls getxattr() / listxattr()
2. OS:       VFS calls getxattr → FSKit → FSKitBridge → TCP → fskit-rs
3. DRIVER:   Filesystem::get_supported_xattr_names(item_id: u64) → Xattrs
             Filesystem::get_xattr(name: &OsStr, item_id: u64) → Vec<u8>
             Filesystem::get_xattrs(item_id: u64) → Xattrs
4. CORE:     Reads xattr data from SquashFS inode metadata
5. DRIVER:   Returns xattr names and/or values
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

### macOS (FSKit via fskit-rs)

```
1. TRIGGER:  Application calls access() or VFS checks permissions
2. OS:       VFS calls access → FSKit → FSKitBridge → TCP → fskit-rs
3. DRIVER:   Filesystem::check_access(item_id: u64, access: Vec<AccessMask>)
4. CORE:     Reads mode bits from SquashFS inode
             Compares against requested access mask
5. DRIVER:   Returns Ok(true) if allowed, Ok(false) if denied
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

### macOS (FSKit via fskit-rs)

```
1. TRIGGER:  User tries to write/create/delete
2. OS:       VFS calls write/create/remove → FSKit → fskit-rs
3. DRIVER:   Filesystem::write() / create_item() / remove_item() / etc.
4. DRIVER:   Returns Err(Error::Posix(libc::EROFS))  // Read-only filesystem
5. OS:       VFS returns EROFS to application
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

### macOS (FSKit via fskit-rs)

```
1. TRIGGER:  User runs: squashbox unmount /tmp/mount_point
             Or: diskutil unmount /tmp/mount_point
2. OS:       VFS calls unmount → FSKit → FSKitBridge → TCP → fskit-rs
3. DRIVER:   Filesystem::unmount() → Ok(())
             Filesystem::deactivate() → Ok(())
4. DRIVER:   TCP session closes, tokio runtime shuts down
5. CORE:     Drops SquashFs handle (closes .sqsh file)
6. OS:       Volume disappears from mount table / Finder
```

---

## Lifecycle Summary Matrix

| Action | Windows (ProjFS) | macOS (FSKit) |
|--------|-----------------|---------------|
| **Mount** | `ProjectedFileSystem::new().start()` | `activate()` → `mount()` → volume queries |
| **List dir** | `list_directory(path)` | `enumerate_directory(dir_id, cookie, verifier)` |
| **Stat file** | `get_directory_entry(path)` | `lookup_item(name, dir_id)` / `get_attributes(id)` |
| **Read file** | `stream_file_content(path, offset, len)` | `open_item(id)` → `read(id, offset, len)` |
| **Symlink** | Resolved transparently by core | `read_symbolic_link(id)` |
| **Xattr** | Not supported | `get_xattr(name, id)` / `get_xattrs(id)` |
| **Permissions** | Inherited from root ACL | `check_access(id, mask)` |
| **Write (deny)** | `handle_notification() → Break` | Return `Err(EROFS)` |
| **Unmount** | `ProjectedFileSystem::stop()` | `unmount()` → `deactivate()` |

---

## Key Architectural Differences

### Addressing Model
- **ProjFS**: Path-based — every callback receives a relative `&Path` from the virtualization root
- **FSKit**: Inode-based — operations use `u64` item IDs (inode numbers), with `lookup_item()` resolving names to IDs

### Caching Model
- **ProjFS**: Hydration-based — files are fully materialized to local disk on first access. Subsequent reads bypass the driver entirely
- **FSKit**: Pass-through — every `read()` goes through the driver. No OS-level caching of file data by default

### Concurrency Model
- **ProjFS** (`windows-projfs`): Synchronous `&self` callbacks — the crate manages internal thread pool. Source must be `Sync`
- **FSKit** (`fskit-rs`): Async `&mut self` callbacks via `async_trait` — runs on a tokio runtime. Messages are serialized over TCP

### API Surface
- **ProjFS**: **4 methods** (2 required + 2 optional). Very focused on "project this tree"
- **FSKit**: **34 methods** (all required). Full POSIX filesystem surface. Many can return `ENOSYS` for read-only use
