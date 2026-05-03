use std::rc::Rc;

use mlua::Lua;

use crate::plugin::resources::{GenerationId, PluginId, ResourceLedger};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationState {
    Active,
    Unloading,
}

pub struct PluginGeneration {
    pub plugin_id: PluginId,
    pub generation_id: GenerationId,
    pub state: GenerationState,
    pub lua: Rc<Lua>,
    pub ledger: ResourceLedger,
}

impl PluginGeneration {
    pub fn new(plugin_id: PluginId, generation_id: GenerationId, lua: Rc<Lua>) -> Self {
        let ledger = ResourceLedger::new(plugin_id.clone(), generation_id);
        Self {
            plugin_id,
            generation_id,
            state: GenerationState::Active,
            lua,
            ledger,
        }
    }

    pub fn mark_unloading(&mut self) {
        self.state = GenerationState::Unloading;
    }
}
