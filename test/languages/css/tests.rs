use super::analysis::*;
use super::lexer::{self, Dialect, Role};
use crate::languages::ItemKind;

/// The role of the first `needle` of `text`.
fn role_at(text: &str, dialect: Dialect, needle: &str) -> Role {
    let at = text
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} in {text:?}"));
    let (tokens, _) = lexer::tokens(text, dialect);
    let roles = lexer::roles(text, &tokens, dialect);
    tokens
        .iter()
        .zip(roles)
        .find(|(t, _)| t.range.start <= at && at < t.range.end)
        .map_or(Role::Text, |(_, r)| r)
}

#[test]
fn css_selectors_properties_and_values_have_their_roles() {
    let text = "a.button:hover > #main, ul li::before {\n  color: #fff !important;\n  margin: -2px calc(100% - 4em);\n  background: url(img/a.png);\n}\n@media screen and (min-width: 600px) { }";
    let d = Dialect::Css;
    assert_eq!(role_at(text, d, "a."), Role::Tag);
    assert_eq!(role_at(text, d, "button"), Role::Class);
    assert_eq!(role_at(text, d, "hover"), Role::Pseudo);
    assert_eq!(role_at(text, d, "#main"), Role::Class);
    assert_eq!(role_at(text, d, "before"), Role::Pseudo);
    assert_eq!(role_at(text, d, "color"), Role::Property);
    assert_eq!(role_at(text, d, "#fff"), Role::Number);
    assert_eq!(role_at(text, d, "!important"), Role::Keyword);
    assert_eq!(role_at(text, d, "-2px"), Role::Number);
    assert_eq!(role_at(text, d, "calc"), Role::Function);
    assert_eq!(role_at(text, d, "img/a.png"), Role::String);
    assert_eq!(role_at(text, d, "@media"), Role::Keyword);
    assert_eq!(role_at(text, d, "and"), Role::Keyword);
    assert_eq!(role_at(text, d, "min-width"), Role::Property);
}

#[test]
fn scss_nesting_variables_mixins_and_interpolation() {
    let text = "$primary: #333;\n@mixin card($pad) { padding: $pad; }\n.list {\n  &:hover { color: darken($primary, 10%); }\n  &-item { @include card(4px); }\n  .#{$name}-x { top: 0 } // note\n}";
    let d = Dialect::Scss;
    assert_eq!(role_at(text, d, "$primary"), Role::Variable);
    assert_eq!(role_at(text, d, "@mixin"), Role::Keyword);
    assert_eq!(role_at(text, d, "card"), Role::Function);
    assert_eq!(role_at(text, d, "padding"), Role::Property);
    assert_eq!(role_at(text, d, "&:hover"), Role::Keyword);
    assert_eq!(role_at(text, d, "hover"), Role::Pseudo);
    assert_eq!(role_at(text, d, "darken"), Role::Function);
    assert_eq!(role_at(text, d, "-item"), Role::Class);
    assert_eq!(role_at(text, d, "#{"), Role::Interpolation);
    assert_eq!(role_at(text, d, "top"), Role::Property);
    assert_eq!(role_at(text, d, "// note"), Role::Comment);
}

#[test]
fn in_css_a_double_slash_is_no_comment() {
    let text = "a { b: c } // d";
    assert_ne!(role_at(text, Dialect::Css, "// d"), Role::Comment);
    assert_eq!(role_at(text, Dialect::Scss, "// d"), Role::Comment);
}

#[test]
fn sass_lines_and_indentation_make_the_statements() {
    let text = "$c: red\n.a\n  color: $c\n  &:hover\n    color: blue\n=mixin($x)\n  margin: $x\n.b\n  +mixin(2px)\n";
    let d = Dialect::Sass;
    assert_eq!(role_at(text, d, ".a"), Role::Class);
    assert_eq!(role_at(text, d, "color"), Role::Property);
    assert_eq!(role_at(text, d, "hover"), Role::Pseudo);
    assert_eq!(role_at(text, d, "mixin"), Role::Function);
    assert_eq!(role_at(text, d, "margin"), Role::Property);
    let symbols = symbols(text, d);
    let names: Vec<(&str, usize)> = symbols.iter().map(|s| (s.name.as_str(), s.depth)).collect();
    assert_eq!(
        names,
        [
            ("$c", 0),
            (".a", 0),
            ("&:hover", 1),
            ("mixin", 0),
            (".b", 0)
        ]
    );
}

#[test]
fn less_variables_are_variables() {
    let text = "@base: #f04615;\n.a { color: @base; }";
    assert_eq!(role_at(text, Dialect::Less, "@base:"), Role::Variable);
    let use_at = text.rfind("@base").unwrap();
    let places = definition(text, use_at + 1, Dialect::Less);
    assert_eq!(places.len(), 1);
    assert_eq!((places[0].line, places[0].column), (0, 0));
}

