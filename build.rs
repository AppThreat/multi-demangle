fn main() {
    #[cfg(feature = "swift")]
    {
        let files = &[
            "src/swiftdemangle.cpp",
            "vendor/swift/lib/Demangling/Context.cpp",
            "vendor/swift/lib/Demangling/Demangler.cpp",
            "vendor/swift/lib/Demangling/ManglingUtils.cpp",
            "vendor/swift/lib/Demangling/NodeDumper.cpp",
            "vendor/swift/lib/Demangling/NodePrinter.cpp",
            "vendor/swift/lib/Demangling/Punycode.cpp",
            "vendor/swift/lib/Demangling/Remangler.cpp",
            "vendor/swift/lib/Demangling/Errors.cpp",
            "vendor/swift/lib/Demangling/CrashReporter.cpp",
        ];

        cc::Build::new()
            .cpp(true)
            .std("c++17")
            .files(files)
            .include("vendor/swift/include")
            .define("SWIFT_STDLIB_HAS_TYPE_PRINTING", "1")
            .define("SWIFT_SUPPORTS_CONCURRENCY", "1")
            .define("LLVM_DISABLE_ABI_BREAKING_CHECKS_ENFORCING", "1")
            .define("SWIFT_RUNTIME_NO_CRASH_REPORTER", "1")
            .flag_if_supported("-Wno-unused-parameter")
            .flag_if_supported("-Wno-deprecated-declarations")
            .compile("swiftdemangle");

        for file in files {
            println!("cargo:rerun-if-changed={}", file);
        }
    }
}