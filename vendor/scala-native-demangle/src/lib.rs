//! Demangle Scala Native identifiers
//!
//! Turn mangled Scala Native identifiers into a more readable form.
//!
//! 1. Name mangling rules: https://scala-native.org/en/latest/contrib/mangling.html
//! 2. Scala implementation: https://github.com/indoorvivants/sn-demangler

pub type DemangleError = String;
pub type ParsingResult<T> = Result<T, DemangleError>;

pub struct DemanglingConfig {
    pub collapse_scala_names: bool,
    pub debug: bool,
}

static DEFAULT_CONFIG: DemanglingConfig = DemanglingConfig {
    collapse_scala_names: true,
    debug: false,
};

impl Default for DemanglingConfig {
    fn default() -> Self {
        DemanglingConfig {
            collapse_scala_names: true,
            debug: false,
        }
    }
}

impl DemanglingConfig {
    fn log(&self, str: &str) -> () {
        if self.debug {
            println!("{str}")
        }
    }
    fn log_name(&self, name: &str, str: &str) -> () {
        if self.debug {
            println!("{name}: {str}")
        }
    }
}

pub fn demangle(input: &str, config: &DemanglingConfig) -> ParsingResult<String> {
    if !input.starts_with("_S") {
        return Err("identifier doesn't start with _S".to_string());
    } else {
        config.log_name("demangle", &input[2..]);
        return defn_name(&input[2..], config);
    }
}

pub fn demangle_with_defaults(input: &str) -> ParsingResult<String> {
    return demangle(input, &DEFAULT_CONFIG);
}

// private sub parsers
// <defn-name> ::=
//     T <name>                       // top-level name
//     M <name> <sig-name>            // member name
fn defn_name(input: &str, config: &DemanglingConfig) -> ParsingResult<String> {
    config.log_name("defn_name", input);
    if input.starts_with("T") {
        return toplevel_name(&input[1..], config);
    } else if input.starts_with("M") {
        return member_name(&input[1..], config);
    } else {
        if input.len() > 0 {
            return Err(format!(
                "defn_name: unknown name modifier '{}'",
                input[0..0].to_string()
            ));
        } else {
            return Err("defn_name: unexpectedly empty rest of identifier".to_string());
        }
    }
}

fn toplevel_name(input: &str, config: &DemanglingConfig) -> ParsingResult<String> {
    config.log_name("toplevel_name", input);
    return name(input, config).map(|t| t.1);
}
fn member_name(input: &str, config: &DemanglingConfig) -> ParsingResult<String> {
    config.log_name("member_name", input);
    let (consumed, owner) = name(input, config)?;
    let signature = sig_name(
        input.get(consumed..).ok_or("member_name: out of bounds".to_string())?,
        config,
    );

    return signature.and_then(|s| return Ok(format!("{}.{}", owner, s)));
}

