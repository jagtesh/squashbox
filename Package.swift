// swift-tools-version: 6.0
// The swift-tools-version declares the minimum version of Swift required to build this package.

import PackageDescription

let package = Package(
    name: "Squashbox",
    platforms: [
        .macOS(.v14),
    ],
    products: [
        .executable(name: "sqb", targets: ["sqb"]),
        .library(name: "SquashboxCore", targets: ["SquashboxCore"]),
        .library(name: "SquashFsSource", targets: ["SquashFsSource"]),
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

        // ─── SOURCES (format plugins, platform-agnostic) ────────────────
        //
        // CSquashFS: C bridging target for libsqfs
        // TODO: Set up once libsqfs acquisition strategy is decided.
        //       For now, SquashFsSource compiles without CSquashFS and
        //       the provider implementation is stubbed.
        //
        .target(
            name: "SquashFsSource",
            dependencies: ["SquashboxCore"],
            path: "Sources/SquashFsSource"
        ),

        // ─── DRIVERS (platform-specific OS integration) ─────────────────
        //
        // Windows ProjFS driver — only built on Windows
        //
        // TODO: Uncomment when CProjFS bridge is implemented.
        // .target(
        //     name: "CProjFS",
        //     path: "Sources/Drivers/Windows/CProjFS",
        //     cSettings: [
        //         .unsafeFlags(["-I", "C:/Program Files (x86)/Windows Kits/10/Include"]),
        //     ],
        //     linkerSettings: [
        //         .linkedLibrary("ProjectedFSLib"),
        //     ]
        // ),
        // .target(
        //     name: "ProjFsDriver",
        //     dependencies: ["SquashboxCore", "CProjFS"],
        //     path: "Sources/Drivers/Windows/ProjFsDriver"
        // ),

        // ─── CLI ────────────────────────────────────────────────────────
        .executableTarget(
            name: "sqb",
            dependencies: [
                "SquashboxCore",
                "SquashFsSource",
                .product(name: "ArgumentParser", package: "swift-argument-parser"),
            ],
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
    ]
)
