// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "SimpleFP",
    platforms: [.macOS(.v15)],
    targets: [
        // Built by build.sh from crates/ffi (Rust engine + UniFFI scaffolding).
        .binaryTarget(name: "SimpleFPCore", path: "Frameworks/SimpleFPCore.xcframework"),
        .executableTarget(
            name: "SimpleFP",
            dependencies: ["SimpleFPCore"],
            path: "Sources/SimpleFP",
            swiftSettings: [.swiftLanguageMode(.v6)]
        ),
    ]
)
