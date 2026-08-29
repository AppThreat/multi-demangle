//! Structured demangling: typed fields extracted from demangled renderings.
//!
//! A demangled *string* forces every consumer to re-parse text to answer the
//! question they actually have — *which function is this?* — so this module
//! extracts the structure up front: the namespace path, the leaf name, the
//! entity kind (function, method, closure, vtable, ...), generic arguments,
//! parameter and return types, and compiler disambiguation hashes.
//!
//! The extraction rules are text-derived (Phase 1 of the structured-API plan)
//! and absorb the canonicalization heuristics of the primary consumer (OWASP
//! blint's `callgraph/canon.py`) as the specification: trailing Rust hashes
//! and `.llvm.N` clone counters are captured into [`DemangledInfo::hash`],
//! `<Type as Trait>::` and `<impl Trait for Type>::` prefixes are reduced to
//! the implementing type in [`DemangledInfo::namespace`], and kinds follow
//! blint's closure/glue/intrinsic/method tables.
//!
//! # Examples
//!
//! ```
//! # #[cfg(feature = "rust")] {
//! use multi_demangle::{Demangle, DemangleOptions};
//! use symbolic_common::Name;
//!
//! let info = Name::from("_ZN3std2io4Read11read_to_end17hb85a0f6802e14499E")
//!     .demangle_structured(DemangleOptions::complete())
//!     .unwrap();
//! assert_eq!(info.namespace, ["std", "io", "Read"]);
//! assert_eq!(info.name, "read_to_end");
//! assert_eq!(info.hash.as_deref(), Some("hb85a0f6802e14499"));
//! assert_eq!(info.kind, multi_demangle::DemangledKind::Method);
//! # }
//! ```

use std::fmt::Display;

#[cfg(feature = "msvc")]
mod msvc;
#[cfg(feature = "swift")]
mod swift;

use symbolic_common::{Language, Name, NameMangling};

use crate::{Demangle, DemangleOptions, SymbolStatus};

/// The demangler's structured view of a mangled symbol.
///
/// Fields mirror the plan API; the struct is [`non_exhaustive`] so new fields
/// are not breaking changes.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DemangledInfo {
    /// Detected language (`Unknown` for Scala Native, which has no
    /// `Language` variant of its own).
    pub language: Language,
    /// Full verbose rendering (what `demangle()` returns today).
    pub display: String,
    /// Name-only rendering.
    pub simple: String,
    /// Namespace/module/class path, outermost first. Components keep their
    /// generic arguments as rendered (`"Vec<u8>"`); `<Type as Trait>::` and
    /// `<impl Trait for Type>::` prefixes are reduced to the implementing
    /// type, and the original rendering stays available in `display`.
    pub namespace: Vec<String>,
    /// Leaf name: function/method name, or the selector for ObjC.
    pub name: String,
    /// What kind of entity the symbol denotes.
    pub kind: DemangledKind,
    /// Parameter type renderings, when the scheme encodes them (`None` for
    /// legacy Rust, whose mangling does not encode parameter types).
    pub parameters: Option<Vec<String>>,
    /// Return type rendering, when encoded.
    pub return_type: Option<String>,
    /// Trailing disambiguation material captured from the symbol: the legacy
    /// Rust hash (`h<hash>`), a C++ linker hash (`$<hash>`), and/or the
    /// `.llvm.<N>` clone counter, in the order they appear.
    pub hash: Option<String>,
    /// Generic/template argument renderings found in the path, in path
    /// order (`Vec<u8>` contributes `"u8"`).
    pub template_args: Option<Vec<String>>,
    /// True when the name carries generic/template arguments; the argument
    /// renderings live in `template_args` when the backend provides them
    /// (an MSVC scope template sets this flag without renderings).
    pub is_generic: bool,
    /// The original mangled symbol.
    pub mangled: String,
}

