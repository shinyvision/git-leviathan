//! Context-aware keymap parsing, resolution, and dispatch.

use std::collections::HashMap;

use crate::plugin::commands::{
    dispatch_command_as, CommandDispatchEnv, CommandInvocationCaller, InvokeOutcome,
};
use crate::plugin::diagnostic::{
    DiagnosticSeverity, DiagnosticStore, PluginDiagnostic, PluginSourceSpan,
};
use crate::plugin::events::EventPayload;
use crate::plugin::resources::{GenerationId, PluginId};

/// Sentinel plugin id used by the host when registering built-in
/// keymaps and for diagnostics emitted by the resolver itself. Same
/// convention as registered commands.
pub const HOST_KEYMAP_PLUGIN_ID: &str = "<host>";

/// User-config "plugin id". User-keymap rows are stored under this
/// sentinel so they sort and resolve as a distinct tier.
pub const USER_KEYMAP_PLUGIN_ID: &str = "<user>";

/// `plugin_screen:<id>` and `overlay:<id>` are handled at use time —
/// any context string starting with `plugin_screen:` or `overlay:`
/// matches the intended scope.
pub const KNOWN_CONTEXTS: &[&str] = &[
    "global",
    "repository",
    "repository.sidebar",
    "repository.graph",
    "repository.details",
    "repository.details.dirty",
    "repository.diff",
    "tab_bar",
];

/// A single key press with optional modifiers. A *chord* is a
/// non-empty sequence of these, evaluated left-to-right.
///
/// The `key` field is the lower-cased base key. For an ASCII letter
/// `a..=z` it's the letter itself; for a named key it's one of the
/// canonical names emitted by [`parse_key_sequence`] (`enter`, `esc`,
/// `tab`, `space`, `backspace`, `up`, `down`, `left`, `right`,
/// `f1`..`f12`).
///
/// Shift on a printable ASCII letter is *normalised away* — `<S-a>`
/// and a literal upper-case `A` both round-trip as
/// `Keystroke { key: "a", shift: true, ctrl: false, alt: false }`.
/// This keeps user / plugin lookups equivalent to what the input
/// layer will produce from a real key event.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Keystroke {
    pub key: String,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Keystroke {
    pub fn plain(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            ctrl: false,
            shift: false,
            alt: false,
        }
    }

    pub fn with_ctrl(mut self, ctrl: bool) -> Self {
        self.ctrl = ctrl;
        self
    }

    pub fn with_shift(mut self, shift: bool) -> Self {
        self.shift = shift;
        self
    }

    pub fn with_alt(mut self, alt: bool) -> Self {
        self.alt = alt;
        self
    }

    /// Render back into vim-style notation. Used by the devtools
    /// snapshot so the `key` column displays the original lhs the
    /// plugin author wrote rather than the internal struct.
    pub fn render(&self) -> String {
        let mut mods: Vec<&str> = Vec::new();
        if self.ctrl {
            mods.push("C");
        }
        if self.shift {
            mods.push("S");
        }
        if self.alt {
            mods.push("A");
        }
        if mods.is_empty() {
            // Plain printable single-char keys render as the key
            // itself; named keys still get the angle-bracket form so
            // they're unambiguous.
            if self.key.len() == 1 {
                self.key.clone()
            } else {
                format!("<{}>", self.key)
            }
        } else {
            format!("<{}-{}>", mods.join("-"), self.key)
        }
    }
}

/// Render an entire chord. Single-keystroke chords render exactly
/// one [`Keystroke`]; multi-key chords concatenate their renderings.
pub fn render_chord(chord: &[Keystroke]) -> String {
    chord.iter().map(Keystroke::render).collect()
}

/// Parse a vim-style lhs into a sequence of [`Keystroke`]s.
///
/// Recognised forms:
///
/// - bare ASCII letters/digits/punctuation: each character is one
///   [`Keystroke`] (so `gl` is two keystrokes);
/// - `<leader>` expands using the supplied `leader` string (parsed
///   recursively, so `<leader>` can be e.g. `,` or even `<Space>`);
/// - `<C-x>`, `<S-x>`, `<A-x>`, and combinations `<C-S-x>` etc.;
/// - named keys: `<CR>`, `<Enter>`, `<Esc>`, `<Tab>`, `<Space>`,
///   `<BS>`, `<Up>`, `<Down>`, `<Left>`, `<Right>`, `<F1>`..`<F12>`.
///
/// Returns `Err(reason)` for malformed input. The reason string is
/// surfaced verbatim in the `keymap.invalid_key` diagnostic.
pub fn parse_key_sequence(lhs: &str, leader: &str) -> Result<Vec<Keystroke>, String> {
    if lhs.is_empty() {
        return Err("empty key sequence".into());
    }
    let mut out: Vec<Keystroke> = Vec::new();
    let bytes = lhs.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c == '<' {
            // Find the matching '>'.
            let Some(end_rel) = lhs[i + 1..].find('>') else {
                return Err(format!("unterminated `<` token at position {i} in `{lhs}`"));
            };
            let end = i + 1 + end_rel;
            let token = &lhs[i + 1..end];
            if token.is_empty() {
                return Err(format!("empty `<>` token at position {i} in `{lhs}`"));
            }
            if token.eq_ignore_ascii_case("leader") {
                if leader.is_empty() {
                    return Err("leader is empty; set the leader before binding `<leader>`".into());
                }
                let mut expanded = parse_key_sequence(leader, "")?;
                out.append(&mut expanded);
                i = end + 1;
                continue;
            }
            // Modifier-prefixed forms: `C-x`, `S-Tab`, `C-S-x`.
            // Walk dash-separated parts; the *last* part is the base
            // key; anything before is a modifier.
            let parts: Vec<&str> = token.split('-').collect();
            let (mods, base) = if parts.len() == 1 {
                (Vec::new(), parts[0])
            } else {
                let (mods_slice, base_slice) = parts.split_at(parts.len() - 1);
                (mods_slice.to_vec(), base_slice[0])
            };
            let mut ctrl = false;
            let mut shift = false;
            let mut alt = false;
            for m in &mods {
                match m.to_ascii_uppercase().as_str() {
                    "C" | "CTRL" => ctrl = true,
                    "S" | "SHIFT" => shift = true,
                    "A" | "ALT" | "M" | "META" => alt = true,
                    other => {
                        return Err(format!(
                            "unknown modifier `{other}` in `<{token}>` (expected C/S/A)"
                        ));
                    }
                }
            }
            let key = canonicalise_named_key(base)
                .ok_or_else(|| format!("unknown named key `{base}` in `<{token}>`"))?;
            // For modifier-less single-char letters, fold case into the
            // shift modifier so `<S-a>` and `A` resolve identically.
            let mut k = Keystroke {
                key,
                ctrl,
                shift,
                alt,
            };
            normalise_letter_case(&mut k);
            out.push(k);
            i = end + 1;
            continue;
        }
        // Bare character. Treat ASCII letters/digits/printable punct
        // as a one-keystroke chord step. Reject control bytes and
        // multi-byte UTF-8 to keep the matcher predictable.
        if !c.is_ascii() {
            return Err(format!(
                "non-ASCII character `{c}` in `{lhs}` is not supported in keymaps"
            ));
        }
        if c.is_ascii_control() {
            return Err(format!(
                "control byte at position {i} in `{lhs}` is not a valid key"
            ));
        }
        let mut k = Keystroke::plain(c.to_string());
        normalise_letter_case(&mut k);
        out.push(k);
        i += 1;
    }
    Ok(out)
}

