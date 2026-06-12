use super::*;

/// Wall-clock timestamp (UNIX millis) used by reload-history rows. The
/// history view sorts by this, so a non-monotonic clock just shuffles
/// neighbouring entries — never important enough to switch to
/// `Instant`.
pub(super) fn build_watch_event_table(
    lua: &Lua,
    event: &crate::plugin::watchers::WatchEvent,
) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("kind", event.kind.as_str())?;
    let paths = lua.create_table()?;
    for (i, p) in event.paths.iter().enumerate() {
        paths.set(i + 1, p.as_str())?;
    }
    t.set("paths", paths)?;
    Ok(t)
}

pub(super) fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Hash the fields of each ref that plugins can observe via
/// `leviathan.repository`. Ref ordering matters — libgit2 is stable within
/// a run, and a shuffled list with otherwise-identical content would
/// produce the same table anyway. Cheap on typical repos (a few dozen
/// refs); kept self-contained so the host doesn't depend on the
/// projection-layer `ProjectionSignature`.
pub(super) fn compute_repo_hash<T: std::hash::Hash>(input: &T) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;

    let mut h = DefaultHasher::new();
    input.hash(&mut h);
    h.finish()
}

/// Plugin-local declaration order is carried by a `sequence` field on
/// each raw record. Implemented for every lane item so the generic
/// [`ThreeLaneMerge`] can order them without knowing their concrete
/// type.
pub(super) trait HasSequence {
    fn sequence(&self) -> u64;
}

impl HasSequence for api::RawAutocmd {
    fn sequence(&self) -> u64 {
        self.sequence
    }
}

impl HasSequence for api::RawAutocmdGroup {
    fn sequence(&self) -> u64 {
        self.sequence
    }
}

impl HasSequence for api::RawAutocmdClear {
    fn sequence(&self) -> u64 {
        self.sequence
    }
}

impl HasSequence for staged_reload::StagedAutocmd {
    fn sequence(&self) -> u64 {
        self.sequence
    }
}

/// One lane of the three-way merge: an autocmd create (generic over
/// the create-record type), a group create, or a group clear.
pub(super) enum Lane<A> {
    Create(A),
    Group(api::RawAutocmdGroup),
    Clear(api::RawAutocmdClear),
}

/// Iterator that merges autocmd / group / clear ops into a single
/// stream sorted by their plugin-local sequence id. Streams produced
/// by the Lua API are already in ascending sequence order, so a 3-way
/// head comparison is enough.
pub(super) struct ThreeLaneMerge<A> {
    autocmds: std::vec::IntoIter<A>,
    groups: std::vec::IntoIter<api::RawAutocmdGroup>,
    clears: std::vec::IntoIter<api::RawAutocmdClear>,
    next_autocmd: Option<A>,
    next_group: Option<api::RawAutocmdGroup>,
    next_clear: Option<api::RawAutocmdClear>,
}

impl<A: HasSequence> ThreeLaneMerge<A> {
    pub(super) fn new(
        autocmds: Vec<A>,
        groups: Vec<api::RawAutocmdGroup>,
        clears: Vec<api::RawAutocmdClear>,
    ) -> Self {
        let mut autocmds = autocmds.into_iter();
        let mut groups = groups.into_iter();
        let mut clears = clears.into_iter();
        let next_autocmd = autocmds.next();
        let next_group = groups.next();
        let next_clear = clears.next();
        Self {
            autocmds,
            groups,
            clears,
            next_autocmd,
            next_group,
            next_clear,
        }
    }
}

impl<A: HasSequence> Iterator for ThreeLaneMerge<A> {
    type Item = Lane<A>;
    fn next(&mut self) -> Option<Self::Item> {
        let a = self.next_autocmd.as_ref().map(HasSequence::sequence);
        let g = self.next_group.as_ref().map(HasSequence::sequence);
        let c = self.next_clear.as_ref().map(HasSequence::sequence);
        let mut min: Option<(u64, u8)> = None; // (sequence, lane: 0=a, 1=g, 2=c)
        for (lane, value) in [(0u8, a), (1, g), (2, c)] {
            if let Some(seq) = value {
                if min.map(|(s, _)| seq < s).unwrap_or(true) {
                    min = Some((seq, lane));
                }
            }
        }
        let (_, lane) = min?;
        match lane {
            0 => {
                let item = self.next_autocmd.take()?;
                self.next_autocmd = self.autocmds.next();
                Some(Lane::Create(item))
            }
            1 => {
                let item = self.next_group.take()?;
                self.next_group = self.groups.next();
                Some(Lane::Group(item))
            }
            _ => {
                let item = self.next_clear.take()?;
                self.next_clear = self.clears.next();
                Some(Lane::Clear(item))
            }
        }
    }
}

