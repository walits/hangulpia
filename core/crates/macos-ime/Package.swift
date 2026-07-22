// swift-tools-version: 5.9
//
// Package.swift
// HangulJapaneseIME
//
// Swift Package Manager config for building the IME.
// NOTE: For the full .app bundle with Info.plist, use build.sh instead.
//       This file is provided for code editing / syntax checking in Xcode.
//

import PackageDescription

let package = Package(
    name: "HangulJapaneseIME",
    platforms: [.macOS(.v13)],
    products: [
        .executable(name: "HangulJapaneseIME", targets: ["HangulJapaneseIME"]),
    ],
    targets: [
        .systemLibrary(
            name: "CHJEngine",
            path: "include",
            pkgConfig: nil,
            providers: nil
        ),
        .executableTarget(
            name: "HangulJapaneseIME",
            dependencies: ["CHJEngine"],
            path: "HangulJapaneseIME/Sources",
            swiftSettings: [
                .unsafeFlags([
                    "-import-objc-header",
                    "HangulJapaneseIME/Sources/BridgingHeader.h",
                ]),
            ],
            linkerSettings: [
                .linkedFramework("Cocoa"),
                .linkedFramework("InputMethodKit"),
                .linkedFramework("Security"),
                .unsafeFlags(["-L", "../../target/release"]),
                .linkedLibrary("hj_engine"),
            ]
        ),
    ]
)