/// What kind of entity a demangled symbol denotes.
///
/// The classification is a best-effort hint modeled on the consumer's tables
/// (closures, compiler glue, intrinsics, and the CamelCase-owner method
/// heuristic), so compiler-generated artefacts can be kept apart from
/// ordinary source functions. The enum is [`non_exhaustive`].
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DemangledKind {
    /// A free function.
    Function,
    /// A method or other member of a type (including constructors).
    Method,
    /// A compiler-generated closure.
    Closure,
    /// C runtime/linker glue or Rust drop glue with no source counterpart.
    Glue,
    /// A compiler intrinsic or `__`-prefixed builtin.
    Intrinsic,
    /// A virtual/override thunk adjusting `this` or the vtable pointer.
    MethodThunk,
    /// A vtable or VTT symbol.
    VirtualTable,
    /// A typeinfo or typeinfo-name symbol.
    TypeInfo,
    /// A static variable (currently only reported by AST-backed backends).
    StaticVariable,
    /// An Objective-C method; `class_method` distinguishes `+method` from
    /// `-method`.
    ObjCMethod {
        /// Whether this is a class (`+`) method.
        class_method: bool,
    },
    /// Anything that does not fit the known kinds.
    Other(String),
}

impl DemangledKind {
    /// The lowercase snake-case name of the kind, as used across the JSON
    /// and Python APIs.
    pub fn kind_name(&self) -> &str {
        match self {
            DemangledKind::Function => "function",
            DemangledKind::Method => "method",
            DemangledKind::Closure => "closure",
            DemangledKind::Glue => "glue",
            DemangledKind::Intrinsic => "intrinsic",
            DemangledKind::MethodThunk => "method_thunk",
            DemangledKind::VirtualTable => "virtual_table",
            DemangledKind::TypeInfo => "type_info",
            DemangledKind::StaticVariable => "static_variable",
            DemangledKind::ObjCMethod { .. } => "objc_method",
            DemangledKind::Other(_) => "other",
        }
    }

    /// Whether this is an Objective-C class (`+`) method.
    pub fn class_method(&self) -> Option<bool> {
        match self {
            DemangledKind::ObjCMethod { class_method } => Some(*class_method),
            _ => None,
        }
    }
}

impl Display for DemangledKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.kind_name())
    }
}

/// C runtime / linker glue that has no source counterpart, ported from the
/// consumer's `_CRT_GLUE` table. Leaf names (and bare paths) in this set are
/// classified as [`DemangledKind::Glue`].
const CRT_GLUE: &[&str] = &[
    "_start",
    "_init",
    "_fini",
    "deregister_tm_clones",
    "register_tm_clones",
    "__do_global_dtors_aux",
    "frame_dummy",
    "__libc_csu_init",
    "__libc_csu_fini",
    "_dl_relocate_static_pie",
    "__rust_alloc",
    "__rust_dealloc",
    "__rust_realloc",
    "__rust_alloc_zeroed",
];

