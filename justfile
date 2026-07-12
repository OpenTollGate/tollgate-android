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

# Generate the UniFFI Kotlin bindings from the compiled .so (run after `so`),
# then post-patch them. UniFFI 0.28.3 emits an ambiguous double-`message`
# property for error variants named `message` (clashes with Throwable.message
# under Kotlin 2.x); patch-uniffi-bindings.py collapses it to a single
# `override val message`. Idempotent.
bindings: so
    cargo run -p uniffi-bindgen -- {{so_dir}}/libtollgate_mobile.so {{bindings_dir}}
    python3 {{repo_root}}/scripts/patch-uniffi-bindings.py {{bindings_dir}}/uniffi/tollgate_mobile/tollgate_mobile.kt

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
