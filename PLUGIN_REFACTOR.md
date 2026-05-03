# Plugin System Refactor Plan

This is the plan for reaching the full dream plugin system, not a reduced
version of it.

The end state is a Neovim-class plugin platform for Git Leviathan:

- Lua plugins with runtimepath, `require`, after-directories, health checks,
  commands, keymaps, autocmds, lazy loading, reload, and first-class devtools.
- Host-owned, typed, declarative UI rendered through Iced.
- Explicit capabilities with persisted user grants and audit trails.
- Transactional hot reload with plugin generations and complete cleanup.
- Typed Git, repository, persistence, async, command, keymap, event, service,
  settings, and UI APIs.
- Dependency resolution, lockfiles, package tooling, tests, and registry-ready
  packaging.

The current plugin system has the right seed:

- one Lua state per plugin
- `plugin.toml`
- Lua-host API tables
- declarative widget trees
- region slots
- events
- commands
- persistence
- capabilities
- audit log
- service registry
- health checks
- hot reload support

This plan hardens that seed into the final architecture.

## Core Rule

Every phase must move toward the final system directly. Temporary scaffolding is
allowed only when it has a clear removal path and does not shape the public API.

No phase is allowed to create a second plugin model. There is one model:

```text
plugin package
  -> manifest and dependency resolver
  -> capability grant resolver
  -> plugin generation
  -> isolated Lua runtime
  -> typed host APIs
  -> resource ledger
  -> declarative UI AST
  -> host-owned renderer
  -> typed app effects
  -> devtools and audit trail
```

## Engineering Invariants

These are enforced from Phase 1 onward and never relaxed.

1. The host owns every effect.

   Plugins request work. Rust validates, schedules, executes, renders, and
   records it.

2. Every plugin-owned thing belongs to a generation.

   Slots, callbacks, commands, keymaps, autocmds, timers, jobs, file watchers,
   service implementations, persisted handles, and UI state are attached to
   `(plugin_id, generation_id)`.

3. Reload is transactional.

   A failed reload leaves the previous generation active.

4. Unload is complete.

   Unload removes every resource even if plugin Lua code fails.

5. Host boundaries are typed.

   Lua can be dynamic internally. Data crossing into Rust becomes versioned,
   validated Rust data with precise diagnostics.

6. Capabilities are enforced at use time.

   Manifest declarations are not enough. Every sensitive API call checks the
   persisted grant.

7. Devtools are not optional.

   Every major subsystem ships with inspection and diagnostic output.

8. Tests prove cleanup, failure containment, and upgrade behavior.

   Happy path tests are insufficient.

## Phase Gates

Each phase has an acceptance gate. A phase is not done until:

- bundled demo plugins still work or have a documented migration
- old public APIs are either supported or intentionally version-gated
- tests cover success, failure, reload, and unload paths
- devtools or diagnostic output can explain new runtime state
- no plugin failure can panic the host

## Phase 0: Baseline And Contract Freeze

Goal: capture what exists before changing it.

### Work

- Inventory every existing `leviathan.*` Lua API.
- Inventory every widget kind and field.
- Inventory every plugin-owned resource currently stored by `PluginHost`.
- Inventory bundled plugins and which APIs they use.
- Write generated plugin API docs.
- Write generated plugin widget docs.
- Add Lua type annotations for the current API under `docs/lua/`.
- Add a bundled-plugin smoke test.
- Add a snapshot test for current slot registrations from bundled plugins.
- Add a snapshot test for current region descriptors.

### Deliverables

- generated plugin API docs
- generated plugin widget docs
- generated or handwritten Lua annotations
- bundled-plugin test harness
- baseline snapshots

### Acceptance Gate

- The existing plugin system is documented well enough that future migrations
  can be intentional.
- Every bundled plugin loads in tests.
- Current APIs have a named surface: `api_version = "1.0"`.

## Phase 1: Plugin Identity, Generations, And Resource Ledger

Goal: make ownership explicit before adding more capabilities.

### Work

- Add `PluginId`, `GenerationId`, `ResourceId`, and `PluginResourceKind`
  newtypes.
- Replace ad hoc plugin-owned collections with a central `ResourceLedger`.
- Every registration records:
  - plugin id
  - generation id
  - resource kind
  - host-side handle
  - source location when available
  - creation time
