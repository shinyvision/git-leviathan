//! Phase 15: plugin dependency resolver.
//!
//! Walks a set of [`PluginManifest`]s, validates the version requirements
//! declared in `dependencies` (required) and `optional_dependencies`
//! (optional), detects cycles and conflicts, and produces a
//! deterministic load order.
//!
//! Deterministic ordering is achieved by:
//! 1. Sorting input manifests by id before walking.
//! 2. Within each topological "level" (no remaining unresolved deps),
//!    sorting candidates by id before emitting them.
//!
//! The resolver is pure — it never touches disk and never records
//! diagnostics. Callers (the host) consume [`ResolutionReport`] and
//! decide which diagnostics to record.
//!
//! Required-dep failures (missing, conflict, cycle) propagate to every
//! transitive consumer: a plugin that depends on a blocked plugin is
//! also marked blocked. Optional misses just produce a warning entry
//! and the consumer still loads.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use git_leviathan_plugin_api::manifest::PluginManifest;
use semver::{Version, VersionReq};

/// One row in the dependency graph projection. Phase 15 devtools row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencySummary {
    pub consumer_plugin_id: String,
    pub dependency_id: String,
    pub requirement: String,
    pub resolved_version: Option<String>,
    /// `"required"` or `"optional"`.
    pub kind: &'static str,
    /// `"resolved"`, `"missing"`, `"conflict"`.
    pub status: &'static str,
}

