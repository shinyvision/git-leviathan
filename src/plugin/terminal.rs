use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

use crate::plugin::resources::{GenerationId, PluginId};

const DEFAULT_ROWS: u16 = 24;
const DEFAULT_COLS: u16 = 80;
const SCROLLBACK_ROWS: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TerminalId(u64);

impl TerminalId {
    pub fn get(self) -> u64 {
        self.0
    }
}

impl From<u64> for TerminalId {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

#[derive(Clone, Default)]
pub struct TerminalRegistry {
    inner: Arc<Mutex<TerminalRegistryInner>>,
}

#[derive(Default)]
struct TerminalRegistryInner {
    next_id: u64,
    sessions: HashMap<TerminalId, TerminalSession>,
}

struct TerminalSession {
    plugin_id: PluginId,
    generation_id: GenerationId,
    name: String,
    parser: vt100::Parser,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    rx: Receiver<Vec<u8>>,
    _reader: JoinHandle<()>,
    size: PtySize,
    exited: bool,
}

#[derive(Clone)]
pub struct TerminalOpenSpec {
    pub plugin_id: PluginId,
    pub generation_id: GenerationId,
    pub name: String,
    pub executable: PathBuf,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub rows: Option<u16>,
    pub cols: Option<u16>,
}

#[derive(Clone, Debug, Default)]
pub struct TerminalSnapshot {
    pub id: Option<TerminalId>,
    pub name: String,
    pub rows: u16,
    pub cols: u16,
    pub cursor_row: u16,
    pub cursor_col: u16,
    pub cursor_visible: bool,
    pub cells: Vec<TerminalCell>,
    pub missing: bool,
    pub exited: bool,
}

#[derive(Clone, Debug)]
pub struct TerminalCell {
    pub row: u16,
    pub col: u16,
    pub text: String,
    pub fg: vt100::Color,
    pub bg: vt100::Color,
    pub bold: bool,
    pub inverse: bool,
}

pub fn registry() -> &'static TerminalRegistry {
    static REGISTRY: OnceLock<TerminalRegistry> = OnceLock::new();
    REGISTRY.get_or_init(TerminalRegistry::default)
}

