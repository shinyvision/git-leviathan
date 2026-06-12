use super::*;

impl PluginHost {
    /// Drain every plugin's deferred-call queue. Order per plugin:
    ///
    /// 1. Immediate (`leviathan.api.schedule(fn)`) callbacks, in FIFO order.
    /// 2. Delayed (`defer_fn(ms, fn)`) callbacks whose deadline is now in
    ///    the past.
    /// 3. Resumable coroutines parked from a previous
    ///    `invoke_user_command` or earlier tick. Coroutines that yield
    ///    again are re-parked for the next tick.
    ///
    /// Errors from any callback / resume are logged; other queue entries
    /// keep processing so one buggy callback can't stall a plugin.
    pub fn tick(&mut self) {
        // Drain any `CommandExecuted` events queued by Lua-
        // initiated dispatch since the last tick. The Rust entry
        // (`invoke_command`) flushes synchronously; this catches
        // anything the Lua API queued in between.
        self.flush_pending_command_events();
        // Drain any typed events the git Lua API queued so
        // `HeadChanged` / `RefsChanged` / etc. fire on the next tick
        // even if the plugin invoked the op via `tick`-deferred Lua.
        self.flush_pending_git_events();
        let terminal_changed = crate::plugin::terminal::registry().drain();
        if !terminal_changed.is_empty() {
            let only = terminal_changed.into_iter().collect::<HashSet<_>>();
            self.invalidate_dynamic_widgets(
                &[crate::plugin::ui::invalidation::UiInvalidationCause::PluginStateChanged],
                Some(&only),
            );
        }
        let now = Instant::now();
        let ids: Vec<String> = self.plugins.keys().cloned().collect();
        let mut pending: Vec<PluginDiagnostic> = Vec::new();
        let mut deferred_state_changed: HashSet<String> = HashSet::new();
        for id in ids {
            let Some(plugin) = self.plugins.get(&id) else {
                continue;
            };
            let lua = plugin.lua_rc();
            let queue = Rc::clone(&plugin.deferred);
            let ledger = plugin.ledger();
            let generation_id = plugin.generation.generation_id;
            let chunk_name = format!("plugins/{id}/init.lua");

            let immediate = queue.borrow_mut().drain_immediate();
            for callback in immediate {
                match lua.registry_value::<Function>(&callback.key) {
                    Ok(f) => {
                        deferred_state_changed.insert(id.clone());
                        self.bump_lua_activity();
                        if let Err(e) = f.call::<()>(()) {
                            pending.push(
                                PluginDiagnostic::new(
                                    PluginId::from(id.clone()),
                                    DiagnosticSeverity::Error,
                                    "lua.callback_error",
                                    "scheduled fn error".to_string(),
                                )
                                .with_generation(generation_id)
                                .with_mlua_error(&chunk_name, &e),
                            );
                        }
                    }
                    Err(e) => pending.push(
                        PluginDiagnostic::new(
                            PluginId::from(id.clone()),
                            DiagnosticSeverity::Error,
                            "lua.handler_lookup_failed",
                            format!("scheduled fn lookup failed: {e}"),
                        )
                        .with_generation(generation_id)
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: "leviathan.api.schedule".into(),
                        }),
                    ),
                }
                ledger.remove_resource(callback.resource_id);
            }

