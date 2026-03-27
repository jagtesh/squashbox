# Squashbox

**The first app to mount SquashFS images natively on Windows — no FUSE, no WSL, no virtual machines.**

Squashbox uses [Windows Projected File System (ProjFS)](https://learn.microsoft.com/en-us/windows/win32/projfs/projected-file-system) to make the contents of a SquashFS image appear as a real folder on your filesystem, fully browsable in Explorer and accessible to any program.

## What is SquashFS?

SquashFS is a compressed, read-only filesystem used everywhere in the Linux world: Ubuntu and Debian live ISOs, Docker image layers, embedded firmware images, OpenWRT router firmware, Raspberry Pi OS images, and more. Until now, accessing those images on Windows meant either a WSL session, a VM, or a third-party tool that extracted everything to disk first. Squashbox mounts them in-place — files are decompressed on demand, only when you read them.

## Why ProjFS instead of FUSE?

[FUSE](https://github.com/libfuse/libfuse) is the standard way to implement user-space filesystems on Linux and macOS. Windows does not ship with FUSE support — third-party FUSE shims exist, but they require installing a kernel driver and carry their own security implications. ProjFS is Microsoft's own, built-in, production-hardened alternative that ships with every modern version of Windows. It requires no extra drivers and no administrator rights for reading.

## Prerequisites

- Windows 10 version 1809 or newer (Windows 11 recommended)
- ProjFS feature enabled (see below)

**Check/enable ProjFS:**
```powershell
# Check
Get-WindowsOptionalFeature -Online -FeatureName Client-ProjFS

# Enable (requires reboot)
Enable-WindowsOptionalFeature -Online -FeatureName Client-ProjFS -NoRestart
```

## Installation

```powershell
cargo install --git https://github.com/jagtesh/squashbox squashbox-windows
```

Or build from source:
```powershell
git clone https://github.com/jagtesh/squashbox
cd squashbox
cargo build --release -p squashbox-windows
# Binary is at target\release\sqb.exe
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

```
sqb mount <FILE> <DIR>

  sqb mount ubuntu.squashfs C:\mnt\ubuntu
  sqb mount firmware.sqsh   C:\mnt\firmware
```

Press **Ctrl+C** to unmount. The mount directory is left clean automatically.

If you killed a previous mount without Ctrl+C (e.g. via Task Manager), the directory may have a stale state. Use `--force` to clear it:

```
sqb mount --force ubuntu.squashfs C:\mnt\ubuntu
```

### What `--force` does

ProjFS marks a mount directory with a special filesystem tag ("reparse point"). On a clean Ctrl+C exit Squashbox removes it. If the process was killed abruptly, the tag is left behind and blocks the next mount. `--force` detects this tag and removes it before mounting. It is a no-op on a clean directory, so it is always safe to use.

## Supported image formats

| Format     | Supported |
|------------|-----------|
| SquashFS   | ✅ Full (all compression: gzip, lz4, lzo, xz, zstd) |
| ISO / UDF  | Planned |
| ZIP / tar  | Planned |

## Architecture

Squashbox is built as three layers:

```
┌────────────────────────────────────┐
│  sqb (CLI)                         │  squashbox-windows
│  sqb mount / sqb image             │
├────────────────────────────────────┤
│  ProjFS adapter                    │  squashbox-windows
│  Translates OS callbacks →         │
│  VirtualFsProvider calls           │
├────────────────────────────────────┤
│  squashbox-core                    │  platform-agnostic Rust
│  SquashFS parser, inode index,     │
│  case-insensitive name lookup,     │
│  on-demand decompression           │
└────────────────────────────────────┘
```

The core library is platform-agnostic and is designed to also drive a future macOS driver using [FSKit](https://developer.apple.com/documentation/fskit).

### A note on Linux filenames

SquashFS is a Linux-native format. Linux filenames can legally contain characters that Windows forbids: backslash (`\`), colon (`:`), asterisk (`*`), question mark (`?`), and others. Squashbox maps these characters to the Unicode [Private Use Area](https://en.wikipedia.org/wiki/Private_Use_Areas) (PUA) so they appear safely in Windows Explorer while remaining lossless and internally consistent.

Similarly, Linux filesystems are case-sensitive — `Makefile` and `makefile` can coexist in the same directory. Windows is case-insensitive. Squashbox resolves collisions by appending a numeric suffix to the later entry (`makefile (1)`), ensuring all files are visible.

## Future goals

Squashbox as it stands is a **proof of concept and a solid foundation**. The current architecture is intentionally simple: `sqb mount` is a foreground process that acts as both the server (serving ProjFS callbacks) and the user interface. Pressing Ctrl+C stops both.

For production use, the right approach is to split these responsibilities:

- **A background service** (`sqbd`) that runs as a Windows service and holds the ProjFS sessions alive.
- **A lightweight CLI frontend** (`sqb`) that communicates with the service over a local socket or named pipe to mount, unmount, and list images — without the CLI process itself needing to stay alive.

This architecture is how macOS's `diskimagectl` / `hdiutil`, Linux's `gvfsd`, and Windows's own VHD mounting work. It would make Squashbox:
- startable at login with multiple images pre-mounted
- manageable without an open terminal window
- integratable with Explorer context menus ("Mount with Squashbox")

The core library (`squashbox-core`) and the ProjFS adapter are already written in a way that supports this transition cleanly — the service model is purely a CLI and IPC layer change, not a rewrite.

---

## License

Copyright © 2026 Jagtesh Chadha. Licensed under the [BSD 3-Clause License](LICENSE).

### Acknowledgements

- [backhand](https://github.com/wcampbell0x2a/backhand) — Rust SquashFS parser (we maintain a [fork](https://github.com/jagtesh/backhand) with Windows path fixes)
- [windows-projfs](https://github.com/ok-ryoko/windows-projfs) — Safe Rust bindings for the Windows ProjFS API
