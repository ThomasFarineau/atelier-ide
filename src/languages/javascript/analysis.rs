//! Static analysis of a JavaScript document with tree-sitter: syntax errors, the declarations
//! visible at each position (scopes) and a light inference of the type of an expression.

use super::builtins;
use crate::languages::{Diagnostic, Severity};
use std::cell::Cell;
use std::ops::Range;
use tree_sitter::{Node, Parser, Tree};

/// The flavour of the language a document is written in. The syntax trees differ, the rest of
/// the analysis is shared.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Dialect {
    #[default]
    Js,
    Ts,
    Tsx,
}

impl Dialect {
    pub fn is_ts(self) -> bool {
        self != Dialect::Js
    }

    /// The dialect of a document: from its extension, else from the language of the editor.
    pub fn of(path: Option<&std::path::Path>, language: &str) -> Self {
        let extension = path
            .and_then(|p| p.extension())
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        match extension.as_deref() {
            Some("tsx") => Dialect::Tsx,
            Some("ts" | "mts" | "cts") => Dialect::Ts,
            Some(_) => Dialect::Js,
            None if language == "ts" => Dialect::Ts,
            None => Dialect::Js,
        }
    }
}

thread_local! {
    static DIALECT: Cell<Dialect> = const { Cell::new(Dialect::Js) };
}

/// Puts the analysis of this thread in a dialect until the guard is dropped. The analysis
/// functions take the text only: the dialect travels here, so that they stay the same for
/// JavaScript and TypeScript.
pub struct DialectGuard(Dialect);

impl Drop for DialectGuard {
    fn drop(&mut self) {
        DIALECT.with(|d| d.set(self.0));
    }
}

pub fn use_dialect(dialect: Dialect) -> DialectGuard {
    DialectGuard(DIALECT.with(|d| d.replace(dialect)))
}

pub fn dialect() -> Dialect {
    DIALECT.with(Cell::get)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DeclKind {
    #[default]
    Var,
    Interface,
    TypeAlias,
    Enum,
    Namespace,
    Let,
    Const,
    Function,
    Class,
    Param,
    Import,
    Catch,
}

#[derive(Clone, Debug, Default)]
pub struct Decl {
    pub name: String,
    pub kind: DeclKind,
    pub name_range: Range<usize>,
    /// The declaration as a whole (statement, function, class), for the signature and JSDoc.
    pub node: Range<usize>,
    /// The initial value or default value, when the name is bound to it directly.
    pub value: Option<Range<usize>>,
    /// For an import: the module and the imported name (`default`, `*` or the name).
    pub import: Option<(String, String)>,
    /// For a parameter: the function it belongs to.
    pub owner: Option<Range<usize>>,
    /// The type annotation (`: string`), in TypeScript.
    pub annot: Option<Range<usize>>,
    /// For the variable of `for (const x of items)`: the `items`.
    pub element_of: Option<Range<usize>>,
}

#[derive(Debug)]
pub struct Scope {
    pub range: Range<usize>,
    pub parent: Option<usize>,
    pub is_function: bool,
    pub decls: Vec<Decl>,
}

/// What an expression is, as far as the analysis can tell.
#[derive(Clone, PartialEq, Debug, Default)]
pub enum Ty {
    #[default]
    Unknown,
    Str,
    Num,
    Bool,
    Null,
    Array,
    /// An array whose elements have this type (TypeScript, or a literal).
    ArrayOf(Box<Ty>),
    /// A promise of this type (`Promise<User>`).
    PromiseOf(Box<Ty>),
    /// An interface or an object type, by its node range.
    Interface(Range<usize>),
    /// An enum (its constants), by its node range.
    Enum(Range<usize>),
    /// Several types at once: `A & B`.
    Intersection(Vec<Ty>),
    /// A function, by its node range.
    Function(Range<usize>),
    /// An object literal, by its node range.
    Object(Range<usize>),
    /// A class (its static members), by its node range.
    Class(Range<usize>),
    /// An instance of a class.
    Instance(Range<usize>),
    /// An instance of a built-in type (`Map`, `Promise`…), the key of `builtins::members`.
    Builtin(&'static str),
    /// A global object or constructor (`Math`, `Object`…), the key of `builtins::statics`.
    Static(&'static str),
    /// A required module.
    Module(String),
}

/// A member of a type, for completion and hover.
#[derive(Clone, Debug, Default)]
pub struct Prop {
    pub name: String,
    pub method: bool,
    /// Signature or type.
    pub detail: String,
    pub doc: String,
    pub ty: Ty,
    /// Where the member is declared, for a local type.
    pub def: Option<Range<usize>>,
}

pub struct Analysis {
    pub text: String,
    pub tree: Tree,
    pub scopes: Vec<Scope>,
}

const MAX_DEPTH: usize = 10;

fn text_of<'a>(node: Node, src: &'a str) -> &'a str {
    &src[node.byte_range()]
}

pub(crate) fn slice<'a>(node: Node, src: &'a str) -> &'a str {
    &src[node.byte_range()]
}

fn parse_tree(text: &str) -> Option<Tree> {
    let mut parser = Parser::new();
    let language = match dialect() {
        Dialect::Js => tree_sitter_javascript::LANGUAGE.into(),
        Dialect::Ts => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        Dialect::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
    };
    parser.set_language(&language).ok()?;
    parser.parse(text, None)
}

/// Syntax errors of a document, one diagnostic per error found.
pub fn syntax_errors(text: &str) -> Vec<Diagnostic> {
    let Some(tree) = parse_tree(text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if tree.root_node().has_error() {
        collect_errors(tree.root_node(), text, &mut out);
    }
    out
}

fn collect_errors(node: Node, src: &str, out: &mut Vec<Diagnostic>) {
    const LIMIT: usize = 50;
    if out.len() >= LIMIT {
        return;
    }
    if node.is_missing() {
        let start = node.start_byte();
        let range = if start > 0 && src.as_bytes()[start - 1] != b'\n' {
            let mut from = start - 1;
            while !src.is_char_boundary(from) {
                from -= 1;
            }
            from..start
        } else {
            let mut end = (start + 1).min(src.len());
            while !src.is_char_boundary(end) {
                end += 1;
            }
            start..end.min(src.len())
        };
        let kind = node.kind();
        let message = match kind {
            "identifier" | "property_identifier" => "Identifiant attendu".to_string(),
            _ => format!("« {kind} » attendu"),
        };
        out.push(Diagnostic {
            range,
            severity: Severity::Error,
            message,
            source: "syntaxe".into(),
        });
    } else if node.is_error() {
        // The error node spans everything the parser gave up on: point at its first token.
        let mut range = node.byte_range();
        if let Some(first) = node.child(0)
            && first.byte_range().len() < range.len()
        {
            range = first.byte_range();
        }
        if range.len() > 40 {
            let mut end = range.start + 40;
            while !src.is_char_boundary(end) {
                end -= 1;
            }
            range.end = end;
        }
        let shown = src[range.clone()].trim();
        let message = if shown.is_empty() || shown.contains('\n') {
            "Syntaxe invalide".to_string()
        } else {
            format!("Syntaxe inattendue « {shown} »")
        };
        out.push(Diagnostic {
            range,
            severity: Severity::Error,
            message,
            source: "syntaxe".into(),
        });
    } else if node.has_error() {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.has_error() || child.is_missing() {
                collect_errors(child, src, out);
            }
        }
    }
}

struct Builder<'a> {
    src: &'a str,
    scopes: Vec<Scope>,
}

