//! The `multi-demangle` CLI: a cxxfilt-style filter over the library.
//!
//! With arguments, each argument is demangled to one output line. Without
//! arguments, the tool runs in filter mode: lines are read from stdin, every
//! whitespace-separated token that looks mangled is demangled, and everything
//! else (addresses, nm type letters, ordinary words) passes through unchanged.
//! This keeps it composable with `nm` / `objdump` pipelines:
//!
//! ```text
//! nm libfoo.so | multi-demangle
//! ```
//!
//! The tool is a thin shell over the library API: auto-detected languages,
//! `DemangleOptions`, the `Normalizer` hygiene passes, and the same
//! "unmangled input passes through verbatim" contract as `demangle_one`.

use std::borrow::Cow;
use std::io::{self, BufRead, BufWriter, IsTerminal, Write};
use std::process::ExitCode;
use std::sync::LazyLock;

use clap::{CommandFactory, FromArgMatches, Parser, ValueEnum};
use multi_demangle::{
    classify_symbol, language_name, Decoration, Demangle, DemangleOptions, DemangledInfo,
    Normalizer, SymbolStatus,
};
use symbolic_common::{Language, Name, NameMangling};

/// Pads the language column of `--list-languages` output.
const LANGUAGE_COLUMN_WIDTH: usize = 14;

#[derive(Parser, Debug)]
#[command(
    name = "multi-demangle",
    about = "Demangle C++, Rust, Swift, ObjC, and Scala Native symbols"
)]
struct Cli {
    /// Mangled symbols to demangle, one result per line. When omitted,
    /// filter mode reads whitespace-separated tokens from stdin and passes
    /// everything that does not look mangled through unchanged.
    // Hyphen-prefixed symbols such as ObjC selectors (`-[Foo bar:]`) are
    // values, not flags.
    #[arg(allow_hyphen_values = true)]
    symbols: Vec<String>,

    /// Print names only, without parameters or return types.
    #[arg(short = 'n', long)]
    name_only: bool,

    /// Leave out function parameter types.
    #[arg(long, conflicts_with = "name_only")]
    no_parameters: bool,

    /// Leave out the function return type.
    #[arg(long, conflicts_with = "name_only")]
    no_return_type: bool,

    /// Force this language backend instead of auto-detecting.
    #[arg(short = 'l', long, value_enum)]
    language: Option<LanguageArg>,

    /// Apply the symbol hygiene passes (Rust hash suffix and legacy escape
    /// cleanup, `__imp_` import pointers, `@plt` call stubs, ELF versions,
    /// pseudo-symbols) to symbols that cannot be demangled, and demangle the
    /// cleaned symbol when it then succeeds. In filter mode every token is
    /// processed, not only the ones that look mangled.
    #[arg(long)]
    normalize: bool,

    /// Print one JSON record per symbol with its demangled form, status,
    /// language, and linker decorations. In filter mode, records are
    /// produced only for tokens that look like symbols or that the pipeline
    /// changed.
    #[arg(short = 's', long)]
    structured: bool,

    /// List the supported languages and the backends enabled in this build.
    #[arg(long)]
    list_languages: bool,

    /// When to colorize successfully demangled output.
    #[arg(long, value_enum, default_value_t = ColorWhen::Auto)]
    color: ColorWhen,
}

/// The `--language` choices; the value names are the kebab-case variants.
#[derive(Clone, Copy, Debug, ValueEnum)]
enum LanguageArg {
    /// C++ (Itanium, GNU v2, CodeWarrior, and MSVC mangling).
    Cpp,
    /// Rust (legacy and v0 mangling).
    Rust,
    /// Swift.
    Swift,
    /// Objective-C selectors (returned unchanged).
    Objc,
    /// Objective-C++ (ObjC selectors plus the C++ backends).
    Objcpp,
    /// Scala Native (`_SM`-prefixed symbols).
    ScalaNative,
}

