use super::*;

fn project(files: &[(&str, &str)]) -> Scratch {
    let dir = scratch();
    for (name, body) in files {
        let p = dir.path().join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }
    dir
}

const PAIR: &str =
    "function helper(x){ return x + 1 }\nexport function caller(x){ return helper(x) * 2 }\n";

#[test]
fn the_six_tools_are_listed_with_their_own_options() {
    let specs = specs();
    let names: Vec<&str> = specs.iter().filter_map(|s| s["name"].as_str()).collect();
    assert_eq!(
        names,
        [
            "crapper", "dryer", "verifier", "mutator", "checker", "cleaner"
        ]
    );
    let properties = |name: &str| {
        let spec = specs.iter().find(|s| s["name"] == name).unwrap();
        let mut keys: Vec<String> = spec["inputSchema"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        keys.sort();
        keys
    };
    assert_eq!(
        properties("dryer"),
        [
            "changed",
            "dryThreshold",
            "jobs",
            "minLines",
            "minNodes",
            "paths"
        ]
    );
    assert!(properties("checker").contains(&"mergeThreshold".to_string()));
    assert!(!properties("checker").contains(&"testCommand".to_string()));
    assert!(properties("cleaner").contains(&"testCommand".to_string()));
}

#[test]
fn arguments_become_the_options_of_each_gate() {
    let args = json!({"paths": ["src/a.js"], "noCoverage": true, "changed": false, "crapThreshold": 12, "dryThreshold": 0.9, "testCommand": "npm test"});
    assert_eq!(
        argv(&args, "crapper").unwrap(),
        ["src/a.js", "--no-coverage", "--threshold=12"]
    );
    assert_eq!(
        argv(&args, "dryer").unwrap(),
        ["src/a.js", "--threshold=0.9"]
    );
    assert_eq!(
        argv(&args, "mutator").unwrap(),
        ["src/a.js", "--no-coverage", "--test-command=npm test"]
    );
    assert_eq!(
        argv(&json!({"paths": "src lib"}), "dryer").unwrap(),
        ["src", "lib"]
    );
    assert!(argv(&json!({"nope": true}), "dryer").is_err());
    assert!(argv(&json!({"paths": 3}), "dryer").is_err());
    assert!(argv(&json!({"jobs": [1]}), "dryer").is_err());
}

#[test]
fn a_tool_returns_its_report_and_its_verdict() {
    let _serial = run::serial();
    let dir = project(&[("src/a.js", PAIR)]);
    let text = tool("verifier", &json!({"noCoverage": true}), dir.path())
        .unwrap()
        .unwrap();
    assert!(
        text.contains("merge helper (CRAP 1.0) into caller"),
        "{text}"
    );
    assert!(text.ends_with("Result: fail: merges proposed"), "{text}");
    assert!(tool("git_diff", &json!({}), dir.path()).is_none());
}

#[test]
fn checker_runs_the_three_gates_and_sums_them_up() {
    let _serial = run::serial();
    let dir = project(&[("src/a.js", PAIR)]);
    let text = tool("checker", &json!({"noCoverage": true}), dir.path())
        .unwrap()
        .unwrap();
    for section in [
        "## crapper: pass",
        "## dryer: pass",
        "## verifier: fail: merges proposed",
        "## Summary",
    ] {
        assert!(text.contains(section), "{section} in {text}");
    }
    assert!(current().is_none(), "the run ends with the call");
}

#[test]
fn bad_arguments_run_nothing() {
    let _serial = run::serial();
    let dir = project(&[("src/a.js", PAIR)]);
    assert!(
        tool("checker", &json!({"nope": 1}), dir.path())
            .unwrap()
            .is_err()
    );
    assert!(!dir.path().join(".metrics").exists());
}

#[test]
fn one_run_at_a_time() {
    let _serial = run::serial();
    let dir = project(&[("src/a.js", PAIR)]);
    let _other = run::start("cleaner").unwrap();
    let refused = tool("dryer", &json!({}), dir.path()).unwrap();
    assert!(refused.unwrap_err().contains("cleaner"));
}

#[test]
fn the_coverage_is_measured_once_for_all_the_gates() {
    let _serial = run::serial();
    // An lcov report on disk and no package.json: only the first gate may try to measure.
    let dir = project(&[
        ("src/a.js", PAIR),
        (
            "coverage/lcov.info",
            "SF:src/a.js\nDA:1,1\nDA:2,1\nend_of_record\n",
        ),
    ]);
    // Measured again, the coverage would be empty: 2 + 2 is not under 3. Read from the report
    // the first gate left, it is full: 1 + 1 is.
    let args = json!({"mergeThreshold": 3});
    let text = tool("checker", &args, dir.path()).unwrap().unwrap();
    assert!(text.contains("## crapper: pass"), "{text}");
    assert!(
        text.contains("## verifier: fail: merges proposed"),
        "the verifier read the report on disk: {text}"
    );
}

#[test]
fn every_command_runs_its_tool_and_carries_its_rules() {
    let names: Vec<String> = prompts()
        .iter()
        .map(|p| p["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        names,
        [
            "crapper", "dryer", "verifier", "mutator", "checker", "cleaner", "ub-mode"
        ]
    );
    let text = prompt("checker", &json!({"paths": "src lib"})).unwrap();
    assert!(text.contains("Call the Atelier tool `checker` with `paths` [\"src\",\"lib\"]"));
    assert!(text.contains("under CRAP 10") && text.contains("0.8 or above"));
    assert!(text.find("### crapper").unwrap() < text.find("### verifier").unwrap());
    assert!(!text.contains("Kill the survivors"));
    assert!(
        prompt("cleaner", &json!({}))
            .unwrap()
            .contains("Kill the survivors")
    );
    let ub = prompt("ub-mode", &json!({})).unwrap();
    assert!(ub.contains("# First Make it Work") && ub.contains("`checker`"));
    assert!(prompt("nope", &json!({})).is_none());
}
