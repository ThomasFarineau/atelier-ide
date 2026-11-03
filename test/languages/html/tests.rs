use super::*;
use crate::languages::{Severity, format::apply};

const PAGE: &str = r#"<!DOCTYPE html>
<html lang="fr">
<head>
  <title>Test &amp; co</title>
  <style>
    .box { color: red; }
  </style>
</head>
<body>
  <div id="main" class="box wide">
    <label for="name">Nom</label>
    <input id="name" type="text" onclick="go()">
    <p style="margin: 0">a < b</p>
  </div>
  <script>
    function go() { return 1; }
  </script>
</body>
</html>
"#;

fn at(text: &str, needle: &str) -> usize {
    text.find(needle)
        .unwrap_or_else(|| panic!("{needle:?} in {text:?}"))
}

// ---- Reading -----------------------------------------------------------------------------

#[test]
fn tags_attributes_and_raw_text_are_read() {
    let scan = lexer::scan(PAGE);
    let div = scan
        .tags
        .iter()
        .find(|t| t.name == "div" && !t.closing)
        .unwrap();
    assert_eq!(div.value(PAGE, "id"), Some("main"));
    assert_eq!(div.value(PAGE, "class"), Some("box wide"));
    // `a < b` is text: no tag named ` b`.
    assert!(!scan.tags.iter().any(|t| t.name.is_empty() || t.name == "b"));
    // The script is not markup: no tag inside it.
    let script = scan
        .raw
        .iter()
        .find(|r| scan.tags[r.tag].name == "script")
        .unwrap();
    assert!(PAGE[script.content.clone()].contains("function go()"));
    assert!(
        scan.tokens
            .iter()
            .any(|t| t.kind == lexer::Kind::Entity && &PAGE[t.range.clone()] == "&amp;")
    );
    assert!(scan.unclosed.is_empty());
}

#[test]
fn a_script_ends_only_at_its_closing_tag() {
    let text = "<script>if (a < b && c > d) { x = '<div>'; }</script><p>after</p>";
    let scan = lexer::scan(text);
    let names: Vec<&str> = scan.tags.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["script", "script", "p", "p"]);
}

#[test]
fn omitted_closing_tags_close_where_html_closes_them() {
    let text = "<ul>\n<li>one\n<li>two\n</ul>\n<p>a\n<div>b</div>";
    let scan = lexer::scan(text);
    let tree = tree::build(&scan);
    assert!(tree.unclosed.is_empty(), "{:?}", tree.unclosed);
    assert!(tree.stray.is_empty());
    // The second `li` is a sibling of the first, not its child.
    let depths: Vec<(String, usize)> = tree
        .elements
        .iter()
        .map(|e| (scan.tags[e.open].name.clone(), e.depth))
        .collect();
    assert!(depths.contains(&("li".into(), 1)));
    assert!(!depths.contains(&("li".into(), 2)));
    // The `div` ends the paragraph.
    assert!(depths.contains(&("div".into(), 0)));
}

// ---- Errors ------------------------------------------------------------------------------

#[test]
fn a_clean_page_has_no_errors() {
    let errors = analysis::syntax_errors(PAGE);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn unclosed_and_stray_tags_are_reported() {
    let text = "<div>\n  <span>text\n</div>\n</section>";
    let errors = analysis::syntax_errors(text);
    let unclosed = errors.iter().find(|d| d.message.contains("span")).unwrap();
    assert_eq!(unclosed.severity, Severity::Warning);
    assert_eq!(&text[unclosed.range.clone()], "span");
    let stray = errors
        .iter()
        .find(|d| d.message.contains("section"))
        .unwrap();
    assert_eq!(stray.severity, Severity::Error);
}

#[test]
fn duplicate_ids_unknown_elements_and_images_without_alt_are_warned() {
    let text = "<p id=\"a\"></p><p id=\"a\"></p><blink>x</blink><img src=\"x.png\">\
                <svg><path d=\"M0 0\"/></svg><my-widget></my-widget>";
    let errors = analysis::syntax_errors(text);
    let messages: Vec<&str> = errors.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(
        errors
            .iter()
            .filter(|d| d.message.contains("« a »"))
            .count(),
        1
    );
    assert!(messages.iter().any(|m| m.contains("blink")));
    assert!(messages.iter().any(|m| m.contains("alt")));
    // SVG and custom elements are not unknown.
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("path") || m.contains("my-widget"))
    );
}

