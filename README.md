<div align="center">
  <img src="packaging/icons/git-leviathan.png" alt="Git Leviathan logo" width="200" />

  # Git Leviathan

  **A fast, native Git client for humans who like to see the shape of their history.**

  [![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
  [![Built with Rust](https://img.shields.io/badge/Built%20with-Rust-orange.svg)](https://www.rust-lang.org/)
  [![Powered by Iced](https://img.shields.io/badge/GUI-Iced-purple.svg)](https://iced.rs/)
</div>

---

## Beta software
This is a brand new piece of software. Expect bugs and instability! If you encounter a bug, I would hugely appreciate if you'd create a new issue.

**Looking for MacOS and Windows users to test. Please let me know what issues you run into!**

## About

Git Leviathan is a desktop Git repository visualizer written in Rust and built on the [Iced](https://iced.rs/) GUI framework. It turns the tangled mass of your commit history into a clean, navigable graph — branches, merges, tags, and all — so you can read a repository the way it actually is, not the way `git log` pretends.

Under the hood it talks to your repos via `libgit2` (vendored, no system dependency), persists settings in SQLite, and reacts to filesystem changes in real time. The result: a responsive, native-feeling client that boots fast, stays out of your way, and never phones home.

## Features

- **Graph-first history view** — every branch, merge, and tag rendered as a readable DAG.
- **Multi-tab workspace** — keep several repositories open side by side.
- **Diff viewer with syntax highlighting** — powered by `syntect` and `two-face`.
- **Live filesystem watching** — the UI updates when your working tree does.
- **Structured error reporting** — auth failures, corrupt objects, and network issues surfaced with context, not stack traces.
- **Cross-platform** — Linux, macOS, and Windows.
- **Fully native, fully offline** — no Electron, no telemetry, no account.

## Installation

Pre-built binaries for every tagged release are available on the [Releases page](https://github.com/shinyvision/git-leviathan/releases). Pick the asset that matches your platform:

| Platform | Architecture | Asset |
|----------|--------------|-------|
| Debian / Ubuntu | x86_64 | `git-leviathan_<version>-1_amd64.deb` |
| Debian / Ubuntu | arm64 | `git-leviathan_<version>-1_arm64.deb` |
| Fedora / RHEL / openSUSE | x86_64 | `git-leviathan-<version>-1.x86_64.rpm` |
| Fedora / RHEL / openSUSE | arm64 | `git-leviathan-<version>-1.aarch64.rpm` |
| Other Linux (portable) | x86_64 | `git-leviathan-<version>-linux-amd64.tar.gz` |
| Other Linux (portable) | arm64 | `git-leviathan-<version>-linux-arm64.tar.gz` |
| macOS (Apple Silicon) | arm64 | `git-leviathan-<version>-aarch64-apple-darwin.tar.gz` |
| Windows | x86_64 | `git-leviathan-<version>-x86_64-pc-windows-msvc.zip` |

> Replace `<version>` with the release you're downloading (e.g. `0.1.0`).

### Debian / Ubuntu

```bash
sudo dpkg -i git-leviathan_<version>-1_amd64.deb
sudo apt-get install -f   # pull in any missing dependencies
```

### Fedora / RHEL / openSUSE

```bash
sudo dnf install ./git-leviathan-<version>-1.x86_64.rpm
# or: sudo rpm -i git-leviathan-<version>-1.x86_64.rpm
```

### Any Linux (portable tarball)

Extract the archive to `/` to install system-wide (binary goes to `/usr/bin`, desktop entry and icons land in `/usr/share`):

```bash
sudo tar -xzf git-leviathan-<version>-linux-amd64.tar.gz -C /
sudo gtk-update-icon-cache /usr/share/icons/hicolor   # optional, refreshes icon cache
```

To uninstall:

```bash
sudo rm /usr/bin/git_leviathan \
        /usr/share/applications/git-leviathan.desktop \
        /usr/share/icons/hicolor/*/apps/git-leviathan.png
```

### macOS (Apple Silicon)

```bash
tar -xzf git-leviathan-<version>-aarch64-apple-darwin.tar.gz
mv "git-leviathan-<version>-aarch64-apple-darwin/Git Leviathan.app" /Applications/
```

The app is **not signed or notarized**. The first time you launch it, macOS will block it — right-click `Git Leviathan.app` → **Open**, then confirm. After that it launches normally.

### Windows

1. Download `git-leviathan-<version>-x86_64-pc-windows-msvc.zip`.
2. Extract it somewhere permanent (e.g. `C:\Program Files\Git Leviathan\`).
3. Run `git_leviathan.exe`. Optionally create a Start Menu shortcut.

SmartScreen may warn on first launch — click **More info** → **Run anyway**.

### Build from source

Requires a recent Rust toolchain. On Linux you also need `fontconfig` development headers.

```bash
# Linux prerequisites (Debian/Ubuntu)
sudo apt-get install libfontconfig1-dev

git clone https://github.com/shinyvision/git-leviathan.git
cd git-leviathan
cargo build --release
./target/release/git_leviathan
```

To build a distributable package yourself:

```bash
cargo install cargo-deb cargo-generate-rpm
cargo deb                 # -> target/debian/*.deb
cargo generate-rpm        # -> target/generate-rpm/*.rpm
```

## Tech Stack

| Area | Crate |
|------|-------|
| GUI | [`iced`](https://crates.io/crates/iced) 0.14 |
| Git | [`git2`](https://crates.io/crates/git2) 0.19 (vendored libgit2) |
| Async | [`tokio`](https://crates.io/crates/tokio) 1 |
| Persistence | [`rusqlite`](https://crates.io/crates/rusqlite) 0.32 (bundled) |
| Syntax highlighting | [`syntect`](https://crates.io/crates/syntect) + [`two-face`](https://crates.io/crates/two-face) |
| Filesystem watching | [`notify`](https://crates.io/crates/notify) 7 |
| Text shaping | [`swash`](https://crates.io/crates/swash) |

## License

Released under the [MIT License](LICENSE). © 2026 Rachel Snijders.
