# Squashbox — Initial Research & Feasibility Analysis

> Research date: March 2026

---

## 1. SquashFS Background

SquashFS is a compressed, **read-only** filesystem originally designed for Linux. It is widely used in live CDs/USBs, embedded devices, container images (e.g. Snap, AppImage), and archival scenarios. Key characteristics:

- **Read-only by design** — immutable once created
- **High compression** — supports gzip, lzma, lzo, xz, lz4, zstd
- **On-disk efficiency** — deduplication, sparse file support, metadata compression
- **Read-write layering** — typically achieved via OverlayFS on top of a SquashFS mount

### Current state on Windows & macOS

There is **no native SquashFS mount support** on either platform today. Existing options are limited:

| Approach | Platform | Notes |
|----------|----------|-------|
| WSL + `squashfs-tools` | Windows | Full Linux tooling, but requires WSL; not "native" |
| 7-Zip extraction | Windows | Read-only extraction only, no mount |
| `squashfuse` (FUSE) | macOS/Linux | Requires FUSE/macFUSE kernel extension |
| `squashfs-tools-ng` CLI | Both (via MinGW/MSVC) | CLI extract/inspect only, no mount |

**This gap is exactly what Squashbox aims to fill** — native, first-class mount support without FUSE or WSL dependencies.

---

## 2. Windows: ProjFS (Projected File System)

### Overview

ProjFS is a Windows API (Windows 10 1809+) that lets user-mode apps ("providers") project hierarchical data into the filesystem. Files and directories appear as normal NTFS entries but their content is fetched on-demand from a backing store.

Originally built for VFS for Git (handling the Windows source monorepo), ProjFS is now a general-purpose virtual filesystem mechanism.

### How it works

```
User/App reads file → NTFS → ProjFS minifilter → Callback to Provider → Provider returns data
```

1. Provider starts a **virtualization instance** on a root directory
2. ProjFS creates **placeholder** entries (metadata only) in the filesystem
3. When a file is opened/read, ProjFS invokes **callbacks** on the provider
4. Provider returns the data; ProjFS caches it locally ("hydration")

### Key callbacks for a read-only provider

| Callback | Purpose |
|----------|---------|
| `StartDirectoryEnumeration` | Begin listing a directory |
| `GetDirectoryEnumeration` | Return entries for a directory listing |
| `EndDirectoryEnumeration` | End directory listing |
| `GetPlaceholderInfo` | Provide metadata (size, timestamps, attributes) for a file/dir |
| `GetFileData` | Provide actual file content bytes |
| `QueryFileName` | Check if a path exists in the backing store |
| `Notification` | Receive notifications about operations (optional) |

### API availability