impl LanguageArg {
    /// The [`Language`] to pin a `Name` to. Scala Native has no `Language`
    /// variant; `Language::Unknown` plus an explicit mangled marker routes
    /// straight to the Scala Native fallback.
    fn language(self) -> Language {
        match self {
            LanguageArg::Cpp => Language::Cpp,
            LanguageArg::Rust => Language::Rust,
            LanguageArg::Swift => Language::Swift,
            LanguageArg::Objc => Language::ObjC,
            LanguageArg::Objcpp => Language::ObjCpp,
            LanguageArg::ScalaNative => Language::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ColorWhen {
    /// Colorize only when stdout is a terminal (and `NO_COLOR` is unset).
    Auto,
    /// Always colorize.
    Always,
    /// Never colorize.
    Never,
}

/// The demangling settings shared by both input modes.
struct Pipeline {
    opts: DemangleOptions,
    language: Option<Language>,
    normalizer: Option<Normalizer>,
}

impl Pipeline {
    /// Demangles one token. A demangling that changes the input always wins;
    /// everything else (failed demangling, and identity results — the Scala
    /// Native fallback accepts already-readable strings unchanged) goes to
    /// the normalize fallback.
    fn run<'a>(&self, sym: &'a str) -> Cow<'a, str> {
        match self.demangle_owned(sym) {
            Some(demangled) if demangled != sym => Cow::Owned(demangled),
            _ => self.normalize_fallback(sym),
        }
    }

    /// The `--normalize` fallback for symbols that were not really
    /// demangled: apply the hygiene passes, then attempt demangling the
    /// cleaned symbol once more — a version-suffixed mangled symbol such as
    /// `_Z1hic@GLIBC_2.2.5` only demangles after its decoration is stripped.
    /// Without a normalizer the token passes through unchanged.
    fn normalize_fallback<'a>(&self, sym: &'a str) -> Cow<'a, str> {
        let Some(normalizer) = &self.normalizer else {
            return Cow::Borrowed(sym);
        };
        let cleaned = normalizer.normalize(sym);
        if cleaned == sym {
            return cleaned;
        }
        match self.demangle_owned(&cleaned) {
            Some(demangled) if demangled != cleaned => Cow::Owned(demangled),
            _ => cleaned,
        }
    }

    /// Demangles with auto-detection, or with the `--language` backend
    /// forced. The `Demangle::try_demangle` methods borrow the `Name`, so
    /// the owned `demangle` is used here.
    fn demangle_owned(&self, sym: &str) -> Option<String> {
        match self.language {
            Some(language) => Name::new(sym, NameMangling::Mangled, language).demangle(self.opts),
            None => Name::from(sym).demangle(self.opts),
        }
    }

    /// The structured view of a symbol, honoring a forced `--language`.
    fn structured(&self, sym: &str) -> Option<DemangledInfo> {
        let name = match self.language {
            Some(language) => Name::new(sym, NameMangling::Mangled, language),
            None => Name::from(sym),
        };
        name.demangle_structured(self.opts)
    }
}

/// Output rendering shared by both input modes.
struct Renderer {
    pipeline: Pipeline,
    color: bool,
}

impl Renderer {
    /// One output line for a single token or argument. Successfully
    /// demangled tokens (those whose output differs from the input) are
    /// wrapped in bold when color is on.
    fn render(&self, sym: &str) -> String {
        let demangled = self.pipeline.run(sym);
        if self.color && demangled != sym {
            format!("\x1b[1m{demangled}\x1b[0m")
        } else {
            demangled.into_owned()
        }
    }

    /// One JSON record for a token together with its precomputed
    /// classification and structured fields.
    fn record(&self, sym: &str, status: &SymbolStatus) -> String {
        let info = self.pipeline.structured(sym);
        structured_record(sym, &self.pipeline.run(sym), status, info.as_ref())
    }
}

/// Cheap gate for filter mode: whether a token is worth a demangling pass.
/// Prefix- and pattern-based, mirroring the library's classification; tokens
/// it rejects always render as themselves anyway.
fn is_symbol_candidate(token: &str) -> bool {
    !matches!(classify_symbol(token), SymbolStatus::Unmangled)
}

