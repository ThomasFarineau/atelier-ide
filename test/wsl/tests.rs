use super::*;

#[test]
fn wsl_shares_become_linux_paths_and_back() {
    let linux = |p: &str| parse(Path::new(p));
    let home = WslPath {
        distro: "Ubuntu".into(),
        linux: "/home/thomas/workspace".into(),
    };
    assert_eq!(
        linux(r"\\?\UNC\wsl.localhost\Ubuntu\home\thomas\workspace"),
        Some(home.clone())
    );
    assert_eq!(
        linux(r"\\wsl.localhost\Ubuntu\home\thomas\workspace\"),
        Some(home.clone())
    );
    assert_eq!(
        linux(r"\\wsl$\Debian\srv"),
        Some(WslPath {
            distro: "Debian".into(),
            linux: "/srv".into()
        })
    );
    assert_eq!(
        linux(r"\\wsl.localhost\Ubuntu"),
        Some(WslPath {
            distro: "Ubuntu".into(),
            linux: "/".into()
        })
    );
    assert_eq!(linux(r"\\server\share\x"), None);
    assert_eq!(linux(r"C:\Users\thomas"), None);
    assert_eq!(
        to_windows("Ubuntu", "/home/thomas/workspace"),
        PathBuf::from(r"\\wsl.localhost\Ubuntu\home\thomas\workspace")
    );
    assert_eq!(parse(&to_windows("Ubuntu", "/a/b")).unwrap().linux, "/a/b");
}

#[test]
fn posix_commands_quote_every_word() {
    assert_eq!(
        shell_command("python3", &["a b.py".into(), "it's".into(), "$(x)".into()]),
        r"'python3' 'a b.py' 'it'\''s' '$(x)'"
    );
}
