# Git Leviathan - Agent Guide

## IMPORTANT: READ FIRST
- You will get instructed to use "caveman mode". Respect it aggressively.
- You might have the urge to overthink things, saying "but wait" or "actually," or "let me take a different approach". When you get this urge, you'll catch yourself, going with your initial instinct. Making mistakes are OK — we can always fix those later.

## Project Overview

Git Leviathan is a Git repository visualizer built with Rust and the Iced GUI framework. It provides a visual interface for browsing Git history, branches, and commits with a graph-based visualization.

## Architecture

### Technology Stack

- **Language**: Rust (Edition 2021)
- **GUI Framework**: Iced v0.13 (advanced, canvas, SVG, tokio)
- **Git Integration**: git2 v0.19 (vendored-libgit2)
- **Async Runtime**: Tokio v1 (time, sync, rt)
- **Date/Time**: chrono v0.4
- **Font/Text**: swash v0.1.19
- **File Watching**: notify v7 (macOS fsevent)
- **Persistence**: rusqlite v0.32 (bundled)
- **Dialogs**: rfd v0.15

### Project Structure

Post-refactor layout (Phases 1–10). See `ARCHITECTURE.md` for the full plan
and acceptance criteria; `ACCEPTANCE.md` for the as-built validation.

```
src/
├── main.rs                  # Entry point
├── config.rs                # Env + config plumbing
├── style.rs / theme.rs      # Design tokens + palette
├── toast.rs                 # Toast stack (animations + manager)
├── utils.rs
├── assets/
│
├── app/                     # App layer (Phase 9)
│   ├── mod.rs               # Glue-only: App struct + top-level hooks
│   ├── update.rs            # update dispatchers (≤150 LOC)
│   ├── view.rs              # Single enum-dispatch for active screen
│   ├── tabs.rs              # TabManager
│   ├── fetch_policy.rs      # Single-slot fetch orchestration
│   ├── subscription.rs      # Subscription builder
│   ├── input.rs             # Keyboard routing
│   ├── commands.rs          # App-level tasks (open repo dialog, open url)
│   ├── fetch_ops.rs         # Fetch start/cancel helpers
│   └── animation.rs         # Animation tick bookkeeping
│
├── message/                 # Hierarchical message tree (Phase 2)
│   ├── mod.rs               # enum Message { App, Screen, Toast }
│   ├── app.rs               # AppMessage
│   ├── screen.rs            # ScreenRouted { Active, Tab } + ScreenMessage
│   └── toast.rs             # ToastMessage
│
├── core/                    # Pure git-domain types (Phase 4)
│   ├── commit.rs            # Commit, CommitKind, ChangeKind, ChangedFile
│   └── ids.rs               # TabId, RepoVersion newtypes
│
├── view_model/              # UI projections of core data (Phase 4)
│   ├── commit_presentation.rs
│   ├── diff_view.rs         # CommitDiffState
│   ├── graph_view.rs        # GraphRow, ProjectionSignature, RepositoryProjection
│   ├── sidebar_view.rs
│   └── loaded.rs            # LoadedRepo, LoadedRefs, Loaded*Outcome
│
├── services/
│   ├── mod.rs
│   ├── git_error.rs         # Structured GitError (CorruptObject, AuthFailed, …)
│   ├── git_detect.rs        # GitStatus detection
│   ├── snapshot.rs          # RepoSnapshot, RefsSnapshot DTOs
│   ├── settings.rs          # SQLite-backed persistence
│   ├── file_watcher.rs
│   ├── text_measurement.rs
│   ├── syntax_highlight.rs
│   ├── test_support.rs      # (cfg test) fixture repos
│   ├── git/                 # Low-level git2 / git CLI operations
│   │   ├── mod.rs           # GitService facade
│   │   ├── helpers.rs       # wrap_git2_error, find_branch_or, …
│   │   ├── loader.rs        # Full snapshot loader
│   │   ├── refs.rs          # Ref-snapshot loader
│   │   ├── diff.rs / working_tree_diff.rs / conflict_resolution.rs
│   │   ├── worktree.rs / stashes.rs / tags.rs / remotes.rs
│   │   ├── checkout.rs / merge.rs / rebase.rs / cherry_pick.rs
│   │   └── squash.rs / reword.rs / push.rs / fetch.rs
│   ├── gateway/             # Segregated gateway traits (Phase 3)
│   │   ├── read.rs          # RepoRead
│   │   ├── branch_ops.rs    # BranchOps : RepoRead
│   │   ├── worktree_ops.rs  # WorktreeOps : RepoRead
│   │   ├── commit_ops.rs    # CommitOps : RepoRead
│   │   ├── remote_ops.rs    # RemoteOps : RepoRead  (+ PushGatewayOutcome)
│   │   ├── stash_ops.rs / tag_ops.rs
│   │   ├── shared.rs        # GitRepositoryGateway impls + Arc-wrapped alias
│   │   └── mod.rs           # Composed `Repository` super-trait
│   └── presenter/           # View-model projection (Phase 4)
│       ├── mod.rs           # Presenter trait
│       ├── default.rs       # DefaultPresenter (Arc<dyn Presenter>)
│       ├── projection.rs    # Pure projection helpers
│       └── signature.rs     # ProjectionSignature cache key
│
├── screens/
│   ├── mod.rs
│   ├── screen_trait.rs      # Screen trait (Phase 1)
│   ├── component.rs         # Component trait (Phase 1; reserved seams)
│   ├── dialog.rs            # Dialog trait (Phase 1; consumed in Phase 6)
│   ├── blank/               # BlankScreen
│   ├── no_git/              # NoGitScreen + install guides
│   └── repository/          # Main repo screen
│       ├── mod.rs           # RepositoryScreen + Screen impl (≤400 LOC)
│       ├── messages.rs      # RepositoryMessage
│       ├── panel_messages.rs # CenterAction, DetailAction, DiffPanelAction, OverlayPanelAction
│       ├── view.rs          # View composition
│       ├── input.rs         # Keyboard routing
│       ├── animation.rs     # Per-screen animation tick
│       ├── commit_search.rs # Ctrl+F search overlay
│       ├── commands/        # Git-op task spawners
│       │   ├── mod.rs       # Result dispatch
│       │   ├── helpers.rs   # Shared spawn helpers
│       │   ├── loaders.rs / branch_ops.rs / commit_ops.rs
│       │   ├── remote_ops.rs / stash_ops.rs / tag_ops.rs
│       ├── state/           # Decomposed state (Phase 8)
│       │   ├── mod.rs       # RepositoryData composition + scroll ids
│       │   ├── snapshot.rs  # RepositorySnapshot
│       │   ├── selection.rs / animation.rs / commit_index.rs
│       │   ├── diff_cache.rs
│       │   ├── popout.rs / context_menu.rs / popout_tests.rs
│       ├── panels/          # Components (Phase 5)
│       │   ├── sidebar/     # mod, state, update, view
│       │   ├── center/      # commit list + graph
│       │   ├── detail/      # commit details panel
│       │   ├── diff/        # mod, dirty, commit, merged, conflict,
│       │   │                # search, selection, auto_scroll, update, view
│       └── overlays/        # Dialogs (Phase 6) — one module per dialog
│           ├── mod.rs       # OverlayManager + ActiveDialog enum
│           ├── validation.rs / widgets.rs
│           ├── conflict_checkout.rs / delete_branch.rs / delete_tag.rs
│           ├── rename_branch.rs / discard.rs / create_branch.rs
│           ├── create_tag.rs / add_remote/{mod,view,styles}.rs
│           ├── set_upstream.rs / force_push.rs / push_behind.rs
│           └── cherry_pick_confirm.rs / stash_delete.rs
│
└── widgets/                 # Shared widget kernel (Phase 7)
    ├── palette.rs           # PaletteRole, palette_color, lane_color
    ├── shared.rs / search_widget.rs / context_menu.rs
    ├── primitives/          # hoverable, wheel_intercept, slide_overlay, spinner
    ├── text/                # canvas, layout, selection, hit_test
    ├── list/                # HoverableList kernel (ContextMenu + BranchPopout wrap it)
    ├── graph/               # program, rendering, cache, lane_painter
    ├── diff/                # canvas, conflict_canvas
    ├── branch_label/        # layout, popout, context_menu, cell
    └── chrome/              # menu_bar, tab_bar, main_bar, status_bar
```

