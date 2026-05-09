use std::cell::RefCell;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc::channel;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use mlua::{Function, Lua, Table, Value as LuaValue};
use serde_json::json;

use crate::plugin::api::async_api::{JobCallbacks, LuaJobHandle};
use crate::plugin::async_jobs::{
    AsyncJobRegistration, AsyncJobRegistry, CancellationToken, JobOutcome,
};
use crate::plugin::capabilities::CapabilityGuard;
use crate::plugin::resources::{GenerationId, PluginId, PluginResourceKind, ResourceLedger};
use crate::utils::configure_background_command;

const DEFAULT_OUTPUT_LIMIT: usize = 2 * 1024 * 1024;
const MAX_OUTPUT_LIMIT: usize = 16 * 1024 * 1024;

pub struct ShellInstallContext {
    pub guard: Rc<CapabilityGuard>,
    pub ledger: ResourceLedger,
    pub registry: AsyncJobRegistry,
    pub job_callbacks: Rc<RefCell<JobCallbacks>>,
    pub plugin_id: PluginId,
    pub generation_id: GenerationId,
}

#[derive(Clone)]
struct ShellTarget {
    name: String,
    executable: Option<PathBuf>,
    kind: ShellKind,
}

#[derive(Clone, Copy)]
enum ShellKind {
    PosixLogin,
    Command,
    Cmd,
    PowerShell,
}

#[derive(Debug)]
struct ShellRunSpec {
    command: String,
    cwd: Option<PathBuf>,
    env: Vec<(String, String)>,
    input: Option<String>,
    timeout_ms: Option<u64>,
    max_output_bytes: usize,
    ansi: bool,
}

#[derive(Debug)]
struct ShellOpenSpec {
    cwd: Option<PathBuf>,
    env: Vec<(String, String)>,
    rows: Option<u16>,
    cols: Option<u16>,
}

pub fn install(lua: &Lua, leviathan: &Table, ctx: ShellInstallContext) -> mlua::Result<()> {
    let ctx = Rc::new(ctx);
    install_shell_table(
        lua,
        leviathan,
        "shell",
        default_shell_target(),
        Rc::clone(&ctx),
    )?;
    install_shell_table(
        lua,
        leviathan,
        "bash",
        named_shell_target("bash"),
        Rc::clone(&ctx),
    )?;
    install_shell_table(lua, leviathan, "zsh", named_shell_target("zsh"), ctx)?;
    Ok(())
}

fn install_shell_table(
    lua: &Lua,
    leviathan: &Table,
    table_name: &'static str,
    target: ShellTarget,
    ctx: Rc<ShellInstallContext>,
) -> mlua::Result<()> {
    let table = lua.create_table()?;
    table.set("name", target.name.clone())?;
    table.set("is_available", target.executable.is_some())?;

    let target_for_run = target.clone();
    let ctx_for_run = Rc::clone(&ctx);
    table.set(
        "run",
        lua.create_function(
            move |lua_inner, (value, on_complete): (LuaValue, Option<Function>)| {
                run_shell(
                    lua_inner,
                    table_name,
                    &target_for_run,
                    Rc::clone(&ctx_for_run),
                    value,
                    on_complete,
                )
            },
        )?,
    )?;
    let target_for_open = target.clone();
    let ctx_for_open = Rc::clone(&ctx);
    table.set(
        "open",
        lua.create_function(move |lua_inner, value: Option<LuaValue>| {
            open_terminal(
                lua_inner,
                table_name,
                &target_for_open,
                Rc::clone(&ctx_for_open),
                value.unwrap_or(LuaValue::Nil),
            )
        })?,
    )?;

    let ctx_for_write = Rc::clone(&ctx);
    table.set(
        "write",
        lua.create_function(move |_, (id, data): (u64, String)| {
            terminal_write(Rc::clone(&ctx_for_write), id, data)
        })?,
    )?;

    let ctx_for_resize = Rc::clone(&ctx);
    table.set(
        "resize",
        lua.create_function(move |_, (id, cols, rows): (u64, u16, u16)| {
            terminal_resize(Rc::clone(&ctx_for_resize), id, cols, rows)
        })?,
    )?;

    let ctx_for_close = Rc::clone(&ctx);
    table.set(
        "close",
        lua.create_function(move |_, id: u64| terminal_close(Rc::clone(&ctx_for_close), id))?,
    )?;

    let ctx_for_is_running = Rc::clone(&ctx);
    table.set(
        "is_running",
        lua.create_function(move |_, id: u64| {
            terminal_is_running(Rc::clone(&ctx_for_is_running), id)
        })?,
    )?;

    leviathan.set(table_name, table)?;
    Ok(())
}