- Track these resource kinds:
  - Lua registry key
  - slot
  - screen
  - overlay placeholder
  - command
  - keymap placeholder
  - autocmd
  - timer placeholder
  - async job placeholder
  - file watcher placeholder
  - service registration
  - dynamic widget cache
  - persisted screen state
- Add `PluginGeneration`:

```rust
struct PluginGeneration {
    plugin_id: PluginId,
    generation_id: GenerationId,
    state: GenerationState,
    lua: Rc<Lua>,
    ledger: ResourceLedger,
}
```

- Add cleanup through the ledger.
- Add tests that intentionally fail cleanup callbacks and prove host cleanup
  still completes.

### Deliverables

- `src/plugin/generation.rs`
- `src/plugin/resources.rs`
- migrated `LoadedPlugin` ownership model
- devtools output listing resources by plugin and generation

### Acceptance Gate

- Unloading a plugin removes all known resources.
- Tests can assert that a plugin has zero remaining resources after unload.
- No resource cleanup depends on Lua code succeeding.

## Phase 2: Host API Descriptors And Schema Generation

Goal: make the plugin API self-describing.

### Work

- Introduce a descriptor layer for Lua APIs:

```rust
ApiModule {
    name,
    version,
    functions,
    events,
    types,
    capabilities,
}
```

- Describe every existing host API function in descriptors.
- Generate:
  - Lua annotations
  - Markdown API docs
  - JSON schemas for manifest sections and widget specs
  - runtime validator metadata
- Move region descriptors and widget descriptors into the same source of truth.
- Add `leviathan.has("module.feature@version")`.
- Add `leviathan.api.describe()`.

### Deliverables

- `git_leviathan_plugin_api/src/descriptor/api.rs`
- generated docs under `docs/generated/`
- generated Lua annotations under `docs/generated/lua/`
- schema files under `docs/generated/schema/`

### Acceptance Gate

- Docs, Lua annotations, and runtime validation are generated from the same
  descriptors.
- Adding a host API without a descriptor fails tests.
- Existing plugin APIs remain available under versioned descriptors.

## Phase 3: Typed Error Model And Diagnostics

Goal: make every plugin failure explainable.

### Work

- Add `PluginDiagnostic`:

```rust
struct PluginDiagnostic {
    plugin_id: PluginId,
    generation_id: Option<GenerationId>,
    severity: DiagnosticSeverity,
    code: String,
    message: String,
    source: Option<PluginSourceSpan>,
    context: serde_json::Value,
    timestamp: Instant,
}
```

- Convert Lua load, callback, schema, capability, widget, reload, and cleanup
  errors into diagnostics.
- Add source spans for:
  - manifest path and key
  - Lua file and line when available
  - widget path like `widget.children[2].child`
  - API function name
- Replace scattered `eprintln!` calls in plugin host paths with diagnostics.
- Keep stderr logging as a sink, not the source of truth.

### Deliverables

- `src/plugin/diagnostic.rs`
- diagnostic store on `PluginHost`
- devtools diagnostics list
- test helpers for asserting diagnostics

### Acceptance Gate

- Invalid manifest, invalid widget, Lua callback error, denied capability, and
  failed reload all produce structured diagnostics.
- The app keeps running after each failure.

## Phase 4: Typed Widget AST

Goal: replace ad hoc JSON rendering with a validated retained AST.

### Work

- Define `WidgetAst` enums for every current widget kind.
- Decode Lua tables into `WidgetAst` with field-level errors.
- Preserve existing Lua table syntax.
- Add stable node ids:

```lua
{ kind = "button", id = "refresh", icon = "refresh-cw", on_click = "refresh" }
```

- Normalize defaults during decoding.
- Store ASTs in slot and screen state instead of raw `serde_json::Value`.
- Render from `WidgetAst` to Iced.
- Add max tree depth, max node count, max string length, and max image size.
- Add an error widget for invalid plugin UI.
- Add AST snapshot tests.
- Add fuzz tests for random widget trees.

### Deliverables

- `src/plugin/ui/widget_ast.rs`
- migrated `bridge/widget_tree` renderer
- widget validation diagnostics
- AST snapshot tests

### Acceptance Gate

- A malformed widget tree cannot panic the renderer.
- Widget errors point to the exact bad field.
- Bundled plugins render through `WidgetAst`.
- Existing static and dynamic widgets still work.

