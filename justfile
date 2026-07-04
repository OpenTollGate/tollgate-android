# tollgate-android build recipes — modelled on fips-android's justfile.
#
# Prereqs: Rust stable + `cargo install cargo-ndk`, Android NDK (set
# ANDROID_NDK_HOME), JDK 17, Android SDK. The `aarch64-linux-android` rustc
# target must be installed (`rustup target add aarch64-linux-android`).

repo_root      := `dirname justfile`
ndk            := env_var_or_default("ANDROID_NDK_HOME", env_var_or_default("ANDROID_NDK_ROOT", ""))
target_dir     := env_var_or_default("CARGO_TARGET_DIR", repo_root / "target")
abi            := "arm64-v8a"
rust_target    := "aarch64-linux-android"
so_dir         := "android/app/src/main/jniLibs/" + abi
bindings_dir   := "android/app/src/main/kotlin"

# default: show recipes
default:
    @just --list

# Run the host test suite (no Android toolchain needed).
test:
    CARGO_TARGET_DIR={{target_dir}} cargo test -p tollgate-mobile

# Cross-compile libtollgate_mobile.so for arm64-v8a.
so:
    @test -n "{{ndk}}" || { echo "set ANDROID_NDK_HOME"; exit 1; }
    ANDROID_NDK_HOME={{ndk}} ANDROID_NDK_ROOT={{ndk}} \
    CARGO_TARGET_DIR={{target_dir}} \
    cargo ndk -t {{rust_target}} build -p tollgate-mobile --release
    mkdir -p {{so_dir}}
    cp {{target_dir}}/{{rust_target}}/release/libtollgate_mobile.so {{so_dir}}/

# Generate the UniFFI Kotlin bindings from the compiled .so (run after `so`).
bindings: so
    cargo run --bin uniffi-bindgen-cli -- generate \
        --library {{so_dir}}/libtollgate_mobile.so \
        --language kotlin \
        --out-dir {{bindings_dir}} \
        || uniffi-bindgen generate \
            --library {{so_dir}}/libtollgate_mobile.so \
            --language kotlin \
            --out-dir {{bindings_dir}}

# Assemble the debug APK (run after `bindings`).
apk: bindings
    cd android && ./gradlew assembleDebug

# Build .so + bindings + APK in one shot.
build: apk

# Install + launch on a connected device.
device: build
    cd android && ./gradlew installDebug
    adb shell am start -n org.opentollgate.android/.MainActivity

# Wipe generated artifacts.
clean:
    rm -rf {{so_dir}} {{target_dir}}/{{rust_target}}
    cd android && ./gradlew clean || true