            let due = queue.borrow_mut().drain_due(now);
            for callback in due {
                match lua.registry_value::<Function>(&callback.key) {
                    Ok(f) => {
                        deferred_state_changed.insert(id.clone());
                        self.bump_lua_activity();
                        if let Err(e) = f.call::<()>(()) {
                            pending.push(
                                PluginDiagnostic::new(
                                    PluginId::from(id.clone()),
                                    DiagnosticSeverity::Error,
                                    "lua.callback_error",
                                    "defer_fn error".to_string(),
                                )
                                .with_generation(generation_id)
                                .with_mlua_error(&chunk_name, &e),
                            );
                        }
                    }
                    Err(e) => pending.push(
                        PluginDiagnostic::new(
                            PluginId::from(id.clone()),
                            DiagnosticSeverity::Error,
                            "lua.handler_lookup_failed",
                            format!("defer_fn lookup failed: {e}"),
                        )
                        .with_generation(generation_id)
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: "leviathan.api.defer_fn".into(),
                        }),
                    ),
                }
                ledger.remove_resource(callback.resource_id);
            }

            let drained: Vec<DeferredCallback> = std::mem::take(&mut queue.borrow_mut().coroutines);
            for callback in drained {
                let thread: Thread = match lua.registry_value(&callback.key) {
                    Ok(t) => t,
                    Err(e) => {
                        pending.push(
                            PluginDiagnostic::new(
                                PluginId::from(id.clone()),
                                DiagnosticSeverity::Error,
                                "lua.handler_lookup_failed",
                                format!("coroutine lookup failed: {e}"),
                            )
                            .with_generation(generation_id)
                            .with_source(
                                PluginSourceSpan::ApiFunction {
                                    name: "leviathan.api.coroutine".into(),
                                },
                            ),
                        );
                        ledger.remove_resource(callback.resource_id);
                        continue;
                    }
                };
                deferred_state_changed.insert(id.clone());
                self.bump_lua_activity();
                if let Err(e) = thread.resume::<()>(()) {
                    pending.push(
                        PluginDiagnostic::new(
                            PluginId::from(id.clone()),
                            DiagnosticSeverity::Error,
                            "lua.callback_error",
                            "coroutine resume error".to_string(),
                        )
                        .with_generation(generation_id)
                        .with_mlua_error(&chunk_name, &e),
                    );
                    ledger.remove_resource(callback.resource_id);
                    continue;
                }
                ledger.remove_resource(callback.resource_id);
                if thread.status() == ThreadStatus::Resumable {
                    match lua.create_registry_value(thread) {
                        Ok(new_key) => {
                            let resource_id =
                                ledger.record(PluginResourceKind::AsyncJob, "coroutine", None);
                            ledger.record(
                                PluginResourceKind::LuaRegistryKey,
                                format!("coroutine:{resource_id}"),
                                None,
                            );
                            queue.borrow_mut().coroutines.push(DeferredCallback {
                                key: new_key,
                                resource_id,
                            });
                        }
                        Err(e) => pending.push(
                            PluginDiagnostic::new(
                                PluginId::from(id.clone()),
                                DiagnosticSeverity::Error,
                                "host.coroutine_repark_failed",
                                format!("re-parking coroutine failed: {e}"),
                            )
                            .with_generation(generation_id)
                            .with_source(
                                PluginSourceSpan::ApiFunction {
                                    name: "leviathan.api.coroutine".into(),
                                },
                            ),
                        ),
                    }
                }
            }
        }
        for diag in pending {
            self.diagnostics.record(diag);
        }
        if !deferred_state_changed.is_empty() {
            self.invalidate_dynamic_widgets(
                &[UiInvalidationCause::PluginStateChanged],
                Some(&deferred_state_changed),
            );
        }

        // async runtime: drain finished async jobs, due timers, and queued
        // file-watcher events. Each fires a Lua callback on the
        // matching plugin's main state.
        self.drive_async_runtime(now);
    }

    /// async runtime: invoke plugin Lua callbacks for finished async jobs,
    /// due timers, and buffered file-watcher events. Errors are
    /// recorded as diagnostics; one buggy callback can't stall the
    /// next.
    pub(super) fn drive_async_runtime(&mut self, now: Instant) {
        let mut pending: Vec<PluginDiagnostic> = Vec::new();
        let mut state_causes: HashMap<String, Vec<UiInvalidationCause>> = HashMap::new();

        // Async jobs.
        for job in self.async_jobs.drain_finished() {
            let Some(plugin) = self.plugins.get(job.plugin_id.as_str()) else {
                continue;
            };
            if plugin.generation.generation_id != job.generation_id {
                // Stale generation — skip, the new gen owns its own
                // jobs.
                continue;
            }
            plugin.ledger().remove_resource(job.resource_id);
            let key_opt = plugin.job_callbacks.borrow_mut().remove(job.job_id);
            let Some(key) = key_opt else { continue };
            let lua = plugin.lua_rc();
            let func: Function = match lua.registry_value(&key) {
                Ok(f) => f,
                Err(e) => {
                    pending.push(
                        PluginDiagnostic::new(
                            job.plugin_id.clone(),
                            DiagnosticSeverity::Error,
                            "lua.handler_lookup_failed",
                            format!("async on_complete lookup failed: {e}"),
                        )
                        .with_generation(job.generation_id)
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: "leviathan.async.spawn".into(),
                        }),
                    );
                    continue;
                }
            };
            let chunk_name = format!("plugins/{}/init.lua", job.plugin_id.as_str());
            // Budget the async on-complete callback against
            // the `AsyncJob` budget. The async body itself runs off
            // the main thread, but on_complete is the only Lua-side
            // step the host invokes synchronously, so it's the
            // user-observable cost.
            let callback_id = format!("async:on_complete:{}", job.job_id.get());
            self.bump_lua_activity();
            let perf_outcome = self.budget_tracker.track_call::<(), mlua::Error>(
                CallbackKind::AsyncJob,
                &job.plugin_id,
                job.generation_id,
                &callback_id,
                || match job.outcome {
                    JobOutcome::Ok(value) => {
                        let lua_value = lua.to_value(&value).unwrap_or(mlua::Value::Nil);
                        func.call::<()>((true, lua_value))
                    }
                    JobOutcome::Cancelled => func.call::<()>((false, "cancelled")),
                    JobOutcome::Failed(msg) => func.call::<()>((false, msg)),
                },
            );
            match perf_outcome {
                PerfOutcome::Ok(()) => push_ui_cause(
                    &mut state_causes,
                    job.plugin_id.as_str(),
                    UiInvalidationCause::JobCallbackChangedPluginState,
                ),
                PerfOutcome::Err(e) => {
                    push_ui_cause(
                        &mut state_causes,
                        job.plugin_id.as_str(),
                        UiInvalidationCause::JobCallbackChangedPluginState,
                    );
                    pending.push(
                        PluginDiagnostic::new(
                            job.plugin_id.clone(),
                            DiagnosticSeverity::Error,
                            "lua.callback_error",
                            "async on_complete error".to_string(),
                        )
                        .with_generation(job.generation_id)
                        .with_mlua_error(&chunk_name, &e),
                    );
                }
                PerfOutcome::Skipped => {}
            }
        }

        // Timers.
        let due_timers = self.timers.drain_due(now);
        for due in due_timers {
            let Some(plugin) = self.plugins.get(due.plugin_id.as_str()) else {
                continue;
            };
            if plugin.generation.generation_id != due.generation_id {
                continue;
            }
            let lua = plugin.lua_rc();
            let chunk_name = format!("plugins/{}/init.lua", due.plugin_id.as_str());
            let func_opt: Option<Function> = match due.kind {
                crate::plugin::timers::TimerKind::After => {
                    let key_opt = plugin.timer_callbacks.borrow_mut().remove(due.timer_id);
                    plugin.ledger().remove_resource(due.resource_id);
                    key_opt.and_then(|k| lua.registry_value::<Function>(&k).ok())
                }
                crate::plugin::timers::TimerKind::Every => plugin
                    .timer_callbacks
                    .borrow()
                    .get(due.timer_id)
                    .and_then(|k| lua.registry_value::<Function>(k).ok()),
            };
            let Some(func) = func_opt else { continue };
            // Time the timer callback against the `Timer`
            // budget. The callback id is the timer kind + id so each
            // timer's stats roll up independently.
            let callback_id = format!("timer:{}:{}", due.kind.as_str(), due.timer_id.get());
            self.bump_lua_activity();
            let perf_outcome = self.budget_tracker.track_call::<(), mlua::Error>(
                CallbackKind::Timer,
                &due.plugin_id,
                due.generation_id,
                &callback_id,
                || func.call::<()>(()),
            );
            match perf_outcome {
                PerfOutcome::Ok(()) => push_ui_cause(
                    &mut state_causes,
                    due.plugin_id.as_str(),
                    UiInvalidationCause::TimerCallbackChangedPluginState,
                ),
                PerfOutcome::Err(e) => {
                    push_ui_cause(
                        &mut state_causes,
                        due.plugin_id.as_str(),
                        UiInvalidationCause::TimerCallbackChangedPluginState,
                    );
                    pending.push(
                        PluginDiagnostic::new(
                            due.plugin_id.clone(),
                            DiagnosticSeverity::Error,
                            "lua.callback_error",
                            format!("timer.{} callback error", due.kind.as_str()),
                        )
                        .with_generation(due.generation_id)
                        .with_mlua_error(&chunk_name, &e),
                    );
                }
                PerfOutcome::Skipped => {}
            }
        }

        // File watchers.
        let events = self.watchers.drain_events();
        for ev in events {
            let Some(plugin) = self.plugins.get(ev.plugin_id.as_str()) else {
                continue;
            };
            if plugin.generation.generation_id != ev.generation_id {
                continue;
            }
            let lua = plugin.lua_rc();
            let func: Option<Function> = {
                let callbacks = plugin.watcher_callbacks.borrow();
                callbacks
                    .get(ev.watch_id)
                    .and_then(|k| lua.registry_value::<Function>(k).ok())
            };
            let Some(func) = func else { continue };
            let event_table = match build_watch_event_table(&lua, &ev.event) {
                Ok(t) => t,
                Err(e) => {
                    pending.push(
                        PluginDiagnostic::new(
                            ev.plugin_id.clone(),
                            DiagnosticSeverity::Error,
                            "lua.watch_event_build_failed",
                            format!("watch event table build failed: {e}"),
                        )
                        .with_generation(ev.generation_id)
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: "leviathan.fs.watch".into(),
                        }),
                    );
                    continue;
                }
            };
            let chunk_name = format!("plugins/{}/init.lua", ev.plugin_id.as_str());
            self.bump_lua_activity();
            let perf_outcome = self.budget_tracker.track_call::<(), mlua::Error>(
                CallbackKind::EventCallback,
                &ev.plugin_id,
                ev.generation_id,
                &format!("fs.watch:{}", ev.watch_id.get()),
                || func.call::<()>(event_table),
            );
            match perf_outcome {
                PerfOutcome::Ok(()) => push_ui_cause(
                    &mut state_causes,
                    ev.plugin_id.as_str(),
                    UiInvalidationCause::WatchCallbackChangedPluginState,
                ),
                PerfOutcome::Err(e) => {
                    push_ui_cause(
                        &mut state_causes,
                        ev.plugin_id.as_str(),
                        UiInvalidationCause::WatchCallbackChangedPluginState,
                    );
                    pending.push(
                        PluginDiagnostic::new(
                            ev.plugin_id.clone(),
                            DiagnosticSeverity::Error,
                            "lua.callback_error",
                            "fs.watch callback error".to_string(),
                        )
                        .with_generation(ev.generation_id)
                        .with_mlua_error(&chunk_name, &e),
                    );
                }
                PerfOutcome::Skipped => {}
            }
        }

        for diag in pending {
            self.diagnostics.record(diag);
        }
        for (plugin_id, causes) in state_causes {
            let mut only = HashSet::new();
            only.insert(plugin_id);
            self.invalidate_dynamic_widgets(&causes, Some(&only));
        }
    }

    /// Invoke a plugin's named user command. The function is wrapped in
    /// a Lua coroutine so cooperative yields are honoured: if the command
    /// yields, it's parked in the plugin's `coroutines` bucket and
    /// resumed on subsequent `tick` calls. Returns once the first resume
    /// finishes (either completed or yielded).
    pub fn invoke_user_command(&mut self, plugin_id: &str, name: &str) -> mlua::Result<()> {
        self.bump_lua_activity();
        let plugin = self
            .plugins
            .get(plugin_id)
            .ok_or_else(|| mlua::Error::external(format!("plugin '{plugin_id}' not loaded")))?;
        let f: Function = {
            let cmds = plugin.user_commands.borrow();
            let key = cmds.commands.get(name).ok_or_else(|| {
                mlua::Error::external(format!("user command '{name}' not registered"))
            })?;
            plugin.lua().registry_value(key)?
        };
        let thread = plugin.lua().create_thread(f)?;
        thread.resume::<()>(())?;
        if thread.status() == ThreadStatus::Resumable {
            let key = plugin.lua().create_registry_value(thread)?;
            let ledger = plugin.ledger();
            let resource_id = ledger.record(PluginResourceKind::AsyncJob, "user_command", None);
            ledger.record(
                PluginResourceKind::LuaRegistryKey,
                format!("user_command:{resource_id}"),
                None,
            );
            plugin
                .deferred
                .borrow_mut()
                .coroutines
                .push(DeferredCallback { key, resource_id });
        }
        Ok(())
    }
}

fn push_ui_cause(
    map: &mut HashMap<String, Vec<UiInvalidationCause>>,
    plugin_id: &str,
    cause: UiInvalidationCause,
) {
    let entry = map.entry(plugin_id.to_string()).or_default();
    if !entry.contains(&cause) {
        entry.push(cause);
    }
}