## Phase 5: Runtimepath And Lua Module Loader

Goal: make Lua authoring feel like real Neovim-style Lua, not one giant
`init.lua`.

### Work

- Add plugin package layout support:

```text
plugin.toml
init.lua
lua/<plugin_id>/
after/plugin/
assets/
doc/
tests/
migrations/
```

- Implement per-plugin `require` resolution.
- Add plugin-local module cache owned by generation.
- Add dependency-visible module paths.
- Prevent accidental private module access across plugins.
- Add `after/` loading after dependencies.
- Add strict globals by default.
- Add runtime introspection:

```lua
leviathan.runtime.path()
leviathan.runtime.find(module)
leviathan.runtime.module_graph()
```

- Add tests for:
  - plugin-local modules
  - dependency modules
  - forbidden private modules
  - after-directory order
  - module cache cleared on reload

### Deliverables

- `src/plugin/runtime_path.rs`
- `src/plugin/lua_loader.rs`
- Lua module tests
- docs for plugin package layout

### Acceptance Gate

- Plugin authors can write `require("plugin_id.module")`.
- Reload drops old module state.
- Runtime path order is deterministic and visible in devtools.

## Phase 6: Transactional Reload

Goal: make hot reload reliable enough for daily plugin development.

### Work

- Introduce staging generations.
- Reload algorithm:
  1. keep old generation active
  2. parse new manifest
  3. resolve API version and capabilities
  4. create new Lua state
  5. install APIs
  6. run init in staging mode
  7. validate all resources
  8. run health check
  9. migrate serializable state
  10. atomically swap generations
  11. clean old generation
- Add rollback diagnostics.
- Add state migration hook:

```lua
function M.reload(old_state)
  return old_state
end
```

- Preserve active plugin screen when the screen still exists.
- Preserve split sizes when node ids still match.
- Preserve plugin settings.
- Do not preserve old registry keys, callbacks, timers, jobs, watchers, or
  services.

### Deliverables

- staged reload implementation
- reload state migration support
- reload devtools panel
- reload failure tests

### Acceptance Gate

- A syntax error during reload leaves the old plugin active.
- A bad widget during reload leaves the old plugin active.
- A failed migration leaves the old plugin active.
- Successful reload leaves no old-generation resources behind.

## Phase 7: Autocmd Groups And Typed Events

Goal: give plugins Neovim-like event composition.

### Work

- Replace raw event strings with event descriptors.
- Define core event schemas:
  - `AppStarted`
  - `AppWillQuit`
  - `RepositoryOpened`
  - `RepositoryClosed`
  - `RepositoryChanged`
  - `RefsChanged`
  - `HeadChanged`
  - `BranchChanged`
  - `CommitSelected`
  - `CommitListChanged`
  - `DiffLoaded`
  - `WorktreeChanged`
  - `FetchStarted`
  - `FetchFinished`
  - `PushStarted`
  - `PushFinished`
  - `TabAdded`
  - `TabRemoved`
  - `TabSelected`
  - `TabMoved`
  - `ThemeChanged`
  - `SettingsChanged`
  - `CommandExecuted`
- Add autocmd groups:

```lua
local group = leviathan.autocmd.group("my_plugin", { clear = true })
leviathan.autocmd.create("CommitSelected", {
  group = group,
  once = false,
  debounce_ms = 50,
  callback = function(ev) end,
})
```

- Add `once`, `clear`, `pattern`, `debounce_ms`, and `priority`.
- Add event replay in tests.
- Add failure counters per autocmd.

### Deliverables

- `src/plugin/events.rs`
- event schema descriptors
- autocmd group implementation
- event devtools view

### Acceptance Gate

- Events are typed and documented.
- Event callbacks are owned by generation.
- Failing autocmds can be disabled without disabling the whole plugin.
- Tests can replay event sequences deterministically.

## Phase 8: Commands And Command Palette

Goal: make plugin actions first-class user-facing commands.

### Work

- Define typed command descriptors:

```lua
leviathan.command.create("CommitLensRefresh", {
  title = "Commit Lens: Refresh",
  context = "repository",
  args = {
    { name = "force", type = "boolean", default = false },
  },
  run = function(args) end,
})
```