fn run_shell(
    lua: &Lua,
    table_name: &'static str,
    target: &ShellTarget,
    ctx: Rc<ShellInstallContext>,
    value: LuaValue,
    on_complete: Option<Function>,
) -> mlua::Result<(Option<LuaJobHandle>, Option<String>)> {
    let Some(executable) = target.executable.clone() else {
        return Ok((None, Some(format!("{} is not available", target.name))));
    };
    let spec = match parse_run_spec(value) {
        Ok(spec) => spec,
        Err(err) => return Ok((None, Some(err))),
    };

    let source = ResourceLedger::source_location(lua);
    if let Err(err) = check_process_capability(
        &ctx.guard,
        &executable,
        &spec.command,
        table_name,
        "run",
        source.as_deref(),
    ) {
        return Ok((None, Some(err)));
    }

    let cancel = CancellationToken::new();
    let job_id = ctx.registry.allocate_job_id();
    let resource_id = ctx.ledger.record(
        PluginResourceKind::AsyncJob,
        format!("shell.run:{}:{}", table_name, job_id.get()),
        source.clone(),
    );

    if let Some(callback) = on_complete {
        let key = lua.create_registry_value(callback)?;
        ctx.job_callbacks.borrow_mut().insert(job_id, key);
        ctx.ledger.record(
            PluginResourceKind::LuaRegistryKey,
            format!("shell.run:{}:{}:on_complete", table_name, job_id.get()),
            source,
        );
    }

    let target_for_worker = target.clone();
    let (tx, rx) = channel::<JobOutcome>();
    let cancel_for_worker = cancel.clone();
    let join = std::thread::spawn(move || {
        let outcome = run_command(target_for_worker, executable, spec, cancel_for_worker);
        let _ = tx.send(outcome);
    });

    ctx.registry.register(AsyncJobRegistration {
        plugin_id: ctx.plugin_id.clone(),
        generation_id: ctx.generation_id,
        job_id,
        resource_id,
        cancel,
        join,
        rx,
    });

    Ok((
        Some(LuaJobHandle {
            job_id,
            registry: ctx.registry.clone(),
        }),
        None,
    ))
}

fn open_terminal(
    lua: &Lua,
    table_name: &'static str,
    target: &ShellTarget,
    ctx: Rc<ShellInstallContext>,
    value: LuaValue,
) -> mlua::Result<(Option<u64>, Option<String>)> {
    let Some(executable) = target.executable.clone() else {
        return Ok((None, Some(format!("{} is not available", target.name))));
    };
    let spec = match parse_open_spec(value) {
        Ok(spec) => spec,
        Err(err) => return Ok((None, Some(err))),
    };
    let source = ResourceLedger::source_location(lua);
    if let Err(err) = check_process_capability(
        &ctx.guard,
        &executable,
        "interactive shell",
        table_name,
        "open",
        source.as_deref(),
    ) {
        return Ok((None, Some(err)));
    }
    let id =
        match crate::plugin::terminal::registry().open(crate::plugin::terminal::TerminalOpenSpec {
            plugin_id: ctx.plugin_id.clone(),
            generation_id: ctx.generation_id,
            name: target.name.clone(),
            executable,
            cwd: spec.cwd,
            env: spec.env,
            rows: spec.rows,
            cols: spec.cols,
        }) {
            Ok(id) => id.get(),
            Err(err) => return Ok((None, Some(err))),
        };
    Ok((Some(id), None))
}

fn terminal_write(
    ctx: Rc<ShellInstallContext>,
    id: u64,
    data: String,
) -> mlua::Result<(bool, Option<String>)> {
    let id = crate::plugin::terminal::TerminalId::from(id);
    if !crate::plugin::terminal::registry().owns(id, &ctx.plugin_id, ctx.generation_id) {
        return Ok((
            false,
            Some("terminal session not owned by plugin".to_string()),
        ));
    }
    match crate::plugin::terminal::registry().write(id, data.as_bytes()) {
        Ok(()) => Ok((true, None)),
        Err(err) => Ok((false, Some(err))),
    }
}

