---
description: How to build and test the Swift port of Squashbox on Windows
---
// turbo-all

## Prerequisites
- Swift 6.2.4+ installed via `winget install --id Swift.Toolchain`
- Visual Studio 2022 Build Tools installed (for MSVC linker)

## Build

1. Run the build script (sets up VS dev environment + Swift PATH + SDKROOT automatically):
```
cmd /c build.bat
```

## Test

1. Run the test script:
```
cmd /c test.bat
```

## Build manually (if you need to customize)

1. Open a Developer Command Prompt or set up the environment:
```
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvarsall.bat" amd64
set "PATH=C:\Users\Macan User\AppData\Local\Programs\Swift\Toolchains\6.2.4+Asserts\usr\bin;C:\Users\Macan User\AppData\Local\Programs\Swift\Runtimes\6.2.4\usr\bin;%PATH%"
set "SDKROOT=C:\Users\Macan User\AppData\Local\Programs\Swift\Platforms\6.2.4\Windows.platform\Developer\SDKs\Windows.sdk"
```

2. Build:
```
swift build
```

3. Test:
```
swift test
```

4. Run the CLI:
```
swift run sqb image <path-to-squashfs-image>
```