/// Reads lines from stdin, demangling each whitespace-separated token while
/// preserving the surrounding whitespace verbatim. Invalid UTF-8 bytes are
/// replaced, mirroring what a symbol table can contain.
fn run_filter(renderer: &Renderer, structured: bool) -> io::Result<()> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let mut buf = Vec::new();
    loop {
        buf.clear();
        if reader.read_until(b'\n', &mut buf)? == 0 {
            return out.flush();
        }
        let line = String::from_utf8_lossy(&buf);
        let line = line.strip_suffix('\n').unwrap_or(&line);

        if structured {
            for token in line.split_whitespace() {
                let status = classify_symbol(token);
                let result = renderer.pipeline.run(token);
                // A token that is not a symbol and that the pipeline left
                // unchanged — a plain address, an nm type letter — stays out
                // of the output; a token the passes cleaned (or demangled)
                // is reported even though it classifies as unmangled.
                if matches!(status, SymbolStatus::Unmangled) && result == token {
                    continue;
                }
                let info = renderer.pipeline.structured(token);
                writeln!(
                    out,
                    "{}",
                    structured_record(token, &result, &status, info.as_ref())
                )?;
            }
        } else {
            let mut rendered = String::with_capacity(line.len() + 32);
            // `split_inclusive` keeps each separator attached to the token
            // before it, so runs of whitespace survive the round-trip.
            for chunk in line.split_inclusive(char::is_whitespace) {
                let (token, whitespace) = chunk.split_at(chunk.trim_end().len());
                if renderer.pipeline.normalizer.is_some() || is_symbol_candidate(token) {
                    rendered.push_str(&renderer.render(token));
                } else {
                    rendered.push_str(token);
                }
                rendered.push_str(whitespace);
            }
            writeln!(out, "{rendered}")?;
        }
    }
}

/// Demangles each command line argument to one output line.
fn run_arguments(renderer: &Renderer, structured: bool, symbols: &[String]) -> io::Result<()> {
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    for sym in symbols {
        if structured {
            let status = classify_symbol(sym);
            writeln!(out, "{}", renderer.record(sym, &status))?;
        } else {
            writeln!(out, "{}", renderer.render(sym))?;
        }
    }
    out.flush()
}

/// One decoration as its `(kind, optional value)` JSON pair.
type DecorationEntry<'a> = (&'static str, Option<&'a str>);

/// Flattens a [`SymbolStatus`] into the status name, the innermost language
/// name, and the outermost-first decoration list.
fn describe<'a>(
    status: &'a SymbolStatus,
) -> (&'static str, Option<&'static str>, Vec<DecorationEntry<'a>>) {
    let mut decorations = Vec::new();
    let mut current = status;
    while let SymbolStatus::Decorated { decoration, inner } = current {
        let entry = match decoration {
            Decoration::ImportPointer => ("import-pointer", None),
            Decoration::CallStub => ("call-stub", None),
            Decoration::Version(version) => ("version", Some(version.as_str())),
            Decoration::LinkerHash => ("linker-hash", None),
            Decoration::ColdSection => ("cold-section", None),
            Decoration::SafeSeh => ("safe-seh", None),
            Decoration::Anonymous => ("anonymous", None),
            Decoration::ExceptTable => ("except-table", None),
            // `Decoration` is non-exhaustive; keep unknown future variants
            // representable instead of failing the whole record.
            _ => ("unknown", None),
        };
        decorations.push(entry);
        current = inner;
    }
    let (status_name, language) = match current {
        SymbolStatus::Mangled(language) => ("mangled", language_name(*language)),
        SymbolStatus::Unsupported(language) => ("unsupported", language_name(*language)),
        SymbolStatus::Unmangled => ("unmangled", None),
        SymbolStatus::Decorated { .. } => unreachable!("decorations are unwrapped above"),
        // `SymbolStatus` is non-exhaustive; see the `Decoration` arm above.
        _ => ("unknown", None),
    };
    (status_name, language, decorations)
}