impl Builder<'_> {
    fn scope(&mut self, range: Range<usize>, parent: Option<usize>, is_function: bool) -> usize {
        self.scopes.push(Scope {
            range,
            parent,
            is_function,
            decls: Vec::new(),
        });
        self.scopes.len() - 1
    }

    fn function_scope(&self, mut scope: usize) -> usize {
        while !self.scopes[scope].is_function {
            match self.scopes[scope].parent {
                Some(parent) => scope = parent,
                None => break,
            }
        }
        scope
    }

    fn declare(&mut self, scope: usize, decl: Decl) {
        // A `var` declared twice, or a function redeclared, keeps one entry.
        let decls = &mut self.scopes[scope].decls;
        if !decls
            .iter()
            .any(|d| d.name == decl.name && d.kind == decl.kind && d.name_range == decl.name_range)
        {
            decls.push(decl);
        }
    }

    /// Declares every name bound by a pattern (`a`, `{ a, b: c }`, `[a, ...rest]`, `a = 1`).
    fn bind(
        &mut self,
        pattern: Node,
        scope: usize,
        kind: DeclKind,
        node: Range<usize>,
        value: Option<Range<usize>>,
        owner: Option<Range<usize>>,
    ) {
        self.bind_typed(pattern, scope, kind, node, value, owner, None);
    }

    #[allow(clippy::too_many_arguments)]
    fn bind_typed(
        &mut self,
        pattern: Node,
        scope: usize,
        kind: DeclKind,
        node: Range<usize>,
        value: Option<Range<usize>>,
        owner: Option<Range<usize>>,
        annot: Option<Range<usize>>,
    ) {
        match pattern.kind() {
            "identifier" | "shorthand_property_identifier_pattern" => {
                let decl = Decl {
                    name: slice(pattern, self.src).to_string(),
                    kind,
                    name_range: pattern.byte_range(),
                    node,
                    value,
                    import: None,
                    owner,
                    annot,
                    ..Default::default()
                };
                self.declare(scope, decl);
            }
            // TypeScript: `name: Type`, `name?: Type`, `name = default`
            "required_parameter" | "optional_parameter" => {
                let Some(name) = pattern
                    .child_by_field_name("pattern")
                    .or_else(|| pattern.child_by_field_name("name"))
                else {
                    return;
                };
                let default = pattern.child_by_field_name("value").map(|v| v.byte_range());
                let annot = pattern.child_by_field_name("type").map(|t| t.byte_range());
                let value = if name.kind() == "identifier" {
                    default
                } else {
                    None
                };
                self.bind_typed(name, scope, kind, node, value, owner, annot);
            }
            "assignment_pattern" | "object_assignment_pattern" => {
                if let Some(left) = pattern.child_by_field_name("left") {
                    let default = pattern.child_by_field_name("right").map(|r| r.byte_range());
                    let value = if left.kind() == "identifier" {
                        default
                    } else {
                        None
                    };
                    self.bind_typed(left, scope, kind, node, value, owner, annot);
                }
            }
            "pair_pattern" => {
                if let Some(value) = pattern.child_by_field_name("value") {
                    self.bind(value, scope, kind, node, None, owner);
                }
            }
            "object_pattern"
            | "array_pattern"
            | "rest_pattern"
            | "formal_parameters"
            | "parenthesized_expression" => {
                let mut cursor = pattern.walk();
                for child in pattern.named_children(&mut cursor) {
                    self.bind(child, scope, kind, node.clone(), None, owner.clone());
                }
            }
            _ => {}
        }
    }

    fn params(&mut self, function: Node, scope: usize) {
        let owner = Some(function.byte_range());
        if let Some(parameters) = function.child_by_field_name("parameters") {
            let mut cursor = parameters.walk();
            for parameter in parameters.named_children(&mut cursor) {
                self.bind(
                    parameter,
                    scope,
                    DeclKind::Param,
                    function.byte_range(),
                    None,
                    owner.clone(),
                );
            }
        } else if let Some(parameter) = function.child_by_field_name("parameter") {
            self.bind(
                parameter,
                scope,
                DeclKind::Param,
                function.byte_range(),
                None,
                owner,
            );
        }
    }

    fn visit_children(&mut self, node: Node, scope: usize) {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.visit(child, scope);
        }
    }

    fn visit(&mut self, node: Node, scope: usize) {
        match node.kind() {
            "function_declaration" | "generator_function_declaration" => {
                if let Some(name) = node.child_by_field_name("name") {
                    let decl = Decl {
                        name: slice(name, self.src).to_string(),
                        kind: DeclKind::Function,
                        name_range: name.byte_range(),
                        node: node.byte_range(),
                        value: Some(node.byte_range()),
                        import: None,
                        owner: None,
                        ..Default::default()
                    };
                    // Function declarations are hoisted to the enclosing function.
                    let target = self.function_scope(scope);
                    self.declare(target, decl);
                }
                let inner = self.scope(node.byte_range(), Some(scope), true);
                self.params(node, inner);
                if let Some(body) = node.child_by_field_name("body") {
                    self.visit_children(body, inner);
                }
            }
            "function_expression"
            | "generator_function"
            | "arrow_function"
            | "method_definition" => {
                let inner = self.scope(node.byte_range(), Some(scope), true);
                if let Some(name) = node.child_by_field_name("name")
                    && matches!(node.kind(), "function_expression" | "generator_function")
                {
                    let decl = Decl {
                        name: slice(name, self.src).to_string(),
                        kind: DeclKind::Function,
                        name_range: name.byte_range(),
                        node: node.byte_range(),
                        value: Some(node.byte_range()),
                        import: None,
                        owner: None,
                        ..Default::default()
                    };
                    self.declare(inner, decl);
                }
                self.params(node, inner);
                if let Some(body) = node.child_by_field_name("body") {
                    if body.kind() == "statement_block" {
                        self.visit_children(body, inner);
                    } else {
                        self.visit(body, inner);
                    }
                }
            }
            "interface_declaration"
            | "type_alias_declaration"
            | "enum_declaration"
            | "function_signature"
            | "internal_module" => {
                let kind = match node.kind() {
                    "interface_declaration" => DeclKind::Interface,
                    "type_alias_declaration" => DeclKind::TypeAlias,
                    "enum_declaration" => DeclKind::Enum,
                    "internal_module" => DeclKind::Namespace,
                    _ => DeclKind::Function,
                };
                if let Some(name) = node.child_by_field_name("name") {
                    let decl = Decl {
                        name: slice(name, self.src).to_string(),
                        kind,
                        name_range: name.byte_range(),
                        node: node.byte_range(),
                        value: (kind == DeclKind::Function).then(|| node.byte_range()),
                        ..Default::default()
                    };
                    let target = if kind == DeclKind::Function {
                        self.function_scope(scope)
                    } else {
                        scope
                    };
                    self.declare(target, decl);
                }
                if kind == DeclKind::Namespace {
                    let inner = self.scope(node.byte_range(), Some(scope), false);
                    if let Some(body) = node.child_by_field_name("body") {
                        self.visit_children(body, inner);
                    }
                } else if kind == DeclKind::Function {
                    // Overload signatures carry parameters, not code.
                } else {
                    self.visit_children(node, scope);
                }
            }
            "class_declaration" | "abstract_class_declaration" => {
                if let Some(name) = node.child_by_field_name("name") {
                    let decl = Decl {
                        name: slice(name, self.src).to_string(),
                        kind: DeclKind::Class,
                        name_range: name.byte_range(),
                        node: node.byte_range(),
                        value: Some(node.byte_range()),
                        import: None,
                        owner: None,
                        ..Default::default()
                    };
                    self.declare(scope, decl);
                }
                self.visit_children(node, scope);
            }
            "lexical_declaration" | "variable_declaration" => {
                let keyword = node.child(0).map_or("", |c| slice(c, self.src));
                let (kind, target) = match keyword {
                    "const" => (DeclKind::Const, scope),
                    "let" => (DeclKind::Let, scope),
                    _ => (DeclKind::Var, self.function_scope(scope)),
                };
                let mut cursor = node.walk();
                for declarator in node.named_children(&mut cursor) {
                    if declarator.kind() != "variable_declarator" {
                        continue;
                    }
                    let value = declarator.child_by_field_name("value");
                    if let Some(name) = declarator.child_by_field_name("name") {
                        let direct = if name.kind() == "identifier" {
                            value.map(|v| v.byte_range())
                        } else {
                            None
                        };
                        let annot = declarator
                            .child_by_field_name("type")
                            .map(|t| t.byte_range());
                        self.bind_typed(name, target, kind, node.byte_range(), direct, None, annot);
                    }
                    if let Some(value) = value {
                        self.visit(value, scope);
                    }
                }
            }
            "import_statement" => {
                let source = node
                    .child_by_field_name("source")
                    .map(|s| slice(s, self.src).trim_matches(['"', '\'']).to_string())
                    .unwrap_or_default();
                let mut cursor = node.walk();
                for clause in node.named_children(&mut cursor) {
                    if clause.kind() == "import_clause" {
                        self.import_clause(clause, scope, &source, node.byte_range());
                    }
                }
            }
            "statement_block" | "for_statement" | "for_in_statement" | "switch_statement"
            | "class_static_block" => {
                let inner = self.scope(
                    node.byte_range(),
                    Some(scope),
                    node.kind() == "class_static_block",
                );
                if node.kind() == "for_in_statement"
                    && let Some(left) = node.child_by_field_name("left")
                {
                    let keyword = node
                        .child_by_field_name("kind")
                        .map_or("", |k| slice(k, self.src));
                    let (kind, target) = match keyword {
                        "var" => (DeclKind::Var, self.function_scope(scope)),
                        "const" => (DeclKind::Const, inner),
                        _ => (DeclKind::Let, inner),
                    };
                    self.bind(left, target, kind, node.byte_range(), None, None);
                    // `for (const x of items)`: x is an element of items.
                    let of = node
                        .child_by_field_name("operator")
                        .is_some_and(|o| slice(o, self.src) == "of");
                    if of && let Some(right) = node.child_by_field_name("right") {
                        for decl in &mut self.scopes[target].decls {
                            if decl.name_range.start >= left.start_byte()
                                && decl.name_range.end <= left.end_byte()
                                && left.kind() == "identifier"
                            {
                                decl.element_of = Some(right.byte_range());
                            }
                        }
                    }
                }
                self.visit_children(node, inner);
            }
            "catch_clause" => {
                let inner = self.scope(node.byte_range(), Some(scope), false);
                if let Some(parameter) = node.child_by_field_name("parameter") {
                    self.bind(
                        parameter,
                        inner,
                        DeclKind::Catch,
                        node.byte_range(),
                        None,
                        None,
                    );
                }
                if let Some(body) = node.child_by_field_name("body") {
                    self.visit_children(body, inner);
                }
            }
            _ => self.visit_children(node, scope),
        }
    }

    fn import_clause(&mut self, clause: Node, scope: usize, source: &str, node: Range<usize>) {
        let mut cursor = clause.walk();
        for part in clause.named_children(&mut cursor) {
            match part.kind() {
                "identifier" => self.import_name(part, scope, source, "default", &node),
                "namespace_import" => {
                    let mut inner = part.walk();
                    if let Some(name) = part
                        .named_children(&mut inner)
                        .find(|c| c.kind() == "identifier")
                    {
                        self.import_name(name, scope, source, "*", &node);
                    }
                }
                "named_imports" => {
                    let mut inner = part.walk();
                    for specifier in part.named_children(&mut inner) {
                        if specifier.kind() != "import_specifier" {
                            continue;
                        }
                        let imported = specifier
                            .child_by_field_name("name")
                            .map(|n| slice(n, self.src).to_string());
                        let local = specifier
                            .child_by_field_name("alias")
                            .or_else(|| specifier.child_by_field_name("name"));
                        if let (Some(local), Some(imported)) = (local, imported) {
                            self.import_name(local, scope, source, &imported, &node);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn import_name(
        &mut self,
        name: Node,
        scope: usize,
        source: &str,
        imported: &str,
        node: &Range<usize>,
    ) {
        let decl = Decl {
            name: slice(name, self.src).to_string(),
            kind: DeclKind::Import,
            name_range: name.byte_range(),
            node: node.clone(),
            value: None,
            import: Some((source.to_string(), imported.to_string())),
            owner: None,
            ..Default::default()
        };
        self.declare(scope, decl);
    }
}

impl Analysis {
    pub fn new(text: &str) -> Option<Self> {
        let tree = parse_tree(text)?;
        let root = tree.root_node();
        let mut builder = Builder {
            src: text,
            scopes: Vec::new(),
        };
        let module = builder.scope(0..text.len().max(root.end_byte()), None, true);
        builder.visit_children(root, module);
        Some(Self {
            text: text.to_string(),
            tree,
            scopes: builder.scopes,
        })
    }

    pub fn root(&self) -> Node<'_> {
        self.tree.root_node()
    }

    /// The node of exactly this byte range.
    pub fn node_at_range(&self, range: &Range<usize>) -> Option<Node<'_>> {
        let mut node = self
            .root()
            .descendant_for_byte_range(range.start, range.end)?;
        while node.byte_range() != *range {
            node = node.parent()?;
        }
        Some(node)
    }

    /// Scopes containing `offset`, innermost first.
    pub fn scope_chain(&self, offset: usize) -> Vec<usize> {
        let mut best: Option<usize> = None;
        for (i, scope) in self.scopes.iter().enumerate() {
            let inside = scope.range.start <= offset && offset <= scope.range.end;
            if inside && best.is_none_or(|b| self.scopes[b].range.len() >= scope.range.len()) {
                best = Some(i);
            }
        }
        let mut chain = Vec::new();
        let mut current = best;
        while let Some(i) = current {
            chain.push(i);
            current = self.scopes[i].parent;
        }
        chain
    }

    /// The declaration `name` refers to at `offset`.
    pub fn resolve(&self, name: &str, offset: usize) -> Option<&Decl> {
        self.scope_chain(offset)
            .into_iter()
            .find_map(|s| self.scopes[s].decls.iter().find(|d| d.name == name))
    }

    /// Every declaration visible at `offset`, the innermost first; a shadowed name once.
    pub fn visible(&self, offset: usize) -> Vec<&Decl> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for s in self.scope_chain(offset) {
            for decl in &self.scopes[s].decls {
                if seen.insert(decl.name.as_str()) {
                    out.push(decl);
                }
            }
        }
        out
    }

    // ---- Types ---------------------------------------------------------------------------

    pub fn ty_of(&self, node: Node, scope_at: usize) -> Ty {
        self.ty(node, scope_at, 0)
    }

    pub(crate) fn ty(&self, node: Node, at: usize, depth: usize) -> Ty {
        if depth > MAX_DEPTH {
            return Ty::Unknown;
        }
        let src = self.text.as_str();
        match node.kind() {
            "string" | "template_string" => Ty::Str,
            "number" => Ty::Num,
            "true" | "false" => Ty::Bool,
            "null" => Ty::Null,
            "array" => node
                .named_child(0)
                .map(|first| self.ty(first, at, depth + 1))
                .filter(|t| *t != Ty::Unknown)
                .map_or(Ty::Array, |element| Ty::ArrayOf(Box::new(element))),
            // `x as T` has the type T; `x satisfies T` the one of x.
            "as_expression" => {
                let last = node.named_child(node.named_child_count().saturating_sub(1) as u32);
                let declared = last
                    .filter(|t| text_of(*t, src) != "const")
                    .map_or(Ty::Unknown, |t| self.ty_from_type(t, at, depth + 1));
                if declared != Ty::Unknown {
                    declared
                } else {
                    node.named_child(0)
                        .map_or(Ty::Unknown, |inner| self.ty(inner, at, depth + 1))
                }
            }
            "satisfies_expression" => node
                .named_child(0)
                .map_or(Ty::Unknown, |inner| self.ty(inner, at, depth + 1)),
            "type_assertion" => node
                .named_child(0)
                .and_then(|arguments| arguments.named_child(0))
                .map_or(Ty::Unknown, |t| self.ty_from_type(t, at, depth + 1)),
            "non_null_expression" => node
                .named_child(0)
                .map_or(Ty::Unknown, |inner| self.ty(inner, at, depth + 1)),
            "regex" => Ty::Builtin("RegExp"),
            "object" => Ty::Object(node.byte_range()),
            "function_expression" | "arrow_function" | "generator_function" => {
                Ty::Function(node.byte_range())
            }
            "class" => Ty::Class(node.byte_range()),
            "parenthesized_expression" => node
                .named_child(0)
                .map_or(Ty::Unknown, |inner| self.ty(inner, at, depth + 1)),
            "await_expression" => match node
                .named_child(0)
                .map(|inner| self.ty(inner, at, depth + 1))
            {
                Some(Ty::PromiseOf(inner)) => *inner,
                _ => Ty::Unknown,
            },
            "binary_expression" => {
                let op = node
                    .child_by_field_name("operator")
                    .map_or("", |o| slice(o, src));
                match op {
                    "-" | "*" | "/" | "%" | "**" | "&" | "|" | "^" | "<<" | ">>" | ">>>" => Ty::Num,
                    "==" | "===" | "!=" | "!==" | "<" | ">" | "<=" | ">=" | "instanceof" | "in" => {
                        Ty::Bool
                    }
                    "+" => {
                        let left = node
                            .child_by_field_name("left")
                            .map_or(Ty::Unknown, |l| self.ty(l, at, depth + 1));
                        let right = node
                            .child_by_field_name("right")
                            .map_or(Ty::Unknown, |r| self.ty(r, at, depth + 1));
                        match (left, right) {
                            (Ty::Str, _) | (_, Ty::Str) => Ty::Str,
                            (Ty::Num, Ty::Num) => Ty::Num,
                            _ => Ty::Unknown,
                        }
                    }
                    _ => Ty::Unknown,
                }
            }
            "unary_expression" => match node.child(0).map(|c| slice(c, src)) {
                Some("!") => Ty::Bool,
                Some("typeof") => Ty::Str,
                Some("-" | "+" | "~") => Ty::Num,
                _ => Ty::Unknown,
            },
            "identifier" => self.ty_of_name(slice(node, src), at, depth),
            "this" => self.this_type(node),
            "new_expression" => {
                let Some(constructor) = node.child_by_field_name("constructor") else {
                    return Ty::Unknown;
                };
                match self.ty(constructor, at, depth + 1) {
                    Ty::Class(range) => Ty::Instance(range),
                    Ty::Static(name) => builtin_instance(name),
                    _ => Ty::Unknown,
                }
            }
            "call_expression" => self.ty_of_call(node, at, depth),
            "member_expression" => {
                let (Some(object), Some(property)) = (
                    node.child_by_field_name("object"),
                    node.child_by_field_name("property"),
                ) else {
                    return Ty::Unknown;
                };
                let object = self.ty(object, at, depth + 1);
                match self
                    .props_of(&object, at)
                    .into_iter()
                    .find(|p| p.name == slice(property, src))
                {
                    Some(p) if p.method && !matches!(p.ty, Ty::Function(_)) => {
                        Ty::Builtin("Function")
                    }
                    Some(p) => p.ty,
                    None => Ty::Unknown,
                }
            }
            "subscript_expression" => match node
                .child_by_field_name("object")
                .map(|o| self.ty(o, at, depth + 1))
            {
                Some(Ty::ArrayOf(element)) => *element,
                _ => Ty::Unknown,
            },
            "assignment_expression" => node
                .child_by_field_name("right")
                .map_or(Ty::Unknown, |r| self.ty(r, at, depth + 1)),
            "ternary_expression" => node
                .child_by_field_name("consequence")
                .map_or(Ty::Unknown, |c| self.ty(c, at, depth + 1)),
            _ => Ty::Unknown,
        }
    }

    fn ty_of_call(&self, node: Node, at: usize, depth: usize) -> Ty {
        let src = self.text.as_str();
        let Some(callee) = node.child_by_field_name("function") else {
            return Ty::Unknown;
        };
        // `require('fs')`
        if callee.kind() == "identifier"
            && slice(callee, src) == "require"
            && self.resolve("require", at).is_none()
            && let Some(arguments) = node.child_by_field_name("arguments")
            && let Some(first) = arguments.named_child(0)
            && first.kind() == "string"
        {
            return Ty::Module(slice(first, src).trim_matches(['"', '\'']).to_string());
        }
        if callee.kind() == "member_expression" {
            // A method: its own result type for a local one, the documented one for a built-in.
            let (Some(object), Some(property)) = (
                callee.child_by_field_name("object"),
                callee.child_by_field_name("property"),
            ) else {
                return Ty::Unknown;
            };
            let object = self.ty(object, at, depth + 1);
            let method = slice(property, src);
            if let Ty::ArrayOf(element) = &object {
                // The methods whose result is made of the elements.
                match method {
                    "at" | "find" | "findLast" | "pop" | "shift" => return (**element).clone(),
                    "filter" | "slice" | "concat" | "sort" | "reverse" | "toSorted"
                    | "toReversed" | "toSpliced" | "splice" | "with" => return object.clone(),
                    "map" | "flatMap" => {
                        let callback = node
                            .child_by_field_name("arguments")
                            .and_then(|a| a.named_child(0))
                            .filter(|c| {
                                matches!(c.kind(), "arrow_function" | "function_expression")
                            });
                        return match callback.map(|c| self.return_type(&c.byte_range(), depth + 1))
                        {
                            Some(result) if result != Ty::Unknown => Ty::ArrayOf(Box::new(result)),
                            _ => Ty::Array,
                        };
                    }
                    _ => {}
                }
            }
            if let (Ty::PromiseOf(inner), "then") = (&object, method) {
                let _ = inner;
                return Ty::Builtin("Promise");
            }
            return match self
                .props_of(&object, at)
                .into_iter()
                .find(|p| p.name == method && p.method)
            {
                Some(prop) => match prop.ty {
                    Ty::Function(range) => self.return_type(&range, depth),
                    other => other,
                },
                None => Ty::Unknown,
            };
        }
        match self.ty(callee, at, depth + 1) {
            Ty::Function(range) => self.return_type(&range, depth),
            Ty::Static(name) => match name {
                "String" => Ty::Str,
                "Number" => Ty::Num,
                "Boolean" => Ty::Bool,
                "Array" => Ty::Array,
                "Object" => Ty::Builtin("Object"),
                _ => Ty::Unknown,
            },
            _ => Ty::Unknown,
        }
    }

    /// Type of the first `return` of a function, outside nested functions.
    fn return_type(&self, function: &Range<usize>, depth: usize) -> Ty {
        let Some(node) = self.node_at_range(function) else {
            return Ty::Unknown;
        };
        let is_async = slice(node, &self.text).trim_start().starts_with("async");
        // A declared return type: what a call gives.
        if let Some(annotation) = node.child_by_field_name("return_type") {
            let declared = self.ty_from_type(annotation, node.start_byte(), depth + 1);
            if declared != Ty::Unknown {
                return declared;
            }
        }
        let inferred = self.inferred_return(node, depth);
        if is_async {
            return match inferred {
                Ty::Unknown => Ty::Builtin("Promise"),
                other => Ty::PromiseOf(Box::new(other)),
            };
        }
        inferred
    }

    /// The type of what a function body returns, from its first `return`.
    fn inferred_return(&self, node: Node, depth: usize) -> Ty {
        let Some(body) = node.child_by_field_name("body") else {
            return Ty::Unknown;
        };
        if body.kind() != "statement_block" {
            return self.ty(body, body.start_byte(), depth + 1);
        }
        fn find_return(node: Node) -> Option<Node> {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                match child.kind() {
                    "return_statement" => return Some(child),
                    "function_declaration"
                    | "function_expression"
                    | "arrow_function"
                    | "class_declaration"
                    | "method_definition" => {}
                    _ => {
                        if let Some(found) = find_return(child) {
                            return Some(found);
                        }
                    }
                }
            }
            None
        }
        find_return(body)
            .and_then(|r| r.named_child(0))
            .map_or(Ty::Unknown, |value| {
                self.ty(value, value.start_byte(), depth + 1)
            })
    }

    fn this_type(&self, node: Node) -> Ty {
        let mut current = node.parent();
        while let Some(parent) = current {
            match parent.kind() {
                "method_definition" | "field_definition" => {
                    if let Some(body) = parent.parent() {
                        return match body.kind() {
                            "class_body" => body
                                .parent()
                                .map_or(Ty::Unknown, |c| Ty::Instance(c.byte_range())),
                            "object" => Ty::Object(body.byte_range()),
                            _ => Ty::Unknown,
                        };
                    }
                    return Ty::Unknown;
                }
                "function_declaration"
                | "function_expression"
                | "generator_function"
                | "generator_function_declaration" => {
                    return Ty::Unknown;
                }
                _ => current = parent.parent(),
            }
        }
        Ty::Unknown
    }

    pub fn ty_of_name(&self, name: &str, at: usize, depth: usize) -> Ty {
        if depth > MAX_DEPTH {
            return Ty::Unknown;
        }
        let Some(decl) = self.resolve(name, at) else {
            return match builtins::global_type(name) {
                Some(key) => self.global_ty(key),
                None => Ty::Unknown,
            };
        };
        self.decl_ty(decl, depth)
    }

    /// A global with members or a constructor: its static side (`Math.max`, `new Map()`).
    fn global_ty(&self, key: &'static str) -> Ty {
        Ty::Static(key)
    }

    pub fn decl_ty(&self, decl: &Decl, depth: usize) -> Ty {
        // A type written in the code wins.
        if let Some(annotation) = decl.annot.as_ref().and_then(|a| self.node_at_range(a)) {
            let ty = self.ty_from_type(annotation, decl.node.start, depth + 1);
            if ty != Ty::Unknown {
                return ty;
            }
        }
        if let Some(iterable) = &decl.element_of {
            let element = self.element_ty(iterable, depth);
            if element != Ty::Unknown {
                return element;
            }
        }
        if decl.kind == DeclKind::Param
            && decl.annot.is_none()
            && let Some(ty) = self.contextual_param_ty(decl, depth)
        {
            return ty;
        }
        if let Some(ty) = self.jsdoc_type(decl) {
            return ty;
        }
        match decl.kind {
            DeclKind::Interface => self
                .node_at_range(&decl.node)
                .map_or(Ty::Unknown, |n| Ty::Interface(n.byte_range())),
            DeclKind::Enum => self
                .node_at_range(&decl.node)
                .map_or(Ty::Unknown, |n| Ty::Enum(n.byte_range())),
            DeclKind::TypeAlias | DeclKind::Namespace => Ty::Unknown,
            DeclKind::Class => self
                .node_at_range(&decl.node)
                .map_or(Ty::Unknown, |n| Ty::Class(n.byte_range())),
            DeclKind::Function => Ty::Function(decl.node.clone()),
            DeclKind::Import => match &decl.import {
                Some((module, imported)) if imported == "default" || imported == "*" => {
                    Ty::Module(module.clone())
                }
                _ => Ty::Unknown,
            },
            _ => decl
                .value
                .as_ref()
                .and_then(|range| self.node_at_range(range))
                .map_or(Ty::Unknown, |value| {
                    self.ty(value, decl.node.start, depth + 1)
                }),
        }
    }

    /// `@param {string} name` of the function a parameter belongs to, or `@type {T}` of a
    /// variable declaration.
    fn jsdoc_type(&self, decl: &Decl) -> Option<Ty> {
        let (doc_target, tag, name) = match decl.kind {
            DeclKind::Param => (decl.owner.clone()?, "@param", Some(decl.name.as_str())),
            DeclKind::Var | DeclKind::Let | DeclKind::Const => (decl.node.clone(), "@type", None),
            _ => return None,
        };
        let doc = self.jsdoc_before(&doc_target)?;
        for line in doc.lines() {
            let line = line.trim();
            let Some(rest) = line.strip_prefix(tag) else {
                continue;
            };
            let rest = rest.trim_start();
            let (type_text, tail) = rest.strip_prefix('{')?.split_once('}')?;
            if let Some(name) = name {
                let tail = tail.trim_start().trim_start_matches('[');
                if !tail.starts_with(name) {
                    continue;
                }
            }
            return Some(ty_from_name(type_text.trim()));
        }
        None
    }

    /// The `/** … */` comment right before a declaration, without the comment marks.
    pub fn jsdoc_before(&self, node_range: &Range<usize>) -> Option<String> {
        let mut node = self.node_at_range(node_range)?;
        // The comment sits before the statement: `export`, `const x = …`.
        while let Some(parent) = node.parent() {
            if matches!(
                parent.kind(),
                "export_statement" | "lexical_declaration" | "variable_declaration"
            ) {
                node = parent;
            } else {
                break;
            }
        }
        let previous = node.prev_sibling()?;
        if previous.kind() != "comment" {
            return None;
        }
        let comment = slice(previous, &self.text);
        if !comment.starts_with("/**") {
            return None;
        }
        // Only blank space may separate the comment from the declaration.
        if !self.text[previous.end_byte()..node.start_byte()]
            .trim()
            .is_empty()
        {
            return None;
        }
        let body = comment.trim_start_matches("/**").trim_end_matches("*/");
        let lines: Vec<&str> = body
            .lines()
            .map(|l| l.trim().trim_start_matches('*').trim())
            .collect();
        let text = lines.join("\n");
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_string())
    }

    // ---- Members -------------------------------------------------------------------------

    /// The members of a type.
    pub fn props_of(&self, ty: &Ty, at: usize) -> Vec<Prop> {
        let builtin = |members: &[builtins::Member], extra: &[builtins::Member]| -> Vec<Prop> {
            members
                .iter()
                .chain(extra)
                .map(|(name, sig, doc, ret)| Prop {
                    name: (*name).to_string(),
                    method: sig.starts_with('('),
                    detail: (*sig).to_string(),
                    doc: (*doc).to_string(),
                    ty: ty_from_return(ret),
                    def: None,
                })
                .collect()
        };
        match ty {
            Ty::Str => builtin(builtins::STRING, builtins::OBJECT),
            Ty::Num => builtin(builtins::NUMBER, builtins::OBJECT),
            Ty::Bool => builtin(builtins::BOOLEAN, builtins::OBJECT),
            Ty::Array | Ty::ArrayOf(_) => builtin(builtins::ARRAY, builtins::OBJECT),
            Ty::PromiseOf(_) => builtin(builtins::PROMISE, builtins::OBJECT),
            Ty::Interface(range) => {
                let mut props = self.interface_props(range, at, 0);
                props.extend(builtin(builtins::OBJECT, &[]));
                props
            }
            Ty::Enum(range) => self.enum_props(range),
            Ty::Intersection(parts) => {
                let mut props: Vec<Prop> = Vec::new();
                for part in parts {
                    for prop in self.props_of(part, at) {
                        if !props.iter().any(|p| p.name == prop.name) {
                            props.push(prop);
                        }
                    }
                }
                props
            }
            Ty::Function(_) => builtin(builtins::FUNCTION, builtins::OBJECT),
            Ty::Builtin(key) => builtin(builtins::members(key), builtins::OBJECT),
            Ty::Static(key) => builtin(builtins::statics(key), builtins::FUNCTION),
            Ty::Object(range) => {
                let mut props = self.object_props(range, at, 0);
                props.extend(builtin(builtins::OBJECT, &[]));
                props
            }
            Ty::Class(range) => {
                let mut props = self.class_props(range, true, at, 0);
                props.extend(builtin(builtins::FUNCTION, &[]));
                props
            }
            Ty::Instance(range) => {
                let mut props = self.class_props(range, false, at, 0);
                props.extend(builtin(builtins::OBJECT, &[]));
                props
            }
            Ty::Module(name) => module_props(name),
            Ty::Null | Ty::Unknown => Vec::new(),
        }
    }

    fn object_props(&self, range: &Range<usize>, at: usize, depth: usize) -> Vec<Prop> {
        let mut props = Vec::new();
        let Some(object) = self.node_at_range(range) else {
            return props;
        };
        let src = self.text.as_str();
        let mut cursor = object.walk();
        for member in object.named_children(&mut cursor) {
            match member.kind() {
                "pair" => {
                    let (Some(key), Some(value)) = (
                        member.child_by_field_name("key"),
                        member.child_by_field_name("value"),
                    ) else {
                        continue;
                    };
                    let name = slice(key, src).trim_matches(['"', '\'']).to_string();
                    let ty = self.ty(value, at, depth + 1);
                    let method = matches!(ty, Ty::Function(_));
                    let detail = if method {
                        self.signature(&value)
                    } else {
                        self.show(&ty)
                    };
                    props.push(Prop {
                        name,
                        method,
                        detail,
                        doc: String::new(),
                        ty,
                        def: Some(key.byte_range()),
                    });
                }
                "method_definition" => {
                    if let Some(name) = member.child_by_field_name("name") {
                        props.push(Prop {
                            name: slice(name, src).to_string(),
                            method: true,
                            detail: self.signature(&member),
                            doc: self.jsdoc_before(&member.byte_range()).unwrap_or_default(),
                            ty: Ty::Function(member.byte_range()),
                            def: Some(name.byte_range()),
                        });
                    }
                }
                "shorthand_property_identifier" => {
                    let name = slice(member, src).to_string();
                    let ty = self.ty_of_name(&name, at, depth + 1);
                    props.push(Prop {
                        detail: self.show(&ty),
                        name,
                        method: false,
                        doc: String::new(),
                        ty,
                        def: Some(member.byte_range()),
                    });
                }
                "spread_element" if depth < 3 => {
                    if let Some(inner) = member.named_child(0)
                        && let Ty::Object(other) = self.ty(inner, at, depth + 1)
                        && other != *range
                    {
                        props.extend(self.object_props(&other, at, depth + 1));
                    }
                }
                _ => {}
            }
        }
        props
    }

    /// `constructor(private id: number)` declares the property `id`.
    fn parameter_properties(
        &self,
        member: Node,
        range: &Range<usize>,
        at: usize,
        depth: usize,
        props: &mut Vec<Prop>,
    ) {
        let src = self.text.as_str();
        let Some(parameters) = member.child_by_field_name("parameters") else {
            return;
        };
        let mut walker = parameters.walk();
        for parameter in parameters.named_children(&mut walker) {
            if !matches!(
                parameter.kind(),
                "required_parameter" | "optional_parameter"
            ) {
                continue;
            }
            let mut inner = parameter.walk();
            let kids: Vec<_> = parameter.children(&mut inner).collect();
            let access = kids
                .iter()
                .find(|c| c.kind() == "accessibility_modifier")
                .map(|c| slice(*c, src));
            let readonly = kids
                .iter()
                .any(|c| !c.is_named() && slice(*c, src) == "readonly");
            if access.is_none() && !readonly {
                continue;
            }
            if matches!(access, Some("private" | "protected"))
                && !(range.start <= at && at <= range.end)
            {
                continue;
            }
            let Some(name) = parameter
                .child_by_field_name("pattern")
                .filter(|n| n.kind() == "identifier")
            else {
                continue;
            };
            let ty = parameter
                .child_by_field_name("type")
                .map_or(Ty::Unknown, |t| self.ty_from_type(t, at, depth + 1));
            props.push(Prop {
                name: slice(name, src).to_string(),
                method: false,
                detail: self.show(&ty),
                doc: String::new(),
                ty,
                def: Some(name.byte_range()),
            });
        }
    }

    fn class_props(
        &self,
        range: &Range<usize>,
        statics: bool,
        at: usize,
        depth: usize,
    ) -> Vec<Prop> {
        let mut props: Vec<Prop> = Vec::new();
        let Some(class) = self.node_at_range(range) else {
            return props;
        };
        let src = self.text.as_str();
        if let Some(body) = class.child_by_field_name("body") {
            let mut cursor = body.walk();
            for member in body.named_children(&mut cursor) {
                let is_static = member
                    .children(&mut member.walk())
                    .any(|c| !c.is_named() && slice(c, src) == "static");
                // `private` and `protected` members are seen from inside the class only.
                let hidden = {
                    let mut inner = member.walk();
                    let restricted = member.children(&mut inner).any(|c| {
                        c.kind() == "accessibility_modifier"
                            && matches!(slice(c, src), "private" | "protected")
                    });
                    restricted && !(range.start <= at && at <= range.end)
                };
                if !statics
                    && member.kind() == "method_definition"
                    && member
                        .child_by_field_name("name")
                        .is_some_and(|n| slice(n, src) == "constructor")
                {
                    self.constructor_assignments(member, &mut props, at, depth);
                }
                if is_static != statics || hidden {
                    continue;
                }
                match member.kind() {
                    "abstract_method_signature" | "method_signature" => {
                        let Some(name) = member.child_by_field_name("name") else {
                            continue;
                        };
                        props.push(Prop {
                            name: slice(name, src).to_string(),
                            method: true,
                            detail: self.signature(&member),
                            doc: self.jsdoc_before(&member.byte_range()).unwrap_or_default(),
                            ty: Ty::Function(member.byte_range()),
                            def: Some(name.byte_range()),
                        });
                    }
                    "method_definition" => {
                        let Some(name) = member.child_by_field_name("name") else {
                            continue;
                        };
                        let name = slice(name, src);
                        if name == "constructor" || name.starts_with('#') {
                            if name == "constructor" && !statics {
                                self.parameter_properties(member, range, at, depth, &mut props);
                            }
                            continue;
                        }
                        let accessor = member.children(&mut member.walk()).any(|c| {
                            matches!(slice(c, src), "get" | "set")
                                && c.start_byte()
                                    < member
                                        .child_by_field_name("name")
                                        .map_or(0, |n| n.start_byte())
                        });
                        props.push(Prop {
                            name: name.to_string(),
                            method: !accessor,
                            detail: self.signature(&member),
                            doc: self.jsdoc_before(&member.byte_range()).unwrap_or_default(),
                            ty: if accessor {
                                Ty::Unknown
                            } else {
                                Ty::Function(member.byte_range())
                            },
                            def: member.child_by_field_name("name").map(|n| n.byte_range()),
                        });
                    }
                    "field_definition" | "public_field_definition" => {
                        let Some(name) = member
                            .child_by_field_name("property")
                            .or_else(|| member.child_by_field_name("name"))
                        else {
                            continue;
                        };
                        let name_node = name;
                        let name = slice(name, src);
                        if name.starts_with('#') {
                            continue;
                        }
                        let declared = member
                            .child_by_field_name("type")
                            .map(|t| self.ty_from_type(t, at, depth + 1))
                            .filter(|t| *t != Ty::Unknown);
                        let ty = declared.unwrap_or_else(|| {
                            member
                                .child_by_field_name("value")
                                .map_or(Ty::Unknown, |v| self.ty(v, at, depth + 1))
                        });
                        let _ = name_node;
                        let method = matches!(ty, Ty::Function(_));
                        props.push(Prop {
                            name: name.to_string(),
                            method,
                            detail: if method {
                                String::new()
                            } else {
                                self.show(&ty)
                            },
                            doc: self.jsdoc_before(&member.byte_range()).unwrap_or_default(),
                            ty,
                            def: member
                                .child_by_field_name("property")
                                .or_else(|| member.child_by_field_name("name"))
                                .map(|n| n.byte_range()),
                        });
                    }
                    _ => {}
                }
            }
        }
        // Inherited members of a local base class.
        if depth < 4
            && let Some(heritage) = class
                .children(&mut class.walk())
                .find(|c| c.kind() == "class_heritage")
            && let Some(base) = heritage.named_child(0)
            && let Ty::Class(base_range) = self.ty(base, at, depth + 1)
            && base_range != *range
        {
            for inherited in self.class_props(&base_range, statics, at, depth + 1) {
                if !props.iter().any(|p| p.name == inherited.name) {
                    props.push(inherited);
                }
            }
        }
        props
    }

    /// `this.name = value` in a constructor declares the property `name`.
    fn constructor_assignments(
        &self,
        constructor: Node,
        props: &mut Vec<Prop>,
        at: usize,
        depth: usize,
    ) {
        let src = self.text.as_str();
        fn walk<'t>(node: Node<'t>, out: &mut Vec<Node<'t>>) {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if child.kind() == "assignment_expression" {
                    out.push(child);
                }
                if !matches!(
                    child.kind(),
                    "function_declaration" | "function_expression" | "class_declaration"
                ) {
                    walk(child, out);
                }
            }
        }
        let mut assignments = Vec::new();
        walk(constructor, &mut assignments);
        for assignment in assignments {
            let (Some(left), Some(right)) = (
                assignment.child_by_field_name("left"),
                assignment.child_by_field_name("right"),
            ) else {
                continue;
            };
            if left.kind() != "member_expression"
                || left
                    .child_by_field_name("object")
                    .is_none_or(|o| o.kind() != "this")
            {
                continue;
            }
            let Some(property) = left.child_by_field_name("property") else {
                continue;
            };
            let name = slice(property, src).to_string();
            if props.iter().any(|p| p.name == name) {
                continue;
            }
            let ty = self.ty(right, at, depth + 1);
            props.push(Prop {
                detail: self.show(&ty),
                method: false,
                name,
                doc: String::new(),
                ty,
                def: Some(property.byte_range()),
            });
        }
    }

    /// `name(a, b)` of a function-like node, from its parameters.
    pub fn signature(&self, node: &Node) -> String {
        let src = self.text.as_str();
        let params = node
            .child_by_field_name("parameters")
            .map(|p| collapse(slice(p, src)))
            .or_else(|| {
                node.child_by_field_name("parameter")
                    .map(|p| format!("({})", slice(p, src)))
            })
            .unwrap_or_else(|| "()".into());
        match node.child_by_field_name("return_type") {
            Some(annotation) => format!("{params}: {}", self.annotation_text(annotation)),
            None => format!("{params} => any"),
        }
    }

    /// A short rendering of a type, as in a tooltip.
    pub fn show(&self, ty: &Ty) -> String {
        match ty {
            Ty::Unknown => "any".into(),
            Ty::Str => "string".into(),
            Ty::Num => "number".into(),
            Ty::Bool => "boolean".into(),
            Ty::Null => "null".into(),
            Ty::Array => "any[]".into(),
            Ty::ArrayOf(element) => {
                let inner = self.show(element);
                if inner.contains([' ', '|', '&']) {
                    format!("({inner})[]")
                } else {
                    format!("{inner}[]")
                }
            }
            Ty::PromiseOf(inner) => format!("Promise<{}>", self.show(inner)),
            Ty::Interface(range) => self
                .node_at_range(range)
                .and_then(|n| n.child_by_field_name("name"))
                .map_or_else(|| "{ … }".to_string(), |n| slice(n, &self.text).to_string()),
            Ty::Enum(range) => self
                .node_at_range(range)
                .and_then(|n| n.child_by_field_name("name"))
                .map_or_else(
                    || "enum".to_string(),
                    |n| format!("typeof {}", slice(n, &self.text)),
                ),
            Ty::Intersection(parts) => parts
                .iter()
                .map(|p| self.show(p))
                .collect::<Vec<_>>()
                .join(" & "),
            Ty::Function(range) => self
                .node_at_range(range)
                .map_or("Function".into(), |n| self.signature(&n)),
            Ty::Object(range) => {
                let names: Vec<String> = self
                    .object_props(range, range.start, 0)
                    .into_iter()
                    .take(4)
                    .map(|p| p.name)
                    .collect();
                if names.is_empty() {
                    "{}".into()
                } else {
                    format!("{{ {} }}", names.join(", "))
                }
            }
            Ty::Class(range) => self
                .class_name(range)
                .map_or("typeof class".into(), |n| format!("typeof {n}")),
            Ty::Instance(range) => self.class_name(range).unwrap_or_else(|| "object".into()),
            Ty::Builtin(name) => (*name).to_string(),
            Ty::Static(name) => format!("typeof {name}"),
            Ty::Module(name) => format!("typeof import('{name}')"),
        }
    }

    fn class_name(&self, range: &Range<usize>) -> Option<String> {
        let class = self.node_at_range(range)?;
        class
            .child_by_field_name("name")
            .map(|n| slice(n, &self.text).to_string())
    }
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn builtin_instance(name: &'static str) -> Ty {
    match name {
        "String" => Ty::Str,
        "Number" => Ty::Num,
        "Boolean" => Ty::Bool,
        "Array" => Ty::Array,
        other => Ty::Builtin(other),
    }
}

