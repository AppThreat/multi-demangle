//! Integration tests driving the `multi-demangle` binary end to end:
//! argument mode, stdin filter mode over `nm` fixtures, the `--normalize`
//! hygiene passes, structured JSON output, and the informational flags.

#![cfg(feature = "cli")]

use std::io::Write;
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_multi-demangle");

struct Output {
    stdout: String,
    status: i32,
}

fn run_bytes(args: &[&str], stdin: &[u8]) -> Output {
    let mut child = Command::new(BIN)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary spawns");
    // Stdin is written from a thread so the parent can drain stdout while
    // the child is still reading; large inputs would otherwise deadlock on
    // the pipe buffers.
    let mut pipe = child.stdin.take().expect("stdin piped");
    let input = stdin.to_vec();
    let writer = std::thread::spawn(move || {
        pipe.write_all(&input).expect("stdin write");
    });
    let output = child.wait_with_output().expect("wait");
    writer.join().expect("stdin writer");
    Output {
        stdout: String::from_utf8(output.stdout).expect("stdout utf-8"),
        status: output.status.code().expect("exit code"),
    }
}

fn run(args: &[&str], stdin: Option<&str>) -> Output {
    run_bytes(args, stdin.map(str::as_bytes).unwrap_or(b""))
}

#[test]
fn arg_mode_demangles_each_argument() {
    let out = run(&["_ZN3foo3barEv", "libc.so.6", "_Z1hic"], None);
    assert_eq!(out.status, 0);
    assert_eq!(out.stdout, "foo::bar()\nlibc.so.6\nh(int, char)\n");
}

#[test]
fn name_only_flag() {
    let out = run(&["-n", "_Z1hic"], None);
    assert_eq!(out.stdout, "h\n");
}

#[test]
fn scala_native_output_flags() {
    let symbol = "_SM17java.lang.IntegerD7compareiiiEo";
    let out = run(&[symbol], None);
    assert_eq!(out.stdout, "java.lang.Integer.compare(Int,Int): Int\n");
    let out = run(&["--no-return-type", symbol], None);
    assert_eq!(out.stdout, "java.lang.Integer.compare(Int,Int)\n");
    let out = run(&["-n", symbol], None);
    assert_eq!(out.stdout, "java.lang.Integer.compare\n");
}

#[test]
fn forced_language() {
    let out = run(
        &["-l", "scala-native", "_SM17java.lang.IntegerD7compareiiiEo"],
        None,
    );
    assert_eq!(out.stdout, "java.lang.Integer.compare(Int,Int): Int\n");

    // Forcing a language the symbol does not use passes it through verbatim.
    let out = run(&["-l", "swift", "_ZN3foo3barEv"], None);
    assert_eq!(out.stdout, "_ZN3foo3barEv\n");
}

#[test]
fn filter_mode_nm_fixture() {
    let input = include_str!("fixtures/nm_dump.txt");
    let expected = include_str!("fixtures/nm_demangled.txt");
    let out = run(&[], Some(input));
    assert_eq!(out.status, 0);
    assert_eq!(out.stdout, expected);
}

#[test]
fn filter_mode_normalize_fixture() {
    let input = include_str!("fixtures/nm_hygiene_dump.txt");
    let expected = include_str!("fixtures/nm_hygiene_demangled.txt");
    let out = run(&["--normalize"], Some(input));
    assert_eq!(out.status, 0);
    assert_eq!(out.stdout, expected);
}

#[test]
fn filter_mode_preserves_whitespace() {
    let out = run(&[], Some("a  _ZN3foo3barEv\tb\n"));
    assert_eq!(out.stdout, "a  foo::bar()\tb\n");
}

#[test]
fn filter_mode_replaces_invalid_utf8() {
    let out = run_bytes(&[], b"_ZN3foo3barEv \xff\xfe text\n");
    assert_eq!(out.status, 0);
    assert_eq!(out.stdout, "foo::bar() \u{FFFD}\u{FFFD} text\n");
}

#[test]
fn empty_stdin() {
    let out = run(&[], Some(""));
    assert_eq!(out.status, 0);
    assert_eq!(out.stdout, "");
}

#[test]
fn normalize_argument_mode() {
    let out = run(
        &[
            "--normalize",
            "memcpy@plt",
            "__imp_CreateFileW",
            "_Z1hic@GLIBC_2.2.5",
            "bar.llvm.12345",
        ],
        None,
    );
    assert_eq!(out.status, 0);
    // The version-stripped C++ symbol is demangled after cleanup.
    assert_eq!(out.stdout, "memcpy\nCreateFileW\nh(int, char)\nbar\n");

    // Without --normalize the decorated symbols pass through.
    let out = run(&["memcpy@plt", "_Z1hic@GLIBC_2.2.5"], None);
    assert_eq!(out.stdout, "memcpy@plt\n_Z1hic@GLIBC_2.2.5\n");
}

#[test]
fn normalize_filter_mode_cleans_unmangled_tokens() {
    // `.llvm.` clone suffixes and legacy Rust `$`-escapes classify as
    // unmangled, so the candidate gate must not hide them from --normalize.
    let out = run(&["--normalize"], Some("bar.llvm.12345 foo$LT$\n"));
    assert_eq!(out.status, 0);
    assert_eq!(out.stdout, "bar foo<\n");

    // Without --normalize they pass through untouched.
    let out = run(&[], Some("bar.llvm.12345 foo$LT$\n"));
    assert_eq!(out.stdout, "bar.llvm.12345 foo$LT$\n");
}

