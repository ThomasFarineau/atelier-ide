use crate::languages::javascript::{
    analysis::{Dialect, syntax_errors, use_dialect},
    complete::complete,
    hover::hover,
    imports::import_fixes,
    navigate::{definition, references},
    symbols::symbols,
};
use crate::languages::{Completion, ItemKind};

/// The text before `|` is the document, `|` the caret.
fn at(source: &str) -> (String, usize) {
    let offset = source.rfind('|').unwrap();
    (format!("{}{}", &source[..offset], &source[offset + 1..]), offset)
}

fn labels(completion: &Completion) -> Vec<String> {
    completion.items.iter().map(|i| i.label.clone()).collect()
}

fn members(source: &str) -> Vec<String> {
    let _dialect = use_dialect(Dialect::Ts);
    let (text, offset) = at(source);
    complete(&text, offset, false, None).map(|c| labels(&c)).unwrap_or_default()
}

const USER: &str = "interface User {\n  name: string;\n  age?: number;\n  greet(): string;\n}\ninterface Admin extends User {\n  role: string;\n}\nenum Color { Red, Green = 2 }\ntype Id = string | undefined;\n";

#[test]
fn typescript_and_tsx_parse_and_errors_are_reported() {
    {
        let _dialect = use_dialect(Dialect::Ts);
        assert!(syntax_errors("interface A { x: number }\nfunction f<T>(a: T[]): Promise<T> { return Promise.resolve(a[0]); }\nclass B<T> implements A { constructor(private x: number) {} }\nconst y = <const>['a'];\n").is_empty());
        assert!(!syntax_errors("let x: = 1;\n").is_empty());
    }
    {
        let _tsx = use_dialect(Dialect::Tsx);
        assert!(syntax_errors("const App = (props: { name: string }) => <div className=\"a\">{props.name}</div>;\n").is_empty());
    }
    // Types are not JavaScript.
    let _js = use_dialect(Dialect::Js);
    assert!(!syntax_errors("let x: number = 1;\n").is_empty());
}

#[test]
fn members_follow_declared_types() {
    let names = members(&format!("{USER}const u: User = load();\nu.|"));
    for expected in ["name", "age", "greet"] {
        assert!(names.contains(&expected.to_string()), "{expected} in {names:?}");
    }
    let names = members(&format!("{USER}function f(a: Admin) {{\n  a.|\n}}"));
    for expected in ["role", "name", "greet"] {
        assert!(names.contains(&expected.to_string()), "{expected} in {names:?}");
    }
    let names = members(&format!("{USER}Color.|"));
    assert!(names.contains(&"Red".to_string()) && names.contains(&"Green".to_string()), "{names:?}");
    let names = members(&format!("{USER}const c: Color = Color.Red;\nc.|"));
    assert!(names.contains(&"toFixed".to_string()), "a number enum: {names:?}");
    // `string | undefined` is a string.
    let names = members(&format!("{USER}function g(id: Id) {{ id.| }}"));
    assert!(names.contains(&"toUpperCase".to_string()), "{names:?}");
}

#[test]
fn elements_callbacks_promises_and_assertions_are_typed() {
    let base = format!("{USER}const users: User[] = [];\n");
    for tail in [
        "users.map(u => u.|)",
        "for (const u of users) {\n  u.|\n}",
        "users[0].|",
        "users.find(u => u.age)?.|",
        "users.filter(u => true).pop()!.|",
        "const first = users.at(0) as User;\nfirst.|",
    ] {
        let names = members(&format!("{base}{tail}"));
        assert!(names.contains(&"greet".to_string()), "{tail}: {names:?}");
    }
    let names = members(&format!("{USER}async function load(): Promise<User> {{ throw 1; }}\nasync function main() {{\n  const u = await load();\n  u.|\n}}"));
    assert!(names.contains(&"greet".to_string()), "{names:?}");
    let names = members(&format!("{USER}declare function load(): Promise<User>;\nload().then(v => v.|)"));
    assert!(names.contains(&"greet".to_string()), "{names:?}");
    let names = members(&format!("{USER}const xs: string[] = [];\nconst lengths = xs.map(s => s.length);\nlengths.|"));
    assert!(names.contains(&"reduce".to_string()), "an array of numbers: {names:?}");
}

