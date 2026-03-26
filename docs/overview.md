# Squashbox

## Overview

Squashbox provides **first-class support for mounting SquashFS natively on Windows and macOS**. Rather than relying on FUSE or other third-party shims, Squashbox integrates directly with each platform's native projected/virtual filesystem APIs:

| Platform | Native API |
|----------|------------|
| Windows  | [ProjFS](https://learn.microsoft.com/en-us/windows/win32/projfs/projected-file-system) (Windows Projected File System) |
| macOS    | [FSKit](https://developer.apple.com/documentation/fskit) |

## Goals

1. **Native SquashFS mounting** — Mount `.sqsh` / `.squashfs` images as regular directories on both Windows and macOS using OS-provided virtual filesystem mechanisms.
2. **Platform-agnostic core** — Keep the core SquashFS reading and integration logic as platform-independent as possible, leveraging existing stable libraries where available.
3. **Filesystem abstraction layer** — Explore the feasibility of an abstraction layer over ProjFS and FSKit so that:
   - Other filesystem formats can be implemented on top of the same foundation.
   - The underlying SquashFS library can be swapped out without touching platform-specific code.

## Architecture (High-Level)

```
┌─────────────────────────────────────────────┐
│              User / Applications            │
├─────────────────────────────────────────────┤
│          OS Virtual FS Interface            │
│      ┌──────────┐    ┌──────────┐           │
│      │  ProjFS  │    │  FSKit   │           │
│      │ (Windows)│    │ (macOS)  │           │
│      └────┬─────┘    └────┬─────┘           │
│           │               │                 │
│      ┌────▼───────────────▼─────┐           │
│      │   Abstraction Layer      │           │
│      │   (common FS provider    │           │
│      │    interface)            │           │
│      └────────────┬─────────────┘           │
│                   │                         │
│      ┌────────────▼─────────────┐           │
│      │   SquashFS Core          │           │
│      │   (platform-agnostic    │           │
│      │    read / decompress)    │           │
│      └──────────────────────────┘           │
└─────────────────────────────────────────────┘
```

## Key Design Principles

- **Leverage existing libraries** — Use stable, well-tested SquashFS libraries for the core filesystem logic rather than reimplementing from scratch.
- **Minimal platform-specific surface** — Platform code should only handle the translation between OS virtual FS callbacks and the common abstraction layer.
- **Pluggable backends** — The abstraction layer should make it straightforward to swap the SquashFS library or add support for entirely different filesystem formats in the future.

## Open Questions

- Which SquashFS library to use as the core backend (e.g., `squashfs-tools-ng`, `squashfuse`, or a Rust/C# binding)?
- Exact shape of the abstraction layer API — how thin or thick should it be?
- Build system and language choice for the shared core vs. platform layers.
