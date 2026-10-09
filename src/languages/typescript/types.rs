//! What TypeScript adds to the analysis: types written in the code (annotations, interfaces,
//! type aliases, enums, generics) and the types they give to names and members.

use crate::languages::javascript::analysis::{Analysis, Decl, DeclKind, Prop, Ty};
use std::ops::Range;
use tree_sitter::Node;

const MAX_DEPTH: usize = 10;

fn text<'a>(node: Node, src: &'a str) -> &'a str {
    &src[node.byte_range()]
}

/// The type a name of the standard library stands for, given its type arguments.
fn library_type(name: &str, args: &[Ty]) -> Ty {
    let first = args.first().cloned();
    match name {
        "Array" | "ReadonlyArray" => first.map_or(Ty::Array, |t| Ty::ArrayOf(Box::new(t))),
        "Promise" | "PromiseLike" => {
            first.map_or(Ty::Builtin("Promise"), |t| Ty::PromiseOf(Box::new(t)))
        }
        "Map" | "WeakMap" | "ReadonlyMap" => Ty::Builtin("Map"),
        "Set" | "WeakSet" | "ReadonlySet" => Ty::Builtin("Set"),
        "Date" => Ty::Builtin("Date"),
        "RegExp" => Ty::Builtin("RegExp"),
        "Error" | "TypeError" | "RangeError" | "SyntaxError" | "ReferenceError" => {
            Ty::Builtin("Error")
        }
        "Function" => Ty::Builtin("Function"),
        "Object" => Ty::Builtin("Object"),
        "URL" => Ty::Builtin("URL"),
        "String" => Ty::Str,
        "Number" => Ty::Num,
        "Boolean" => Ty::Bool,
        "Partial" | "Required" | "Readonly" | "NonNullable" => first.unwrap_or(Ty::Unknown),
        "Awaited" => match first {
            Some(Ty::PromiseOf(inner)) => *inner,
            other => other.unwrap_or(Ty::Unknown),
        },
        _ => Ty::Unknown,
    }
}

impl Analysis {
    /// The type a type annotation or type expression stands for.
    pub fn ty_from_type(&self, node: Node, at: usize, depth: usize) -> Ty {
        if depth > MAX_DEPTH {
            return Ty::Unknown;
        }
        let src = self.text.as_str();
        match node.kind() {
            "type_annotation"
            | "opting_type_annotation"
            | "omitting_type_annotation"
            | "adding_type_annotation"
            | "parenthesized_type"
            | "readonly_type" => node
                .named_child(0)
                .map_or(Ty::Unknown, |inner| self.ty_from_type(inner, at, depth + 1)),
            "predefined_type" => match text(node, src) {
                "string" => Ty::Str,
                "number" => Ty::Num,
                "boolean" => Ty::Bool,
                "object" => Ty::Builtin("Object"),
                _ => Ty::Unknown,
            },
            "literal_type" => match node.named_child(0).map(|c| c.kind()) {
                Some("string" | "template_string") => Ty::Str,
                Some("number" | "unary_expression") => Ty::Num,
                Some("true" | "false") => Ty::Bool,
                Some("null") => Ty::Null,
                _ => Ty::Unknown,
            },
            "template_literal_type" | "index_type_query" => Ty::Str,
            "array_type" => node.named_child(0).map_or(Ty::Array, |inner| {
                Ty::ArrayOf(Box::new(self.ty_from_type(inner, at, depth + 1)))
            }),
            "tuple_type" => Ty::Array,
            "object_type" => Ty::Interface(node.byte_range()),
            "function_type" => Ty::Function(node.byte_range()),
            "union_type" => {
                // `string | undefined`: the type that is there.
                let mut present: Vec<Ty> = Vec::new();
                let mut cursor = node.walk();
                for member in node.named_children(&mut cursor) {
                    let t = text(member, src);
                    if t == "undefined" || t == "null" || t == "void" {
                        continue;
                    }
                    present.push(self.ty_from_type(member, at, depth + 1));
                }
                match present.as_slice() {
                    [only] => only.clone(),
                    [first, rest @ ..] if rest.iter().all(|t| t == first) => first.clone(),
                    _ => Ty::Unknown,
                }
            }
            "intersection_type" => {
                let mut cursor = node.walk();
                let parts: Vec<Ty> = node
                    .named_children(&mut cursor)
                    .map(|member| self.ty_from_type(member, at, depth + 1))
                    .filter(|t| *t != Ty::Unknown)
                    .collect();
                match parts.len() {
                    0 => Ty::Unknown,
                    1 => parts.into_iter().next().unwrap_or(Ty::Unknown),
                    _ => Ty::Intersection(parts),
                }
            }
            "type_query" => node
                .named_child(0)
                .filter(|n| n.kind() == "identifier")
                .map_or(Ty::Unknown, |n| {
                    self.ty_of_name(text(n, src), at, depth + 1)
                }),
            "type_identifier" => self.named_type(text(node, src), &[], at, depth),
            "nested_type_identifier" => node.child_by_field_name("name").map_or(Ty::Unknown, |n| {
                self.named_type(text(n, src), &[], at, depth)
            }),
            "generic_type" => {
                let Some(name) = node.child_by_field_name("name") else {
                    return Ty::Unknown;
                };
                let args: Vec<Ty> = node
                    .child_by_field_name("type_arguments")
                    .map(|arguments| {
                        let mut cursor = arguments.walk();
                        arguments
                            .named_children(&mut cursor)
                            .map(|a| self.ty_from_type(a, at, depth + 1))
                            .collect()
                    })
                    .unwrap_or_default();
                let name = if name.kind() == "nested_type_identifier" {
                    name.child_by_field_name("name")
                        .map_or("", |n| text(n, src))
                } else {
                    text(name, src)
                };
                self.named_type(name, &args, at, depth)
            }
            _ => Ty::Unknown,
        }
    }

