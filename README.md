# Squashbox

**The first app to mount SquashFS images natively on Windows, macOS, and Linux — no FUSE, no WSL, no virtual machines.**

Squashbox uses [Windows Projected File System (ProjFS)](https://learn.microsoft.com/en-us/windows/win32/projfs/projected-file-system) on Windows, and a high-performance **embedded NFSv3 server fallback** (`--nfs`) on macOS and Linux to make the contents of a SquashFS image appear as a real folder on your filesystem, fully browsable in Explorer/Finder and accessible to any program.

## What is SquashFS?

SquashFS is a compressed, read-only filesystem used everywhere in the Linux world: Ubuntu and Debian live ISOs, Docker image layers, embedded firmware images, OpenWRT router firmware, Raspberry Pi OS images, and more. Until now, accessing those images natively without pulling a full VM or installing 3rd-party kernel extensions meant extracting everything to disk first. Squashbox mounts them in-place — files are decompressed on demand, only when you read them. Support for mounting standard **ZIP archives** is also included!

## Zero-Install Architecture

[FUSE](https://github.com/libfuse/libfuse) is the standard way to implement user-space filesystems on Linux and macOS. However, Windows does not ship with FUSE support, and modern versions of macOS are heavily restricting kernel extensions like FUSE or macFUSE.

**The Squashbox Solution:**
- **On Windows:** We use **ProjFS**, Microsoft's built-in, production-hardened alternative that ships with every modern version of Windows.
- **On macOS/Linux:** We use an **Ephemeral NFSv3 Server** baked directly into Squashbox. The CLI spins up a highly optimized loopback NFS server on a random port, and natively invokes the OS `mount` command. Since NFS is natively supported by macOS and Linux kernels, this provides a zero-install, native-feeling mount experience.

## Prerequisites

**Windows:**
- Windows 10 version 1809 or newer (Windows 11 recommended)
- ProjFS feature enabled:
```powershell
# Check
Get-WindowsOptionalFeature -Online -FeatureName Client-ProjFS

# Enable (requires reboot)
Enable-WindowsOptionalFeature -Online -FeatureName Client-ProjFS -NoRestart
```

**macOS / Linux:**
- No prerequisites needed! Native NFS clients are built into the OS.

## Installation

```bash
cargo install --git https://github.com/jagtesh/squashbox squashbox-macos
```
*(Use `squashbox-windows` on Windows)*

Or build from source:
```bash
git clone https://github.com/jagtesh/squashbox
cd squashbox
cargo build --release -p squashbox-macos  # or squashbox-windows
# Binary is at target/release/sqb
```

## Usage

### Inspect an image

Print statistics and a directory listing without mounting anything:

```
sqb image <FILE>

  sqb image ubuntu.squashfs
  sqb image firmware.sqsh
```

### Mount an image

Make the image appear as a folder. The command blocks until you press Ctrl+C:

```bash
# Windows (ProjFS)
sqb mount ubuntu.squashfs C:\mnt\ubuntu

# macOS / Linux (NFS Fallback)
sqb mount --nfs ubuntu.squashfs /mnt/ubuntu
```

#### macOS Dynamic Auto-Mounting
On macOS, the mount directory argument is optional. If omitted, Squashbox will automatically:
1. Create a persistent mount point at `~/.squashbox/mounts/<filename>`.
2. Span an ephemeral NFS server on a dynamically allocated TCP port (meaning you can run an **unlimited number of concurrent mounts** without port collisions).
3. Handshake directly with the native macOS mount daemon to bypass external portmapper lookups.
4. Open the mounted directory in Finder instantly, providing a seamless experience identical to native `.dmg` files.

```bash
# Automatically detects SquashFS vs ZIP, mounts, and pops open in Finder
sqb mount --nfs my_project.zip
sqb mount --nfs firmware.sqsh
```

Press **Ctrl+C** to cleanly unmount. The mount directory is automatically cleaned up.

### What `--force` does (Windows only)

ProjFS marks a mount directory with a special filesystem tag ("reparse point"). On a clean Ctrl+C exit Squashbox removes it. If the process was killed abruptly, the tag is left behind and blocks the next mount. `--force` detects this tag and removes it before mounting. It is a no-op on a clean directory, so it is always safe to use.

## Experimental: Native macOS FSKit Extension

In addition to the zero-install NFS fallback, Squashbox includes an experimental native **FSKit File System Extension** for macOS 15.4 and later.

To build and install the FSKit extension:
```bash
./scripts/build-macos.sh --release
target/release/sqb install
```

**⚠️ Important Local Testing Requirement:**
By default, the build script uses an ad-hoc signature (`-`). However, macOS's `fskitd` daemon strictly requires all FSKit App Extensions to be cryptographically signed with a valid **Apple Developer Team ID**—even with System Integrity Protection (SIP) disabled. If you attempt to turn on the extension in System Settings with an ad-hoc signature, the toggle will silently revert back to "off" (yielding an `ENOENT: retrieving team ID` error in the system logs).

To test the extension locally, you must have an Xcode Apple Development certificate in your keychain. The `build-macos.sh` script automatically detects and uses your personal "Apple Development" identity to embed a valid Team ID. Once installed:
1. Open **System Settings → General → Login Items & Extensions → File System Extensions**.
2. Toggle on "SquashboxFS".

*Note: FSKit development is strictly sandboxed. The host app (`Squashbox.app`) must also carry the `com.apple.security.app-sandbox` entitlement, which the build script automatically applies.*

## Supported image formats

| Format     | Supported |
|------------|-----------|
| SquashFS   | ✅ Full (all compression: gzip, lz4, lzo, xz, zstd) |
| ISO / UDF  | Planned |
| ZIP / tar  | Planned |

## Architecture

Squashbox is built as three layers:

```
┌──────────────────────────────────────┐
│  sqb (CLI)                           │  squashbox-windows / squashbox-macos
│  sqb mount / sqb image               │
├──────────────────────────────────────┤
│  OS Adapter (ProjFS / NFSv3)         │  squashbox-windows / squashbox-core (nfs)
│  Translates OS callbacks →           │
│  VirtualFsProvider calls             │
├──────────────────────────────────────┤
│  squashbox-core                      │  platform-agnostic Rust
│  SquashFS parser, inode index,       │
│  case-insensitive name lookup,       │
│  on-demand decompression             │
└──────────────────────────────────────┘
```

The core library is platform-agnostic, featuring a trait `VirtualFsProvider` that powers the backend adapters without caring if the host is using ProjFS or NFS.

## License

Copyright © 2026 Jagtesh Chadha. Licensed under the [BSD 3-Clause License](LICENSE).

### Acknowledgements

- [backhand](https://github.com/wcampbell0x2a/backhand) — Rust SquashFS parser (we maintain a [fork](https://github.com/jagtesh/backhand) with Windows path fixes)
- [windows-projfs](https://github.com/ok-ryoko/windows-projfs) — Safe Rust bindings for the Windows ProjFS API
- [nfsserve](https://github.com/Arielb/nfsserve) — Embedded async Rust NFSv3 server
