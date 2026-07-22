//
//  HJInputController.swift
//  HangulJapaneseIME
//
//  IMKInputController subclass — core keyboard event handler.
//  Receives hangul keystrokes, shows inline composition (hiragana),
//  and presents a candidate window for kanji/kana selection.
//

import Cocoa
import InputMethodKit

// ────────────────────────────────────────────────────────────────────
// MARK: - Hangul Jamo Composition (Korean syllable assembly)
// ────────────────────────────────────────────────────────────────────

/// Unicode Hangul syllable assembly.
/// Composes individual jamo (ㄱ ㅏ ㄴ) into syllable blocks (간).
private struct HangulComposer {
    // Jamo tables
    static let choseong: [Character] = [
        "ㄱ","ㄲ","ㄴ","ㄷ","ㄸ","ㄹ","ㅁ","ㅂ","ㅃ","ㅅ",
        "ㅆ","ㅇ","ㅈ","ㅉ","ㅊ","ㅋ","ㅌ","ㅍ","ㅎ"
    ]
    static let jungseong: [Character] = [
        "ㅏ","ㅐ","ㅑ","ㅒ","ㅓ","ㅔ","ㅕ","ㅖ","ㅗ","ㅘ",
        "ㅙ","ㅚ","ㅛ","ㅜ","ㅝ","ㅞ","ㅟ","ㅠ","ㅡ","ㅢ","ㅣ"
    ]
    static let jongseong: [Character?] = [
        nil,
        "ㄱ","ㄲ","ㄳ","ㄴ","ㄵ","ㄶ","ㄷ","ㄹ","ㄺ","ㄻ",
        "ㄼ","ㄽ","ㄾ","ㄿ","ㅀ","ㅁ","ㅂ","ㅄ","ㅅ","ㅆ",
        "ㅇ","ㅈ","ㅊ","ㅋ","ㅌ","ㅍ","ㅎ"
    ]

    static func isChoseong(_ c: Character) -> Bool {
        choseong.contains(c)
    }

    static func isJungseong(_ c: Character) -> Bool {
        jungseong.contains(c)
    }

    static func compose(cho: Int, jung: Int, jong: Int = 0) -> Character? {
        let code = 0xAC00 + cho * 21 * 28 + jung * 28 + jong
        return Unicode.Scalar(code).map { Character($0) }
    }

    static func choIndex(_ c: Character) -> Int? {
        choseong.firstIndex(of: c)
    }

    static func jungIndex(_ c: Character) -> Int? {
        jungseong.firstIndex(of: c)
    }

    static func jongIndex(_ c: Character) -> Int? {
        jongseong.firstIndex(of: c)
    }
}

// ────────────────────────────────────────────────────────────────────
// MARK: - Input Controller
// ────────────────────────────────────────────────────────────────────

/// The IMKInputController subclass that handles all keyboard input.
@objc(HJInputController)
class HJInputController: IMKInputController {

    // ── State ───────────────────────────────────────────────────

    /// Accumulated hangul jamo buffer (raw jamo characters).
    private var jamoBuffer: [Character] = []

    /// Composed hangul syllables ready for conversion.
    private var hangulBuffer: String = ""

    /// Current candidates from the engine.
    private var candidates: [Candidate] = []

    /// Index of selected candidate (-1 = none).
    private var selectedIndex: Int = -1

    /// Whether we're showing candidates.
    private var showingCandidates: Bool = false

    /// Candidate window controller.
    private lazy var candidateWindowController = CandidateWindowController()

    // ── IMKInputController overrides ────────────────────────────

    override func activateServer(_ sender: Any!) {
        super.activateServer(sender)
        reset()
    }

    override func deactivateServer(_ sender: Any!) {
        commitCurrentText(sender)
        super.deactivateServer(sender)
    }

