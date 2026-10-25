#!/usr/bin/env bash
# One-shot setup + self-test for macOS. Writes diagnostic.txt to paste back to the developer.
# Usage: ./scripts/mac-setup.sh          (installs missing tools, runs tests)
#        ./scripts/mac-setup.sh --run    (same, then launches the app)
set -u
cd "$(dirname "$0")/.."
REPORT="diagnostic.txt"
: > "$REPORT"
log() { echo "$@" | tee -a "$REPORT"; }
step() { log ""; log "=== $* ==="; }
run() { log "\$ $*"; "$@" 2>&1 | tee -a "$REPORT"; return "${PIPESTATUS[0]}"; }

step "System"
log "$(uname -a)"
log "macOS: $(sw_vers -productVersion 2>/dev/null || echo 'not macOS (skipping installs)')"

if [[ "$(uname)" == "Darwin" && -z "${SKIP_INSTALL:-}" ]]; then
  step "Xcode command line tools"
  if ! xcode-select -p >/dev/null 2>&1; then
    log "Not installed: a system dialog will open. Finish it, then re-run this script."
    xcode-select --install
    exit 1
  fi
  log "ok"

  step "Rust"
  if ! command -v cargo >/dev/null 2>&1; then
    [[ -f "$HOME/.cargo/env" ]] || curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    # shellcheck disable=SC1091
    source "$HOME/.cargo/env"
  fi
  run cargo --version

  step "Tauri CLI"
  if ! cargo tauri --version >/dev/null 2>&1; then
    log "Installing tauri-cli (several minutes)..."
    run cargo install tauri-cli --version "^2" --locked
  fi
  run cargo tauri --version
fi

step "Tests"
run cargo test 2>&1 | grep -E "test result|FAILED|panicked|error" | tee -a "$REPORT"

step "Audio and MIDI devices (plug the Oxygen 49 in first)"
run cargo run -q -p beatmaker-io --bin bmk -- devices

step "Demo render (96 kHz / 24-bit)"
run cargo run -q -p beatmaker-io --bin bmk -- demo demo.wav 96000 24
[[ "$(uname)" == "Darwin" ]] && log "Listen to demo.wav: open demo.wav"

step "Done"
log "Report saved in $REPORT : paste its content in the chat."
if [[ "${1:-}" == "--run" ]]; then
  cargo tauri dev
fi