// <sig-name> ::=
//     F <name> <scope>                    // field name
//     R <type-name>+ E                    // constructor name
//     D <name> <type-name>+ E <scope>     // method name
//     P <name> <type-name>+ E             // proxy name
//     C <name>                            // c extern name
//     G <name>                            // generated name
//     K <sig-name> <type-name>+ E         // duplicate name
fn sig_name(input: &str, config: &DemanglingConfig) -> ParsingResult<String> {
    config.log_name("sig_name", input);
    if input.starts_with("C") || input.starts_with("G") {
        return Ok(name(&input[1..], config)?.1);
    } else if input.starts_with("I") {
        return Ok("<clinit>".to_string());
    } else if input.starts_with("F") {
        let (consumed, field_name) = name(&input[1..], config)?;
        // return field_name.and_then(|nm| {
        let rest = input
            .get((1 + consumed)..)
            .ok_or("sig_name: out of bounds".to_string())?;
        return scope(rest, config).map(|sc| format!("{}{}", render_scope(sc), field_name));
        // });
    } else if input.starts_with("R") {
        let type_names = read_type_names(&input[1..], config)?;
        return Ok(type_names.1.join(", "));
    } else if input.starts_with("K") {
        // TODO: basically the same as D case below
        let after_tag = &input[1..];
        let (consumed, nm) = name(after_tag, config)?;

        let after_name = after_tag
            .get(consumed..)
            .ok_or("sig_name: out of bounds".to_string())?;
        let (_, type_names) = read_type_names(after_name, config)?;

        let signature = match type_names.len() {
            1 => format!("{}: {}", nm, type_names.join(",")),
            // n - 2 underflows for an empty list; a real signature carries at
            // least a return type.
            0 => return Err("sig_name: missing type names".to_string()),
            n => format!(
                "{}({}): {}",
                nm,
                type_names[0..n - 2].join(","),
                type_names.get(n - 1).unwrap_or(&"???".to_string())
            ),
        };

        return Ok(signature);
    } else if input.starts_with("P") {
        // TODO: basically the same as D case below
        let after_tag = &input[1..];
        let (consumed, nm) = name(after_tag, config)?;

        let after_name = after_tag
            .get(consumed..)
            .ok_or("sig_name: out of bounds".to_string())?;
        let (_, type_names) = read_type_names(after_name, config)?;

        let signature = match type_names.len() {
            1 => format!("{}: {}", nm, type_names.join(",")),
            // n - 2 underflows for an empty list; a real signature carries at
            // least a return type.
            0 => return Err("sig_name: missing type names".to_string()),
            n => format!(
                "{}({}): {}",
                nm,
                type_names[0..n - 2].join(","),
                type_names.get(n - 1).unwrap_or(&"???".to_string())
            ),
        };

        return Ok(signature);
    } else if input.starts_with("D") {
        let after_tag = &input[1..];
        let (consumed, nm) = name(after_tag, config)?;

        let after_name = after_tag
            .get(consumed..)
            .ok_or("sig_name: out of bounds".to_string())?;
        let (consumed, type_names) = read_type_names(after_name, config)?;

        // The D tag expects one delimiter byte after the type list.
        let after_types = after_name
            .get(consumed + 1..)
            .ok_or("sig_name: truncated type name list".to_string())?;
        config.log_name(
            "sig_name:D",
            format!("type_names: {type_names:?}, after: {after_types}").as_str(),
        );
        let sc = scope(after_types, config)?;

        let signature = match type_names.len() {
            1 => format!("{}{}: {}", render_scope(sc), nm, type_names.join(",")),
            // n - 1 underflows for an empty list.
            0 => return Err("sig_name: missing type names".to_string()),
            n => format!(
                "{}{}({}): {}",
                render_scope(sc),
                nm,
                type_names[0..n - 1].join(","),
                type_names.get(n - 1).unwrap_or(&"???".to_string())
            ),
        };

        return Ok(signature);
    } else {
        return Err(format!(
            "sig_name: expected to start with F/R/D/P/C/G/K/I, {}",
            &input
        )
        .to_string());
    }
}

fn read_type_names(input: &str, config: &DemanglingConfig) -> ParsingResult<(usize, Vec<String>)> {
    let mut pos = 0;
    let mut result = Vec::new();
    loop {
        // A type name may report a consumed length reaching past the input,
        // and the scan must end at the `E` terminator before that happens.
        let Some(rest) = input.get(pos..) else {
            return Err("type names: out of bounds".to_string());
        };
        if rest.starts_with("E") {
            break;
        }
        let (consumed, nm) = type_name(rest, config)?;
        if consumed == 0 {
            return Err("type names: no progress".to_string());
        }
        result.push(nm);
        pos += consumed;
    }

    return Ok((pos, result));
}

fn scala_root_name(name: &str, config: &DemanglingConfig) -> String {
    if !config.collapse_scala_names {
        return format!("scala.{name}");
    } else {
        return name.to_string();
    };
}

fn common_type_name(name: String, config: &DemanglingConfig) -> String {
    if !config.collapse_scala_names {
        return name;
    } else {
        let immut = "scala.collection.immutable.";

        if name == "java.lang.Object" {
            return "Object".to_string();
        } else if name == "java.lang.String" {
            return "String".to_string();
        } else if name == "java.lang.Throwable" {
            return "Throwable".to_string();
        } else if name.starts_with(immut) {
            return name.strip_prefix(immut).unwrap_or(&name).to_string();
            // return "Throwable".to_string();
        } else {
            return name;
        }
    }
}