#[test]
fn braces_and_strings_left_open_are_errors() {
    let errors = syntax_errors("a { color: red;\nb { x: \"y }\n", Dialect::Css);
    let messages: Vec<&str> = errors.iter().map(|e| e.message.as_str()).collect();
    assert!(messages.iter().any(|m| m.contains('{')), "{messages:?}");
    assert!(errors.len() >= 2, "{messages:?}");
    assert!(syntax_errors("a { b: c; }\n/* ok */", Dialect::Css).is_empty());
    assert_eq!(syntax_errors("a { b: c; } }", Dialect::Css).len(), 1);
    assert_eq!(syntax_errors("a { b: f(x; }", Dialect::Css).len(), 1);
    // Interpolation braces are not blocks.
    assert!(syntax_errors(".a-#{$b} { c: d; }", Dialect::Scss).is_empty());
    assert_eq!(syntax_errors("a { color: ; }", Dialect::Css).len(), 1);
}

#[test]
fn the_structure_lists_rules_variables_and_mixins() {
    let text = "$gap: 4px;\n@mixin flex($dir) { display: flex; }\n@media print {\n  .a, .b { color: red; }\n}\n";
    let symbols = symbols(text, Dialect::Scss);
    let found: Vec<(&str, ItemKind, usize)> = symbols
        .iter()
        .map(|s| (s.name.as_str(), s.kind, s.depth))
        .collect();
    assert_eq!(
        found,
        [
            ("$gap", ItemKind::Variable, 0),
            ("flex", ItemKind::Function, 0),
            ("@media print", ItemKind::Module, 0),
            (".a, .b", ItemKind::Class, 1),
        ]
    );
    assert_eq!(symbols[0].detail, "4px");
}

#[test]
fn a_variable_a_mixin_and_a_placeholder_lead_to_their_declaration() {
    let text = "$size: 2px;\n%base { a: b; }\n@mixin m { }\n.x {\n  @extend %base;\n  @include m;\n  width: $size;\n}";
    let goto = |needle: &str| {
        let at = text.rfind(needle).unwrap() + 1;
        definition(text, at, Dialect::Scss)
            .into_iter()
            .map(|l| l.line)
            .collect::<Vec<_>>()
    };
    assert_eq!(goto("$size"), [0]);
    assert_eq!(goto("%base"), [1]);
    assert_eq!(goto(" m;"), [2]);
    let at = text.find("$size").unwrap();
    assert_eq!(highlights(text, at, Dialect::Scss).len(), 2);
}

#[test]
fn custom_properties_lead_to_their_declaration() {
    let text = ":root { --accent: red; }\na { color: var(--accent); }";
    let at = text.rfind("--accent").unwrap() + 2;
    assert_eq!(definition(text, at, Dialect::Css).len(), 1);
}

fn labels(text: &str, dialect: Dialect) -> Vec<String> {
    let offset = text.find('|').unwrap();
    let text = text.replace('|', "");
    complete(&text, offset, true, dialect)
        .map(|c| c.items.into_iter().map(|i| i.label).collect())
        .unwrap_or_default()
}

#[test]
fn completion_knows_where_it_is() {
    let properties = labels("a {\n  back|\n}", Dialect::Css);
    assert!(properties.contains(&"background-color".to_string()));
    let values = labels("a {\n  display: fl|\n}", Dialect::Css);
    assert!(values.contains(&"flex".to_string()), "{values:?}");
    assert!(values.contains(&"inherit".to_string()));
    let colors = labels("a { color: |}", Dialect::Css);
    assert!(colors.contains(&"rebeccapurple".to_string()));
    let pseudo = labels("a:|", Dialect::Css);
    assert!(pseudo.contains(&"hover".to_string()), "{pseudo:?}");
    let elements = labels("a::|", Dialect::Css);
    assert!(elements.contains(&"before".to_string()), "{elements:?}");
    let at = labels("@me|", Dialect::Css);
    assert!(at.contains(&"@media".to_string()));
    assert!(!at.contains(&"@mixin".to_string()));
    let scss_at = labels("@mi|", Dialect::Scss);
    assert!(scss_at.contains(&"@mixin".to_string()));
    let variables = labels("$main: 1;\n$other: 2;\na { b: $|", Dialect::Scss);
    assert_eq!(variables, ["$main", "$other"]);
    let mixins = labels("@mixin round { }\n.a { @include |", Dialect::Scss);
    assert_eq!(mixins, ["round"]);
    let keyframes = labels("@keyframes spin { }\na { animation-name: |", Dialect::Css);
    assert!(keyframes.contains(&"spin".to_string()));
    // Nothing in a comment.
    assert!(labels("/* back| */", Dialect::Css).is_empty());
}

