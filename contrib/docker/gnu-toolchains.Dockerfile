# Toolchains for collecting real Fortran, Ada, and D symbols.
#
# Three of the four Plan 07 backends are covered by Debian packages, so they
# share one image: it builds on both arm64 and amd64 and needs no downloads
# beyond apt. Kotlin/Native is the exception (see kotlin-native.Dockerfile).
#
# The image also carries binutils, which matters as much as the compilers:
# GNU `c++filt` embeds libiberty's `d-demangle.c`, the mature, independent D
# demangler this crate's backend can be differentially tested against. (Use
# GNU c++filt, not llvm-cxxfilt — LLVM's D demangler is the limited one that
# stops at basic types.)
#
# Build:  docker build -f contrib/docker/gnu-toolchains.Dockerfile -t multi-demangle/gnu-toolchains contrib
# Usage:  see contrib/collect-corpus.sh, which drives this image.
FROM debian:bookworm-slim

# gfortran — gfortran module mangling (__mod_MOD_proc)
# gnat     — GNAT/Ada mangling (pkg__child__proc)
# ldc / gdc— the two D compilers; LDC and GDC agree on the ABI mangling but
#            differ in which symbols they emit, so collecting from both widens
#            the corpus
# binutils — nm (symbol extraction) and c++filt (the D differential oracle)
RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        gfortran \
        gnat \
        ldc \
        gdc \
        binutils \
        make \
    && rm -rf /var/lib/apt/lists/*

# Record the exact toolchain versions in the image. The collection script
# copies this into the corpus provenance file: for Fortran and Ada the symbol
# spelling is a compiler implementation detail, so "which gfortran" is part of
# the test data's meaning.
RUN { \
      echo "image: multi-demangle/gnu-toolchains"; \
      echo "base: debian:bookworm-slim"; \
      echo "arch: $(dpkg --print-architecture)"; \
      echo "gfortran: $(gfortran --version | head -1)"; \
      echo "gnatmake: $(gnatmake --version | head -1)"; \
      echo "ldc2: $(ldc2 --version | head -1)"; \
      echo "gdc: $(gdc --version | head -1)"; \
      echo "binutils: $(nm --version | head -1)"; \
    } > /toolchain-versions.txt

WORKDIR /work
COPY fixtures /fixtures
COPY scripts/emit-symbols.sh /usr/local/bin/emit-symbols
RUN chmod +x /usr/local/bin/emit-symbols

# Default: compile every fixture and print the resulting symbols to stdout.
ENTRYPOINT ["/usr/local/bin/emit-symbols"]
