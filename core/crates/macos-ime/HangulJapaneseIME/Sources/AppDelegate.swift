//
//  AppDelegate.swift
//  HangulJapaneseIME
//

import Cocoa
import Carbon
import InputMethodKit

class AppDelegate: NSObject, NSApplicationDelegate {

    var server: IMKServer?

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSLog("[HJ] ====== applicationDidFinishLaunching ======")

        // 1. Initialize Rust engine
        let appSupport = FileManager.default.urls(
            for: .applicationSupportDirectory, in: .userDomainMask
        ).first!.appendingPathComponent("HangulJapaneseIME")
        try? FileManager.default.createDirectory(at: appSupport, withIntermediateDirectories: true)
        let dbPath = appSupport.appendingPathComponent("hj.db").path

        HJEngine.shared.initialize(dbPath: dbPath)

        // 2. Read connection info from our own Info.plist
        let bundleID = Bundle.main.bundleIdentifier ?? "com.hkd.inputmethod.HangulJapanese"
        let connectionName = "\(bundleID)_Connection"

        NSLog("[HJ] Bundle ID:  \(bundleID)")
        NSLog("[HJ] Connection: \(connectionName)")

        // 3. Create IMKServer
        server = IMKServer(name: connectionName, bundleIdentifier: bundleID)

        if let s = server {
            NSLog("[HJ] ✅ IMKServer created: \(s)")
        } else {
            NSLog("[HJ] ❌ IMKServer creation FAILED!")
        }

        // 4. Register with TIS
        let bundleURL = Bundle.main.bundleURL as CFURL
        let regStatus = TISRegisterInputSource(bundleURL)
        NSLog("[HJ] TISRegisterInputSource: \(regStatus == noErr ? "OK" : "Error \(regStatus)")")

        NSLog("[HJ] ====== Ready ======")
    }

    func applicationWillTerminate(_ notification: Notification) {
        HJEngine.shared.shutdown()
    }
}
