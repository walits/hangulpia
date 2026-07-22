//! Android JNI bridge.
//!
//! Exposes the core IME engine to Kotlin/Java via JNI.

#![allow(non_snake_case)]

/// Placeholder for JNI entry points.
/// Each function will be called from the Android InputMethodService.

#[cfg(target_os = "android")]
use jni::JNIEnv;

/// Initialize the native IME engine. Called once from Android.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_com_hkd_ime_NativeEngine_init(_env: JNIEnv) {
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Debug),
    );
    log::info!("IME native engine initialized");
}

// Non-Android stub for compilation
#[cfg(not(target_os = "android"))]
pub fn init() {
    log::info!("IME native engine initialized (non-Android stub)");
}