    /// A type referred to by name: a declared class, interface, alias or enum, or a library type.
    fn named_type(&self, name: &str, args: &[Ty], at: usize, depth: usize) -> Ty {
        let Some(decl) = self.resolve(name, at) else {
            return library_type(name, args);
        };
        match decl.kind {
            DeclKind::Class => self
                .node_at_range(&decl.node)
                .map_or(Ty::Unknown, |n| Ty::Instance(n.byte_range())),
            DeclKind::Interface => Ty::Interface(decl.node.clone()),
            DeclKind::TypeAlias => self
                .node_at_range(&decl.node)
                .and_then(|n| n.child_by_field_name("value"))
                .map_or(Ty::Unknown, |value| {
                    self.ty_from_type(value, decl.node.start, depth + 1)
                }),
            DeclKind::Enum => {
                let strings = self
                    .node_at_range(&decl.node)
                    .is_some_and(|n| text(n, &self.text).contains(['"', '\'']));
                if strings { Ty::Str } else { Ty::Num }
            }
            _ => Ty::Unknown,
        }
    }

    /// The text of a type annotation without its colon, on one line.
    pub fn annotation_text(&self, node: Node) -> String {
        let raw = text(node, &self.text)
            .trim_start_matches(':')
            .trim_start_matches('?')
            .trim_start_matches(':');
        raw.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// The members of an interface, or of an object type, with those of the interfaces it extends.
    pub fn interface_props(&self, range: &Range<usize>, at: usize, depth: usize) -> Vec<Prop> {
        let mut props = Vec::new();
        let Some(node) = self.node_at_range(range) else {
            return props;
        };
        let src = self.text.as_str();
        let body = node.child_by_field_name("body").unwrap_or(node);
        let mut cursor = body.walk();
        for member in body.named_children(&mut cursor) {
            let Some(name) = member.child_by_field_name("name") else {
                continue;
            };
            match member.kind() {
                "property_signature" => {
                    let annotation = member.child_by_field_name("type");
                    let ty =
                        annotation.map_or(Ty::Unknown, |t| self.ty_from_type(t, at, depth + 1));
                    let method = matches!(ty, Ty::Function(_));
                    props.push(Prop {
                        name: text(name, src).trim_matches(['"', '\'']).to_string(),
                        method,
                        detail: annotation
                            .map(|t| self.annotation_text(t))
                            .unwrap_or_default(),
                        doc: self.jsdoc_before(&member.byte_range()).unwrap_or_default(),
                        ty,
                        def: Some(name.byte_range()),
                    });
                }
                "method_signature" => props.push(Prop {
                    name: text(name, src).to_string(),
                    method: true,
                    detail: self.signature(&member),
                    doc: self.jsdoc_before(&member.byte_range()).unwrap_or_default(),
                    ty: Ty::Function(member.byte_range()),
                    def: Some(name.byte_range()),
                }),
                _ => {}
            }
        }
        // `interface B extends A, C`
        if depth < 4 {
            let mut inner = node.walk();
            let clauses: Vec<Node> = node
                .children(&mut inner)
                .filter(|c| c.kind() == "extends_type_clause")
                .collect();
            for clause in clauses {
                let mut walker = clause.walk();
                for base in clause.named_children(&mut walker) {
                    let ty = self.ty_from_type(base, at, depth + 1);
                    for inherited in self.props_of(&ty, at) {
                        if !props.iter().any(|p| p.name == inherited.name) {
                            props.push(inherited);
                        }
                    }
                }
            }
        }
        props
    }

    /// The members of an enum: its constants.
    pub fn enum_props(&self, range: &Range<usize>) -> Vec<Prop> {
        let Some(node) = self.node_at_range(range) else {
            return Vec::new();
        };
        let Some(body) = node.child_by_field_name("body") else {
            return Vec::new();
        };
        let src = self.text.as_str();
        let strings = text(node, src).contains(['"', '\'']);
        let mut cursor = body.walk();
        body.named_children(&mut cursor)
            .filter_map(|member| {
                let name = if member.kind() == "enum_assignment" {
                    member.child_by_field_name("name")?
                } else {
                    member
                };
                if !matches!(name.kind(), "property_identifier" | "string" | "number") {
                    return None;
                }
                Some(Prop {
                    name: text(name, src).trim_matches(['"', '\'']).to_string(),
                    method: false,
                    detail: text(node.child_by_field_name("name")?, src).to_string(),
                    doc: String::new(),
                    ty: if strings { Ty::Str } else { Ty::Num },
                    def: Some(name.byte_range()),
                })
            })
            .collect()
    }

    /// The type a callback parameter gets from where the callback is passed: the elements of
    /// the array of `items.map(item => …)`, or the parameter types of the function it is given to.
    pub fn contextual_param_ty(&self, decl: &Decl, depth: usize) -> Option<Ty> {
        if depth > MAX_DEPTH {
            return None;
        }
        let function = self.node_at_range(decl.owner.as_ref()?)?;
        if !matches!(
            function.kind(),
            "arrow_function" | "function_expression" | "generator_function"
        ) {
            return None;
        }
        let arguments = function.parent().filter(|p| p.kind() == "arguments")?;
        let call = arguments
            .parent()
            .filter(|p| p.kind() == "call_expression")?;
        let argument_index = {
            let mut cursor = arguments.walk();
            arguments
                .named_children(&mut cursor)
                .position(|a| a.id() == function.id())?
        };
        // Which parameter of the callback is this?
        let parameter_index = if let Some(parameters) = function.child_by_field_name("parameters") {
            let mut cursor = parameters.walk();
            parameters.named_children(&mut cursor).position(|p| {
                p.start_byte() <= decl.name_range.start && decl.name_range.end <= p.end_byte()
            })?
        } else {
            0
        };
        let callee = call.child_by_field_name("function")?;
        let src = self.text.as_str();
        if callee.kind() == "member_expression" && argument_index == 0 {
            let object = self.ty(
                callee.child_by_field_name("object")?,
                decl.node.start,
                depth + 1,
            );
            let method = text(callee.child_by_field_name("property")?, src);
            let element = match &object {
                Ty::ArrayOf(element) => Some((**element).clone()),
                _ => None,
            };
            return match (method, parameter_index, &object) {
                (
                    "map" | "filter" | "find" | "findIndex" | "findLast" | "findLastIndex"
                    | "forEach" | "some" | "every" | "flatMap",
                    0,
                    _,
                ) => element,
                (
                    "map" | "filter" | "find" | "findIndex" | "findLast" | "findLastIndex"
                    | "forEach" | "some" | "every" | "flatMap",
                    1,
                    _,
                ) => Some(Ty::Num),
                ("reduce" | "reduceRight", 1, _) => element,
                ("reduce" | "reduceRight", 2, _) => Some(Ty::Num),
                ("sort" | "toSorted", 0 | 1, _) => element,
                ("then", 0, Ty::PromiseOf(inner)) => Some((**inner).clone()),
                _ => None,
            };
        }
        // A callback given to a local function whose parameter has a function type.
        let Ty::Function(target) = self.ty(callee, decl.node.start, depth + 1) else {
            return None;
        };
        let target = self.node_at_range(&target)?;
        let parameters = target.child_by_field_name("parameters")?;
        let mut cursor = parameters.walk();
        let given = parameters.named_children(&mut cursor).nth(argument_index)?;
        let annotation = given.child_by_field_name("type")?;
        let Ty::Function(callback_type) = self.ty_from_type(annotation, decl.node.start, depth + 1)
        else {
            return None;
        };
        let callback_type = self.node_at_range(&callback_type)?;
        let callback_parameters = callback_type.child_by_field_name("parameters")?;
        let mut inner = callback_parameters.walk();
        let wanted = callback_parameters
            .named_children(&mut inner)
            .nth(parameter_index)?;
        let wanted = wanted.child_by_field_name("type")?;
        Some(self.ty_from_type(wanted, decl.node.start, depth + 1))
    }

    /// The type of the elements of what a `for … of` loops over.
    pub fn element_ty(&self, iterable: &Range<usize>, depth: usize) -> Ty {
        let Some(node) = self.node_at_range(iterable) else {
            return Ty::Unknown;
        };
        match self.ty(node, node.start_byte(), depth + 1) {
            Ty::ArrayOf(element) => *element,
            Ty::Str => Ty::Str,
            _ => Ty::Unknown,
        }
    }
}