/// Appends `value` as a JSON string literal, escaping quotes, backslashes,
/// and control characters.
fn push_json_string(out: &mut String, value: &str) {
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Builds the JSON record mirroring the Python module's
/// `demangle_symbol_ex`: mangled, demangled, status, language, decorations,
/// plus a nested `structured` object (or `null`) from the structured API.
fn structured_record(
    mangled: &str,
    demangled: &str,
    status: &SymbolStatus,
    info: Option<&DemangledInfo>,
) -> String {
    let (status_name, language, decorations) = describe(status);
    let mut out = String::with_capacity(mangled.len() + demangled.len() + 96);
    out.push_str("{\"mangled\":");
    push_json_string(&mut out, mangled);
    out.push_str(",\"demangled\":");
    push_json_string(&mut out, demangled);
    out.push_str(",\"status\":\"");
    out.push_str(status_name);
    out.push('"');
    match language {
        Some(language) => {
            out.push_str(",\"language\":\"");
            out.push_str(language);
            out.push('"');
        }
        None => out.push_str(",\"language\":null"),
    }
    out.push_str(",\"decorations\":[");
    for (idx, (kind, value)) in decorations.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str("{\"kind\":\"");
        out.push_str(kind);
        out.push('"');
        if let Some(value) = value {
            out.push_str(",\"value\":");
            push_json_string(&mut out, value);
        }
        out.push('}');
    }
    out.push_str("],\"structured\":");
    match info {
        Some(info) => push_structured_object(&mut out, info),
        None => out.push_str("null"),
    }
    out.push('}');
    out
}

/// Appends the structured-info object: the fields of the library's
/// `DemangledInfo` (language and mangled are already on the record).
fn push_structured_object(out: &mut String, info: &DemangledInfo) {
    out.push_str("{\"display\":");
    push_json_string(out, &info.display);
    out.push_str(",\"simple\":");
    push_json_string(out, &info.simple);
    out.push_str(",\"name\":");
    push_json_string(out, &info.name);
    out.push_str(",\"namespace\":[");
    for (idx, component) in info.namespace.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        push_json_string(out, component);
    }
    out.push_str("],\"kind\":\"");
    out.push_str(info.kind.kind_name());
    out.push('"');
    if let Some(class_method) = info.kind.class_method() {
        out.push_str(",\"class_method\":");
        out.push_str(if class_method { "true" } else { "false" });
    }
    match &info.parameters {
        Some(parameters) => {
            out.push_str(",\"parameters\":[");
            for (idx, parameter) in parameters.iter().enumerate() {
                if idx > 0 {
                    out.push(',');
                }
                push_json_string(out, parameter);
            }
            out.push(']');
        }
        None => out.push_str(",\"parameters\":null"),
    }
    match &info.return_type {
        Some(return_type) => {
            out.push_str(",\"return_type\":");
            push_json_string(out, return_type);
        }
        None => out.push_str(",\"return_type\":null"),
    }
    match &info.hash {
        Some(hash) => {
            out.push_str(",\"hash\":");
            push_json_string(out, hash);
        }
        None => out.push_str(",\"hash\":null"),
    }
    match &info.template_args {
        Some(template_args) => {
            out.push_str(",\"template_args\":[");
            for (idx, arg) in template_args.iter().enumerate() {
                if idx > 0 {
                    out.push(',');
                }
                push_json_string(out, arg);
            }
            out.push(']');
        }
        None => out.push_str(",\"template_args\":null"),
    }
    out.push_str(",\"is_generic\":");
    out.push_str(if info.is_generic { "true" } else { "false" });
    out.push('}');
}

/// Whether demangled output should be colorized. Structured output is JSON
/// and is never colorized.
fn resolve_color(when: ColorWhen, structured: bool) -> bool {
    if structured {
        return false;
    }
    match when {
        ColorWhen::Always => true,
        ColorWhen::Never => false,
        ColorWhen::Auto => {
            let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
            !no_color && io::stdout().is_terminal()
        }
    }
}

/// The names of the demangler backends compiled into this build.
fn enabled_backends() -> Vec<&'static str> {
    let mut backends = Vec::new();
    if cfg!(feature = "cpp") {
        backends.push("itanium");
    }
    if cfg!(feature = "gnuv2") {
        backends.push("gnuv2");
    }
    if cfg!(feature = "codewarrior") {
        backends.push("codewarrior");
    }
    if cfg!(feature = "msvc") {
        backends.push("msvc");
    }
    if cfg!(feature = "rust") {
        backends.push("rust");
    }
    if cfg!(feature = "swift") {
        backends.push("swift");
    }
    if cfg!(feature = "scala-native") {
        backends.push("scala-native");
    }
    // ObjC selectors are handled without a backend.
    backends.push("objc");
    backends
}

