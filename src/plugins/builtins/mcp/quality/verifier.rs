//! verifier: functions worth merging into their only caller.
//!
//! A function is worth merging when it is defined once, mentioned once, and that mention is a
//! call from another function whose CRAP, added to its own, stays under the threshold.
//! The merge must be a plain paste: not into a closure, and not of a body that exits early.
//! Mentions are looked for everywhere, tests included, so that a doubt keeps the function.

use super::args::{self, Args};
use super::lang::{Lang, parse};
use super::lcov::Coverage;
use super::{Out, crapper, discover, funcs, metrics, par};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tree_sitter::Node;

/// A function, its CRAP, and whether its body leaves before its end.
#[derive(Debug, PartialEq)]
pub struct Def {
    pub name: String,
    pub file: String,
    pub start: usize,
    pub end: usize,
    pub crap: f64,
    pub exits_early: bool,
}

/// A place a name is mentioned: a call, or any other use (a value, a macro argument...).
#[derive(Debug, PartialEq)]
pub struct Use {
    pub name: String,
    pub file: String,
    pub line: usize,
    pub call: bool,
    /// Inside a closure of the function around it.
    pub in_closure: bool,
}

/// `function` is called only from `into`, at `line`.
pub struct Merge<'a> {
    pub function: &'a Def,
    pub into: &'a Def,
    pub line: usize,
}

impl Def {
    fn holds(&self, u: &Use) -> bool {
        self.file == u.file && (self.start..=self.end).contains(&u.line)
    }
}

const NAMES: &[&str] = &[
    "identifier",
    "field_identifier",
    "property_identifier",
    "shorthand_property_identifier",
];
const BINDERS: &[&str] = &[
    "function_item",
    "function_declaration",
    "generator_function_declaration",
    "method_definition",
    "variable_declarator",
    "let_declaration",
];

/// The name a definition or a `let` introduces is not a use of it.
fn is_binding(n: Node) -> bool {
    let named = |p: Node| {
        [
            p.child_by_field_name("name"),
            p.child_by_field_name("pattern"),
        ]
        .contains(&Some(n))
    };
    n.parent()
        .is_some_and(|p| BINDERS.contains(&p.kind()) && named(p))
}

/// `f()`, `a.f()`, `m::f()`, `f::<T>()`: `n` names the function being called.
fn is_call(n: Node) -> bool {
    let last = |p: &Node| {
        ["name", "field", "property"]
            .iter()
            .any(|f| p.child_by_field_name(f) == Some(n))
    };
    let path = n.parent().filter(|p| {
        matches!(
            p.kind(),
            "scoped_identifier" | "field_expression" | "member_expression"
        ) && last(p)
    });
    let callee = path.unwrap_or(n);
    let callee = callee
        .parent()
        .filter(|g| g.kind() == "generic_function")
        .unwrap_or(callee);
    callee
        .parent()
        .is_some_and(|call| call.child_by_field_name("function") == Some(callee))
}

const FUNCTIONS: &[&str] = &[
    "function_item",
    "function_declaration",
    "generator_function_declaration",
    "method_definition",
];
const CLOSURES: &[&str] = &[
    "closure_expression",
    "arrow_function",
    "function_expression",
    "function",
];

/// Whether the code under `n` sits in a closure: a function starts afresh, a closure opens one.
/// `const f = () => ...` defines a function; it is not a closure.
fn inside_closure(n: Node, inside: bool) -> bool {
    let defined = n
        .parent()
        .is_some_and(|p| p.kind() == "variable_declarator");
    match n.kind() {
        k if FUNCTIONS.contains(&k) => false,
        k if CLOSURES.contains(&k) => !defined,
        _ => inside,
    }
}

fn uses_in(n: Node, src: &str, file: &str, inside: bool, out: &mut Vec<Use>) {
    if NAMES.contains(&n.kind()) && !is_binding(n) {
        let (name, line) = (src[n.byte_range()].to_string(), n.start_position().row + 1);
        out.push(Use {
            name,
            file: file.to_string(),
            line,
            call: is_call(n),
            in_closure: inside,
        });
    }
    let inside = inside_closure(n, inside);
    let mut cursor = n.walk();
    for child in n.children(&mut cursor) {
        uses_in(child, src, file, inside, out);
    }
}

/// A `return` that is the last statement of `body` ends the function anyway.
fn is_last(ret: Node, body: Node) -> bool {
    let stmt = ret
        .parent()
        .filter(|p| p.kind() == "expression_statement")
        .unwrap_or(ret);
    let mut cursor = body.walk();
    stmt.parent() == Some(body) && body.named_children(&mut cursor).last() == Some(stmt)
}

/// A `?`, a `let ... else` or a `return` before the end, outside nested functions and closures.
fn exits_early(n: Node, body: Node) -> bool {
    let kind = n.kind();
    if n != body && (FUNCTIONS.contains(&kind) || CLOSURES.contains(&kind)) {
        return false;
    }
    let exits = kind == "try_expression"
        || (kind == "let_declaration" && n.child_by_field_name("alternative").is_some())
        || (matches!(kind, "return_expression" | "return_statement") && !is_last(n, body));
    let mut cursor = n.walk();
    exits || n.children(&mut cursor).any(|c| exits_early(c, body))
}

