//! Windows-specific implementation using low-level keyboard hook.
//!
//! Architecture:
//! 1. Install a WH_KEYBOARD_LL hook to intercept all keystrokes
//! 2. When active: QWERTY → Jamo → Hangul syllable composition
//! 3. On Space/Enter: Hangul → Hiragana via Rust engine → SendInput
//! 4. System tray icon for toggle and status
//!
//! Toggle: Ctrl+Space to switch between active/inactive.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;

use crate::hangul::{qwerty_to_jamo, compose_jamo_buffer};
use crate::engine::ConversionEngine;

// ── Global State ────────────────────────────────────────────

static ACTIVE: AtomicBool = AtomicBool::new(false);
static JAMO_BUFFER: Mutex<Vec<char>> = Mutex::new(Vec::new());
static COMPOSING: AtomicBool = AtomicBool::new(false);

// Engine must be initialized on the main thread
static mut ENGINE: Option<ConversionEngine> = None;

// ── Entry Point ─────────────────────────────────────────────

pub fn run() {
    println!("╔═══════════════════════════════════════════════╗");
    println!("║  한글일본어입력기 (Windows)                     ║");
    println!("║  Ctrl+Space: 입력기 켜기/끄기                   ║");
    println!("║  한글 두벌식 자판 → 히라가나 변환               ║");
    println!("╚═══════════════════════════════════════════════╝");
    println!();

    // Initialize engine
    let app_data = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
    let db_dir = format!("{}/HangulJapaneseIME", app_data);
    std::fs::create_dir_all(&db_dir).ok();
    let db_path = format!("{}/hj.db", db_dir);

    match ConversionEngine::new(Some(&db_path)) {
        Ok(eng) => {
            println!("[엔진] 초기화 완료 (PhoneticMap: {} 항목)", eng.phonetic_map_size());
            unsafe { ENGINE = Some(eng); }
        }
        Err(e) => {
            eprintln!("[엔진] 초기화 실패: {}", e);
            return;
        }
    }

    // Install keyboard hook
    unsafe {
        let hook = SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(keyboard_proc),
            GetModuleHandleW(None).unwrap_or_default(),
            0,
        );

        match hook {
            Ok(h) => {
                println!("[후크] 키보드 후크 설치 완료");
                println!("[상태] 비활성 (Ctrl+Space로 켜기)");
                println!();

                // Message loop (required for hook to work)
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, HWND::default(), 0, 0).into() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }

                let _ = UnhookWindowsHookEx(h);
            }
            Err(e) => {
                eprintln!("[후크] 설치 실패: {:?}", e);
            }
        }
    }
}

// ── Keyboard Hook Callback ──────────────────────────────────

