// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "JustTypeKit",
    // macOS is supported so the shared layer can be built and tested from the
    // command line without booting a simulator.
    platforms: [.iOS(.v17), .macOS(.v14)],
    products: [
        .library(name: "JustTypeKit", targets: ["JustTypeKit"]),
        .library(name: "JustTypeUI", targets: ["JustTypeUI"]),
    ],
    targets: [
        .target(name: "JustTypeKit"),
        .target(
            name: "JustTypeUI",
            dependencies: ["JustTypeKit"],
            // The editor is a web page that ships inside the app: one editor
            // and one set of ProseMirror positions for every client. Copied
            // rather than processed, because it is already built — see
            // spikes/4.1-mobile-editing for how it is made and why.
            resources: [.copy("Resources/Editor")]
        ),
        .testTarget(
            name: "JustTypeKitTests",
            dependencies: ["JustTypeKit"],
            resources: [.copy("Fixtures")]
        ),
    ]
)
