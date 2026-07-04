# AGENTS.md — tollgate-android

## What This Is

Native Android TollGate app. Rust core (`tollgate-mobile`) + Kotlin/Jetpack
Compose shell. FIPS-based networking (no traditional IP firewall). No Tauri,
no Flutter, no JavaScript web wrapper.

## Build Commands

```bash
# Host tests (uses tollgate-rs warm cache)
export CARGO_TARGET_DIR=/home/c03rad0r/repos/tollgate-rs/target
cd ~/repos/tollgate-android
cargo test -p tollgate-mobile

# Cross-compile Rust for Android
export ANDROID_NDK_HOME=~/Android/Sdk/ndk/27.2.12479018
cargo ndk -t arm64-v8a build -p tollgate-mobile --release

# Build APK
cd android && ./gradlew assembleDebug

# Install on device
cd android && ./gradlew installDebug
```

## Architecture

Rust core → UniFFI → Kotlin/Compose. Modeled on Myco (Origami74/myco) and
fips-android (Michael Malmi). See `~/plans/tollgate-android-master-plan.md`
for the full architecture document.

### Key Dependencies (path deps)
- `tollgate-protocol` from `~/repos/tollgate-rs/crates/tollgate-protocol`
- `tollgate-core` from `~/repos/tollgate-rs/crates/tollgate-core`

### UniFFI Boundary
`TollgateMobileNode` exposes: `new`, `pubkey_hex`, `detect`, `pay`,
`start_consume`, `poll_event`, `stop_consume`.

## Conventions

- All code as PRs. Never push directly to main.
- Plan-first: present plan → get approval → implement.
- Tauri is explicitly rejected. Flutter is explicitly rejected. Native only.
- Use rustls (not native-tls) to avoid OpenSSL/NDK issues.
- Match dependency versions to tollgate-rs for cache warmth.

## Kanban

Board: `tollgate-android`
Plan: `~/plans/tollgate-android-master-plan.md`
