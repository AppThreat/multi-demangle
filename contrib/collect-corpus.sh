#!/usr/bin/env bash
# Collects real Fortran, Ada, D, and Kotlin/Native symbols using the
# contrib/ toolchain images, and (for D and Ada) diffs this crate's output
# against GNU c++filt, the independent reference demangler.
#
# This is the counterpart to scripts/collect-corpus.sh, which harvests
# Rust/C++/Swift symbols from binaries that happen to be on the machine. The
# languages here have no such binaries lying around, so the fixtures in
# contrib/fixtures/ are compiled on demand instead.
#
# Usage:
#   contrib/collect-corpus.sh build            # build the toolchain images
#   contrib/collect-corpus.sh collect          # write tests/corpus/*.txt
#   contrib/collect-corpus.sh diff [lang]      # differential vs c++filt
#   contrib/collect-corpus.sh diff-fuzz [count] [seed]
#                                              # generator-driven differential
#   contrib/collect-corpus.sh all
#
# Kotlin/Native is opt-in (WITH_KOTLIN=1): its image is a ~1 GB download and
# is linux/amd64 only, so it runs under emulation on Apple silicon.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CONTRIB="$ROOT/contrib"
CORPUS_DIR="$ROOT/tests/corpus"
GNU_IMAGE="multi-demangle/gnu-toolchains"
KOTLIN_IMAGE="multi-demangle/kotlin-native"
WITH_KOTLIN="${WITH_KOTLIN:-0}"
CLI="${CLI:-$ROOT/target/debug/multi-demangle}"

build_images() {
    echo "==> building $GNU_IMAGE"
    docker build -f "$CONTRIB/docker/gnu-toolchains.Dockerfile" -t "$GNU_IMAGE" "$CONTRIB"
    if [ "$WITH_KOTLIN" = "1" ]; then
        echo "==> building $KOTLIN_IMAGE (large download, amd64 under emulation)"
        docker build -f "$CONTRIB/docker/kotlin-native.Dockerfile" -t "$KOTLIN_IMAGE" "$CONTRIB"
    else
        echo "==> skipping $KOTLIN_IMAGE (set WITH_KOTLIN=1 to include it)"
    fi
}

# Emits the symbols for one language on stdout.
emit() {
    case "$1" in
        kotlin) docker run --rm "$KOTLIN_IMAGE" symbols ;;
        *)      docker run --rm "$GNU_IMAGE" "$1" ;;
    esac
}

collect() {
    mkdir -p "$CORPUS_DIR"
    local languages=(fortran ada dlang)
    [ "$WITH_KOTLIN" = "1" ] && languages+=(kotlin)

    for lang in "${languages[@]}"; do
        local out="$CORPUS_DIR/${lang}_symbols.txt"
        echo "==> ${lang}_symbols.txt"
        emit "$lang" | sort -u >"$out"
        echo "    $(wc -l <"$out") symbols"
    done

    # Provenance: which toolchains produced the files above. The symbol
    # spelling is a compiler implementation detail, so this is part of the
    # test data's meaning, not decoration.
    {
        echo "# Collected $(date -u +%Y-%m-%dT%H:%M:%SZ) via contrib/collect-corpus.sh"
        docker run --rm "$GNU_IMAGE" versions
        [ "$WITH_KOTLIN" = "1" ] && docker run --rm "$KOTLIN_IMAGE" versions
    } >"$CORPUS_DIR/new-languages-provenance.txt"
    echo "==> new-languages-provenance.txt"
}

# Reads a `syms<TAB>gnu<TAB>ours` comparison table and reports the
# classification shared by `diff` and `diff-fuzz`. Both tools echo the input
# unchanged when they cannot demangle, so "output == input" is the rejection
# test. c++filt's gnat mode has a second failure spelling: it wraps the
# symbol in angle brackets (`<corpus__workerTB>`). Counting that as a
# successful demangle would invent functional gaps that do not exist — the
# reference failed too.
classify_cmp() {
    local title="$1" cmp="$2"
    local total agree reject differ
    total=$(wc -l <"$cmp")
    local gnu_failed='($2==$1) || ($2 ~ /^<.*>$/)'
    agree=$(awk -F'\t' '$2==$3' "$cmp" | wc -l)
    reject=$(awk -F'\t' "\$1==\$3 && !($gnu_failed)" "$cmp" | wc -l)
    differ=$(awk -F'\t' "\$1!=\$3 && !($gnu_failed) && \$2!=\$3" "$cmp" | wc -l)

    echo "=== $title ==="
    printf 'total symbols:                   %s\n' "$total"
    printf 'exact agreement:                 %s\n' "$agree"
    printf 'we reject, c++filt demangles:    %s   <- functional gaps\n' "$reject"
    printf 'both demangle, rendering differs: %s\n' "$differ"

    if [ "$reject" -gt 0 ]; then
        echo; echo "--- functional gaps ---"
        awk -F"\t" "\$1==\$3 && !($gnu_failed) {printf \"%-52s -> %s\\n\", \$1, \$2}" "$cmp"
    fi
    if [ "$differ" -gt 0 ]; then
        echo; echo "--- rendering differences ---"
        awk -F"\t" "\$1!=\$3 && !($gnu_failed) && \$2!=\$3 {printf \"sym: %s\\n  gnu: %s\\n  us : %s\\n\", \$1, \$2, \$3}" "$cmp"
    fi
}