/// Builds the structured view of `name` with the given options.
///
/// Returns `None` when the symbol is not mangled in any known scheme (or is
/// an opaque MD5 name) — mirroring `Demangle::demangle`'s `None` — and
/// always populates `display` with the rendering of `opts` and `simple` with
/// a name-only rendering.
pub(crate) fn demangle_structured(name: &Name<'_>, opts: DemangleOptions) -> Option<DemangledInfo> {
    let sym = name.as_str();
    let explicit_language = name.language() != Language::Unknown;
    let language = if explicit_language {
        name.language()
    } else {
        detected_language(sym)?
    };
    if !explicit_language
        && (matches!(crate::classify_symbol(sym), SymbolStatus::Unmangled)
            || crate::is_maybe_md5(sym))
    {
        // MD5 names are opaque identifiers, not structure to extract.
        return None;
    }

    if crate::is_maybe_objc(sym) {
        return Some(objc_info(sym, language, opts));
    }

    let display = name.demangle(opts)?;
    // Name-only options already produce the simple rendering; skip the
    // second demangling pass otherwise required per symbol.
    let simple = if opts == DemangleOptions::name_only() {
        display.clone()
    } else {
        name.demangle(DemangleOptions::name_only())
            .unwrap_or_else(|| display.clone())
    };

    let hash = capture_hash(sym, language);

    // MSVC renderings lead with access specifiers and thunk markers that
    // are not part of the return type; the AST-backed walk below provides
    // kind and identity, the cleaned text still splits the signature.
    #[allow(unused_mut)]
    let mut display_for_split = display.as_str();
    #[cfg(feature = "msvc")]
    if matches!(language, Language::Cpp | Language::ObjCpp) && crate::is_maybe_msvc(sym) {
        display_for_split = msvc::strip_display_prefixes(display_for_split);
    }
    let (path, mut parameters, mut return_type) = match language {
        Language::Cpp | Language::ObjCpp => split_cpp_signature(display_for_split),
        _ => split_generic_signature(&display),
    };
    let separator = match language {
        Language::Cpp | Language::ObjCpp | Language::Rust => "::",
        _ => ".",
    };
    let (mut namespace, mut leaf_name, template_args) = parse_path(&path, separator);
    let mut kind = classify_kind(sym, language, &display, &namespace, &leaf_name);
    let mut is_generic = template_args.is_some();

    // Phase 2: where a backend exposes its parse tree, the AST's kind and
    // identity fields take precedence over the text-derived ones.
    #[cfg(feature = "msvc")]
    if matches!(language, Language::Cpp | Language::ObjCpp) && crate::is_maybe_msvc(sym) {
        if let Some(ast) = msvc::walk_ast(sym) {
            kind = ast.kind;
            namespace = ast.namespace;
            leaf_name = ast.name;
            is_generic = ast.is_template || template_args.is_some();
            // The AST knows where the name ends, so the signature is split
            // by it instead of by the text heuristics (MSVC names can
            // contain spaces, as in `` `vector deleting destructor' ``).
            let (msvc_params, msvc_return) = msvc::split_signature(
                msvc::strip_display_prefixes(&display),
                &leaf_name,
                &namespace,
            );
            parameters = msvc_params.or(parameters);
            return_type = msvc_return.or(return_type);
        }
    }
    #[cfg(feature = "swift")]
    if matches!(language, Language::Swift) {
        if let Some(dump) = crate::try_dump_swift(sym) {
            if let Some(ast) = swift::walk_dump(&dump) {
                kind = ast.kind;
                namespace = ast.namespace;
                if !ast.name.is_empty() {
                    leaf_name = ast.name;
                }
            }
        }
    }

    Some(DemangledInfo {
        language,
        display,
        simple,
        namespace,
        name: leaf_name,
        kind,
        parameters,
        return_type,
        hash,
        is_generic,
        template_args,
        mangled: sym.to_string(),
    })
}

/// The language of `sym`, mapped back from [`crate::detect_language`] so
/// Scala Native (which has no `Language` variant) maps to
/// [`Language::Unknown`].
fn detected_language(sym: &str) -> Option<Language> {
    match crate::detect_language(sym) {
        Some("cpp") => Some(Language::Cpp),
        Some("rust") => Some(Language::Rust),
        Some("swift") => Some(Language::Swift),
        Some("objc") => Some(Language::ObjC),
        Some("objcpp") => Some(Language::ObjCpp),
        Some("scala-native") => Some(Language::Unknown),
        _ => None,
    }
}