/// Map a base-key token (the part after the modifiers in `<C-Tab>`,
/// the bare `Tab` in `<Tab>`, or a single ASCII letter) to its
/// canonical lower-case key string.
fn canonicalise_named_key(token: &str) -> Option<String> {
    if token.len() == 1 && token.is_ascii() {
        return Some(token.to_ascii_lowercase());
    }
    let lower = token.to_ascii_lowercase();
    match lower.as_str() {
        "cr" | "enter" | "return" => Some("enter".into()),
        "esc" | "escape" => Some("esc".into()),
        "tab" => Some("tab".into()),
        "space" => Some("space".into()),
        "bs" | "backspace" => Some("backspace".into()),
        "del" | "delete" => Some("delete".into()),
        "up" => Some("up".into()),
        "down" => Some("down".into()),
        "left" => Some("left".into()),
        "right" => Some("right".into()),
        "home" => Some("home".into()),
        "end" => Some("end".into()),
        "pageup" | "pgup" => Some("pageup".into()),
        "pagedown" | "pgdn" => Some("pagedown".into()),
        f if f.starts_with('f') && f[1..].chars().all(|c| c.is_ascii_digit()) => {
            // F1..F24 — accept up to two digits.
            let digits = &f[1..];
            let n: u32 = digits.parse().ok()?;
            if (1..=24).contains(&n) {
                Some(format!("f{n}"))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn normalise_letter_case(k: &mut Keystroke) {
    if k.key.len() == 1 {
        let c = k.key.as_bytes()[0] as char;
        if c.is_ascii_uppercase() {
            k.shift = true;
            k.key = c.to_ascii_lowercase().to_string();
        }
    }
}

/// Where a binding came from. Drives conflict resolution: built-in
/// beats user beats plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeymapSource {
    /// Built-in app binding registered by the host.
    BuiltIn,
    /// User-config binding loaded from
    /// `~/.config/git_leviathan/keymaps.toml` (or installed
    /// programmatically through [`KeymapRegistry::set_user_keymap`]).
    User,
    /// Plugin binding registered through the Lua API.
    Plugin,
}

impl KeymapSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BuiltIn => "built-in",
            Self::User => "user",
            Self::Plugin => "plugin",
        }
    }

    /// Tier rank used for conflict resolution. Lower beats higher.
    pub fn tier(self) -> u8 {
        match self {
            Self::BuiltIn => 0,
            Self::User => 1,
            Self::Plugin => 2,
        }
    }
}

/// keymaps status flag carried on each [`KeymapEntry`] after the
/// resolver runs. `Active` means the binding is the winner for its
/// `(context, chord)` slot; `ConflictLost` means another tier or a
/// lex-earlier plugin won.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeymapStatus {
    Active,
    ConflictLost,
}

impl KeymapStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::ConflictLost => "conflict_lost",
        }
    }
}

/// One keymap row. The host stores these directly in the registry; on
/// every mutation [`KeymapRegistry::resolve`] re-walks the table and
/// stamps each entry's `status` field plus its `conflict_with`
/// pointer.
pub struct KeymapEntry {
    pub source: KeymapSource,
    pub plugin_id: String,
    pub generation_id: Option<GenerationId>,
    pub context: String,
    pub chord: Vec<Keystroke>,
    pub command: String,
    pub args: serde_json::Value,
    pub description: String,
    /// Plugin-local declaration sequence number. Same plugin's
    /// `set("ctx", "x", ...)` calls preserve registration order in
    /// the resolver's lex tie-break.
    pub sequence: u64,
    /// Stamped by the resolver on every mutation.
    pub status: KeymapStatus,
    /// When `status == ConflictLost`, the `(plugin_id, source)` of
    /// the winner. `None` for active entries.
    pub conflict_with: Option<ConflictInfo>,
    pub source_location: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ConflictInfo {
    pub winner_plugin_id: String,
    pub winner_source: KeymapSource,
}

/// Result of [`chord_match`]. Used by the in-flight chord buffer the
/// app holds (or any test driver replaying key events) to decide
/// whether to dispatch, keep buffering, or reset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchOutcome {
    /// No active binding has the supplied chord as a prefix. The
    /// buffer should be reset and the keystrokes returned to the
    /// underlying app for normal handling.
    None,
    /// At least one active binding has the chord as a prefix but
    /// none matches it exactly yet. The buffer should be kept.
    Pending,
    /// The chord matched an active binding exactly. The caller
    /// should dispatch and reset the buffer. `command` and `args`
    /// are cloned out to avoid holding the registry borrow across
    /// dispatch.
    Match {
        command: String,
        args: serde_json::Value,
        context: String,
        plugin_id: String,
    },
}