#[test]
fn an_unclosed_comment_or_quote_is_an_error() {
    let errors = analysis::syntax_errors("<p class=\"a>text</p>");
    assert!(errors.iter().any(|d| d.severity == Severity::Error));
    let errors = analysis::syntax_errors("<p>a</p><!-- open");
    assert!(errors.iter().any(|d| d.severity == Severity::Error));
}

// ---- Code of other languages -------------------------------------------------------------

#[test]
fn scripts_styles_and_their_attributes_go_to_their_languages() {
    let regions = embedded::regions(PAGE, "html");
    let of = |language: &str| -> Vec<&str> {
        regions
            .iter()
            .filter(|r| r.language == language)
            .map(|r| PAGE[r.range.clone()].trim())
            .collect()
    };
    assert_eq!(of("css"), [".box { color: red; }", "margin: 0"]);
    assert_eq!(of("js"), ["go()", "function go() { return 1; }"]);
}

#[test]
fn the_type_and_lang_of_a_block_choose_its_language() {
    let text = "<script type=\"application/json\">{}</script>\
                <script lang=\"ts\">let a: number = 1;</script>\
                <script type=\"module\">import x from 'y';</script>\
                <style lang=\"scss\">.a { .b { c: d } }</style>";
    let languages: Vec<&str> = embedded::regions(text, "html")
        .iter()
        .map(|r| r.language)
        .collect();
    assert_eq!(languages, ["ts", "js", "scss"]);
}

#[test]
fn the_document_of_a_language_keeps_the_places_of_the_page() {
    let regions = embedded::regions(PAGE, "html");
    let js = embedded::document(PAGE, &regions, "js");
    assert_eq!(js.len(), PAGE.len());
    assert_eq!(js.lines().count(), PAGE.lines().count());
    let function = at(PAGE, "function go()");
    assert_eq!(&js[function..function + 13], "function go()");
    // The handler is a function of its own; the markup is spaces.
    let handler = at(PAGE, "go()\"");
    assert_eq!(&js[handler - 6..handler + 5], ";()=>{go()}");
    assert!(!js.contains("<div"));
    let css = embedded::document(PAGE, &regions, "css");
    let declaration = at(PAGE, "margin: 0");
    assert_eq!(&css[declaration - 2..declaration + 10], "*{margin: 0}");
}

#[test]
fn the_language_at_the_caret_is_the_one_of_its_code() {
    assert_eq!(
        embedded::language_at(PAGE, "html", at(PAGE, "return")),
        "js"
    );
    assert_eq!(
        embedded::language_at(PAGE, "html", at(PAGE, "color:")),
        "css"
    );
    assert_eq!(
        embedded::language_at(PAGE, "html", at(PAGE, "<label")),
        "html"
    );
    assert_eq!(embedded::language_at("x", "rs", 0), "rs");
}

#[test]
fn a_block_is_formatted_alone_and_put_back_at_its_indentation() {
    let page = "<style>\n    .a {\n      color: red;\n    }\n  </style>";
    let start = at(page, "\n    .a");
    let end = at(page, "</style>");
    let block = embedded::Block::of(page, start..end).unwrap();
    assert_eq!(block.code, ".a {\n  color: red;\n}");
    assert_eq!(
        block.indent(".a {\n    color: red;\n}\n"),
        "    .a {\n        color: red;\n    }"
    );
    assert_eq!(
        &page[block.body.clone()],
        "    .a {\n      color: red;\n    }"
    );
    // Code on the line of its tag has no indentation to give back.
    assert!(embedded::Block::of("<script>let a;</script>", 8..14).is_none());
}

// ---- Completion, documentation, navigation ----------------------------------------------

#[test]
fn a_tag_being_opened_completes_elements_with_their_closing_tag() {
    let text = "<body>\n<di";
    let completion = analysis::complete(text, text.len(), false).unwrap();
    assert_eq!(completion.from, text.len() - 2);
    let div = completion.items.iter().find(|i| i.label == "div").unwrap();
    assert_eq!(div.insert, "div></div>");
    assert_eq!(div.caret, Some(4));
    assert!(div.doc.is_some());
    let br = completion.items.iter().find(|i| i.label == "br").unwrap();
    assert_eq!(br.insert, "br>");
}

#[test]
fn a_closing_tag_completes_the_innermost_open_element() {
    let text = "<div><section><p>text</";
    let completion = analysis::complete(text, text.len(), false).unwrap();
    assert_eq!(completion.items[0].label, "p");
    assert_eq!(completion.items[0].insert, "p>");
    assert!(completion.items.iter().any(|i| i.label == "div"));
}

