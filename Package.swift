// swift-tools-version: 6.0
// The swift-tools-version declares the minimum version of Swift required to build this package.

import PackageDescription

// ── Platform-conditional dependencies ──────────────────────────────────
// ProjFS driver is only available on Windows
#if os(Windows)
let driverTargets: [Target] = [
    .target(
        name: "CProjFS",
        path: "Sources/Drivers/Windows/CProjFS",
        cSettings: [
            .unsafeFlags([
                "-I", "C:/Program Files (x86)/Windows Kits/10/Include/10.0.26100.0/um",
                "-I", "C:/Program Files (x86)/Windows Kits/10/Include/10.0.26100.0/shared",
            ]),
        ],
        linkerSettings: [
            .unsafeFlags([
                "-L", "C:/Program Files (x86)/Windows Kits/10/Lib/10.0.26100.0/um/x64",
            ]),
            .linkedLibrary("ProjectedFSLib"),
            .linkedLibrary("Ole32"),
        ]
    ),
    .target(
        name: "ProjFsDriver",
        dependencies: ["SquashboxCore", "CProjFS"],
        path: "Sources/Drivers/Windows/ProjFsDriver"
    ),
]
let driverDependencies: [Target.Dependency] = ["ProjFsDriver"]
#else
let driverTargets: [Target] = []
let driverDependencies: [Target.Dependency] = []
#endif

let package = Package(
    name: "Squashbox",
    platforms: [
        .macOS(.v14),
    ],
    products: [
        .executable(name: "sqb", targets: ["sqb"]),
        .library(name: "SquashboxCore", targets: ["SquashboxCore"]),
        .library(name: "SquashFsSource", targets: ["SquashFsSource"]),
        .library(name: "SwiftSquashFS", targets: ["SwiftSquashFS"]),
    ],
    dependencies: [
        .package(url: "https://github.com/apple/swift-argument-parser.git", from: "1.5.0"),
    ],
    targets: [
        // ─── CORE (platform-agnostic) ───────────────────────────────────
        .target(
            name: "SquashboxCore",
            path: "Sources/SquashboxCore"
        ),

        // ─── VENDORED COMPRESSION LIBRARIES ─────────────────────────────
        //
        // CZlib: Vendored zlib source (zlib license — fully permissive)
        // All source files compiled from vendor/zlib, headers in include/
        //
        .target(
            name: "CZlib",
            path: "Sources/CZlib",
            publicHeadersPath: "include",
            cSettings: [
                .headerSearchPath("include"),
            ]
        ),
        //
        // CZstd: Vendored zstd source (BSD license — fully permissive)
        // Source files in common/, compress/, decompress/ subdirectories
        //
        .target(
            name: "CZstd",
            path: "Sources/CZstd",
            exclude: [],
            publicHeadersPath: "include",
            cSettings: [
                .headerSearchPath("."),
                .headerSearchPath("include"),
                .headerSearchPath("common"),
                .headerSearchPath("compress"),
                .headerSearchPath("decompress"),
                // zstd needs these defines for the amalgamated build
                .define("ZSTD_MULTITHREAD", to: "0"),
                .define("ZSTD_LEGACY_SUPPORT", to: "0"),
            ]
        ),
        //
        // CLzma: Vendored liblzma from xz-utils (0BSD license — public domain equivalent)
        // Provides XZ/LZMA decompression for SquashFS images (the most common format)
        //
        .target(
            name: "CLzma",
            path: "Sources/CLzma",
            exclude: [
                // Tablegen files are build-time generators, not library code
                "check/crc32_tablegen.c",
                "check/crc64_tablegen.c",
                "lzma/fastpos_tablegen.c",
                "rangecoder/price_tablegen.c",
                // Use _fast.c variants; _small.c ones conflict
                "check/crc32_small.c",
                "check/crc64_small.c",
                // Multithreaded stream encode/decode (we don't use threading)
                "common/stream_decoder_mt.c",
                "common/stream_encoder_mt.c",
                "common/outqueue.c",
                "common/hardware_cputhreads.c",
                // tuklib utilities not needed for library-only build
                "common/tuklib_exit.c",
                "common/tuklib_mbstr_fw.c",
                "common/tuklib_mbstr_width.c",
                "common/tuklib_open_stdxxx.c",
                "common/tuklib_progname.c",
            ],
            publicHeadersPath: "include",
            cSettings: [
                .headerSearchPath("."),
                .headerSearchPath("include"),
                .headerSearchPath("api"),
                .headerSearchPath("common"),
                .headerSearchPath("check"),
                .headerSearchPath("lz"),
                .headerSearchPath("lzma"),
                .headerSearchPath("rangecoder"),
                .headerSearchPath("delta"),
                .headerSearchPath("simple"),
                .define("HAVE_CONFIG_H"),
            ]
        ),

        // ─── SWIFT SQUASHFS LIBRARY (pure Swift + vendored C, BSD-3) ────
        .target(
            name: "SwiftSquashFS",
            dependencies: ["CZlib", "CZstd", "CLzma"],
            path: "Sources/SwiftSquashFS"
        ),

        // ─── SOURCES (format plugins, platform-agnostic) ────────────────
        .target(
            name: "SquashFsSource",
            dependencies: ["SquashboxCore", "SwiftSquashFS"],
            path: "Sources/SquashFsSource"
        ),

        // ─── CLI ────────────────────────────────────────────────────────
        .executableTarget(
            name: "sqb",
            dependencies: [
                "SquashboxCore",
                "SquashFsSource",
                .product(name: "ArgumentParser", package: "swift-argument-parser"),
            ] + driverDependencies,
            path: "Sources/sqb"
        ),

        // ─── TESTS ─────────────────────────────────────────────────────
        .testTarget(
            name: "SquashboxCoreTests",
            dependencies: ["SquashboxCore"],
            path: "Tests/SquashboxCoreTests"
        ),
        .testTarget(
            name: "SquashFsSourceTests",
            dependencies: ["SquashFsSource", "SquashboxCore"],
            path: "Tests/SquashFsSourceTests"
        ),
        .testTarget(
            name: "SwiftSquashFSTests",
            dependencies: ["SwiftSquashFS"],
            path: "Tests/SwiftSquashFSTests"
        ),
    ] + driverTargets
)