## Key Patterns

### Architecture Pattern: Elm Architecture (The Elm Architecture - TEA)

The application follows the Elm Architecture pattern:
- **Model**: State is stored in structs (App, HomeScreen, etc.)
- **Update**: Messages trigger state updates via `update()` methods
- **View**: State is rendered to UI elements via `view()` methods

### Segregated Gateway Traits (Phase 3)

Git operations are split across focused traits under `services/gateway/`:
- `RepoRead` — read-only (snapshot, refs, diffs)
- `BranchOps`, `WorktreeOps`, `CommitOps`, `RemoteOps`, `StashOps`, `TagOps`
  (each super-trait on `RepoRead`)
- `Repository` — composed super-trait + blanket impl

Consumers take the narrowest trait they need (`&impl RepoRead` for a
read-only panel, `&impl BranchOps` for a branch-ops dialog). The god-gateway
is gone.

- `GitRepositoryGateway`: Concrete impl using `git2` + `git` CLI helpers
- `SharedRepositoryGateway = Arc<dyn Repository>` for task sharing
- Write operations are serialized via `Arc<Mutex<()>>` ; reads bypass

### Presenter Injection (Phase 4)

`Arc<dyn Presenter>` projects raw service snapshots into `view_model::Loaded*`
types off the main thread. `DefaultPresenter` is the production impl; tests
can swap in a fake without mock macros.