/// One step of the host's autocmd-install replay. Built by
/// [`MergedAutocmdOps`] from the three streams the plugin's init
/// produced (autocmd creates, group creates, group clears) and
/// emitted in plugin-local declaration order.
pub(super) type AutocmdOp = Lane<api::RawAutocmd>;

/// Iterator that merges cold-load autocmd / group / clear ops into a
/// single stream sorted by their plugin-local sequence id.
pub(super) type MergedAutocmdOps = ThreeLaneMerge<api::RawAutocmd>;

/// Staged variant of [`AutocmdOp`] used by `commit_staging`. Mirrors
/// the cold-load merger but operates on `StagedAutocmd` instead of
/// `RawAutocmd` (the staged record carries the resolved canonical
/// event name pre-computed during staging).
pub(super) type StagedAutocmdOp = Lane<staged_reload::StagedAutocmd>;

pub(super) type MergedStagedAutocmdOps = ThreeLaneMerge<staged_reload::StagedAutocmd>;

/// Lightweight snapshot of an [`AutocmdEntry`] used when iterating
/// during dispatch. Cloning the few primitive fields up front lets
/// the funnel hand control off to plugin Lua callbacks without
/// holding a borrow on the EventBus across re-entrant dispatch.
pub(super) struct EntrySnapshot {
    pub(super) id: u64,
    pub(super) index: usize,
    pub(super) plugin_id: String,
    pub(super) generation_id: GenerationId,
    pub(super) once: bool,
    pub(super) debounce_ms: u64,
    pub(super) pattern_present: bool,
    pub(super) matches_pattern: bool,
    pub(super) last_fire_clock_ms: Option<u64>,
    pub(super) disabled: bool,
}

/// Build the typed payload table the Lua callback receives. Mirrors
/// the descriptor's `LeviathanAutocmdEvent` shape. Empty payload
/// tables still surface as `{}` so the callback signature stays
/// uniform across every autocmd event.
pub(super) fn build_payload_table(
    lua: &Lua,
    canonical: &'static event_descriptor::ApiEvent,
    alias_used: Option<&'static str>,
    payload: &EventPayload,
) -> mlua::Result<Table> {
    let outer = lua.create_table()?;
    outer.set("event", canonical.name)?;
    if let Some(alias) = alias_used {
        outer.set("alias", alias)?;
    }
    let payload_value: serde_json::Value = serde_json::Value::Object(payload.clone());
    let payload_lua: LuaValue = lua.to_value(&payload_value)?;
    outer.set("payload", payload_lua)?;
    Ok(outer)
}

/// Build a [`PluginSourceSpan::Lua`] from the optional registration
/// site recorded by the resource ledger. The plugin's chunk name
/// drives the `file` field; `line` is parsed out of the `:N:` infix
/// when present so devtools shows the autocmd registration call site.
pub(super) fn make_lua_span(plugin_id: &str, source_location: Option<&str>) -> PluginSourceSpan {
    let file = format!("plugins/{plugin_id}/init.lua");
    let line = source_location.and_then(|raw| {
        let bytes = raw.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b':' {
                let start = i + 1;
                let mut end = start;
                while end < bytes.len() && bytes[end].is_ascii_digit() {
                    end += 1;
                }
                if end > start {
                    if let Ok(n) = raw[start..end].parse::<u32>() {
                        return Some(n);
                    }
                }
            }
            i += 1;
        }
        None
    });
    PluginSourceSpan::Lua {
        file,
        line,
        traceback: source_location.map(str::to_string),
    }
}

pub(super) fn record_screen_state_resource(
    ledger: &ResourceLedger,
    screen_id: &str,
    source_location: Option<String>,
) {
    let handle = format!("screen_state:{screen_id}");
    ledger.remove_by_kind_handle(PluginResourceKind::PersistedScreenState, &handle);
    ledger.remove_by_kind_handle(PluginResourceKind::LuaRegistryKey, &handle);
    ledger.record(
        PluginResourceKind::PersistedScreenState,
        handle.clone(),
        source_location.clone(),
    );
    ledger.record(PluginResourceKind::LuaRegistryKey, handle, source_location);
}