# Differential comparison against GNU c++filt, which embeds libiberty's
# independent D and GNAT demanglers. Rendering differences are expected and
# informative; *acceptance* differences (we reject what c++filt demangles)
# are functional gaps.
diff_lang() {
    local lang="$1" filter fmt ours
    # `ours` is this crate's --language value; Ada (like Fortran) is opt-in
    # and is not auto-detected, so the differential must request it.
    case "$lang" in
        dlang) filter='^_D'; fmt=dlang; ours=d ;;
        ada)   filter='.'; fmt=gnat; ours=ada ;;
        *) echo "no c++filt oracle for $lang" >&2; return 1 ;;
    esac

    [ -x "$CLI" ] || { echo "build the CLI first: cargo build --bin multi-demangle" >&2; return 1; }

    local tmp; tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' RETURN
    emit "$lang" | grep -E "$filter" | sort -u >"$tmp/syms.txt"
    docker run --rm -i --entrypoint c++filt "$GNU_IMAGE" -s "$fmt" <"$tmp/syms.txt" >"$tmp/gnu.txt"
    : >"$tmp/ours.txt"
    while IFS= read -r s; do
        "$CLI" --language "$ours" -- "$s" 2>/dev/null || echo "$s"
    done <"$tmp/syms.txt" >>"$tmp/ours.txt"

    paste -d'\t' "$tmp/syms.txt" "$tmp/gnu.txt" "$tmp/ours.txt" >"$tmp/cmp.tsv"
    classify_cmp "$lang vs c++filt -s $fmt" "$tmp/cmp.tsv"
}

# Generator-driven differential (Plan 05): synthesizes D symbols from the ABI
# grammar (contrib/scripts/gen_dlang_symbols.py — from the spec, not from the
# parser's shape) and classifies every disagreement with the same rules as
# `diff`. Symbols the oracle itself rejects are generator noise and are
# counted separately ("both reject") so they cannot masquerade as agreement.
diff_fuzz() {
    local count="${1:-50000}" seed="${2:-1}" fmt=dlang
    local gen="$CONTRIB/scripts/gen_dlang_symbols.py"

    [ -x "$CLI" ] || { echo "build the CLI first: cargo build --bin multi-demangle" >&2; return 1; }

    local tmp; tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' RETURN
    python3 "$gen" --count "$count" --seed "$seed" >"$tmp/syms.txt"
    # Filter mode: one token per line in, one demangled (or echoed) line out.
    "$CLI" <"$tmp/syms.txt" >"$tmp/ours.txt"
    docker run --rm -i --entrypoint c++filt "$GNU_IMAGE" -s "$fmt" <"$tmp/syms.txt" >"$tmp/gnu.txt"
    paste -d'\t' "$tmp/syms.txt" "$tmp/gnu.txt" "$tmp/ours.txt" >"$tmp/cmp.tsv"

    classify_cmp "generated D symbols (seed $seed) vs c++filt -s dlang" "$tmp/cmp.tsv"

    local both
    both=$(awk -F'\t' '($2==$1) && ($3==$1)' "$tmp/cmp.tsv" | wc -l)
    printf 'both reject (generator noise):   %s\n' "$both"
    echo; echo "--- we demangle, c++filt rejects ---"
    awk -F'\t' '($2==$1) && ($3!=$1) {printf "%-60s -> %s\n", $1, $3}' "$tmp/cmp.tsv" | head -20
}

case "${1:-all}" in
    build)     build_images ;;
    collect)   collect ;;
    diff)      diff_lang "${2:-dlang}" ;;
    diff-fuzz) diff_fuzz "${2:-50000}" "${3:-1}" ;;
    all)
        build_images
        collect
        diff_lang dlang || true
        diff_lang ada || true
        ;;
    *)
        echo "usage: $0 <build|collect|diff [lang]|diff-fuzz [count] [seed]|all>" >&2
        exit 2
        ;;
esac