#[test]
fn classes_hide_private_members_outside_and_declare_parameter_properties() {
    let class = "class Account {\n  private secret = 1;\n  protected level = 2;\n  public owner: string = '';\n  constructor(private id: number, readonly label: string) {}\n  static create(): Account { return new Account(1, 'x'); }\n  deposit(amount: number): void {}\n  show() {\n    this.|\n  }\n}\n";
    let inside = members(class);
    for expected in ["secret", "level", "owner", "id", "label", "deposit"] {
        assert!(inside.contains(&expected.to_string()), "{expected} inside: {inside:?}");
    }
    let outside = members("class Account {\n  private secret = 1;\n  public owner = '';\n  constructor(private id: number, readonly label: string) {}\n  deposit() {}\n}\nconst a = new Account(1, 'x');\na.|");
    for expected in ["owner", "label", "deposit"] {
        assert!(outside.contains(&expected.to_string()), "{expected} outside: {outside:?}");
    }
    assert!(!outside.contains(&"secret".to_string()) && !outside.contains(&"id".to_string()), "{outside:?}");
    let statics = members("class A {\n  static create() {}\n  inst() {}\n}\nA.|");
    assert!(statics.contains(&"create".to_string()) && !statics.contains(&"inst".to_string()), "{statics:?}");
}

#[test]
fn types_are_completed_where_a_type_goes() {
    let _dialect = use_dialect(Dialect::Ts);
    for source in [
        format!("{USER}let x: Us|"),
        format!("{USER}function f(a: |) {{}}"),
        format!("{USER}const xs: Array<|> = [];"),
        format!("{USER}type Pair = {{ left: |; }}"),
        format!("{USER}const v = value as |;"),
        format!("{USER}class C implements |"),
    ] {
        let (text, offset) = at(&source);
        let completion = complete(&text, offset, true, None).unwrap_or_else(|| panic!("no completion for {}", &source[USER.len()..]));
        let names = labels(&completion);
        assert!(names.contains(&"User".to_string()) && names.contains(&"string".to_string()) && names.contains(&"Promise".to_string()), "{source}: {names:?}");
        assert!(!names.contains(&"const".to_string()), "no statements in a type: {names:?}");
    }
    // Type parameters of the enclosing function are types too.
    let (text, offset) = at("function id<T>(x: T): |  { return x; }");
    let names = labels(&complete(&text, offset, true, None).unwrap());
    assert!(names.contains(&"T".to_string()), "{names:?}");
    // In code, the TypeScript keywords and templates join the others.
    let (text, offset) = at("int|");
    let names = labels(&complete(&text, offset, false, None).unwrap());
    assert!(names.contains(&"interface".to_string()), "{names:?}");
    // Nothing for the name of a declaration.
    let (text, offset) = at("interface Us|");
    assert!(complete(&text, offset, false, None).is_none());
}

#[test]
fn object_literals_get_the_properties_of_their_declared_type() {
    let _dialect = use_dialect(Dialect::Ts);
    for tail in [
        "const u: User = { na| };",
        "const u = { na| } as User;",
        "function make(): User {\n  return { na| };\n}",
        "function show(u: User) {}\nshow({ na| });",
        "const admins: { boss: User } = { boss: { na| } };",
    ] {
        let (text, offset) = at(&format!("{USER}{tail}"));
        let completion = complete(&text, offset, false, None).unwrap_or_else(|| panic!("nothing for {tail}"));
        let names = labels(&completion);
        assert!(names.contains(&"name".to_string()) && names.contains(&"age".to_string()), "{tail}: {names:?}");
        assert_eq!(completion.items.iter().find(|i| i.label == "name").unwrap().insert, "name: ");
    }
    // What is already there is not offered again.
    let (text, offset) = at(&format!("{USER}const u: User = {{ name: 'x', a| }};"));
    let names = labels(&complete(&text, offset, false, None).unwrap());
    assert!(names.contains(&"age".to_string()) && !names.contains(&"name".to_string()), "{names:?}");
}

