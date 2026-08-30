#!/usr/bin/env bash
# Seeds every fuzz target's corpus from tests/corpus/*_symbols.txt.
#
# All corpus files are used for every target — the C++/Rust/Swift dumps as
# much as the toolchain-collected D/Ada/Fortran/Kotlin ones — because
# detection and dispatch see every scheme, and the FFI target must exercise
# its rejection path on non-Swift input too.
#
# libFuzzer treats each FILE in fuzz/corpus/<target>/ as one input, so each
# symbol becomes one file. The dumps hold up to 5,000 lines each; every Nth
# line is taken per file (deterministic, one pass) to keep the committed tree
# to a few thousand seeds per target instead of ~15k. Re-run this script
# after `contrib/collect-corpus.sh collect` to pick up regenerated corpora.
#
# Usage: fuzz/seed-corpus.sh [stride]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CORPUS_DIR="$ROOT/tests/corpus"
STRIDE="${1:-10}"   # take every Nth line per source file

targets=(demangle detect normalize swift_ffi)
for target in "${targets[@]}"; do
    out="$ROOT/fuzz/corpus/$target"
    mkdir -p "$out"
    rm -f "$out"/*.sym
done

for src in "$CORPUS_DIR"/*_symbols.txt; do
    lang="$(basename "$src" _symbols.txt)"
    lines=$(wc -l <"$src")
    # The nm dumps run to 5,000 lines; the toolchain corpora are under ~150.
    # Thin only the big files — every real D/Ada/Kotlin/Fortran symbol is a
    # seed worth keeping.
    if [ "$lines" -gt 1000 ]; then
        selection=$(awk -v stride="$STRIDE" 'NR % stride == 1' "$src")
    else
        selection=$(cat "$src")
    fi
    n=0
    while IFS= read -r sym; do
        # Skip empty/comment lines; the corpora have none, but be explicit.
        [ -z "$sym" ] && continue
        case "$sym" in '#'*) continue ;; esac
        for target in "${targets[@]}"; do
            # One input per file; index-based names (symbol text can carry
            # characters that are awkward as file names).
            printf '%s' "$sym" >"$ROOT/fuzz/corpus/$target/${lang}_$(printf '%05d' "$n").sym"
        done
        n=$((n + 1))
    done <<<"$selection"
    echo "    $lang: seeded $((n)) symbols per target"
done

for target in "${targets[@]}"; do
    echo "==> $target: $(ls "$ROOT/fuzz/corpus/$target" | wc -l | tr -d ' ') seeds"
done
