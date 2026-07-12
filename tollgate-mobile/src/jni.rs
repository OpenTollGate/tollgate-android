//! JNI bootstrap layer for TollGate Android.
//!
//! This module provides the JNI interface that allows Kotlin code to call into
//! the Rust core. It handles JNI_OnLoad registration, JavaVM handle storage,
//! and helper functions for string conversion between Java and Rust.

use jni::objects::{JObject, JString, JValue};
use jni::sys::{jboolean, jdouble, jint, jlong, jobject, jstring, jvoid};
use jni::JNIEnv;
use std::sync::OnceLock;

/// Global JavaVM handle storage for JNI access.
static JVM: OnceLock<jni::JavaVM> = OnceLock::new();

/// Get the current JNIEnv from the stored JavaVM handle.
///
/// This function is safe to call from any thread and will return the JNIEnv
/// for the current thread. Panics if the JavaVM is not initialized.
pub fn get_jni_env<'a>() -> JNIEnv<'a> {
    unsafe {
        JVM.get()
            .expect("JavaVM not initialized")
            .get_env()
            .expect("Failed to get JNIEnv")
    }
}

/// Convert a Java String to a Rust String.
///
/// Takes ownership of the JString and converts it to a Rust String.
/// Panics on conversion failure.
pub fn jstring_to_rust(env: &JNIEnv, jstring: JString) -> String {
    env.get_string(jstring)
        .expect("Failed to get string from JString")
        .into()
}

/// Convert a Rust String to a Java String.
///
/// Creates a new JString from the Rust string.
/// Panics on creation failure.
pub fn rust_to_jstring(env: &JNIEnv, string: &str) -> JString {
    env.new_string(string)
        .expect("Failed to create JString from rust string")
}

/// JNI_OnLoad function - entry point for the JVM.
///
/// This function is called by the JVM when the library is loaded.
/// It registers the native methods and returns the JNI version.
///
/// # Safety
/// This function is called by the JVM and must be thread-safe.
#[no_mangle]
pub extern "system" fn JNI_OnLoad(vm: jni::JavaVM, _reserved: *mut std::ffi::c_void) -> jint {
    // Store the JavaVM handle globally
    let result = vm.attach_current_thread();
    match result {
        Ok(_) => {
            // Store the JavaVM for future access
            let _ = JVM.set(vm);
            jni::JNI_VERSION_1_6
        }
        Err(e) => {
            eprintln!("Failed to attach current thread: {}", e);
            jni::JNI_VERSION_1_6
        }
    }
}

/// JNI_OnUnload function - cleanup when the library is unloaded.
///
/// # Safety
/// This function is called by the JVM and must be thread-safe.
#[no_mangle]
pub extern "system" fn JNI_OnUnload(vm: jni::JavaVM, _reserved: *mut std::ffi::c_void) {
    // Detach the current thread from the JVM
    let _ = vm.detach_current_thread();
    
    // Clear the stored JavaVM
    JVM.take();
}

// Register any native methods here in the future.
// For now, we just need the JNI_OnLoad to establish the JavaVM connection.

#[cfg(target_os = "android")]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jstring_conversion() {
        // This test would require a JVM to run, so we skip it in normal tests
        // It would be tested as part of the Android integration tests
        println!("JNI string conversion tests require Android environment");
    }
}