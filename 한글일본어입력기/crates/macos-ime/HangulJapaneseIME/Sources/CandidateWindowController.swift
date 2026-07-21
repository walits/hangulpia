//
//  CandidateWindowController.swift
//  HangulJapaneseIME
//
//  Floating candidate window that shows conversion candidates
//  near the text insertion point — similar to Google Japanese IME.
//

import Cocoa
import InputMethodKit

class CandidateWindowController: NSObject {

    private var window: NSWindow?
    private var tableView: NSTableView?
    private var scrollView: NSScrollView?
    private var currentCandidates: [Candidate] = []

    private let rowHeight: CGFloat = 26
    private let windowWidth: CGFloat = 300
    private let maxVisibleRows = 9

    // ── Show / Hide / Select ───────────────────────────────

    func show(candidates: [Candidate], near client: Any?) {
        currentCandidates = candidates

        if window == nil {
            setupWindow()
        }

        guard let window = window, let tableView = tableView else { return }

        tableView.reloadData()

        // Calculate window size
        let visibleRows = min(candidates.count, maxVisibleRows)
        let contentHeight = CGFloat(visibleRows) * rowHeight + 2

        // Position near the cursor
        var cursorRect = NSRect.zero
        if let client = client as? (any IMKTextInput) {
            client.attributes(forCharacterIndex: 0, lineHeightRectangle: &cursorRect)
        }

        var origin = cursorRect.origin
        origin.y -= contentHeight + 4 // Below the cursor line

        // Ensure it stays on screen
        if let screen = NSScreen.main {
            let screenFrame = screen.visibleFrame
            if origin.y < screenFrame.minY {
                origin.y = cursorRect.maxY + 4 // Flip above
            }
            if origin.x + windowWidth > screenFrame.maxX {
                origin.x = screenFrame.maxX - windowWidth
            }
        }

        window.setFrame(
            NSRect(x: origin.x, y: origin.y, width: windowWidth, height: contentHeight),
            display: true
        )
        window.orderFront(nil)
    }

    func hide() {
        window?.orderOut(nil)
        currentCandidates = []
    }

    func select(index: Int) {
        guard let tableView = tableView, index >= 0, index < currentCandidates.count else { return }
        tableView.selectRowIndexes(IndexSet(integer: index), byExtendingSelection: false)
        tableView.scrollRowToVisible(index)
    }

    // ── Window Setup ────────────────────────────────────────

    private func setupWindow() {
        let win = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: windowWidth, height: 200),
            styleMask: [.borderless],
            backing: .buffered,
            defer: false
        )
        win.level = .popUpMenu
        win.isOpaque = false
        win.backgroundColor = .clear
        win.hasShadow = true
        win.isReleasedWhenClosed = false

        // Container view with rounded corners
        let container = NSVisualEffectView(frame: win.contentView!.bounds)
        container.autoresizingMask = [.width, .height]
        container.material = .popover
        container.blendingMode = .behindWindow
        container.state = .active
        container.wantsLayer = true
        container.layer?.cornerRadius = 6
        container.layer?.masksToBounds = true
        win.contentView?.addSubview(container)

        // Table view for candidates
        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("CandidateColumn"))
        column.width = windowWidth - 4

        let tv = NSTableView(frame: .zero)
        tv.addTableColumn(column)
        tv.headerView = nil
        tv.rowHeight = rowHeight
        tv.intercellSpacing = NSSize(width: 0, height: 0)
        tv.backgroundColor = .clear
        tv.gridStyleMask = []
        tv.selectionHighlightStyle = .regular
        tv.delegate = self
        tv.dataSource = self

        let sv = NSScrollView(frame: container.bounds)
        sv.autoresizingMask = [.width, .height]
        sv.documentView = tv
        sv.hasVerticalScroller = false
        sv.drawsBackground = false
        sv.borderType = .noBorder
        container.addSubview(sv)

        self.window = win
        self.tableView = tv
        self.scrollView = sv
    }
}

// ── NSTableView DataSource & Delegate ───────────────────────

extension CandidateWindowController: NSTableViewDataSource, NSTableViewDelegate {

    func numberOfRows(in tableView: NSTableView) -> Int {
        return currentCandidates.count
    }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard row < currentCandidates.count else { return nil }

        let candidate = currentCandidates[row]

        let cell = NSTableCellView(frame: NSRect(x: 0, y: 0, width: windowWidth, height: rowHeight))

        // Number label (1-9)
        let numLabel = NSTextField(labelWithString: "\(row + 1)")
        numLabel.font = NSFont.monospacedDigitSystemFont(ofSize: 12, weight: .medium)
        numLabel.textColor = .secondaryLabelColor
        numLabel.frame = NSRect(x: 8, y: 3, width: 16, height: 20)
        cell.addSubview(numLabel)

        // Surface (main text)
        let surfaceLabel = NSTextField(labelWithString: candidate.surface)
        surfaceLabel.font = NSFont.systemFont(ofSize: 15, weight: .regular)
        surfaceLabel.textColor = .labelColor
        surfaceLabel.frame = NSRect(x: 30, y: 3, width: 160, height: 20)
        cell.addSubview(surfaceLabel)

        // Reading (smaller, gray)
        let readingLabel = NSTextField(labelWithString: candidate.reading)
        readingLabel.font = NSFont.systemFont(ofSize: 11)
        readingLabel.textColor = .tertiaryLabelColor
        readingLabel.alignment = .right
        readingLabel.frame = NSRect(x: 195, y: 4, width: 95, height: 18)
        cell.addSubview(readingLabel)

        return cell
    }

    func tableView(_ tableView: NSTableView, heightOfRow row: Int) -> CGFloat {
        return rowHeight
    }
}
