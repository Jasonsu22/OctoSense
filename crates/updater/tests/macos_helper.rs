#![cfg(target_os = "macos")]
//! Run the actual packaged helper protocol, without a network or live user profile.
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    job: PathBuf,
    target: PathBuf,
    nonce: String,
}
fn fixture() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let parent = temp.path().canonicalize().unwrap();
    let nonce = uuid::Uuid::new_v4().to_string();
    let root = parent.join(format!(".octosense-update-{nonce}"));
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let target = parent.join("OctoSense.app");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("untouched"), b"original").unwrap();
    let job = root.join("job.json");
    let ticket = serde_json::json!({"protocol":1,"nonce":nonce,"parent_pid":std::process::id(),
        "target":target,"old_digest":"","new_digest":"","new_tag":"desktop-v0.1.0-rc.6",
        "environment":{},"startup_ack":true,"receipt":root.join("receipt.json")});
    fs::write(&job, serde_json::to_vec(&ticket).unwrap()).unwrap();
    fs::set_permissions(&job, fs::Permissions::from_mode(0o600)).unwrap();
    Fixture {
        _temp: temp,
        root,
        job,
        target,
        nonce,
    }
}
fn start(f: &Fixture) -> Child {
    Command::new(env!("CARGO_BIN_EXE_octosense-update-helper"))
        .arg(&f.job)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}
fn ready(child: &mut Child) {
    let stdout = child.stdout.take().unwrap();
    let (send, recv) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = BufReader::new(stdout).read_line(&mut text);
        let _ = send.send(text);
    });
    let line = recv.recv_timeout(Duration::from_secs(5));
    if line.as_deref() != Ok("ready\n") {
        let _ = child.kill();
        let _ = child.wait();
    }
    assert_eq!(line.unwrap(), "ready\n");
}
fn wait(child: &mut Child) -> std::process::ExitStatus {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(code) = child.try_wait().unwrap() {
            return code;
        }
        if Instant::now() >= until {
            let _ = child.kill();
            let _ = child.wait();
            panic!("helper did not exit");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
#[test]
fn closing_commit_pipe_cancels_and_removes_only_the_staging_folder() {
    for instruction in [None, Some("commit wrong-nonce\n")] {
        let f = fixture();
        let mut child = start(&f);
        ready(&mut child);
        let mut pipe = child.stdin.take().unwrap();
        if let Some(text) = instruction {
            pipe.write_all(text.as_bytes()).unwrap();
        }
        drop(pipe);
        assert!(wait(&mut child).success());
        assert!(!f.root.exists());
        assert_eq!(fs::read(f.target.join("untouched")).unwrap(), b"original");
    }
}
#[test]
fn commit_waits_for_parent_exit_and_a_second_helper_cannot_race_it() {
    let f = fixture();
    let mut child = start(&f);
    ready(&mut child);
    let mut other = start(&f);
    let denied = wait(&mut other);
    let output = other.wait_with_output().unwrap();
    assert!(!denied.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Another update"));
    writeln!(child.stdin.as_mut().unwrap(), "commit {}", f.nonce).unwrap();
    child.stdin.take();
    let until = Instant::now() + Duration::from_secs(5);
    while !f.root.join("state.json").exists() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    let state = fs::read_to_string(f.root.join("state.json"));
    let unchanged = fs::read(f.target.join("untouched"));
    let alive = child.try_wait().unwrap().is_none();
    let _ = child.kill(); // this test owns the helper; model a helper interruption
    let _ = child.wait();
    assert!(alive);
    assert!(state.unwrap().contains("waiting_for_exit"));
    assert_eq!(unchanged.unwrap(), b"original");
    assert!(f.root.exists(), "interruption retains its recovery journal");
}
