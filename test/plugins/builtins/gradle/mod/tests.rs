use super::*;

#[test]
fn plugins_are_read_from_both_dsls() {
    let kotlin = "plugins {\n    java\n    application\n    id(\"org.springframework.boot\") version \"3.3.0\"\n    kotlin(\"jvm\") version \"2.0.0\"\n}\n";
    assert_eq!(
        plugins_of(kotlin),
        [
            "org.springframework.boot",
            "org.jetbrains.kotlin.jvm",
            "java",
            "application"
        ]
    );
    let groovy = "plugins {\n    id 'java-library'\n}\napply plugin: 'war'\n";
    assert_eq!(plugins_of(groovy), ["java-library", "war"]);
}

#[test]
fn default_tasks_follow_the_plugins() {
    let tasks = default_tasks(&["application".into()], true);
    assert!(tasks["application"].iter().any(|t| t.name == "run"));
    assert!(tasks["build"].iter().any(|t| t.name == "build"));
    assert!(tasks.contains_key("build setup"));
    let bare = default_tasks(&[], false);
    assert_eq!(bare.keys().collect::<Vec<_>>(), ["help"]);
}

#[test]
fn projects_come_from_the_settings() {
    let root = std::env::temp_dir().join(format!("atelier-gradle-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("app")).unwrap();
    std::fs::create_dir_all(root.join("libs/core")).unwrap();
    std::fs::write(
        root.join("settings.gradle.kts"),
        "rootProject.name = \"shop\"\ninclude(\"app\")\ninclude(\":libs:core\", \"docs\")\n",
    )
    .unwrap();
    std::fs::write(
        root.join("app/build.gradle.kts"),
        "plugins { application }\n",
    )
    .unwrap();
    let projects = scan(&root);
    let paths: Vec<&str> = projects.iter().map(|p| p.path.as_str()).collect();
    assert_eq!(paths, [":", ":app", ":libs:core", ":docs"]);
    assert_eq!(projects[0].name, "shop");
    assert_eq!(projects[2].name, "core");
    assert_eq!(projects[2].dir, root.join("libs/core"));
    assert!(projects[1].tasks.contains_key("application"));
    assert_eq!(program(&root), "gradle");
    let wrapper = if cfg!(windows) {
        "gradlew.bat"
    } else {
        "gradlew"
    };
    std::fs::write(root.join(wrapper), "").unwrap();
    assert!(program(&root).ends_with(wrapper));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_task_report_of_gradle_is_read() {
    let report = "\
------------------------------------------------------------
Tasks runnable from root project 'shop'
------------------------------------------------------------

Application tasks
-----------------
run - Runs this project as a JVM application
app:run - Runs this project as a JVM application

Build tasks
-----------
assemble - Assembles the outputs of this project.
app:jar - Assembles a jar archive.

Other tasks
-----------
app:compileJava - Compiles main Java source.
custom

Rules
-----
Pattern: clean<TaskName>: Cleans the output files of a task.
";
    let tasks = parse_tasks(report);
    assert_eq!(tasks[":"]["application"][0].name, "run");
    assert_eq!(tasks[":app"]["build"][0].name, "jar");
    assert_eq!(
        tasks[":app"]["other"][0].description,
        "Compiles main Java source."
    );
    assert_eq!(tasks[":"]["other"][0].name, "custom");
    assert!(!tasks[":"].contains_key("rules"));
    assert_eq!(task_path(":app", "run"), ":app:run");
    assert_eq!(task_path(":", "build"), "build");
}

#[test]
fn the_plugin_applies_only_to_a_gradle_build() {
    let empty = std::env::temp_dir().join(format!("atelier-gradle-none-{}", std::process::id()));
    std::fs::create_dir_all(&empty).unwrap();
    let mut plugin = GradlePlugin {
        projects: scan(&empty),
        ..Default::default()
    };
    assert!(!plugin.applies(), "no settings.gradle nor build.gradle");
    std::fs::write(empty.join("build.gradle"), "plugins { id 'java' }\n").unwrap();
    plugin.projects = scan(&empty);
    assert!(plugin.applies());
    let _ = std::fs::remove_dir_all(&empty);
}