    /// Main key event handler.
    override func handle(_ event: NSEvent!, client sender: Any!) -> Bool {
        guard let event = event, event.type == .keyDown else { return false }
        guard let client = sender as? (any IMKTextInput) else { return false }

        let keyCode = event.keyCode
        let chars = event.characters ?? ""

        // ── Special keys ────────────────────────────────────

        // Enter / Return — commit
        if keyCode == 36 {
            return handleReturn(client: client)
        }

        // Escape — cancel composition
        if keyCode == 53 {
            return handleEscape(client: client)
        }

        // Backspace
        if keyCode == 51 {
            return handleBackspace(client: client)
        }

        // Space — commit current or select candidate
        if keyCode == 49 {
            return handleSpace(client: client)
        }

        // Tab — next candidate
        if keyCode == 48 {
            return handleTab(client: client)
        }

        // Arrow Down — open / navigate candidates
        if keyCode == 125 {
            return handleArrowDown(client: client)
        }

        // Arrow Up — navigate candidates
        if keyCode == 126 {
            return handleArrowUp(client: client)
        }

        // Number keys 1-9 for candidate selection
        if showingCandidates, let num = chars.first?.wholeNumberValue, num >= 1, num <= 9 {
            let idx = num - 1
            if idx < candidates.count {
                selectedIndex = idx
                commitCandidate(client: client)
                return true
            }
        }

        // ── Hangul jamo input (direct jamo or QWERTY mapped) ─

        if let ch = chars.first {
            // First check: direct jamo input
            if isHangulJamo(ch) {
                return handleJamo(ch, client: client)
            }
            // Second check: QWERTY → 두벌식 mapping
            if let jamo = qwertyToJamo(ch) {
                return handleJamo(jamo, client: client)
            }
        }

        // ── Non-hangul character — commit and pass through ─

        if !hangulBuffer.isEmpty || !jamoBuffer.isEmpty {
            commitCurrentText(client)
        }
        return false
    }

    // ── QWERTY → 두벌식 한글 매핑 ────────────────────────────

    /// Standard Korean 2-set (두벌식) keyboard layout mapping.
    /// Maps physical QWERTY keys to Hangul jamo.
    private static let qwertyToJamo: [Character: Character] = [
        // Consonants (초성/종성)
        "r": "ㄱ", "R": "ㄲ",
        "s": "ㄴ",
        "e": "ㄷ", "E": "ㄸ",
        "f": "ㄹ",
        "a": "ㅁ",
        "q": "ㅂ", "Q": "ㅃ",
        "t": "ㅅ", "T": "ㅆ",
        "d": "ㅇ",
        "w": "ㅈ", "W": "ㅉ",
        "c": "ㅊ",
        "z": "ㅋ",
        "x": "ㅌ",
        "v": "ㅍ",
        "g": "ㅎ",
        // Vowels (중성)
        "k": "ㅏ",
        "i": "ㅑ",
        "j": "ㅓ",
        "u": "ㅕ",
        "h": "ㅗ",
        "y": "ㅛ",
        "n": "ㅜ",
        "b": "ㅠ",
        "m": "ㅡ",
        "l": "ㅣ",
        "o": "ㅐ", "O": "ㅒ",
        "p": "ㅔ", "P": "ㅖ",
    ]

    // ── Jamo handling ───────────────────────────────────────

    private func isHangulJamo(_ ch: Character) -> Bool {
        let s = ch.unicodeScalars.first?.value ?? 0
        return s >= 0x3131 && s <= 0x3163
    }

    /// Check if a QWERTY key maps to hangul jamo.
    private func qwertyToJamo(_ ch: Character) -> Character? {
        return HJInputController.qwertyToJamo[ch]
    }

    private func handleJamo(_ ch: Character, client: any IMKTextInput) -> Bool {
        jamoBuffer.append(ch)
        recomposeHangul()
        updateComposition(client: client)
        return true
    }

