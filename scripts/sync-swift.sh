#!/usr/bin/env bash
# Syncs the vendored Swift demangler subset (vendor/swift) from upstream and
# runs the full validation gauntlet. Provenance is recorded per sync in
# vendor/swift/SYNC.md.
#
# Usage:
#   scripts/sync-swift.sh [SWIFT_TAG]
#
# Without an argument the newest `swift-*-RELEASE` tag on the Swift repo is
# used. A tag may be given with or without the `-RELEASE` suffix. The LLVM
# headers the demangler depends on are synced from swiftlang/llvm-project at
# the matching release tag.
#
# Environment overrides:
#   SWIFT_REPO   upstream Swift repository   (default: https://github.com/apple/swift.git)
#   LLVM_REPO    upstream LLVM repository    (default: https://github.com/swiftlang/llvm-project.git)
#   LLVM_TAG     LLVM tag to sync            (default: same release tag; `keep` skips LLVM)
#   SKIP_TESTS=1 run the copy only, no validation (not recommended)
#
# The script requires a clean vendor/swift tree (use --force to override) and
# leaves all changes uncommitted for review.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VENDOR="$REPO_ROOT/vendor/swift"
MANIFEST="$VENDOR/MANIFEST.txt"
SYNC_DOC="$VENDOR/SYNC.md"
SWIFT_REPO="${SWIFT_REPO:-https://github.com/apple/swift.git}"
LLVM_REPO="${LLVM_REPO:-https://github.com/swiftlang/llvm-project.git}"

# Headers the demangler may pull in beyond the manifest, kept in the sparse
# checkout so the missing-header fixup loop below can copy them on demand.
SWIFT_SPARSE=(
    '/lib/Demangling/**'
    '/include/swift/Demangling/**'
    '/include/swift/ABI/**'
    '/include/swift/AST/**'
    '/include/swift/Basic/**'
    '/include/swift/Strings.h'
    '/LICENSE.txt'
)
LLVM_SPARSE=(
    '/llvm/include/llvm/ADT/**'
    '/llvm/include/llvm/Support/**'
    '/llvm/include/llvm-c/**'
    '/llvm/LICENSE.TXT'
)

# build.rs compiles these lib/Demangling translation units; CrashReporter.cpp
# is intentionally omitted (SWIFT_RUNTIME_NO_CRASH_REPORTER).
BUILD_CPP="Context.cpp Demangler.cpp ManglingUtils.cpp NodeDumper.cpp NodePrinter.cpp Punycode.cpp Remangler.cpp Errors.cpp"

FORCE=0
TAG=""
for arg in "$@"; do
    case "$arg" in
        --force) FORCE=1 ;;
        -h|--help) sed -n '2,21p' "$0"; exit 0 ;;
        *) TAG="$arg" ;;
    esac
done

say()  { printf '%s\n' "$*"; }
die()  { printf 'sync-swift: %s\n' "$*" >&2; exit 1; }

command -v git >/dev/null || die "git is required"
command -v cargo >/dev/null || die "cargo is required"

# ---------------------------------------------------------------------------
# 0. Preconditions
# ---------------------------------------------------------------------------
cd "$REPO_ROOT"
if [ "$FORCE" -eq 0 ] && [ -n "$(git status --porcelain -- vendor/swift)" ]; then
    die "vendor/swift has uncommitted changes; commit/stash them or pass --force"
fi

# ---------------------------------------------------------------------------
# 1. Resolve the upstream tags
# ---------------------------------------------------------------------------
normalize_tag() {
    case "$1" in
        *-RELEASE) echo "$1" ;;
        *) echo "$1-RELEASE" ;;
    esac
}

if [ -z "$TAG" ]; then
    say "Resolving newest swift-*-RELEASE tag on $SWIFT_REPO ..."
    TAG="$(git ls-remote --tags "$SWIFT_REPO" \
        | sed -n 's#.*refs/tags/\(swift-[0-9.]*-RELEASE\)$#\1#p' \
        | sort -V | tail -1)"
    [ -n "$TAG" ] || die "could not determine the newest Swift release tag"
fi
TAG="$(normalize_tag "$TAG")"

LLVM_TAG="${LLVM_TAG:-$TAG}"
if [ "$LLVM_TAG" != "keep" ]; then
    LLVM_TAG="$(normalize_tag "$LLVM_TAG")"
fi