- Add command palette integration.
- Add command argument schemas.
- Add command result diagnostics.
- Add command execution events.
- Add host commands and plugin commands to one registry.
- Add command search metadata:
  - title
  - description
  - plugin id
  - context
  - destructive flag
  - capability requirements
- Add tests for command registration, invocation, invalid args, unload cleanup,
  and reload replacement.

### Deliverables

- `src/plugin/commands.rs`
- command palette integration
- command schema docs
- command devtools view

### Acceptance Gate

- Plugin commands appear in the command palette.
- Commands can be called from Lua, UI buttons, keymaps, and tests through the
  same dispatch path.
- Unloading a plugin removes its commands.

## Phase 9: Context-Aware Keymaps

Goal: give plugins the other half of the Neovim interaction model.

### Work

- Define keymap contexts:
  - `global`
  - `repository`
  - `repository.sidebar`
  - `repository.graph`
  - `repository.details`
  - `repository.diff`
  - `tab_bar`
  - `plugin_screen:<id>`
  - `overlay:<id>`
- Add Lua API:

```lua
leviathan.keymap.set("repository", "gl", "CommitLensRefresh", {
  description = "Refresh commit annotations",
})
```

- Add conflict resolution:
  - built-ins win by default
  - user mappings win over plugins
  - plugin conflicts are deterministic and visible
- Add keymap inspection.
- Add tests for conflicts, context routing, unload cleanup, and reload.

### Deliverables

- `src/plugin/keymap.rs`
- keymap registry
- input routing integration
- keymap devtools view

### Acceptance Gate

- Plugin keymaps work only in declared contexts.
- Conflicts never produce nondeterministic behavior.
- User overrides are respected.

## Phase 10: Capability Grants And Security UI

Goal: turn capabilities into a durable user-facing security boundary.

### Work

- Separate requested capabilities from granted capabilities.
- Persist grants by:
  - plugin id
  - plugin version
  - capability
  - grant time
  - user decision
- Add fine-grained capabilities:
  - repository read
  - repository observe
  - git read operation set
  - git write operation set
  - filesystem read scopes
  - filesystem write scopes
  - network domain allowlist
  - process executable allowlist
  - clipboard read/write
  - notifications
  - environment variable allowlist
  - credentials
  - UI regions
  - services provided and consumed
- Add capability prompt UI for new grants.
- Add upgrade prompt when requested capabilities change.
- Add grant revoke UI.
- Enforce grants at every sensitive API call.
- Add canonical path and symlink tests.
- Add audit entries with plugin id and generation id.

### Deliverables

- `src/plugin/capability_grants.rs`
- persisted grant store
- security prompt overlay
- capability devtools view
- expanded audit tests

### Acceptance Gate

- A plugin cannot use a requested but ungranted capability.
- A plugin upgrade that requests new access is blocked until approved.
- Revoking a grant takes effect without restarting the app.
- Denied calls produce structured diagnostics and audit entries.

## Phase 11: Typed Repository And Git APIs

Goal: stop plugins from needing shell access for normal Git work.

### Work

- Expose read APIs through existing repository gateways:

```lua
leviathan.repository.current()
leviathan.repository.refs()
leviathan.repository.head()
leviathan.repository.status()
leviathan.repository.commits({ limit = 100, rev = "HEAD" })
leviathan.repository.diff({ commit = hash })
leviathan.repository.file_at({ commit = hash, path = "src/main.rs" })
```

- Expose write APIs through existing app task pipeline:

```lua
leviathan.git.checkout({ ref = "main" })
leviathan.git.create_branch({ name = "topic", start_point = "HEAD" })
leviathan.git.create_tag({ name = "v1.2.3", target = "HEAD" })
leviathan.git.stash_push({ message = "from plugin" })
```

- Add capability checks for each operation.
- Add destructive-operation confirmation policy.
- Add audit entries for all Git writes.
- Add result events for plugin-triggered Git operations.
- Add tests using fixture repositories.

### Deliverables

- `src/plugin/api/git.rs`
- expanded `repository` API
- Git operation descriptors
- fixture tests

### Acceptance Gate

- Normal plugin Git reads do not require process spawn.
- Git writes go through the same task and message flow as built-in UI actions.
- Destructive Git writes require explicit capability and confirmation policy.

## Phase 12: Async Jobs, Timers, And File Watchers

Goal: let plugins do real work without blocking the GUI thread.