/// One visible child in a which-key style prefix menu. The keymap
/// registry produces these so the UI does not have to duplicate
/// context matching, conflict filtering, or chord rendering rules.
#[derive(Debug, Clone)]
pub struct KeymapPrefixHint {
    /// The next keypress after the current prefix.
    pub key: String,
    /// The full chord when the next key completes a command. Empty for
    /// pure intermediate groups.
    pub full_key: String,
    /// Human-facing description supplied by the keymap owner. Pure
    /// intermediate groups receive a generated count label.
    pub description: String,
    pub command: String,
    pub plugin_id: String,
    pub source: String,
    /// Number of active descendants below this next key.
    pub child_count: usize,
    /// True when this next key enters another submenu.
    pub is_group: bool,
}

/// Plugin-side capture of `leviathan.keymap.set` calls during init.
/// Drained by the host into permanent registry entries after
/// init.lua finishes — same shape as autocmds and command registry
/// commands.
pub struct RawKeymap {
    pub context: String,
    pub key: String,
    pub command: String,
    pub args: serde_json::Value,
    pub description: String,
    pub sequence: u64,
    pub source_location: Option<String>,
}

/// `leviathan.keymap.del` row. Applied after staging completes,
/// against the staged generation's bindings only — the host does
/// *not* let a plugin delete another plugin's keymaps.
pub struct RawKeymapDel {
    pub context: String,
    pub key: String,
    pub sequence: u64,
}

/// Single source of truth for every binding the host knows about.
/// Cheap-clonable handles are not exposed; the host owns the only
/// instance and serves dispatch + introspection on its behalf.
#[derive(Default)]
pub struct KeymapRegistry {
    entries: Vec<KeymapEntry>,
    /// Active leader string. Defaults to `\` to match the most
    /// common Neovim default; tests pin it to `,` for predictable
    /// `<leader>gh` behaviour. capability settings will let users pick this in
    /// their config.
    leader: String,
}

