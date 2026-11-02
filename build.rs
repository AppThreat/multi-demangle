fn main() {
    #[cfg(feature = "swift")]
    {
        // Keep this list aligned with the minimal vendored Swift subset in `vendor/swift`.
        // `CrashReporter.cpp` is intentionally omitted because
        // `SWIFT_RUNTIME_NO_CRASH_REPORTER` disables that implementation path.
        let files = &[
            "src/swiftdemangle.cpp",
            "vendor/swift/lib/Demangling/Context.cpp",
            "vendor/swift/lib/Demangling/Demangler.cpp",
            "vendor/swift/lib/Demangling/ManglingUtils.cpp",
            "vendor/swift/lib/Demangling/NodeDumper.cpp",
            "vendor/swift/lib/Demangling/NodePrinter.cpp",
            "vendor/swift/lib/Demangling/Punycode.cpp",
            "vendor/swift/lib/Demangling/Remangler.cpp",
            "vendor/swift/lib/Demangling/DemanglingErrorHandling.cpp",
        ];

        let mut build = cc::Build::new();
        build
            .cpp(true)
            .std("c++17")
            .files(files)
            .include("vendor/swift/include")
            .define("SWIFT_STDLIB_HAS_TYPE_PRINTING", "1")
            .define("SWIFT_SUPPORTS_CONCURRENCY", "1")
            .define("LLVM_DISABLE_ABI_BREAKING_CHECKS_ENFORCING", "1")
            .define("SWIFT_RUNTIME_NO_CRASH_REPORTER", "1")
            .flag_if_supported("-Wno-unused-parameter")
            .flag_if_supported("-Wno-deprecated-declarations");
        if std::env::var("TARGET")
            .map(|target| target.contains("windows-msvc"))
            .unwrap_or(false)
        {
            // MSVC cannot compile the vendored Swift demangler headers (`llvm`
            // is ambiguous with the vendored `__swift::__runtime::llvm`
            // namespace), so this shim needs clang. Pinning it here keeps the
            // choice local to the shim. Setting CC globally instead leaks into
            // unrelated C build scripts, where it breaks them: the alloca
            // crate compiles with -flto under clang, and MSVC's lib.exe
            // cannot archive the bitcode object (LNK1107).
            build.compiler("clang");
        }
        build.compile("swiftdemangle");

        for file in files {
            println!("cargo:rerun-if-changed={}", file);
        }
    }
}