impl TerminalRegistry {
    pub fn open(&self, spec: TerminalOpenSpec) -> Result<TerminalId, String> {
        let rows = spec.rows.unwrap_or(DEFAULT_ROWS).clamp(2, 200);
        let cols = spec.cols.unwrap_or(DEFAULT_COLS).clamp(2, 500);
        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(size).map_err(|e| e.to_string())?;
        let mut command = CommandBuilder::new(&spec.executable);
        if let Some(cwd) = &spec.cwd {
            command.cwd(cwd);
        }
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("CLICOLOR", "1");
        command.env("FORCE_COLOR", "1");
        for (key, value) in &spec.env {
            command.env(key, value);
        }
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|e| format!("failed to spawn {}: {e}", spec.name))?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = Arc::new(Mutex::new(
            pair.master.take_writer().map_err(|e| e.to_string())?,
        ));
        let (tx, rx) = channel();
        let reader_thread = std::thread::spawn(move || {
            let mut buf = [0_u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        let mut inner = self.inner.lock().expect("terminal registry poisoned");
        inner.next_id += 1;
        let id = TerminalId(inner.next_id);
        inner.sessions.insert(
            id,
            TerminalSession {
                plugin_id: spec.plugin_id,
                generation_id: spec.generation_id,
                name: spec.name,
                parser: vt100::Parser::new(rows, cols, SCROLLBACK_ROWS),
                master: pair.master,
                child,
                writer,
                rx,
                _reader: reader_thread,
                size,
                exited: false,
            },
        );
        Ok(id)
    }

    pub fn write(&self, id: TerminalId, bytes: &[u8]) -> Result<(), String> {
        let writer = {
            let inner = self.inner.lock().expect("terminal registry poisoned");
            inner
                .sessions
                .get(&id)
                .map(|session| Arc::clone(&session.writer))
                .ok_or_else(|| format!("terminal session {} not found", id.get()))?
        };
        let result = writer
            .lock()
            .expect("terminal writer poisoned")
            .write_all(bytes)
            .map_err(|e| e.to_string());
        result
    }

    pub fn owns(&self, id: TerminalId, plugin_id: &PluginId, generation_id: GenerationId) -> bool {
        self.inner
            .lock()
            .expect("terminal registry poisoned")
            .sessions
            .get(&id)
            .is_some_and(|session| {
                &session.plugin_id == plugin_id && session.generation_id == generation_id
            })
    }

    pub fn resize(
        &self,
        id: TerminalId,
        rows: u16,
        cols: u16,
        pixel_width: u16,
        pixel_height: u16,
    ) -> Result<(), String> {
        let mut inner = self.inner.lock().expect("terminal registry poisoned");
        let session = inner
            .sessions
            .get_mut(&id)
            .ok_or_else(|| format!("terminal session {} not found", id.get()))?;
        let rows = rows.clamp(2, 200);
        let cols = cols.clamp(2, 500);
        if session.size.rows == rows
            && session.size.cols == cols
            && session.size.pixel_width == pixel_width
            && session.size.pixel_height == pixel_height
        {
            return Ok(());
        }
        let size = PtySize {
            rows,
            cols,
            pixel_width,
            pixel_height,
        };
        session.master.resize(size).map_err(|e| e.to_string())?;
        session.parser.set_size(rows, cols);
        session.size = size;
        Ok(())
    }

    pub fn close(&self, id: TerminalId) -> bool {
        let session = self
            .inner
            .lock()
            .expect("terminal registry poisoned")
            .sessions
            .remove(&id);
        if let Some(mut session) = session {
            let _ = session.child.kill();
            true
        } else {
            false
        }
    }

    pub fn close_for_generation(&self, plugin_id: &PluginId, generation_id: GenerationId) {
        let ids: Vec<TerminalId> = {
            let inner = self.inner.lock().expect("terminal registry poisoned");
            inner
                .sessions
                .iter()
                .filter(|(_, session)| {
                    &session.plugin_id == plugin_id && session.generation_id == generation_id
                })
                .map(|(id, _)| *id)
                .collect()
        };
        for id in ids {
            self.close(id);
        }
    }

    pub fn has_sessions(&self) -> bool {
        !self
            .inner
            .lock()
            .expect("terminal registry poisoned")
            .sessions
            .is_empty()
    }

    pub fn is_running(&self, id: TerminalId) -> bool {
        self.inner
            .lock()
            .expect("terminal registry poisoned")
            .sessions
            .get(&id)
            .is_some_and(|session| !session.exited)
    }

    pub fn drain(&self) -> Vec<String> {
        let mut affected = Vec::new();
        let mut exited = Vec::new();
        let mut inner = self.inner.lock().expect("terminal registry poisoned");
        for (id, session) in inner.sessions.iter_mut() {
            let mut changed = false;
            while let Ok(bytes) = session.rx.try_recv() {
                session.parser.process(&bytes);
                changed = true;
            }
            if !session.exited {
                if let Ok(Some(_)) = session.child.try_wait() {
                    session.exited = true;
                    changed = true;
                    exited.push(*id);
                }
            }
            if changed {
                affected.push(session.plugin_id.as_str().to_string());
            }
        }
        for id in exited {
            inner.sessions.remove(&id);
        }
        affected.sort();
        affected.dedup();
        affected
    }

    pub fn snapshot(&self, id: TerminalId) -> TerminalSnapshot {
        let inner = self.inner.lock().expect("terminal registry poisoned");
        let Some(session) = inner.sessions.get(&id) else {
            return TerminalSnapshot {
                id: Some(id),
                missing: true,
                ..TerminalSnapshot::default()
            };
        };
        let screen = session.parser.screen();
        let (rows, cols) = screen.size();
        let (cursor_row, cursor_col) = screen.cursor_position();
        let mut cells = Vec::with_capacity(rows as usize * cols as usize);
        for row in 0..rows {
            for col in 0..cols {
                if let Some(cell) = screen.cell(row, col) {
                    if cell.is_wide_continuation() {
                        continue;
                    }
                    cells.push(TerminalCell {
                        row,
                        col,
                        text: cell.contents(),
                        fg: cell.fgcolor(),
                        bg: cell.bgcolor(),
                        bold: cell.bold(),
                        inverse: cell.inverse(),
                    });
                }
            }
        }
        TerminalSnapshot {
            id: Some(id),
            name: session.name.clone(),
            rows,
            cols,
            cursor_row,
            cursor_col,
            cursor_visible: !screen.hide_cursor(),
            cells,
            missing: false,
            exited: session.exited,
        }
    }

    pub fn application_cursor(&self, id: TerminalId) -> bool {
        self.inner
            .lock()
            .expect("terminal registry poisoned")
            .sessions
            .get(&id)
            .is_some_and(|session| session.parser.screen().application_cursor())
    }
}