#[test]
fn attributes_and_their_values_are_completed() {
    let text = "<input ty>";
    let offset = at(text, "ty") + 2;
    let completion = analysis::complete(text, offset, false).unwrap();
    let kind = completion.items.iter().find(|i| i.label == "type").unwrap();
    assert_eq!(kind.insert, "type=\"\"");
    assert_eq!(kind.caret, Some(6));
    // Own attributes before the global ones.
    let required = completion
        .items
        .iter()
        .find(|i| i.label == "required")
        .unwrap();
    assert_eq!(required.insert, "required");

    let text = "<input type=\"em\">";
    let completion = analysis::complete(text, at(text, "em") + 2, false).unwrap();
    assert!(completion.items.iter().any(|i| i.label == "email"));
    assert_eq!(completion.from, at(text, "em"));

    // The ids of the page for a reference, the classes for a class.
    let completion = analysis::complete(PAGE, at(PAGE, "for=\"") + 5, true).unwrap();
    assert!(completion.items.iter().any(|i| i.label == "main"));
}

#[test]
fn entities_are_completed() {
    let text = "<p>Tom &am";
    let completion = analysis::complete(text, text.len(), false).unwrap();
    let amp = completion.items.iter().find(|i| i.label == "amp").unwrap();
    assert_eq!(amp.insert, "amp;");
    assert_eq!(completion.from, text.len() - 2);
}

#[test]
fn nothing_is_completed_in_a_comment_or_a_script() {
    let text = "<!-- <di -->";
    assert!(analysis::complete(text, at(text, "di") + 2, true).is_none());
    assert!(analysis::complete(PAGE, at(PAGE, "return") + 3, true).is_none());
}

#[test]
fn elements_attributes_and_entities_are_documented() {
    let hover = analysis::hover(PAGE, at(PAGE, "label for") + 1).unwrap();
    assert_eq!(hover.title, "<label>");
    assert!(!hover.body.is_empty());
    let hover = analysis::hover(PAGE, at(PAGE, "type=") + 1).unwrap();
    assert!(hover.body.contains("<input>"));
    assert!(hover.body.contains("email"));
    let hover = analysis::hover(PAGE, at(PAGE, "&amp;") + 1).unwrap();
    assert!(hover.body.starts_with('&'));
}

#[test]
fn every_element_has_a_description() {
    for element in data::elements() {
        assert!(
            analysis::description(element).is_some(),
            "no description for <{element}>"
        );
    }
}

#[test]
fn a_tag_name_highlights_its_pair_and_an_id_its_references() {
    let ranges = analysis::highlights(PAGE, at(PAGE, "div id") + 1);
    let names: Vec<&str> = ranges.iter().map(|r| &PAGE[r.clone()]).collect();
    assert_eq!(names, ["div", "div"]);
    let ranges = analysis::highlights(PAGE, at(PAGE, "\"name\"") + 1);
    assert_eq!(ranges.len(), 2);
    // A class: its uses and its selector in the style sheet.
    let ranges = analysis::highlights(PAGE, at(PAGE, "box wide") + 1);
    assert_eq!(ranges.len(), 2);
    assert!(ranges.iter().any(|r| r.start == at(PAGE, ".box") + 1));
}

#[test]
fn a_reference_goes_to_its_id_and_a_class_to_its_selector() {
    let found = analysis::definition(PAGE, at(PAGE, "for=\"name") + 6);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].range_in(PAGE).start, at(PAGE, "name\" type"));
    let found = analysis::definition(PAGE, at(PAGE, "box wide") + 1);
    assert_eq!(found[0].range_in(PAGE).start, at(PAGE, ".box") + 1);
    let found = analysis::definition(PAGE, at(PAGE, "</label>") + 3);
    assert_eq!(found[0].range_in(PAGE).start, at(PAGE, "label for"));
}

#[test]
fn the_structure_is_the_tree_of_elements() {
    let symbols = analysis::symbols(PAGE);
    let main = symbols
        .iter()
        .find(|s| s.name == "div#main.box.wide")
        .unwrap();
    assert_eq!(main.depth, 2);
}

// ---- Formatting --------------------------------------------------------------------------