#[test]
fn normalize_structured_filter_mode_records_cleaned_tokens() {
    // The address and the nm type letter are unmangled and unchanged, so
    // they stay out of the output; the cleaned token is reported.
    let out = run(&["-s", "--normalize"], Some("0000 T bar.llvm.12345\n"));
    assert_eq!(out.status, 0);
    assert_eq!(out.stdout.lines().count(), 1, "{}", out.stdout);
    assert!(
        out.stdout.contains(r#""mangled":"bar.llvm.12345""#),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains(r#""demangled":"bar""#),
        "{}",
        out.stdout
    );
}

#[test]
fn objc_selector_argument() {
    // Hyphen-prefixed selectors are accepted as values, not flags.
    let out = run(&["-[Foo bar:blub:]"], None);
    assert_eq!(out.status, 0);
    // Selectors are already readable and pass through unchanged.
    assert_eq!(out.stdout, "-[Foo bar:blub:]\n");

    // Structured mode classifies them as mangled ObjC.
    let out = run(&["-s", "-[Foo bar:blub:]"], None);
    assert_eq!(out.status, 0);
    assert!(
        out.stdout.contains(r#""status":"mangled""#),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains(r#""language":"objc""#),
        "{}",
        out.stdout
    );
}

#[test]
fn structured_argument_mode() {
    let out = run(&["-s", "_ZN3foo3barEv"], None);
    assert_eq!(out.status, 0);
    assert!(
        out.stdout.contains(r#""mangled":"_ZN3foo3barEv""#),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains(r#""demangled":"foo::bar()""#),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains(r#""status":"mangled""#),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains(r#""language":"cpp""#), "{}", out.stdout);
    assert!(out.stdout.contains(r#""decorations":[]"#), "{}", out.stdout);
}

#[test]
fn structured_argument_mode_reports_decorations() {
    let out = run(&["-s", "_Z1hic@GLIBC_2.2.5"], None);
    assert!(
        out.stdout
            .contains(r#""kind":"version","value":"GLIBC_2.2.5""#),
        "{}",
        out.stdout
    );
}

#[test]
fn structured_filter_mode_skips_plain_tokens() {
    let input = "0000000000400528 T _ZN3foo3barEv\nplain words here\n";
    let out = run(&["-s"], Some(input));
    assert_eq!(out.status, 0);
    assert_eq!(out.stdout.lines().count(), 1, "{}", out.stdout);
    assert!(
        out.stdout.contains(r#""mangled":"_ZN3foo3barEv""#),
        "{}",
        out.stdout
    );
}

#[test]
fn structured_output_is_never_colorized() {
    let out = run(&["-s", "--color=always", "_ZN3foo3barEv"], None);
    assert_eq!(out.status, 0);
    assert!(!out.stdout.contains('\x1b'), "{}", out.stdout);
}

#[test]
fn list_languages_prints_languages() {
    let out = run(&["--list-languages"], None);
    assert_eq!(out.status, 0);
    for language in ["cpp", "rust", "swift", "objc", "objcpp", "scala-native"] {
        assert!(
            out.stdout.contains(language),
            "missing {language}: {}",
            out.stdout
        );
    }
}

#[test]
fn version_includes_backends() {
    let out = run(&["--version"], None);
    assert_eq!(out.status, 0);
    assert!(
        out.stdout.contains(env!("CARGO_PKG_VERSION")),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("enabled backends:"), "{}", out.stdout);
}

#[test]
fn color_flag() {
    let out = run(&["--color=always", "_ZN3foo3barEv"], None);
    assert_eq!(out.stdout, "\x1b[1mfoo::bar()\x1b[0m\n");
    let out = run(&["--color=never", "_ZN3foo3barEv"], None);
    assert_eq!(out.stdout, "foo::bar()\n");
    // Unmangled output is never colorized.
    let out = run(&["--color=always", "libc.so.6"], None);
    assert_eq!(out.stdout, "libc.so.6\n");
}

#[test]
fn swift_symbol_with_dollar_argument() {
    // `$` in argv stresses console quoting on the Windows CI runners.
    let out = run(&["$s8mangling6curry1yyF"], None);
    assert_eq!(out.status, 0);
    assert_eq!(out.stdout, "mangling.curry1() -> ()\n");
}

#[test]
fn corpus_filter_mode_preserves_lines() {
    for corpus in [
        "tests/corpus/cpp_symbols.txt",
        "tests/corpus/rust_symbols.txt",
        "tests/corpus/swift_symbols.txt",
    ] {
        let input = std::fs::read_to_string(corpus).expect("corpus file exists");
        let out = run(&[], Some(&input));
        assert_eq!(out.status, 0, "{corpus}");
        assert_eq!(
            out.stdout.lines().count(),
            input.lines().count(),
            "{corpus}"
        );
    }
}

#[test]
fn corpus_cpp_symbol_demangles_via_cli() {
    let input = std::fs::read_to_string("tests/corpus/cpp_symbols.txt").unwrap();
    let first = input.lines().next().unwrap();
    let out = run(&[first], None);
    assert_eq!(out.status, 0);
    assert!(
        out.stdout.contains("DIEVisitor::visitDIERef"),
        "unexpected: {}",
        out.stdout
    );
}
