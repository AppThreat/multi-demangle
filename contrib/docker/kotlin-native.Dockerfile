# Kotlin/Native toolchain for collecting real `kfun:` symbols.
#
# Separate from gnu-toolchains.Dockerfile because Kotlin/Native is not an apt
# package: it is a ~1 GB tarball from GitHub releases, published for
# linux-x86_64 only. On an arm64 host this image therefore runs under
# emulation — pass `--platform linux/amd64` when building (the FROM line
# pins it, so Docker will do the right thing automatically).
#
# BEFORE INVESTING IN THIS IMAGE, answer the scope question it exists to
# settle: modern Kotlin/Native does not necessarily emit the readable
# `kfun:com.example.Foo.bar(kotlin.String)` spelling that src/kotlin_native.rs
# parses. Newer compilers hash much of the symbol table. Build this, compile
# the fixture, and look at what `nm` actually reports before treating the
# backend's coverage as adequate. KOTLIN_VERSION is deliberately overridable
# so several generations can be compared.
#
# Build:  docker build -f contrib/docker/kotlin-native.Dockerfile \
#             --build-arg KOTLIN_VERSION=2.0.21 \
#             -t multi-demangle/kotlin-native contrib
FROM --platform=linux/amd64 debian:bookworm-slim

ARG KOTLIN_VERSION=2.0.21

RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        curl \
        ca-certificates \
        binutils \
        libncurses6 \
        zlib1g \
        openjdk-17-jre-headless \
    && rm -rf /var/lib/apt/lists/*

# The Kotlin/Native compiler ships as a tarball but is not self-contained:
# `kotlinc-native` is a JVM application (hence the JRE above), and its first
# invocation downloads a platform dependency bundle (its own LLVM and sysroot,
# several hundred MB). The throwaway compile below runs that download once at
# build time so container runs do not pay for it again.
RUN curl -fsSL -o /tmp/kn.tar.gz \
        "https://github.com/JetBrains/kotlin/releases/download/v${KOTLIN_VERSION}/kotlin-native-prebuilt-linux-x86_64-${KOTLIN_VERSION}.tar.gz" \
    && mkdir -p /opt/kotlin-native \
    && tar -xzf /tmp/kn.tar.gz -C /opt/kotlin-native --strip-components=1 \
    && rm /tmp/kn.tar.gz
ENV PATH="/opt/kotlin-native/bin:${PATH}"

# Bake the platform dependency bundle into the image with a minimal compile.
RUN mkdir -p /tmp/bake && cd /tmp/bake \
    && printf 'fun main() { println("ok") }\n' > bake.kt \
    && kotlinc-native -produce static -o bake ./bake.kt >/dev/null 2>&1 \
    && rm -rf /tmp/bake

RUN { \
      echo "image: multi-demangle/kotlin-native"; \
      echo "base: debian:bookworm-slim (linux/amd64)"; \
      echo "kotlin-native: ${KOTLIN_VERSION}"; \
      echo "binutils: $(nm --version | head -1)"; \
    } > /toolchain-versions.txt

WORKDIR /work
COPY fixtures/kotlin /fixtures/kotlin
COPY scripts/emit-kotlin-symbols.sh /usr/local/bin/emit-symbols
RUN chmod +x /usr/local/bin/emit-symbols

ENTRYPOINT ["/usr/local/bin/emit-symbols"]
