//
//  main.swift
//  HangulJapaneseIME
//

import Cocoa
import InputMethodKit

autoreleasepool {
    let app = NSApplication.shared
    let delegate = AppDelegate()
    app.delegate = delegate
    app.run()
}