// <type-name> ::=
//     v                              // c vararg
//     R _                            // c pointer type-name
//     R <type-name>+ E               // c function type-name
//     S <type-name>+ E               // c anonymous struct type-name
//     A <type-name> <number> _       // c array type-name
//     <integer-type-name>            // signed integer type-name
//     <integer-type-name> ::=
//         b                              // scala.Byte
//         s                              // scala.Short
//         i                              // scala.Int
//         j                              // scala.Long
//     z                              // scala.Boolean
//     c                              // scala.Char
//     f                              // scala.Float
//     d                              // scala.Double
//     u                              // scala.Unit
//     l                              // scala.Null
//     n                              // scala.Nothing
//     L <nullable-type-name>         // nullable type-name
//     A <type-name> _                // nonnull array type-name
//     X <name>                       // nonnull exact class type-name
//     <name>                         // nonnull class type-name
fn type_name(input: &str, config: &DemanglingConfig) -> ParsingResult<(usize, String)> {
    let mut chars = input.chars();
    config.log(format!("type_name: {input}").as_str());

    let scala_root_namer = |name: &str| scala_root_name(name, config);
    let common_type_namer = |name: String| common_type_name(name, config);

    let result = match chars.next() {
        Some('v') => Ok((1, "<c vararg>".to_string())),
        Some('z') => Ok((1, scala_root_namer("Boolean"))),
        Some('c') => Ok((1, scala_root_namer("Char"))),
        Some('f') => Ok((1, scala_root_namer("Float"))),
        Some('d') => Ok((1, scala_root_namer("Double"))),
        Some('u') => Ok((1, scala_root_namer("Unit"))),
        Some('l') => Ok((1, scala_root_namer("Null"))),
        Some('n') => Ok((1, scala_root_namer("Nothing"))),
        Some('b') => Ok((1, scala_root_namer("Byte"))),
        Some('s') => Ok((1, scala_root_namer("Short"))),
        Some('i') => Ok((1, scala_root_namer("Int"))),
        Some('j') => Ok((1, scala_root_namer("Long"))),

        Some('R') => match chars.next() {
            Some('_') => Ok((2, "<c pointer>".to_string())),
            Some(c) => Err(format!("type_name: after R expected _, got `{c}` instead").to_string()),
            None => Err("type_name: unexpected end of input".to_string()),
        },
        Some('L') => {
            let (consumed, type_name) = nullable_type_name(&input[1..], config)?;
            Ok((consumed + 1, common_type_namer(type_name)))
        }
        Some('A') => {
            let (consumed, tn) = type_name(&input[1..], config)?;
            let after_type_name = input
                .get(1 + consumed..)
                .ok_or("type_name: out of bounds".to_string())?;
            let num = number(after_type_name);
            Ok((
                consumed + num + 1, /* "_" at the end */
                format!("CArray[{}]", tn),
            ))
        }
        Some('X') => {
            let (consumed, class_type_name) = name(&input[1..], config)?;
            Ok((consumed + 1, class_type_name))
        }
        Some(other) => Err(format!("type_name: unexpected start character `{other}`").to_string()),
        None => Err("type_name: unexpected end of input".to_string()),
    };

    return result;
}

fn number(input: &str) -> usize {
    return input.chars().take_while(|c| c.is_digit(10)).count();
}

fn nullable_type_name(input: &str, config: &DemanglingConfig) -> ParsingResult<(usize, String)> {
    let mut chars = input.chars();

    match chars.next() {
        Some('A') => {
            let (consumed, ar) = type_name(&input[1..], config)?;
            return Ok((consumed + 2, format!("Array[{}]", ar)));
        }
        Some('X') => {
            let (consumed, n) = name(input, config)?;

            return Ok((consumed + 1, n));
        }
        Some(d) if d.is_digit(10) => {
            return name(input, config);
        }
        Some(a) => return Err(format!("nullable_type_name: unexpected start `{a}`")),
        None => return Err("nullable_type_name: unexpected end of input".to_string()),
    };
}

enum Scope {
    Public,
    PublicStatic,
    Private(String),
    PrivateStatic(String),
}

fn render_scope(scope: Scope) -> String {
    return match scope {
        Scope::Public => "".to_string(),
        Scope::PublicStatic => "".to_string(),
        Scope::Private(inn) => format!("<private[{}]>", inn),
        Scope::PrivateStatic(inn) => format!("<private[{}]>", inn),
    };
}

// <scope> ::=
//     P <defn-name>                  // private to defn-name
//     O                              // public
fn scope(input: &str, config: &DemanglingConfig) -> Result<Scope, String> {
    config.log_name("scope", input);
    if input.starts_with("O") {
        return Ok(Scope::Public);
    } else if input.starts_with("o") {
        return Ok(Scope::PublicStatic);
    } else if input.starts_with("P") {
        return defn_name(&input[1..], config).map(|i| return Scope::Private(i));
    } else if input.starts_with("p") {
        return defn_name(&input[1..], config).map(|i| return Scope::PrivateStatic(i));
    } else {
        return Err(format!("scope: cannot read `{}`", input).to_string());
    }
}

