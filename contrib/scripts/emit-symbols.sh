#!/usr/bin/env bash
# Compiles the fixtures inside the gnu-toolchains image and prints the
# resulting symbols, one per line, to stdout.
#
# Usage (inside the container):
#   emit-symbols <fortran|ada|dlang|versions|all>
#
# Everything is written under /tmp so the image stays reusable, and each
# language is independent: a toolchain that fails does not take the others
# down with it.
set -uo pipefail

FIXTURES="${FIXTURES:-/fixtures}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Extracts defined (not undefined) symbol names from object files.
# `nm --defined-only` drops imports, which are noise for a demangler corpus.
# With several object files nm interleaves `<file>:` headers and blank lines;
# both must go, or the corpus picks up container paths as if they were
# symbols.
dump_symbols() {
    nm --defined-only "$@" 2>/dev/null |
        awk 'NF && $NF !~ /:$/ {print $NF}' |
        sort -u
}

collect_fortran() {
    local dir="$WORK/fortran"
    mkdir -p "$dir"
    cp "$FIXTURES"/fortran/*.f90 "$dir/" || return 1
    ( cd "$dir" && gfortran -c -O0 ./*.f90 ) || {
        echo "fortran: compilation failed" >&2
        return 1
    }
    dump_symbols "$dir"/*.o
}

collect_ada() {
    local dir="$WORK/ada"
    mkdir -p "$dir"
    cp "$FIXTURES"/ada/* "$dir/" || return 1
    # gnatmake drives compile + bind + link; -gnatW8 enables UTF-8 source
    # identifiers so the U/W escape encodings appear. Keep going on link
    # failure: the object files are what we need.
    ( cd "$dir" && gnatmake -q -gnatW8 -c ./*.adb ) || {
        echo "ada: compilation failed" >&2
        return 1
    }
    dump_symbols "$dir"/*.o
}

collect_dlang() {
    local dir="$WORK/dlang"
    mkdir -p "$dir"
    cp "$FIXTURES"/dlang/*.d "$dir/" || return 1
    local produced=0
    # LDC and GDC agree on the ABI mangling but emit different symbol sets
    # (druntime hooks, template instantiation policy), so collect from both.
    if command -v ldc2 >/dev/null 2>&1; then
        if ( cd "$dir" && ldc2 -c -unittest -of=ldc.o ./corpus.d ) 2>/dev/null; then
            dump_symbols "$dir/ldc.o"
            produced=1
        else
            echo "dlang: ldc2 compilation failed" >&2
        fi
    fi
    if command -v gdc >/dev/null 2>&1; then
        if ( cd "$dir" && gdc -c -funittest -o gdc.o ./corpus.d ) 2>/dev/null; then
            dump_symbols "$dir/gdc.o"
            produced=1
        else
            echo "dlang: gdc compilation failed" >&2
        fi
    fi
    [ "$produced" -eq 1 ]
}

case "${1:-all}" in
    fortran) collect_fortran ;;
    ada)     collect_ada ;;
    dlang)   collect_dlang ;;
    versions) cat /toolchain-versions.txt ;;
    all)
        collect_fortran
        collect_ada
        collect_dlang
        ;;
    *)
        echo "usage: emit-symbols <fortran|ada|dlang|versions|all>" >&2
        exit 2
        ;;
esac