unsafe extern "system" fn keyboard_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code < 0 {
        return CallNextHookEx(None, code, wparam, lparam);
    }

    // Only handle key-down events
    if wparam.0 as u32 != WM_KEYDOWN && wparam.0 as u32 != WM_SYSKEYDOWN {
        return CallNextHookEx(None, code, wparam, lparam);
    }

    let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
    let vk = VIRTUAL_KEY(kb.vkCode as u16);

    // ── Toggle: Ctrl+Space ──────────────────────────────
    if vk == VK_SPACE {
        let ctrl_down = (GetAsyncKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0;
        if ctrl_down {
            let was_active = ACTIVE.fetch_xor(true, Ordering::Relaxed);
            let now_active = !was_active;
            if now_active {
                println!("[상태] ✅ 활성화 — 한글 입력 시작");
            } else {
                // Commit any pending composition
                commit_composition();
                println!("[상태] ⏸  비활성화");
            }
            return LRESULT(1); // Consume the key
        }
    }

    // If not active, pass through
    if !ACTIVE.load(Ordering::Relaxed) {
        return CallNextHookEx(None, code, wparam, lparam);
    }

    // ── Handle special keys ─────────────────────────────

    // Backspace
    if vk == VK_BACK {
        let mut buf = JAMO_BUFFER.lock().unwrap();
        if !buf.is_empty() {
            buf.pop();
            if buf.is_empty() {
                COMPOSING.store(false, Ordering::Relaxed);
                // Clear composition display
                clear_composition_display();
            } else {
                update_composition_display(&buf);
            }
            return LRESULT(1); // Consume
        }
        return CallNextHookEx(None, code, wparam, lparam);
    }

    // Enter — commit composition
    if vk == VK_RETURN {
        if COMPOSING.load(Ordering::Relaxed) {
            commit_composition();
            return LRESULT(1);
        }
        return CallNextHookEx(None, code, wparam, lparam);
    }

    // Space — commit composition
    if vk == VK_SPACE {
        if COMPOSING.load(Ordering::Relaxed) {
            commit_composition();
            return LRESULT(1);
        }
        return CallNextHookEx(None, code, wparam, lparam);
    }

    // Escape — cancel composition
    if vk == VK_ESCAPE {
        if COMPOSING.load(Ordering::Relaxed) {
            let mut buf = JAMO_BUFFER.lock().unwrap();
            buf.clear();
            COMPOSING.store(false, Ordering::Relaxed);
            clear_composition_display();
            return LRESULT(1);
        }
        return CallNextHookEx(None, code, wparam, lparam);
    }

    // ── Character input ─────────────────────────────────

    // Convert virtual key to character
    let shift_down = (GetAsyncKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0;
    let ch = vk_to_char(vk, shift_down);

    if let Some(ascii_ch) = ch {
        if let Some(jamo) = qwerty_to_jamo(ascii_ch) {
            // Add jamo to buffer
            let mut buf = JAMO_BUFFER.lock().unwrap();
            buf.push(jamo);
            COMPOSING.store(true, Ordering::Relaxed);
            update_composition_display(&buf);
            return LRESULT(1); // Consume the key
        } else {
            // Non-hangul character → commit pending, pass through
            if COMPOSING.load(Ordering::Relaxed) {
                drop(JAMO_BUFFER.lock()); // Release lock before commit
                commit_composition();
            }
            return CallNextHookEx(None, code, wparam, lparam);
        }
    }

    CallNextHookEx(None, code, wparam, lparam)
}

// ── Helper Functions ────────────────────────────────────────

/// Convert virtual key code to ASCII character.
fn vk_to_char(vk: VIRTUAL_KEY, shift: bool) -> Option<char> {
    let code = vk.0;
    match code {
        0x41..=0x5A => {
            // A-Z
            let base = (code - 0x41) as u8 + b'a';
            if shift {
                Some((base - 32) as char) // Uppercase
            } else {
                Some(base as char)
            }
        }
        _ => None,
    }
}

/// Commit the current composition: convert hangul → hiragana and send to active window.
fn commit_composition() {
    let mut buf = JAMO_BUFFER.lock().unwrap();
    if buf.is_empty() {
        COMPOSING.store(false, Ordering::Relaxed);
        return;
    }

    let hangul = compose_jamo_buffer(&buf);
    buf.clear();
    COMPOSING.store(false, Ordering::Relaxed);

    // Convert to hiragana via engine
    let hiragana = unsafe {
        if let Some(ref eng) = ENGINE {
            eng.to_hiragana(&hangul).unwrap_or(hangul.clone())
        } else {
            hangul.clone()
        }
    };

    println!("  {} → {}", hangul, hiragana);

    // Send the hiragana string to the active window
    send_string(&hiragana);
}

/// Send a Unicode string to the active window using SendInput.
fn send_string(s: &str) {
    let chars: Vec<u16> = s.encode_utf16().collect();
    let mut inputs: Vec<INPUT> = Vec::with_capacity(chars.len() * 2);

    for &ch in &chars {
        // Key down
        inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: ch,
                    dwFlags: KEYEVENTF_UNICODE,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        });
        // Key up
        inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: ch,
                    dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        });
    }

    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

/// Show current composition in console (real IME would use IME candidate window).
fn update_composition_display(jamos: &[char]) {
    let hangul = compose_jamo_buffer(jamos);
    let hiragana = unsafe {
        if let Some(ref eng) = ENGINE {
            eng.to_hiragana(&hangul).unwrap_or_default()
        } else {
            String::new()
        }
    };
    // Clear line and show composition
    print!("\r  [입력 중] {} → {}    ", hangul, hiragana);
    use std::io::Write;
    std::io::stdout().flush().ok();
}

/// Clear the composition display line.
fn clear_composition_display() {
    print!("\r                                        \r");
    use std::io::Write;
    std::io::stdout().flush().ok();
}