/// Builds the structured view of an Objective-C selector (`-[Class sel:]`).
fn objc_info(sym: &str, language: Language, opts: DemangleOptions) -> DemangledInfo {
    // Selectors are their own readable name; `demangle` returns them
    // unchanged.
    let display = Name::new(sym, NameMangling::Mangled, language)
        .demangle(opts)
        .unwrap_or_else(|| sym.to_string());
    let inner = &sym[2..sym.len() - 1];
    let (class, selector) = inner.split_once(' ').unwrap_or((inner, ""));
    DemangledInfo {
        language,
        simple: display.clone(),
        display,
        namespace: vec![class.to_string()],
        name: selector.to_string(),
        kind: DemangledKind::ObjCMethod {
            class_method: sym.starts_with("+["),
        },
        parameters: None,
        return_type: None,
        hash: None,
        template_args: None,
        is_generic: false,
        mangled: sym.to_string(),
    }
}

/// Splits a C++ rendering into its name path, parameters, and return type
/// using the shared signature analyzer (which locates the parameter list and
/// the name that precedes it). Brace-wrapped special forms (`{vtable(...)}`,
/// `{typeinfo(...)}`, ...) are unwrapped to their inner path first; thunk
/// forms keep their full rendering as the name.
fn split_cpp_signature(display: &str) -> (String, Option<Vec<String>>, Option<String>) {
    // Guard variables and reference temps render as
    // "<kind> for <enclosing function>()::<name>"; the parenthesized
    // function in the middle is not a signature of this entity, so the
    // inner path is taken verbatim.
    for prefix in ["guard variable for ", "reference temporary for "] {
        if let Some(inner) = display.strip_prefix(prefix) {
            return (inner.to_string(), None, None);
        }
    }
    if display.starts_with('{') && display.ends_with('}') {
        let inner = &display[1..display.len() - 1];
        for prefix in ["vtable(", "vtt(", "typeinfo name(", "typeinfo("] {
            if let Some(rest) = inner.strip_prefix(prefix) {
                if let Some(stripped) = rest.strip_suffix(')') {
                    return (stripped.to_string(), None, None);
                }
            }
        }
        return (display.to_string(), None, None);
    }
    match crate::analyze_cpp_like_signature(display) {
        Some((name, params_start, _signature_end)) => {
            let prefix = display[..params_start].trim_end();
            let return_type = if prefix.ends_with(&name) {
                match prefix[..prefix.len() - name.len()].trim() {
                    "" => None,
                    ret => Some(ret.to_string()),
                }
            } else {
                None
            };
            let parameters = crate::matching_paren_end(display, params_start)
                .map(|close| split_top_level_commas(&display[params_start + 1..close - 1]));
            (name, parameters, return_type)
        }
        None => (display.to_string(), None, None),
    }
}

/// Splits a non-C++ rendering at the first top-level `(`: the group holds
/// the parameters and anything after it is the return type (`-> Ret` for
/// Rust and Swift, `: Ret` for Scala Native).
fn split_generic_signature(display: &str) -> (String, Option<Vec<String>>, Option<String>) {
    let mut angle_depth = 0usize;
    let mut paren_depth = 0usize;
    for (idx, ch) in display.char_indices() {
        match ch {
            '<' => angle_depth += 1,
            '>' => angle_depth = angle_depth.saturating_sub(1),
            '(' if angle_depth == 0 && paren_depth == 0 => {
                let Some(close) = crate::matching_paren_end(display, idx) else {
                    break;
                };
                let path = display[..idx].trim_end().to_string();
                let parameters = Some(split_top_level_commas(&display[idx + 1..close - 1]));
                let tail = display[close..].trim_start();
                let return_type = tail
                    .strip_prefix("-> ")
                    .or_else(|| tail.strip_prefix(": "))
                    .map(str::to_string);
                return (path, parameters, return_type);
            }
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            _ => {}
        }
    }
    (display.to_string(), None, None)
}

