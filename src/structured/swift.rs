//! AST-backed Swift structure: walks the vendored demangler's node-tree dump
//! (see `multi_demangle_swift_dump`) to derive the entity kind and the
//! namespace/name path from the demangler's own parse tree instead of
//! guessing from rendered text.
//!
//! The dump is an indented tree of `kind=NodeKind, text="..."` lines. Swift
//! nodes place a function's declaration context (its module and the nominal
//! types it belongs to) as *sibling* children preceding the function's own
//! identifier, so the walker scans an entity's children in order: context
//! nodes extend the namespace, the first `Identifier` outside a type subtree
//! is the entity's name, and the `Type` subtrees that follow are signature.

use crate::DemangledKind;

/// One node of the demangler's dump tree.
#[derive(Debug)]
struct Node {
    kind: String,
    text: Option<String>,
    children: Vec<Node>,
}

/// The structured fields derivable from the dump: entity kind plus the
/// declaration path (outermost first) and the leaf name.
pub(super) struct SwiftDump {
    pub kind: DemangledKind,
    pub namespace: Vec<String>,
    pub name: String,
}

/// Nominal declaration nodes whose identifier extends the namespace path.
const NOMINAL_KINDS: &[&str] = &[
    "Class",
    "Structure",
    "Enum",
    "Protocol",
    "TypeAlias",
    "OtherNominalType",
];

/// `BoundGeneric*` specializations of the nominal nodes above.
const BOUND_GENERIC_KINDS: &[&str] = &[
    "BoundGenericClass",
    "BoundGenericEnum",
    "BoundGenericStructure",
    "BoundGenericProtocol",
    "BoundGenericOtherNominalType",
    "BoundGenericTypeAlias",
];

/// Accessor nodes wrap the variable (or function) they access.
const ACCESSOR_KINDS: &[&str] = &[
    "Getter",
    "GlobalGetter",
    "Setter",
    "MaterializeForSet",
    "ModifyAccessor",
    "Modify2Accessor",
    "ReadAccessor",
    "Read2Accessor",
    "WillSet",
    "DidSet",
    "OwningAddressor",
    "OwningMutableAddressor",
    "NativeOwningAddressor",
    "NativeOwningMutableAddressor",
    "UnsafeAddressor",
    "UnsafeMutableAddressor",
    "InitAccessor",
    "BorrowAccessor",
    "MutateAccessor",
];

/// Parses the node-tree dump and extracts the entity's kind and declaration
/// path. Returns `None` when the dump does not describe a recognized
/// entity; the caller falls back to the text-derived extraction then.
pub(super) fn walk_dump(dump: &str) -> Option<SwiftDump> {
    let root = parse_dump(dump)?;
    if root.kind != "Global" {
        // Type symbols and conformance roots describe no function entity.
        return None;
    }
    let mut walker = Walker {
        namespace: Vec::new(),
        in_nominal: false,
    };
    let mut found = None;
    for child in &root.children {
        if let Some(entity) = walker.entity(child) {
            found = Some(entity);
            break;
        }
    }
    let (kind, name) = found?;
    Some(SwiftDump {
        kind,
        namespace: walker.namespace,
        name,
    })
}

/// Tracks the declaration context while scanning an entity.
struct Walker {
    namespace: Vec<String>,
    in_nominal: bool,
}