impl KeymapRegistry {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            leader: "\\".to_string(),
        }
    }

    pub fn entries(&self) -> &[KeymapEntry] {
        &self.entries
    }

    pub fn leader(&self) -> &str {
        &self.leader
    }

    pub fn set_leader(&mut self, leader: impl Into<String>) {
        self.leader = leader.into();
        self.resolve();
    }

    /// Insert a built-in (host-owned) binding. Always tier 0 — wins
    /// every conflict against a user / plugin binding. Used by the
    /// app at startup to seed bindings like `Ctrl+Tab`.
    pub fn set_builtin(
        &mut self,
        context: impl Into<String>,
        key: &str,
        command: impl Into<String>,
        args: serde_json::Value,
        description: impl Into<String>,
        diagnostics: &DiagnosticStore,
    ) {
        let context = context.into();
        let command = command.into();
        let description = description.into();
        let chord = match parse_key_sequence(key, &self.leader) {
            Ok(c) => c,
            Err(e) => {
                diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(HOST_KEYMAP_PLUGIN_ID),
                        DiagnosticSeverity::Warning,
                        "keymap.invalid_key",
                        format!("built-in keymap rejected: {e}"),
                    )
                    .with_source(PluginSourceSpan::ApiFunction {
                        name: format!("keymap:{context}:{key}"),
                    }),
                );
                return;
            }
        };
        let pre_status = self.snapshot_status();
        self.upsert(KeymapEntry {
            source: KeymapSource::BuiltIn,
            plugin_id: HOST_KEYMAP_PLUGIN_ID.into(),
            generation_id: None,
            context: context.clone(),
            chord: chord.clone(),
            command,
            args,
            description,
            sequence: 0,
            status: KeymapStatus::Active,
            conflict_with: None,
            source_location: None,
        });
        self.resolve();
        // Locate the just-inserted entry by its (source, context, chord) identity.
        let idx = self
            .entries
            .iter()
            .position(|e| {
                e.source == KeymapSource::BuiltIn && e.context == context && e.chord == chord
            })
            .expect("just inserted");
        log_registered(diagnostics, &self.entries[idx]);
        self.emit_loser_transitions(diagnostics, &pre_status);
    }

    /// Insert (or replace) a user-config binding. Tier 1 — beats
    /// every plugin binding for the same `(context, chord)`. Used by
    /// the capability-grant user-config loader and by tests.
    pub fn set_user_keymap(
        &mut self,
        context: impl Into<String>,
        key: &str,
        command: impl Into<String>,
        args: serde_json::Value,
        description: impl Into<String>,
        diagnostics: &DiagnosticStore,
    ) {
        let context = context.into();
        let command = command.into();
        let description = description.into();
        let chord = match parse_key_sequence(key, &self.leader) {
            Ok(c) => c,
            Err(e) => {
                diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(USER_KEYMAP_PLUGIN_ID),
                        DiagnosticSeverity::Warning,
                        "keymap.invalid_key",
                        format!("user keymap rejected: {e}"),
                    )
                    .with_source(PluginSourceSpan::ApiFunction {
                        name: format!("keymap:{context}:{key}"),
                    }),
                );
                return;
            }
        };
        let pre_status = self.snapshot_status();
        self.upsert(KeymapEntry {
            source: KeymapSource::User,
            plugin_id: USER_KEYMAP_PLUGIN_ID.into(),
            generation_id: None,
            context: context.clone(),
            chord: chord.clone(),
            command,
            args,
            description,
            sequence: 0,
            status: KeymapStatus::Active,
            conflict_with: None,
            source_location: None,
        });
        self.resolve();
        let idx = self
            .entries
            .iter()
            .position(|e| {
                e.source == KeymapSource::User && e.context == context && e.chord == chord
            })
            .expect("just inserted");
        log_registered(diagnostics, &self.entries[idx]);
        self.emit_loser_transitions(diagnostics, &pre_status);
    }

    /// Remove a user binding. No-op if no row matches.
    pub fn del_user_keymap(&mut self, context: &str, key: &str) -> bool {
        let chord = match parse_key_sequence(key, &self.leader) {
            Ok(c) => c,
            Err(_) => return false,
        };
        let removed = self.entries.iter().position(|e| {
            e.source == KeymapSource::User && e.context == context && e.chord == chord
        });
        if let Some(i) = removed {
            self.entries.remove(i);
            self.resolve();
            true
        } else {
            false
        }
    }

    /// Insert plugin bindings drained from a plugin's [`RawKeymap`]
    /// list. Bad rows (`parse_key_sequence` rejected the lhs) emit
    /// `keymap.invalid_key` diagnostics and are dropped — registration
    /// must not block plugin load.
    ///
    /// Resolver-status diagnostics fire for both the *new* rows and
    /// any *existing* row whose `(context, chord)` was disturbed by
    /// the insert: a previously-active row that just became
    /// `conflict_lost` (because the new row wins) gets its own
    /// `keymap.conflict_lost` diagnostic so devtools shows a
    /// chronological trail.
    pub fn install_plugin_raws(
        &mut self,
        plugin_id: &str,
        generation_id: GenerationId,
        raws: Vec<RawKeymap>,
        diagnostics: &DiagnosticStore,
    ) {
        // Snapshot every entry's pre-resolve status so we can detect
        // transitions caused by the new rows.
        let pre_status = self.snapshot_status();
        for raw in raws {
            let chord = match parse_key_sequence(&raw.key, &self.leader) {
                Ok(c) => c,
                Err(e) => {
                    let mut diag = PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Warning,
                        "keymap.invalid_key",
                        format!(
                            "keymap `{}` for context `{}` rejected: {e}",
                            raw.key, raw.context
                        ),
                    )
                    .with_generation(generation_id)
                    .with_source(PluginSourceSpan::ApiFunction {
                        name: format!("keymap:{}:{}", raw.context, raw.key),
                    })
                    .with_context(serde_json::json!({
                        "context": raw.context,
                        "key": raw.key,
                        "command": raw.command,
                    }));
                    if let Some(loc) = raw.source_location.as_ref() {
                        diag = diag.with_context(serde_json::json!({ "source": loc }));
                    }
                    diagnostics.record(diag);
                    continue;
                }
            };
            self.upsert(KeymapEntry {
                source: KeymapSource::Plugin,
                plugin_id: plugin_id.to_string(),
                generation_id: Some(generation_id),
                context: raw.context,
                chord,
                command: raw.command,
                args: raw.args,
                description: raw.description,
                sequence: raw.sequence,
                status: KeymapStatus::Active,
                conflict_with: None,
                source_location: raw.source_location,
            });
        }
        self.resolve();
        // Emit `keymap.registered` (and possibly `keymap.conflict_lost`)
        // for the new rows.
        for entry in self
            .entries
            .iter()
            .filter(|e| e.plugin_id == plugin_id && e.generation_id == Some(generation_id))
        {
            log_registered(diagnostics, entry);
        }
        self.emit_loser_transitions(diagnostics, &pre_status);
    }

    fn snapshot_status(
        &self,
    ) -> HashMap<(String, Vec<Keystroke>, KeymapSource, String), KeymapStatus> {
        self.entries
            .iter()
            .map(|e| {
                (
                    (
                        e.context.clone(),
                        e.chord.clone(),
                        e.source,
                        e.plugin_id.clone(),
                    ),
                    e.status,
                )
            })
            .collect()
    }

    fn emit_loser_transitions(
        &self,
        diagnostics: &DiagnosticStore,
        pre_status: &HashMap<(String, Vec<Keystroke>, KeymapSource, String), KeymapStatus>,
    ) {
        for entry in self.entries.iter() {
            let key = (
                entry.context.clone(),
                entry.chord.clone(),
                entry.source,
                entry.plugin_id.clone(),
            );
            let Some(pre) = pre_status.get(&key).copied() else {
                continue;
            };
            if pre == KeymapStatus::Active && entry.status == KeymapStatus::ConflictLost {
                log_conflict_lost(diagnostics, entry);
            }
        }
    }

    /// Apply staged [`RawKeymapDel`] rows. Only matches keymaps owned
    /// by `(plugin_id, generation_id)` — a plugin can't delete a
    /// peer's binding.
    pub fn apply_plugin_dels(
        &mut self,
        plugin_id: &str,
        generation_id: GenerationId,
        dels: Vec<RawKeymapDel>,
        diagnostics: &DiagnosticStore,
    ) {
        if dels.is_empty() {
            return;
        }
        for del in dels {
            let chord = match parse_key_sequence(&del.key, &self.leader) {
                Ok(c) => c,
                Err(e) => {
                    diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id),
                            DiagnosticSeverity::Warning,
                            "keymap.invalid_key",
                            format!(
                                "keymap.del `{}` for context `{}` rejected: {e}",
                                del.key, del.context
                            ),
                        )
                        .with_generation(generation_id)
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: format!("keymap.del:{}:{}", del.context, del.key),
                        })
                        .with_context(serde_json::json!({
                            "context": del.context,
                            "key": del.key,
                        })),
                    );
                    continue;
                }
            };
            self.entries.retain(|e| {
                !(e.source == KeymapSource::Plugin
                    && e.plugin_id == plugin_id
                    && e.generation_id == Some(generation_id)
                    && e.context == del.context
                    && e.chord == chord)
            });
        }
        self.resolve();
    }

    /// Drop every binding owned by `plugin_id` (across all
    /// generations — defensive, mirrors the command path).
    /// Used by `unload_plugin`.
    pub fn drop_for_plugin(&mut self, plugin_id: &str) {
        self.entries
            .retain(|e| !(e.source == KeymapSource::Plugin && e.plugin_id == plugin_id));
        self.resolve();
    }

    /// Drop every binding owned by `(plugin_id, generation_id)`. Used
    /// during reload-commit before the new generation's keymaps are
    /// spliced in.
    pub fn drop_for_generation(&mut self, plugin_id: &str, generation_id: GenerationId) {
        self.entries.retain(|e| {
            !(e.source == KeymapSource::Plugin
                && e.plugin_id == plugin_id
                && e.generation_id == Some(generation_id))
        });
        self.resolve();
    }

    /// Run the chord matcher against the supplied buffer in the
    /// supplied context. Returns the match outcome; the caller is
    /// responsible for routing a [`MatchOutcome::Match`] through
    /// [`dispatch_command`].
    pub fn match_chord(&self, context: &str, buffer: &[Keystroke]) -> MatchOutcome {
        if buffer.is_empty() {
            return MatchOutcome::None;
        }
        // Look only at active rows in this context (or `global`).
        let candidates: Vec<&KeymapEntry> = self
            .entries
            .iter()
            .filter(|e| e.status == KeymapStatus::Active)
            .filter(|e| context_matches(&e.context, context))
            .collect();
        let mut exact: Option<&KeymapEntry> = None;
        let mut has_prefix = false;
        for e in &candidates {
            if e.chord.as_slice() == buffer {
                // If multiple contexts match (e.g. global + repository
                // both have the same chord), prefer the most specific
                // (the longer / non-global context) — this is
                // deterministic given the resolver already disabled
                // tied losers.
                exact = Some(match exact {
                    None => *e,
                    Some(prev) => specifier_wins(prev, e),
                });
            } else if e.chord.len() > buffer.len() && &e.chord[..buffer.len()] == buffer {
                has_prefix = true;
            }
        }
        if has_prefix {
            return MatchOutcome::Pending;
        }
        if let Some(e) = exact {
            return MatchOutcome::Match {
                command: e.command.clone(),
                args: e.args.clone(),
                context: e.context.clone(),
                plugin_id: e.plugin_id.clone(),
            };
        }
        MatchOutcome::None
    }

    /// Return the visible next-step hints for a partially-entered
    /// chord in `context`. Only active keymaps are considered, and
    /// the same `global` / hierarchical context matching used by
    /// dispatch is applied.
    pub fn prefix_hints(&self, context: &str, prefix: &[Keystroke]) -> Vec<KeymapPrefixHint> {
        if prefix.is_empty() {
            return Vec::new();
        }

        struct Group {
            next: Keystroke,
            exact_idx: Option<usize>,
            child_count: usize,
        }

        let mut groups: Vec<Group> = Vec::new();
        for (idx, entry) in self.entries.iter().enumerate() {
            if entry.status != KeymapStatus::Active
                || !context_matches(&entry.context, context)
                || entry.chord.len() <= prefix.len()
                || &entry.chord[..prefix.len()] != prefix
            {
                continue;
            }

            let next = entry.chord[prefix.len()].clone();
            let group_idx = match groups.iter().position(|group| group.next == next) {
                Some(group_idx) => group_idx,
                None => {
                    groups.push(Group {
                        next,
                        exact_idx: None,
                        child_count: 0,
                    });
                    groups.len() - 1
                }
            };

            if entry.chord.len() == prefix.len() + 1 {
                let exact_idx = groups[group_idx]
                    .exact_idx
                    .map(|prev_idx| {
                        if specifier_left_wins(&self.entries[prev_idx], entry) {
                            prev_idx
                        } else {
                            idx
                        }
                    })
                    .unwrap_or(idx);
                groups[group_idx].exact_idx = Some(exact_idx);
            } else {
                groups[group_idx].child_count += 1;
            }
        }

        let mut hints: Vec<KeymapPrefixHint> = groups
            .into_iter()
            .map(|group| {
                let exact = group.exact_idx.map(|idx| &self.entries[idx]);
                let description =
                    exact
                        .map(|entry| entry.description.clone())
                        .unwrap_or_else(|| match group.child_count {
                            1 => "+1 keymap".to_string(),
                            n => format!("+{n} keymaps"),
                        });
                KeymapPrefixHint {
                    key: group.next.render(),
                    full_key: exact
                        .map(|entry| render_chord(&entry.chord))
                        .unwrap_or_default(),
                    description,
                    command: exact.map(|entry| entry.command.clone()).unwrap_or_default(),
                    plugin_id: exact
                        .map(|entry| entry.plugin_id.clone())
                        .unwrap_or_default(),
                    source: exact
                        .map(|entry| entry.source.as_str().to_string())
                        .unwrap_or_default(),
                    child_count: group.child_count,
                    is_group: group.child_count > 0,
                }
            })
            .collect();
        hints.sort_by(|a, b| {
            a.key
                .cmp(&b.key)
                .then(a.description.cmp(&b.description))
                .then(a.command.cmp(&b.command))
        });
        hints
    }

    /// Public dispatch. Walks [`Self::match_chord`] and, on a match,
    /// routes through the command registry [`dispatch_command`] funnel.
    /// Returns the dispatch outcome plus the matched keymap so the
    /// caller can fire the typed `KeymapTriggered` event.
    pub fn dispatch(
        &self,
        context: &str,
        buffer: &[Keystroke],
        env: &CommandDispatchEnv,
        diagnostics: &DiagnosticStore,
    ) -> KeymapDispatchOutcome {
        match self.match_chord(context, buffer) {
            MatchOutcome::None => KeymapDispatchOutcome::Unhandled,
            MatchOutcome::Pending => KeymapDispatchOutcome::Pending,
            MatchOutcome::Match {
                command,
                args,
                context: ctx,
                plugin_id,
            } => {
                let caller = match plugin_id.as_str() {
                    HOST_KEYMAP_PLUGIN_ID | USER_KEYMAP_PLUGIN_ID => None,
                    _ => Some(CommandInvocationCaller {
                        plugin_id: plugin_id.clone(),
                        capability_guard: None,
                    }),
                };
                let outcome = dispatch_command_as(env, caller, &command, args.clone());
                let key_label = render_chord(buffer);
                diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id.as_str()),
                        DiagnosticSeverity::Info,
                        "keymap.dispatched",
                        format!(
                            "keymap `{key_label}` in `{ctx}` -> `{command}` ({})",
                            outcome_label(&outcome)
                        ),
                    )
                    .with_source(PluginSourceSpan::ApiFunction {
                        name: format!("keymap:{ctx}:{key_label}"),
                    })
                    .with_context(serde_json::json!({
                        "context": ctx,
                        "key": key_label,
                        "command": command,
                        "plugin_id": plugin_id,
                        "outcome": outcome_label(&outcome),
                    })),
                );
                KeymapDispatchOutcome::Dispatched {
                    context: ctx,
                    key: key_label,
                    command,
                    plugin_id,
                    outcome,
                }
            }
        }
    }

    /// Project every entry into a stable summary the devtools view
    /// (and `leviathan.keymap.list`) consume. Sorted by
    /// `(context, key, source-tier, plugin_id)` so external tools see
    /// a stable order.
    pub fn summaries(&self) -> Vec<KeymapSummary> {
        let mut out: Vec<KeymapSummary> = self
            .entries
            .iter()
            .map(|e| KeymapSummary {
                context: e.context.clone(),
                key: render_chord(&e.chord),
                command: e.command.clone(),
                plugin_id: e.plugin_id.clone(),
                generation_id: e.generation_id.map(|g| g.get()),
                source: e.source.as_str().to_string(),
                status: e.status.as_str().to_string(),
                description: e.description.clone(),
                conflict_with: e.conflict_with.as_ref().map(|c| ConflictRef {
                    plugin_id: c.winner_plugin_id.clone(),
                    source: c.winner_source.as_str().to_string(),
                }),
            })
            .collect();
        out.sort_by(|a, b| {
            a.context
                .cmp(&b.context)
                .then(a.key.cmp(&b.key))
                .then(a.source.cmp(&b.source))
                .then(a.plugin_id.cmp(&b.plugin_id))
        });
        out
    }

    fn upsert(&mut self, entry: KeymapEntry) {
        // Same-source same-key replacement keeps the registry small
        // and matches the command "later wins within a
        // (plugin_id, name) slot" rule.
        if let Some(i) = self.entries.iter().position(|e| {
            e.source == entry.source
                && e.plugin_id == entry.plugin_id
                && e.generation_id == entry.generation_id
                && e.context == entry.context
                && e.chord == entry.chord
        }) {
            self.entries[i] = entry;
        } else {
            self.entries.push(entry);
        }
    }

    /// Fully re-stamp every entry's `status` and `conflict_with`.
    /// Cheap (we only have a handful of bindings); called on every
    /// mutation so reads can rely on the stamps being current.
    fn resolve(&mut self) {
        // Group entries by `(context, chord)` and pick the winner per
        // group. The chord match path also has to deal with `global`
        // overlaps (a `global` chord competes against a context
        // chord), so the resolver records `conflict_lost` for cross-
        // context losers too — but only when the *exact* (context,
        // chord) collides.
        let mut groups: HashMap<(String, Vec<Keystroke>), Vec<usize>> = HashMap::new();
        for (i, e) in self.entries.iter().enumerate() {
            groups
                .entry((e.context.clone(), e.chord.clone()))
                .or_default()
                .push(i);
        }
        // Reset.
        for e in self.entries.iter_mut() {
            e.status = KeymapStatus::Active;
            e.conflict_with = None;
        }
        for indices in groups.values() {
            if indices.len() <= 1 {
                continue;
            }
            // Pick winner deterministically:
            //   tier (built-in < user < plugin) -> plugin_id lex ->
            //   sequence asc.
            let mut sorted = indices.clone();
            sorted.sort_by(|&a, &b| {
                let ea = &self.entries[a];
                let eb = &self.entries[b];
                ea.source
                    .tier()
                    .cmp(&eb.source.tier())
                    .then(ea.plugin_id.cmp(&eb.plugin_id))
                    .then(ea.sequence.cmp(&eb.sequence))
            });
            let winner_idx = sorted[0];
            let winner_plugin = self.entries[winner_idx].plugin_id.clone();
            let winner_source = self.entries[winner_idx].source;
            for &i in &sorted[1..] {
                self.entries[i].status = KeymapStatus::ConflictLost;
                self.entries[i].conflict_with = Some(ConflictInfo {
                    winner_plugin_id: winner_plugin.clone(),
                    winner_source,
                });
            }
        }
    }
}

