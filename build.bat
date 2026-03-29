@echo off
setlocal

:: Set up MSVC environment
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvarsall.bat" amd64 >nul 2>&1

:: Set up Swift toolchain
set "PATH=C:\Users\Macan User\AppData\Local\Programs\Swift\Toolchains\6.2.4+Asserts\usr\bin;C:\Users\Macan User\AppData\Local\Programs\Swift\Runtimes\6.2.4\usr\bin;%PATH%"
set "SDKROOT=C:\Users\Macan User\AppData\Local\Programs\Swift\Platforms\6.2.4\Windows.platform\Developer\SDKs\Windows.sdk"

:: Parse args: default to debug, pass --release for release
set SWIFT_CONFIG=
set CARGO_CONFIG=
if /i "%1"=="--release" (
    set SWIFT_CONFIG=-c release
    set CARGO_CONFIG=--release
    echo Build config: release
) else (
    echo Build config: debug  [pass --release for optimized build]
)

echo.
echo Swift version:
swift --version
echo.

:: Step 1: Build Rust static library (squashbox_uniffi.lib)
echo [1/2] Building Rust library (backhand + UniFFI)...
cargo build -p squashbox-uniffi %CARGO_CONFIG% 2>&1
if %ERRORLEVEL% neq 0 (
    echo ERROR: Rust build failed.
    exit /b %ERRORLEVEL%
)

:: Step 2: Build Swift package
echo.
echo [2/2] Building Swift package...
swift build %SWIFT_CONFIG% 2>&1
echo.
echo Exit code: %ERRORLEVEL%
endlocal