#[test]
fn reformat_indents_by_depth_and_keeps_pre_and_scripts() {
    let text = "<div>\n<p>\ntext\n</p>\n<pre>\n  keep   \n</pre>\n<script>\n        if (a) {\n          b();\n        }\n</script>\n</div>";
    let formatted = format::reformat(text);
    assert_eq!(
        formatted,
        "<div>\n    <p>\n        text\n    </p>\n    <pre>\n  keep   \n</pre>\n    <script>\n        if (a) {\n          b();\n        }\n    </script>\n</div>"
    );
    // Formatting twice changes nothing more.
    assert_eq!(format::reformat(&formatted), formatted);
}

#[test]
fn reformat_takes_the_indentation_of_the_file() {
    let text = "<ul>\n  <li>a</li>\n<li>b</li>\n</ul>";
    assert_eq!(
        format::reformat(text),
        "<ul>\n  <li>a</li>\n  <li>b</li>\n</ul>"
    );
}

// ---- Pug ---------------------------------------------------------------------------------

const PUG: &str = "doctype html
html(lang='fr')
  head
    style.
      .card { color: red; }
  body
    //- a comment
    mixin card(title)
      .card#first(data-x=1, class=active ? 'on' : '')= title
    +card('Hello')
    ul
      each item, i in items
        li: a(href='/' + item) #{item}
    | piped #{name} text
    - const total = items.length
    script.
      function hello() { return total; }
";

#[test]
fn pug_tags_classes_ids_and_attributes_are_read() {
    let scan = pug::scan(PUG);
    let card = scan.tags.iter().find(|t| t.id.is_some()).unwrap();
    assert_eq!(card.name, "div");
    assert_eq!(&PUG[card.id.clone().unwrap()], "first");
    assert_eq!(&PUG[card.classes[0].clone()], "card");
    let names: Vec<&str> = card.attrs.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["data-x", "class"]);
    let value = card.attrs[1].value.clone().unwrap();
    assert_eq!(&PUG[value], "active ? 'on' : ''");
    // `li: a` puts the link on the line of the item.
    assert!(scan.tags.iter().any(|t| t.name == "a" && t.inline));
    assert!(scan.problems.is_empty(), "{:?}", scan.problems);
}

#[test]
fn pug_script_and_style_blocks_go_to_their_languages() {
    let scan = pug::scan(PUG);
    let analyzed: Vec<(&str, &str)> = scan
        .regions
        .iter()
        .filter(|r| r.analyzed)
        .map(|r| (r.language, PUG[r.range.clone()].trim()))
        .collect();
    assert_eq!(
        analyzed,
        [
            ("css", ".card { color: red; }"),
            ("js", "function hello() { return total; }")
        ]
    );
    // The code of the lines is only colored as JavaScript.
    let colored: Vec<&str> = scan
        .regions
        .iter()
        .filter(|r| !r.analyzed)
        .map(|r| PUG[r.range.clone()].trim())
        .collect();
    for code in [
        "items",
        "item",
        "name",
        "const total = items.length",
        "title",
        "'Hello'",
    ] {
        assert!(colored.contains(&code), "{code:?} in {colored:?}");
    }
    assert_eq!(embedded::language_at(PUG, "pug", at(PUG, "hello")), "js");
}

#[test]
fn pug_comments_keywords_and_mixins_have_their_roles() {
    let scan = pug::scan(PUG);
    let role = |needle: &str| {
        let start = at(PUG, needle);
        scan.tokens
            .iter()
            .find(|(r, _)| r.start <= start && start < r.end)
            .map(|(_, role)| *role)
    };
    assert_eq!(role("//- a comment"), Some(pug::Role::Comment));
    assert_eq!(role("doctype"), Some(pug::Role::Keyword));
    assert_eq!(role("each"), Some(pug::Role::Keyword));
    assert_eq!(role("+card"), Some(pug::Role::Name));
    assert_eq!(role("ul"), Some(pug::Role::Tag));
    assert_eq!(role("data-x"), Some(pug::Role::AttrName));
}

#[test]
fn a_pug_mixin_call_goes_to_its_declaration() {
    let found = analysis::pug_definition(PUG, at(PUG, "+card") + 2);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].range_in(PUG).start, at(PUG, "card(title)"));
    let ranges = analysis::pug_highlights(PUG, at(PUG, "card(title)"));
    assert_eq!(ranges.len(), 2);
    assert!(analysis::pug_symbols(PUG).iter().any(|s| s.name == "card"));
}