#[test]
fn a_property_is_inserted_with_its_colon_and_semicolon() {
    let completion = complete("a { colo }", 8, true, Dialect::Css).unwrap();
    let color = completion
        .items
        .iter()
        .find(|i| i.label == "color")
        .unwrap();
    assert_eq!(color.insert, "color: ;");
    assert_eq!(color.caret, Some(7));
    assert_eq!(completion.from, 4);
}

#[test]
fn a_minified_sheet_is_printed_out_clean() {
    let text =
        "a,b>c{color:red;margin:0 auto}@media (min-width:600px) and (max-width:900px){.x{top:0}}";
    let clean = super::format::reformat(text, Dialect::Css, "  ").unwrap();
    assert_eq!(
        clean,
        "a,\nb > c {\n  color: red;\n  margin: 0 auto;\n}\n\n\
         @media (min-width: 600px) and (max-width: 900px) {\n  .x {\n    top: 0;\n  }\n}\n"
    );
}

#[test]
fn reformat_keeps_comments_blank_lines_and_scss() {
    let text = "/* head */\n$c:#fff; // the color\n.a{\n  &:hover{color:darken($c,10%)}\n\n\n  @include m(1px,2px);\n  .#{$n}-x{top:0}\n}\n";
    let clean = super::format::reformat(text, Dialect::Scss, "  ").unwrap();
    assert_eq!(
        clean,
        "/* head */\n$c: #fff; // the color\n.a {\n  &:hover {\n    color: darken($c, 10%);\n  }\n\n  @include m(1px, 2px);\n  .#{$n}-x {\n    top: 0;\n  }\n}\n"
    );
    // Already clean: unchanged.
    assert_eq!(
        super::format::reformat(&clean, Dialect::Scss, "  ").unwrap(),
        clean
    );
    assert!(super::format::reformat("a\n  b: c\n", Dialect::Sass, "  ").is_err());
    assert_eq!(indent_of("a {\n    b: c;\n}"), "    ");
}

#[test]
fn minify_keeps_what_means_something() {
    let text = "/*! license */\n/* note */\na :hover, b > c {\n  color : red ;\n  width: calc(100% - 2px);\n}\n@media screen and (min-width: 600px) {\n  .x { top: 0; }\n}\n";
    let small = super::format::minify(text, Dialect::Css).unwrap();
    assert_eq!(
        small,
        "/*! license */a :hover,b>c{color:red;width:calc(100% - 2px)}@media screen and (min-width:600px){.x{top:0}}"
    );
    // Minified then printed again: the same rules.
    let again = super::format::reformat(&small, Dialect::Css, "  ").unwrap();
    assert!(again.contains("width: calc(100% - 2px);"), "{again}");
}

#[test]
fn colors_are_found_and_written_back_in_their_notation() {
    use super::colors::{Notation, find, write};
    let text = "a { color: #f00; background: rgba(0, 0, 255, 0.5); border-color: hsl(120, 100%, 25%); outline: 1px solid tomato; }\n.red { x: y }";
    let found = find(text, Dialect::Css);
    let colors: Vec<(&str, [u8; 4])> = found
        .iter()
        .map(|s| (&text[s.range.clone()], s.rgba))
        .collect();
    assert_eq!(
        colors,
        [
            ("#f00", [255, 0, 0, 255]),
            ("rgba(0, 0, 255, 0.5)", [0, 0, 255, 128]),
            ("hsl(120, 100%, 25%)", [0, 128, 0, 255]),
            ("tomato", [255, 99, 71, 255]),
        ]
    );
    assert_eq!(found[1].notation, Notation::Rgb);
    // A selector named after a color is no color.
    assert!(!colors.iter().any(|(t, _)| *t == "red"));
    assert_eq!(write([18, 52, 86, 255], Notation::Hex), "#123456");
    assert_eq!(
        write([0, 0, 255, 128], Notation::Rgb),
        "rgba(0, 0, 255, 0.5)"
    );
    assert_eq!(
        write([0, 128, 0, 255], Notation::Hsl),
        "hsl(120, 100%, 25%)"
    );
    // A variable inside: the color is not known.
    assert!(find("a { color: rgb($r, 0, 0) }", Dialect::Scss).is_empty());
}