/// Splits `path` into namespace components and the leaf name at top-level
/// occurrences of `sep` (never inside `<...>` or `(...)`), capturing the
/// generic argument groups of each component. A leading `<Type as Trait>::`
/// or `<impl Trait for Type>::` group is reduced to the implementing type
/// first, mirroring the consumer's canonicalization.
fn parse_path(path: &str, sep: &str) -> (Vec<String>, String, Option<Vec<String>>) {
    let mut template_args: Option<Vec<String>> = None;
    let mut components = Vec::new();
    for component in split_top_level(&reduce_qualified_self(path), sep) {
        if component.is_empty() {
            continue;
        }
        // Namespace components keep their generic arguments as rendered
        // (`Vec<u8>`); the leaf name below is bare, and the argument groups
        // are collected into `template_args`. Unreduced `<...>` groups
        // (mid-path `<impl Trait for Type>`) are not template arguments.
        if !component.starts_with('<') {
            if let Some(args) = component_args(component) {
                template_args.get_or_insert_with(Vec::new).extend(args);
            }
        }
        components.push(component.to_string());
    }

    let Some((leaf, namespace)) = components.split_last() else {
        return (Vec::new(), path.to_string(), template_args);
    };
    let (name, _) = split_component_args(leaf);
    (namespace.to_vec(), name, template_args)
}

/// Reduces a leading balanced `<...>` group to the implementing type:
/// `<Type as Trait>::method` becomes `Type::method`,
/// `<impl Trait for Type>::method` becomes `Type::method`, and
/// `<impl Type>::method` becomes `Type::method`. Applied recursively, so a
/// reduced prefix that is itself qualified is reduced again.
fn reduce_qualified_self(path: &str) -> String {
    let Some((inner, rest)) = split_leading_angle_group(path) else {
        return path.to_string();
    };

    let reduced = if let Some(body) = inner.strip_prefix("impl ") {
        match find_top_level(body, " for ") {
            Some(idx) => &body[idx + " for ".len()..],
            None => body,
        }
    } else {
        match find_top_level(inner, " as ") {
            Some(idx) => &inner[..idx],
            None => inner,
        }
    };
    let reduced = reduced.trim();
    format!("{}{rest}", reduce_qualified_self(reduced))
}

/// Splits a leading balanced `<...>` group from `path`, returning the group
/// contents (without brackets) and everything after the closing `>`.
fn split_leading_angle_group(path: &str) -> Option<(&str, &str)> {
    if !path.starts_with('<') {
        return None;
    }
    let mut depth = 0usize;
    for (idx, ch) in path.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&path[1..idx], &path[idx + 1..]));
                }
            }
            _ => {}
        }
    }
    None
}

/// Finds `needle` in `text` outside any `<...>` group, else `None`.
fn find_top_level(text: &str, needle: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (idx, ch) in text.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 && text[idx..].starts_with(needle) => return Some(idx),
            _ => {}
        }
    }
    None
}

/// Splits `path` at top-level occurrences of `sep`.
fn split_top_level<'a>(path: &'a str, sep: &str) -> Vec<&'a str> {
    let mut parts = Vec::new();
    let mut angle_depth = 0usize;
    let mut paren_depth = 0usize;
    let mut start = 0usize;
    let mut idx = 0usize;
    while idx < path.len() {
        let ch = path[idx..].chars().next().expect("non-empty remainder");
        match ch {
            '<' => angle_depth += 1,
            '>' => angle_depth = angle_depth.saturating_sub(1),
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            _ if angle_depth == 0 && paren_depth == 0 && path[idx..].starts_with(sep) => {
                parts.push(&path[start..idx]);
                idx += sep.len();
                start = idx;
                continue;
            }
            _ => {}
        }
        idx += ch.len_utf8();
    }
    parts.push(&path[start..]);
    parts
}

/// Extracts the top-level generic argument groups of a path component.
/// An unbalanced `<` (as in C++ `operator<`) yields no arguments.
fn component_args(component: &str) -> Option<Vec<String>> {
    split_component_args(component).1
}

