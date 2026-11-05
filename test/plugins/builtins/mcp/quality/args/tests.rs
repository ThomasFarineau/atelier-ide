use super::*;

fn v(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn parses_flags_values_and_positionals() {
    let a = Args::parse(
        &v(&["x", "--changed", "--threshold=30", "--n", "2"]),
        &["changed"],
        &["threshold", "n"],
    )
    .unwrap();
    assert!(a.has("changed"));
    assert_eq!(a.number("threshold", 0).unwrap(), 30);
    assert_eq!(a.number("n", 0).unwrap(), 2);
    assert_eq!(a.positional, vec!["x"]);
}

#[test]
fn exit_reports_errors_as_one() {
    let out = Out::default();
    assert_eq!(exit("t", Ok(3), &out), 3);
    assert_eq!(exit("t", Err("x".into()), &out), 1);
    assert_eq!(out.take(), "t: x\n");
}

#[test]
fn rejects_unknown_options_and_missing_values() {
    assert!(Args::parse(&v(&["--nope"]), &[], &[]).is_err());
    assert!(Args::parse(&v(&["--n"]), &[], &["n"]).is_err());
}
