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

### From source

```bash
git clone https://github.com/shinyvision/git-leviathan.git
cd git_leviathan
cargo build --release
./target/release/git_leviathan
```

### Debian / Ubuntu

```bash
cargo install cargo-deb
cargo deb
sudo dpkg -i target/debian/git-leviathan_*.deb
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
