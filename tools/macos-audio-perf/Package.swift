// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "handy-audio-perf",
    platforms: [.macOS(.v13)],
    targets: [
        .executableTarget(
            name: "handy-audio-perf",
            path: "Sources/handy-audio-perf"
        )
    ]
)
