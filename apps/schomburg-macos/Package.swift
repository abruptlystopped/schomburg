// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "SchomburgMacOS",
    platforms: [.macOS(.v13)],
    products: [.executable(name: "SchomburgMacOS", targets: ["SchomburgMacOS"])],
    targets: [
        .executableTarget(name: "SchomburgMacOS"),
        .testTarget(name: "SchomburgMacOSTests", dependencies: ["SchomburgMacOS"]),
    ]
)
