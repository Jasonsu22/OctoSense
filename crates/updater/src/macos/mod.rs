//! macOS application replacement. Preparation runs on the updater worker;
//! the independent helper waits for an explicit commit AND process exit.
//! No shell commands or privilege escalation. App data stays in its profile.
use crate::{Cancellation, Platform, VerifiedArtifact};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::CString,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        ffi::OsStrExt,
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{mpsc, Once},
    time::{Duration, Instant},
};

mod stage;
#[cfg(test)]
mod tests;

type Result<T> = std::result::Result<T, String>;
const HELPER: &str = "octosense-update-helper";
const BUNDLE_ID: &str = "org.octosense.desktop";
const ACK_ENV: &str = "OCTOSENSE_UPDATE_JOB";
const STARTUP_WAIT: Duration = Duration::from_secs(45);
const LEGACY_GRACE: Duration = Duration::from_secs(15);

#[derive(Debug, Serialize, Deserialize)]
struct Ticket {
    protocol: u32,
    nonce: String,
    parent_pid: u32,
    target: PathBuf,
    old_digest: String,
    new_digest: String,
    new_tag: String,
    environment: BTreeMap<String, String>,
    startup_ack: bool,
    receipt: PathBuf,
}

/// Owns a helper waiting for the shell's normal, vetoable shutdown.
/// Dropping it before commit closes the pipe; the helper cancels and cleans up.
pub struct Prepared {
    child: Option<Child>,
    input: Option<ChildStdin>,
    nonce: String,
}
impl Prepared {
    /// Called once, only after every hosted app agreed to close.
    pub fn commit(mut self) -> Result<()> {
        let input = self.input.as_mut().ok_or("Update helper is unavailable.")?;
        writeln!(input, "commit {}", self.nonce)
            .map_err(|_| "Update helper stopped before installation.".to_string())?;
        input
            .flush()
            .map_err(|_| "Could not confirm the update.".to_string())?;
        self.input.take();
        // The helper must outlive this process; do not wait on the UI thread.
        self.child.take();
        Ok(())
    }
}
impl Drop for Prepared {
    fn drop(&mut self) {
        self.input.take();
        if let Some(mut child) = self.child.take() {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}

/// Whether this executable belongs to a packaged installation with
/// our helper. Permission errors during actual staging still fail closed.
pub fn available() -> bool {
    current_bundle().is_ok_and(|bundle| bundle.join("Contents/MacOS").join(HELPER).is_file())
}

fn current_bundle() -> Result<PathBuf> {
    let exe = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    bundle_for_executable(&exe)
}

fn bundle_for_executable(exe: &Path) -> Result<PathBuf> {
    let macos = exe.parent().ok_or("Missing executable folder.")?;
    let contents = macos.parent().ok_or("Missing bundle contents.")?;
    let bundle = contents.parent().ok_or("Missing app bundle.")?;
    if exe.file_name().is_none_or(|n| n != "octosense")
        || macos.file_name().is_none_or(|n| n != "MacOS")
        || contents.file_name().is_none_or(|n| n != "Contents")
        || bundle.extension().is_none_or(|n| n != "app")
    {
        return Err(
            "Install & Restart requires an installed OctoSense.app. Use Open installer instead."
                .into(),
        );
    }
    Ok(bundle.to_path_buf())
}

fn plist_field(bundle: &Path, field: &str) -> Result<String> {
    let output = Command::new("/usr/bin/plutil")
        .args(["-extract", field, "raw", "-o", "-"])
        .arg(bundle.join("Contents/Info.plist"))
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() || output.stdout.len() > 4096 {
        return Err("The app has invalid bundle metadata.".into());
    }
    Ok(String::from_utf8(output.stdout)
        .map_err(|e| e.to_string())?
        .trim()
        .to_owned())
}

fn validate_bundle(bundle: &Path, version: Option<&str>) -> Result<()> {
    if !fs::symlink_metadata(bundle).is_ok_and(|m| m.is_dir())
        || plist_field(bundle, "CFBundleIdentifier")? != BUNDLE_ID
        || plist_field(bundle, "CFBundleExecutable")? != "octosense"
        || version
            .is_some_and(|v| plist_field(bundle, "CFBundleShortVersionString").as_deref() != Ok(v))
    {
        return Err("The downloaded app does not match OctoSense's release identity.".into());
    }
    let exe = bundle.join("Contents/MacOS/octosense");
    if !fs::symlink_metadata(&exe).is_ok_and(|m| m.is_file() && m.mode() & 0o111 != 0) {
        return Err("The downloaded app has no executable.".into());
    }
    Ok(())
}

fn private_dir(path: &Path) -> Result<()> {
    let m = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o077 != 0 {
        return Err("The update staging folder is not private.".into());
    }
    Ok(())
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    f.write_all(&serde_json::to_vec(value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    f.sync_all().map_err(|e| e.to_string())?;
    fs::rename(&temp, path).map_err(|e| e.to_string())?;
    File::open(path.parent().ok_or("No update folder.")?)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())
}

fn read_ticket(job: &Path) -> Result<Ticket> {
    let root = job.parent().ok_or("Missing staging folder.")?;
    private_dir(root)?;
    if job.file_name().is_none_or(|n| n != "job.json")
        || root.canonicalize().map_err(|e| e.to_string())? != root
    {
        return Err("Invalid update job path.".into());
    }
    let mut f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(job)
        .map_err(|e| e.to_string())?;
    let m = f.metadata().map_err(|e| e.to_string())?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o077 != 0
        || m.len() > 32768
    {
        return Err("Invalid update job file.".into());
    }
    let mut data = Vec::new();
    f.read_to_end(&mut data).map_err(|e| e.to_string())?;
    let ticket: Ticket = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
    if ticket.protocol != 1
        || ticket.parent_pid < 2
        || root.file_name().and_then(|n| n.to_str())
            != Some(&format!(".octosense-update-{}", ticket.nonce))
        || uuid::Uuid::parse_str(&ticket.nonce).is_err()
        || ticket.target.parent() != root.parent()
        || ticket.target.extension().is_none_or(|n| n != "app")
        || ticket.target == root
    {
        return Err("The update job has an invalid destination.".into());
    }
    for key in ticket.environment.keys() {
        if !environment_keys().contains(&key.as_str()) {
            return Err("Unexpected relaunch setting.".into());
        }
    }
    Ok(ticket)
}

fn environment_keys() -> Vec<&'static str> {
    let mut keys = vec![
        "HOME",
        "USER",
        "LOGNAME",
        "PATH",
        "LANG",
        "TMPDIR",
        "OCTOSENSE_HOME",
        "MAKEPAD_HOME",
        "OCTOS_APP_CORE_DIR",
        "OCTOSENSE_MAIL_VAULT",
        "OCTOSENSE_LLM_VAULT",
        "OCTOSENSE_SECRETS",
    ];
    if cfg!(debug_assertions) {
        keys.extend(["MAKEPAD_HIDE_WINDOWS", "MAKEPAD_REMOTE"]);
    }
    keys
}

fn digest_tree(root: &Path) -> Result<String> {
    fn walk(
        root: &Path,
        here: &Path,
        hash: &mut Sha256,
        count: &mut usize,
        bytes: &mut u64,
    ) -> Result<()> {
        let mut entries = fs::read_dir(here)
            .map_err(|e| e.to_string())?
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(|e| e.to_string())?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            *count += 1;
            if *count > 50000 {
                return Err("Too many files in the update.".into());
            }
            let path = entry.path();
            let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
            let name = path
                .strip_prefix(root)
                .map_err(|e| e.to_string())?
                .as_os_str()
                .as_bytes();
            hash.update((name.len() as u64).to_le_bytes());
            hash.update(name);
            hash.update((meta.mode() & 0o777).to_le_bytes());
            if meta.is_dir() {
                hash.update(b"directory");
                walk(root, &path, hash, count, bytes)?;
            } else if meta.file_type().is_symlink() {
                if !path
                    .canonicalize()
                    .map_err(|e| e.to_string())?
                    .starts_with(root)
                {
                    return Err("An update resource points outside the app bundle.".into());
                }
                hash.update(b"link");
                let link = fs::read_link(&path).map_err(|e| e.to_string())?;
                let bytes = link.as_os_str().as_bytes();
                hash.update((bytes.len() as u64).to_le_bytes());
                hash.update(bytes);
            } else if meta.is_file() {
                *bytes += meta.len();
                if *bytes > 4 * 1024 * 1024 * 1024 {
                    return Err("The app bundle is too large.".into());
                }
                hash.update(b"file");
                hash.update(meta.len().to_le_bytes());
                let mut f = File::open(&path).map_err(|e| e.to_string())?;
                let mut buffer = [0; 65536];
                loop {
                    let n = f.read(&mut buffer).map_err(|e| e.to_string())?;
                    if n == 0 {
                        break;
                    }
                    hash.update(&buffer[..n]);
                }
            } else {
                return Err("Unsupported file in the app bundle.".into());
            }
        }
        Ok(())
    }
    if !fs::symlink_metadata(root).is_ok_and(|m| m.is_dir()) {
        return Err("The app bundle must be a directory, not a link.".into());
    }
    let mut hash = Sha256::new();
    walk(root, root, &mut hash, &mut 0, &mut 0)?;
    Ok(format!("{:x}", hash.finalize()))
}

/// Exchange whole bundles in one filesystem operation: even a power loss
/// cannot leave the installed path absent between two separate renames.
fn exchange(a: &Path, b: &Path) -> Result<()> {
    let a = CString::new(a.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let b = CString::new(b.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    if unsafe { libc::renamex_np(a.as_ptr(), b.as_ptr(), libc::RENAME_SWAP) } != 0 {
        return Err(format!(
            "Could not replace the app: {}. Use Open installer instead.",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

fn process_exists(pid: u32) -> bool {
    unsafe {
        libc::kill(pid as i32, 0) == 0
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
}

/// Refuse to replace a bundle still used by another instance or bundled child.
/// proc_pidpath reports executable paths without exposing command-line secrets.
fn ensure_bundle_stopped(bundle: &Path) -> Result<()> {
    let mut pids = vec![0i32; 65536];
    let bytes =
        unsafe { libc::proc_listpids(1, 0, pids.as_mut_ptr().cast(), (pids.len() * 4) as i32) };
    if bytes <= 0 || bytes as usize >= pids.len() * 4 {
        return Err("Could not check for running app instances. No replacement was made.".into());
    }
    for pid in pids
        .into_iter()
        .take(bytes as usize / 4)
        .filter(|pid| *pid > 0)
    {
        let mut path = [0u8; 4096];
        let size = unsafe { libc::proc_pidpath(pid, path.as_mut_ptr().cast(), path.len() as u32) };
        if size <= 0 {
            continue;
        } // Exited or a process owned by another user.
        let end = path.iter().position(|b| *b == 0).unwrap_or(path.len());
        let executable = Path::new(std::ffi::OsStr::from_bytes(&path[..end]));
        if executable.starts_with(bundle) {
            return Err("Another process is using this app bundle. Close its other instances before installing.".into());
        }
    }
    Ok(())
}

/// Idempotent recovery, including a helper interrupted on either side of the
/// atomic exchange. It never guesses from the journal alone.
fn restore_previous(ticket: &Ticket, job: &Path, candidate: &Path) -> Result<()> {
    let installed = digest_tree(&ticket.target)?;
    let staged = digest_tree(candidate)?;
    if installed == ticket.new_digest && staged == ticket.old_digest {
        exchange(&ticket.target, candidate)?;
    } else if installed != ticket.old_digest || staged != ticket.new_digest {
        return Err(
            "Recovery stopped because a bundle changed. Both bundles were preserved.".into(),
        );
    }
    write_json(
        &job.parent().unwrap().join("state.json"),
        &serde_json::json!({"phase":"rolled_back"}),
    )?;
    status(
        ticket,
        "rolled_back",
        "The previous version was restored. Your app data was not replaced.",
    )
}

fn launch(ticket: &Ticket, job: Option<&Path>) -> Result<Child> {
    let cache = ticket.receipt.parent().ok_or("No update log folder.")?;
    private_dir(cache)?;
    let log = OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(cache.join(format!("relaunch-{}.log", ticket.nonce)))
        .map_err(|e| e.to_string())?;
    let mut command = Command::new(ticket.target.join("Contents/MacOS/octosense"));
    command
        .env_clear()
        .envs(&ticket.environment)
        .current_dir(ticket.target.parent().ok_or("No app folder.")?)
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log);
    if let Some(job) = job {
        command.env(ACK_ENV, job);
    } else {
        command.env("OCTOSENSE_UPDATE_ROLLED_BACK", "1");
    }
    command
        .spawn()
        .map_err(|e| format!("Could not relaunch OctoSense: {e}"))
}

fn status(ticket: &Ticket, state: &str, message: &str) -> Result<()> {
    private_dir(ticket.receipt.parent().ok_or("No receipt folder.")?)?;
    write_json(
        &ticket.receipt,
        &serde_json::json!({"version":ticket.new_tag,"state":state,"message":message}),
    )
}

/// Entrypoint of the separately packaged installer, copied outside the app
/// before launch. A pipe, not a command-line flag, commits the replacement.
pub fn helper_main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let first = args.next().ok_or("Missing update job.")?;
    let restore = first == "--restore";
    let job = PathBuf::from(if restore {
        args.next().ok_or("Missing recovery job.")?
    } else {
        first
    });
    if args.next().is_some() {
        return Err("Unexpected helper arguments.".into());
    }
    let ticket = read_ticket(&job)?;
    let root = job.parent().unwrap();
    let candidate = root.join("candidate.app");
    let lock_path = ticket.target.with_extension("octosense-update-lock");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(lock_path)
        .map_err(|e| e.to_string())?;
    use std::os::fd::AsRawFd;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err("Another update is already installing.".into());
    }
    let meta = lock.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
        return Err("Invalid update lock file.".into());
    }
    if restore {
        ensure_bundle_stopped(&ticket.target)?;
        restore_previous(&ticket, &job, &candidate)?;
        println!("Previous app restored. You can open it normally; your profile was not replaced.");
        return Ok(());
    }
    println!("ready");
    std::io::stdout().flush().map_err(|e| e.to_string())?;
    let mut instruction = String::new();
    std::io::stdin()
        .lock()
        .take(128)
        .read_line(&mut instruction)
        .map_err(|e| e.to_string())?;
    if instruction.trim_end() != format!("commit {}", ticket.nonce) {
        fs::remove_dir_all(root).map_err(|e| e.to_string())?;
        return Ok(());
    }
    write_json(
        &root.join("state.json"),
        &serde_json::json!({"phase":"waiting_for_exit"}),
    )?;
    let deadline = Instant::now() + Duration::from_secs(180);
    while process_exists(ticket.parent_pid) {
        if Instant::now() >= deadline {
            status(
                &ticket,
                "cancelled",
                "OctoSense did not close. The installed app was not changed.",
            )?;
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let result = ensure_bundle_stopped(&ticket.target)
        .and_then(|_| replace_and_start(&ticket, &job, &candidate));
    if let Err(ref error) = result {
        let _ = status(&ticket, "failed", error);
    }
    result
}

fn replace_and_start(ticket: &Ticket, job: &Path, candidate: &Path) -> Result<()> {
    if digest_tree(&ticket.target)? != ticket.old_digest
        || digest_tree(candidate)? != ticket.new_digest
    {
        return Err("The installed app or staged update changed. No replacement was made.".into());
    }
    validate_bundle(candidate, ticket.new_tag.strip_prefix("desktop-v"))?;
    let root = job.parent().unwrap();
    write_json(
        &root.join("state.json"),
        &serde_json::json!({"phase":"swapping"}),
    )?;
    exchange(&ticket.target, candidate)?;
    // The former app now lives at candidate.app and remains available for recovery.
    let started = (|| {
        write_json(
            &root.join("state.json"),
            &serde_json::json!({"phase":"starting"}),
        )?;
        let mut child = launch(ticket, Some(job))?;
        let deadline = Instant::now()
            + if ticket.startup_ack {
                STARTUP_WAIT
            } else {
                LEGACY_GRACE
            };
        loop {
            if child.try_wait().map_err(|e| e.to_string())?.is_some() {
                return Err("The new app exited before startup completed.".to_string());
            }
            if ticket.startup_ack
                && fs::read_to_string(root.join("started")).is_ok_and(|s| s == ticket.nonce)
            {
                break;
            }
            if Instant::now() >= deadline {
                if !ticket.startup_ack {
                    break;
                }
                let _ = child.kill();
                let _ = child.wait();
                return Err("The new app did not confirm startup.".to_string());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(())
    })();
    if let Err(error) = started {
        if digest_tree(&ticket.target)? != ticket.new_digest
            || digest_tree(candidate)? != ticket.old_digest
        {
            return Err(format!("{error} Automatic recovery stopped because an app bundle changed. The previous bundle is retained."));
        }
        ensure_bundle_stopped(&ticket.target)?;
        restore_previous(ticket, job, candidate)?;
        let _ = launch(ticket, None)?;
        return Ok(());
    }
    write_json(
        &root.join("state.json"),
        &serde_json::json!({"phase":"complete","startup_ack":ticket.startup_ack}),
    )?;
    status(
        ticket,
        "complete",
        if ticket.startup_ack {
            "Update installed and startup confirmed. The previous app is retained for recovery."
        } else {
            "Update installed. The older release stayed running for 15 seconds; it has no startup acknowledgement. The previous app is retained for recovery."
        },
    )
}

/// Called after the shell draws its first frame. Receipt IO runs off the UI
/// thread; only the expected executable at the exact installed path can ACK.
pub fn acknowledge_startup() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let Some(job) = std::env::var_os(ACK_ENV).map(PathBuf::from) else {
            return;
        };
        std::thread::spawn(move || {
            let result = (|| -> Result<()> {
                let ticket = read_ticket(&job)?;
                if current_bundle()? != ticket.target
                    || digest_tree(&ticket.target)? != ticket.new_digest
                {
                    return Err("Relaunch does not match the prepared update.".into());
                }
                let path = job.parent().unwrap().join("started");
                let mut f = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(path)
                    .map_err(|e| e.to_string())?;
                f.write_all(ticket.nonce.as_bytes())
                    .map_err(|e| e.to_string())?;
                f.sync_all().map_err(|e| e.to_string())
            })();
            if let Err(error) = result {
                eprintln!("Update startup acknowledgement: {error}");
            }
        });
    });
}

pub use stage::prepare;