/// The functions of a file with their CRAP (their complexity without coverage), and every name it mentions.
pub fn scan(root: &Path, path: &Path, cov: Option<&Coverage>) -> (Vec<Def>, Vec<Use>) {
    let Some(lang) = Lang::from_path(path) else {
        return Default::default();
    };
    let Ok(src) = std::fs::read_to_string(path) else {
        return Default::default();
    };
    let Some(tree) = parse(lang, &src) else {
        return Default::default();
    };
    let file = discover::rel(root, path);
    let module = funcs::module_name(root, path, lang);
    let crap = |f: &funcs::Func, cc: usize| {
        cov.map_or(cc as f64, |c| {
            crapper::crap(cc, c.function_percent(path, f.start_line, f.end_line))
        })
    };
    let def = |f: &funcs::Func| Def {
        name: f.name.clone(),
        file: file.clone(),
        start: f.start_line,
        end: f.end_line,
        crap: crap(f, funcs::complexity(f.node)),
        exits_early: f
            .node
            .child_by_field_name("body")
            .is_some_and(|b| exits_early(b, b)),
    };
    let defs = funcs::functions(&tree, &src, lang, &module)
        .iter()
        .map(def)
        .collect();
    let mut uses = Vec::new();
    uses_in(tree.root_node(), &src, &file, false, &mut uses);
    (defs, uses)
}

/// The innermost function around a use.
fn caller<'a>(defs: &'a [Def], u: &Use) -> Option<&'a Def> {
    defs.iter()
        .filter(|d| d.holds(u))
        .min_by_key(|d| d.end - d.start)
}

/// Functions defined once and mentioned once, by a call from another function, that can be pasted
/// there as they are (no closure around the call, no early exit) and whose CRAP and their
/// caller's add up to less than `limit`.
pub fn merges<'a>(defs: &'a [Def], uses: &[Use], limit: f64) -> Vec<Merge<'a>> {
    let mut by_name: HashMap<&str, Vec<&Use>> = HashMap::new();
    for u in uses {
        by_name.entry(&u.name).or_default().push(u);
    }
    let once = |d: &Def| defs.iter().filter(|o| o.name == d.name).count() == 1;
    defs.iter()
        .filter_map(|d| {
            let [u] = by_name.get(d.name.as_str())?.as_slice() else {
                return None;
            };
            let pastable = u.call && !u.in_closure && !d.exits_early;
            let into = caller(defs, u).filter(|_| pastable && once(d) && !d.holds(u))?;
            (into.crap + d.crap < limit).then_some(Merge {
                function: d,
                into,
                line: u.line,
            })
        })
        .collect()
}

fn report(merges: &[Merge], limit: f64) -> String {
    if merges.is_empty() {
        return format!(
            "Nothing to merge: no function called once whose CRAP plus its caller's is under {limit}.\n"
        );
    }
    let mut out =
        format!("Merge Report\n============\nCalled once, CRAP + caller's CRAP < {limit}:\n\n");
    for m in merges {
        let (f, into) = (m.function, m.into);
        out += &format!(
            "{}:{}  merge {} (CRAP {:.1}) into {} (CRAP {:.1}) = {:.1}\n",
            f.file,
            m.line,
            f.name,
            f.crap,
            into.name,
            into.crap,
            f.crap + into.crap
        );
    }
    out
}

fn to_edn(merges: &[Merge]) -> String {
    let rows: Vec<String> = merges
        .iter()
        .map(|m| {
            format!(
                "{{:function {} :into {} :file {} :line {} :crap {} :into-crap {}}}",
                crapper::edn_string(&m.function.name),
                crapper::edn_string(&m.into.name),
                crapper::edn_string(&m.function.file),
                m.line,
                crapper::edn_number(Some(m.function.crap)),
                crapper::edn_number(Some(m.into.crap))
            )
        })
        .collect();
    format!("{{:merges [{}]}}\n", rows.join("\n          "))
}

/// The options `verifier` takes.
pub const FLAGS: &[&str] = &["no-coverage", "use-existing-coverage", "changed"];
pub const VALUES: &[&str] = &["threshold", "jobs"];

/// Exit 0 when there is nothing to merge, 2 when merges are proposed, 1 on an error.
pub fn run_in(root: &Path, argv: &[String], out: &Out) -> i32 {
    args::exit("verifier", execute(root, argv, out), out)
}

/// Everything scanned: functions of the source files only, mentions from every file.
fn scan_all(root: &Path, args: &Args, out: &Out) -> Result<(Vec<Def>, Vec<Use>), String> {
    let files = discover::with_tests(root);
    let sources: HashSet<String> = discover::select(root, &[], false)?
        .iter()
        .map(|f| discover::rel(root, f))
        .collect();
    let cov = crapper::load_coverage(args, root, &files, out)?;
    let scanned = par::map(&files, par::jobs(args)?, |f| scan(root, f, cov.as_ref()));
    let (mut defs, mut uses) = (Vec::new(), Vec::new());
    for (d, u) in scanned {
        defs.extend(d.into_iter().filter(|d| sources.contains(&d.file)));
        uses.extend(u);
    }
    Ok((defs, uses))
}

fn execute(root: &Path, argv: &[String], out: &Out) -> Result<i32, String> {
    let args = Args::parse(argv, FLAGS, VALUES)?;
    let limit = args.number("threshold", 8.0)?;
    let targets: HashSet<String> = discover::select(root, &args.positional, args.has("changed"))?
        .iter()
        .map(|f| discover::rel(root, f))
        .collect();
    if targets.is_empty() {
        out.line("No source files to analyze.");
        return Ok(0);
    }
    let (defs, uses) = scan_all(root, &args, out)?;
    let found: Vec<Merge> = merges(&defs, &uses, limit)
        .into_iter()
        .filter(|m| targets.contains(&m.function.file))
        .collect();
    out.print(&report(&found, limit));
    metrics::write(root, "merge.edn", &to_edn(&found), out);
    Ok(if found.is_empty() { 0 } else { 2 })
}

#[cfg(test)]
#[path = "../../../../../test/plugins/builtins/mcp/quality/verifier/tests.rs"]
mod tests;
