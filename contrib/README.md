# contrib/ — toolchain images for the new-language backends

The D, Fortran, Kotlin/Native, and Ada backends (Plan 07) were written
without any of their compilers installed. Every test for them was
hand-written by the same process that wrote the parser, which means a wrong
belief about the grammar produced both a wrong parser *and* a test asserting
the wrong behavior. This directory breaks that circularity: it compiles real
fixtures with real compilers and treats `nm` output as ground truth.

That is not a hypothetical concern. The first run of these images found four
silent-corruption bugs that the hand-written suite asserted as correct:

| Bug | Hand-written belief | What the compiler actually emits |
| --- | ------------------- | -------------------------------- |
| Fortran length suffix | `__m_MOD_step_12` → `m::step` | `step_12` is the real name; gfortran appends no suffix |
| Fortran g77 form | `two_words_` is invalid | gfortran emits exactly that for `subroutine two_words` |
| D static array | `Gi3` (type, then dimension) | `G3i` — dimension first; `c++filt` rejects the reverse |
| D function pointer | `int function(int)*` | no trailing `*` — `P` over a function type is the pointer |

## Layout

```
contrib/
  collect-corpus.sh          driver: build images, collect corpora, diff vs c++filt
  docker/
    gnu-toolchains.Dockerfile   gfortran + gnat + ldc2 + gdc + binutils
    kotlin-native.Dockerfile    Kotlin/Native (opt-in; large, amd64-only)
  fixtures/
    fortran/corpus.f90       module procedures, names ending in digits, bare subprograms
    ada/                     packages, child packages, operators, tasks, nested blocks
    dlang/corpus.d           templates, member functions, modifiers, delegates, back references
    kotlin/corpus.kt         top-level funs, classes, generics, nullables, data classes
  scripts/
    emit-symbols.sh          compiles fixtures inside the GNU image, prints symbols
    emit-kotlin-symbols.sh   same for the Kotlin/Native image
    update-corpus-expectations.sh  regenerates the D/Ada golden/snapshot expectation tiers
```

## Usage

```bash
# One-time: build the GNU image (~2 min, no large downloads).
contrib/collect-corpus.sh build

# Write tests/corpus/{fortran,ada,dlang}_symbols.txt + provenance.
contrib/collect-corpus.sh collect

# Differential comparison against the reference demangler.
cargo build --bin multi-demangle
contrib/collect-corpus.sh diff dlang
contrib/collect-corpus.sh diff ada

# Generator-driven differential (Plan 05): synthesizes D symbols from the
# ABI grammar and classifies every disagreement with the same rules.
contrib/collect-corpus.sh diff-fuzz 50000 1   # count, seed
```

Kotlin/Native is opt-in because its image is a ~1 GB download published for
linux/amd64 only, so it runs under emulation on Apple silicon:

```bash
WITH_KOTLIN=1 contrib/collect-corpus.sh build
WITH_KOTLIN=1 contrib/collect-corpus.sh collect
```

## The oracle

GNU `c++filt` embeds libiberty's independent D and GNAT demanglers, so it is
a genuine second implementation to diff against — not the one this crate's D
backend was ported from:

```bash
docker run --rm -i --entrypoint c++filt multi-demangle/gnu-toolchains -s dlang
docker run --rm -i --entrypoint c++filt multi-demangle/gnu-toolchains -s gnat
```

**Use GNU `c++filt`, not `llvm-cxxfilt`, for D.** LLVM's `DLangDemangle.cpp`
is the limited implementation that stops at basic types — it is what
`src/dlang.rs` was ported *from*, so diffing against it would mostly confirm
the port of a stub. libiberty's `d-demangle.c` is the complete one.

Two failure spellings matter when reading the diff. Both tools echo the input
unchanged when they cannot demangle, and `c++filt -s gnat` additionally wraps
its failures in angle brackets (`<corpus__workerTB>`). Counting either as a
successful demangle invents functional gaps that do not exist;
`collect-corpus.sh` accounts for both.

