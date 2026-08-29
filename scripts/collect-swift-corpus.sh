#!/usr/bin/env bash
# Collects a per-toolchain Swift mangling corpus snapshot.
#
# Compiles scripts/swift-corpus-fixture.swift with the selected Swift
# toolchain, collects the mangled symbols from the object file with `nm`, and
# stores them as tests/corpus/swift/<version>/symbols.txt together with a
# provenance note. The snapshot tests (tests/test_swift_corpus.rs) then pin
# exactly how this project's demangler renders that toolchain's output, which
# is what makes the README's "up to Swift X" claim testable.
#
# Usage:
#   scripts/collect-swift-corpus.sh [version] [swiftc]
#
#   version  corpus directory name     (default: parsed from `swiftc --version`, e.g. 6.3)
#   swiftc   compiler to use           (default: $SWIFTC, `xcrun --find swiftc` on macOS, else `swiftc`)
#
# Re-run for every toolchain you want represented (swiftly- or Xcode-installed
# toolchains: pass their swiftc path). Older mangling schemes do not need a
# compiler: they come from system/app dylibs via scripts/collect-corpus.sh.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CORPUS_ROOT="$REPO_ROOT/tests/corpus/swift"
FIXTURE="$REPO_ROOT/scripts/swift-corpus-fixture.swift"
MAX_SYMBOLS="${MAX_SYMBOLS:-2000}"

if [ -n "${2:-}" ]; then
    SWIFTC="$2"
elif [ -n "${SWIFTC:-}" ]; then
    : # honor the caller's $SWIFTC
elif command -v xcrun >/dev/null 2>&1; then
    SWIFTC="$(xcrun --find swiftc)"
else
    SWIFTC="swiftc"
fi

command -v "$SWIFTC" >/dev/null 2>&1 || { echo "swiftc not found: $SWIFTC" >&2; exit 1; }

# A toolchain invoked by path (not via `xcrun swiftc`) needs an explicit SDK
# to load the standard library for its target.
if [ -z "${SDKROOT:-}" ] && command -v xcrun >/dev/null 2>&1; then
    SDKROOT="$(xcrun --sdk macosx --show-sdk-path)"
    export SDKROOT
fi
[ -f "$FIXTURE" ] || { echo "fixture missing: $FIXTURE" >&2; exit 1; }

TOOLCHAIN_DESC="$("$SWIFTC" --version | head -1)"

if [ -n "${1:-}" ]; then
    VERSION="$1"
else
    # "Apple Swift version 6.3 (swift-6.3-RELEASE)" -> "6.3"
    VERSION="$(printf '%s\n' "$TOOLCHAIN_DESC" \
        | sed -n 's/.*[Vv]ersion \([0-9][0-9.]*\).*/\1/p')"
    [ -n "$VERSION" ] || { echo "could not parse a version from: $TOOLCHAIN_DESC" >&2; exit 1; }
fi

OUT_DIR="$CORPUS_ROOT/$VERSION"
mkdir -p "$OUT_DIR"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# Internal (non-public) symbols are exactly what the demangler sees in real
# binaries' symtabs, so no visibility gymnastics are needed; -parse-as-library
# keeps top-level code out of the way of actors/global-actor isolation.
"$SWIFTC" -c -parse-as-library -o "$TMP/fixture.o" "$FIXTURE"

# Collect into the temp dir and only publish a corpus that passed the checks
# below: writing straight to $OUT_DIR would truncate the committed snapshot
# before the pipeline runs, so any failure here would leave an empty corpus
# staged in git.
nm "$TMP/fixture.o" | awk '{print $NF}' \
    | { grep -E '^(_\$[sS]|\$[sS]|_T0|_Tt)' || true; } \
    | sort -u > "$TMP/all-symbols.txt"

TOTAL="$(grep -c '' < "$TMP/all-symbols.txt")"
[ "$TOTAL" -gt 0 ] || { echo "no mangled symbols collected; refusing to write an empty corpus" >&2; exit 1; }
if [ "$TOTAL" -gt "$MAX_SYMBOLS" ]; then
    echo "note: $TOTAL symbols collected, keeping the first $MAX_SYMBOLS (raise MAX_SYMBOLS to keep more)" >&2
fi
awk -v max="$MAX_SYMBOLS" 'NR <= max' < "$TMP/all-symbols.txt" > "$TMP/symbols.txt"
COUNT="$(grep -c '' < "$TMP/symbols.txt")"

{
    echo "version: $VERSION"
    echo "collected: $(date +%Y-%m-%d)"
    echo "toolchain: $TOOLCHAIN_DESC"
    # The symbol set depends on the compilation target, so record it: the same
    # "6.3.3" corpus collected on macOS/arm64 and Linux/x86_64 is not the same
    # set, and a mismatch would otherwise read as a snapshot regression.
    echo "target: $("$SWIFTC" -print-target-info 2>/dev/null \
        | sed -n 's/.*"unversionedTriple"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -1)"
    echo "source: scripts/swift-corpus-fixture.swift compiled with $SWIFTC"
} > "$TMP/provenance.txt"

mv "$TMP/symbols.txt" "$OUT_DIR/symbols.txt"
mv "$TMP/provenance.txt" "$OUT_DIR/provenance.txt"

echo "wrote $OUT_DIR/symbols.txt ($COUNT symbols, $TOOLCHAIN_DESC)"
echo "next: MULTI_DEMANGLE_UPDATE_SNAPSHOTS=1 cargo test --all-features --test test_swift_corpus"
