use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use super::harness::MockHost;
use crate::plugin::terminal::{registry, TerminalId};

const MANIFEST_WITH_SPAWN: &str = r#"
id = "shellp"
name = "Shell"
version = "0.1.0"
api_version = "1.0"
capabilities = ["process:spawn"]
"#;

const MANIFEST_NO_SPAWN: &str = r#"
id = "shellp"
name = "Shell"
version = "0.1.0"
api_version = "1.0"
"#;

fn shell_api_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn drain_until<F: FnMut(&MockHost) -> bool>(host: &mut MockHost, mut cond: F) {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(10) {
        host.tick();
        if cond(host) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("condition did not become true");
}

fn terminal_text(id: u64) -> String {
    let snapshot = registry().snapshot(TerminalId::from(id));
    let mut rows = vec![String::new(); snapshot.rows as usize];
    for cell in snapshot.cells {
        if !cell.text.is_empty() {
            rows[cell.row as usize].push_str(&cell.text);
        }
    }
    rows.join("\n")
}

#[test]
fn shell_tables_expose_requested_host_fields() {
    let _guard = shell_api_test_lock().lock().unwrap();
    let mut host = MockHost::new();
    host.load_inline(
        "shellp",
        MANIFEST_NO_SPAWN,
        r#"
        _G.shell_name = leviathan.shell.name
        _G.shell_available = leviathan.shell.is_available and 1 or 0
        _G.bash_available_type = type(leviathan.bash.is_available)
        _G.zsh_available_type = type(leviathan.zsh.is_available)
        "#,
    )
    .expect("load");

    assert!(host.read_global_string("shellp", "shell_name").is_some());
    assert!(matches!(
        host.read_global_i64("shellp", "shell_available"),
        Some(0 | 1)
    ));
    assert_eq!(
        host.read_global_string("shellp", "bash_available_type")
            .as_deref(),
        Some("boolean")
    );
    assert_eq!(
        host.read_global_string("shellp", "zsh_available_type")
            .as_deref(),
        Some("boolean")
    );
}

#[test]
fn shell_run_completes_on_runtime_tick() {
    let _guard = shell_api_test_lock().lock().unwrap();
    let command = if cfg!(windows) {
        "echo shell-ok"
    } else {
        "printf shell-ok"
    };
    let init = format!(
        r#"
        _G.done = 0
        _G.output = ""
        local job, err = leviathan.shell.run({{
          command = {command:?},
          cwd = leviathan.repository.workdir_path ~= "" and leviathan.repository.workdir_path or nil,
          ansi = true,
        }}, function(ok, result)
          _G.done = 1
          if ok then
            _G.output = result.combined
            _G.exit_code = result.exit_code or -1
          else
            _G.output = result
          end
        end)
        _G.started = job and 1 or 0
        _G.start_err = err or ""
        "#,
    );

    let mut host = MockHost::new();
    host.load_inline("shellp", MANIFEST_WITH_SPAWN, &init)
        .expect("load");

    assert_eq!(host.read_global_i64("shellp", "started"), Some(1));
    assert_eq!(
        host.read_global_string("shellp", "start_err").as_deref(),
        Some("")
    );
    drain_until(&mut host, |h| {
        h.read_global_i64("shellp", "done") == Some(1)
    });
    assert_eq!(
        host.read_global_string("shellp", "output").as_deref(),
        Some("shell-ok")
    );
    assert_eq!(host.read_global_i64("shellp", "exit_code"), Some(0));
}

#[test]
fn shell_run_timeout_completes_without_blocking_runtime() {
    let _guard = shell_api_test_lock().lock().unwrap();
    let command = if cfg!(windows) {
        "ping -n 3 127.0.0.1 >NUL"
    } else {
        "sleep 2"
    };
    let init = format!(
        r#"
        _G.done = 0
        _G.status = ""
        local job, err = leviathan.shell.run({{
          command = {command:?},
          timeout_ms = 100,
        }}, function(ok, result)
          _G.done = 1
          if ok then
            _G.status = result.status
            _G.timed_out = result.timed_out and 1 or 0
          else
            _G.status = result
            _G.timed_out = 0
          end
        end)
        _G.started = job and 1 or 0
        _G.start_err = err or ""
        "#,
    );

    let mut host = MockHost::new();
    host.load_inline("shellp", MANIFEST_WITH_SPAWN, &init)
        .expect("load");

    assert_eq!(host.read_global_i64("shellp", "started"), Some(1));
    assert_eq!(
        host.read_global_string("shellp", "start_err").as_deref(),
        Some("")
    );
    host.tick();
    assert_eq!(host.read_global_i64("shellp", "done"), Some(0));
    drain_until(&mut host, |h| {
        h.read_global_i64("shellp", "done") == Some(1)
    });
    assert_eq!(
        host.read_global_string("shellp", "status").as_deref(),
        Some("timed_out")
    );
    assert_eq!(host.read_global_i64("shellp", "timed_out"), Some(1));
}

#[test]
fn shell_open_starts_pty_and_accepts_input() {
    let _guard = shell_api_test_lock().lock().unwrap();
    let command = if cfg!(windows) {
        "echo pty-ok"
    } else {
        "printf pty-ok"
    };
    let init = format!(
        r#"
        local id, err = leviathan.shell.open({{ rows = 8, cols = 40 }})
        _G.session = id or 0
        _G.err = err or ""
        if id then
          local ok, write_err = leviathan.shell.write(id, {input:?})
          _G.write_ok = ok and 1 or 0
          _G.write_err = write_err or ""
        else
          _G.write_ok = 0
          _G.write_err = ""
        end
        "#,
        input = format!("{command}\r"),
    );

    let mut host = MockHost::new();
    host.load_inline("shellp", MANIFEST_WITH_SPAWN, &init)
        .expect("load");

    let session = host
        .read_global_i64("shellp", "session")
        .unwrap_or_default();
    assert!(
        session > 0,
        "open failed: {:?}",
        host.read_global_string("shellp", "err")
    );
    assert_eq!(host.read_global_i64("shellp", "write_ok"), Some(1));
    assert_eq!(
        host.read_global_string("shellp", "write_err").as_deref(),
        Some("")
    );

    drain_until(&mut host, |_| {
        terminal_text(session as u64).contains("pty-ok")
    });
    assert!(registry().close(TerminalId::from(session as u64)));
}

#[test]
fn shell_open_reports_not_running_after_exit() {
    let _guard = shell_api_test_lock().lock().unwrap();
    let mut host = MockHost::new();
    host.load_inline(
        "shellp",
        MANIFEST_WITH_SPAWN,
        r#"
        local id, err = leviathan.shell.open({ rows = 8, cols = 40 })
        _G.session = id or 0
        _G.err = err or ""
        _G.running_initial = id and leviathan.shell.is_running(id) and 1 or 0
        if id then
          leviathan.shell.write(id, "exit\r")
          local function poll()
            _G.running_later = leviathan.shell.is_running(id) and 1 or 0
            if _G.running_later == 1 then
              leviathan.api.defer_fn(20, poll)
            end
          end
          leviathan.api.defer_fn(20, poll)
        end
        "#,
    )
    .expect("load");

    let session = host
        .read_global_i64("shellp", "session")
        .unwrap_or_default();
    assert!(
        session > 0,
        "open failed: {:?}",
        host.read_global_string("shellp", "err")
    );
    assert_eq!(host.read_global_i64("shellp", "running_initial"), Some(1));
    drain_until(&mut host, |h| {
        h.read_global_i64("shellp", "running_later") == Some(0)
    });
    assert!(!registry().is_running(TerminalId::from(session as u64)));
}

#[test]
fn shell_run_requires_process_spawn_capability() {
    let _guard = shell_api_test_lock().lock().unwrap();
    let mut host = MockHost::new();
    host.load_inline(
        "shellp",
        MANIFEST_NO_SPAWN,
        r#"
        local job, err = leviathan.shell.run("echo nope", function() end)
        _G.started = job and 1 or 0
        _G.err = err or ""
        "#,
    )
    .expect("load");

    assert_eq!(host.read_global_i64("shellp", "started"), Some(0));
    let err = host.read_global_string("shellp", "err").unwrap_or_default();
    assert!(err.contains("process:spawn"), "got: {err}");
}

#[test]
fn shell_open_requires_process_spawn_capability() {
    let _guard = shell_api_test_lock().lock().unwrap();
    let mut host = MockHost::new();
    host.load_inline(
        "shellp",
        MANIFEST_NO_SPAWN,
        r#"
        local id, err = leviathan.shell.open({ rows = 8, cols = 40 })
        _G.started = id and 1 or 0
        _G.err = err or ""
        "#,
    )
    .expect("load");

    assert_eq!(host.read_global_i64("shellp", "started"), Some(0));
    let err = host.read_global_string("shellp", "err").unwrap_or_default();
    assert!(err.contains("process:spawn"), "got: {err}");
}