/// Stable-shape projection consumed by `InspectorSnapshot.keymaps`
/// and by `leviathan.keymap.list()`.
#[derive(Debug, Clone)]
pub struct KeymapSummary {
    pub context: String,
    pub key: String,
    pub command: String,
    pub plugin_id: String,
    pub generation_id: Option<u64>,
    pub source: String,
    pub status: String,
    pub description: String,
    pub conflict_with: Option<ConflictRef>,
}

#[derive(Debug, Clone)]
pub struct ConflictRef {
    pub plugin_id: String,
    pub source: String,
}

/// What [`KeymapRegistry::dispatch`] reports back.
#[derive(Debug, Clone)]
pub enum KeymapDispatchOutcome {
    /// No active binding has the chord even as a prefix in this
    /// context. The app should handle the keystrokes through its
    /// existing logic.
    Unhandled,
    /// One or more active bindings have the chord as a prefix but
    /// none matches yet. The caller should keep buffering.
    Pending,
    /// A binding matched and the command dispatcher returned
    /// `outcome`. The host fires `KeymapTriggered` either way; the
    /// caller does not need to.
    Dispatched {
        context: String,
        key: String,
        command: String,
        plugin_id: String,
        outcome: InvokeOutcome,
    },
}

impl KeymapDispatchOutcome {
    pub fn is_handled(&self) -> bool {
        !matches!(self, Self::Unhandled)
    }
}

