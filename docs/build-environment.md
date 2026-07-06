# Build Environment

> The T470 (7GB RAM) cannot build the Android APK — cargo-ndk + Gradle
> exhausts memory and gets OOM-killed. Use DQ05 (11GB RAM) instead.

## The Problem

T470 (hostname: CobridorWave) has 7GB RAM. The Android build pipeline
requires:
1. `cargo ndk` cross-compiling Rust → `.so` (heavy: tokio, reqwest, secp256k1)
2. `./gradlew assembleDebug` compiling Kotlin + packaging APK

Both running concurrently or even sequentially exceeds 7GB. Result:
OOM-killer terminates the process with SIGTERM (exit code 137).

Kanban task `t_3b390929` (Phase 0: Android Gradle scaffold) crashed **13
times** on T470 — every single run was killed. The build has never
succeeded on this machine.

## The Fix: Build on DQ05

DQ05 (hostname: c03rad0r-DQ05proplus, IP: 192.168.1.218) has 11GB RAM.
This is sufficient for the full cargo-ndk + Gradle build.

### DQ05 Setup (One-Time)

```bash
ssh c03rad0r@192.168.1.218

# 1. Rust Android targets
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android

# 2. cargo-ndk
cargo install cargo-ndk

# 3. JDK for Gradle
sudo apt install -y default-jdk

# 4. Android NDK (via command-line tools)
# Download cmdline-tools from developer.android.com
# Then: sdkmanager "ndk;27.2.12479018"
# Set: export ANDROID_NDK_HOME=~/Android/Sdk/ndk/27.2.12479018

# 5. Clone repos (path deps are absolute)
git clone https://github.com/OpenTollGate/tollgate-android ~/repos/tollgate-android
git clone https://github.com/OpenTollGate/tollgate-rs ~/repos/tollgate-rs
```

### Build on DQ05

```bash
ssh c03rad0r@192.168.1.218
cd ~/repos/tollgate-android
export CARGO_TARGET_DIR=~/repos/tollgate-rs/target
export ANDROID_NDK_HOME=~/Android/Sdk/ndk/27.2.12479018

# Rust → .so
cargo ndk -t arm64-v8a build -p tollgate-mobile --release

# APK
cd android && ./gradlew assembleDebug
```

### Install on Device (from T470 with adb)

Build on DQ05, then transfer APK to T470 (which has the phone connected
via adb):

```bash
# On DQ05: copy APK to T470
scp android/app/build/outputs/apk/debug/app-debug.apk \
    c03rad0r@cobridorwave:~/repos/tollgate-android/android/app/build/outputs/apk/debug/

# On T470: install
cd ~/repos/tollgate-android/android
./gradlew installDebug
# OR: adb install app/build/outputs/apk/debug/app-debug.apk
```

## Machine Reference

| Machine | Hostname | RAM | KVM | Role |
|---------|----------|-----|-----|------|
| T470 | CobridorWave | 7GB | No | Hermes runtime, signal-cli, adb (phone connected) |
| DQ05 | c03rad0r-DQ05proplus | 11GB | Yes | Build server, cargo-ndk, Gradle |
