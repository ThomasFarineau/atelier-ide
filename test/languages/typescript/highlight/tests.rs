use super::*;

fn palette() -> Palette {
    let c = |n: u8| Color32::from_rgb(n, n, n);
    Palette {
        keyword: c(1),
        string: c(2),
        number: c(3),
        comment: c(4),
        function: c(5),
        constant: c(6),
        annotation: c(7),
        tag: c(8),
        escape: c(9),
        text: c(0),
    }
}

fn color_of(
    lines: &[Vec<(Range<usize>, Color32)>],
    line: usize,
    text: &str,
    word: &str,
) -> Option<u8> {
    let source = text.lines().nth(line).unwrap();
    let at = source.find(word)?;
    lines[line]
        .iter()
        .find(|(r, _)| r.start <= at && at < r.end)
        .map(|(_, c)| c.r())
}

#[test]
fn the_whole_typescript_syntax_is_colored() {
    let text = "interface User {\n  name: string;\n  count?: number;\n}\n// note\ntype Id = string | number;\nenum Color { Red = 1 }\nexport class Box<T> implements User {\n  private readonly items: T[] = [];\n  constructor(public label: string) {}\n  add(item: T): void { this.items.push(item); }\n}\nconst msg: string = `hi ${1}`;\nlet n = 42 as const;\n@Injectable()\nclass Service {}\n";
    let lines = colors(text, &palette()).expect("the queries compile");
    assert_eq!(lines.len(), text.split('\n').count());
    assert_eq!(color_of(&lines, 0, text, "interface"), Some(1), "interface");
    assert_eq!(
        color_of(&lines, 1, text, "string"),
        Some(1),
        "a primitive type"
    );
    assert_eq!(color_of(&lines, 2, text, "number"), Some(1));
    assert_eq!(color_of(&lines, 4, text, "note"), Some(4), "a comment");
    assert_eq!(color_of(&lines, 5, text, "type"), Some(1), "type alias");
    assert_eq!(color_of(&lines, 6, text, "enum"), Some(1));
    assert_eq!(color_of(&lines, 7, text, "implements"), Some(1));
    assert_eq!(color_of(&lines, 8, text, "private"), Some(1), "a modifier");
    assert_eq!(color_of(&lines, 8, text, "readonly"), Some(1));
    assert_eq!(
        color_of(&lines, 12, text, "hi"),
        Some(2),
        "a template string"
    );
    assert_eq!(color_of(&lines, 13, text, "42"), Some(3));
    assert_eq!(color_of(&lines, 13, text, "const"), Some(1));
}

#[test]
fn tsx_is_colored_when_typescript_alone_does_not_parse() {
    let text = "const App = () => <div className=\"a\">{1}</div>;\n";
    let lines = colors(text, &palette()).expect("the tsx grammar is used");
    assert_eq!(color_of(&lines, 0, text, "\"a\""), Some(2));
}
