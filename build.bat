@echo off
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvarsall.bat" amd64 >nul 2>&1
set "PATH=C:\Users\Macan User\AppData\Local\Programs\Swift\Toolchains\6.2.4+Asserts\usr\bin;C:\Users\Macan User\AppData\Local\Programs\Swift\Runtimes\6.2.4\usr\bin;%PATH%"
set "SDKROOT=C:\Users\Macan User\AppData\Local\Programs\Swift\Platforms\6.2.4\Windows.platform\Developer\SDKs\Windows.sdk"
echo Swift version:
swift --version
echo.
echo Building...
swift build 2>&1
echo.
echo Exit code: %ERRORLEVEL%