/// Splits one path component into its base name and its top-level generic
/// argument groups. An unbalanced `<` (as in C++ `operator<`) is kept
/// verbatim in the base.
fn split_component_args(component: &str) -> (String, Option<Vec<String>>) {
    let mut base = String::with_capacity(component.len());
    let mut args: Option<Vec<String>> = None;
    let mut rest = component;
    while let Some(open) = rest.find('<') {
        // Find the `>` matching the one at `open`, tracking nesting; an
        // unbalanced group is not a generic argument list.
        let mut depth = 0usize;
        let mut close = None;
        for (offset, ch) in rest[open..].char_indices() {
            match ch {
                '<' => depth += 1,
                '>' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(open + offset);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(close) = close else { break };
        base.push_str(&rest[..open]);
        args.get_or_insert_with(Vec::new)
            .extend(split_top_level_commas(&rest[open + 1..close]));
        rest = &rest[close + 1..];
    }
    base.push_str(rest);
    (base.trim().to_string(), args)
}

/// Splits `text` on commas that sit outside any `<...>`, `(...)`, or
/// `[...]` group, trimming whitespace around each entry.
fn split_top_level_commas(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    let mut idx = 0usize;
    while idx < text.len() {
        let ch = text[idx..].chars().next().expect("non-empty remainder");
        match ch {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(text[start..idx].trim().to_string());
                idx += ch.len_utf8();
                start = idx;
                continue;
            }
            _ => {}
        }
        idx += ch.len_utf8();
    }
    if start < text.len() || !parts.is_empty() {
        parts.push(text[start..].trim().to_string());
    }
    // An empty parameter list `()` is an empty vec, not [""].
    if parts.len() == 1 && parts[0].is_empty() {
        parts.clear();
    }
    parts
}

/// Captures trailing disambiguation material from the mangled symbol: the
/// `.llvm.<N>` clone counter, the legacy Rust hash (`<len>h<hash>E`), and
/// the C++ linker hash (`$<32 hex>`).
fn capture_hash(sym: &str, language: Language) -> Option<String> {
    let mut hash = String::new();

    // The clone counter may also trail an already-captured hash.
    let (base, counter) = split_llvm_counter(sym);
    if let Some(counter) = counter {
        hash.push_str(counter);
    }

    match language {
        Language::Rust => {
            if let Some(legacy) = legacy_rust_hash(base) {
                hash.insert_str(0, legacy);
            }
        }
        Language::Cpp | Language::ObjCpp => {
            let stripped = crate::strip_hash_suffix(base);
            if stripped.len() < base.len() {
                hash.insert_str(0, &base[stripped.len()..]);
            }
        }
        _ => {}
    }

    if hash.is_empty() {
        None
    } else {
        Some(hash)
    }
}

/// Splits a trailing `.llvm.<digits>` clone counter from `sym`, returning
/// the base symbol and the counter text (including the `.llvm.` separator).
fn split_llvm_counter(sym: &str) -> (&str, Option<&str>) {
    let Some(idx) = sym.rfind(".llvm.") else {
        return (sym, None);
    };
    if idx == 0 {
        return (sym, None);
    }
    let counter = &sym[idx..];
    if counter[6..].bytes().all(|b| b.is_ascii_digit()) && counter.len() > 6 {
        (&sym[..idx], Some(counter))
    } else {
        (sym, None)
    }
}

/// Extracts the legacy Rust hash from a `<len>h<hash>E` symbol tail:
/// at least eight hex digits follow the `h`, and the length digits precede
/// it. Returns the hash including its `h` prefix.
fn legacy_rust_hash(base: &str) -> Option<&str> {
    if !base.ends_with('E') {
        return None;
    }
    let mut hash_end = base.len() - 1;
    let mut hex_len = 0usize;
    while hash_end > 0 {
        let b = base.as_bytes()[hash_end - 1];
        if b.is_ascii_hexdigit() && !b.is_ascii_uppercase() {
            hash_end -= 1;
            hex_len += 1;
        } else {
            break;
        }
    }
    if hex_len < 8 || hash_end == 0 || base.as_bytes()[hash_end - 1] != b'h' {
        return None;
    }
    // The legacy grammar encodes the hash length as digits before the `h`;
    // verify they are there but keep them out of the captured hash.
    let mut len_start = hash_end - 1;
    while len_start > 0 && base.as_bytes()[len_start - 1].is_ascii_digit() {
        len_start -= 1;
    }
    if len_start == hash_end - 1 {
        return None;
    }
    Some(&base[hash_end - 1..base.len() - 1])
}

/// Classifies a symbol into a [`DemangledKind`], porting the consumer's
/// kind tables: closure markers, drop glue, the CRT-glue set, `__`-builtins,
/// and the CamelCase-owner method heuristic.
fn classify_kind(
    sym: &str,
    language: Language,
    display: &str,
    namespace: &[String],
    name: &str,
) -> DemangledKind {
    if matches!(language, Language::ObjC) {
        // Callers hand ObjC selectors to `objc_info`; selectors forced
        // through an explicit ObjCpp language still land here.
        return DemangledKind::ObjCMethod {
            class_method: sym.starts_with("+["),
        };
    }

    if matches!(language, Language::Cpp | Language::ObjCpp) {
        // Itanium constructor-style prefixes survive any number of leading
        // underscores (macOS `__Z`, Windows `___Z`).
        let bare = sym.trim_start_matches('_');
        let prefixed = bare.strip_prefix('Z').unwrap_or(bare);
        if prefixed.starts_with("TV") || prefixed.starts_with("TT") || prefixed.starts_with("TC") {
            return DemangledKind::VirtualTable;
        }
        if prefixed.starts_with("TI") || prefixed.starts_with("TS") {
            return DemangledKind::TypeInfo;
        }
        if prefixed.starts_with("Th") || prefixed.starts_with("Tv") {
            return DemangledKind::MethodThunk;
        }
        // Guard variables and reference temps of static storage.
        if prefixed.starts_with("GV") || prefixed.starts_with("GR") {
            return DemangledKind::StaticVariable;
        }
        if display.starts_with("{vtable(") || display.starts_with("{vtt(") {
            return DemangledKind::VirtualTable;
        }
        if display.starts_with("{typeinfo") {
            return DemangledKind::TypeInfo;
        }
        if display.starts_with("{virtual override thunk") || display.starts_with("{virtual thunk") {
            return DemangledKind::MethodThunk;
        }
    }

    // Closure markers in the rendering or as a path component.
    if display.contains("{{closure}}")
        || namespace.iter().any(|c| is_closure_component(c))
        || is_closure_component(name)
    {
        return DemangledKind::Closure;
    }

    if display.contains("drop_in_place") {
        return DemangledKind::Glue;
    }

    if CRT_GLUE.contains(&name) || namespace.len() == 1 && CRT_GLUE.contains(&namespace[0].as_str())
    {
        return DemangledKind::Glue;
    }

    if name.starts_with("__") || (namespace.is_empty() && name.starts_with('_')) {
        // Known limitation: this also matches deliberately underscore-prefixed
        // C++ names (`__cxxabiv1::__class_type_info`); the vtable/typeinfo
        // prefix checks above already handle the compiler-internal forms.
        return DemangledKind::Intrinsic;
    }

    // C++ member operators (`operator()`, `operator<`, ...) are methods
    // regardless of the owner's casing; the template-heavy owners they
    // appear on (`function_ref<void ()>`) often are not CamelCase.
    if !namespace.is_empty() && name.starts_with("operator") {
        return DemangledKind::Method;
    }

    // A CamelCase penultimate segment indicates a method on a type.
    if let Some(owner) = namespace.last() {
        if owner.chars().next().is_some_and(|c| c.is_uppercase()) {
            return DemangledKind::Method;
        }
    }

    DemangledKind::Function
}

/// Whether a path component is a compiler-generated closure name:
/// `{{closure}}` or `closure`/`closure_2202_77`-style segments.
fn is_closure_component(component: &str) -> bool {
    if component == "{{closure}}" {
        return true;
    }
    let Some(rest) = component.strip_prefix("closure") else {
        return false;
    };
    // The remainder must be `_`-separated digit groups (`_2202_77`) or
    // empty; anything else (`closurefish`) is an ordinary name.
    rest.is_empty()
        || (rest.starts_with('_')
            && rest[1..]
                .split('_')
                .all(|g| !g.is_empty() && g.bytes().all(|b| b.is_ascii_digit())))
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::Demangle;

    fn info(sym: &str) -> DemangledInfo {
        Name::from(sym)
            .demangle_structured(DemangleOptions::complete())
            .expect("structured demangling succeeds")
    }

    #[test]
    fn legacy_rust_read_to_end() {
        let info = info("_ZN3std2io4Read11read_to_end17hb85a0f6802e14499E");
        assert_eq!(info.language, Language::Rust);
        assert_eq!(info.namespace, ["std", "io", "Read"]);
        assert_eq!(info.name, "read_to_end");
        assert_eq!(info.kind, DemangledKind::Method);
        assert_eq!(info.hash.as_deref(), Some("hb85a0f6802e14499"));
        assert_eq!(info.parameters, None);
        assert!(!info.is_generic);
    }

    #[test]
    fn qualified_self_reduction() {
        let info = info("__ZN102_$LT$core..iter..adapters..map..Map$LT$I$C$F$GT$$u20$as$u20$core..iter..traits..iterator..Iterator$GT$4next17h588c4c3ad8f9f79aE");
        assert_eq!(
            info.namespace,
            ["core", "iter", "adapters", "map", "Map<I,F>"]
        );
        assert_eq!(info.name, "next");
        assert_eq!(info.hash.as_deref(), Some("h588c4c3ad8f9f79a"));
        assert_eq!(
            info.template_args,
            Some(vec!["I".to_string(), "F".to_string()])
        );
        assert!(info.is_generic);
        assert_eq!(info.kind, DemangledKind::Method);
    }

    #[test]
    fn closure_kind() {
        let info = info("_ZN10wasm_smith4core15closure_2202_7717h0123456789abcdefE");
        assert_eq!(info.kind, DemangledKind::Closure);
    }

    #[test]
    fn crt_glue_kind() {
        let info = info("__RNvCsdBezzDwma51_7___rustc12___rust_alloc");
        assert_eq!(info.kind, DemangledKind::Glue);
        assert_eq!(info.namespace, ["__rustc"]);
        assert_eq!(info.name, "__rust_alloc");
    }

    #[test]
    fn cpp_function_split() {
        let info = info("_ZN3foo3barEv");
        assert_eq!(info.namespace, ["foo"]);
        assert_eq!(info.name, "bar");
        assert_eq!(info.kind, DemangledKind::Function);
        assert_eq!(info.parameters, Some(vec![]));
    }

    #[test]
    fn objc_selector() {
        let info = info("-[Foo bar:blub:]");
        assert_eq!(info.language, Language::ObjC);
        assert_eq!(info.namespace, ["Foo"]);
        assert_eq!(info.name, "bar:blub:");
        assert_eq!(
            info.kind,
            DemangledKind::ObjCMethod {
                class_method: false
            }
        );
    }

    #[test]
    fn unmangled_input_is_none() {
        assert_eq!(
            Name::from("libc.so.6").demangle_structured(DemangleOptions::complete()),
            None
        );
    }

    #[test]
    fn llvm_counter_captured() {
        let info = info("_ZN5tokio7runtime4task7harness20Harness$LT$T$C$S$GT$8complete17h79b950493dfd179dE.llvm.3144946739014404372");
        assert_eq!(
            info.hash.as_deref(),
            Some("h79b950493dfd179d.llvm.3144946739014404372")
        );
        assert_eq!(info.name, "complete");
        assert_eq!(
            info.template_args,
            Some(vec!["T".to_string(), "S".to_string()])
        );
    }
}