### Work

- Add job ownership by generation.
- Add cancellation tokens.
- Add timeouts.
- Add memory and output limits where practical.
- Add APIs:

```lua
local job = leviathan.async.spawn(function(ctx) end)
job:cancel()

leviathan.timer.after(250, callback)
leviathan.timer.every(1000, callback)

leviathan.fs.watch(path, { recursive = false }, callback)
```

- Ensure Lua states are not shared unsafely across threads.
- Serialize cross-thread values through typed channels.
- Add scheduled main-thread callbacks:

```lua
leviathan.schedule(function() end)
```

- Add devtools for jobs, timers, and watchers.
- Add tests:
  - unload cancels jobs
  - reload cancels old-generation jobs
  - timer callback failure is contained
  - watcher path capability is enforced

### Deliverables

- `src/plugin/async_jobs.rs`
- `src/plugin/timers.rs`
- `src/plugin/watchers.rs`
- async devtools view

### Acceptance Gate

- Plugin async work cannot freeze the GUI thread.
- Unload and reload cancel old generation async resources.
- Jobs and timers are visible in devtools.

## Phase 13: Persistence, Settings, Migrations, And Secrets

Goal: make plugin state reliable and user-manageable.

### Work

- Split storage:
  - state
  - config
  - per-repo state
  - cache
  - secrets
- Add transactional persistence API:

```lua
leviathan.persist.transaction(function(tx)
  tx:set("a", 1)
  tx:set("b", 2)
end)
```

- Add migration files under `migrations/`.
- Add settings schemas:

```lua
local settings = leviathan.settings.get()
leviathan.settings.on_change(function(new_settings) end)
```

- Render settings UI from schema.
- Validate settings before save.
- Add secret store using OS keychain when available.
- Add devtools state browser and reset.
- Add tests for migration failure, corrupt state, settings validation, and
  reset.

### Deliverables

- `src/plugin/settings.rs`
- `src/plugin/secrets.rs`
- persistence migration runner
- settings UI integration
- state devtools view

### Acceptance Gate

- Plugin state migrations are atomic.
- Corrupt plugin state does not break the app.
- User can inspect and reset plugin state.
- Secrets are not stored in plain plugin state.

## Phase 14: Inter-Plugin Services

Goal: make plugin composition powerful without making plugins invisible to the
host.

### Work

- Version service names.
- Support required and optional consumers.
- Enforce load order for required services.
- Add provider registration:

```lua
leviathan.services.register("issue_tracker", 1, {
  lookup = function(commit) end,
})
```

- Add consumer lookup:

```lua
local tracker = leviathan.services.get("issue_tracker", 1)
```

- Trace service calls.
- Enforce caller capabilities even when a provider performs work.
- Add failure isolation for provider errors.
- Add devtools service graph.

### Deliverables

- hardened service registry
- service call tracing
- service graph devtools
- service version tests

### Acceptance Gate

- Service calls are visible to the host.
- Provider failure degrades consumers gracefully.
- Services cannot bypass capability checks.

## Phase 15: Dependency Resolver And Lockfile

Goal: make plugin loading deterministic and package-manager ready.

### Work

- Resolve plugin dependencies before load.
- Support SemVer ranges.
- Support optional dependencies.
- Detect cycles.
- Detect conflicts.
- Add deterministic load order.
- Add lockfile:

```toml
[[plugin]]
id = "commit_lens"
version = "2.4.1"
source = "registry"
checksum = "sha256:..."
```

- Add local development overrides.
- Add dependency diagnostics.
- Add tests for cycles, optional deps, missing deps, incompatible versions, and
  lockfile stability.

### Deliverables

- `src/plugin/dependency.rs`
- `src/plugin/lockfile.rs`
- lockfile read/write
- dependency devtools view

### Acceptance Gate

- Plugin load order is deterministic.
- Missing required dependencies block only affected plugins.
- Optional dependencies can appear after reload and activate dependent features.

## Phase 16: Lazy Loading And Activation

Goal: make plugin startup scalable and Neovim-like.

### Work

- Add manifest activation triggers:
  - events
  - commands
  - keymaps
  - regions
  - repository shape
  - file presence
  - explicit user action
- Add stub registrations for lazy commands and keymaps.
- Load plugin on first trigger.
- Add activation diagnostics.
- Add tests for lazy command invocation, lazy keymap invocation, lazy event
  activation, and failed lazy activation.