say "Swift tag: $TAG"
if [ "$LLVM_TAG" = "keep" ]; then
    say "LLVM headers: keeping the currently vendored set"
else
    say "LLVM tag:   $LLVM_TAG"
fi

# ---------------------------------------------------------------------------
# 2. Shallow, blobless, sparse clones of the upstream trees
# ---------------------------------------------------------------------------
TMP="$(mktemp -d)"
# Validation logs must outlive $TMP so a FAIL pointer stays readable; they are
# per-run paths rather than fixed /tmp names shared with other users' runs.
LOG_DIR="$(mktemp -d)"
CARGO_LOG="$LOG_DIR/cargo-test.log"
PY_LOG="$LOG_DIR/pytest.log"
trap 'rm -rf "$TMP"' EXIT INT TERM
SWIFT_TREE="$TMP/swift"
LLVM_TREE="$TMP/llvm"

sparse_clone() { # repo tag dest patterns...
    local repo="$1" tag="$2" dest="$3"
    shift 3
    git clone --quiet --depth 1 --branch "$tag" --filter=blob:none --sparse "$repo" "$dest" \
        || die "clone of $repo at $tag failed (tag missing?)"
    git -C "$dest" sparse-checkout set --no-cone "$@"
}

say "Cloning Swift sources (sparse) ..."
sparse_clone "$SWIFT_REPO" "$TAG" "$SWIFT_TREE" "${SWIFT_SPARSE[@]}"

if [ "$LLVM_TAG" != "keep" ]; then
    say "Cloning LLVM sources (sparse) ..."
    sparse_clone "$LLVM_REPO" "$LLVM_TAG" "$LLVM_TREE" "${LLVM_SPARSE[@]}"
fi

SWIFT_COMMIT="$(git -C "$SWIFT_TREE" rev-parse HEAD)"
LLVM_COMMIT=""
if [ "$LLVM_TAG" != "keep" ]; then
    LLVM_COMMIT="$(git -C "$LLVM_TREE" rev-parse HEAD)"
fi