    /// Recompose jamo buffer into hangul syllables.
    /// Implements standard 2-set (두벌식) hangul composition.
    private func recomposeHangul() {
        var result = ""
        var i = 0
        let jamos = jamoBuffer

        while i < jamos.count {
            let ch = jamos[i]

            // If it's a consonant (choseong candidate)
            if let choIdx = HangulComposer.choIndex(ch) {
                // Need a vowel next to form a syllable
                if i + 1 < jamos.count, let jungIdx = HangulComposer.jungIndex(jamos[i + 1]) {
                    // Check for compound vowel
                    var finalJung = jungIdx
                    var vowelLen = 1
                    if i + 2 < jamos.count, let nextJung = HangulComposer.jungIndex(jamos[i + 2]) {
                        if let compound = compoundJungseong(jungIdx, nextJung) {
                            finalJung = compound
                            vowelLen = 2
                        }
                    }

                    // Check for jongseong (final consonant)
                    let afterVowel = i + 1 + vowelLen
                    if afterVowel < jamos.count, let jongIdx = HangulComposer.jongIndex(jamos[afterVowel]) {
                        // Check if the jongseong is actually the choseong of the next syllable
                        if afterVowel + 1 < jamos.count, HangulComposer.isJungseong(jamos[afterVowel + 1]) {
                            // Next character is a vowel → this consonant starts a new syllable
                            if let syllable = HangulComposer.compose(cho: choIdx, jung: finalJung) {
                                result.append(syllable)
                            }
                            i += 1 + vowelLen
                            continue
                        }

                        // Check for compound jongseong
                        var finalJong = jongIdx
                        var jongLen = 1
                        if afterVowel + 1 < jamos.count, let nextJong = HangulComposer.jongIndex(jamos[afterVowel + 1]) {
                            if let compound = compoundJongseong(jongIdx, nextJong) {
                                // But if after compound there's a vowel, don't combine
                                if afterVowel + 2 < jamos.count, HangulComposer.isJungseong(jamos[afterVowel + 2]) {
                                    // Split: first jong stays, second becomes next choseong
                                } else {
                                    finalJong = compound
                                    jongLen = 2
                                }
                            }
                        }

                        if let syllable = HangulComposer.compose(cho: choIdx, jung: finalJung, jong: finalJong) {
                            result.append(syllable)
                        }
                        i += 1 + vowelLen + jongLen
                        continue
                    }

                    // No jongseong
                    if let syllable = HangulComposer.compose(cho: choIdx, jung: finalJung) {
                        result.append(syllable)
                    }
                    i += 1 + vowelLen
                    continue
                }

                // Consonant alone (no vowel follows)
                result.append(ch)
                i += 1
                continue
            }

            // Vowel without preceding consonant
            if HangulComposer.isJungseong(ch) {
                result.append(ch)
                i += 1
                continue
            }

            // Unknown — pass through
            result.append(ch)
            i += 1
        }

        hangulBuffer = result
    }

    /// Compound jungseong (ㅗ+ㅏ→ㅘ, etc.)
    private func compoundJungseong(_ a: Int, _ b: Int) -> Int? {
        switch (a, b) {
        case (8, 0): return 9    // ㅗ+ㅏ→ㅘ
        case (8, 1): return 10   // ㅗ+ㅐ→ㅙ
        case (8, 20): return 11  // ㅗ+ㅣ→ㅚ
        case (13, 4): return 14  // ㅜ+ㅓ→ㅝ
        case (13, 5): return 15  // ㅜ+ㅔ→ㅞ
        case (13, 20): return 16 // ㅜ+ㅣ→ㅟ
        case (18, 20): return 19 // ㅡ+ㅣ→ㅢ
        default: return nil
        }
    }

    /// Compound jongseong (ㄱ+ㅅ→ㄳ, etc.)
    private func compoundJongseong(_ a: Int, _ b: Int) -> Int? {
        switch (a, b) {
        case (1, 19): return 3    // ㄱ+ㅅ→ㄳ
        case (4, 22): return 5    // ㄴ+ㅈ→ㄵ
        case (4, 27): return 6    // ㄴ+ㅎ→ㄶ
        case (8, 1): return 9     // ㄹ+ㄱ→ㄺ
        case (8, 16): return 10   // ㄹ+ㅁ→ㄻ
        case (8, 17): return 11   // ㄹ+ㅂ→ㄼ
        case (8, 19): return 12   // ㄹ+ㅅ→ㄽ
        case (8, 25): return 13   // ㄹ+ㅌ→ㄾ
        case (8, 26): return 14   // ㄹ+ㅍ→ㄿ
        case (8, 27): return 15   // ㄹ+ㅎ→ㅀ
        case (17, 19): return 18  // ㅂ+ㅅ→ㅄ
        default: return nil
        }
    }

    // ── Composition display ─────────────────────────────────

    private func updateComposition(client: any IMKTextInput) {
        // Get hiragana preview from the engine
        let displayText: String
        if !hangulBuffer.isEmpty {
            if let hiragana = HJEngine.shared.toHiragana(hangul: hangulBuffer) {
                displayText = hiragana
            } else {
                displayText = hangulBuffer
            }
        } else {
            displayText = String(jamoBuffer)
        }

        // Show inline composition (underlined text in the text field)
        client.setMarkedText(
            NSAttributedString(string: displayText, attributes: markAttributes()),
            selectionRange: NSRange(location: displayText.utf16.count, length: 0),
            replacementRange: NSRange(location: NSNotFound, length: NSNotFound)
        )

        // Update candidates
        if !hangulBuffer.isEmpty {
            candidates = HJEngine.shared.convert(hangul: hangulBuffer)
            if !candidates.isEmpty {
                showingCandidates = true
                selectedIndex = 0
                candidateWindowController.show(candidates: candidates, near: client)
            } else {
                hideCandidates()
            }
        }
    }

