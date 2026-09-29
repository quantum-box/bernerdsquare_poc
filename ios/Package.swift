// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "BanaskuTouchEntryCore",
    platforms: [.macOS(.v14)],
    products: [
        .library(name: "BanaskuTouchEntryCore", targets: ["BanaskuTouchEntryCore"])
    ],
    targets: [
        .target(
            name: "BanaskuTouchEntryCore",
            path: "BanaskuTouchEntry",
            exclude: ["BanaskuTouchEntryApp.swift", "BanaskuTouchEntry.entitlements", "ContentView.swift"],
            sources: ["Models.swift", "Services.swift", "AppStore.swift"]
        ),
        .testTarget(
            name: "BanaskuTouchEntryCoreTests",
            dependencies: ["BanaskuTouchEntryCore"],
            path: "Tests/BanaskuTouchEntryCoreTests"
        )
    ],
    swiftLanguageModes: [.v5]
)
