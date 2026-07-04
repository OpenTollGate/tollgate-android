# Keep the UniFFI generated bindings + the Rust FFI symbols.
-keep class uniffi.** { *; }
-keepclassmembers class * {
    native <methods>;
}
