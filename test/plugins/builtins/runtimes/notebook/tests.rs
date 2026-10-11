use super::*;

#[test]
fn a_notebook_comes_back_as_jupyter_writes_it() {
    let text = "{\n \"cells\": [\n  {\n   \"cell_type\": \"code\",\n   \"execution_count\": null,\n   \"metadata\": {},\n   \"outputs\": [],\n   \"source\": [\n    \"print(1)\\n\",\n    \"x = 2\"\n   ]\n  }\n ],\n \"metadata\": {},\n \"nbformat\": 4,\n \"nbformat_minor\": 5\n}\n";
    let notebook = parse(text).unwrap();
    assert_eq!(source(&notebook["cells"][0]), "print(1)\nx = 2");
    assert_eq!(to_text(&notebook), text);
    assert!(parse("{\"a\": 1}").is_none(), "no cells");
}

#[test]
fn sources_are_stored_as_lines() {
    assert_eq!(lines("a\nb\n"), json!(["a\n", "b\n"]));
    assert_eq!(lines("a\nb"), json!(["a\n", "b"]));
    assert_eq!(lines(""), json!([]));
}

#[test]
fn streams_go_on_and_results_are_added() {
    let mut cell = new_cell("code", false);
    apply_output(
        &mut cell,
        "stream",
        &json!({"name": "stdout", "text": "a\n"}),
    );
    apply_output(
        &mut cell,
        "stream",
        &json!({"name": "stdout", "text": "b\n"}),
    );
    apply_output(&mut cell, "stream", &json!({"name": "stderr", "text": "c"}));
    apply_output(
        &mut cell,
        "execute_result",
        &json!({"execution_count": 3, "data": {"text/plain": "4"}, "metadata": {}}),
    );
    let outputs = cell["outputs"].as_array().unwrap();
    assert_eq!(outputs.len(), 3);
    assert_eq!(text_of(&outputs[0]["text"]), "a\nb\n");
    assert_eq!(outputs[1]["name"], "stderr");
    assert_eq!(outputs[2]["execution_count"], 3);
    apply_output(&mut cell, "clear_output", &json!({"wait": false}));
    assert!(cell["outputs"].as_array().unwrap().is_empty());
}

#[test]
fn new_cells_have_an_id_from_format_4_5() {
    assert!(new_cell("code", true)["id"].is_string());
    assert!(new_cell("markdown", false).get("id").is_none());
    assert!(new_cell("markdown", false).get("outputs").is_none());
    assert!(has_ids(&json!({"nbformat": 4, "nbformat_minor": 5})));
    assert!(!has_ids(&json!({"nbformat": 4, "nbformat_minor": 2})));
}

#[test]
fn tracebacks_lose_their_colors() {
    assert_eq!(
        strip_ansi("\u{1b}[0;31mValueError\u{1b}[0m: bad"),
        "ValueError: bad"
    );
}

#[test]
fn images_are_decoded_from_base64() {
    assert_eq!(base64_decode("aGVs\nbG8=").unwrap(), b"hello");
    assert_eq!(base64_decode("aGk=").unwrap(), b"hi");
    assert!(base64_decode("a!b").is_none());
}

#[test]
fn the_language_comes_from_the_metadata() {
    assert_eq!(language(&json!({"metadata": {}})), "python");
    assert_eq!(
        language(&json!({"metadata": {"kernelspec": {"language": "R"}}})),
        "R"
    );
    assert_eq!(language(&empty()), "python");
}
