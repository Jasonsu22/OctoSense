# Updating OctoSense

English | [简体中文](updater.zh-CN.md)

Open **OctoSense Updates** in the desktop or Home launcher. Android Home also
offers **Settings → Updates → Open OctoSense app updater**. Choose a channel,
then **Check for updates**. Review the offered version and download size, choose
**Download update**, then explicitly choose installation after verification.

| Channel | Eligible releases |
| --- | --- |
| Stable | Stable releases only. |
| Release candidate | Stable releases and `rc` prereleases; excludes beta, alpha and nightly releases. |
| Preview | Stable releases and all semantic prereleases. |

An installed RC defaults to the release-candidate channel. Versions compare
numerically: `rc.10` follows `rc.9`, and the corresponding stable version follows
its RCs. Switching channels does not permit a downgrade.

Desktop [RC6](https://github.com/OctoSense-org/OctoSense/releases/tag/desktop-v0.1.0-rc.6)
is the first desktop release with this updater. Install RC6 manually once when
coming from RC5 or earlier; use **OctoSense Updates** for subsequent eligible
releases. Home `0.1.0-beta.2` does not contain the updater, and this desktop
release does not update Home. Unversioned development builds do not know their
installed release and cannot offer an update.

## Installation

| Platform | Handoff | What the user finishes |
| --- | --- | --- |
| macOS, Apple silicon | This source build offers **Install & Restart** in a packaged app with its helper; RC6 opens the verified `.dmg` | Save or resolve any app close questions. The helper replaces the bundle and reopens the same profile. **Open installer manually** remains available. |
| Windows, x64 | Starts the verified NSIS `.exe` directly | Follow its prompts; close OctoSense when requested. |
| Linux, x86_64, Debian package | Saves a copy of the verified `.deb` in the Downloads folder, checks it, and opens it with the desktop's package handler | Approve the package installation, then restart. Without a desktop package handler, install that file with the distribution's package manager. |
| Linux, x86_64, AppImage | Saves a checked, executable copy in the Downloads folder and opens that folder | Quit OctoSense, replace the old AppImage with the new one, then open it. The updater does not overwrite the running image. |
| Android, ARM64 | Checks the verified APK against installed Home and stages it with Android's package installer | Review and confirm the operating system's installation prompt. |

Opening an installer does **not** report a completed installation. Checks remain
manual. The updater does not elevate itself or remove the user's profile.
On macOS, replacement happens only after **Install & Restart**, app close review,
and the old process exiting. The operating system's
installer still runs its own checks, but macOS and Windows do not show the
warnings they give a file downloaded in a browser (Gatekeeper's, SmartScreen's):
the updater does not mark its downloads as coming from the internet. It relies
on GitHub's signature on the release instead ([Verification and
privacy](#verification-and-privacy)).

### macOS Install & Restart (source build; not in RC6)

The first release containing this helper still needs to be installed through
RC6's manual installer flow. Subsequent updates can replace the complete app
bundle, including resources and the kernel, without a Finder drag operation.
A renamed app stays at its current path; an unwritable installation folder
falls back to manual installation without asking for elevated privileges.

Preparation runs in the background: recheck the signed DMG, mount it read-only,
validate its bundle ID and release version, and copy it to a private sibling
folder. Fingerprints cover file contents, names, permissions and contained
symlinks. Only then does the host ask apps to close through their existing
save/close protocol. A refusal waits for the person; an unanswered request
cancels the update instead of force-closing the app. Cancelling or closing the
Updates window before shutdown cancels the waiting helper.

The helper is copied outside the app before it starts. It requires confirmation
on a private pipe, waits for the host to exit, and refuses replacement while
another process uses that bundle. One atomic filesystem exchange installs the
new bundle and retains the previous one. The new app reopens with the same
profile and acknowledges its first drawn frame. A failed startup restores the
previous bundle when both fingerprints still match. App data is **not** rolled
back: compatibility of any future profile migration remains a release concern.

An older target such as public RC6 has no startup-acknowledgement protocol.
For that compatibility test only, remaining alive for 15 seconds is the health
check; it does not prove a functioning UI. The receipt records that distinction.
Neither path proves that every app or model provider works after an update.

Backups are retained in `.octosense-update-<id>/candidate.app` beside the
installed app. `updates/last-install.json` records the outcome; private
`install-<id>.log` files record helper errors. A helper interruption leaves a
complete bundle at the installed path and both versions available. With all
instances closed, the retained helper's `--restore <job.json>` operation can
restore the original bundle, including when interruption occurred immediately
before or after exchange. It refuses changed bundles and is safe to repeat.
Delete a retained staging folder only after confirming the new version works
and no installer is running. Backup retention is manual in this implementation.

Linux package handlers may be unable to read the hidden profile folder the
download lives in (Ubuntu's App Center is a snap, and snaps cannot see hidden
folders), so Linux packages are handed over from the Downloads folder
(`XDG_DOWNLOAD_DIR`, else `~/Downloads`). A file of the same name there is never
replaced: the copy takes the next free name, such as
`octosense_0.2.0_amd64 (2).deb`. Copies in Downloads are yours to delete.

Android 13 (API 33) or newer is required. Android updates **standalone Home** only. System Bridge and ROM updates remain
separate. The installed package and candidate must have the public standalone
Home signing identity; system-installed Home, ROM signatures, isolated test
packages and other signers are refused. The candidate must have a higher Android
version code and a compatible minimum Android version. If Android has not
allowed OctoSense to install packages, the user can open its permission settings,
return, and explicitly try installation again. No automatic installation resumes
after changing that permission. An APK is never installed by first uninstalling
Home to bypass a signer mismatch. **Review and install update** and **Allow
updates in Android Settings** act only on a real tap on the screen: Makepad's
input provenance, which a script, a remote control or a synthetic click cannot
produce.

## Verification and privacy

The host reads public releases from `OctoSense-org/OctoSense` on GitHub. Desktop
selects `desktop-v…` tags; Home selects `home-v…` tags. Semantic version and the
exact platform asset name decide the offer; release publication order does not.

Both GitHub's SHA-256 asset digests and `SHA256SUMS` are required. The updater
checks the checksum file's own digest, requires its installer hash to match
GitHub's metadata, then verifies the complete downloaded installer and declared
size. Downloads use private staging and are rechecked before OS handoff.
Malformed metadata, unexpected URLs, symlinks, incomplete or modified files are
refused.

Digests and checksums only prove that the files are the release's own: anyone
who can publish a release can make them agree. So a desktop release is offered
only when its `SHA256SUMS` also carries **GitHub's signature** from the
[`sign-desktop-release`](../.github/workflows/sign-desktop-release.yml)
workflow, run on `main` in the `desktop-release-signing` environment, whose
reviewers approve each run. The signature is a Sigstore bundle attached to the
release as `SHA256SUMS.sigstore.json`. The updater verifies it offline, the way
a device verifies App Hub's catalog: the certificate, its timestamps and the
transparency-log proof against Sigstore's public trust root; then GitHub's
claims about the run (this repository and its owner, that workflow on `main`, a
manual run, that environment, a GitHub-hosted runner); then the signed subject,
`<tag>.SHA256SUMS` with the exact digest of the file it downloaded. A desktop
release without that signature is refused with an error, not skipped. Android
Home needs none: Android installs only an APK signed with the pinned standalone
release key.

The release list is read newest first, at most ten pages of 100 releases; a
page may hold up to 16 MB (a release's notes and asset list took about 28 KB in
October 2026).

Desktop downloads live in the shell's `updates/` directory (`OCTOSENSE_HOME` if
set, otherwise `~/.octosense/updates`). Android uses app-private cache under
`octosense-updates/`. A verified download is deleted a day after it was last
written, whether or not its installer was opened, and an interrupted one after
an hour; this happens when OctoSense starts and before each check. Only the
updater's own `verified-…` and `pending-…` folders holding plain files are
removed, never recursively. Checking releases needs no OctoSense, GitHub or Google
login. No mail, app contents, model credentials or agent conversations are sent.
The updater is a native host interface, not an App Hub app or an agent tool.

