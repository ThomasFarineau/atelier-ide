use super::*;

const REPORT: &str =
    "SF:src/a.ts\nDA:1,1\nDA:2,0\nDA:3,1\nBRDA:6,0,0,1\nBRDA:6,0,1,0\nend_of_record\n";

#[test]
fn lines_without_branches_use_line_hits() {
    let cov = Coverage::parse(REPORT, Path::new("."));
    let p = Path::new("./src/a.ts");
    assert!((cov.function_percent(p, 1, 3) - 200.0 / 3.0).abs() < 1e-9);
}

#[test]
fn branches_win_when_present() {
    let cov = Coverage::parse(REPORT, Path::new("."));
    assert_eq!(cov.function_percent(Path::new("./src/a.ts"), 5, 8), 50.0);
}

#[test]
fn unknown_file_scores_zero() {
    let cov = Coverage::parse(REPORT, Path::new("."));
    assert_eq!(cov.function_percent(Path::new("./src/zz.ts"), 1, 3), 0.0);
}

#[test]
fn lines_outside_a_file_record_are_ignored() {
    let cov = Coverage::parse("DA:1,1\nend_of_record\n", Path::new("."));
    assert!(cov.is_empty());
}
