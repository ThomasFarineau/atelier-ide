use super::*;

#[test]
fn a_project_name_is_one_a_folder_can_have() {
    assert!(valid_name("mon-projet"));
    assert!(valid_name("  Projet 2  "));
    for name in ["", "  ", ".", "..", "a/b", r"a\b", "c:", "x?", "fin."] {
        assert!(!valid_name(name), "{name:?}");
    }
}

#[test]
fn a_new_project_takes_an_empty_or_new_folder() {
    let base = std::env::temp_dir().join(format!("atelier-new-project-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(base.join("empty")).unwrap();
    fs::create_dir_all(base.join("full")).unwrap();
    fs::write(base.join("full").join("a.txt"), "x").unwrap();
    assert_eq!(create_project(&base, "new").unwrap(), base.join("new"));
    assert!(base.join("new").is_dir());
    assert!(create_project(&base, "empty").is_ok());
    assert!(create_project(&base, "full").is_err());
    assert!(create_project(&base, "a/b").is_err());
    let _ = fs::remove_dir_all(&base);
}