/// Build the typed `KeymapTriggered` payload the host fires after a
/// successful dispatch. Kept here so the field set lives next to the
/// descriptor's payload schema.
pub fn keymap_triggered_payload(
    context: &str,
    key: &str,
    command: &str,
    plugin_id: &str,
    ok: bool,
) -> EventPayload {
    let mut p = EventPayload::new();
    p.insert("context".into(), serde_json::Value::String(context.into()));
    p.insert("key".into(), serde_json::Value::String(key.into()));
    p.insert("command".into(), serde_json::Value::String(command.into()));
    p.insert(
        "plugin_id".into(),
        serde_json::Value::String(plugin_id.into()),
    );
    p.insert("ok".into(), serde_json::Value::Bool(ok));
    p
}

/// Build the typed `KeymapPrefixChanged` payload. The host owns chord
/// parsing and conflict filtering; plugins own how the resulting hint
/// data is presented.
pub fn keymap_prefix_changed_payload(
    active: bool,
    context: &str,
    prefix: &str,
    hints: Vec<KeymapPrefixHint>,
    reason: &str,
) -> EventPayload {
    let mut p = EventPayload::new();
    p.insert("active".into(), serde_json::Value::Bool(active));
    p.insert("context".into(), serde_json::Value::String(context.into()));
    p.insert("prefix".into(), serde_json::Value::String(prefix.into()));
    p.insert("reason".into(), serde_json::Value::String(reason.into()));
    p.insert(
        "hints".into(),
        serde_json::Value::Array(
            hints
                .into_iter()
                .map(|hint| {
                    serde_json::json!({
                        "key": hint.key,
                        "full_key": hint.full_key,
                        "description": hint.description,
                        "command": hint.command,
                        "plugin_id": hint.plugin_id,
                        "source": hint.source,
                        "child_count": hint.child_count,
                        "is_group": hint.is_group,
                    })
                })
                .collect(),
        ),
    );
    p
}

