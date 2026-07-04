# tollgate-android

Native Android TollGate client — pay for internet access with Cashu over FIPS mesh.

Rust core ([`tollgate-mobile`](tollgate-mobile)) + Kotlin/Jetpack Compose shell.
Built on the [TollGate](https://github.com/OpenTollGate/tollgate-rs) v2 protocol
and [FIPS](https://github.com/k0sti/fips) mesh networking.

## Why Native?

Previous Android TollGate clients used Flutter or Tauri (web wrappers). This app
is fully native: Rust for the protocol/wallet layer, Kotlin/Compose for the UI.
FIPS mesh networking solves the Android gateway problem — no firewall rules or
OS fork needed.

## Status

**Phase 0: Scaffold** — Rust core ported (5/5 tests pass), Android shell pending.

See the [master plan](../../plans/tollgate-android-master-plan.md) for the full roadmap.

## Build

Requirements: Rust 1.95+, cargo-ndk, Android NDK 27.2, Android SDK 35.

```bash
# Test the Rust core
cargo test -p tollgate-mobile

# Cross-compile for Android (arm64)
cargo ndk -t arm64-v8a build -p tollgate-mobile --release

# Build the APK
cd android && ./gradlew assembleDebug
```

## Architecture

```
tollgate-mobile/    Rust crate — TollGate v2 protocol client (UniFFI facade)
android/            Kotlin/Jetpack Compose app shell
```

Modeled on [Myco](https://github.com/Origami74/myco) (native FIPS Android app).

## License

MIT