/// The text printed for `--version`: the crate version plus the enabled
/// backends. Clap prefixes the command name itself. Held in a static so it
/// can be handed to `Command::version` as a `&'static str`.
static VERSION_TEXT: LazyLock<String> = LazyLock::new(|| {
    format!(
        "{}\nenabled backends: {}",
        env!("CARGO_PKG_VERSION"),
        enabled_backends().join(", ")
    )
});

/// The ` [enabled]` / ` [disabled: ...]` suffix for a feature-gated language.
fn feature_note(enabled: bool, feature: &str) -> String {
    if enabled {
        " [enabled]".to_string()
    } else {
        format!(" [disabled: built without feature \"{feature}\"]")
    }
}

/// The suffix describing which C++ backends are enabled.
fn cpp_note() -> String {
    let mut backends = Vec::new();
    if cfg!(feature = "cpp") {
        backends.push("itanium");
    }
    if cfg!(feature = "gnuv2") {
        backends.push("gnuv2");
    }
    if cfg!(feature = "codewarrior") {
        backends.push("codewarrior");
    }
    if cfg!(feature = "msvc") {
        backends.push("msvc");
    }
    if backends.is_empty() {
        " [disabled: built without cpp features]".to_string()
    } else {
        format!(" [enabled: {}]", backends.join(", "))
    }
}

/// Prints the supported languages and whether their backends are enabled.
fn list_languages() {
    println!(
        "{:<width$}C++ (Itanium, GNU v2, CodeWarrior, MSVC){}",
        "cpp",
        cpp_note(),
        width = LANGUAGE_COLUMN_WIDTH
    );
    println!(
        "{:<width$}Rust (legacy and v0){}",
        "rust",
        feature_note(cfg!(feature = "rust"), "rust"),
        width = LANGUAGE_COLUMN_WIDTH
    );
    println!(
        "{:<width$}Swift{}",
        "swift",
        feature_note(cfg!(feature = "swift"), "swift"),
        width = LANGUAGE_COLUMN_WIDTH
    );
    println!(
        "{:<width$}Objective-C (selectors pass through unchanged) [enabled]",
        "objc",
        width = LANGUAGE_COLUMN_WIDTH
    );
    println!(
        "{:<width$}Objective-C++ (ObjC selectors plus C++ backends){}",
        "objcpp",
        cpp_note(),
        width = LANGUAGE_COLUMN_WIDTH
    );
    println!(
        "{:<width$}Scala Native{}",
        "scala-native",
        feature_note(cfg!(feature = "scala-native"), "scala-native"),
        width = LANGUAGE_COLUMN_WIDTH
    );
}

/// Translates the CLI flags into [`DemangleOptions`]. `--name-only` is a
/// shorthand that wins over the individual toggles.
fn demangle_options(cli: &Cli) -> DemangleOptions {
    DemangleOptions::complete()
        .return_type(!(cli.name_only || cli.no_return_type))
        .parameters(!(cli.name_only || cli.no_parameters))
}