### Async Task Pattern

Long-running operations use Iced's `Task` with `gateway_work`, which wraps
`tokio::task::spawn_blocking` so libgit2 / subprocess calls don't pin the
runtime worker:

```rust
Task::perform(
    gateway_work(move || {
        repo.load_repo(COMMIT_LOAD_LIMIT).map(|s| presenter.project_loaded(s))
    }),
    move |result| Message::tab(tab_id, RepositoryMessage::RepoLoaded(result)),
)
```

## Important Constants

### UI Dimensions
- `SIDEBAR_WIDTH`: 240px
- `BRANCH_COL_WIDTH`: 185px
- `DETAIL_PANEL_WIDTH`: 510px
- `TAB_HEIGHT`: 34px
- `TOOLBAR_HEIGHT`: 50px
- `STATUS_BAR_HEIGHT`: 20px
- `ROW_H`: 34px (commit row height)
- `LANE_WIDTH`: 26px (graph lane width)

### Data Limits
- `COMMIT_LOAD_LIMIT`: 500 commits per load

### Animation Timing
- `ENTER_MS`: 180ms (toast enter animation)
- `HOLD_MS`: 8000ms (toast display time)
- `EXIT_MS`: 200ms (toast exit animation)
- `OVERLAY_SLIDE_SPEED_PX_PER_MS`: 6.25
- `OVERLAY_ENTER_OFFSET`: 2000px

## Configuration

Environment variables:
- `GIT_LEVIATHAN_REPO_PATH`: Path to the Git repository to visualize
- Falls back to `$HOME/projects/ghs-sulu` or current directory

### Test hooks: `GIT_LEVIATHAN_FORCE_SCREEN`

Forces a specific startup screen, bypassing normal detection. Useful for
previewing UI states on any host OS without reproducing the underlying
condition (missing git, fresh install, etc.).

| Value              | What you see                                                       |
|--------------------|--------------------------------------------------------------------|
| *(unset)*          | Normal startup: detect git, load last session or blank screen.     |
| `blank`            | `BlankScreen` (no tabs / no saved repos).                          |
| `no-git-linux`     | `NoGitScreen` with the Linux install guide (apt / dnf / pacman).   |
| `no-git-macos`     | `NoGitScreen` with the macOS install guide (brew / xcode-select).  |
| `no-git-windows`   | `NoGitScreen` with the Windows install guide (winget / choco).     |
| `no-git-macos-clt` | `NoGitScreen` for the macOS Command Line Tools-missing variant.    |

Unknown values print a warning and are ignored.

