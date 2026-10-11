//! Launch input: settings that arrive with a launch, which another app on
//! the device may have chosen.
//!
//! **Who can send it.** Home's main activity is exported: it is the app
//! drawer entry, the Home app, a share target and a deep-link target, so any
//! installed app can start it, with extras. When the activity is created,
//! Makepad's Android glue copies the extras `makepad.<NAME>` into the
//! environment as `MAKEPAD_<NAME>` (`APP_CONFIG`, `PROVISION_CONFIG`,
//! `OCTOS_PROXY`, the `SEED_*` hooks and others). On OpenHarmony the entry
//! ability passes the Want's `makepad.*` parameters on in the same way, but
//! only in a development build (`phone/ohos/EntryAbility.ets`). On a desktop
//! these variables come from the local user's own environment, as
//! command-line flags do.
//!
//! **The rule.** OctoSense reads launch input only in a development build
//! (`cfg(dev_mode)`: a debug build, or `--features dev-mode`; never a store
//! build). That covers the app config below (test actions, Mail's demo
//! mailbox, module open parameters), the kernel's launch proxy
//! (`octosense-kernel`) and AppCard's provisioning and seeding hooks
//! (`octos-app`). A release build ignores them, and says so once in the log
//! without echoing the value. `--test-action` flags on a desktop command
//! line are arguments only the local user can pass; they work in every
//! build (`App::run_test_actions` in `lib.rs`).
//!
//! Test builds that need these hooks on a phone are development builds
//! under a separate package name (`rom/scripts/build-home.py --dev-mode`).

use crate::dev_mode::BuildKind;
use makepad_strict_json::Value;

/// The app config: one JSON object,
/// `{"test_actions": ["<name>", …], "mail_demo": true,
/// "module_open": {"<module id>": {…}}, "mail_endpoint": "…"}`.
pub const APP_CONFIG: &str = "MAKEPAD_APP_CONFIG";

/// Whether a build of this kind reads launch input: a development build
/// only.
pub fn honoured(build: BuildKind) -> bool {
    build == BuildKind::Development
}

/// The app config, parsed, when this build reads launch input. A release
/// build answers `None` whatever the variable holds.
pub fn app_config() -> Option<Value> {
    let text = std::env::var(APP_CONFIG).ok()?;
    let build = BuildKind::current();
    if !honoured(build) {
        note_ignored();
    }
    parse_app_config(build, &text)
}

/// [`app_config`] for a given build and variable text.
pub fn parse_app_config(build: BuildKind, text: &str) -> Option<Value> {
    if !honoured(build) {
        return None;
    }
    makepad_strict_json::parse(text.as_bytes()).ok()
}

/// The app config's `test_actions`, in order, as `--test-action` names;
/// none in a release build.
pub fn test_actions() -> Vec<String> {
    app_config().map(|config| test_actions_in(&config)).unwrap_or_default()
}

/// [`test_actions`] for a given build and variable text.
pub fn test_actions_for(build: BuildKind, text: &str) -> Vec<String> {
    parse_app_config(build, text).map(|config| test_actions_in(&config)).unwrap_or_default()
}

fn test_actions_in(config: &Value) -> Vec<String> {
    config
        .get("test_actions")
        .and_then(|actions| actions.as_arr())
        .map(|actions| actions.iter().filter_map(|action| action.as_str()).map(str::to_owned).collect())
        .unwrap_or_default()
}

/// The `true`/`false` value `key` has in the app config, when this build
/// reads it.
pub fn flag(key: &str) -> bool {
    app_config().and_then(|config| config.get(key).and_then(|value| value.as_bool())).unwrap_or(false)
}

fn note_ignored() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        makepad_widgets::log!(
            "launch input: {APP_CONFIG} ignored; a release build takes no test actions or demo settings from a launch"
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"{"test_actions":["capture:/sdcard/Download/frame.png","launch-mail","system-chat-send:hello"],"mail_demo":true}"#;

    #[test]
    fn a_release_build_ignores_test_actions_from_the_launch() {
        for build in [BuildKind::Release, BuildKind::Store] {
            assert!(!honoured(build), "{build:?}");
            assert!(test_actions_for(build, CONFIG).is_empty(), "{build:?}");
            assert!(parse_app_config(build, CONFIG).is_none(), "{build:?}");
        }
    }

    #[test]
    fn a_development_build_keeps_them_in_order() {
        assert!(honoured(BuildKind::Development));
        assert_eq!(
            test_actions_for(BuildKind::Development, CONFIG),
            ["capture:/sdcard/Download/frame.png", "launch-mail", "system-chat-send:hello"]
        );
        let config = parse_app_config(BuildKind::Development, CONFIG).expect("parsed");
        assert_eq!(config.get("mail_demo").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn malformed_or_missing_actions_give_none() {
        for text in ["", "not json", r#"{"test_actions":"launch-mail"}"#, r#"{"mail_demo":true}"#] {
            assert!(test_actions_for(BuildKind::Development, text).is_empty(), "{text:?}");
        }
        // Entries that are not strings are skipped, the rest kept.
        assert_eq!(test_actions_for(BuildKind::Development, r#"{"test_actions":[1,"launch-news",null]}"#), ["launch-news"]);
    }

    /// The shell has one reader of the app config: this module. Anything
    /// else reading the variable directly would bypass the build check.
    #[test]
    fn no_other_shell_source_reads_the_app_config_directly() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders = Vec::new();
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("read src") {
                let path = entry.expect("entry").path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") && !path.ends_with("launch_input.rs") {
                    let text = std::fs::read_to_string(&path).expect("read source");
                    if text.contains("env::var(\"MAKEPAD_APP_CONFIG\"") || text.contains("env::var_os(\"MAKEPAD_APP_CONFIG\"") {
                        offenders.push(path.strip_prefix(&root).unwrap().display().to_string());
                    }
                }
            }
        }
        assert!(offenders.is_empty(), "read MAKEPAD_APP_CONFIG through launch_input: {offenders:?}");
    }
}