## Reading the output

The diff separates two categories, and they mean very different things:

- **Functional gaps** — we reject a symbol the reference demangles. These are
  defects. Drive them to zero.
- **Rendering differences** — both demangle, spelled differently. Most are
  deliberate. This crate renders `int delegate(int)` where libiberty renders
  `int(int) delegate`, and prints a variable's type where libiberty omits it;
  both are intentional. Read them, do not blindly converge on them.

Current baseline: **D 0 functional gaps**, 5 rendering differences — all
deliberate (the `int delegate(int)` / variable-type choices documented in
`src/dlang.rs`; the `Mx` const-qualifier gap this README used to flag is
closed, and `() const` now matches the reference). **Ada 0 rendering
differences** since the `'Elab_Body` handling landed; the task-companion
symbols (`workerTB`, `workerZ`, `valueIP`, ...) fail in `c++filt` too, so
their renderings follow `exp_dbug.ads` and are snapshot-tier in
`tests/corpus/`. **Ada 1 functional gap** — `b.2` → `b`, deliberately
rejected, since claiming a bare single letter as Ada would be a
false-positive disaster across every other language's symbols.

The committed corpora in `tests/corpus/` are covered by
`tests/test_new_language_corpus.rs`, which splits expectations into a
`c++filt`-verified **golden** tier and a **snapshot** tier for the
deliberate divergences — see that directory's README for the workflow. CI
additionally runs the `new-language-differential` ratchet, which fails when
the functional-gap counts above rise (D > 0, Ada > 1).

## The generator-driven differential

`diff` replays a *fixed* corpus; `diff-fuzz` closes the loop. It generates D
mangled names from the grammar in the [D ABI
spec](https://dlang.org/spec/abi.html) (`contrib/scripts/gen_dlang_symbols.py`
— from the spec, deliberately not from `src/dlang.rs`'s structure, so a wrong
belief shared by parser and generator cannot manufacture agreement), feeds
them to both demanglers, and classifies every disagreement with the same
failure-spelling rules as `diff`. Symbols the oracle itself rejects are
generator noise (~9% of draws) and are counted separately as "both reject",
so they cannot inflate either side's numbers.

The first run found 7,128 functional gaps per 50,000 symbols — a missing
call kind, combined parameter markers, anonymous back references,
member-function-type parameters — all fixed; the generator now reports zero
functional gaps across seeds, and the fixed corpus baselines above are
unchanged. Crash-artifact and gap regression tests live in
`tests/test_dlang.rs` and `tests/test_gnuv2.rs`, each citing the input that
produced it.

## The Kotlin/Native scope answer

The `kotlin-native.Dockerfile` existed to settle one question before more
effort went into `src/kotlin_native.rs`: does a current compiler still emit
the readable `kfun:` spelling? It does — the 2.0.21 prebuilt compiler puts
~950 `kfun:` symbols in the fixture's object files — so the backend covers a
live format. The spelling changed, though: the dotted
`kfun:com.example.Foo.bar(kotlin.String)` form from the 2018 issue the
backend was originally written from is not what modern compilers emit. The
real grammar (`kfun:<pkg>#<member>(<params>){<bounds>}<ret>`, `#static` /
`#internal` / `-trampoline` markers) is documented in the module, and the
parser now handles it; the corpus in `tests/corpus/kotlin_symbols.txt` pins
it against the compiler.

## Why images rather than a setup script

`scripts/collect-corpus.sh` harvests Rust/C++/Swift symbols from binaries
that happen to be on the machine. The languages here have no such binaries
lying around, and installing four toolchains on a developer laptop to run a
test suite is not a reasonable ask. Pinning them in images also makes the
corpus reproducible: the symbol spelling is a compiler implementation
detail, so "which gfortran" is part of the test data's meaning, which is why
`collect` writes a provenance file alongside the symbols.
