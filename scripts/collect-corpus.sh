#!/usr/bin/env bash
# Regenerates the real-world symbol corpora in tests/corpus/ used by the
# `batch` criterion benchmark (benches/batch.rs) and shared with the
# robustness/fuzzing plan.
#
# Each file holds one raw (still-mangled) symbol per line, filtered to the
# mangling scheme named by the file, deduplicated, and capped. Sources are
# machine-specific, so the defaults below can be overridden via environment
# variables; the toolchains used for the current files:
#
#   rust_symbols.txt  — nm dumps of release Rust binaries
#                       (rust-analyzer, wasm-pack from ~/.cargo/bin)
#   swift_symbols.txt — nm dump of a bundled macOS Swift dylib
#                       (libswiftCore.dylib shipped inside an .app bundle;
#                       system Swift dylibs live in the dyld shared cache
#                       and carry no symbol table)
#   cpp_symbols.txt   — nm dump of the Xcode C++ compiler (Itanium ABI)
#
# Usage: scripts/collect-corpus.sh
set -euo pipefail

MAX_PER_FILE="${MAX_PER_FILE:-5000}"
CORPUS_DIR="$(cd "$(dirname "$0")/.." && pwd)/tests/corpus"
mkdir -p "$CORPUS_DIR"

RUST_BINARIES=(${RUST_BINARIES:-$HOME/.cargo/bin/rust-analyzer $HOME/.cargo/bin/wasm-pack})
SWIFT_DYLIB="${SWIFT_DYLIB:-$(ls /Applications/*/Contents/Frameworks/libswiftCore.dylib 2>/dev/null | head -1 || true)}"
CPP_BINARY="${CPP_BINARY:-/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/clang++}"

# Legacy (`_ZN...E`) and v0 (`_RN...`) Rust symbols, with macOS `_`/`__`
# platform prefixes.
rust_symbols() {
    for bin in "${RUST_BINARIES[@]}"; do
        nm "$bin" 2>/dev/null | awk '{print $NF}'
    done | grep -E '^_{1,2}(ZN|RN)' | sort -u | awk -v max="$MAX_PER_FILE" 'NR <= max'
}

# Current (`$s`/`$S`) and pre-Swift-5 (`_T0`/`_Tt`) Swift mangling.
swift_symbols() {
    nm "$SWIFT_DYLIB" 2>/dev/null | awk '{print $NF}' |
        grep -E '^(_\$[sS]|\$[sS]|_T0|_Tt)' | sort -u | awk -v max="$MAX_PER_FILE" 'NR <= max'
}

# Itanium-ABI C++ symbols.
cpp_symbols() {
    nm "$CPP_BINARY" 2>/dev/null | awk '{print $NF}' |
        grep -E '^_{1,2}ZN' | sort -u | awk -v max="$MAX_PER_FILE" 'NR <= max'
}

echo "rust_symbols.txt  <- ${RUST_BINARIES[*]}"
rust_symbols >"$CORPUS_DIR/rust_symbols.txt"
if [ -n "$SWIFT_DYLIB" ]; then
    echo "swift_symbols.txt <- $SWIFT_DYLIB"
    swift_symbols >"$CORPUS_DIR/swift_symbols.txt"
else
    echo "no Swift dylib found; skipping swift_symbols.txt"
fi
if [ -f "$CPP_BINARY" ]; then
    echo "cpp_symbols.txt   <- $CPP_BINARY"
    cpp_symbols >"$CORPUS_DIR/cpp_symbols.txt"
else
    echo "no C++ binary found; skipping cpp_symbols.txt"
fi

wc -l "$CORPUS_DIR"/*.txt
