//! AST-backed MSVC structure: walks `msvc_demangler`'s public parse tree
//! (`parse` → `ParseResult`) to derive the entity kind and the
//! namespace/name path exactly, instead of inferring them from the rendered
//! string. This is what distinguishes a static variable
//! (`?value@ns@@3HA` → `int ns::value`, no parameter list in sight) from a
//! function, and a vftable or RTTI descriptor from an ordinary member.
//!
//! Parameters and return types stay text-derived (the crate has no public
//! type serializer; the rendered signature splits reliably once the access
//! specifier and thunk prefixes are removed).

use crate::DemangledKind;
use msvc_demangler::{parse, serialize, DemangleFlags, Name, Operator, ParseResult, Symbol, Type};

/// The structured fields derivable from the MSVC parse tree.
pub(super) struct MsvcAst {
    pub kind: DemangledKind,
    pub namespace: Vec<String>,
    pub name: String,
    /// Whether the entity is a template instantiation.
    pub is_template: bool,
}

/// Walks the parse tree of an MSVC-mangled symbol. Returns `None` when the
/// symbol does not parse; the caller falls back to text-derived extraction.
pub(super) fn walk_ast(sym: &str) -> Option<MsvcAst> {
    let parsed = parse(sym).ok()?;

    let mut kind = match &parsed.symbol_type {
        Type::MemberFunction(func_class, ..) => {
            if func_class.contains(msvc_demangler::FuncClass::THUNK) {
                DemangledKind::MethodThunk
            } else {
                DemangledKind::Method
            }
        }
        Type::NonMemberFunction(..) => DemangledKind::Function,
        Type::CXXVFTable(..) => DemangledKind::VirtualTable,
        Type::CXXVBTable(..) => DemangledKind::Other("vbtable".to_string()),
        Type::VCallThunk(..) => DemangledKind::MethodThunk,
        Type::Var(..) => DemangledKind::StaticVariable,
        _ => DemangledKind::Function,
    };

    let namespace: Vec<String> = parsed
        .symbol
        .scope
        .names
        .iter()
        // MSVC qualifies innermost-first; the path is outermost-first.
        .rev()
        .map(serialize_name)
        .collect();

    let mut is_template = false;
    let mut name = match &parsed.symbol.name {
        Name::Template(inner, _params) => {
            is_template = true;
            serialize_name(inner)
        }
        other => serialize_name(other),
    };

    // Constructors and destructors mangle as `?0`/`?1` operators whose
    // rendered name is the owning class; keep that identity explicit while
    // the namespace stays on the path (matching the `Bar::Bar` rendering).
    match &parsed.symbol.name {
        Name::Operator(Operator::Ctor) => {
            if let Some(owner) = namespace.last() {
                name = owner.clone();
            }
            kind = DemangledKind::Method;
        }
        Name::Operator(Operator::Dtor) => {
            if let Some(owner) = namespace.last() {
                name = format!("~{owner}");
            }
            kind = DemangledKind::Method;
        }
        Name::Operator(
            Operator::RTTITypeDescriptor(..)
            | Operator::RTTIBaseClassDescriptor(..)
            | Operator::RTTIBaseClassArray
            | Operator::RTTIClassHierarchyDescriptor
            | Operator::RTTIClassCompleteObjectLocator,
        ) => {
            kind = DemangledKind::TypeInfo;
            for keyword in ["class ", "struct ", "union ", "enum "] {
                if let Some(stripped) = name.strip_prefix(keyword) {
                    name = stripped.to_string();
                    break;
                }
            }
        }
        Name::Operator(Operator::VFTable) => kind = DemangledKind::VirtualTable,
        Name::Operator(Operator::VBTable) => kind = DemangledKind::Other("vbtable".to_string()),
        _ => {}
    }

    // Template arguments in the scope (e.g. `?$Vec@H@`) also make this an
    // instantiation.
    if !is_template
        && parsed
            .symbol
            .scope
            .names
            .iter()
            .any(|n| matches!(n, Name::Template(..)))
    {
        is_template = true;
    }

    Some(MsvcAst {
        kind,
        namespace,
        name,
        is_template,
    })
}

