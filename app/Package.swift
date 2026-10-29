// swift-tools-version:5.9
import PackageDescription
import Foundation

// Absolute path of ../target/release, worked out from this file so it also works when Xcode builds from elsewhere.
let rustLibs = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().appendingPathComponent("target/release").path

// The app links the Rust library built by `cargo build --release -p micdeck-ffi` (see build.sh).
// `-L<rustLibs>` says where to find libmicdeck_ffi.a; the frameworks are what the Rust code uses.
let package = Package(
    name: "MicDeck",
    platforms: [.macOS(.v13)],
    targets: [
        .target(name: "CMicDeck"),
        .executableTarget(
            name: "MicDeck",
            dependencies: ["CMicDeck"],
            resources: [.process("Resources")],
            linkerSettings: [
                .unsafeFlags(["-L" + rustLibs, "-lmicdeck_ffi"]),
                .linkedFramework("CoreAudio"),
                .linkedFramework("IOKit"),
                .linkedFramework("CoreFoundation"),
                .linkedFramework("Security"),
            ]
        ),
    ]
)