### Deliverables

- activation resolver
- lazy registration store
- activation devtools view

### Acceptance Gate

- A lazy plugin can contribute commands and keymaps before its Lua state exists.
- First use activates the plugin exactly once.
- Failed lazy activation is visible and contained.

## Phase 17: Extension Point Expansion

Goal: let plugins extend Git Leviathan deeply without replacing core panels.

### Work

- Expand region descriptors:
  - `status_bar.left`
  - `status_bar.center`
  - `status_bar.right`
  - `repository.sidebar.section:<id>`
  - `repository.graph.top`
  - `repository.graph.row:<commit_hash>`
  - `repository.graph.decorations`
  - `repository.graph.context_menu`
  - `repository.details.commit_header`
  - `repository.details.files`
  - `repository.diff.toolbar`
  - `repository.diff.line:<file>:<line>`
  - `repository.diff.hunk:<id>`
  - `repository.diff.context_menu`
- Add overlays and context menu extension APIs.
- Add graph decoration AST.
- Add diff decoration AST.
- Add extension-point capability checks.
- Add rendering tests for each extension point.

### Deliverables

- expanded region descriptor table
- graph decoration API
- diff decoration API
- context menu extension API
- overlay registration API

### Acceptance Gate

- Plugins can add targeted behavior to graph, diff, details, sidebar, status,
  and context menus without replacing whole screens.
- Each extension point is capability-gated and documented.

## Phase 18: Performance Budgets And Circuit Breakers

Goal: keep the app responsive under bad plugins.

### Work

- Add budgets:
  - init soft and hard limits
  - event callback soft and hard limits
  - UI callback soft and hard limits
  - command callback soft and hard limits
  - widget AST size limits
  - log rate limits
  - network response limits
  - process output limits
- Add per-callback timing.
- Add repeated-failure counters.
- Add circuit breaker:
  - disable callback after repeated failures
  - degrade plugin after repeated callback disables
  - allow user re-enable from devtools
- Add performance traces.
- Add tests with intentionally slow callbacks.

### Deliverables

- `src/plugin/performance.rs`
- callback budget enforcement
- circuit breaker
- performance devtools view

### Acceptance Gate

- Slow plugins cannot indefinitely stall interaction.
- Repeated failures degrade predictably.
- User can see why something was disabled.

## Phase 19: Devtools Completion

Goal: make the plugin system inspectable enough to debug without stderr.

### Work

- Build devtools panels:
  - installed plugins
  - active generations
  - runtime path
  - manifest
  - requested and granted capabilities
  - audit log
  - diagnostics
  - event subscriptions
  - commands
  - keymaps
  - slots and regions
  - widget AST inspector
  - services graph
  - async jobs
  - timers
  - file watchers
  - persisted state
  - reload history
  - performance traces
- Add command palette commands:
  - `Plugin: Reload`
  - `Plugin: Disable`
  - `Plugin: Enable`
  - `Plugin: Open Log`
  - `Plugin: Inspect UI Tree`
  - `Plugin: Run Health Check`
  - `Plugin: Clear State`
  - `Plugin: Export Diagnostic Bundle`
  - `Plugin: Show Capability Audit`
  - `Plugin: Show Runtime Path`

### Deliverables

- complete plugin devtools screen
- diagnostic bundle exporter
- devtools snapshot tests

### Acceptance Gate

- Every plugin-owned resource is visible somewhere in devtools.
- Diagnostic bundles include enough data to debug plugin failures without
  copying stderr.
- Sensitive data is excluded unless user explicitly includes it.

## Phase 20: Plugin Test Harness And Fuzzing

Goal: make plugin behavior testable like application code.

### Work

- Add Lua test harness:

```lua
local t = require("leviathan.test")

t.describe("commit_lens", function()
  t.it("registers a refresh command", function(host)
    host.load_plugin("./")
    t.assert.command_exists("CommitLensRefresh")
  end)
end)
```

- Add host-side fixture API:
  - create repository
  - create commits
  - create branches
  - select commit
  - dispatch event
  - invoke command
  - press keymap
  - reload plugin
  - unload plugin
- Add fuzz targets:
  - manifests
  - widget ASTs
  - event payloads
  - reload failure points
  - capability paths and symlinks
  - async unload races
  - dependency graphs