#[test]
fn a_css_file_is_minified_next_to_itself() {
    let dir = std::env::temp_dir().join(format!("atelier-minify-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("site.css");
    std::fs::write(&file, "a {\n  color: red;\n}\n").unwrap();
    super::minify_file(&file).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("site.min.css")).unwrap(),
        "a{color:red}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_dialect_comes_from_the_extension_or_the_text() {
    assert_eq!(Dialect::of("scss"), Dialect::Scss);
    assert_eq!(
        Dialect::of_path(Some(std::path::Path::new("a/b.LESS"))),
        Dialect::Less
    );
    assert_eq!(Dialect::guess(".a\n  color: red\n"), Dialect::Sass);
    assert_eq!(Dialect::guess(".a { color: red }"), Dialect::Scss);
}

#[test]
fn a_translucent_color_comes_back_to_the_unit() {
    use super::colors::{find, write};
    let text = "a { background: rgba(53, 116, 240, 0.15); color: #3672f226; }";
    let found = find(text, Dialect::Css);
    assert_eq!(found[0].rgba, [53, 116, 240, 38]);
    assert_eq!(
        write(found[0].rgba, found[0].notation),
        "rgba(53, 116, 240, 0.15)"
    );
    assert_eq!(write(found[1].rgba, found[1].notation), "#3672f226");
}

fn convert(text: &str, from: Dialect, to: Dialect) -> String {
    super::convert::convert(text, from, to).unwrap()
}

#[test]
fn css_becomes_nested_scss() {
    let css = "/* card */\n.card { padding: 4px; }\n.card .title { font-weight: bold; }\n.card:hover { color: red; }\n.card > .icon { width: 1em; }\n@media (max-width: 600px) {\n  .card { padding: 0; }\n}\n";
    assert_eq!(
        convert(css, Dialect::Css, Dialect::Scss),
        "/* card */\n.card {\n  padding: 4px;\n  .title {\n    font-weight: bold;\n  }\n  &:hover {\n    color: red;\n  }\n  > .icon {\n    width: 1em;\n  }\n}\n\n@media (max-width: 600px) {\n  .card {\n    padding: 0;\n  }\n}\n"
    );
}

#[test]
fn scss_becomes_sass_and_back() {
    let scss = "$main: #333 !default;\n@mixin round($r: 4px) {\n  border-radius: $r;\n}\n.box {\n  color: $main;\n  @include round(2px);\n  @extend %base;\n  .#{$name}-x { top: 0; }\n}\n";
    let sass = convert(scss, Dialect::Scss, Dialect::Sass);
    assert_eq!(
        sass,
        "$main: #333 !default\n=round($r: 4px)\n  border-radius: $r\n\n.box\n  color: $main\n  +round(2px)\n  @extend %base\n  .#{$name}-x\n    top: 0\n"
    );
    // And back: the same SCSS, the blank line between the rules aside.
    assert_eq!(
        convert(&sass, Dialect::Sass, Dialect::Scss),
        "$main: #333 !default;\n@mixin round($r: 4px) {\n  border-radius: $r;\n}\n\n.box {\n  color: $main;\n  @include round(2px);\n  @extend %base;\n  .#{$name}-x {\n    top: 0;\n  }\n}\n"
    );
}

#[test]
fn scss_becomes_less_and_back() {
    let scss = "$main: #333 !default;\n@mixin round($r) {\n  border-radius: $r;\n}\n.box {\n  color: darken($main, 10%);\n  @include round(2px);\n  @extend .base;\n  .#{$name}-x { top: 0; }\n}\n";
    let less = convert(scss, Dialect::Scss, Dialect::Less);
    assert_eq!(
        less,
        "@main: #333;\n.round(@r) {\n  border-radius: @r;\n}\n\n.box {\n  color: darken(@main, 10%);\n  .round(2px);\n  &:extend(.base);\n  .@{name}-x {\n    top: 0;\n  }\n}\n"
    );
    let back = convert(&less, Dialect::Less, Dialect::Scss);
    assert!(back.contains("$main: #333;"), "{back}");
    assert!(back.contains("@mixin round($r) {"), "{back}");
    assert!(back.contains("@include round(2px);"), "{back}");
    assert!(back.contains("@extend .base;"), "{back}");
    assert!(back.contains("color: darken($main, 10%);"), "{back}");
}

#[test]
fn a_converted_file_never_overwrites_one() {
    let dir = std::env::temp_dir().join(format!("atelier-convert-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("site.css");
    std::fs::write(&file, "a { b: c; }\n").unwrap();
    std::fs::write(dir.join("site.scss"), "keep").unwrap();
    let written = super::convert_file(&file, Dialect::Scss).unwrap();
    assert_eq!(written, dir.join("site-2.scss"));
    assert_eq!(
        std::fs::read_to_string(dir.join("site.scss")).unwrap(),
        "keep"
    );
    assert!(super::convert::convert("a{}", Dialect::Scss, Dialect::Css).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
