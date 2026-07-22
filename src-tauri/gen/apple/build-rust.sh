#!/bin/sh

set -eu

# Xcode launched from Finder does not inherit the interactive shell's PATH.
# Add the standard Rust and Homebrew locations, then activate whichever Node
# version manager is installed before invoking Tauri.
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

if ! command -v npm >/dev/null 2>&1; then
  if command -v fnm >/dev/null 2>&1; then
    eval "$(fnm env --shell bash)"
  elif [ -s "$HOME/.nvm/nvm.sh" ]; then
    # shellcheck disable=SC1090
    . "$HOME/.nvm/nvm.sh"
    nvm use --silent >/dev/null
  elif [ -d "$HOME/.volta/bin" ]; then
    export PATH="$HOME/.volta/bin:$PATH"
  fi
fi

if ! command -v npm >/dev/null 2>&1; then
  echo "Koma's iOS build requires Node.js and npm, but Xcode could not find them." >&2
  echo "Install Node.js with Homebrew, fnm, nvm, or Volta, then build again." >&2
  exit 127
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "Koma's iOS build requires Rust, but Xcode could not find cargo." >&2
  exit 127
fi

REPOSITORY_ROOT="$(cd "$SRCROOT/../../.." && pwd)"
cd "$REPOSITORY_ROOT"

TAURI_LOG="$(mktemp "${TMPDIR:-/tmp}/koma-tauri-xcode.XXXXXX")"
trap 'rm -f "$TAURI_LOG"' EXIT

if npm run -- tauri ios xcode-script -v \
  --platform "${PLATFORM_DISPLAY_NAME:?}" \
  --sdk-root "${SDKROOT:?}" \
  --framework-search-paths "${FRAMEWORK_SEARCH_PATHS:?}" \
  --header-search-paths "${HEADER_SEARCH_PATHS:?}" \
  --gcc-preprocessor-definitions "${GCC_PREPROCESSOR_DEFINITIONS:-}" \
  --configuration "${CONFIGURATION:?}" \
  ${FORCE_COLOR:-} \
  ${ARCHS:?} >"$TAURI_LOG" 2>&1; then
  cat "$TAURI_LOG"
  exit 0
else
  TAURI_STATUS=$?
fi

cat "$TAURI_LOG"

# Tauri normally supplies build options over a local RPC connection. That
# connection is absent when someone opens the generated project and presses
# Build directly in Xcode. Tauri 2 has reported this both as a missing CLI
# options error and, in newer releases, as a missing server address file.
# Preserve genuine Tauri errors while supporting the standalone Xcode workflow.
if ! grep -Eq "failed to read CLI options|failed to read missing addr file|server-addr: No such file or directory" "$TAURI_LOG"; then
  exit "$TAURI_STATUS"
fi

echo "Tauri runner is not active; building Koma directly for Xcode."
npm run build

case "${PLATFORM_NAME:-}" in
  iphoneos)
    RUST_TARGET="aarch64-apple-ios"
    ;;
  iphonesimulator)
    case " ${ARCHS:?} " in
      *" x86_64 "*) RUST_TARGET="x86_64-apple-ios" ;;
      *) RUST_TARGET="aarch64-apple-ios-sim" ;;
    esac
    ;;
  *)
    echo "Unsupported Apple platform: ${PLATFORM_NAME:-unknown}" >&2
    exit 1
    ;;
esac

CARGO_PROFILE_ARGS=""
if [ "${CONFIGURATION:?}" = "release" ]; then
  CARGO_PROFILE_ARGS="--release"
fi

cargo build \
  --manifest-path "$REPOSITORY_ROOT/src-tauri/Cargo.toml" \
  --lib \
  --target "$RUST_TARGET" \
  --features custom-protocol \
  $CARGO_PROFILE_ARGS

RUST_PROFILE="${CONFIGURATION:?}"
LIBRARY_DIR="$SRCROOT/Externals/${ARCHS%% *}/$CONFIGURATION"
mkdir -p "$LIBRARY_DIR"
cp "$REPOSITORY_ROOT/target/$RUST_TARGET/$RUST_PROFILE/libkoma_lib.a" \
  "$LIBRARY_DIR/libapp.a"
