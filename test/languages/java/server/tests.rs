use super::*;

#[test]
fn java_versions_are_read_old_and_new() {
    assert_eq!(
        major_version("openjdk version \"21.0.2\" 2024-01-16"),
        Some(21)
    );
    assert_eq!(major_version("java version \"1.8.0_202\""), Some(8));
    assert_eq!(major_version("openjdk version \"25-ea\""), Some(25));
    assert_eq!(major_version("nothing"), None);
}

#[test]
fn jdtls_starts_from_its_launcher_with_a_folder_per_project() {
    let dir = Path::new("/opt/jdtls");
    let jar = dir.join("plugins/org.eclipse.equinox.launcher_1.6.900.jar");
    let args = arguments(dir, &jar, Path::new("/work/shop"));
    let at = |flag: &str| args.iter().position(|a| a == flag).unwrap();
    assert_eq!(args[at("-jar") + 1], jar.to_string_lossy());
    assert!(args[at("-configuration") + 1].contains("config_"));
    assert!(args[at("-data") + 1].contains("shop-"));
    let other = arguments(dir, &jar, Path::new("/work/other"));
    assert_ne!(args.last(), other.last(), "each project has its own data");
}
