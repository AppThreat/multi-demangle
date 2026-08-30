#!/usr/bin/env bash
# Compiles the Kotlin fixture inside the kotlin-native image and prints the
# resulting symbols, one per line, to stdout.
#
# Usage (inside the container):
#   emit-symbols [symbols|versions|raw]
#
#   symbols  — symbol names only (default)
#   raw      — the unfiltered nm output, for when you need to see section
#              and type letters to work out what the compiler is emitting
#   versions — the baked-in toolchain versions
set -uo pipefail

FIXTURES="${FIXTURES:-/fixtures}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

build() {
    cp "$FIXTURES"/kotlin/*.kt "$WORK/" || return 1
    # -produce static gives a .a whose objects carry the full symbol table;
    # a linked executable would have had much of it stripped or hidden.
    ( cd "$WORK" && kotlinc-native -produce static -o corpus ./corpus.kt ) >/dev/null 2>&1 || {
        echo "kotlin: compilation failed" >&2
        return 1
    }
    find "$WORK" -name '*.a' -o -name '*.o' | head -20
}

case "${1:-symbols}" in
    versions) cat /toolchain-versions.txt ;;
    raw)
        mapfile -t artifacts < <(build) || exit 1
        [ "${#artifacts[@]}" -gt 0 ] || { echo "kotlin: no artifacts produced" >&2; exit 1; }
        nm --defined-only "${artifacts[@]}" 2>/dev/null
        ;;
    symbols)
        mapfile -t artifacts < <(build) || exit 1
        [ "${#artifacts[@]}" -gt 0 ] || { echo "kotlin: no artifacts produced" >&2; exit 1; }
        nm --defined-only "${artifacts[@]}" 2>/dev/null |
            awk 'NF && $NF !~ /:$/ {print $NF}' |
            grep -E '^_*kfun:' |
            sort -u |
            # Keep the corpus small and legible (grammar coverage, not
            # volume): every symbol of the fixture's own package, plus a
            # deterministic ~1-in-8 sample of the compiler/runtime symbols
            # for background coverage of box/unbox, coroutines, arrays.
            awk '{ if ($0 ~ /com\.example/) print; else if (NR % 8 == 0) print }' |
            sort -u
        ;;
    *)
        echo "usage: emit-symbols [symbols|raw|versions]" >&2
        exit 2
        ;;
esac
