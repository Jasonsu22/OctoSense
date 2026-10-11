# Updating OctoSense

English | [简体中文](updater.zh-CN.md)

Open **OctoSense Updates** in the desktop or Home launcher. Android Home also
offers **Settings → Updates → Open OctoSense app updater**. Choose a channel,
then **Check for updates**. Review the offered version and download size, choose
**Download update**, and open the installer separately after verification.

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
| macOS, Apple silicon | Opens the verified `.dmg` | Quit OctoSense, drag the new app into Applications, confirm replacement, then reopen it. |
| Windows, x64 | Starts the verified NSIS `.exe` directly | Follow its prompts; close OctoSense when requested. |
| Linux, x86_64, Debian package | Saves a copy of the verified `.deb` in the Downloads folder, checks it, and opens it with the desktop's package handler | Approve the package installation, then restart. Without a desktop package handler, install that file with the distribution's package manager. |
| Linux, x86_64, AppImage | Saves a checked, executable copy in the Downloads folder and opens that folder | Quit OctoSense, replace the old AppImage with the new one, then open it. The updater does not overwrite the running image. |
| Android, ARM64 | Checks the verified APK against installed Home and stages it with Android's package installer | Review and confirm the operating system's installation prompt. |

Opening an installer does **not** report a completed installation. This first
version does not check periodically, silently quit, elevate itself, overwrite
the running application, or remove the user's profile. The operating system's
installer still runs its own checks, but macOS and Windows do not show the
warnings they give a file downloaded in a browser (Gatekeeper's, SmartScreen's):
the updater does not mark its downloads as coming from the internet. It relies
on GitHub's signature on the release instead ([Verification and
privacy](#verification-and-privacy)).

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

Native installer handoff and replacing an installed desktop app remain
**unverified** for RC6. Full installed-app workflows and model calls were not
part of this release acceptance. Before signature enforcement was added,
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