/// Reason a plugin was blocked from loading by the resolver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockReason {
    /// Required dep id is not present in the manifest set.
    MissingRequired {
        dependency_id: String,
        requirement: String,
    },
    /// Required dep is present but its version doesn't satisfy the requirement.
    Conflict {
        dependency_id: String,
        requirement: String,
        actual_version: String,
    },
    /// Plugin participates in a cycle. `cycle` lists the ids in cycle
    /// order (starting at the plugin owning this reason).
    Cycle { cycle: Vec<String> },
    /// A required transitive dependency is itself blocked. The host
    /// records this as a follow-on diagnostic so users see the chain.
    BlockedTransitive { dependency_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionalMiss {
    pub consumer_plugin_id: String,
    pub dependency_id: String,
    pub requirement: String,
    pub reason: OptionalMissReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionalMissReason {
    NotPresent,
    VersionMismatch { actual_version: String },
}

/// Output of [`resolve`]. `load_order` lists every loadable plugin's id
/// in the order the host should call `load_plugin` on. Blocked plugins
/// appear in `blocked` keyed by id, and `optional_misses` records every
/// optional dep that wasn't satisfied (so the host can emit warnings).
#[derive(Debug, Clone, Default)]
pub struct ResolutionReport {
    pub load_order: Vec<String>,
    pub blocked: BTreeMap<String, Vec<BlockReason>>,
    pub optional_misses: Vec<OptionalMiss>,
    pub graph: Vec<DependencySummary>,
}

/// Resolve dependencies for the given manifest set. The result is
/// independent of input ordering — the same input always yields the
/// same `load_order`.
pub fn resolve(manifests: &[PluginManifest]) -> ResolutionReport {
    let mut by_id: BTreeMap<String, &PluginManifest> = BTreeMap::new();
    for m in manifests {
        by_id.insert(m.id.clone(), m);
    }

    // 1) Detect cycles among required dependencies first. A cycle
    //    blocks every plugin in it; we still want to surface optional
    //    misses for plugins outside the cycle.
    let cycles = detect_cycles(&by_id);
    let mut blocked: BTreeMap<String, Vec<BlockReason>> = BTreeMap::new();
    for cycle in &cycles {
        for id in cycle {
            blocked
                .entry(id.clone())
                .or_default()
                .push(BlockReason::Cycle {
                    cycle: cycle.clone(),
                });
        }
    }

    // 2) Required-dep validation: missing or version mismatch.
    for (id, manifest) in &by_id {
        for (dep_id, req) in &manifest.dependencies {
            match by_id.get(dep_id) {
                None => {
                    blocked
                        .entry(id.clone())
                        .or_default()
                        .push(BlockReason::MissingRequired {
                            dependency_id: dep_id.clone(),
                            requirement: req.to_string(),
                        });
                }
                Some(dep) => {
                    if !satisfies(req, &dep.version) {
                        blocked
                            .entry(id.clone())
                            .or_default()
                            .push(BlockReason::Conflict {
                                dependency_id: dep_id.clone(),
                                requirement: req.to_string(),
                                actual_version: dep.version.to_string(),
                            });
                    }
                }
            }
        }
    }

    // 3) Propagate "blocked" through required deps. If `a` requires
    //    `b` and `b` is blocked, mark `a` blocked too with a
    //    `BlockedTransitive` reason. Iterate to a fixed point.
    propagate_block(&by_id, &mut blocked);

    // 4) Optional miss detection — only emit if the consumer itself is
    //    not blocked. A blocked consumer's optional misses don't help
    //    a user diagnose anything that isn't already reported.
    let mut optional_misses: Vec<OptionalMiss> = Vec::new();
    for (id, manifest) in &by_id {
        if blocked.contains_key(id) {
            continue;
        }
        let mut entries: Vec<(&String, &VersionReq)> =
            manifest.optional_dependencies.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        for (dep_id, req) in entries {
            match by_id.get(dep_id) {
                None => optional_misses.push(OptionalMiss {
                    consumer_plugin_id: id.clone(),
                    dependency_id: dep_id.clone(),
                    requirement: req.to_string(),
                    reason: OptionalMissReason::NotPresent,
                }),
                Some(dep) if !satisfies(req, &dep.version) => {
                    optional_misses.push(OptionalMiss {
                        consumer_plugin_id: id.clone(),
                        dependency_id: dep_id.clone(),
                        requirement: req.to_string(),
                        reason: OptionalMissReason::VersionMismatch {
                            actual_version: dep.version.to_string(),
                        },
                    });
                }
                _ => {} // present and version-compatible — counts as resolved
            }
        }
    }

    // 5) Topological sort over the remaining (non-blocked) plugins,
    //    sorting by id at every level so the order is deterministic.
    let load_order = topo_sort(&by_id, &blocked);

    // 6) Build flat dependency graph projection used by devtools.
    let graph = build_graph(&by_id, &blocked);

    ResolutionReport {
        load_order,
        blocked,
        optional_misses,
        graph,
    }
}

fn satisfies(req: &VersionReq, version: &Version) -> bool {
    req.matches(version)
}

/// Find every simple cycle among required dependencies. Returns a list
/// of cycles where each cycle is the ordered list of node ids. Strongly
/// connected components of size >= 2 (or a self-loop) qualify.
fn detect_cycles(by_id: &BTreeMap<String, &PluginManifest>) -> Vec<Vec<String>> {
    // Tarjan's SCC over the required-dep graph. Iterate in id order so
    // the resulting cycles are deterministic.
    let nodes: Vec<&str> = by_id.keys().map(String::as_str).collect();
    let index_of: HashMap<&str, usize> = nodes.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let n = nodes.len();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, id) in nodes.iter().enumerate() {
        if let Some(m) = by_id.get(*id) {
            let mut edges: Vec<&str> = m
                .dependencies
                .keys()
                .filter_map(|dep| index_of.get(dep.as_str()).map(|_| dep.as_str()))
                .collect();
            edges.sort();
            for e in edges {
                adj[i].push(index_of[e]);
            }
        }
    }

    let mut idx = 0usize;
    let mut stack: Vec<usize> = Vec::new();
    let mut on_stack: Vec<bool> = vec![false; n];
    let mut indices: Vec<i64> = vec![-1; n];
    let mut lowlink: Vec<i64> = vec![-1; n];
    let mut sccs: Vec<Vec<usize>> = Vec::new();

    fn strongconnect(
        v: usize,
        idx: &mut usize,
        stack: &mut Vec<usize>,
        on_stack: &mut [bool],
        indices: &mut [i64],
        lowlink: &mut [i64],
        adj: &[Vec<usize>],
        sccs: &mut Vec<Vec<usize>>,
    ) {
        indices[v] = *idx as i64;
        lowlink[v] = *idx as i64;
        *idx += 1;
        stack.push(v);
        on_stack[v] = true;
        for &w in &adj[v] {
            if indices[w] == -1 {
                strongconnect(w, idx, stack, on_stack, indices, lowlink, adj, sccs);
                lowlink[v] = lowlink[v].min(lowlink[w]);
            } else if on_stack[w] {
                lowlink[v] = lowlink[v].min(indices[w]);
            }
        }
        if lowlink[v] == indices[v] {
            let mut scc = Vec::new();
            loop {
                let w = stack.pop().expect("stack");
                on_stack[w] = false;
                scc.push(w);
                if w == v {
                    break;
                }
            }
            sccs.push(scc);
        }
    }

    for v in 0..n {
        if indices[v] == -1 {
            strongconnect(
                v,
                &mut idx,
                &mut stack,
                &mut on_stack,
                &mut indices,
                &mut lowlink,
                &adj,
                &mut sccs,
            );
        }
    }

    let mut cycles = Vec::new();
    for scc in sccs {
        if scc.len() > 1 {
            let mut ids: Vec<String> = scc.iter().map(|&i| nodes[i].to_string()).collect();
            ids.sort();
            cycles.push(ids);
        } else {
            // self-loop?
            let v = scc[0];
            if adj[v].iter().any(|&w| w == v) {
                cycles.push(vec![nodes[v].to_string()]);
            }
        }
    }
    cycles.sort();
    cycles
}

fn propagate_block(
    by_id: &BTreeMap<String, &PluginManifest>,
    blocked: &mut BTreeMap<String, Vec<BlockReason>>,
) {
    loop {
        let mut added = false;
        for (id, manifest) in by_id {
            if blocked.contains_key(id) {
                continue;
            }
            let mut blockers: Vec<String> = manifest
                .dependencies
                .keys()
                .filter(|dep_id| blocked.contains_key(dep_id.as_str()))
                .cloned()
                .collect();
            blockers.sort();
            if !blockers.is_empty() {
                let entry = blocked.entry(id.clone()).or_default();
                for dep_id in blockers {
                    entry.push(BlockReason::BlockedTransitive {
                        dependency_id: dep_id,
                    });
                }
                added = true;
            }
        }
        if !added {
            break;
        }
    }
}

fn topo_sort(
    by_id: &BTreeMap<String, &PluginManifest>,
    blocked: &BTreeMap<String, Vec<BlockReason>>,
) -> Vec<String> {
    let candidates: BTreeSet<String> = by_id
        .keys()
        .filter(|id| !blocked.contains_key(id.as_str()))
        .cloned()
        .collect();

    // Compute in-degree for candidates (count required deps that are
    // also candidates). Optional deps don't constrain order — a
    // consumer can load before its optional dep, the dep just won't be
    // wired. (If we ever switch to "optional present implies ordered",
    // this is the place.)
    let mut indeg: HashMap<String, usize> = candidates.iter().map(|id| (id.clone(), 0)).collect();
    let mut rev: HashMap<String, Vec<String>> = HashMap::new();
    for id in &candidates {
        let m = by_id[id];
        for dep_id in m.dependencies.keys() {
            if candidates.contains(dep_id) {
                *indeg.get_mut(id).unwrap() += 1;
                rev.entry(dep_id.clone()).or_default().push(id.clone());
            }
        }
    }

    let mut order = Vec::with_capacity(candidates.len());
    let mut ready: BTreeSet<String> = indeg
        .iter()
        .filter_map(|(id, &d)| (d == 0).then(|| id.clone()))
        .collect();
    let mut emitted: HashSet<String> = HashSet::new();
    while !ready.is_empty() {
        // Take the smallest id at this level — ensures deterministic
        // tie-breaking.
        let id = ready.iter().next().cloned().expect("non-empty");
        ready.remove(&id);
        if !emitted.insert(id.clone()) {
            continue;
        }
        order.push(id.clone());
        if let Some(consumers) = rev.get(&id) {
            let mut sorted = consumers.clone();
            sorted.sort();
            for consumer in sorted {
                if let Some(d) = indeg.get_mut(&consumer) {
                    if *d > 0 {
                        *d -= 1;
                        if *d == 0 {
                            ready.insert(consumer);
                        }
                    }
                }
            }
        }
    }
    order
}

fn build_graph(
    by_id: &BTreeMap<String, &PluginManifest>,
    blocked: &BTreeMap<String, Vec<BlockReason>>,
) -> Vec<DependencySummary> {
    let mut rows = Vec::new();
    for (id, manifest) in by_id {
        // Required edges
        let mut req_entries: Vec<(&String, &VersionReq)> = manifest.dependencies.iter().collect();
        req_entries.sort_by(|a, b| a.0.cmp(b.0));
        for (dep_id, req) in req_entries {
            let (resolved_version, status) = match by_id.get(dep_id) {
                Some(dep) if satisfies(req, &dep.version) => (
                    Some(dep.version.to_string()),
                    if blocked.contains_key(dep_id.as_str()) {
                        "missing"
                    } else {
                        "resolved"
                    },
                ),
                Some(dep) => (Some(dep.version.to_string()), "conflict"),
                None => (None, "missing"),
            };
            rows.push(DependencySummary {
                consumer_plugin_id: id.clone(),
                dependency_id: dep_id.clone(),
                requirement: req.to_string(),
                resolved_version,
                kind: "required",
                status,
            });
        }
        // Optional edges
        let mut opt_entries: Vec<(&String, &VersionReq)> =
            manifest.optional_dependencies.iter().collect();
        opt_entries.sort_by(|a, b| a.0.cmp(b.0));
        for (dep_id, req) in opt_entries {
            let (resolved_version, status) = match by_id.get(dep_id) {
                Some(dep) if satisfies(req, &dep.version) => {
                    (Some(dep.version.to_string()), "resolved")
                }
                Some(dep) => (Some(dep.version.to_string()), "conflict"),
                None => (None, "missing"),
            };
            rows.push(DependencySummary {
                consumer_plugin_id: id.clone(),
                dependency_id: dep_id.clone(),
                requirement: req.to_string(),
                resolved_version,
                kind: "optional",
                status,
            });
        }
    }
    rows.sort_by(|a, b| {
        a.consumer_plugin_id
            .cmp(&b.consumer_plugin_id)
            .then(a.dependency_id.cmp(&b.dependency_id))
    });
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use git_leviathan_plugin_api::api_version::ApiVersion;
    use std::collections::HashMap;

    fn manifest(
        id: &str,
        version: &str,
        deps: &[(&str, &str)],
        opts: &[(&str, &str)],
    ) -> PluginManifest {
        let mut dependencies = HashMap::new();
        for (k, v) in deps {
            dependencies.insert((*k).to_string(), VersionReq::parse(v).unwrap());
        }
        let mut optional_dependencies = HashMap::new();
        for (k, v) in opts {
            optional_dependencies.insert((*k).to_string(), VersionReq::parse(v).unwrap());
        }
        PluginManifest {
            id: id.to_string(),
            name: id.to_string(),
            version: Version::parse(version).unwrap(),
            api_version: ApiVersion { major: 1, minor: 0 },
            description: None,
            capabilities: vec![],
            provides_services: vec![],
            consumes_services: vec![],
            dependencies,
            optional_dependencies,
            requires_plugins: vec![],
            runtime: Default::default(),
            activation: None,
        }
    }

    #[test]
    fn linear_chain_orders_deterministically() {
        let manifests = vec![
            manifest("c", "1.0.0", &[("b", ">=1.0")], &[]),
            manifest("b", "1.0.0", &[("a", ">=1.0")], &[]),
            manifest("a", "1.0.0", &[], &[]),
        ];
        let report = resolve(&manifests);
        assert_eq!(report.load_order, vec!["a", "b", "c"]);
        assert!(report.blocked.is_empty());
    }

    #[test]
    fn cycle_blocks_all_in_cycle() {
        let manifests = vec![
            manifest("a", "1.0.0", &[("b", ">=1.0")], &[]),
            manifest("b", "1.0.0", &[("a", ">=1.0")], &[]),
        ];
        let report = resolve(&manifests);
        assert!(report.blocked.contains_key("a"));
        assert!(report.blocked.contains_key("b"));
        assert!(!report.load_order.contains(&"a".to_string()));
    }

    #[test]
    fn missing_required_blocks_consumer() {
        let manifests = vec![manifest("a", "1.0.0", &[("missing", ">=1.0")], &[])];
        let report = resolve(&manifests);
        assert!(report.blocked.contains_key("a"));
    }

    #[test]
    fn version_mismatch_emits_conflict() {
        let manifests = vec![
            manifest("a", "1.0.0", &[("b", ">=2.0")], &[]),
            manifest("b", "1.0.0", &[], &[]),
        ];
        let report = resolve(&manifests);
        assert!(report.blocked.contains_key("a"));
        assert!(matches!(
            &report.blocked["a"][0],
            BlockReason::Conflict { .. }
        ));
    }

    #[test]
    fn optional_missing_does_not_block() {
        let manifests = vec![manifest("a", "1.0.0", &[], &[("missing", ">=1.0")])];
        let report = resolve(&manifests);
        assert!(!report.blocked.contains_key("a"));
        assert_eq!(report.optional_misses.len(), 1);
    }
}