fn terminal_resize(
    ctx: Rc<ShellInstallContext>,
    id: u64,
    cols: u16,
    rows: u16,
) -> mlua::Result<(bool, Option<String>)> {
    let id = crate::plugin::terminal::TerminalId::from(id);
    if !crate::plugin::terminal::registry().owns(id, &ctx.plugin_id, ctx.generation_id) {
        return Ok((
            false,
            Some("terminal session not owned by plugin".to_string()),
        ));
    }
    match crate::plugin::terminal::registry().resize(id, rows, cols, 0, 0) {
        Ok(()) => Ok((true, None)),
        Err(err) => Ok((false, Some(err))),
    }
}

fn terminal_close(ctx: Rc<ShellInstallContext>, id: u64) -> mlua::Result<bool> {
    let id = crate::plugin::terminal::TerminalId::from(id);
    if !crate::plugin::terminal::registry().owns(id, &ctx.plugin_id, ctx.generation_id) {
        return Ok(false);
    }
    Ok(crate::plugin::terminal::registry().close(id))
}

fn terminal_is_running(ctx: Rc<ShellInstallContext>, id: u64) -> mlua::Result<bool> {
    let id = crate::plugin::terminal::TerminalId::from(id);
    if !crate::plugin::terminal::registry().owns(id, &ctx.plugin_id, ctx.generation_id) {
        return Ok(false);
    }
    Ok(crate::plugin::terminal::registry().is_running(id))
}

fn parse_run_spec(value: LuaValue) -> Result<ShellRunSpec, String> {
    match value {
        LuaValue::String(s) => Ok(ShellRunSpec {
            command: s.to_str().map_err(|e| e.to_string())?.to_string(),
            cwd: None,
            env: Vec::new(),
            input: None,
            timeout_ms: None,
            max_output_bytes: DEFAULT_OUTPUT_LIMIT,
            ansi: true,
        }),
        LuaValue::Table(t) => {
            let command: String = t
                .get("command")
                .map_err(|_| "shell.run spec requires command".to_string())?;
            let cwd = t
                .get::<Option<String>>("cwd")
                .map_err(|e| e.to_string())?
                .filter(|s| !s.is_empty())
                .map(PathBuf::from);
            let input = t
                .get::<Option<String>>("input")
                .map_err(|e| e.to_string())?;
            let timeout_ms = t
                .get::<Option<u64>>("timeout_ms")
                .map_err(|e| e.to_string())?
                .filter(|ms| *ms > 0);
            let max_output_bytes = t
                .get::<Option<u64>>("max_output_bytes")
                .map_err(|e| e.to_string())?
                .map(|n| n as usize)
                .unwrap_or(DEFAULT_OUTPUT_LIMIT)
                .clamp(1024, MAX_OUTPUT_LIMIT);
            let ansi = t
                .get::<Option<bool>>("ansi")
                .map_err(|e| e.to_string())?
                .unwrap_or(true);
            let mut env = Vec::new();
            if let Some(env_table) = t.get::<Option<Table>>("env").map_err(|e| e.to_string())? {
                for pair in env_table.pairs::<String, String>() {
                    env.push(pair.map_err(|e| e.to_string())?);
                }
            }
            Ok(ShellRunSpec {
                command,
                cwd,
                env,
                input,
                timeout_ms,
                max_output_bytes,
                ansi,
            })
        }
        _ => Err("shell.run expects a command string or spec table".to_string()),
    }
    .and_then(|spec| {
        if spec.command.trim().is_empty() {
            Err("shell.run command must not be empty".to_string())
        } else {
            Ok(spec)
        }
    })
}