    private func markAttributes() -> [NSAttributedString.Key: Any] {
        return [
            .underlineStyle: NSUnderlineStyle.single.rawValue,
            .foregroundColor: NSColor.textColor
        ]
    }

    // ── Candidate handling ──────────────────────────────────

    private func hideCandidates() {
        showingCandidates = false
        selectedIndex = -1
        candidateWindowController.hide()
    }

    private func commitCandidate(client: any IMKTextInput) {
        guard selectedIndex >= 0, selectedIndex < candidates.count else {
            commitCurrentText(client)
            return
        }

        let selected = candidates[selectedIndex]
        client.insertText(
            selected.surface,
            replacementRange: NSRange(location: NSNotFound, length: NSNotFound)
        )
        reset()
    }

    // ── Key handlers ────────────────────────────────────────

    private func handleReturn(client: any IMKTextInput) -> Bool {
        if showingCandidates && selectedIndex >= 0 {
            commitCandidate(client: client)
            return true
        }
        if !hangulBuffer.isEmpty || !jamoBuffer.isEmpty {
            commitCurrentText(client)
            return true
        }
        return false
    }

    private func handleEscape(client: any IMKTextInput) -> Bool {
        if showingCandidates {
            hideCandidates()
            return true
        }
        if !hangulBuffer.isEmpty || !jamoBuffer.isEmpty {
            client.setMarkedText(
                "", selectionRange: NSRange(location: 0, length: 0),
                replacementRange: NSRange(location: NSNotFound, length: NSNotFound)
            )
            reset()
            return true
        }
        return false
    }

    private func handleBackspace(client: any IMKTextInput) -> Bool {
        if jamoBuffer.isEmpty {
            return false
        }
        jamoBuffer.removeLast()
        if jamoBuffer.isEmpty {
            client.setMarkedText(
                "", selectionRange: NSRange(location: 0, length: 0),
                replacementRange: NSRange(location: NSNotFound, length: NSNotFound)
            )
            reset()
        } else {
            recomposeHangul()
            updateComposition(client: client)
        }
        return true
    }

    private func handleSpace(client: any IMKTextInput) -> Bool {
        if showingCandidates && selectedIndex >= 0 {
            commitCandidate(client: client)
            return true
        }
        if !hangulBuffer.isEmpty {
            // Commit top candidate or hiragana, then insert space
            if let top = candidates.first {
                client.insertText(
                    top.surface,
                    replacementRange: NSRange(location: NSNotFound, length: NSNotFound)
                )
            } else {
                commitCurrentText(client)
            }
            reset()
            return true
        }
        return false
    }

    private func handleTab(client: any IMKTextInput) -> Bool {
        guard showingCandidates, !candidates.isEmpty else { return false }
        selectedIndex = (selectedIndex + 1) % candidates.count
        candidateWindowController.select(index: selectedIndex)
        return true
    }

    private func handleArrowDown(client: any IMKTextInput) -> Bool {
        guard showingCandidates, !candidates.isEmpty else { return false }
        selectedIndex = min(selectedIndex + 1, candidates.count - 1)
        candidateWindowController.select(index: selectedIndex)
        return true
    }

    private func handleArrowUp(client: any IMKTextInput) -> Bool {
        guard showingCandidates, !candidates.isEmpty else { return false }
        selectedIndex = max(selectedIndex - 1, 0)
        candidateWindowController.select(index: selectedIndex)
        return true
    }

    // ── Commit / Reset ──────────────────────────────────────

    private func commitCurrentText(_ sender: Any?) {
        guard let client = sender as? (any IMKTextInput) else { return }

        let text: String
        if showingCandidates, selectedIndex >= 0, selectedIndex < candidates.count {
            text = candidates[selectedIndex].surface
        } else if !hangulBuffer.isEmpty {
            text = HJEngine.shared.toHiragana(hangul: hangulBuffer) ?? hangulBuffer
        } else {
            text = String(jamoBuffer)
        }

        if !text.isEmpty {
            client.insertText(
                text,
                replacementRange: NSRange(location: NSNotFound, length: NSNotFound)
            )
        }
        reset()
    }

    private func reset() {
        jamoBuffer.removeAll()
        hangulBuffer = ""
        candidates.removeAll()
        selectedIndex = -1
        hideCandidates()
    }
}
