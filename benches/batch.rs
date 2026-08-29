//! Criterion benchmarks for the batch demangling pipeline.
//!
//! Two scenario families, both measured against the naive per-symbol loop
//! (`demangle_one` in a map) that consumers used before the batch API:
//!
//! - `synthetic_*`: 100k-symbol mixes with realistic language ratios for a
//!   Rust binary's and a Swift app's symbol table, including the ~3×
//!   duplication a combined dynsym + symtab + PLT/GOT view produces.
//! - `corpus_*`: real symbol dumps from `tests/corpus/` (see
//!   `scripts/collect-corpus.sh` for provenance), benchmarked both as the
//!   deduplicated dump and as a duplicated table-like view.

use std::fs;
use std::path::Path;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use multi_demangle::{demangle_iter, demangle_one, DemangleOptions};

/// How often the same symbol shows up when a binary is viewed the way
/// consumers see it (dynsym, symtab, version tables, and GOT/PLT maps
/// merged), per the batch-API motivation.
const TABLE_DUPLICATION: usize = 3;

/// Size of the synthetic mixes, in symbols.
const SYNTHETIC_SIZE: usize = 100_000;

/// Reads a corpus file (one symbol per line, `#` comments allowed). Missing
/// files skip their benchmarks instead of failing the run.
fn load_corpus(name: &str) -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join(name);
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(_) => {
            eprintln!("corpus file {} not found; skipping", path.display());
            return Vec::new();
        }
    };
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(String::from)
        .collect()
}

/// A unique legacy-Rust symbol (`_ZN5bench8process42E` style) per index.
fn rust_legacy_unique(i: usize) -> String {
    let function = format!("process{i}");
    format!("_ZN5bench{}17h{i:016x}E", function.len())
}

/// A unique v0-Rust symbol per index.
fn rust_v0_unique(i: usize) -> String {
    let function = format!("process{i}");
    format!("_RNvNtCs1234_7mycrate{}{function}", function.len())
}

/// A unique Itanium C++ symbol per index.
fn cpp_unique(i: usize) -> String {
    let function = format!("process{i}");
    format!("_ZN5bench{}{function}Ev", function.len())
}

/// A unique MSVC C++ symbol per index.
fn msvc_unique(i: usize) -> String {
    format!("?process{i}@bench@@YAXH@Z")
}

/// A unique Swift symbol per index.
fn swift_unique(i: usize) -> String {
    let function = format!("process{i}");
    format!("$s5bench{}{function}yyF", function.len())
}

/// A unique plain C symbol per index.
fn unmangled_unique(i: usize) -> String {
    format!("local_static_{i}")
}

/// Hand-picked, verified-demanglable samples per language. Small pools are
/// fine: they make those languages repeat a little more inside the mix.
const CPP_ITANIUM_SAMPLES: &[&str] = &[
    "_ZN3foo3barEv",
    "_ZNK4llvm5Value7getNameEv",
    "_ZNSt3__16vectorIiNS_9allocatorIiEEE9push_backERS2_",
    "_ZNSt6vectorIiSaIiEE9push_backERKi",
    "_ZN9wasm::Pass3runEPNS_6ModuleE",
    "_ZN2cv3Mat8releasedEv",
    "_ZN12fmt::v106detail8vformatERVNS0_17basic_format_argsINS0_21basic_format_contextINS0_8appenderEcEEEE",
    "_ZN3gtl9raw_hash_setINS_17FlatHashMapPolicyIiSt4pairIKiiEEENS_4HashIiEENS_8sharableEE8rehashAndInsertIfEmptyEPKi",
];

const MSVC_SAMPLES: &[&str] = &[
    "?h@@YAXH@Z",
    "?foo@bar@@YAXXZ",
    "?memcpy@std@@YAPEAXPEAX0_K@Z",
    "??0Fixture@Test@@QEAA@XZ",
    "?read_some@ssl_stream@detail@asio@boost@@QEAAAIPEADPEADI@Z",
];

const SWIFT_SAMPLES: &[&str] = &[
    "$s8mangling12GenericUnionO3FooyACyxGSicAEmlF",
    "$s7example1fyyYaF",
    "$s4main20receiveInstantiationyySo34__CxxTemplateInst12MagicWrapperIiEVzF",
    "$s4diff1hyyS2iYjlXEF",
    "$s17distributed_thunk2DAC1fyyFTE",
    "$s8mangling14varargsVsArray3arr1nySid_SStF",
    "$S8mangling12any_protocolyyypF",
    "$s8mangling6curry1yyF",
];

const OBJC_SAMPLES: &[&str] = &[
    "-[NSApplication delegate]",
    "+[NSString stringWithUTF8String:]",
    "-[UIViewController viewDidLoad]",
    "-[NSUserDefaults objectForKey:]",
];

const UNMANGLED_SAMPLES: &[&str] = &[
    "main",
    "malloc",
    "free",
    "memcpy",
    "printf",
    "pthread_create",
    "getaddrinfo",
    "lua_pushstring",
    "SDL_OpenAudioDevice",
    "krb5_mk_req_extended",
    "zstd_decompress",
    "XML_Parse",
];

/// Replicates the symbol list `factor` times as interleaved passes, matching
/// a merged dynsym + symtab + PLT/GOT view where the same names recur
/// across table boundaries rather than in adjacent runs.
fn with_table_duplication(symbols: Vec<String>, factor: usize) -> Vec<String> {
    let mut out = Vec::with_capacity(symbols.len() * factor);
    for _ in 0..factor {
        out.extend(symbols.iter().cloned());
    }
    out
}