impl Walker {
    /// Classifies one entity node, scanning its children in order for the
    /// declaration context and the entity's own name.
    fn entity(&mut self, node: &Node) -> Option<(DemangledKind, String)> {
        let default_kind = match node.kind.as_str() {
            // Closures carry no identifier of their own (their rendered
            // name, "closure #N in ...", is text-derived), and the nodes
            // inside them describe what they initialize, not their
            // namespace.
            "ExplicitClosure" | "ImplicitClosure" => {
                return Some((DemangledKind::Closure, String::new()));
            }
            // Accessors wrap the entity they access; the wrapped node
            // provides the name and context, the accessor makes it a method.
            kind if ACCESSOR_KINDS.contains(&kind) => {
                let child = node.children.first()?;
                let (_, name) = self.entity(child)?;
                return Some((DemangledKind::Method, name));
            }
            "Function" | "ThinFunctionType" | "CoroutineContinuationPrototype" => {
                DemangledKind::Function
            }
            "Constructor" => return Some((DemangledKind::Method, "init".to_string())),
            "Allocator" | "Initializer" | "IVarInitializer" | "DefaultArgumentInitializer" => {
                return Some((DemangledKind::Method, "init".to_string()));
            }
            "Destructor" => return Some((DemangledKind::Method, "deinit".to_string())),
            "Deallocator" | "IVarDestroyer" | "IsolatedDeallocator" => {
                return Some((DemangledKind::Method, "deinit".to_string()));
            }
            "Subscript" => {
                return Some((DemangledKind::Method, "subscript".to_string()));
            }
            "Variable" => DemangledKind::StaticVariable,
            // Specialization markers and attributes wrap the real entity.
            "FunctionSignatureSpecialization"
            | "GenericSpecialization"
            | "GenericSpecializationInResilienceDomain"
            | "GenericPartialSpecialization"
            | "GenericPartialSpecializationNotReAbstracted"
            | "SpecializationPassID"
            | "IsSerialized"
            | "Static"
            | "ObjCAttribute"
            | "DynamicAttribute"
            | "AccessorFunctionReference"
            | "MergedFunction"
            | "BackDeploymentThunk"
            | "BackDeploymentFallback" => {
                // Some wrappers carry sibling parameter nodes before the
                // wrapped entity, so every child is tried.
                for child in &node.children {
                    if let Some(entity) = self.entity(child) {
                        return Some(entity);
                    }
                }
                return None;
            }
            // Type-relationship descriptors: not functions, so keep an
            // explicit Other kind instead of the text heuristic's guess.
            "AssociatedTypeDescriptor" | "ProtocolDescriptor" => {
                let mut name = None;
                for child in &node.children {
                    match child.kind.as_str() {
                        "Module" => {
                            if let Some(text) = &child.text {
                                self.namespace.push(text.clone());
                            }
                        }
                        kind if NOMINAL_KINDS.contains(&kind) => {
                            self.push_context(child);
                            self.in_nominal = true;
                        }
                        "DependentAssociatedTypeRef" => {
                            name = first_identifier_text(child);
                        }
                        "Identifier" => {
                            name = child.text.clone();
                            break;
                        }
                        _ => break,
                    }
                }
                return Some((
                    DemangledKind::Other(node.kind.to_string()),
                    name.unwrap_or_default(),
                ));
            }
            _ => return None,
        };

        let mut name: Option<String> = None;
        for child in &node.children {
            match child.kind.as_str() {
                "Module" => {
                    if let Some(text) = &child.text {
                        self.namespace.push(text.clone());
                    }
                }
                kind if NOMINAL_KINDS.contains(&kind) => {
                    self.push_context(child);
                    self.in_nominal = true;
                }
                kind if BOUND_GENERIC_KINDS.contains(&kind) => {
                    // The first child is the unspecialized declaration; the
                    // second is the type list.
                    if let Some(declaration) = child.children.first() {
                        self.push_context(declaration);
                        self.in_nominal = true;
                    }
                }
                "Identifier" => {
                    // The first identifier outside a type subtree is the
                    // entity's own name; everything after it is signature.
                    name = child.text.clone();
                    break;
                }
                // Private discriminators and local indices are not part of
                // the readable path.
                "PrivateDeclName" | "LocalDeclName" | "Suffix" | "Number" => {}
                // The signature begins here.
                _ => break,
            }
        }
        let kind = match default_kind {
            // A function whose declaration context contains a nominal type
            // is a method; the context scan above updated `in_nominal`.
            DemangledKind::Function
                if node.kind == "Function"
                    || node.kind == "ThinFunctionType"
                    || node.kind == "CoroutineContinuationPrototype" =>
            {
                if self.in_nominal {
                    DemangledKind::Method
                } else {
                    DemangledKind::Function
                }
            }
            other => other,
        };
        Some((kind, name.unwrap_or_default()))
    }

    /// Extends the namespace with the declaration context inside a nominal
    /// node: `[Module, Identifier]`, with nested nominal contexts in
    /// between.
    fn push_context(&mut self, node: &Node) {
        for child in &node.children {
            match child.kind.as_str() {
                "Module" => {
                    if let Some(text) = &child.text {
                        self.namespace.push(text.clone());
                    }
                }
                kind if NOMINAL_KINDS.contains(&kind) => self.push_context(child),
                "Identifier" => {
                    if let Some(text) = &child.text {
                        self.namespace.push(text.clone());
                    }
                    return;
                }
                _ => {}
            }
        }
    }
}

/// The text of the first `Identifier` node in the subtree, depth-first.
fn first_identifier_text(node: &Node) -> Option<String> {
    if node.kind == "Identifier" {
        return node.text.clone();
    }
    for child in &node.children {
        if let Some(text) = first_identifier_text(child) {
            return Some(text);
        }
    }
    None
}

/// Parses the indented `kind=..., text="..."` lines into a tree. Each
/// node's parent is the preceding node with a shallower indent.
fn parse_dump(dump: &str) -> Option<Node> {
    let mut stack: Vec<(usize, Node)> = Vec::new();
    for line in dump.lines() {
        let depth = (line.len() - line.trim_start_matches(' ').len()) / 2;
        let line = line.trim();
        let Some(rest) = line.strip_prefix("kind=") else {
            continue;
        };
        let (kind, text) = match rest.find(", text=\"") {
            Some(pos) => {
                let text = rest[pos + 8..]
                    .strip_suffix('"')
                    .unwrap_or(&rest[pos + 8..]);
                (rest[..pos].to_string(), Some(text.to_string()))
            }
            None => (rest.to_string(), None),
        };
        // Close out every open node at this depth or deeper, attaching each
        // to its parent (the new top of the stack).
        while let Some((top_depth, _)) = stack.last() {
            if *top_depth < depth {
                break;
            }
            let (_, child) = stack.pop().expect("stack checked non-empty");
            if let Some((_, parent)) = stack.last_mut() {
                parent.children.push(child);
            }
        }
        stack.push((
            depth,
            Node {
                kind,
                text,
                children: Vec::new(),
            },
        ));
    }
    // Attach the remaining chain up to the root.
    while stack.len() > 1 {
        let (_, child) = stack.pop().expect("stack checked non-empty");
        stack
            .last_mut()
            .expect("stack checked non-empty")
            .1
            .children
            .push(child);
    }
    stack.into_iter().next().map(|(_, root)| root)
}