#[test]
fn hover_and_navigation_know_declared_types() {
    let _dialect = use_dialect(Dialect::Ts);
    let source = format!("{USER}const u: User = load();\nconst id: Id = undefined;\nu.greet();\n");
    let found = |needle: &str, shift: usize| source.find(needle).unwrap() + shift;
    let title = |offset: usize| hover(&source, offset).map(|h| h.title).unwrap_or_default();
    assert_eq!(title(found("u: User", 0)), "const u: User");
    assert_eq!(title(found("User = load", 1)), "interface User");
    assert_eq!(title(found("Id = undefined", 1)), "type Id = string | undefined");
    assert_eq!(title(found("Color {", 1)), "enum Color");
    assert_eq!(title(found("string;", 1)), "string");
    assert_eq!(title(found("greet();", 1)), "(method) greet(): string");
    assert!(hover(&source, source.find("string;").unwrap() + 1).unwrap().body.contains("Texte"));

    // A type leads to its declaration, a member to its signature.
    let to = definition(&source, found("User = load", 1), None);
    assert_eq!(to.len(), 1);
    assert_eq!(to[0].line, 0);
    let to = definition(&source, found("greet();", 1), None);
    assert_eq!((to.len(), to[0].line), (1, 3));
    // The uses of a type include the places it is written.
    let uses = references(&source, found("User {", 1));
    assert!(uses.len() >= 2, "{uses:?}");
}

#[test]
fn the_structure_has_interfaces_types_enums_and_class_members() {
    let _dialect = use_dialect(Dialect::Ts);
    let source = format!("{USER}export abstract class Repo<T> {{\n  private items: T[] = [];\n  abstract find(id: string): T;\n  count(): number {{ return 0; }}\n}}\nnamespace Util {{ export function run() {{}} }}\n");
    let all = symbols(&source);
    let summary: Vec<(String, ItemKind, usize)> = all.iter().map(|s| (s.name.clone(), s.kind, s.depth)).collect();
    for expected in [
        ("User", ItemKind::Interface, 0),
        ("name", ItemKind::Property, 1),
        ("greet", ItemKind::Method, 1),
        ("Admin", ItemKind::Interface, 0),
        ("Color", ItemKind::Enum, 0),
        ("Red", ItemKind::Constant, 1),
        ("Id", ItemKind::Interface, 0),
        ("Repo", ItemKind::Class, 0),
        ("items", ItemKind::Property, 1),
        ("find", ItemKind::Method, 1),
        ("count", ItemKind::Method, 1),
        ("Util", ItemKind::Module, 0),
        ("run", ItemKind::Function, 1),
    ] {
        assert!(summary.iter().any(|(n, k, d)| (n.as_str(), *k, *d) == expected), "{expected:?} in {summary:?}");
    }
    assert_eq!(all.iter().find(|s| s.name == "Admin").unwrap().detail, "extends User");
}

#[test]
fn imports_of_types_go_without_extension_and_packages_are_typed() {
    let root = std::env::temp_dir().join(format!("atelier-ts-imports-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("node_modules").join("@types").join("lib")).unwrap();
    std::fs::write(root.join("src").join("user.ts"), "export interface Profile { id: number }\nexport function makeProfile(): Profile { return { id: 1 }; }\n").unwrap();
    std::fs::write(root.join("node_modules").join("@types").join("lib").join("index.d.ts"), "export declare function lib(): void;\n").unwrap();
    let file = root.join("src").join("main.ts");
    let _dialect = use_dialect(Dialect::Ts);

    let source = "const p: Profile = makeProfile();\n";
    let fixes = import_fixes(source, source.find("Profile").unwrap() + 1, Some(&file), &root);
    assert_eq!(fixes.len(), 1, "{fixes:?}");
    assert_eq!(fixes[0].title, "Importer « Profile » depuis « ./user »");
    let fixes = import_fixes(source, source.find("makeProfile").unwrap() + 1, Some(&file), &root);
    assert_eq!(fixes[0].title, "Importer « makeProfile » depuis « ./user »");
    // The library types need nothing.
    let source = "const m: Promise<Map<string, number>> = load();\n";
    assert!(import_fixes(source, source.find("Promise").unwrap() + 1, Some(&file), &root).is_empty());
    assert!(import_fixes(source, source.find("Map").unwrap() + 1, Some(&file), &root).is_empty());

    // A package that has no declarations of its own is typed by @types.
    let (text, offset) = at("import { lib } from 'lib';\n|lib();\n");
    let found = definition(&text, offset, Some(&file));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].path.as_ref().unwrap().ends_with("index.d.ts"), "{found:?}");
    let _ = std::fs::remove_dir_all(&root);
}
