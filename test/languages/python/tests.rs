use super::analysis::*;
use crate::languages::ItemKind;

const SAMPLE: &str = "import os\nfrom typing import List as L\n\nMAX = 10\ncount = 0\n\n\nclass Shape(Base):\n    sides = 0\n\n    def __init__(self, name: str, size=1):\n        self.name = name\n        self.size = size\n\n    @property\n    def area(self) -> float:\n        total = self.size * 2\n        return total\n\n    def describe(self):\n        return self.name + str(self.area)\n\n\ndef main(argv):\n    shape = Shape(argv[0])\n    for i, part in enumerate(argv):\n        print(i, part, count)\n    return [x * 2 for x in argv]\n";

fn at(text: &str, needle: &str) -> usize {
    text.rfind(needle).unwrap_or_else(|| panic!("{needle:?}")) + 1
}

#[test]
fn the_structure_lists_classes_methods_and_module_names() {
    let found: Vec<(String, ItemKind, usize)> = symbols(SAMPLE)
        .into_iter()
        .map(|s| (s.name, s.kind, s.depth))
        .collect();
    assert_eq!(
        found,
        [
            ("MAX".into(), ItemKind::Constant, 0),
            ("count".into(), ItemKind::Variable, 0),
            ("Shape".into(), ItemKind::Class, 0),
            ("sides".into(), ItemKind::Property, 1),
            ("__init__".into(), ItemKind::Method, 1),
            ("area".into(), ItemKind::Method, 1),
            ("describe".into(), ItemKind::Method, 1),
            ("main".into(), ItemKind::Function, 0),
        ]
    );
    let area = symbols(SAMPLE)
        .into_iter()
        .find(|s| s.name == "area")
        .unwrap();
    assert_eq!(area.detail, "(self) -> float");
}

#[test]
fn a_name_leads_to_its_declaration_in_its_scope() {
    let line_of = |needle: &str| {
        definition(SAMPLE, at(SAMPLE, needle))
            .first()
            .map(|l| l.line)
    };
    // `total` of `area`, `count` of the module, a parameter, a loop variable, an import alias.
    assert_eq!(line_of("total\n"), Some(16));
    assert_eq!(line_of("count)"), Some(4));
    assert_eq!(line_of("argv[0]"), Some(23));
    assert_eq!(line_of("part, count"), Some(25));
    assert_eq!(line_of("Shape(argv"), Some(7));
    assert_eq!(definition("from a import b as c\nc()\n", 21)[0].line, 0);
    // `self.name` and `self.area`: in the class.
    assert_eq!(line_of("name + str"), Some(11));
    assert_eq!(line_of("area)"), Some(15));
}

#[test]
fn occurrences_stay_in_their_scope() {
    // `size` the parameter of `__init__`, not `self.size`.
    let text = "def f(size):\n    return size\n\ndef g(size):\n    return size\n";
    let ranges = highlights(text, at(text, "size):\n    return size\n\ndef g"));
    assert_eq!(ranges.len(), 2);
    assert!(ranges.iter().all(|r| r.start < 30));
    // `self.size` in the whole class.
    let members = highlights(SAMPLE, at(SAMPLE, "size * 2"));
    assert_eq!(members.len(), 2, "{members:?}");
}

#[test]
fn syntax_errors_are_found() {
    assert!(!syntax_errors("def f(:\n    pass\n").is_empty());
    assert!(syntax_errors(SAMPLE).is_empty());
}

fn labels(text: &str) -> Vec<String> {
    let offset = text.find('|').unwrap();
    let text = text.replace('|', "");
    complete(&text, offset, true)
        .map(|c| c.items.into_iter().map(|i| i.label).collect())
        .unwrap_or_default()
}

#[test]
fn completion_knows_the_file_self_and_templates() {
    let names = labels("value = 1\ndef run(items):\n    va|\n");
    assert!(names.contains(&"value".to_string()));
    assert!(names.contains(&"items".to_string()));
    assert!(names.contains(&"print".to_string()));
    assert!(
        names.contains(&"main".to_string()),
        "templates at the start of a statement"
    );
    let members = labels(
        "class A:\n    def __init__(self):\n        self.x = 1\n    def go(self):\n        self.|\n",
    );
    assert_eq!(members, ["__init__", "go", "x"]);
    // Nothing in a string or a comment, nothing after another object's dot.
    assert!(labels("s = \"pri|\"\n").is_empty());
    assert!(labels("# pri|\n").is_empty());
    assert!(labels("os.pa|\n").is_empty());
}

#[test]
fn a_template_takes_the_indentation_of_its_line() {
    let text = "def f():\n    tr";
    let completion = complete(text, text.len(), true).unwrap();
    let item = completion
        .items
        .iter()
        .find(|i| i.label == "try" && i.kind == ItemKind::Snippet)
        .unwrap();
    assert_eq!(
        item.insert,
        "try:\n        \n    except Exception as e:\n        raise"
    );
    assert_eq!(item.caret, Some(13));
}