- Add CI jobs for plugin host tests.

### Deliverables

- `src/plugin/tests/harness.rs` expansion
- Lua test library
- fuzz targets
- CI integration

### Acceptance Gate

- Plugin authors can test plugins without launching the GUI.
- Host fuzzing covers invalid input and lifecycle races.
- Bundled plugins ship with tests.

## Phase 21: Plugin Author Tooling

Goal: make authoring and publishing practical.

### Work

- Add xtask commands:
  - `xtask plugin new <id>`
  - `xtask plugin test`
  - `xtask plugin lint`
  - `xtask plugin package`
  - `xtask plugin inspect`
  - `xtask plugin publish`
- Add templates:
  - main bar slot
  - repository sidebar panel
  - command and keymap plugin
  - graph decoration plugin
  - diff decoration plugin
  - service provider plugin
  - lazy-loaded plugin
- Add lint rules:
  - manifest schema
  - undeclared capabilities
  - unknown API calls where statically detectable
  - invalid widget fields
  - missing docs
  - missing tests
- Add package format with checksums.

### Deliverables

- xtask plugin commands
- plugin templates
- linter
- package builder

### Acceptance Gate

- A new plugin can be scaffolded, tested, linted, packaged, and installed from
  local files.
- Generated templates follow the final architecture.

## Phase 22: Registry, Signing, And Supply Chain

Goal: make the ecosystem trustworthy.

### Work

- Define plugin package metadata.
- Add package checksums.
- Add signature verification.
- Add trust roots.
- Add registry index format.
- Add local registry support for development.
- Add revocation list support.
- Add upgrade plan preview:
  - version changes
  - dependency changes
  - capability changes
  - checksums
  - signatures
- Add tests for tampered packages, revoked versions, and changed capabilities.

### Deliverables

- registry index format
- package signature verifier
- package installer
- upgrade planner

### Acceptance Gate

- Installed plugin versions are reproducible from lockfile.
- Tampered packages are rejected.
- Capability changes are shown before upgrade.

## Phase 23: API Migration

Goal: bring existing plugins forward without freezing the new system in old
shapes.

### Work

- Reject incompatible plugin API manifests.
- Migrate bundled plugins to v1 APIs.
- Add boundary tests:
  - incompatible plugin API manifests are rejected
  - v1 plugins use new descriptors
  - multiple v1 plugins can coexist
- Add `leviathan.has(...)` examples.

### Deliverables

- migrated bundled plugins
- v1 boundary tests

### Acceptance Gate

- Existing bundled plugins declare the v1 API.
- New plugin templates use v1 only.

## Phase 24: Full System Acceptance

Goal: prove the dream system exists.

### Required Demo Plugins

Build and ship these as fixtures:

1. `repository_info_v1`

   Replaces built-in repo info through typed slots and dynamic AST.

2. `commit_lens`

   Adds graph row decorations, details panel annotations, commands, keymaps,
   async refresh, persistence, and health checks.

3. `issue_tracker`

   Provides a versioned service consumed by `commit_lens`.

4. `diff_notes`

   Adds diff line decorations and context menu actions.

5. `repo_guard`

   Uses Git write APIs with confirmation policy and capability prompts.

6. `lazy_demo`

   Loads only when a command, keymap, or event is triggered.

7. `bad_plugin_suite`

   Intentionally bad plugins for invalid UI, denied capabilities, slow
   callbacks, failed reload, dependency cycles, and async cleanup races.

### Final Acceptance Tests

- Load all good demo plugins.
- Trigger commands, keymaps, events, services, UI slots, graph decorations, diff
  decorations, async jobs, timers, watchers, settings, and persistence.
- Reload each plugin successfully.
- Reload each plugin with a staged failure and prove old generation survives.
- Unload each plugin and prove zero resources remain.
- Revoke capabilities and prove calls fail.
- Upgrade a plugin requesting new capabilities and prove user approval is
  required.
- Run fuzz tests.
- Export diagnostic bundle.
- Verify devtools can inspect every active resource.

### Final Acceptance Gate

The system is accepted only when all statements are true:

- Plugin authors can write idiomatic Lua modules with `require`.
- Plugins can use autocmds, commands, keymaps, lazy loading, health checks,
  services, settings, persistence, async jobs, timers, watchers, screens,
  overlays, slots, graph decorations, diff decorations, and context menus.