## Code and release identity

- [`crates/updater`](../crates/updater/src/lib.rs) owns channels, release
  selection, bounded downloads, cancellation, verified artifacts and removing
  stale downloads. Its [`signature.rs`](../crates/updater/src/signature.rs)
  verifies GitHub's signature on a desktop release's `SHA256SUMS`, which
  [`sign-desktop-release.yml`](../.github/workflows/sign-desktop-release.yml)
  makes for each release ([desktop/README.md](../desktop/README.md#cutting-a-desktop-release)).
- [`crates/updater-ui`](../crates/updater-ui/src/lib.rs) owns the native module
  and explicit user actions. Network work and hashing run off the drawing thread.
- [`macos`](../crates/updater/src/macos/mod.rs) owns staging, the independent
  helper, atomic exchange and recovery; the shell bridges installation to its
  existing close review in [`update_restart.rs`](../crates/shell/src/update_restart.rs).
- [`install.rs`](../crates/updater-ui/src/install.rs) uses literal arguments
  for desktop installer handoff; release data never becomes a shell command.
- Android's native extension independently checks package, version, signer and
  bytes before staging a session that requires OS user confirmation.
- [`native-apps.json`](../native-apps.json) registers the module in desktop and
  Home, without an agent update tool.

Cargo's common `0.1.0` package version is not the installed release identity.
Packaging embeds `OCTOSENSE_RELEASE_TAG` instead. The
[desktop packager](../desktop/scripts/package.py) derives it from an explicit
`--version-from-tag` or `--version`. Tagged `--skip-build` packaging requires a
matching tag and binary SHA-256 receipt from an earlier build, preventing a stale
binary from being relabeled. Generic builds clear inherited tags. The
desktop build and packager explicitly select the native Rust target; the binary
and identity receipt come from `target/<host-triple>/release/`. Inherited
`CARGO_TARGET_DIR`, `CARGO_BUILD_TARGET` or Cargo configuration cannot redirect
the build while leaving packaging to read a stale `target/release` executable.
The [Home builder](../rom/scripts/build-home.py) sets the tag only for a versioned,
signed standalone build; development and ROM builds leave it empty.

## What we learned from OctosCode

The native OctosCode app at `d47fade94498` documents a manual RC download; it
does not contain the CLI updater. The separate
[`octoscode update` CLI](https://github.com/octos-org/octoscode/blob/8722b551a7ee653c2457f1152a636160351b9b1f/src/cmd/update.rs)
uses `axoupdater` for receipt-owned cargo-dist installations and defers to
Homebrew, npm or Cargo for installations they own. Its prerelease option resolves
an exact channel tag and tests numeric RC ordering. OctoSense follows those
ownership and explicit-channel principles while handing complete app packages
to the native installer. Unlike that CLI's explicit prerelease channel switch,
this interface does not offer a stable-to-RC downgrade.

The [octos kernel updater](https://github.com/octos-org/octos/blob/084baa522a508cf3af7d7ce9585868ac710eb1d9/crates/octos-services/src/updater.rs)
backs up and replaces a bounded set of CLI binaries with rollback. It is not a
full Makepad app-bundle or Android installer. OctoSense also requires checksums
even for older releases, rather than allowing the kernel updater's legacy
missing-checksum fallback.

## Validation scope

Desktop handoff tests check executable names and literal arguments without
launching installers. Packaging tests cover release identity and stale-binary
refusal; signature tests cover certificate, transparency-log and GitHub claims,
including tampered proofs and unexpected workflows, branches and subjects.

[RC6](https://github.com/OctoSense-org/OctoSense/releases/tag/desktop-v0.1.0-rc.6)
was built from `f9f6fea9` by the [official release workflow](https://github.com/OctoSense-org/OctoSense/actions/runs/38125407028)
and signed by the [protected checksum workflow](https://github.com/OctoSense-org/OctoSense/actions/runs/38137543497)
after normal reviewer approval. All ten final assets were downloaded and their
digests checked. The exact RC6 verifier accepted its real signature and rejected
modified checksums and a different release tag.

After publication, the exact-source updater Client, using a **synthetic RC5
installed identity**, selected RC6 for macOS arm64, Windows x64, Linux DEB and
Linux AppImage. It also downloaded and revalidated the macOS installer. This
checks the new Client's offer and verification logic; RC5 itself has no updater.

| Packaged runtime | Executed acceptance |
| --- | --- |
| macOS arm64 | Fresh public download; RC/Preview report up to date, Stable reports unavailable. Public App Hub search, installation, actual Repo Radar launch and restart persistence passed in an isolated profile. |
| Linux x64 | Six native startup checks; live signed RC check reports up to date in an isolated DEB filesystem layout and extracted official AppImage. No system package installation or FUSE launch. |
| Windows x64 | Official build and static archive, executable, resource and kernel-receipt checks. GUI installation and launch remain **unverified**. |

The macOS Install & Restart source build passed 21 isolated native checks
against the signed public RC6 DMG: cancel, automatic replacement and relaunch,
RC6/up-to-date UI, unchanged Unicode note, preserved and runnable Repo Radar,
recovery and cleanup. The starting build was **source labelled RC5**, not the
historical RC5 release. Local source-bundle fixtures separately passed 13 checks
for first-frame acknowledgement, failed-start rollback, profile preservation
and cleanup; the rollback notification was also visually reviewed. These
fixtures do not replace release-signature verification. An actual cargo-packager
run included the exact helper and protocol marker. Windows/Linux installation,
other installed-app workflows and model calls remain **unverified**. Before signature enforcement was added,
isolated macOS tests exercised an RC1 → RC2 download, tamper rejection,
channel changes and cancellation; those are historical checks, not proof that
unsigned older releases are accepted by RC6. Cleanup and Linux's copy into
Downloads are tested in temporary folders. Android's real-tap check on a
physical device remains **unverified**.

On an owned API 35 Android emulator, a fixture using the actual Java updater
adapter and OS PackageInstaller passed 25 acceptance checks, including cleanup: permission changes
did not replay installation automatically; wrong signatures, packages, hashes,
version codes and minimum SDK levels were rejected; the visible OS confirmation
could be cancelled and retried; version code 1000 → 1001 retained the app UID
and synthetic app data, with status recovery after replacement. This exercised
the native adapter, **not** the complete Rust Home binary. The physical phone
was untouched; end-to-end installation from a released Home build remains
**unverified**.
