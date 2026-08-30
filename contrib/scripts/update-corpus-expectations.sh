#!/usr/bin/env bash
# Regenerates the oracle-backed expectation files for the new-language
# corpora: tests/corpus/{dlang,ada}_{golden,snapshot}.txt.
#
# The split is by authority (see tests/corpus/README.md):
#
#   golden   — our rendering agrees with GNU c++filt (libiberty's
#              independent D/GNAT demanglers). A change is a bug and fails
#              CI; regenerate only after a deliberate behavior change.
#   snapshot — everything else: the documented deliberate divergences from
#              the reference, and the symbols it fails. A change needs an
#              explicit refresh commit, but is not automatically a bug.
#
# Fortran (round-trip against fixture source names) and Kotlin/Native (no
# oracle exists) have hand-curated golden files and are not touched here.
#
# Usage:
#   cargo build --bin multi-demangle
#   contrib/scripts/update-corpus-expectations.sh
#
# Requires the multi-demangle/gnu-toolchains image (contrib/collect-corpus.sh
# build) and docker.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
CORPUS_DIR="$ROOT/tests/corpus"
GNU_IMAGE="multi-demangle/gnu-toolchains"
CLI="$ROOT/target/debug/multi-demangle"

[ -x "$CLI" ] || { echo "build the CLI first: cargo build --bin multi-demangle" >&2; exit 1; }

split_lang() {
    local lang="$1" fmt golden snapshot
    case "$lang" in
        dlang) fmt=dlang ;;
        ada)   fmt=gnat ;;
    esac
    golden="$CORPUS_DIR/${lang}_golden.txt"
    snapshot="$CORPUS_DIR/${lang}_snapshot.txt"

    local tmp; tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' RETURN
    # The whole corpus is split, not just the scheme-typical subset: the
    # fixture also emits deliberately-unmangled controls (C linkage, the
    # entry point) that pin the pipeline's rejection behavior.
    docker run --rm "$GNU_IMAGE" "$lang" 2>/dev/null | sort -u >"$tmp/syms.txt"
    docker run --rm -i --entrypoint c++filt "$GNU_IMAGE" -s "$fmt" \
        <"$tmp/syms.txt" >"$tmp/gnu.txt"

    : >"$tmp/golden"; : >"$tmp/snapshot"
    paste -d'\t' "$tmp/syms.txt" "$tmp/gnu.txt" | while IFS=$'\t' read -r sym gnu; do
        ours="$("$CLI" -- "$sym" 2>/dev/null || echo "$sym")"
        if [ "$ours" = "$sym" ]; then
            # The pipeline did not claim the symbol at all.
            printf '%s\t<rejected>\n' "$sym" >>"$tmp/snapshot"
        elif [ "$gnu" = "$ours" ]; then
            printf '%s\t%s\n' "$sym" "$ours" >>"$tmp/golden"
        else
            printf '%s\t%s\n' "$sym" "$ours" >>"$tmp/snapshot"
        fi
    done

    {
        echo "# Golden tier: renderings verified against GNU c++filt -s $fmt."
        echo "# Regenerate with contrib/scripts/update-corpus-expectations.sh;"
        echo "# a mismatch in tests/test_new_language_corpus.rs is a bug."
    } >"$golden"
    cat "$tmp/golden" >>"$golden"
    {
        echo "# Snapshot tier: our rendering where the oracle disagrees or fails."
        echo "# Deliberate divergences are documented in the module docs and"
        echo "# tests/corpus/README.md; refresh via"
        echo "# contrib/scripts/update-corpus-expectations.sh and review the diff."
    } >"$snapshot"
    cat "$tmp/snapshot" >>"$snapshot"

    printf '%-6s %3d golden, %3d snapshot\n' "$lang" \
        "$(grep -vc '^#' "$golden")" "$(grep -vc '^#' "$snapshot")"
}

split_lang dlang
split_lang ada