fn name(input: &str, config: &DemanglingConfig) -> ParsingResult<(usize, String)> {
    //println!("name: {}", input);
    config.log_name("name", input);
    let mut number_end: usize = 0;
    for c in input.chars() {
        if c.is_digit(10) {
            number_end += 1
        } else {
            break;
        }
    }
    if number_end >= 1 {
        let (length, rest) = input.split_at(number_end);

        match usize::from_str_radix(length, 10) {
            Ok(res) => {
                if rest.starts_with("-") {
                    match rest.get(1..(1 + res)) {
                        Some(name) => {
                            return Ok((length.len() + 1 + res, name.to_string()));
                        }
                        None => return Err("name: invalid length".to_string()),
                    }
                } else {
                    match rest.get(0..res) {
                        Some(name) => {
                            return Ok((length.len() + res, name.to_string()));
                        }
                        None => return Err("name: invalid length".to_string()),
                    }
                }
            }
            Err(_) => {
                return Err("name: invalid length".to_string());
            }
        }
    } else {
        return Err(format!("name: invalid input `{}`", input.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use crate::demangle;

    fn run(s: &str) -> String {
        return demangle(s, &Default::default()).unwrap();
    }

    fn run_raw(s: &str) -> String {
        return demangle(
            s,
            &crate::DemanglingConfig {
                collapse_scala_names: false,
                ..Default::default()
            },
        )
        .unwrap();
    }

    #[test]
    fn it_works() {
        assert_eq!(run("_ST10__dispatch"), "__dispatch");
        assert_eq!(
            run_raw("_SM42sttp.model.headers.CacheDirective$MinFreshD12productArityiEO"),
            "sttp.model.headers.CacheDirective$MinFresh.productArity: scala.Int"
        );
        assert_eq!(
            run("_SM42sttp.model.headers.CacheDirective$MinFreshD12productArityiEO"),
            "sttp.model.headers.CacheDirective$MinFresh.productArity: Int"
        );

        assert_eq!(run_raw("_SM42scala.scalanative.runtime.SymbolFormatter$D10inBounds$1L32scala.scalanative.unsigned.ULongizEPT42scala.scalanative.runtime.SymbolFormatter$"), 
            "scala.scalanative.runtime.SymbolFormatter$.<private[scala.scalanative.runtime.SymbolFormatter$]>inBounds$1(scala.scalanative.unsigned.ULong,scala.Int): scala.Boolean");

        assert_eq!(run("_SM42scala.scalanative.runtime.SymbolFormatter$D10inBounds$1L32scala.scalanative.unsigned.ULongizEPT42scala.scalanative.runtime.SymbolFormatter$"), 
            "scala.scalanative.runtime.SymbolFormatter$.<private[scala.scalanative.runtime.SymbolFormatter$]>inBounds$1(scala.scalanative.unsigned.ULong,Int): Boolean");

        assert_eq!(run_raw("_SM41scalaboot.template.scalatemplate$package$D10$anonfun$3L26scalaboot.template.ContextL15scala.Function1L23java.lang.StringBuilderL31scalaboot.template.UnsafeCursorL23scalaboot.template.MoveuEPT41scalaboot.template.scalatemplate$package$"), 
            "scalaboot.template.scalatemplate$package$.<private[scalaboot.template.scalatemplate$package$]>$anonfun$3(scalaboot.template.Context,scala.Function1,java.lang.StringBuilder,scalaboot.template.UnsafeCursor,scalaboot.template.Move): scala.Unit");

        assert_eq!(run("_SM41scalaboot.template.scalatemplate$package$D10$anonfun$3L26scalaboot.template.ContextL15scala.Function1L23java.lang.StringBuilderL31scalaboot.template.UnsafeCursorL23scalaboot.template.MoveuEPT41scalaboot.template.scalatemplate$package$"), 
            "scalaboot.template.scalatemplate$package$.<private[scalaboot.template.scalatemplate$package$]>$anonfun$3(scalaboot.template.Context,scala.Function1,java.lang.StringBuilder,scalaboot.template.UnsafeCursor,scalaboot.template.Move): Unit");

        assert_eq!(run("_SM33scala.scalanative.unsafe.package$D11fromCStringL28scala.scalanative.unsafe.PtrL24java.nio.charset.CharsetL16java.lang.StringEO"), "scala.scalanative.unsafe.package$.fromCString(scala.scalanative.unsafe.Ptr,java.nio.charset.Charset): String");

        assert_eq!(
            run("_SM17java.lang.IntegerD7compareiiiEo"),
            "java.lang.Integer.compare(Int,Int): Int"
        );

        assert_eq!(
            run("_SM38scala.scalanative.junit.JUnitFrameworkIE"),
            "scala.scalanative.junit.JUnitFramework.<clinit>"
        );

        assert_eq!(run("_SM10fansi.TrieD17$init$$$anonfun$5cLAL10fansi.Trie_L12scala.Tuple2uEpT10fansi.Trie"), "fansi.Trie.<private[fansi.Trie]>$init$$$anonfun$5(Char,Array[fansi.Trie],scala.Tuple2): Unit")
    }
}
