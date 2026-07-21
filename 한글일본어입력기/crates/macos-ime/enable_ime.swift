#!/usr/bin/env swift
import Carbon
import Foundation

let targetBundleID = "com.hkd.inputmethod.HangulJapanese"

print("🔍 Scanning all input sources...\n")

guard let sourceList = TISCreateInputSourceList(nil, true)?.takeRetainedValue() as? [TISInputSource] else {
    print("❌ Failed to get input source list")
    exit(1)
}

var found = false
var ourSource: TISInputSource?

for source in sourceList {
    guard let rawID = TISGetInputSourceProperty(source, kTISPropertyInputSourceID) else { continue }
    let sourceID = Unmanaged<CFString>.fromOpaque(rawID).takeUnretainedValue() as String

    if !sourceID.contains("hkd") && !sourceID.contains("HangulJapanese") { continue }
    found = true

    var bundleID = "?"
    if let raw = TISGetInputSourceProperty(source, kTISPropertyBundleID) {
        bundleID = Unmanaged<CFString>.fromOpaque(raw).takeUnretainedValue() as String
    }

    var typeStr = "?"
    if let raw = TISGetInputSourceProperty(source, kTISPropertyInputSourceType) {
        typeStr = Unmanaged<CFString>.fromOpaque(raw).takeUnretainedValue() as String
    }

    var enabled = false
    if let raw = TISGetInputSourceProperty(source, kTISPropertyInputSourceIsEnabled) {
        enabled = CFBooleanGetValue(Unmanaged<CFBoolean>.fromOpaque(raw).takeUnretainedValue())
    }

    var selected = false
    if let raw = TISGetInputSourceProperty(source, kTISPropertyInputSourceIsSelected) {
        selected = CFBooleanGetValue(Unmanaged<CFBoolean>.fromOpaque(raw).takeUnretainedValue())
    }

    print("  ✅ FOUND: \(sourceID)")
    print("     Bundle:   \(bundleID)")
    print("     Type:     \(typeStr)")
    print("     Enabled:  \(enabled)")
    print("     Selected: \(selected)")

    if !enabled {
        print("  ⚡ Enabling...")
        let s = TISEnableInputSource(source)
        print("     \(s == noErr ? "✅ OK" : "❌ Error \(s)")")
        enabled = (s == noErr)
    }

    if enabled {
        ourSource = source
    }
    print()
}

if let source = ourSource {
    print("  ⚡ Attempting select...")
    let s = TISSelectInputSource(source)
    if s == noErr {
        print("  ✅ Selected! Check menu bar.")
    } else {
        print("  ⚠️  Select returned \(s). This is normal for some IME types.")
        print("     The IME should still appear in the menu bar dropdown.")
    }
} else if !found {
    print("  ❌ NOT found in TIS. Checking bundle...")
    let appPath = NSString(string: "~/Library/Input Methods/HangulJapaneseIME.app").expandingTildeInPath
    if FileManager.default.fileExists(atPath: appPath) {
        print("  ✅ App exists. Trying TISRegisterInputSource...")
        let url = URL(fileURLWithPath: appPath) as CFURL
        let s = TISRegisterInputSource(url)
        print("     Register: \(s == noErr ? "✅ OK" : "❌ Error \(s)")")
        if s == noErr {
            print("     Re-run this script to enable & select.")
        }
    } else {
        print("  ❌ App not found at \(appPath)")
    }
}

print("\nDone.")
