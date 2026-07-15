#!/usr/bin/env bash
#
# TollGate Android — Physical Router Test Automation
#
# Runs the full test chain:
#   1. Host-level Rust wallet E2E tests (against FakeWallet mint)
#   2. ADB device check + APK install
#   3. Maestro UI flow tests (launch, nav, discover, wallet, status, settings)
#   4. Physical router probe (gateway detect + pay on real TollGate router)
#
# Usage: ./maestro/run-tests.sh [--device <serial>] [--apk <path>]
#
# Prerequisites:
#   - FakeWallet mint running (default: http://192.168.2.33:4444)
#   - ADB device connected (physical phone or emulator)
#   - Maestro CLI installed (https://maestro.mobile.dev)
#   - Physical TollGate router in range (for Phase 4 only)
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
ADB="${ANDROID_HOME:-$HOME/Android/Sdk}/platform-tools/adb"
MAESTRO="${MAESTRO_BIN:-/tmp/maestro-cli/maestro/bin/maestro}"
APK="${APK:-$REPO_DIR/android/app/build/outputs/apk/debug/app-debug.apk}"
FAKEWALLET_URL="${FAKEWALLET_URL:-http://192.168.2.33:4444}"
DEVICE_SERIAL=""
PHASE="${PHASE:-all}"  # all|host|ui|router

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m'

log()  { echo -e "${GREEN}[TOLLGATE]${NC} $*"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }
fail() { echo -e "${RED}[FAIL]${NC} $*"; exit 1; }

# --- Parse args ---
while [[ $# -gt 0 ]]; do
    case $1 in
        --device) DEVICE_SERIAL="$2"; shift 2 ;;
        --apk)    APK="$2"; shift 2 ;;
        --phase)  PHASE="$2"; shift 2 ;;
        *) fail "Unknown arg: $1" ;;
    esac
done

# --- Phase 1: Host-level wallet E2E tests ---
run_host_tests() {
    log "Phase 1: Host-level wallet E2E tests against FakeWallet ($FAKEWALLET_URL)"

    # Check FakeWallet is up
    if ! curl -s --connect-timeout 3 "$FAKEWALLET_URL/v1/keys" | grep -q "keysets"; then
        fail "FakeWallet mint not reachable at $FAKEWALLET_URL — start it first"
    fi
    log "FakeWallet is UP"

    cd "$REPO_DIR"
    export CARGO_TARGET_DIR="$REPO_DIR/../tollgate-rs/target"
    export TOLLGATE_MINT_URL="$FAKEWALLET_URL"

    # Run standard tests (unit + gateway protocol)
    log "Running unit + gateway tests..."
    if cargo test -p tollgate-mobile 2>&1 | tee /tmp/tollgate-host-tests.txt; then
        log "Unit + gateway tests: PASS"
    else
        fail "Unit + gateway tests: FAIL"
    fi

    # Run wallet E2E tests (requires live mint)
    log "Running wallet E2E tests (requires live FakeWallet)..."
    if cargo test -p tollgate-mobile --test wallet_e2e -- --ignored --nocapture 2>&1 | tee /tmp/tollgate-wallet-e2e.txt; then
        log "Wallet E2E tests: PASS"
    else
        fail "Wallet E2E tests: FAIL"
    fi
}

