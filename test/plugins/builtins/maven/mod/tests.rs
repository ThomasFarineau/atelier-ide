use super::*;

const POM: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.shop</groupId>
    <artifactId>shop-parent</artifactId>
    <version>1.2.0</version>
  </parent>
  <artifactId>shop-api</artifactId>
  <name>Shop API</name>
  <dependencies>
    <dependency>
      <groupId>org.springframework.boot</groupId>
      <artifactId>spring-boot-starter-web</artifactId>
    </dependency>
    <dependency>
      <groupId>org.junit.jupiter</groupId>
      <artifactId>junit-jupiter</artifactId>
      <version>5.10.0</version>
      <scope>test</scope>
    </dependency>
  </dependencies>
  <build>
    <plugins>
      <plugin>
        <groupId>org.springframework.boot</groupId>
        <artifactId>spring-boot-maven-plugin</artifactId>
        <configuration><mainClass>shop.App</mainClass></configuration>
      </plugin>
    </plugins>
  </build>
  <profiles>
    <profile><id>dev</id></profile>
    <profile><id>prod</id></profile>
  </profiles>
</project>
"#;

#[test]
fn a_pom_gives_its_coordinates_plugins_profiles_and_dependencies() {
    let project = read_pom(POM).unwrap();
    assert_eq!(project.artifact, "shop-api");
    assert_eq!(project.group, "com.shop", "the group of the parent");
    assert_eq!(project.version, "1.2.0", "the version of the parent");
    assert_eq!(project.packaging, "jar");
    assert_eq!(project.profiles, ["dev", "prod"]);
    assert_eq!(project.plugins[0], "spring-boot-maven-plugin");
    assert!(
        project.plugins.iter().any(|p| p == "maven-compiler-plugin"),
        "the default ones"
    );
    assert_eq!(project.dependencies.len(), 2);
    assert_eq!(project.dependencies[0].scope, "compile");
    assert_eq!(project.dependencies[1].version, "5.10.0");
    assert_eq!(project.dependencies[1].scope, "test");
    assert!(read_pom("<settings/>").is_none());
}

#[test]
fn long_lists_are_folded_until_opened() {
    assert!(open_by_default("pom.xml"));
    assert!(open_by_default("api/pom.xml#lifecycle"));
    assert!(!open_by_default("pom.xml#plugins"));
    assert!(!open_by_default("pom.xml#dependencies"));
    assert!(!open_by_default("pom.xml#plugins#maven-jar-plugin"));
}

#[test]
fn goals_are_named_by_their_plugin_prefix() {
    assert_eq!(prefix("maven-compiler-plugin"), "compiler");
    assert_eq!(prefix("spring-boot-maven-plugin"), "spring-boot");
    assert_eq!(prefix("custom"), "custom");
    assert!(goals("spring-boot-maven-plugin").contains(&"run"));
}

#[test]
fn runs_name_the_pom_the_options_and_the_goals() {
    let mut project = read_pom(POM).unwrap();
    project.rel = "api/pom.xml".into();
    let options = Options {
        offline: true,
        skip_tests: true,
        profiles: ["prod".to_string(), "absent".to_string()].into(),
    };
    assert_eq!(
        arguments(&project, &options, &["clean".into(), "install".into()]),
        [
            "-f",
            "api/pom.xml",
            "-o",
            "-DskipTests",
            "-Pprod",
            "clean",
            "install"
        ]
    );
    assert_eq!(
        arguments(&project, &Options::default(), &["test".into()]),
        ["-f", "api/pom.xml", "test"]
    );
}

#[test]
fn the_wrapper_of_the_project_is_preferred_and_modules_are_found() {
    let root = std::env::temp_dir().join(format!("atelier-maven-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("api/target")).unwrap();
    std::fs::write(root.join("pom.xml"), "<project><groupId>g</groupId><artifactId>parent</artifactId><version>1</version><packaging>pom</packaging><modules><module>api</module></modules></project>").unwrap();
    std::fs::write(root.join("api/pom.xml"), POM).unwrap();
    std::fs::write(root.join("api/target/pom.xml"), POM).unwrap();
    let projects = scan(&root);
    let rels: Vec<&str> = projects.iter().map(|p| p.rel.as_str()).collect();
    assert_eq!(rels, ["pom.xml", "api/pom.xml"], "target is skipped");
    assert_eq!(projects[0].modules, ["api"]);
    assert_eq!(program(&root.join("api"), &root), "mvn");
    let wrapper = if cfg!(windows) { "mvnw.cmd" } else { "mvnw" };
    std::fs::write(root.join(wrapper), "").unwrap();
    assert!(program(&root.join("api"), &root).ends_with(wrapper));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_plugin_applies_only_to_a_maven_project() {
    let mut plugin = MavenPlugin::default();
    assert!(!plugin.applies(), "no pom.xml found yet");
    plugin.projects = vec![read_pom(POM).unwrap()];
    assert!(plugin.applies());
}