fn parse_open_spec(value: LuaValue) -> Result<ShellOpenSpec, String> {
    match value {
        LuaValue::Nil => Ok(ShellOpenSpec {
            cwd: None,
            env: Vec::new(),
            rows: None,
            cols: None,
        }),
        LuaValue::Table(t) => {
            let cwd = t
                .get::<Option<String>>("cwd")
                .map_err(|e| e.to_string())?
                .filter(|s| !s.is_empty())
                .map(PathBuf::from);
            let rows = t
                .get::<Option<u16>>("rows")
                .map_err(|e| e.to_string())?
                .filter(|n| *n > 0);
            let cols = t
                .get::<Option<u16>>("cols")
                .map_err(|e| e.to_string())?
                .filter(|n| *n > 0);
            let mut env = Vec::new();
            if let Some(env_table) = t.get::<Option<Table>>("env").map_err(|e| e.to_string())? {
                for pair in env_table.pairs::<String, String>() {
                    env.push(pair.map_err(|e| e.to_string())?);
                }
            }
            Ok(ShellOpenSpec {
                cwd,
                env,
                rows,
                cols,
            })
        }
        _ => Err("shell.open expects nil or a spec table".to_string()),
    }
}

fn check_process_capability(
    guard: &CapabilityGuard,
    executable: &Path,
    command: &str,
    table_name: &str,
    function_name: &str,
    source_location: Option<&str>,
) -> Result<(), String> {
    let mut caps = vec!["process:spawn".to_string()];
    if let Some(name) = executable.file_name().and_then(|n| n.to_str()) {
        caps.push(format!("process:spawn:{name}"));
        if let Some(stem) = Path::new(name).file_stem().and_then(|n| n.to_str()) {
            if stem != name {
                caps.push(format!("process:spawn:{stem}"));
            }
        }
    }
    guard.check_any_named_for_target(
        &caps,
        command,
        &format!("leviathan.{table_name}.{function_name}"),
        source_location,
        "Declare and grant `process:spawn` or the matching `process:spawn:<shell>` capability.",
    )
}

fn run_command(
    target: ShellTarget,
    executable: PathBuf,
    spec: ShellRunSpec,
    cancel: CancellationToken,
) -> JobOutcome {
    let mut command = Command::new(&executable);
    configure_background_command(&mut command);
    command.args(shell_args(target.kind, &spec.command));
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    if spec.input.is_some() {
        command.stdin(Stdio::piped());
    }
    if let Some(cwd) = &spec.cwd {
        command.current_dir(cwd);
    }
    if spec.ansi {
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("CLICOLOR", "1");
        command.env("CLICOLOR_FORCE", "1");
        command.env("FORCE_COLOR", "1");
    }
    for (key, value) in &spec.env {
        command.env(key, value);
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => return JobOutcome::Failed(format!("failed to spawn {}: {e}", target.name)),
    };

    if let Some(input) = spec.input {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(input.as_bytes());
        }
    }

    let stdout_reader = child
        .stdout
        .take()
        .map(|out| read_limited(out, spec.max_output_bytes));
    let stderr_reader = child
        .stderr
        .take()
        .map(|err| read_limited(err, spec.max_output_bytes));
    let started = Instant::now();
    let mut timed_out = false;

    let status = loop {
        if cancel.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            join_reader(stdout_reader);
            join_reader(stderr_reader);
            return JobOutcome::Cancelled;
        }
        if spec
            .timeout_ms
            .is_some_and(|ms| started.elapsed() >= Duration::from_millis(ms))
        {
            timed_out = true;
            let _ = child.kill();
            break child.wait().ok();
        }
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => {
                return JobOutcome::Failed(format!("failed to wait for {}: {e}", target.name))
            }
        }
    };

    let stdout = bytes_to_string(join_reader(stdout_reader));
    let stderr = bytes_to_string(join_reader(stderr_reader));
    let exit_code = status.as_ref().and_then(|s| s.code());
    let success = status.as_ref().is_some_and(|s| s.success()) && !timed_out;
    let status_text = if timed_out {
        "timed_out"
    } else if success {
        "success"
    } else {
        "failed"
    };
    let combined = if stderr.is_empty() {
        stdout.clone()
    } else if stdout.is_empty() {
        stderr.clone()
    } else {
        format!("{stdout}{stderr}")
    };

    JobOutcome::Ok(json!({
        "shell": target.name,
        "command": spec.command,
        "cwd": spec.cwd.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
        "success": success,
        "status": status_text,
        "exit_code": exit_code,
        "stdout": stdout,
        "stderr": stderr,
        "combined": combined,
        "timed_out": timed_out,
    }))
}

