// swift-tools-version:6.1
import PackageDescription

let package = Package(
    name: "VoiceGhostty",
    platforms: [.macOS(.v13)],
    dependencies: [
        .package(url: "https://github.com/migueldeicaza/SwiftTerm.git", from: "1.2.0")
    ],
    targets: [
        .executableTarget(
            name: "VoiceGhostty",
            dependencies: [
                .product(name: "SwiftTerm", package: "SwiftTerm")
            ],
            path: "Sources/VoiceGhostty",
            swiftSettings: [
                .swiftLanguageMode(.v5) // 先用 Swift 5 语言模式,避免严格并发检查
            ]
        )
    ]
)