Examples:
```bash
GIT_LEVIATHAN_FORCE_SCREEN=no-git-windows cargo run
GIT_LEVIATHAN_FORCE_SCREEN=no-git-macos-clt ./target/debug/git_leviathan
GIT_LEVIATHAN_FORCE_SCREEN=blank cargo run
```

Note: when `no-git-*` is forced, the Recheck button still runs real
detection — clicking it on a system that has git will exit the screen.

## Building

```bash
cargo build --release
```

## Testing

```bash
cargo test
```

Tests are co-located with source files using `#[cfg(test)]` modules.

## Coding Conventions

### File Organization
- One module per file
- Use `mod.rs` for directory modules
- Re-exports in module files using `pub use`

### Naming
- Snake_case for files, functions, variables
- PascalCase for types, traits, enums
- SCREAMING_SNAKE_CASE for constants

### Error Handling
- Use `GitError` enum for all service-layer errors
- Convert git2 errors to `GitError` variants at the `GitService` boundary
- `Result<T, GitError>` for all `RepositoryGateway` methods
- Use `humanize_git_error()` for user-facing messages

### State Management
- State is immutable except within `update()` methods
- Use `clone()` freely for cheap data transfer
- Repository state is versioned with `repo_version: u64`

### UI Patterns
- Views take view models (structs with references to data)
- Use Iced's `Element<Message>` for components
- Custom widgets implement `canvas::Program`

## Git Operations

### Supported Operations
- Load repository with pagination
- Checkout branches (local and remote)
- Create branches (at HEAD or specific commit)
- Delete branches (local, remote, or both)
- Rename branches
- Stash/pop for clean checkout
- Diff loading on demand
- Merge operations
- Conflict resolution

### Conflict Handling
When checking out a remote branch that conflicts with local:
1. Shows conflict overlay with options
2. Can create new branch from remote
3. Can reset local to remote
4. Can cancel operation

## Widget System

