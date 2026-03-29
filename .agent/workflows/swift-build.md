---
description: How to build and test the Swift port of Squashbox on Windows
---
// turbo-all

## Prerequisites
- Swift 6.2.4+ installed via `winget install --id Swift.Toolchain`
- Visual Studio 2022 Build Tools installed (for MSVC linker)
- Rust toolchain installed (`rustup`), target `x86_64-pc-windows-msvc`

## Build

The build has two steps: Rust first (produces `squashbox_uniffi.lib`),
then Swift (links against it). `build.bat` does both automatically.

1. Debug build (fast, default):
```
cmd /c build.bat
```

2. Release build (optimized, for distribution):
```
cmd /c build.bat --release
```

Output binary location:
- Debug:   `.build\x86_64-unknown-windows-msvc\debug\sqb.exe`
- Release: `.build\x86_64-unknown-windows-msvc\release\sqb.exe`

## Test

1. Run the test script:
```
cmd /c test.bat
```

## Manual build (if you need to customize)

1. Set up the environment (MSVC + Swift):
```
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvarsall.bat" amd64
set "PATH=C:\Users\Macan User\AppData\Local\Programs\Swift\Toolchains\6.2.4+Asserts\usr\bin;C:\Users\Macan User\AppData\Local\Programs\Swift\Runtimes\6.2.4\usr\bin;%PATH%"
set "SDKROOT=C:\Users\Macan User\AppData\Local\Programs\Swift\Platforms\6.2.4\Windows.platform\Developer\SDKs\Windows.sdk"
```

2. Build Rust static library (must be done before Swift):
```
cargo build -p squashbox-uniffi --release
```

3. Build Swift:
```
swift build -c release
```

4. Run the CLI:
```
.build\x86_64-unknown-windows-msvc\release\sqb.exe image <path-to-squashfs-image>
```

## Regenerating UniFFI bindings

If you change the Rust API surface in `crates/squashbox-uniffi/src/lib.rs`,
regenerate the Swift bindings and copy them into place:

```
cargo build -p squashbox-uniffi --release
cargo run -p squashbox-uniffi --bin uniffi-bindgen --release -- generate --library target\release\squashbox_uniffi.dll --language swift --out-dir bindings\swift
copy bindings\swift\squashbox_uniffi.swift Sources\SquashboxUniFFI\
copy bindings\swift\squashbox_uniffiFFI.h Sources\squashbox_uniffiFFI\include\
```
