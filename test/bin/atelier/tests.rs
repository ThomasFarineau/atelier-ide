use super::*;
use std::io::Write;

fn scratch(name: &str) -> PathBuf {
    let dir = env::temp_dir().join(format!("atelier-bootstrap-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn tar_gz(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (name, data) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, name, *data).unwrap();
    }
    let tar = builder.into_inner().unwrap();
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(&tar).unwrap();
    gz.finish().unwrap()
}

fn zipped(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in files {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn versions_compare_by_number_not_by_text() {
    assert!(newer("v0.10.0", "v0.9.3"));
    assert!(newer("1.0.0", "0.99.99"));
    assert!(newer("v1.2", "v1.1.9"));
    assert!(!newer("v1.2.0", "1.2"), "the same release");
    assert!(!newer("v0.0.1", "v0.0.2"));
    assert!(newer("v2.0.0-beta", "v1.9.0"), "a suffix is ignored");
}

#[test]
fn archives_and_checksums_are_found_by_name() {
    assert_eq!(
        archive_name("windows", "x86_64"),
        "atelier-ide-windows-x86_64.zip"
    );
    assert_eq!(
        archive_name("macos", "aarch64"),
        "atelier-ide-macos-aarch64.tar.gz"
    );
    let sums = "AABB  atelier-ide-linux-x86_64.tar.gz\nccdd *atelier-ide-windows-x86_64.zip\n";
    assert_eq!(
        checksum_of(sums, "atelier-ide-linux-x86_64.tar.gz").as_deref(),
        Some("aabb")
    );
    assert_eq!(
        checksum_of(sums, "atelier-ide-windows-x86_64.zip").as_deref(),
        Some("ccdd")
    );
    assert_eq!(checksum_of(sums, "other.zip"), None);
    assert_eq!(
        hex(&Sha256::digest(b"abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn an_update_replaces_the_app_whole_and_a_broken_one_changes_nothing() {
    for (name, archive) in [
        (
            "atelier-ide-linux-x86_64.tar.gz",
            tar_gz(&[(EXE, b"new ide"), ("extra.txt", b"x")]),
        ),
        (
            "atelier-ide-windows-x86_64.zip",
            zipped(&[(EXE, b"new ide"), ("extra.txt", b"x")]),
        ),
    ] {
        let dir = scratch("install");
        fs::create_dir_all(dir.join(APP_DIR)).unwrap();
        fs::write(dir.join(APP_DIR).join(EXE), "old ide").unwrap();
        fs::write(dir.join(APP_DIR).join("stale.txt"), "gone").unwrap();
        fs::write(dir.join(APP_DIR).join("VERSION"), "v0.0.1").unwrap();
        install(&dir, name, &archive, "v0.0.2").unwrap();
        assert_eq!(
            fs::read_to_string(dir.join(APP_DIR).join(EXE)).unwrap(),
            "new ide"
        );
        assert_eq!(
            fs::read_to_string(dir.join(APP_DIR).join("VERSION")).unwrap(),
            "v0.0.2"
        );
        assert!(
            !dir.join(APP_DIR).join("stale.txt").exists(),
            "the old files go with the old version"
        );
        assert!(!dir.join("app.new").exists() && !dir.join("app.old").exists());
        // An archive without the IDE is refused and the installation stays.
        let broken = if name.ends_with(".zip") {
            zipped(&[("readme.txt", b"?")])
        } else {
            tar_gz(&[("readme.txt", b"?")])
        };
        assert!(install(&dir, name, &broken, "v0.0.3").is_err());
        assert_eq!(
            fs::read_to_string(dir.join(APP_DIR).join("VERSION")).unwrap(),
            "v0.0.2"
        );
        assert_eq!(
            fs::read_to_string(dir.join(APP_DIR).join(EXE)).unwrap(),
            "new ide"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}

#[test]
fn a_first_install_needs_no_previous_app() {
    let dir = scratch("first");
    install(
        &dir,
        "atelier-ide-linux-x86_64.tar.gz",
        &tar_gz(&[(EXE, b"ide")]),
        "v1.0.0",
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(dir.join(APP_DIR).join(EXE)).unwrap(),
        "ide"
    );
    let _ = fs::remove_dir_all(&dir);
}