fn main() -> ExitCode {
    let mut cmd = Cli::command();
    // `Command::version` requires a `&'static str` (clap's `Str` does not
    // take owned strings); a process-lifetime static provides one.
    cmd = cmd.version(VERSION_TEXT.as_str());
    let matches = cmd.get_matches();
    let cli = Cli::from_arg_matches(&matches).expect("matches come from the same command");

    if cli.list_languages {
        list_languages();
        return ExitCode::SUCCESS;
    }

    let renderer = Renderer {
        pipeline: Pipeline {
            opts: demangle_options(&cli),
            language: cli.language.map(LanguageArg::language),
            normalizer: cli.normalize.then(Normalizer::matching),
        },
        color: resolve_color(cli.color, cli.structured),
    };

    let result = if cli.symbols.is_empty() {
        run_filter(&renderer, cli.structured)
    } else {
        run_arguments(&renderer, cli.structured, &cli.symbols)
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        // A closed pipe (`multi-demangle ... | head`) is a normal end for a
        // filter, not an error.
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("multi-demangle: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_demangles_and_falls_back() {
        let pipeline = Pipeline {
            opts: DemangleOptions::complete(),
            language: None,
            normalizer: None,
        };
        assert_eq!(pipeline.run("_ZN3foo3barEv"), "foo::bar()");
        assert_eq!(pipeline.run("libc.so.6"), "libc.so.6");
        // Without --normalize, names the hygiene passes could clean pass
        // through unchanged.
        assert_eq!(pipeline.run("bar.llvm.12345"), "bar.llvm.12345");
    }

    #[test]
    fn test_pipeline_normalize_fallback() {
        let pipeline = Pipeline {
            opts: DemangleOptions::complete(),
            language: None,
            normalizer: Some(Normalizer::matching()),
        };
        assert_eq!(pipeline.run("memcpy@plt"), "memcpy");
        assert_eq!(pipeline.run("__imp_CreateFileW"), "CreateFileW");
        // Names that classify as unmangled are cleaned too.
        assert_eq!(pipeline.run("bar.llvm.12345"), "bar");
        assert_eq!(pipeline.run("foo$LT$"), "foo<");
        assert_eq!(
            pipeline.run("std::io::Read::read_to_end::hb85a0f6802e14499"),
            "std::io::Read::read_to_end"
        );
        // The cleaned symbol is demangled once more.
        assert_eq!(pipeline.run("_Z1hic@GLIBC_2.2.5"), "h(int, char)");
        assert_eq!(pipeline.run("__imp__ZN3foo3barEv"), "foo::bar()");
        // Directly successful demangling is never normalized.
        assert_eq!(pipeline.run("_ZN3foo3barEv"), "foo::bar()");
    }

    #[test]
    fn test_pipeline_forced_language() {
        let pipeline = Pipeline {
            opts: DemangleOptions::complete(),
            language: Some(Language::Swift),
            normalizer: None,
        };
        // Not Swift, so the forced backend declines and the token passes
        // through.
        assert_eq!(pipeline.run("_ZN3foo3barEv"), "_ZN3foo3barEv");
    }

    #[test]
    fn test_demangle_options_flags() {
        let cli = Cli::parse_from(["multi-demangle"]);
        assert_eq!(
            format!("{:?}", demangle_options(&cli)),
            format!("{:?}", DemangleOptions::complete())
        );

        let cli = Cli::parse_from(["multi-demangle", "-n"]);
        assert_eq!(
            format!("{:?}", demangle_options(&cli)),
            format!("{:?}", DemangleOptions::name_only())
        );

        let cli = Cli::parse_from(["multi-demangle", "--no-return-type"]);
        assert_eq!(
            format!("{:?}", demangle_options(&cli)),
            format!("{:?}", DemangleOptions::complete().return_type(false))
        );

        let cli = Cli::parse_from(["multi-demangle", "--no-parameters"]);
        assert_eq!(
            format!("{:?}", demangle_options(&cli)),
            format!("{:?}", DemangleOptions::complete().parameters(false))
        );
    }

    #[test]
    fn test_json_escaping() {
        let mut out = String::new();
        push_json_string(&mut out, "quote\" back\\ slash\n\t\u{1}");
        assert_eq!(out, "\"quote\\\" back\\\\ slash\\n\\t\\u0001\"");
    }

    #[test]
    fn test_structured_record() {
        let status = classify_symbol("_Z1hic@GLIBC_2.2.5");
        let record = structured_record("_Z1hic@GLIBC_2.2.5", "_Z1hic@GLIBC_2.2.5", &status, None);
        assert_eq!(
            record,
            "{\"mangled\":\"_Z1hic@GLIBC_2.2.5\",\"demangled\":\"_Z1hic@GLIBC_2.2.5\",\
             \"status\":\"mangled\",\"language\":\"cpp\",\
             \"decorations\":[{\"kind\":\"version\",\"value\":\"GLIBC_2.2.5\"}],\"structured\":null}"
        );
    }

    #[test]
    fn test_structured_record_unmangled() {
        let status = classify_symbol("libc.so.6");
        let record = structured_record("libc.so.6", "libc.so.6", &status, None);
        assert_eq!(
            record,
            "{\"mangled\":\"libc.so.6\",\"demangled\":\"libc.so.6\",\
             \"status\":\"unmangled\",\"language\":null,\"decorations\":[],\"structured\":null}"
        );
    }
}