/// The type a documented return type name stands for.
fn ty_from_return(name: &str) -> Ty {
    match name {
        "string" => Ty::Str,
        "number" => Ty::Num,
        "boolean" => Ty::Bool,
        "Array" => Ty::Array,
        "Promise" => Ty::Builtin("Promise"),
        "Map" => Ty::Builtin("Map"),
        "Set" => Ty::Builtin("Set"),
        "Date" => Ty::Builtin("Date"),
        "Object" => Ty::Builtin("Object"),
        "Function" => Ty::Builtin("Function"),
        _ => Ty::Unknown,
    }
}

/// The type a JSDoc type expression names.
fn ty_from_name(name: &str) -> Ty {
    let name = name.trim().trim_start_matches('!');
    let base = name.split(['<', '|']).next().unwrap_or(name).trim();
    if name.ends_with("[]") || base.eq_ignore_ascii_case("array") {
        return Ty::Array;
    }
    match base.to_ascii_lowercase().as_str() {
        "string" => Ty::Str,
        "number" | "integer" | "int" | "float" => Ty::Num,
        "boolean" | "bool" => Ty::Bool,
        "function" => Ty::Builtin("Function"),
        "promise" => Ty::Builtin("Promise"),
        "date" => Ty::Builtin("Date"),
        "regexp" => Ty::Builtin("RegExp"),
        "map" => Ty::Builtin("Map"),
        "set" => Ty::Builtin("Set"),
        "object" => Ty::Builtin("Object"),
        "error" => Ty::Builtin("Error"),
        _ => Ty::Unknown,
    }
}