# ---------------------------------------------------------------------------
# 3. Copy the manifest file list from the upstream trees
#    (include/llvm/* and LICENSE_LLVM.txt map onto the LLVM tree)
# ---------------------------------------------------------------------------
upstream_path_for() { # <repo-relative vendored path> -> prints absolute source,
                      # "KEEP" when LLVM syncing is off for llvm-owned paths, or
                      # "" when the file no longer exists upstream
    case "$1" in
        include/llvm/*|include/llvm-c/*)
            if [ "$LLVM_TAG" = "keep" ]; then echo "KEEP"; return 0; fi
            echo "$LLVM_TREE/llvm/$1" ;;
        LICENSE_LLVM.txt)
            if [ "$LLVM_TAG" = "keep" ]; then echo "KEEP"; return 0; fi
            echo "$LLVM_TREE/llvm/LICENSE.TXT" ;;
        *)
            echo "$SWIFT_TREE/$1" ;;
    esac
}

say "Copying manifest files ..."
missing_upstream=()
copied=0
while IFS= read -r rel; do
    case "$rel" in
        ""|\#*) continue ;;
        MANIFEST.txt|README.md|SYNC.md) continue ;;   # ours, not upstream
        # Our stubs for CMake-generated upstream headers: no source version of
        # these files exists upstream (see each stub's header comment).
        include/llvm/Config/llvm-config.h|include/llvm/Config/abi-breaking.h) continue ;;
    esac
    src="$(upstream_path_for "$rel")"
    if [ "$src" = "KEEP" ]; then
        continue
    fi
    if [ -z "$src" ] || [ ! -f "$src" ]; then
        missing_upstream+=("$rel")
        continue
    fi
    mkdir -p "$VENDOR/$(dirname "$rel")"
    cp "$src" "$VENDOR/$rel"
    copied=$((copied + 1))
done < "$MANIFEST"

if [ "${#missing_upstream[@]}" -gt 0 ]; then
    say "WARNING: ${#missing_upstream[@]} manifest file(s) no longer exist upstream:"
    printf '  %s\n' "${missing_upstream[@]}"
    say "         They were left in place; drop them from MANIFEST.txt if the build passes."
fi

# ---------------------------------------------------------------------------
# 4. Build probe: copy headers upstream now requires but the manifest lacks
# ---------------------------------------------------------------------------
added_headers=()
probe_status=0
probe_log="$TMP/probe-build.log"
# Runs the build, leaving its output in $probe_log and cargo's exit code in
# $probe_status. It is a command (not a `$(...)` substitution) precisely so the
# status survives: a subshell could not set $probe_status for the caller.
probe_build() {
    if cargo build --features swift --message-format=short >"$probe_log" 2>&1; then
        probe_status=0
    else
        probe_status=$?
    fi
}

say "Probing build for headers new to the demangler's dependency graph ..."
attempts=0
while :; do
    probe_build
    out="$(cat "$probe_log")"
    if [ "$probe_status" -eq 0 ]; then
        break
    fi
    header="$(printf '%s\n' "$out" \
        | sed -n "s/^.*fatal error: '\([^']*\)' file not found.*/\1/p" | head -1)"
    if [ -z "$header" ]; then
        break
    fi
    attempts=$((attempts + 1))
    if [ "$attempts" -gt 25 ]; then
        say "Giving up after 25 missing-header fixups; remaining build output:"
        printf '%s\n' "$out"
        exit 1
    fi
    # The compiler reports the path as written in the #include (relative to an
    # include root); the vendored tree stores headers under `include/`.
    case "$header" in
        include/*) dest="$header" ;;
        *) dest="include/$header" ;;
    esac
    src=""
    for candidate in "$header" "$dest"; do
        src="$(upstream_path_for "$candidate")"
        if [ -n "$src" ] && [ "$src" != "KEEP" ] && [ -f "$src" ]; then
            break
        fi
        src=""
    done
    if [ -z "$src" ] && [ "$LLVM_TAG" = "keep" ]; then
        say "Build needs '$header', but LLVM syncing is disabled (LLVM_TAG=keep)."
        say "Re-run without LLVM_TAG=keep, or vendor the header by hand."
        printf '%s\n' "$out"
        exit 1
    fi
    if [ -z "$src" ]; then
        say "Build needs '$header', which is absent from both upstream trees."
        say "(If it is generated upstream or lives outside the sparse checkout,"
        say " vendor a minimal stub by hand and re-run.)"
        printf '%s\n' "$out"
        exit 1
    fi
    mkdir -p "$VENDOR/$(dirname "$dest")"
    cp "$src" "$VENDOR/$dest"
    added_headers+=("$dest")
    say "  + $dest"
done

# Trust cargo's exit code rather than grepping its text: a linker failure or a
# differently-worded diagnostic must not read as a successful sync.
if [ "$probe_status" -ne 0 ]; then
    say "Build still failing after header fixups (a new upstream translation unit may be required):"
    printf '%s\n' "$out"
    exit 1
fi

# New upstream translation units the manifest/build.rs do not know about.
new_cpp=""
for cpp in "$SWIFT_TREE"/lib/Demangling/*.cpp; do
    base="$(basename "$cpp")"
    case " $BUILD_CPP " in
        *" $base "*) ;;
        *) new_cpp="$new_cpp  lib/Demangling/$base (not compiled by build.rs)\n" ;;
    esac
done
if [ -n "$new_cpp" ]; then
    say "NOTE: upstream lib/Demangling has translation units this build does not compile:"
    printf '%b' "$new_cpp"
    say "      CrashReporter.cpp is intentionally omitted; for any other file, judge"
    say "      whether it belongs to the demangler's dependency graph and add it to"
    say "      build.rs and MANIFEST.txt."
fi

# ---------------------------------------------------------------------------
# 6. Validation gauntlet
# ---------------------------------------------------------------------------

# Sanitizer corpus pass: compiles the vendored demangler with ASan/UBSan and
# pushes the real-world corpus through demangle + node dump. This is the
# check that catches C++ regressions slipping in through a sync. Returns 0
# both on success and when the check cannot run (compiler missing); fails
# only when the harness runs and trips a sanitizer.
sanitize_corpus() {
    local cxx="${CXX:-clang++}"
    if ! command -v "$cxx" >/dev/null; then
        say "  skipped: no C++ compiler"
        return 0
    fi
    local harness="$TMP/sanitize_harness.cpp"
    cat > "$harness" <<'EOF'
// Reads mangled symbols from stdin and exercises the vendored demangler:
// string demangling, symbol detection, and the node-tree dump. Built with
// ASan/UBSan by scripts/sync-swift.sh; any sanitizer abort fails the sync.
#include "swift/Demangling/Demangle.h"
#include <iostream>
#include <string>

int main() {
    std::string line;
    swift::Demangle::Context context;
    while (std::getline(std::cin, line)) {
        if (line.empty()) continue;
        auto demangled = swift::Demangle::demangleSymbolAsString(
            llvm::StringRef(line),
            swift::Demangle::DemangleOptions::SimplifiedUIDemangleOptions());
        bool is_swift = swift::Demangle::isSwiftSymbol(line);
        if (is_swift) {
            if (auto *node = context.demangleSymbolAsNode(llvm::StringRef(line))) {
                (void)swift::Demangle::getNodeTreeAsString(node);
            }
        }
        (void)demangled;
    }
    return 0;
}
EOF
    local srcs=""
    local base
    for base in $BUILD_CPP; do
        srcs="$srcs $VENDOR/lib/Demangling/$base"
    done
    # shellcheck disable=SC2086
    if ! "$cxx" -std=c++17 -fsanitize=address,undefined -fno-sanitize-recover=undefined \
            -fno-omit-frame-pointer -g -O1 \
            -I"$VENDOR/include" \
            -DSWIFT_STDLIB_HAS_TYPE_PRINTING=1 \
            -DSWIFT_SUPPORTS_CONCURRENCY=1 \
            -DLLVM_DISABLE_ABI_BREAKING_CHECKS_ENFORCING=1 \
            -DSWIFT_RUNTIME_NO_CRASH_REPORTER=1 \
            -Wno-unused-parameter -Wno-deprecated-declarations \
            $srcs "$harness" -o "$TMP/sanitize_harness" 2>"$TMP/sanitize-build.log"; then
        say "  skipped: sanitizer harness failed to compile ($(head -3 "$TMP/sanitize-build.log" | tr '\n' ' '))"
        return 0
    fi
    if [ -f "$REPO_ROOT/tests/corpus/swift_symbols.txt" ]; then
        "$TMP/sanitize_harness" < "$REPO_ROOT/tests/corpus/swift_symbols.txt"
    else
        # shellcheck disable=SC2016  # a mangled symbol, not a shell expansion
        printf '_$s4main1fyySitF\n' | "$TMP/sanitize_harness"
    fi
}

swift_test="SKIPPED"
py_test="SKIPPED"
sanitizer_test="SKIPPED"

if [ "${SKIP_TESTS:-0}" != "1" ]; then
    say "Running cargo test --all-features ..."
    # `--all-features` implies `extension-module`, which on macOS only links
    # with dynamic symbol lookup (mirrors CI.yml).
    cargo_env=()
    if [ "$(uname -s)" = "Darwin" ]; then
        cargo_env=("RUSTFLAGS=-C link-arg=-undefined -C link-arg=dynamic_lookup")
    fi
    # `env` with no assignments is a no-op, but an empty array expansion is
    # fatal under `set -u` on bash < 4.4, so guard it.
    if env ${cargo_env[@]+"${cargo_env[@]}"} cargo test --all-features >"$CARGO_LOG" 2>&1; then
        swift_test="PASS"
    else
        swift_test="FAIL (log: $CARGO_LOG)"
    fi

    if [ -x "$REPO_ROOT/.venv/bin/maturin" ] && [ -x "$REPO_ROOT/.venv/bin/python" ]; then
        say "Running maturin develop --all-features + pytest ..."
        if "$REPO_ROOT/.venv/bin/maturin" develop --all-features >"$PY_LOG" 2>&1 \
            && "$REPO_ROOT/.venv/bin/python" -m pytest python/tests >>"$PY_LOG" 2>&1; then
            py_test="PASS"
        else
            py_test="FAIL (log: $PY_LOG)"
        fi
    fi

    say "Running ASan/UBSan corpus pass ..."
    if sanitize_corpus; then
        sanitizer_test="PASS"
    else
        sanitizer_test="FAIL (see output above)"
    fi
fi

# ---------------------------------------------------------------------------
# 7. Provenance: SYNC.md entry + machine-readable metadata block
# ---------------------------------------------------------------------------
today="$(date +%Y-%m-%d)"
diffstat="$(git diff --stat -- vendor/swift | tail -1)"
# -z + NUL parsing keeps paths with spaces intact (they land in SYNC.md).
untracked="$(git status --porcelain -z -- vendor/swift \
    | tr '\0' '\n' | sed -n 's/^?? /  /p')"
added_list="none"
if [ "${#added_headers[@]}" -gt 0 ]; then
    added_list="$(printf '%s\n' "${added_headers[@]}")"
fi
missing_list="none"
if [ "${#missing_upstream[@]}" -gt 0 ]; then
    missing_list="$(printf '%s\n' "${missing_upstream[@]}")"
fi

# Rewrite any previous metadata block so exactly one (the latest) remains.
if [ -f "$SYNC_DOC" ]; then
    sed -i.bak '/^<!-- sync-metadata$/,/^-->$/d' "$SYNC_DOC" && rm -f "$SYNC_DOC.bak"
fi

if [ ! -f "$SYNC_DOC" ]; then
    cat > "$SYNC_DOC" <<'EOF'
# Swift vendor sync log

Records of every `vendor/swift` sync from upstream Swift, produced by
`scripts/sync-swift.sh <swift-tag>` (run it rather than editing by hand).
Each entry lists the upstream refs, what changed in the vendored subset,
the diffstat, and the validation results. The `sync-metadata` block at the
bottom always describes the most recent sync; the monthly CI reminder
workflow parses it to list upstream commits since the last sync.
EOF
fi

{
    echo
    echo "## $today — $TAG"
    echo
    echo "- Swift ref: $TAG ($SWIFT_COMMIT)"
    if [ -n "$LLVM_COMMIT" ]; then
        echo "- LLVM ref: $LLVM_TAG ($LLVM_COMMIT)"
    else
        echo "- LLVM: vendored set kept"
    fi
    echo "- Headers added to the manifest: $added_list"
    echo "- Manifest files missing upstream: $missing_list"
    echo "- Diffstat: ${diffstat:-no changes}"
    if [ -n "$untracked" ]; then
        echo "- New files:"
        printf '%s\n' "$untracked"
    fi
    echo "- Validation: cargo test --all-features $swift_test; pytest $py_test; ASan/UBSan corpus $sanitizer_test"
    echo
    echo "<!-- sync-metadata"
    echo "swift-ref: $TAG"
    echo "swift-commit: $SWIFT_COMMIT"
    echo "llvm-ref: ${LLVM_TAG:-keep}"
    echo "llvm-commit: ${LLVM_COMMIT:-}"
    echo "date: $today"
    echo "-->"
} >> "$SYNC_DOC"

# ---------------------------------------------------------------------------
# 7b. Regenerate MANIFEST.txt (kept files under vendor/swift, sorted)
#     Runs after SYNC.md so the manifest enumerates it too.
# ---------------------------------------------------------------------------
{
    echo "# Vendored Swift subset manifest"
    echo "#"
    echo "# This file is intentionally checked in and enumerates every kept file under"
    echo "# vendor/swift after trimming the upstream Swift import to the minimal subset"
    echo "# required by multi-demangle."
    echo "#"
    echo "# Regenerated by scripts/sync-swift.sh; validate builds/tests after each sync."
    count="$(find "$VENDOR" -type f ! -name '.DS_Store' | wc -l | tr -d ' ')"
    echo "# File count: $count"
    echo
    (cd "$VENDOR" && find . -type f ! -name '.DS_Store' | sed 's|^\./||' | LC_ALL=C sort)
} > "$MANIFEST"

# ---------------------------------------------------------------------------
# 8. Summary + human checklist
# ---------------------------------------------------------------------------
say
say "Sync complete: vendor/swift is now at $TAG ($SWIFT_COMMIT)."
say "  cargo test --all-features: $swift_test"
say "  pytest:                    $py_test"
say "  ASan/UBSan corpus:         $sanitizer_test"
say
say "Manual checklist before committing:"
say "  1. Review the vendored diff (git diff --stat -- vendor/swift) and SYNC.md."
say "  2. If demangling output changed, update the corpus snapshots:"
say "       MULTI_DEMANGLE_UPDATE_SNAPSHOTS=1 cargo test --all-features --test test_swift_corpus"
say "     and review the expected.txt diff — blint matching depends on it."
say "  3. Check upstream option-bit parity (SYMBOLIC_SWIFT_FEATURE_*):"
grep -A 30 "struct DemangleOptions" "$SWIFT_TREE/include/swift/Demangling/Demangle.h" | sed 's/^/         /' || true
say "  4. If the supported-version claim moved, update the README table and"
say "     lib.rs docs, and add corpus per the new toolchain:"
say "       scripts/collect-swift-corpus.sh"
say "  5. Commit vendor/swift, scripts, and SYNC.md together."