- Every plugin resource belongs to a generation.
- Unload removes every resource.
- Failed reload leaves the old plugin running.
- Invalid UI cannot panic the host.
- Slow callbacks are measured and contained.
- Repeated failures trip a circuit breaker.
- Capabilities are requested, granted, persisted, checked, audited, revoked,
  and rechecked on upgrade.
- Git writes are typed, capability-gated, audited, and routed through the app's
  task pipeline.
- Plugin dependencies resolve deterministically.
- Lockfile installs are reproducible.
- Package signatures and checksums are enforced.
- Devtools explain runtime path, resources, diagnostics, audit logs, services,
  jobs, timers, watchers, widget ASTs, commands, keymaps, and performance.
- Docs, Lua annotations, schemas, and runtime validators come from the same API
  descriptors.
- A Neovim user recognizes the model immediately.

## Suggested Implementation Order

The phases above are ordered for dependency safety. The shortest credible path
is:

1. Baseline and freeze the current API surface.
2. Add generations and the resource ledger.
3. Add descriptors, schemas, diagnostics.
4. Type the widget AST.
5. Make reload transactional.
6. Add runtimepath and Lua modules.
7. Add events, commands, and keymaps.
8. Harden capabilities and Git APIs.
9. Add async ownership.
10. Add persistence, settings, services, dependencies, lazy loading.
11. Expand extension points.
12. Add performance budgets and devtools.
13. Add tests, tooling, packaging, registry, and API migration.
14. Pass full system acceptance.

Do not swap steps 2 and 4. The resource ledger needs to exist before more
resources are introduced.

Do not add broad process or filesystem APIs as a shortcut around missing Git or
repository APIs. That creates the wrong ecosystem.

Do not make plugin UI imperative. The host-owned AST is the line that keeps the
application stable.

## Risk Register

### Risk: The host accumulates stale API hacks

Mitigation:

- keep v1 descriptors clean
- make new templates v1 only

### Risk: Capability prompts become annoying

Mitigation:

- group low-risk capabilities
- explain high-risk capabilities clearly
- persist grants
- show capability changes only on upgrade

### Risk: Lua callback timing is hard to enforce

Mitigation:

- start with measurement and diagnostics
- add soft budgets before hard cancellation
- isolate long work into host-owned async jobs

### Risk: Widget AST becomes too limited

Mitigation:

- expand targeted extension points
- add domain widgets like graph and diff decorations
- keep host renderer extensible through descriptors

### Risk: Devtools lag behind runtime features

Mitigation:

- every phase has a devtools or diagnostic deliverable
- resource ledger is the common data source

### Risk: Dependency and package management distract from core runtime

Mitigation:

- build deterministic local dependency resolution first
- add signing and registry only after lifecycle, reload, capabilities, and
  devtools are solid

## Definition Of Done

This refactor is done when plugin code feels like this:

```lua
local M = {}

function M.activate()
  local group = leviathan.autocmd.group("commit_lens", { clear = true })

  leviathan.command.create("CommitLensRefresh", {
    title = "Commit Lens: Refresh",
    context = "repository",
    run = function()
      require("commit_lens.refresh").run()
    end,
  })

  leviathan.keymap.set("repository.graph", "gl", "CommitLensRefresh", {
    description = "Refresh commit annotations",
  })

  leviathan.autocmd.create("CommitSelected", {
    group = group,
    debounce_ms = 50,
    callback = function(ev)
      require("commit_lens.view").select(ev.commit.hash)
    end,
  })

  leviathan.ui.slot.add({
    region = "repository",
    pane = "details",
    section = "top",
    id = "commit_lens.summary",
    priority = 20,
    widget = function()
      return require("commit_lens.view").summary()
    end,
  })
end

function M.health()
  return require("commit_lens.health").check()
end

return M
```

And the host can answer all of these questions without guesswork:

- Which plugin owns this button?
- Which generation registered this callback?
- Which capability allowed this file read?
- Why was this network request denied?
- Which event caused this UI update?
- Why did reload fail?
- Did unload remove every resource?
- Which plugin made this Git write?
- Which keymap won this conflict?
- Which service provider handled this call?
- Why was this plugin disabled?

That is the target. Build every phase toward that target.