fn outcome_label(out: &InvokeOutcome) -> &'static str {
    match out {
        InvokeOutcome::Ok => "ok",
        InvokeOutcome::InvalidArgs(_) => "invalid_args",
        InvokeOutcome::CapabilityDenied(_) => "capability_denied",
        InvokeOutcome::ContextDenied(_) => "context_denied",
        InvokeOutcome::NotFound => "not_found",
        InvokeOutcome::Failed(_) => "failed",
    }
}

fn log_registered(diagnostics: &DiagnosticStore, entry: &KeymapEntry) {
    let key_label = render_chord(&entry.chord);
    let mut diag = PluginDiagnostic::new(
        PluginId::from(entry.plugin_id.as_str()),
        DiagnosticSeverity::Info,
        "keymap.registered",
        format!(
            "{} keymap `{key_label}` in `{}` -> `{}`",
            entry.source.as_str(),
            entry.context,
            entry.command,
        ),
    )
    .with_source(PluginSourceSpan::ApiFunction {
        name: format!("keymap:{}:{}", entry.context, key_label),
    })
    .with_context(serde_json::json!({
        "context": entry.context,
        "key": key_label,
        "command": entry.command,
        "source": entry.source.as_str(),
    }));
    if let Some(gid) = entry.generation_id {
        diag = diag.with_generation(gid);
    }
    diagnostics.record(diag);
    if entry.status == KeymapStatus::ConflictLost {
        log_conflict_lost(diagnostics, entry);
    }
}

fn log_conflict_lost(diagnostics: &DiagnosticStore, entry: &KeymapEntry) {
    let Some(conflict) = entry.conflict_with.as_ref() else {
        return;
    };
    let key_label = render_chord(&entry.chord);
    let mut lost = PluginDiagnostic::new(
        PluginId::from(entry.plugin_id.as_str()),
        DiagnosticSeverity::Warning,
        "keymap.conflict_lost",
        format!(
            "keymap `{key_label}` in `{}` lost conflict to {} `{}`",
            entry.context,
            conflict.winner_source.as_str(),
            conflict.winner_plugin_id,
        ),
    )
    .with_source(PluginSourceSpan::ApiFunction {
        name: format!("keymap:{}:{}", entry.context, key_label),
    })
    .with_context(serde_json::json!({
        "context": entry.context,
        "key": key_label,
        "winner_plugin_id": conflict.winner_plugin_id,
        "winner_source": conflict.winner_source.as_str(),
    }));
    if let Some(gid) = entry.generation_id {
        lost = lost.with_generation(gid);
    }
    diagnostics.record(lost);
}

/// `binding_context` matches `active_context` when they're equal or
/// when the binding is in `global`. Future-proof: a context like
/// `repository.diff` should also be served by a `repository` binding,
/// but keymaps keeps the rule narrow — the plan calls for the
/// concrete contexts exactly. The dispatcher's caller is expected to
/// pass the most specific active context for the keystroke.
fn context_matches(binding_context: &str, active_context: &str) -> bool {
    if binding_context == active_context {
        return true;
    }
    if binding_context == "global" {
        return true;
    }
    // Hierarchical match: `repository` binding fires under
    // `repository.diff`, etc. Keeps the contract simple — a binding's
    // context is a prefix of the active context.
    if let Some(rest) = active_context.strip_prefix(binding_context) {
        if rest.starts_with('.') {
            return true;
        }
    }
    false
}

/// When two active candidates have the *same* chord but different
/// contexts (one `global`, one `repository`), prefer the more
/// specific (longer) context. Ties on length break by lex.
fn specifier_wins<'a>(a: &'a KeymapEntry, b: &'a KeymapEntry) -> &'a KeymapEntry {
    if specifier_left_wins(a, b) {
        a
    } else {
        b
    }
}

