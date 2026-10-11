use super::*;

#[test]
fn the_structure_lists_functions_classes_members_and_constants() {
    let source = "const LIMIT = 3;\nlet counter = 0;\nexport function add(a, b) {\n  function inner() {}\n  return a + b;\n}\nconst twice = (x) => x * 2;\nclass Dog extends Animal {\n  name = 'rex';\n  constructor(n) {}\n  bark() {}\n  static create() {}\n}\n";
    let all = symbols(source);
    let summary: Vec<(String, ItemKind, usize)> = all
        .iter()
        .map(|s| (s.name.clone(), s.kind, s.depth))
        .collect();
    assert_eq!(
        summary,
        [
            ("LIMIT".to_string(), ItemKind::Constant, 0),
            ("counter".to_string(), ItemKind::Variable, 0),
            ("add".to_string(), ItemKind::Function, 0),
            ("inner".to_string(), ItemKind::Function, 1),
            ("twice".to_string(), ItemKind::Function, 0),
            ("Dog".to_string(), ItemKind::Class, 0),
            ("name".to_string(), ItemKind::Property, 1),
            ("constructor".to_string(), ItemKind::Method, 1),
            ("bark".to_string(), ItemKind::Method, 1),
            ("create".to_string(), ItemKind::Method, 1),
        ]
    );
    assert_eq!(all[2].detail, "(a, b)");
    assert_eq!(all[5].detail, "extends Animal");
    assert_eq!(&source[all[2].range.clone()], "add");
}
