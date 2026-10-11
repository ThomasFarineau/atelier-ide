use super::*;

const SAMPLE: &str = "\
package demo;

import java.util.List;

public class Shop {
    private final List<String> items = List.of();
    static int count;

    public Shop(int size) {
        count = size;
    }

    public int total(int price, int quantity) {
        int sum = price * quantity;
        for (String item : items) {
            sum += item.length();
        }
        return sum;
    }

    enum Kind { BOOK, PEN }

    interface Priced {
        int price();
    }

    record Line(String name, int amount) {}
}
";

#[test]
fn the_structure_lists_types_and_their_members() {
    let symbols = symbols(SAMPLE);
    let shown: Vec<(&str, usize)> = symbols.iter().map(|s| (s.name.as_str(), s.depth)).collect();
    assert_eq!(
        shown,
        [
            ("Shop", 0),
            ("items", 1),
            ("count", 1),
            ("Shop", 1),
            ("total", 1),
            ("Kind", 1),
            ("BOOK", 2),
            ("PEN", 2),
            ("Priced", 1),
            ("price", 2),
            ("Line", 1),
        ]
    );
    let total = symbols.iter().find(|s| s.name == "total").unwrap();
    assert_eq!(total.kind, ItemKind::Method);
    assert_eq!(total.detail, "(int price, int quantity): int");
    let items = symbols.iter().find(|s| s.name == "items").unwrap();
    assert_eq!(items.detail, "List<String>");
}

#[test]
fn syntax_errors_are_found() {
    assert!(
        syntax_errors(SAMPLE).is_empty(),
        "{:?}",
        syntax_errors(SAMPLE)
    );
    assert!(!syntax_errors("class A { void f() { int x = ; } }").is_empty());
}

#[test]
fn a_name_goes_to_its_declaration() {
    let use_of_sum = SAMPLE.rfind("sum;").unwrap();
    let found = definition(SAMPLE, use_of_sum + 1);
    assert_eq!(found.len(), 1);
    let declared = SAMPLE.find("int sum").unwrap() + 4;
    assert_eq!(found[0].range_in(SAMPLE), declared..declared + 3);
    let use_of_item = SAMPLE.find("item.length").unwrap();
    let found = definition(SAMPLE, use_of_item);
    let declared = SAMPLE.find("String item ").unwrap() + 7;
    assert_eq!(found[0].range_in(SAMPLE), declared..declared + 4);
    let use_of_count = SAMPLE.find("count = size").unwrap();
    let found = definition(SAMPLE, use_of_count);
    let declared = SAMPLE.find("int count").unwrap() + 4;
    assert_eq!(found[0].range_in(SAMPLE), declared..declared + 5, "a field");
    assert!(
        definition(SAMPLE, SAMPLE.find("length").unwrap()).is_empty(),
        "a member of another object"
    );
}

#[test]
fn occurrences_of_a_name_are_highlighted() {
    assert_eq!(highlights(SAMPLE, SAMPLE.find("sum").unwrap()).len(), 3);
    assert_eq!(
        highlights(SAMPLE, SAMPLE.find("quantity").unwrap()).len(),
        2
    );
}

#[test]
fn completion_offers_names_keywords_and_live_templates() {
    let src = "class A {\n    int total;\n    void run(int size) {\n        so\n    }\n    void other(int hidden) {}\n}\n";
    let at = src.find("        so").unwrap() + 10;
    let completion = complete(src, at, false).unwrap();
    let labels: Vec<&str> = completion.items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"total"));
    assert!(labels.contains(&"size"));
    assert!(!labels.contains(&"hidden"), "a parameter of another method");
    assert!(labels.contains(&"sout"));
    let sout = completion.items.iter().find(|i| i.label == "sout").unwrap();
    assert_eq!(&sout.insert[..sout.caret.unwrap()], "System.out.println(");
    let dotted = "class A { void f() { String s = \"\"; s.le } }";
    assert!(complete(dotted, dotted.find(".le").unwrap() + 3, true).is_none());
}
