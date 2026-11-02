# Source this on macOS when Xcode is installed but its license has not been
# accepted (xcrun refuses to run, so the default cc/ar shims fail).
# It points cargo, cc-rs and the linker at the Command Line Tools directly.
#
#   source scripts/macos-clt-env.sh
#
CLT_BIN=/Library/Developer/CommandLineTools/usr/bin
if [ ! -x "$CLT_BIN/clang" ]; then
  echo "Command Line Tools not found. Run: xcode-select --install" >&2
  return 1 2>/dev/null || exit 1
fi
export SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk
export CC="$CLT_BIN/clang"
export CXX="$CLT_BIN/clang++"
export AR="$CLT_BIN/ar"
export RANLIB="$CLT_BIN/ranlib"
export CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER="$CLT_BIN/clang"
export CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER="$CLT_BIN/clang"
