use super::schema::*;

pub const API_CAPABILITIES: &[ApiCapability] = &[
    ApiCapability {
        name: "fs:read",
        since: "1.0",
        doc: "Alias for fs:read:plugin.",
    },
    ApiCapability {
        name: "fs:read:plugin",
        since: "1.0",
        doc: "Read paths under the plugin directory.",
    },
    ApiCapability {
        name: "fs:read:state",
        since: "1.0",
        doc: "Read paths under the plugin state directory.",
    },
    ApiCapability {
        name: "fs:read:config",
        since: "1.0",
        doc: "Read paths under the plugin config directory.",
    },
    ApiCapability {
        name: "fs:read:workdir",
        since: "1.0",
        doc: "Read paths under the active workdir when configured.",
    },
    ApiCapability {
        name: "fs:read:any",
        since: "1.0",
        doc: "Read any host path.",
    },
    ApiCapability {
        name: "fs:read:scope:<dir>",
        since: "1.0",
        doc: "Read paths under an explicit user-chosen directory (canonicalised; symlinks escaping the scope are denied).",
    },
    ApiCapability {
        name: "fs:write:plugin",
        since: "1.0",
        doc: "Write paths under the plugin directory.",
    },
    ApiCapability {
        name: "fs:write:state",
        since: "1.0",
        doc: "Write paths under the plugin state directory.",
    },
    ApiCapability {
        name: "fs:write:config",
        since: "1.0",
        doc: "Write paths under the plugin config directory.",
    },
    ApiCapability {
        name: "fs:write:workdir",
        since: "1.0",
        doc: "Write paths under the active workdir when configured.",
    },
    ApiCapability {
        name: "fs:write:any",
        since: "1.0",
        doc: "Write any host path.",
    },
    ApiCapability {
        name: "fs:write:scope:<dir>",
        since: "1.0",
        doc: "Write paths under an explicit user-chosen directory (canonicalised; symlinks escaping the scope are denied).",
    },
    ApiCapability {
        name: "process:spawn",
        since: "1.0",
        doc: "Reserved process-spawn capability (any binary).",
    },
    ApiCapability {
        name: "process:spawn:<binary>",
        since: "1.0",
        doc: "Spawn a specific binary by basename (e.g. `process:spawn:git`).",
    },
    ApiCapability {
        name: "net:fetch",
        since: "1.0",
        doc: "Reserved network-fetch capability (any host).",
    },
    ApiCapability {
        name: "net:fetch:<domain>",
        since: "1.0",
        doc: "Fetch from a specific domain (e.g. `net:fetch:github.com`).",
    },
    ApiCapability {
        name: "clipboard",
        since: "1.0",
        doc: "Alias for clipboard read+write.",
    },
    ApiCapability {
        name: "clipboard:read",
        since: "1.0",
        doc: "Read the system clipboard.",
    },
    ApiCapability {
        name: "clipboard:write",
        since: "1.0",
        doc: "Write to the system clipboard.",
    },
    ApiCapability {
        name: "notify",
        since: "1.0",
        doc: "Surface a host notification banner.",
    },
    ApiCapability {
        name: "env",
        since: "1.0",
        doc: "Read every process environment variable.",
    },
    ApiCapability {
        name: "env:<glob>",
        since: "1.0",
        doc: "Read environment variables whose name matches the glob (e.g. `env:GIT_*`).",
    },
    ApiCapability {
        name: "credentials",
        since: "1.0",
        doc: "Read host-stored credentials.",
    },
    ApiCapability {
        name: "repo:read",
        since: "1.0",
        doc: "Observe the active repository projection (refs, head, status).",
    },
    ApiCapability {
        name: "git:read:status",
        since: "1.0",
        doc: "Read working tree status.",
    },
    ApiCapability {
        name: "git:read:log",
        since: "1.0",
        doc: "Read commit history.",
    },
    ApiCapability {
        name: "git:read:diff",
        since: "1.0",
        doc: "Read diffs between commits or against the index.",
    },
    ApiCapability {
        name: "git:read:show",
        since: "1.0",
        doc: "Read a commit's tree or a file at a commit.",
    },
    ApiCapability {
        name: "git:read:blame",
        since: "1.0",
        doc: "Read line-level blame for a tracked file.",
    },
    ApiCapability {
        name: "git:write:checkout",
        since: "1.0",
        doc: "Move HEAD to a ref or commit.",
    },
    ApiCapability {
        name: "git:write:branch",
        since: "1.0",
        doc: "Create, delete, or rename branches.",
    },
    ApiCapability {
        name: "git:write:tag",
        since: "1.0",
        doc: "Create or delete tags.",
    },
    ApiCapability {
        name: "git:write:commit",
        since: "1.0",
        doc: "Create commits.",
    },
    ApiCapability {
        name: "git:write:stash",
        since: "1.0",
        doc: "Push, pop, or drop stashes.",
    },
    ApiCapability {
        name: "git:write:reset",
        since: "1.0",
        doc: "Reset the index or working tree.",
    },
    ApiCapability {
        name: "git:write:fetch",
        since: "1.0",
        doc: "Fetch from a remote.",
    },
    ApiCapability {
        name: "git:write:push",
        since: "1.0",
        doc: "Push to a remote.",
    },
    ApiCapability {
        name: "git:write:merge",
        since: "1.0",
        doc: "Merge refs into HEAD.",
    },
    ApiCapability {
        name: "git:write:rebase",
        since: "1.0",
        doc: "Rebase HEAD.",
    },
    ApiCapability {
        name: "ui:region:<region>",
        since: "1.0",
        doc: "Restrict slot registrations to a specific region (e.g. `ui:region:repository.sidebar`).",
    },
    ApiCapability {
        name: "services:provide:<service@version>",
        since: "1.0",
        doc: "Provide a versioned service to other plugins.",
    },
    ApiCapability {
        name: "services:consume:<service@version>",
        since: "1.0",
        doc: "Consume a versioned service from another plugin.",
    },
    ApiCapability {
        name: "async:spawn",
        since: "1.12",
        doc: "Spawn a host-managed background worker thread.",
    },
    ApiCapability {
        name: "timer:create",
        since: "1.12",
        doc: "Schedule one-shot or repeating timers.",
    },
    ApiCapability {
        name: "fs:watch",
        since: "1.12",
        doc: "Watch plugin-scoped paths for filesystem events.",
    },
    ApiCapability {
        name: "fs:watch:scope:<dir>",
        since: "1.12",
        doc: "Watch paths under an explicit user-chosen directory.",
    },
    ApiCapability {
        name: "ui:overlay",
        since: "1.0",
        doc: "Register modal overlays that the host renders above the active screen.",
    },
    ApiCapability {
        name: "ui:context_menu",
        since: "1.0",
        doc: "Contribute items to host-rendered context menus at extension points.",
    },
    ApiCapability {
        name: "ui:graph_decoration",
        since: "1.0",
        doc: "Attach badges, icons, markers, or lanes to commit rows in the graph.",
    },
    ApiCapability {
        name: "ui:diff_decoration",
        since: "1.0",
        doc: "Attach line hints, hunk badges, or line gutters to the diff view.",
    },
];