/// Builds a symbol-table-shaped mix with the given language ratios. Every
/// slice is filled with unique symbols per index, except the ObjC selectors,
/// which pass through demangling unchanged anyway.
fn synthetic_mix(
    rust: usize,
    v0: usize,
    cpp: usize,
    msvc: usize,
    swift: usize,
    objc: usize,
    c: usize,
) -> Vec<String> {
    let total = rust + v0 + cpp + msvc + swift + objc + c;
    let mut out: Vec<String> = Vec::with_capacity(total);
    let mut i = 0usize;
    for _ in 0..rust {
        out.push(rust_legacy_unique(i));
        i += 1;
    }
    for _ in 0..v0 {
        out.push(rust_v0_unique(i));
        i += 1;
    }
    // Generated uniques are interleaved with a small share of complex
    // hand-picked samples (templates, generics, overloads) so the mix also
    // exercises the demanglers' heavy paths.
    for _ in 0..cpp {
        if i.is_multiple_of(8) {
            out.push(CPP_ITANIUM_SAMPLES[(i / 8) % CPP_ITANIUM_SAMPLES.len()].to_string());
        } else {
            out.push(cpp_unique(i));
        }
        i += 1;
    }
    for _ in 0..msvc {
        if i.is_multiple_of(4) {
            out.push(MSVC_SAMPLES[(i / 4) % MSVC_SAMPLES.len()].to_string());
        } else {
            out.push(msvc_unique(i));
        }
        i += 1;
    }
    for _ in 0..swift {
        if i.is_multiple_of(32) {
            out.push(SWIFT_SAMPLES[(i / 32) % SWIFT_SAMPLES.len()].to_string());
        } else {
            out.push(swift_unique(i));
        }
        i += 1;
    }
    // Plain C: generated unique names interleaved with the familiar
    // shared-library imports, which repeat heavily in real tables.
    for _ in 0..c {
        if i.is_multiple_of(4) {
            out.push(UNMANGLED_SAMPLES[(i / 4) % UNMANGLED_SAMPLES.len()].to_string());
        } else {
            out.push(unmangled_unique(i));
        }
        i += 1;
    }
    for _ in 0..objc {
        out.push(OBJC_SAMPLES[i % OBJC_SAMPLES.len()].to_string());
        i += 1;
    }
    assert_eq!(out.len(), total);
    out
}

/// A Rust binary's symbol table: mostly Rust, a quarter plain C, a few C++
/// and MSVC stragglers from bundled native code.
fn synthetic_rust_table() -> Vec<String> {
    let uniques = SYNTHETIC_SIZE / TABLE_DUPLICATION;
    synthetic_mix(
        uniques * 50 / 100,
        uniques * 20 / 100,
        uniques * 4 / 100,
        uniques / 100,
        0,
        0,
        uniques * 25 / 100,
    )
}

/// A Swift app's symbol table: mostly Swift, some C, ObjC selectors, a few
/// C++ templates.
fn synthetic_swift_table() -> Vec<String> {
    let uniques = SYNTHETIC_SIZE / TABLE_DUPLICATION;
    synthetic_mix(
        0,
        0,
        uniques * 4 / 100,
        uniques / 100,
        uniques * 70 / 100,
        uniques * 15 / 100,
        uniques * 10 / 100,
    )
}

fn bench_batch(c: &mut Criterion) {
    let opts = DemangleOptions::name_only();

    let corpus_rust = load_corpus("rust_symbols.txt");
    let corpus_cpp = load_corpus("cpp_symbols.txt");
    let corpus_swift = load_corpus("swift_symbols.txt");

    let mut scenarios: Vec<(String, Vec<String>)> = vec![
        (
            "synthetic_rust_table".to_string(),
            with_table_duplication(synthetic_rust_table(), TABLE_DUPLICATION),
        ),
        (
            "synthetic_swift_table".to_string(),
            with_table_duplication(synthetic_swift_table(), TABLE_DUPLICATION),
        ),
    ];
    // Real dumps, unique-heavy (as `nm` reports them).
    if !corpus_rust.is_empty() {
        scenarios.push(("corpus_rust_unique".to_string(), corpus_rust.clone()));
    }
    if !corpus_swift.is_empty() {
        scenarios.push(("corpus_swift_unique".to_string(), corpus_swift.clone()));
    }
    // The mixed real-world corpus, as a table-like view with duplicates.
    if !(corpus_rust.is_empty() && corpus_cpp.is_empty() && corpus_swift.is_empty()) {
        let mut mixed = corpus_rust;
        mixed.extend(corpus_cpp);
        mixed.extend(corpus_swift);
        scenarios.push((
            "corpus_mixed_table".to_string(),
            with_table_duplication(mixed, TABLE_DUPLICATION),
        ));
    }

    let mut group = c.benchmark_group("batch_demangle");
    for (name, symbols) in &scenarios {
        let refs: Vec<&str> = symbols.iter().map(String::as_str).collect();
        group.throughput(Throughput::Elements(refs.len() as u64));

        group.bench_with_input(BenchmarkId::new("naive_loop", name), &refs, |b, symbols| {
            b.iter(|| {
                symbols
                    .iter()
                    .map(|&sym| demangle_one(sym, opts))
                    .collect::<Vec<_>>()
            })
        });

        group.bench_with_input(
            BenchmarkId::new("demangle_iter", name),
            &refs,
            |b, symbols| {
                b.iter(|| demangle_iter(symbols.iter().copied(), opts))
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_batch);
criterion_main!(benches);
