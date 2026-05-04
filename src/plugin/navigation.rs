#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginNavigationEffect {
    NavigateRepository,
    OpenScreen {
        plugin_id: String,
        screen_id: String,
    },
}
