use std::collections::{HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, LazyLock, Mutex,
};

use tree_sitter::{Language, Query};

use super::util::normalize_language_key;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum QueryType {
    Highlights,
    Injections,
    Locals,
}

impl QueryType {
    fn filename(self) -> &'static str {
        match self {
            Self::Highlights => "highlights.scm",
            Self::Injections => "injections.scm",
            Self::Locals => "locals.scm",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Highlights => "highlights",
            Self::Injections => "injections",
            Self::Locals => "locals",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct QuerySources {
    highlights: Option<String>,
    injections: Option<String>,
    locals: Option<String>,
}

impl QuerySources {
    pub(super) fn set(&mut self, query_type: QueryType, source: String) {
        match query_type {
            QueryType::Highlights => self.highlights = Some(source),
            QueryType::Injections => self.injections = Some(source),
            QueryType::Locals => self.locals = Some(source),
        }
    }

    pub(super) fn get(&self, query_type: QueryType) -> Option<&str> {
        match query_type {
            QueryType::Highlights => self.highlights.as_deref(),
            QueryType::Injections => self.injections.as_deref(),
            QueryType::Locals => self.locals.as_deref(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct CompiledQueries {
    highlights: Option<Arc<Query>>,
    injections: Option<Arc<Query>>,
    identity: QuerySetIdentity,
}

impl CompiledQueries {
    pub(super) fn disabled() -> Self {
        Self::default()
    }

    pub(super) fn highlights(&self) -> Option<&Query> {
        self.highlights.as_deref()
    }

    pub(super) fn injections(&self) -> Option<&Query> {
        self.injections.as_deref()
    }

    pub(super) fn identity(&self) -> QuerySetIdentity {
        self.identity
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub(super) struct QuerySetIdentity {
    pub(super) version: u32,
    pub(super) highlights: Option<u64>,
    pub(super) injections: Option<u64>,
    pub(super) locals: Option<u64>,
}

const QUERY_IDENTITY_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct QueryBuildError {
    message: String,
}

impl QueryBuildError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub(super) fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct QueryCacheKey {
    language: String,
    query_type: QueryType,
    content_hash: u64,
}

static QUERY_CACHE: LazyLock<Mutex<HashMap<QueryCacheKey, Arc<Query>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static QUERY_CACHE_HITS: AtomicUsize = AtomicUsize::new(0);
static QUERY_CACHE_MISSES: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QueryCacheStats {
    pub hits: usize,
    pub misses: usize,
}

pub(super) fn clear_query_cache() {
    if let Ok(mut cache) = QUERY_CACHE.lock() {
        cache.clear();
    }
    QUERY_CACHE_HITS.store(0, Ordering::Relaxed);
    QUERY_CACHE_MISSES.store(0, Ordering::Relaxed);
}

#[cfg(test)]
pub(super) fn query_cache_stats() -> QueryCacheStats {
    QueryCacheStats {
        hits: QUERY_CACHE_HITS.load(Ordering::Relaxed),
        misses: QUERY_CACHE_MISSES.load(Ordering::Relaxed),
    }
}

pub(super) fn compile_query_bundle(
    language_name: &str,
    language: &Language,
    query_sources: &HashMap<String, QuerySources>,
    override_dir: Option<&Path>,
) -> Result<CompiledQueries, QueryBuildError> {
    let highlights = compile_optional_query(
        language_name,
        language,
        QueryType::Highlights,
        query_sources,
        override_dir,
    )?;
    let injections = compile_optional_query(
        language_name,
        language,
        QueryType::Injections,
        query_sources,
        override_dir,
    )?;
    let locals = compile_optional_query(
        language_name,
        language,
        QueryType::Locals,
        query_sources,
        override_dir,
    )?;

    Ok(CompiledQueries {
        highlights: highlights.query,
        injections: injections.query,
        identity: QuerySetIdentity {
            version: QUERY_IDENTITY_VERSION,
            highlights: highlights.source_hash,
            injections: injections.source_hash,
            locals: locals.source_hash,
        },
    })
}

struct OptionalCompiledQuery {
    query: Option<Arc<Query>>,
    source_hash: Option<u64>,
}

fn compile_optional_query(
    language_name: &str,
    language: &Language,
    query_type: QueryType,
    query_sources: &HashMap<String, QuerySources>,
    override_dir: Option<&Path>,
) -> Result<OptionalCompiledQuery, QueryBuildError> {
    let Some(source) =
        compose_query_source(language_name, query_type, query_sources, override_dir)?
    else {
        return Ok(OptionalCompiledQuery {
            query: None,
            source_hash: None,
        });
    };

    let source_hash = hash_query_source(&source);
    compile_query(language_name, language, query_type, &source).map(|query| OptionalCompiledQuery {
        query: Some(query),
        source_hash: Some(source_hash),
    })
}

fn compile_query(
    language_name: &str,
    language: &Language,
    query_type: QueryType,
    source: &str,
) -> Result<Arc<Query>, QueryBuildError> {
    let key = QueryCacheKey {
        language: normalize_language_key(language_name),
        query_type,
        content_hash: hash_query_source(source),
    };

    if let Ok(cache) = QUERY_CACHE.lock() {
        if let Some(query) = cache.get(&key) {
            QUERY_CACHE_HITS.fetch_add(1, Ordering::Relaxed);
            return Ok(Arc::clone(query));
        }
    }
    QUERY_CACHE_MISSES.fetch_add(1, Ordering::Relaxed);

    let query = Arc::new(Query::new(language, source).map_err(|err| {
        QueryBuildError::new(format!(
            "failed to compile Tree-sitter {} query for {}: {}",
            query_type.label(),
            language_name,
            err
        ))
    })?);

    if let Ok(mut cache) = QUERY_CACHE.lock() {
        cache.insert(key, Arc::clone(&query));
    }

    Ok(query)
}

fn compose_query_source(
    language_name: &str,
    query_type: QueryType,
    query_sources: &HashMap<String, QuerySources>,
    override_dir: Option<&Path>,
) -> Result<Option<String>, QueryBuildError> {
    let mut stack = HashSet::new();
    compose_layer(
        &normalize_language_key(language_name),
        query_type,
        query_sources,
        override_dir,
        &mut stack,
    )
}

fn compose_layer(
    language_name: &str,
    query_type: QueryType,
    query_sources: &HashMap<String, QuerySources>,
    override_dir: Option<&Path>,
    stack: &mut HashSet<(String, QueryType)>,
) -> Result<Option<String>, QueryBuildError> {
    let key = (language_name.to_string(), query_type);
    if !stack.insert(key.clone()) {
        return Err(QueryBuildError::new(format!(
            "cyclic Tree-sitter query inheritance for {} {}",
            language_name,
            query_type.label()
        )));
    }

    let result = match read_override_query(override_dir, language_name, query_type)? {
        Some(override_source) => {
            let modeline = parse_modelines(&override_source);
            let mut parts = inherited_sources(
                &modeline.inherits,
                query_type,
                query_sources,
                override_dir,
                stack,
            )?;
            if modeline.extends {
                if let Some(base) = compose_base(
                    language_name,
                    query_type,
                    query_sources,
                    override_dir,
                    stack,
                )? {
                    parts.push(base);
                }
            }
            parts.push(override_source);
            Some(join_query_parts(parts))
        }
        None => compose_base(
            language_name,
            query_type,
            query_sources,
            override_dir,
            stack,
        )?,
    };

    stack.remove(&key);
    Ok(result)
}

fn compose_base(
    language_name: &str,
    query_type: QueryType,
    query_sources: &HashMap<String, QuerySources>,
    override_dir: Option<&Path>,
    stack: &mut HashSet<(String, QueryType)>,
) -> Result<Option<String>, QueryBuildError> {
    let Some(source) = query_sources
        .get(language_name)
        .and_then(|sources| sources.get(query_type))
    else {
        return Ok(None);
    };

    let modeline = parse_modelines(source);
    let mut parts = inherited_sources(
        &modeline.inherits,
        query_type,
        query_sources,
        override_dir,
        stack,
    )?;
    parts.push(source.to_string());
    Ok(Some(join_query_parts(parts)))
}

fn inherited_sources(
    inherits: &[String],
    query_type: QueryType,
    query_sources: &HashMap<String, QuerySources>,
    override_dir: Option<&Path>,
    stack: &mut HashSet<(String, QueryType)>,
) -> Result<Vec<String>, QueryBuildError> {
    let mut parts = Vec::new();
    for inherited in inherits {
        if let Some(source) =
            compose_layer(inherited, query_type, query_sources, override_dir, stack)?
        {
            parts.push(source);
        }
    }
    Ok(parts)
}

fn read_override_query(
    override_dir: Option<&Path>,
    language_name: &str,
    query_type: QueryType,
) -> Result<Option<String>, QueryBuildError> {
    let Some(override_dir) = override_dir else {
        return Ok(None);
    };
    let path = override_query_path(override_dir, language_name, query_type);
    match fs::read_to_string(&path) {
        Ok(source) => Ok(Some(source)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(QueryBuildError::new(format!(
            "failed to read Tree-sitter query override {}: {}",
            path.display(),
            err
        ))),
    }
}

fn override_query_path(override_dir: &Path, language_name: &str, query_type: QueryType) -> PathBuf {
    override_dir
        .join(normalize_language_key(language_name))
        .join(query_type.filename())
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct QueryModeline {
    pub(super) extends: bool,
    pub(super) inherits: Vec<String>,
}

pub(super) fn parse_modelines(source: &str) -> QueryModeline {
    let mut modeline = QueryModeline::default();
    for line in source.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        if !trimmed.starts_with(';') {
            break;
        }

        let directive = trimmed.trim_start_matches(';').trim();
        if directive == "extends" {
            modeline.extends = true;
        } else if let Some(rest) = directive.strip_prefix("inherits:") {
            modeline.inherits.extend(
                rest.split([',', ' ', '\t'])
                    .map(normalize_language_key)
                    .filter(|language| !language.is_empty()),
            );
        }
    }
    modeline
}

fn join_query_parts(parts: Vec<String>) -> String {
    parts
        .into_iter()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn hash_query_source(source: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_map(entries: &[(&str, QueryType, &str)]) -> HashMap<String, QuerySources> {
        let mut sources = HashMap::new();
        for (language, query_type, source) in entries {
            sources
                .entry(normalize_language_key(language))
                .or_insert_with(QuerySources::default)
                .set(*query_type, source.to_string());
        }
        sources
    }

    #[test]
    fn parses_extends_and_inherits_modelines() {
        let modeline = parse_modelines("; extends\n; inherits: javascript, typescript\n(node)");

        assert!(modeline.extends);
        assert_eq!(modeline.inherits, vec!["javascript", "typescript"]);
    }

    #[test]
    fn composes_inherited_query_sources_deterministically() {
        let sources = source_map(&[
            ("base", QueryType::Highlights, "(identifier) @variable\n"),
            (
                "child",
                QueryType::Highlights,
                "; inherits: base\n(function_item) @function\n",
            ),
        ]);

        let composed =
            compose_query_source("child", QueryType::Highlights, &sources, None).unwrap();

        assert_eq!(
            composed.unwrap(),
            "(identifier) @variable\n\n; inherits: base\n(function_item) @function\n"
        );
    }

    #[test]
    fn override_without_extends_replaces_base_query() {
        let tmp = tempfile::tempdir().unwrap();
        let rust_dir = tmp.path().join("rust");
        fs::create_dir_all(&rust_dir).unwrap();
        fs::write(
            rust_dir.join(QueryType::Highlights.filename()),
            "(identifier) @function\n",
        )
        .unwrap();
        let sources = source_map(&[("rust", QueryType::Highlights, "(identifier) @variable\n")]);

        let composed =
            compose_query_source("rust", QueryType::Highlights, &sources, Some(tmp.path()))
                .unwrap();

        assert_eq!(composed.unwrap(), "(identifier) @function\n");
    }

    #[test]
    fn override_with_extends_appends_to_base_query() {
        let tmp = tempfile::tempdir().unwrap();
        let rust_dir = tmp.path().join("rust");
        fs::create_dir_all(&rust_dir).unwrap();
        fs::write(
            rust_dir.join(QueryType::Highlights.filename()),
            "; extends\n(identifier) @function\n",
        )
        .unwrap();
        let sources = source_map(&[("rust", QueryType::Highlights, "(line_comment) @comment\n")]);

        let composed =
            compose_query_source("rust", QueryType::Highlights, &sources, Some(tmp.path()))
                .unwrap();

        assert_eq!(
            composed.unwrap(),
            "(line_comment) @comment\n\n; extends\n(identifier) @function\n"
        );
    }
}
