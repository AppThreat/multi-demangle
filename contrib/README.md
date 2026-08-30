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

Current baseline: **D 0 functional gaps**, 7 rendering differences (of which
the missing `const` suffix on `Mx` member functions is a real gap worth
closing). **Ada 1 functional gap** — `b.2` → `b`, deliberately rejected,
since claiming a bare single letter as Ada would be a false-positive
disaster across every other language's symbols.

## Why images rather than a setup script

`scripts/collect-corpus.sh` harvests Rust/C++/Swift symbols from binaries
that happen to be on the machine. The languages here have no such binaries
lying around, and installing four toolchains on a developer laptop to run a
test suite is not a reasonable ask. Pinning them in images also makes the
corpus reproducible: the symbol spelling is a compiler implementation
detail, so "which gfortran" is part of the test data's meaning, which is why
`collect` writes a provenance file alongside the symbols.
