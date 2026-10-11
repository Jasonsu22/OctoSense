use super::*;
use tempfile::TempDir;

struct Fixture {
    _temp: TempDir,
    ticket: Ticket,
    job: PathBuf,
    candidate: PathBuf,
}

fn bundle(path: &Path, version: &str, program: &str) {
    fs::create_dir_all(path.join("Contents/MacOS")).unwrap();
    fs::write(path.join("Contents/Info.plist"), format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>CFBundleIdentifier</key><string>{BUNDLE_ID}</string>
<key>CFBundleExecutable</key><string>octosense</string>
<key>CFBundleShortVersionString</key><string>{version}</string></dict></plist>"#)).unwrap();
    let exe = path.join("Contents/MacOS/octosense");
    fs::write(&exe, program).unwrap();
    fs::set_permissions(exe, fs::Permissions::from_mode(0o755)).unwrap();
}

fn fixture() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let parent = temp.path().canonicalize().unwrap();
    let nonce = uuid::Uuid::new_v4().to_string();
    let root = parent.join(format!(".octosense-update-{nonce}"));
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let target = parent.join("OctoSense Renamed.app");
    let candidate = root.join("candidate.app");
    bundle(&target, "0.1.0-rc.5", "#!/bin/sh\nexit 0\n");
    bundle(&candidate, "0.1.0-rc.6", "#!/bin/sh\nexit 7\n");
    let receipt_dir = parent.join("updates");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&receipt_dir)
        .unwrap();
    let ticket = Ticket {
        protocol: 1,
        nonce,
        parent_pid: std::process::id(),
        old_digest: digest_tree(&target).unwrap(),
        new_digest: digest_tree(&candidate).unwrap(),
        new_tag: "desktop-v0.1.0-rc.6".into(),
        target,
        environment: BTreeMap::from([(
            "OCTOSENSE_HOME".into(),
            parent.join("profile").display().to_string(),
        )]),
        startup_ack: true,
        receipt: receipt_dir.join("last-install.json"),
    };
    fs::create_dir(ticket.environment.get("OCTOSENSE_HOME").unwrap()).unwrap();
    fs::write(
        Path::new(&ticket.environment["OCTOSENSE_HOME"]).join("note.md"),
        "Keep this note 升级",
    )
    .unwrap();
    let job = root.join("job.json");
    write_json(&job, &ticket).unwrap();
    Fixture {
        _temp: temp,
        ticket,
        job,
        candidate,
    }
}

#[test]
fn renamed_installations_are_detected_but_loose_binaries_are_not() {
    let f = fixture();
    assert_eq!(
        bundle_for_executable(&f.ticket.target.join("Contents/MacOS/octosense")).unwrap(),
        f.ticket.target
    );
    for relative in ["octosense", "Contents/octosense", "Contents/MacOS/other"] {
        assert!(bundle_for_executable(&f.ticket.target.join(relative)).is_err());
    }
    validate_bundle(&f.candidate, Some("0.1.0-rc.6")).unwrap();
    assert!(validate_bundle(&f.candidate, Some("0.1.0-rc.7")).is_err());
}

#[test]
fn tampering_and_outside_symlinks_are_refused_before_exchange() {
    let f = fixture();
    fs::write(
        f.candidate.join("Contents/extra"),
        b"changed after verification",
    )
    .unwrap();
    assert!(replace_and_start(&f.ticket, &f.job, &f.candidate)
        .unwrap_err()
        .contains("changed"));
    assert_eq!(digest_tree(&f.ticket.target).unwrap(), f.ticket.old_digest);
    std::os::unix::fs::symlink(
        f.job.parent().unwrap().join("job.json"),
        f.candidate.join("Contents/escape"),
    )
    .unwrap();
    assert!(digest_tree(&f.candidate).unwrap_err().contains("outside"));
}