#[test]
fn pug_completes_tags_keywords_and_attributes() {
    let text = "div\n  inp";
    let completion = analysis::pug_complete(text, text.len(), false).unwrap();
    assert!(completion.items.iter().any(|i| i.label == "input"));
    let text = "input(ty)";
    let completion = analysis::pug_complete(text, at(text, "ty") + 2, false).unwrap();
    let kind = completion.items.iter().find(|i| i.label == "type").unwrap();
    assert_eq!(kind.insert, "type=''");
}

#[test]
fn pug_mixed_indentation_and_unclosed_attributes_are_reported() {
    let errors = analysis::pug_errors("div\n  p\n\tspan\na(href='x'");
    assert!(errors.iter().any(|d| d.severity == Severity::Warning));
    assert!(errors.iter().any(|d| d.severity == Severity::Error));
}

#[test]
fn pug_reformat_only_removes_trailing_spaces() {
    assert_eq!(
        format::reformat_pug("div   \n  p text  \n"),
        "div\n  p text\n"
    );
}

#[test]
fn reformatted_html_with_edits_gives_the_formatted_text() {
    let text = "<div>\n<p>a</p>\n</div>";
    let formatted = format::reformat(text);
    let edits = crate::languages::format::edits_between(text, &formatted);
    assert_eq!(apply(text, &edits), formatted);
}

// ---- Emmet -------------------------------------------------------------------------------

/// The text after Tab at the end of `text`, with `|` where the caret goes.
fn tab(text: &str) -> Option<String> {
    let (range, html, caret) = emmet::expand_at(text, text.len(), "    ")?;
    let mut out = text.to_string();
    out.replace_range(range.clone(), &html);
    out.insert(range.start + caret, '|');
    Some(out)
}

#[test]
fn an_abbreviation_with_an_id_gives_the_element_with_the_caret_inside() {
    assert_eq!(tab("p#test").unwrap(), "<p id=\"test\">|</p>");
    assert_eq!(tab("div.a.b").unwrap(), "<div class=\"a b\">|</div>");
    assert_eq!(tab(".box").unwrap(), "<div class=\"box\">|</div>");
    assert_eq!(tab("a").unwrap(), "<a href=\"|\"></a>");
    assert_eq!(tab("br").unwrap(), "<br>|");
    assert_eq!(tab("p{Hello}").unwrap(), "<p>Hello</p>|");
    assert_eq!(
        tab("input[type=email name=mail required]").unwrap(),
        "<input type=\"email\" name=\"mail\" required>|"
    );
}

#[test]
fn children_siblings_repeats_and_numbers_are_laid_out() {
    assert_eq!(
        tab("ul>li.item$*3").unwrap(),
        "<ul>\n    <li class=\"item1\">|</li>\n    <li class=\"item2\"></li>\n    <li class=\"item3\"></li>\n</ul>"
    );
    assert_eq!(
        tab("div>h1+p^footer").unwrap(),
        "<div>\n    <h1>|</h1>\n    <p></p>\n</div>\n<footer></footer>"
    );
    assert_eq!(
        tab("table>tr*2>.cell").unwrap(),
        "<table>\n    <tr>\n        <td class=\"cell\">|</td>\n    </tr>\n    <tr>\n        <td class=\"cell\"></td>\n    </tr>\n</table>"
    );
    assert_eq!(
        tab("(header>nav)+main").unwrap(),
        "<header>\n    <nav>|</nav>\n</header>\n<main></main>"
    );
    assert_eq!(tab("p>span").unwrap(), "<p><span>|</span></p>");
}

#[test]
fn an_abbreviation_takes_the_indentation_of_its_line() {
    assert_eq!(
        tab("<body>\n    ul>li").unwrap(),
        "<body>\n    <ul>\n        <li>|</li>\n    </ul>"
    );
    // After a tag on the same line.
    assert_eq!(
        tab("<div>span.x").unwrap(),
        "<div><span class=\"x\">|</span>"
    );
}

#[test]
fn what_is_not_an_abbreviation_is_left_alone() {
    // A word of text, a tag being written, a comment, a script.
    assert!(tab("Hello world").is_none());
    assert!(tab("<p class=\"a").is_none());
    assert!(tab("<!-- p#x").is_none());
    assert!(tab("<script>a.b").is_none());
    assert!(tab("x > 1").is_none());
}

#[test]
fn an_exclamation_mark_gives_a_page() {
    let page = tab("!").unwrap();
    assert!(page.starts_with("<!doctype html>"));
    assert!(page.contains("<meta charset=\"UTF-8\">"));
}
