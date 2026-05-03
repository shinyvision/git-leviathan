use super::{title_case_id, DynResult};

#[derive(Debug, Clone, Copy)]
pub(super) enum Template {
    MainBar,
    SidebarPanel,
    CommandKeymap,
    GraphDecoration,
    DiffDecoration,
    ServiceProvider,
    LazyLoaded,
}

impl Template {
    pub(super) fn by_name(name: &str) -> DynResult<Self> {
        match name {
            "main-bar" | "main_bar" => Ok(Self::MainBar),
            "repository-sidebar" | "sidebar-panel" | "sidebar_panel" => Ok(Self::SidebarPanel),
            "command-keymap" | "command_keymap" => Ok(Self::CommandKeymap),
            "graph-decoration" | "graph_decoration" => Ok(Self::GraphDecoration),
            "diff-decoration" | "diff_decoration" => Ok(Self::DiffDecoration),
            "service-provider" | "service_provider" => Ok(Self::ServiceProvider),
            "lazy-loaded" | "lazy_loaded" => Ok(Self::LazyLoaded),
            _ => Err(format!(
                "unknown template `{name}`; expected one of: main-bar, repository-sidebar, command-keymap, graph-decoration, diff-decoration, service-provider, lazy-loaded"
            )
            .into()),
        }
    }

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::MainBar => "main-bar",
            Self::SidebarPanel => "repository-sidebar",
            Self::CommandKeymap => "command-keymap",
            Self::GraphDecoration => "graph-decoration",
            Self::DiffDecoration => "diff-decoration",
            Self::ServiceProvider => "service-provider",
            Self::LazyLoaded => "lazy-loaded",
        }
    }

    pub(super) fn manifest(self, id: &str) -> String {
        let capabilities = match self {
            Self::GraphDecoration => "capabilities = [\"ui:graph_decoration\"]\n",
            Self::DiffDecoration => "capabilities = [\"ui:diff_decoration\"]\n",
            _ => "",
        };
        let services = match self {
            Self::ServiceProvider => "provides_services = [\"sample_status@1\"]\n",
            _ => "",
        };
        let activation = match self {
            Self::LazyLoaded => format!(
                "\n[activation]\ncommands = [\"{id}.show\"]\nregions = [\"main_bar\"]\nmanual = false\n"
            ),
            _ => String::new(),
        };
        format!(
            "id = \"{id}\"\nname = \"{}\"\nversion = \"0.1.0\"\napi_version = \"1.0\"\ndescription = \"Generated Git Leviathan plugin.\"\n{capabilities}{services}{activation}",
            title_case_id(id)
        )
    }

    pub(super) fn init_lua(self, id: &str) -> String {
        match self {
            Self::MainBar => format!(
                r#"local slot_id = "plugin.{id}.main"

leviathan.ui.regions.add_slot({{
  region = "main_bar",
  section = "right",
  id = slot_id,
  priority = 100,
  widget = {{
    kind = "button",
    on_click = "{id}.hello",
    child = {{
      kind = "text",
      value = "{id}",
    }},
  }},
}})

leviathan.command.create("{id}.hello", {{
  title = "{id}: Hello",
  context = "global",
  run = function()
    leviathan.log("{id} command ran")
  end,
}})
"#
            ),
            Self::SidebarPanel => format!(
                r#"leviathan.ui.regions.add_slot({{
  region = "repository",
  pane = "sidebar",
  section = "top",
  id = "plugin.{id}.sidebar",
  priority = 100,
  widget = {{
    kind = "padding",
    top = 8,
    right = 8,
    bottom = 8,
    left = 8,
    child = {{
      kind = "column",
      spacing = 6,
      children = {{
        {{ kind = "text", value = "{id}", size = 13 }},
        {{ kind = "text", value = "Repository sidebar panel", size = 12 }},
      }},
    }},
  }},
}})
"#
            ),
            Self::CommandKeymap => format!(
                r#"leviathan.command.create("{id}.refresh", {{
  title = "{id}: Refresh",
  description = "Run the generated command.",
  context = "repository",
  run = function()
    leviathan.log("{id}.refresh")
  end,
}})

leviathan.keymap.set("repository", "<leader>r", "{id}.refresh", {{
  description = "{id}: refresh",
}})
"#
            ),
            Self::GraphDecoration => format!(
                r##"leviathan.ui.graph_decoration("HEAD", {{
  kind = "badge",
  id = "plugin.{id}.head_badge",
  label = "{id}",
  color = "#66D9EF",
  priority = 100,
}})
"##
            ),
            Self::DiffDecoration => format!(
                r#"leviathan.ui.diff_decoration({{
  kind = "line_hint",
  id = "plugin.{id}.hint",
  file = "README.md",
  line = 1,
  text = "{id}",
  priority = 100,
}})
"#
            ),
            Self::ServiceProvider => format!(
                r#"leviathan.services.register("sample_status", 1, {{
  label = function()
    return "{id} ready"
  end,
}})
"#
            ),
            Self::LazyLoaded => format!(
                r#"leviathan.command.create("{id}.show", {{
  title = "{id}: Show",
  context = "global",
  run = function()
    leviathan.log("{id} activated")
  end,
}})

leviathan.ui.regions.add_slot({{
  region = "main_bar",
  section = "right",
  id = "plugin.{id}.lazy",
  priority = 100,
  widget = {{
    kind = "button",
    on_click = "{id}.show",
    child = {{ kind = "text", value = "{id}" }},
  }},
}})
"#
            ),
        }
    }

    pub(super) fn readme(self, id: &str) -> String {
        format!(
            "# {}\n\nGenerated `{}` template for Git Leviathan.\n\n## Test\n\nRun `cargo run -p xtask -- plugin test {}` from the repository root.\n",
            title_case_id(id),
            self.name(),
            id
        )
    }

    pub(super) fn test_lua(self, id: &str) -> String {
        let command = match self {
            Self::CommandKeymap => format!(r#"assert(leviathan.command.invoke("{id}.refresh"))"#),
            Self::LazyLoaded => format!(r#"assert(leviathan.command.invoke("{id}.show"))"#),
            Self::MainBar => format!(r#"assert(leviathan.command.invoke("{id}.hello"))"#),
            _ => "assert(leviathan ~= nil)".to_string(),
        };
        format!("{command}\n")
    }
}