/// Renders one `Name` node on its own (name-only flags, no symbol type), so
/// scope components become clean namespace strings.
fn serialize_name(name: &Name<'_>) -> String {
    let parse_result = ParseResult {
        symbol: Symbol {
            name: name.clone(),
            scope: msvc_demangler::NameSequence { names: Vec::new() },
        },
        symbol_type: Type::None,
    };
    serialize(
        &parse_result,
        DemangleFlags::NAME_ONLY | DemangleFlags::NO_ACCESS_SPECIFIERS,
    )
}

/// Splits the cleaned rendering into parameters and return type using the
/// AST-derived name and namespace, instead of guessing the name: the name's
/// last occurrence (delimited, not inside an identifier) starts the
/// signature, the namespace prefix before it is not the return type.
pub(super) fn split_signature(
    display: &str,
    name: &str,
    namespace: &[String],
) -> (Option<Vec<String>>, Option<String>) {
    let Some(name_start) = find_delimited(display, name) else {
        return (None, None);
    };
    let after = &display[name_start + name.len()..];
    let Some(open) = after.find('(') else {
        return (None, None);
    };
    // Balanced scan for the closing paren of the parameter list.
    let bytes = after.as_bytes();
    let mut depth = 0usize;
    let mut close = None;
    for (idx, b) in bytes.iter().enumerate().skip(open) {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(idx);
                    break;
                }
            }
            _ => {}
        }
    }
    let Some(close) = close else {
        return (None, None);
    };
    let parameters = Some(split_params(&after[open + 1..close]));

    let mut before = display[..name_start].trim_end();
    if !namespace.is_empty() {
        let prefix = format!("{}::", namespace.join("::"));
        if let Some(stripped) = before.strip_suffix(&prefix) {
            before = stripped;
        }
    }
    let return_type = match before.trim() {
        "" => None,
        ret => Some(ret.to_string()),
    };
    (parameters, return_type)
}

/// Finds the last occurrence of `needle` in `text` that stands alone: not
/// preceded or followed by identifier characters.
fn find_delimited(text: &str, needle: &str) -> Option<usize> {
    let mut search_from = 0usize;
    let mut found = None;
    while let Some(pos) = text[search_from..].find(needle) {
        let start = search_from + pos;
        let end = start + needle.len();
        let before_ok = start == 0
            || !text[..start]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '~');
        let after_ok = end >= text.len()
            || !text[end..]
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
        if before_ok && after_ok {
            found = Some(start);
        }
        search_from = start + needle.len().max(1);
        if search_from >= text.len() {
            break;
        }
    }
    found
}

/// Splits a parameter list at top-level commas.
fn split_params(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (idx, b) in text.bytes().enumerate() {
        match b {
            b'<' | b'(' => depth += 1,
            b'>' | b')' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                parts.push(text[start..idx].trim().to_string());
                start = idx + 1;
            }
            _ => {}
        }
    }
    parts.push(text[start..].trim().to_string());
    if parts.len() == 1 && parts[0].is_empty() {
        parts.clear();
    }
    parts
}

/// Removes the prefixes the MSVC serializer places before the signature
/// (`public:`, `[thunk]:`, `virtual`, `static`, ...), which are not part of
/// the return type.
pub(super) fn strip_display_prefixes(display: &str) -> &str {
    let mut rest = display.trim();
    loop {
        let stripped = rest
            .strip_prefix("[thunk]: ")
            .or_else(|| rest.strip_prefix("public: "))
            .or_else(|| rest.strip_prefix("private: "))
            .or_else(|| rest.strip_prefix("protected: "))
            .or_else(|| rest.strip_prefix("virtual "))
            .or_else(|| rest.strip_prefix("static "));
        match stripped {
            Some(next) => rest = next.trim_start(),
            None => return rest,
        }
    }
}
