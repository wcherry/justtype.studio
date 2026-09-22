// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "ScriptoriumKit",
    // macOS is supported so the shared layer can be built and tested from the
    // command line without booting a simulator.
    platforms: [.iOS(.v17), .macOS(.v14)],
    products: [
        .library(name: "ScriptoriumKit", targets: ["ScriptoriumKit"]),
        .library(name: "ScriptoriumUI", targets: ["ScriptoriumUI"]),
    ],
    targets: [
        .target(name: "ScriptoriumKit"),
        .target(name: "ScriptoriumUI", dependencies: ["ScriptoriumKit"]),
        .testTarget(name: "ScriptoriumKitTests", dependencies: ["ScriptoriumKit"]),
    ]
)