/// The names a well-known Node.js module exports.
pub fn module_member_names(name: &str) -> Vec<String> {
    module_props(name).into_iter().map(|p| p.name).collect()
}

fn module_props(name: &str) -> Vec<Prop> {
    let table: &[(&str, &str, &str)] = match name.trim_start_matches("node:") {
        "path" => &[
            (
                "basename",
                "(path: string, suffix?: string): string",
                "Dernier segment d'un chemin.",
            ),
            ("dirname", "(path: string): string", "Dossier d'un chemin."),
            (
                "extname",
                "(path: string): string",
                "Extension d'un chemin.",
            ),
            (
                "isAbsolute",
                "(path: string): boolean",
                "Indique si le chemin est absolu.",
            ),
            (
                "join",
                "(...paths: string[]): string",
                "Assemble des segments de chemin.",
            ),
            (
                "normalize",
                "(path: string): string",
                "Normalise un chemin.",
            ),
            ("parse", "(path: string): object", "Décompose un chemin."),
            (
                "relative",
                "(from: string, to: string): string",
                "Chemin relatif entre deux chemins.",
            ),
            (
                "resolve",
                "(...paths: string[]): string",
                "Chemin absolu résolu.",
            ),
            ("sep", "string", "Séparateur de chemin de la plateforme."),
        ],
        "fs" => &[
            (
                "appendFile",
                "(path, data, callback): void",
                "Ajoute des données à un fichier.",
            ),
            (
                "copyFile",
                "(src, dest, callback): void",
                "Copie un fichier.",
            ),
            (
                "existsSync",
                "(path: string): boolean",
                "Indique si le chemin existe.",
            ),
            (
                "mkdir",
                "(path, options, callback): void",
                "Crée un dossier.",
            ),
            (
                "mkdirSync",
                "(path: string, options?: object): string | undefined",
                "Crée un dossier.",
            ),
            (
                "promises",
                "object",
                "Version à promesses de l'API fichiers.",
            ),
            (
                "readFile",
                "(path, options, callback): void",
                "Lit un fichier.",
            ),
            (
                "readFileSync",
                "(path: string, options?: string | object): string | Buffer",
                "Lit un fichier.",
            ),
            (
                "readdir",
                "(path, options, callback): void",
                "Liste un dossier.",
            ),
            (
                "readdirSync",
                "(path: string, options?: object): string[]",
                "Liste un dossier.",
            ),
            (
                "rmSync",
                "(path: string, options?: object): void",
                "Supprime un fichier ou un dossier.",
            ),
            (
                "stat",
                "(path, callback): void",
                "Informations sur un fichier.",
            ),
            (
                "statSync",
                "(path: string): Stats",
                "Informations sur un fichier.",
            ),
            ("unlink", "(path, callback): void", "Supprime un fichier."),
            (
                "writeFile",
                "(path, data, callback): void",
                "Écrit un fichier.",
            ),
            (
                "writeFileSync",
                "(path: string, data: string | Buffer): void",
                "Écrit un fichier.",
            ),
        ],
        "os" => &[
            ("EOL", "string", "Fin de ligne de la plateforme."),
            ("arch", "(): string", "Architecture du processeur."),
            ("cpus", "(): object[]", "Informations sur les processeurs."),
            ("homedir", "(): string", "Dossier personnel."),
            ("hostname", "(): string", "Nom de la machine."),
            ("platform", "(): string", "Système d'exploitation."),
            ("tmpdir", "(): string", "Dossier temporaire."),
            ("totalmem", "(): number", "Mémoire totale, en octets."),
        ],
        _ => &[],
    };
    table
        .iter()
        .map(|(name, sig, doc)| Prop {
            name: (*name).to_string(),
            method: sig.starts_with('('),
            detail: (*sig).to_string(),
            doc: (*doc).to_string(),
            ty: if *name == "promises" {
                Ty::Unknown
            } else if sig.ends_with(": string") {
                Ty::Str
            } else {
                Ty::Unknown
            },
            def: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ty_at(text: &str, marker: &str, name: &str) -> (Analysis, Ty) {
        let analysis = Analysis::new(text).unwrap();
        let at = text.find(marker).unwrap();
        let ty = analysis.ty_of_name(name, at, 0);
        (analysis, ty)
    }

    #[test]
    fn syntax_errors_are_reported_at_the_bad_token() {
        assert!(syntax_errors("const a = 1;\nfunction f(x) { return x * 2; }\n").is_empty());
        let errors = syntax_errors("const a = ;\nlet b = 2;\n");
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!((8..=10).contains(&errors[0].range.start), "{errors:?}");
        let errors = syntax_errors("function f( {\n  return 1;\n}\n");
        assert!(!errors.is_empty());
        let errors = syntax_errors("if (x) {\n  foo(\n}\n");
        assert!(!errors.is_empty());
    }

    #[test]
    fn scopes_hoist_var_and_functions_but_not_let() {
        let text = "function outer(p) {\n  var a = 1;\n  if (p) { let b = 2; var c = 3; }\n  return a;\n  //here\n}\nconst top = 1;\n";
        let analysis = Analysis::new(text).unwrap();
        let at = text.find("//here").unwrap();
        let names: Vec<&str> = analysis
            .visible(at)
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        assert!(
            names.contains(&"p") && names.contains(&"a") && names.contains(&"c"),
            "{names:?}"
        );
        assert!(
            names.contains(&"outer") && names.contains(&"top"),
            "{names:?}"
        );
        assert!(!names.contains(&"b"), "block scoped: {names:?}");
        let inside = text.find("let b").unwrap();
        assert!(analysis.visible(inside).iter().any(|d| d.name == "b"));
    }

    #[test]
    fn destructuring_imports_and_catch_are_declared() {
        let text = "import def, { a, b as c } from './m.js';\nimport * as ns from 'x';\nconst { d, e: [f, ...g], h = 1 } = obj;\ntry {} catch (err) { //in\n}\n";
        let analysis = Analysis::new(text).unwrap();
        let names: Vec<&str> = analysis
            .visible(text.find("//in").unwrap())
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        for expected in ["def", "a", "c", "ns", "d", "f", "g", "h", "err"] {
            assert!(names.contains(&expected), "{expected} in {names:?}");
        }
        assert!(!names.contains(&"b") && !names.contains(&"e"));
        let c = analysis.resolve("c", 0).unwrap();
        assert_eq!(c.import, Some(("./m.js".into(), "b".into())));
    }

    #[test]
    fn literal_types_flow_through_variables_and_calls() {
        let text = "const s = 'x';\nconst n = 1 + 2;\nconst list = [1, 2];\nconst up = s.toUpperCase();\nconst parts = s.split(',');\nconst m = new Map();\nfunction mk() { return { a: 1 }; }\nconst o = mk();\nlet z; //end\n";
        let at = |name| ty_at(text, "//end", name).1;
        assert_eq!(at("s"), Ty::Str);
        assert_eq!(at("n"), Ty::Num);
        assert_eq!(at("list"), Ty::ArrayOf(Box::new(Ty::Num)));
        assert_eq!(at("up"), Ty::Str);
        assert_eq!(at("parts"), Ty::Array);
        assert_eq!(at("m"), Ty::Builtin("Map"));
        assert!(matches!(at("o"), Ty::Object(_)));
        assert_eq!(at("z"), Ty::Unknown);
        assert_eq!(at("Math"), Ty::Static("Math"));
    }

    #[test]
    fn classes_expose_methods_fields_constructor_properties_and_statics() {
        let text = "class Animal {\n  legs = 4;\n  static create() { return new Animal('x'); }\n  constructor(name) { this.name = 'n'; }\n  speak() {}\n}\nclass Dog extends Animal { bark() {} }\nconst d = new Dog();\n//end\n";
        let analysis = Analysis::new(text).unwrap();
        let at = text.find("//end").unwrap();
        let Ty::Instance(range) = analysis.ty_of_name("d", at, 0) else {
            panic!("a Dog instance");
        };
        let names: Vec<String> = analysis
            .props_of(&Ty::Instance(range), at)
            .into_iter()
            .map(|p| p.name)
            .collect();
        for expected in ["bark", "speak", "legs", "name", "hasOwnProperty"] {
            assert!(
                names.contains(&expected.to_string()),
                "{expected} in {names:?}"
            );
        }
        let Ty::Class(class) = analysis.ty_of_name("Animal", at, 0) else {
            panic!("a class");
        };
        let statics: Vec<String> = analysis
            .props_of(&Ty::Class(class), at)
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert!(
            statics.contains(&"create".to_string()) && !statics.contains(&"speak".to_string()),
            "{statics:?}"
        );
    }

    #[test]
    fn jsdoc_types_and_comments_are_read() {
        let text = "/**\n * Adds.\n * @param {string} name - the name\n */\nfunction greet(name, other) { name; }\n";
        let analysis = Analysis::new(text).unwrap();
        let at = text.find("name;").unwrap();
        assert_eq!(analysis.ty_of_name("name", at, 0), Ty::Str);
        assert_eq!(analysis.ty_of_name("other", at, 0), Ty::Unknown);
        let decl = analysis.resolve("greet", at).unwrap();
        let doc = analysis.jsdoc_before(&decl.node).unwrap();
        assert!(doc.starts_with("Adds."), "{doc}");
    }
}