# --- Phase 2: ADB device + APK install ---
run_ui_tests() {
    log "Phase 2: ADB device + Maestro UI tests"

    # Check ADB
    if ! command -v "$ADB" &>/dev/null; then
        fail "ADB not found at $ADB — install Android platform-tools"
    fi

    # Check device
    if [ -n "$DEVICE_SERIAL" ]; then
        log "Using device: $DEVICE_SERIAL"
        export ANDROID_SERIAL="$DEVICE_SERIAL"
    fi

    local device_count
    device_count=$("$ADB" devices | grep -c "device$") || 0
    if [ "$device_count" -eq 0 ]; then
        warn "No ADB device connected — skipping UI tests"
        warn "Connect a phone or start an emulator, then re-run with --phase ui"
        return 0
    fi
    log "ADB device connected"

    # Install APK
    if [ ! -f "$APK" ]; then
        fail "APK not found at $APK — build first with: cd android && ./gradlew assembleDebug"
    fi
    log "Installing APK: $APK"
    "$ADB" install -r "$APK" || fail "APK install failed"
    log "APK installed"

    # Check Maestro
    if ! command -v "$MAESTRO" &>/dev/null; then
        fail "Maestro not found at $MAESTRO — install from https://mobile.dev"
    fi

    # Run Maestro flows
    local flows=(
        "$SCRIPT_DIR/launch.yaml"
        "$SCRIPT_DIR/wallet_view.yaml"
        "$SCRIPT_DIR/pay_screen.yaml"
        "$SCRIPT_DIR/status_screen.yaml"
        "$SCRIPT_DIR/settings_screen.yaml"
        "$SCRIPT_DIR/discover_scan.yaml"
    )

    local pass=0 fail_count=0
    for flow in "${flows[@]}"; do
        local name
        name=$(basename "$flow" .yaml)
        log "Running Maestro flow: $name"
        if "$MAESTRO" test "$flow" 2>&1 | tee "/tmp/maestro-$name.log"; then
            log "  $name: PASS"
            ((pass++))
        else
            warn "  $name: FAIL"
            ((fail_count++))
        fi
    done

    log "Maestro results: $pass passed, $fail_count failed"
    [ "$fail_count" -eq 0 ] && log "All UI tests: PASS" || warn "Some UI tests failed"
}

# --- Phase 3: Physical router probe ---
run_router_tests() {
    log "Phase 3: Physical router test automation"

    # Check device
    local device_count
    device_count=$("$ADB" devices | grep -c "device$") || 0
    if [ "$device_count" -eq 0 ]; then
        warn "No ADB device — skipping router tests"
        return 0
    fi

    # Check if phone is connected to a TollGate WiFi network
    log "Checking WiFi connection..."
    local wifi_ssid
    wifi_ssid=$("$ADB" shell dumpsys wifi | grep "current SSID" | head -1 | sed 's/.*: //' || echo "")
    if echo "$wifi_ssid" | grep -qi "TollGate"; then
        log "Connected to TollGate WiFi: $wifi_ssid"
    else
        warn "Not connected to TollGate WiFi (current: $wifi_ssid)"
        warn "Connect to a TollGate router WiFi network first"
        return 0
    fi

    # Probe the gateway
    log "Probing gateway..."
    local gateway_ip
    gateway_ip=$("$ADB" shell ip route | grep default | awk '{print $3}' | head -1)
    log "Gateway IP: $gateway_ip"

    if [ -z "$gateway_ip" ]; then
        warn "No gateway IP found — phone may not have network"
        return 0
    fi

    # Check if gateway is a TollGate (responds on port 8080)
    log "Probing gateway HTTP endpoint..."
    if "$ADB" shell "curl -s --connect-timeout 3 http://$gateway_ip:8080/" 2>/dev/null | grep -qi "tollgate"; then
        log "TollGate gateway detected at $gateway_ip:8080"
    else
        warn "Gateway at $gateway_ip:8080 does not respond as TollGate"
        warn "Ensure the router has TollGate firmware running"
        return 0
    fi

    # Run the discover+connect+pay Maestro flow
    log "Running router integration Maestro flow..."
    if [ -f "$SCRIPT_DIR/router_pay.yaml" ]; then
        "$MAESTRO" test "$SCRIPT_DIR/router_pay.yaml" 2>&1 | tee /tmp/maestro-router-pay.log \
            && log "Router pay flow: PASS" \
            || warn "Router pay flow: FAIL"
    else
        warn "router_pay.yaml not found — skipping automated pay flow"
        warn "Manually test: Discover → Connect → Pay → Verify internet access"
    fi

    # Verify internet access after payment
    log "Verifying internet access..."
    if "$ADB" shell "ping -c 3 1.1.1.1" 2>/dev/null | grep -q "3 received"; then
        log "Internet access: PASS"
    else
        warn "Internet access: FAIL (payment may not have been processed)"
    fi
}

# --- Main ---
log "TollGate Android Test Automation"
log "Repo: $REPO_DIR"
log "APK:  $APK"
log "Phase: $PHASE"
echo ""

case "$PHASE" in
    all)
        run_host_tests
        echo ""
        run_ui_tests
        echo ""
        run_router_tests
        ;;
    host)   run_host_tests ;;
    ui)     run_ui_tests ;;
    router) run_router_tests ;;
    *) fail "Unknown phase: $PHASE (use: all|host|ui|router)" ;;
esac

echo ""
log "Test automation complete"