#[test]
fn failed_launch_rolls_back_without_touching_app_data() {
    let f = fixture();
    replace_and_start(&f.ticket, &f.job, &f.candidate).unwrap();
    assert_eq!(digest_tree(&f.ticket.target).unwrap(), f.ticket.old_digest);
    assert_eq!(digest_tree(&f.candidate).unwrap(), f.ticket.new_digest);
    let receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(&f.ticket.receipt).unwrap()).unwrap();
    assert_eq!(receipt["state"], "rolled_back");
    assert_eq!(
        fs::read_to_string(Path::new(&f.ticket.environment["OCTOSENSE_HOME"]).join("note.md"))
            .unwrap(),
        "Keep this note 升级"
    );
}

#[test]
fn interrupted_exchange_can_be_recovered_on_either_side_and_repeated() {
    let f = fixture();
    restore_previous(&f.ticket, &f.job, &f.candidate).unwrap(); // interruption before swap
    exchange(&f.ticket.target, &f.candidate).unwrap(); // interruption after swap
    assert!(f.ticket.target.is_dir() && f.candidate.is_dir());
    restore_previous(&f.ticket, &f.job, &f.candidate).unwrap();
    restore_previous(&f.ticket, &f.job, &f.candidate).unwrap();
    assert_eq!(digest_tree(&f.ticket.target).unwrap(), f.ticket.old_digest);
    fs::write(f.candidate.join("Contents/extra"), b"unexpected").unwrap();
    assert!(restore_previous(&f.ticket, &f.job, &f.candidate).is_err());
    assert_eq!(digest_tree(&f.ticket.target).unwrap(), f.ticket.old_digest);
}

#[test]
fn tickets_require_private_paths_and_only_known_relaunch_settings() {
    let mut f = fixture();
    assert_eq!(read_ticket(&f.job).unwrap().target, f.ticket.target);
    f.ticket
        .environment
        .insert("DYLD_INSERT_LIBRARIES".into(), "foreign".into());
    write_json(&f.job, &f.ticket).unwrap();
    assert!(read_ticket(&f.job).unwrap_err().contains("setting"));
    f.ticket.environment.remove("DYLD_INSERT_LIBRARIES");
    f.ticket.target = f
        .ticket
        .target
        .parent()
        .unwrap()
        .join("outside/OctoSense.app");
    write_json(&f.job, &f.ticket).unwrap();
    assert!(read_ticket(&f.job).is_err());
    fs::set_permissions(&f.job, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(read_ticket(&f.job).unwrap_err().contains("file"));
}

#[test]
fn relaunch_uses_the_explicit_profile_and_strips_inherited_configuration() {
    let mut f = fixture();
    bundle(
        &f.ticket.target,
        "0.1.0-rc.5",
        "#!/bin/sh\nprintf '%s' \"$OCTOSENSE_HOME\" > \"$OCTOSENSE_HOME/observed\"\n",
    );
    let mut child = launch(&f.ticket, None).unwrap();
    assert!(child.wait().unwrap().success());
    assert_eq!(
        fs::read_to_string(Path::new(&f.ticket.environment["OCTOSENSE_HOME"]).join("observed"))
            .unwrap(),
        f.ticket.environment["OCTOSENSE_HOME"]
    );
    f.ticket.environment.clear();
    assert!(!environment_keys().contains(&"DYLD_INSERT_LIBRARIES"));
    assert!(!environment_keys().contains(&"OCTOSENSE_RELEASE_TAG"));
}

#[test]
fn an_in_use_native_bundle_is_not_replaced() {
    let f = fixture();
    fs::copy(
        "/bin/sleep",
        f.ticket.target.join("Contents/MacOS/octosense"),
    )
    .unwrap();
    let mut child = Command::new(f.ticket.target.join("Contents/MacOS/octosense"))
        .arg("10")
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let result = ensure_bundle_stopped(&f.ticket.target);
    let _ = child.kill();
    let _ = child.wait();
    assert!(result.unwrap_err().contains("Another process"));
    ensure_bundle_stopped(&f.ticket.target).unwrap();
}