fn specifier_left_wins(a: &KeymapEntry, b: &KeymapEntry) -> bool {
    if a.context == b.context {
        return true;
    }
    let a_specific = a.context != "global";
    let b_specific = b.context != "global";
    match (a_specific, b_specific) {
        (true, false) => true,
        (false, true) => false,
        _ => {
            if a.context.len() > b.context.len() {
                true
            } else if b.context.len() > a.context.len() {
                false
            } else {
                a.context <= b.context
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::plugin::diagnostic::NullSink;

    fn diag_store() -> DiagnosticStore {
        DiagnosticStore::with_sink(Arc::new(NullSink))
    }

    fn ks(s: &str) -> Vec<Keystroke> {
        parse_key_sequence(s, "\\").unwrap()
    }

    #[test]
    fn parses_single_letter() {
        let v = ks("a");
        assert_eq!(v, vec![Keystroke::plain("a")]);
    }

    #[test]
    fn parses_chord_two_letters() {
        let v = ks("gl");
        assert_eq!(v, vec![Keystroke::plain("g"), Keystroke::plain("l")]);
    }

    #[test]
    fn parses_named_keys() {
        assert_eq!(ks("<CR>"), vec![Keystroke::plain("enter")]);
        assert_eq!(ks("<Esc>"), vec![Keystroke::plain("esc")]);
        assert_eq!(ks("<Tab>"), vec![Keystroke::plain("tab")]);
        assert_eq!(ks("<Space>"), vec![Keystroke::plain("space")]);
    }

    #[test]
    fn parses_modifiers() {
        let v = ks("<C-r>");
        assert_eq!(v, vec![Keystroke::plain("r").with_ctrl(true)]);
        let v = ks("<C-S-x>");
        assert_eq!(
            v,
            vec![Keystroke::plain("x").with_ctrl(true).with_shift(true)]
        );
        let v = ks("<S-Tab>");
        assert_eq!(v, vec![Keystroke::plain("tab").with_shift(true)]);
    }

    #[test]
    fn upper_case_letter_folds_to_shift() {
        let v = ks("A");
        assert_eq!(v, vec![Keystroke::plain("a").with_shift(true)]);
    }

    #[test]
    fn parses_leader_expansion() {
        let v = parse_key_sequence("<leader>gh", ",").unwrap();
        assert_eq!(
            v,
            vec![
                Keystroke::plain(","),
                Keystroke::plain("g"),
                Keystroke::plain("h"),
            ]
        );
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_key_sequence("<C-", "\\").is_err());
        assert!(parse_key_sequence("<>", "\\").is_err());
        assert!(parse_key_sequence("<C-zz>", "\\").is_err());
        assert!(parse_key_sequence("", "\\").is_err());
    }

    #[test]
    fn registry_resolves_plugin_vs_plugin_by_lex() {
        let mut reg = KeymapRegistry::new();
        let store = diag_store();
        reg.install_plugin_raws(
            "zzz",
            GenerationId::new(1),
            vec![RawKeymap {
                context: "repository".into(),
                key: "gl".into(),
                command: "zzz.go".into(),
                args: serde_json::Value::Null,
                description: "z".into(),
                sequence: 1,
                source_location: None,
            }],
            &store,
        );
        reg.install_plugin_raws(
            "aaa",
            GenerationId::new(1),
            vec![RawKeymap {
                context: "repository".into(),
                key: "gl".into(),
                command: "aaa.go".into(),
                args: serde_json::Value::Null,
                description: "a".into(),
                sequence: 1,
                source_location: None,
            }],
            &store,
        );
        let summaries = reg.summaries();
        let aaa = summaries.iter().find(|s| s.plugin_id == "aaa").unwrap();
        let zzz = summaries.iter().find(|s| s.plugin_id == "zzz").unwrap();
        assert_eq!(aaa.status, "active");
        assert_eq!(zzz.status, "conflict_lost");
        assert_eq!(zzz.conflict_with.as_ref().unwrap().plugin_id, "aaa");
    }

    #[test]
    fn registry_resolves_built_in_over_plugin() {
        let mut reg = KeymapRegistry::new();
        let store = diag_store();
        reg.install_plugin_raws(
            "p",
            GenerationId::new(1),
            vec![RawKeymap {
                context: "global".into(),
                key: "<C-r>".into(),
                command: "p.refresh".into(),
                args: serde_json::Value::Null,
                description: String::new(),
                sequence: 1,
                source_location: None,
            }],
            &store,
        );
        reg.set_builtin(
            "global",
            "<C-r>",
            "host.reload",
            serde_json::Value::Null,
            "reload",
            &store,
        );
        let summaries = reg.summaries();
        let host = summaries.iter().find(|s| s.source == "built-in").unwrap();
        let plugin = summaries.iter().find(|s| s.source == "plugin").unwrap();
        assert_eq!(host.status, "active");
        assert_eq!(plugin.status, "conflict_lost");
    }

    #[test]
    fn registry_resolves_user_over_plugin() {
        let mut reg = KeymapRegistry::new();
        let store = diag_store();
        reg.install_plugin_raws(
            "p",
            GenerationId::new(1),
            vec![RawKeymap {
                context: "repository".into(),
                key: "gl".into(),
                command: "p.go".into(),
                args: serde_json::Value::Null,
                description: String::new(),
                sequence: 1,
                source_location: None,
            }],
            &store,
        );
        reg.set_user_keymap(
            "repository",
            "gl",
            "user.go",
            serde_json::Value::Null,
            "user",
            &store,
        );
        let summaries = reg.summaries();
        let user = summaries.iter().find(|s| s.source == "user").unwrap();
        let plugin = summaries.iter().find(|s| s.source == "plugin").unwrap();
        assert_eq!(user.status, "active");
        assert_eq!(plugin.status, "conflict_lost");
    }

    #[test]
    fn match_chord_returns_pending_for_prefix() {
        let mut reg = KeymapRegistry::new();
        let store = diag_store();
        reg.install_plugin_raws(
            "p",
            GenerationId::new(1),
            vec![RawKeymap {
                context: "repository".into(),
                key: "gl".into(),
                command: "p.go".into(),
                args: serde_json::Value::Null,
                description: String::new(),
                sequence: 1,
                source_location: None,
            }],
            &store,
        );
        let out = reg.match_chord("repository", &[Keystroke::plain("g")]);
        assert!(matches!(out, MatchOutcome::Pending));
        let out = reg.match_chord("repository", &ks("gl"));
        assert!(matches!(out, MatchOutcome::Match { .. }));
        let out = reg.match_chord("tab_bar", &ks("gl"));
        assert!(matches!(out, MatchOutcome::None));
    }

    #[test]
    fn drop_for_generation_cleans_up() {
        let mut reg = KeymapRegistry::new();
        let store = diag_store();
        reg.install_plugin_raws(
            "p",
            GenerationId::new(1),
            vec![RawKeymap {
                context: "repository".into(),
                key: "gl".into(),
                command: "p.go".into(),
                args: serde_json::Value::Null,
                description: String::new(),
                sequence: 1,
                source_location: None,
            }],
            &store,
        );
        assert_eq!(reg.entries().len(), 1);
        reg.drop_for_generation("p", GenerationId::new(1));
        assert!(reg.entries().is_empty());
    }
}