fn shell_args(kind: ShellKind, command: &str) -> Vec<String> {
    match kind {
        ShellKind::PosixLogin => vec!["-lc".to_string(), command.to_string()],
        ShellKind::Command => vec!["-c".to_string(), command.to_string()],
        ShellKind::Cmd => vec!["/D".to_string(), "/C".to_string(), command.to_string()],
        ShellKind::PowerShell => vec![
            "-NoLogo".to_string(),
            "-NoProfile".to_string(),
            "-Command".to_string(),
            command.to_string(),
        ],
    }
}

fn read_limited<R: Read + Send + 'static>(mut reader: R, max_bytes: usize) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let mut buf = [0_u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let remaining = max_bytes.saturating_sub(out.len());
                    if remaining > 0 {
                        out.extend_from_slice(&buf[..n.min(remaining)]);
                    }
                }
                Err(_) => break,
            }
        }
        out
    })
}

fn join_reader(reader: Option<JoinHandle<Vec<u8>>>) -> Vec<u8> {
    reader
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default()
}

fn bytes_to_string(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(&bytes).into_owned()
}

fn default_shell_target() -> ShellTarget {
    let mut candidates = Vec::new();
    #[cfg(windows)]
    {
        if let Ok(shell) = std::env::var("COMSPEC") {
            candidates.push(shell);
        }
        candidates.extend(["pwsh".into(), "powershell".into(), "cmd.exe".into()]);
    }
    #[cfg(not(windows))]
    {
        if let Ok(shell) = std::env::var("SHELL") {
            candidates.push(shell);
        }
        candidates.extend(["zsh".into(), "bash".into(), "sh".into()]);
    }

    for candidate in candidates {
        if let Some(path) = resolve_executable(&candidate) {
            let name = shell_name(&path).unwrap_or(candidate);
            return ShellTarget {
                kind: shell_kind(&name),
                name,
                executable: Some(path),
            };
        }
    }

    ShellTarget {
        name: "shell".to_string(),
        executable: None,
        kind: ShellKind::Command,
    }
}

fn named_shell_target(name: &str) -> ShellTarget {
    let executable = resolve_executable(name);
    ShellTarget {
        name: name.to_string(),
        executable,
        kind: ShellKind::PosixLogin,
    }
}

fn resolve_executable(raw: &str) -> Option<PathBuf> {
    if raw.is_empty() {
        return None;
    }
    let path = Path::new(raw);
    if path.components().count() > 1 {
        return path.is_file().then(|| path.to_path_buf());
    }
    find_in_path(raw)
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let has_ext = Path::new(name).extension().is_some();
            if !has_ext {
                for ext in std::env::var_os("PATHEXT")
                    .and_then(|v| v.into_string().ok())
                    .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_string())
                    .split(';')
                {
                    let candidate = dir.join(format!("{name}{ext}"));
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
        }
    }
    None
}

fn shell_name(path: &Path) -> Option<String> {
    path.file_name().map(|n| n.to_string_lossy().into_owned())
}

fn shell_kind(name: &str) -> ShellKind {
    let lower = name.to_ascii_lowercase();
    if lower == "cmd" || lower == "cmd.exe" {
        ShellKind::Cmd
    } else if lower == "pwsh"
        || lower == "pwsh.exe"
        || lower == "powershell"
        || lower == "powershell.exe"
    {
        ShellKind::PowerShell
    } else if matches!(
        lower.as_str(),
        "sh" | "sh.exe"
            | "bash"
            | "bash.exe"
            | "zsh"
            | "zsh.exe"
            | "dash"
            | "dash.exe"
            | "ksh"
            | "ksh.exe"
    ) {
        ShellKind::PosixLogin
    } else {
        ShellKind::Command
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_string_spec() {
        let lua = Lua::new();
        let spec = parse_run_spec(LuaValue::String(lua.create_string("echo hi").unwrap())).unwrap();
        assert_eq!(spec.command, "echo hi");
        assert!(spec.ansi);
    }

    #[test]
    fn rejects_empty_command() {
        let lua = Lua::new();
        let err = parse_run_spec(LuaValue::String(lua.create_string(" ").unwrap())).unwrap_err();
        assert!(err.contains("must not be empty"));
    }
}