- **Native C API** — `projectedfslib.h` in the Windows SDK
- **Managed (.NET) API** — [`Microsoft.Windows.ProjFS`](https://www.nuget.org/packages/Microsoft.Windows.ProjFS) NuGet package; pure C# P/Invoke wrapper, no C++ toolchain needed
- **Feature activation required** — ProjFS must be enabled via `Enable-WindowsOptionalFeature -Online -FeatureName Client-ProjFS`

### Feasibility for Squashbox: ✅ High

**Strengths:**
- Mature, well-documented API with official samples (`SimpleProviderManaged`, `RegFS`)
- Perfect fit for read-only projection — SquashFS is inherently read-only
- On-demand hydration means only accessed files consume disk space
- Provider runs as a normal user-mode process (no admin after initial feature enable)
- .NET managed API makes development accessible with C#

**Concerns:**
- Requires Windows 10 1809+ (should be fine for target audience)
- ProjFS feature must be enabled once (admin privilege required for setup)
- NTFS-only for symlink support (minor — SquashFS symlinks could be projected)
- Hydrated files persist locally; may need a scrubbing/cleanup strategy
- Not designed for slow backing stores (but squashfs images are local files, so this is fine)

---

## 3. macOS: FSKit

### Overview

FSKit is Apple's new framework (macOS Sequoia 15.4+, 2024–2025) for implementing filesystems in **user space** as app extensions. It's Apple's strategic replacement for kernel extensions (kexts) and is positioned as the successor to the macFUSE approach.

### How it works

```
User/App accesses mount → VFS → FSKit → App Extension → Extension returns data
```

1. Developer creates an **FSKit app extension** in Xcode
2. Extension declares itself as a filesystem module via `Info.plist`
3. Extension conforms to `UnaryFileSystemExtension` or `FileSystemExtension` protocol
4. Core method: `loadResource(resource:options:replyHandler:)` makes a resource available
5. Volume operations are handled through protocol conformance on an `FSVolume` subclass

### Design flows

| Flow | Description | Status |
|------|-------------|--------|
| `FSUnaryFileSystem` | Simple: one resource → one volume | **Currently supported** |
| `FSFileSystem` | Advanced: multiple resources and volumes | Planned/partial |

For Squashbox, `FSUnaryFileSystem` is the right fit — one `.sqsh` image → one mounted volume.

### Key protocols

| Protocol | Purpose |
|----------|---------|
| `UnaryFileSystemExtension` | Entry point for the app extension |
| `FSUnaryFileSystemOperations` | Core FS lifecycle operations |
| `FSVolume.Operations` | Basic volume operations (read) |
| `FSVolume.ReadWriteOperations` | Write operations (**optional — omit for read-only**) |

### Feasibility for Squashbox: ✅ Moderate-High

**Strengths:**
- Official Apple-sanctioned approach (replaces kexts)
- User-space execution = better stability and security
- App Store distributable
- Read-only is trivially achieved by not conforming to write protocols
- Swift-native development
- macFUSE 5.0.0 already uses FSKit as a backend (validates the approach)

**Concerns:**
- **Very new** — macOS 15.4+ only (Sequoia, released 2025)
- **Limited documentation** — few real-world examples beyond Apple's passthrough sample and community samples (KhaosT/FSKitSample)
- **API still evolving** — `FSFileSystem` flow is not yet fully supported
- **Swift-only** — the extension must be written in Swift (though C/C++ can be bridged)
- **App extension model** — distribution requires a host app + extension bundle
- POSIX semantics and feature completeness are still being expanded in FSKit

---

## 4. SquashFS Libraries — Evaluation

### C/C++ Options

#### `libsquashfs` (from squashfs-tools-ng)
- **Language:** C
- **License:** LGPLv3
- **Maturity:** High — actively maintained, used in production
- **API:** Generic, abstracts disk I/O; designed for embedding
- **Compression:** All major algorithms (gzip, xz, lzo, lz4, zstd)
- **Windows:** Pre-built via MinGW; MSVC support in progress (`feature/goliath/msvc` branch)
- **macOS:** Builds natively
- **Assessment:** Strong candidate. Well-tested, full-featured, cross-platform potential. LGPL license requires dynamic linking or license compliance.

#### `libsquash`
- **Language:** C (derived from squashfuse)
- **License:** BSD-2-Clause
- **Maturity:** Moderate — less actively developed
- **API:** Mirrors syscall API (`squash_open`, `squash_read`, `squash_stat`, etc.)
- **Key feature:** No FUSE dependency; reads from memory; CMake project for VS/Xcode
- **Cross-platform:** Explicitly targets Windows, macOS, Linux
- **Assessment:** Interesting for its portability and permissive license. Simpler API but less actively maintained. Could be a good lightweight option.

#### `squashfuse`
- **Language:** C
- **License:** BSD-2-Clause
- **Maturity:** High — well-established
- **API:** Tightly coupled to FUSE
- **Cross-platform:** Linux, macOS, FreeBSD, etc. (FUSE-dependent)
- **Assessment:** Not ideal for Squashbox — we want to avoid FUSE. Internal decompression code is useful as reference, but the FUSE coupling makes it hard to reuse directly.

### Rust Options

#### `backhand`
- **Language:** Pure Rust
- **License:** MIT/Apache-2.0
- **Maturity:** Good — v0.25.0, actively developed
- **API:** `FilesystemReader` for reading, full struct traversal
- **Compression:** xz, gzip, lzo, zstd, lz4 (feature-gated)
- **Parallelism:** Built-in parallel decompression
- **Vendor formats:** Supports non-standard SquashFS variants
- **Assessment:** Excellent candidate if choosing Rust. Pure Rust = easy cross-compilation, no C dependencies. Rich API, actively maintained, permissive license.

#### `squashfs-ng-rs` (Rust bindings for libsquashfs)
- **Language:** Rust (FFI bindings to C)
- **License:** LGPLv3 (follows upstream)
- **API:** High-level safe wrappers around libsquashfs
- **Assessment:** Good if wanting libsquashfs's maturity with Rust ergonomics, but inherits the C dependency and LGPL license.

#### Other Rust crates
- **`squishy`** — High-level wrapper around `backhand` for simple read/extract
- **`squashfs_reader`** — Pure Rust, read-only, with caching
- **`squashfs-async`** — Async read + FUSE integration

### Recommendation matrix

| Library | Language | License | Windows | macOS | No FUSE | Actively Maintained | Best For |
|---------|----------|---------|---------|-------|---------|--------------------|----|
| `libsquashfs` | C | LGPLv3 | ✅ (WIP) | ✅ | ✅ | ✅ | Cross-platform C core |
| `libsquash` | C | BSD-2 | ✅ | ✅ | ✅ | ⚠️ | Lightweight embedding |
| `backhand` | Rust | MIT/Apache | ✅ | ✅ | ✅ | ✅ | Pure Rust, best DX |
| `squashfuse` | C | BSD-2 | ❌ | ⚠️ | ❌ | ✅ | Reference only |

---

## 5. Abstraction Layer Feasibility

### Goal

Create a common interface over ProjFS (Windows) and FSKit (macOS) so that:
1. The SquashFS backend only needs to implement one API
2. Other filesystem formats could be plugged in later
3. The platform provider (ProjFS/FSKit) can be swapped independently

### Conceptual mapping

Both ProjFS and FSKit follow a similar pattern — the OS calls into user-mode code when filesystem operations are needed. The core operations map well:

| Operation | ProjFS | FSKit |
|-----------|--------|-------|
| List directory | `GetDirectoryEnumeration` | `FSVolume.Operations` (enumeration) |
| Get file metadata | `GetPlaceholderInfo` | `FSVolume.Operations` (lookup/getattr) |
| Read file data | `GetFileData` | `FSVolume.Operations` (read) |
| Check path exists | `QueryFileName` | `FSVolume.Operations` (lookup) |
| Mount/start | `PrjStartVirtualizing` | `loadResource` |
| Unmount/stop | `PrjStopVirtualizing` | Extension lifecycle |

### Proposed abstraction

```
trait/interface VirtualFSProvider {
    // Lifecycle
    fn mount(source: Path, mount_point: Path) -> Result<()>
    fn unmount() -> Result<()>

    // Read operations (called by platform layer)
    fn list_directory(path: &str) -> Result<Vec<DirEntry>>
    fn get_metadata(path: &str) -> Result<FileMetadata>
    fn read_file(path: &str, offset: u64, length: u64) -> Result<Vec<u8>>
    fn path_exists(path: &str) -> Result<bool>
}
```

The platform-specific adapters (ProjFS adapter, FSKit adapter) would translate OS callbacks into calls on this interface.

### Feasibility assessment: ✅ Feasible with caveats

**What works well:**
- Both APIs are callback-driven / pull-based — the OS asks for data, the provider responds
- Read-only use case simplifies everything enormously (no write conflict handling)
- Core operations (enumerate, stat, read) map cleanly between both platforms
- SquashFS being read-only means no state synchronization issues

**Challenges:**
- **Language boundary:** ProjFS is best consumed via C/C++ or C#, FSKit requires Swift. A shared abstraction in Rust or C would need FFI bridges on both sides.
- **Threading models differ:** ProjFS delivers callbacks on a thread pool; FSKit uses structured concurrency / async
- **Metadata models differ:** NTFS attributes vs. POSIX-style modes; SquashFS stores POSIX metadata which needs translation for Windows
- **Error handling:** Different error code conventions (HRESULT vs. NSError/errno patterns)
- **Platform-specific features:** Some capabilities don't translate (e.g., ProjFS placeholder states, FSKit security-scoped resource access)

### Recommended approach

Rather than a single binary with compile-time platform selection, consider:

1. **Shared core library** (Rust or C) — handles SquashFS reading, implements the `VirtualFSProvider` interface
2. **Platform adapters** — thin, platform-native wrappers:
   - **Windows:** C# project using `Microsoft.Windows.ProjFS` NuGet, calls into core via P/Invoke or as a standalone process communicating via IPC
   - **macOS:** Swift app extension using FSKit, calls into core via Swift-C interop
3. **The adapters are intentionally thin** — they translate OS callbacks to core interface calls and core results back to OS responses

This keeps ~80-90% of code in the shared core and minimizes per-platform work.

---

## 6. Language & Build System Considerations

### Option A: Rust core + platform adapters

| Component | Language | Rationale |
|-----------|----------|-----------|
| SquashFS core | Rust | `backhand` crate is pure Rust, excellent cross-compilation |
| VFS abstraction | Rust | Define traits in Rust, expose via C FFI |
| Windows adapter | Rust or C# | Rust: use `windows` crate for ProjFS; C#: use managed API |
| macOS adapter | Swift | Required by FSKit; calls Rust core via C FFI |

**Pros:** Single language for core + potential Windows adapter, memory safety, great tooling
**Cons:** Swift-Rust FFI adds complexity, ProjFS via Rust's windows crate is less documented than the C# path

### Option B: C core + platform adapters

| Component | Language | Rationale |
|-----------|----------|-----------|
| SquashFS core | C | `libsquashfs` or `libsquash`, proven and portable |
| VFS abstraction | C headers | Simple function pointer table |
| Windows adapter | C# | ProjFS managed API is the path of least resistance |
| macOS adapter | Swift | FSKit requires Swift; Swift-C interop is excellent |

**Pros:** C interop is universal, both Swift and C# have great C interop
**Cons:** C lacks ergonomics, manual memory management, harder testing

### Option C: Mixed — Rust core, C# Windows, Swift macOS

This is a pragmatic hybrid:
- Rust `backhand` for SquashFS reading (best library for the job)
- Expose a C API from the Rust core (`#[no_mangle] extern "C"`)
- C# on Windows consumes the C API via P/Invoke + wraps ProjFS
- Swift on macOS consumes the C API + wraps FSKit

**Pros:** Best tool for each job; clean separation; C API is the universal glue
**Cons:** Three languages in one project; more complex build/CI

---

## 7. Risk Summary

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| FSKit API changes / instability | High | Medium | Start with ProjFS (stable), treat macOS as fast-follow |
| libsquashfs Windows/MSVC build issues | Medium | Medium | Use `backhand` (pure Rust) instead; or pre-build via MinGW |
| ProjFS feature not enabled on user machines | Medium | Low | Installer/setup wizard enables it; clear error messaging |
| macOS version requirement (15.4+) | Medium | Low | Document requirement clearly; no workaround exists |
| Abstraction layer over-engineering | Medium | Medium | Start with SquashFS only; add abstraction incrementally once both platforms work |
| Performance (decompression latency on read) | Low | Low | Both APIs support on-demand read; cache aggressively; `backhand` has parallel decompression |

---

## 8. Recommended Next Steps

1. **Prototype ProjFS provider** — Build a minimal C# provider using the managed API that projects a hardcoded directory tree. Validates the ProjFS integration path.

2. **Prototype SquashFS reading** — Use `backhand` (Rust) or `libsquashfs` (C) to enumerate and read files from a `.sqsh` image. Validates the SquashFS library choice.

3. **Connect the two** — Wire the SquashFS reader as the backing store for the ProjFS provider. This gives us a working Windows prototype.

4. **Evaluate FSKit** — Build a minimal FSKit app extension that projects a hardcoded tree. Given its newness, this validates that FSKit is actually usable for our needs.

5. **Design the abstraction** — With both platform prototypes in hand, extract the common interface. The real shape of the abstraction will become clear from implementation experience.

6. **Decide on language strategy** — The prototype phase will reveal whether Rust-everywhere, C-core, or mixed approach is most practical.