### Custom Widgets
- **widgets/graph/**: Commit graph canvas (program + rendering + cache +
  lane_painter).
- **widgets/branch_label/**: Branch label pill (layout + popout + context
  menu + cell).
- **widgets/chrome/**: App chrome split into menu_bar / tab_bar / main_bar
  / status_bar.
- **widgets/list/hoverable_list.rs**: Shared hover-row list kernel.
  `widgets/context_menu.rs` and `branch_label/popout.rs` wrap it.
- **widgets/text/**: Monospace text canvas (canvas, layout, selection,
  hit_test) shared by diff/conflict views.

### Context Menus
- Branch context menu: copy name, delete, rename
- Commit context menu: create branch at commit
- Both support keyboard dismissal

## Animation System

### Toast Animations
- Enter: Slide from left with scale up
- Hold: Static display
- Exit: Slide left with fade
- Stacking: Multiple toasts stack with gap

### Overlay Animations
- Dialogs slide in from right
- Use `slide_offset` for animation state
- Tick-based updates at 60 FPS

## Theme System

Colors defined in `theme.rs`:
- Background layers: `BG_BASE`, `BG_SIDEBAR`, `BG_PANEL`, etc.
- Text: `TEXT_PRIMARY`, `TEXT_SECONDARY`, `TEXT_DIM`
- Accents: `ACCENT_BLUE`, `ACCENT_GREEN`, `ACCENT_ORANGE`
- Lane colors: 8-color cycle for graph lanes

Custom theme applied via `iced::Theme::custom()` with `leviathan` palette.

## Text Measurement Service

The `TextMeasurementService` provides exact pixel-level text measurements using Iced's integrated cosmic_text font system.

### Location
- **Service**: `src/services/text_measurement.rs`
- **Exports**: `services::text_measurement::{FontFamily, TextMeasureResult, TextMeasurementService}`

### Usage Pattern

```rust
use crate::services::{TextMeasurementService, FontFamily};

// Create service (no initialization needed - uses Iced's global font system)
let service = TextMeasurementService::new();

// Measure single-line text
let result = service.measure_single_line("Hello, World!", FontFamily::Default, 14.0);
println!("Width: {}px, Height: {}px", result.width, result.height);

// Measure wrapped text
let wrapped = service.measure_wrapped(
    "Long text that needs wrapping",
    FontFamily::Default,
    12.0,
    100.0,  // max_width
);
println!("Lines: {}", wrapped.line_count);
```

### Font Family Selection

```rust
use crate::services::FontFamily;

// Available options:
FontFamily::Default    // System default sans-serif
FontFamily::Monospace  // System monospace font
```

Note: `FontFamily::Custom` was removed because Iced's `Font::with_name()` requires a `&'static str`.

### Use in Widgets

When implementing custom widgets that need exact text sizing:

1. **Don't use character count approximations** - always measure actual text width
2. **Consider caching** if measuring the same text repeatedly in layout passes
3. **Respect UTF-8 boundaries** when truncating (see `display_name_for_width` in `branch_label.rs`)

### Example: Branch Label Truncation

`branch_label.rs` uses the service for exact truncation:

```rust
fn display_name_for_width(name: &str, max_width: f32) -> String {
    let text_service = TextMeasurementService::new();

    // Measure full text
    let full_result = text_service.measure_single_line(name, FontFamily::Default, theme::FONT_SM as f32);
    if full_result.width <= max_width {
        return name.to_string();  // Fits entirely
    }

    // Reserve space for ellipsis
    let ellipsis_result = text_service.measure_single_line("…", FontFamily::Default, theme::FONT_SM as f32);
    let available_width = max_width - ellipsis_result.width;

    // Binary search to find truncation point
    // ... (see implementation for full details)
}
```

### Key Principles

1. **Exact pixels**: Measurements are precise down to a single pixel
2. **No approximations**: Never use `AVG_CHAR_WIDTH` or similar estimates
3. **Font-aware**: Different font families produce different measurements
4. **Thread-safe**: The service acquires Iced's global font system lock internally

### Implementation Details

The service uses Iced's advanced graphics API:
- `graphics_text::font_system()` - Access to the global `cosmic_text::FontSystem`
- `cosmic_text::Buffer` - Performs text layout and shaping
- `graphics_text::measure()` - Returns exact `Size { width, height }`

Text shaping is always "Advanced" to properly handle complex scripts and font features.

## Plugins

Plugins are dynamically loaded at startup from the `plugins/` directory. Each plugin is a directory with:
- `plugin.toml` — Metadata (id, name, version, api version)
- `init.lua` — Plugin code executed in a Lua runtime

See `src/plugin/` for the host implementation.

### Regions, panes, and slots

Plugins can inject UI widgets into three plugin-extensible regions:

| Region        | Type    | Subdivisions                                                                          |
| ------------- | ------- | ------------------------------------------------------------------------------------- |
| `main_bar`    | chrome  | `section = "left" / "center" / "right"`                                               |
| `tab_bar`     | chrome  | `section = "left" / "right"`                                                          |
| `repository`  | content | `pane = "sidebar" / "graph" / "details"`, each with `section = "top" / "bottom"`      |

Every contribution is a slot: a widget tree with an `id`, `priority`,
optional `on_click`, and an addressing `(region, [pane], section)` triple.

```lua
leviathan.ui.regions.add_slot {
  region = "main_bar",
  section = "right",
  id = "plugin.<your-id>.<button-id>",
  priority = 10,
  widget = { kind = "text", value = "hello" },
  on_click = function(slot_id) ... end,
}

leviathan.ui.regions.add_slot {
  region = "repository",
  pane = "sidebar",
  section = "top",
  id = "plugin.<your-id>.<slot-id>",
  priority = 10,
  widget = {
    kind = "padding",
    top = 6, right = 8, bottom = 6, left = 8,
    child = { kind = "text", value = "banner" },
  },
}
```

Style rule: padding is always its own widget (`kind = "padding"`). No widget
carries a `padding` field of its own — wrap it in a padding widget instead.

Use `leviathan.ui.regions.remove_slot { region, section?, pane?, id }` and
`leviathan.ui.regions.replace_slot({ region, section?, pane?, id }, spec)`
to mutate previously-registered slots.

The legacy `leviathan.ui.main_bar.{add,remove,replace}` API still works
and is equivalent to the corresponding `regions.*` call with `region = "main_bar"`.