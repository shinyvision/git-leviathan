use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, LazyLock, Mutex, RwLock};

use iced::Color;
use tree_sitter::Tree;

#[cfg(test)]
mod benchmarks;
mod caches;
mod detection;
mod highlighting;
mod injections;
pub mod installation;
mod parser_loading;
mod queries;
pub mod registry;

use caches::{
    estimated_tree_bytes, HighlightCache, HighlightDocumentCacheKey, HighlightFileCacheKey,
    HighlightSpanCache, HighlightSpanCacheKey, LazyHighlightState, ParseTreeCache,
    ParseTreeCacheKey, DOCUMENT_LINE_CACHE_CAPACITY,
};
use detection::detect_language;
pub use detection::file_extension_from_path;
use highlighting::{
    clear_injection_cache, highlight_bounds, plain_spans_for_line, TreeSitterHighlighter,
};
use installation::{
    AcceptingRegistryVerifier, GrammarInstallStatus, GrammarInstallationService,
    LocalGrammarTransport, RegistryFetchSource, RuntimeGrammarTransport, RuntimePackageDecoder,
};
use parser_loading::ParserLoader;
use registry::{BuiltinSyntaxRegistry, RuntimeGrammarSecurityPolicy, TreeSitterSyntax};

pub type LanguageDetection = detection::LanguageDetection;
pub type GrammarInventoryReport = registry::GrammarInventoryReport;
pub type GrammarInstallError = installation::GrammarInstallError;
pub type GrammarLanguageInstallStatus = installation::GrammarLanguageInstallStatus;
pub type RegistryFetchOutcome = installation::RegistryFetchOutcome;

static SYNTAX_SERVICE: LazyLock<RwLock<Option<Arc<SyntaxHighlightService>>>> =
    LazyLock::new(|| RwLock::new(None));

static HIGHLIGHT_CACHE: LazyLock<Mutex<HighlightCache>> =
    LazyLock::new(|| Mutex::new(HighlightCache::new()));
static PARSE_TREE_CACHE: LazyLock<Mutex<ParseTreeCache>> =
    LazyLock::new(|| Mutex::new(ParseTreeCache::new()));
static HIGHLIGHT_SPAN_CACHE: LazyLock<Mutex<HighlightSpanCache>> =
    LazyLock::new(|| Mutex::new(HighlightSpanCache::new()));

struct EagerInstallBus {
    tx: tokio::sync::mpsc::UnboundedSender<String>,
    rx: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<String>>>,
    seen: Mutex<BTreeSet<String>>,
}

static EAGER_INSTALL_BUS: LazyLock<EagerInstallBus> = LazyLock::new(|| {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    EagerInstallBus {
        tx,
        rx: Mutex::new(Some(rx)),
        seen: Mutex::new(BTreeSet::new()),
    }
});

pub fn request_eager_grammar_install(language: &str) {
    let Ok(mut seen) = EAGER_INSTALL_BUS.seen.lock() else {
        return;
    };
    if !seen.insert(language.to_string()) {
        return;
    }
    drop(seen);
    let _ = EAGER_INSTALL_BUS.tx.send(language.to_string());
}

pub fn take_eager_grammar_install_receiver() -> Option<tokio::sync::mpsc::UnboundedReceiver<String>>
{
    EAGER_INSTALL_BUS
        .rx
        .lock()
        .ok()
        .and_then(|mut guard| guard.take())
}

const LAZY_HIGHLIGHT_WINDOW_LINES: u32 = 48;

pub struct SyntaxHighlightService {
    registry: BuiltinSyntaxRegistry,
    parser_loader: ParserLoader,
    highlighter: TreeSitterHighlighter,
    runtime_inventory: Option<GrammarInventoryReport>,
    runtime_dir: Option<PathBuf>,
    query_override_dir: Option<PathBuf>,
    installation: Option<GrammarInstallationService>,
    runtime_policy: RuntimeGrammarSecurityPolicy,
}

pub enum HighlightLineResult {
    Ready(HighlightedLine),
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxGrammarStatus {
    Available {
        language_id: String,
        syntax_key: String,
    },
    Missing {
        language_id: String,
        syntax_key: String,
        registry_checked: bool,
        installable: bool,
        install_status: GrammarInstallStatus,
    },
    PlainText {
        language_id: String,
    },
}

impl SyntaxHighlightService {
    fn new() -> Self {
        let runtime_dir = default_runtime_grammar_dir();
        Self::with_runtime_and_query_override_dirs(runtime_dir.as_deref(), None)
    }

    pub fn with_runtime_and_query_override_dirs(
        runtime_dir: Option<&Path>,
        query_override_dir: Option<&Path>,
    ) -> Self {
        Self::with_runtime_policy_and_query_override_dirs(
            runtime_dir,
            query_override_dir,
            RuntimeGrammarSecurityPolicy::from_env(),
        )
    }

    fn with_runtime_policy_and_query_override_dirs(
        runtime_dir: Option<&Path>,
        query_override_dir: Option<&Path>,
        runtime_policy: RuntimeGrammarSecurityPolicy,
    ) -> Self {
        let registry = match query_override_dir {
            Some(dir) => BuiltinSyntaxRegistry::new_with_query_override_dir(Some(dir)),
            None => BuiltinSyntaxRegistry::new(),
        };
        let runtime_dir = runtime_dir.map(Path::to_path_buf);
        let query_override_dir = query_override_dir.map(Path::to_path_buf);
        let mut service = Self {
            registry,
            parser_loader: ParserLoader::new(),
            highlighter: TreeSitterHighlighter::new(),
            runtime_inventory: None,
            runtime_dir: runtime_dir.clone(),
            query_override_dir,
            installation: runtime_dir
                .clone()
                .map(|dir| GrammarInstallationService::with_policy(dir, runtime_policy.clone())),
            runtime_policy,
        };

        service.reload_runtime_inventory();

        service
    }

    #[cfg(test)]
    pub fn highlight_line(
        &self,
        document: &HighlightDocument,
        line_number: u32,
    ) -> Option<HighlightedLine> {
        self.highlight_range(document, line_number, line_number)
            .into_iter()
            .next()
    }

    pub fn highlight_line_for_document(
        &self,
        document: &HighlightDocument,
        line_number: u32,
    ) -> HighlightLineResult {
        let Some((line_number, _)) = highlight_bounds(document, line_number, line_number) else {
            return HighlightLineResult::Missing;
        };
        let detection = self.detect_language(document);
        let syntax = self.syntax_for_detection(&detection);
        if syntax.is_none() {
            self.queue_missing_grammar_for_detection(&detection);
        }
        let span_key = self.highlight_span_cache_key(document, &detection, syntax, line_number);

        {
            let mut state = document.lazy_state.borrow_mut();
            if let Some(spans) = state.cached_line(&span_key) {
                return HighlightLineResult::Ready(HighlightedLine { line_number, spans });
            }
        }
        if let Ok(mut cache) = HIGHLIGHT_SPAN_CACHE.lock() {
            if let Some(spans) = cache.get(&span_key) {
                let mut state = document.lazy_state.borrow_mut();
                state.record_span_cache_hit();
                state.insert_line(span_key, spans.clone());
                return HighlightLineResult::Ready(HighlightedLine { line_number, spans });
            }
        }

        let spans = match syntax {
            Some(syntax) => self
                .highlight_window_with_syntax(document, &detection, syntax, line_number)
                .or_else(|| document.line(line_number).map(plain_spans_for_line)),
            None => document.line(line_number).map(plain_spans_for_line),
        };

        match spans {
            Some(spans) => {
                if let Ok(mut cache) = HIGHLIGHT_SPAN_CACHE.lock() {
                    cache.insert(span_key.clone(), spans.clone());
                }
                let mut state = document.lazy_state.borrow_mut();
                state.insert_line(span_key, spans.clone());
                HighlightLineResult::Ready(HighlightedLine { line_number, spans })
            }
            None => HighlightLineResult::Missing,
        }
    }

    #[cfg(test)]
    pub fn highlight_range(
        &self,
        document: &HighlightDocument,
        start_line: u32,
        end_line: u32,
    ) -> Vec<HighlightedLine> {
        let Some((start_line, end_line)) = highlight_bounds(document, start_line, end_line) else {
            return Vec::new();
        };

        (start_line..=end_line)
            .filter_map(|line_number| {
                match self.highlight_line_for_document(document, line_number) {
                    HighlightLineResult::Ready(line) => Some(line),
                    _ => None,
                }
            })
            .collect()
    }

    pub fn highlight_document(&self, document: &HighlightDocument) -> HighlightedFile {
        let code = document.content();
        let file_extension = document.syntax_token();
        let span = crate::perf::Span::new("cpu.syntax_highlight_full_file")
            .field("extension", file_extension)
            .field("bytes", code.len());

        let before_stats = document.highlight_stats();
        let file = self.highlight_document_eager(document);
        let runtime_grammar_errors = self
            .runtime_inventory
            .as_ref()
            .map_or(0, |report| report.errors.len());
        span.field("lines", file.line_count())
            .field("tree_parse_hits", before_stats.parse_hits)
            .field("tree_parse_misses", before_stats.parse_misses)
            .field("lazy_cached_lines", before_stats.cached_lines)
            .field("lazy_cache_hits", before_stats.cache_hits)
            .field("lazy_cache_misses", before_stats.cache_misses)
            .field("runtime_grammar_errors", runtime_grammar_errors)
            .finish_with("spans", file.span_count());
        file
    }

    fn highlight_document_eager(&self, document: &HighlightDocument) -> HighlightedFile {
        let detection = self.detect_language(document);
        let syntax = self.syntax_for_detection(&detection);
        if syntax.is_none() {
            self.queue_missing_grammar_for_detection(&detection);
        }
        let mut tree = None;
        let mut spans = Vec::new();
        let mut line_ranges = Vec::new();

        for line_number in 1..=document.line_count() as u32 {
            let line_start = spans.len() as u32;
            let span_key = self.highlight_span_cache_key(document, &detection, syntax, line_number);
            let line_spans = HIGHLIGHT_SPAN_CACHE
                .lock()
                .ok()
                .and_then(|mut cache| cache.get(&span_key))
                .or_else(|| {
                    let computed = match syntax {
                        Some(syntax) => {
                            if tree.is_none() {
                                tree = self.ensure_tree(document, syntax);
                            }
                            tree.as_ref().and_then(|tree| {
                                self.highlight_line_from_tree(document, syntax, tree, line_number)
                            })
                        }
                        None => None,
                    }
                    .or_else(|| document.line(line_number).map(plain_spans_for_line));

                    if let Some(line_spans) = computed.as_ref() {
                        if let Ok(mut cache) = HIGHLIGHT_SPAN_CACHE.lock() {
                            cache.insert(span_key, line_spans.clone());
                        }
                    }
                    computed
                });

            if let Some(line_spans) = line_spans {
                spans.extend(line_spans.iter().cloned());
            }
            let line_end = spans.len() as u32;
            line_ranges.push((line_start, line_end));
            spans.push(SyntaxHighlightedSpan {
                text: "\n".to_string(),
                style: SyntaxStyle::default(),
            });
        }

        if spans.is_empty() {
            spans.push(SyntaxHighlightedSpan {
                text: document.content().to_string(),
                style: SyntaxStyle::default(),
            });
        }

        HighlightedFile { spans, line_ranges }
    }

    fn highlight_window_with_syntax(
        &self,
        document: &HighlightDocument,
        detection: &LanguageDetection,
        syntax: &TreeSitterSyntax,
        line_number: u32,
    ) -> Option<Arc<[SyntaxHighlightedSpan]>> {
        let tree = self.ensure_tree(document, syntax)?;
        let end_line = line_number
            .saturating_add(LAZY_HIGHLIGHT_WINDOW_LINES - 1)
            .min(document.line_count() as u32);
        let highlighted = self.highlighter.highlight_range_from_tree(
            document,
            syntax,
            &tree,
            &self.registry,
            &self.parser_loader,
            line_number..=end_line,
        );
        let mut requested = None;
        for (highlighted_line, spans) in highlighted {
            let span_key =
                self.highlight_span_cache_key(document, detection, Some(syntax), highlighted_line);
            if let Ok(mut cache) = HIGHLIGHT_SPAN_CACHE.lock() {
                cache.insert(span_key.clone(), spans.clone());
            }
            let mut state = document.lazy_state.borrow_mut();
            state.insert_line(span_key, spans.clone());
            if highlighted_line == line_number {
                requested = Some(spans);
            }
        }
        requested
    }

    fn ensure_tree(&self, document: &HighlightDocument, syntax: &TreeSitterSyntax) -> Option<Tree> {
        let key = self.parse_tree_cache_key(document, syntax);
        {
            let mut state = document.lazy_state.borrow_mut();
            if let Some(tree) = state.cached_tree(&key) {
                return Some(tree);
            }
        }
        if let Ok(mut cache) = PARSE_TREE_CACHE.lock() {
            if let Some(tree) = cache.get(&key) {
                let mut state = document.lazy_state.borrow_mut();
                state.store_cached_tree(key, tree.clone());
                return Some(tree);
            }
        }

        let tree = self.parser_loader.parse_tree(syntax, document.content())?;
        if let Ok(mut cache) = PARSE_TREE_CACHE.lock() {
            cache.insert(
                key.clone(),
                tree.clone(),
                estimated_tree_bytes(document.byte_count(), document.line_count()),
            );
        }
        let mut state = document.lazy_state.borrow_mut();
        state.store_parsed_tree(key, tree.clone(), document.line_count());
        Some(tree)
    }

    fn highlight_line_from_tree(
        &self,
        document: &HighlightDocument,
        syntax: &TreeSitterSyntax,
        tree: &Tree,
        line_number: u32,
    ) -> Option<Arc<[SyntaxHighlightedSpan]>> {
        self.highlighter.highlight_line_from_tree(
            document,
            syntax,
            tree,
            &self.registry,
            &self.parser_loader,
            line_number,
        )
    }

    pub fn detect_language(&self, document: &HighlightDocument) -> LanguageDetection {
        detect_language(
            document.content(),
            document.path(),
            document.syntax_token(),
            &self.registry,
        )
    }

    pub fn syntax_status_for_document(&self, document: &HighlightDocument) -> SyntaxGrammarStatus {
        let detection = self.detect_language(document);
        let language_id = detection.language_id.as_str().to_string();
        let Some(syntax_key) = detection.syntax_key() else {
            return SyntaxGrammarStatus::PlainText { language_id };
        };

        if self.registry.get(syntax_key).is_some() {
            return SyntaxGrammarStatus::Available {
                language_id,
                syntax_key: syntax_key.to_string(),
            };
        }

        let installable = self
            .runtime_inventory
            .as_ref()
            .and_then(|report| {
                report.missing.iter().find(|missing| {
                    let missing_language = normalize_language_key(&missing.entry.language);
                    missing_language == normalize_language_key(&language_id)
                        || missing_language == normalize_language_key(syntax_key)
                })
            })
            .is_some_and(|missing| missing.compatibility.is_compatible());
        let install_status = self
            .installation
            .as_ref()
            .map(|installation| installation.status_for_language(syntax_key).status)
            .unwrap_or(GrammarInstallStatus::Missing);

        SyntaxGrammarStatus::Missing {
            language_id,
            syntax_key: syntax_key.to_string(),
            registry_checked: self.runtime_inventory.is_some(),
            installable,
            install_status,
        }
    }

    pub fn list_grammar_inventory(&self, runtime_dir: impl AsRef<Path>) -> GrammarInventoryReport {
        registry::list_grammar_inventory_for_app_version(
            runtime_dir.as_ref(),
            &self.registry,
            &registry::current_app_version(),
            &self.runtime_policy,
        )
    }

    fn queue_missing_grammar_for_detection(
        &self,
        detection: &LanguageDetection,
    ) -> Option<GrammarLanguageInstallStatus> {
        let syntax_key = detection.syntax_key()?;
        if self.registry.get(syntax_key).is_some() {
            return None;
        }
        let status = self
            .installation
            .as_ref()
            .map(|installation| installation.queue_install_for_language(syntax_key))?;
        if status.status == GrammarInstallStatus::Queued {
            request_eager_grammar_install(syntax_key);
        }
        Some(status)
    }

    fn reload_runtime_inventory(&mut self) {
        let Some(runtime_dir) = self.runtime_dir.clone() else {
            return;
        };
        self.registry = match self.query_override_dir.as_deref() {
            Some(dir) => BuiltinSyntaxRegistry::new_with_query_override_dir(Some(dir)),
            None => BuiltinSyntaxRegistry::new(),
        };
        let report = self.list_grammar_inventory(&runtime_dir);
        self.registry
            .load_runtime_grammars(&report.installed, &self.runtime_policy);
        self.runtime_inventory = Some(report);
    }

    fn syntax_for_detection(&self, detection: &LanguageDetection) -> Option<&TreeSitterSyntax> {
        let key = detection.syntax_key()?;
        self.registry.get(key)
    }

    fn parse_tree_cache_key(
        &self,
        document: &HighlightDocument,
        syntax: &TreeSitterSyntax,
    ) -> ParseTreeCacheKey {
        ParseTreeCacheKey::new(
            document.path().map(ToString::to_string),
            document.content_hash(),
            syntax.name(),
            syntax.parser_identity().clone(),
        )
    }

    fn highlight_document_cache_key(
        &self,
        document: &HighlightDocument,
        detection: &LanguageDetection,
        syntax: Option<&TreeSitterSyntax>,
    ) -> HighlightDocumentCacheKey {
        HighlightDocumentCacheKey::new(
            document.path().map(ToString::to_string),
            document.content_hash(),
            detection.cache_key(),
            syntax.map(|syntax| syntax.name().to_string()),
            syntax.map(|syntax| syntax.parser_identity().clone()),
            syntax.map(TreeSitterSyntax::query_identity),
            self.registry.cache_identity(),
        )
    }

    fn highlight_span_cache_key(
        &self,
        document: &HighlightDocument,
        detection: &LanguageDetection,
        syntax: Option<&TreeSitterSyntax>,
        line_number: u32,
    ) -> HighlightSpanCacheKey {
        HighlightSpanCacheKey::new(
            self.highlight_document_cache_key(document, detection, syntax),
            line_number,
            line_number,
        )
    }
}

#[derive(Debug, Clone)]
pub struct HighlightDocument {
    content: Arc<str>,
    line_offsets: Arc<[usize]>,
    content_hash: u64,
    syntax_token: String,
    path: Option<String>,
    byte_count: usize,
    lazy_state: Rc<RefCell<LazyHighlightState>>,
}

impl HighlightDocument {
    pub fn new(content: impl AsRef<str>, syntax_token: impl Into<String>) -> Self {
        Self::with_cache_limits(content, syntax_token, DOCUMENT_LINE_CACHE_CAPACITY)
    }

    pub fn from_path(content: impl AsRef<str>, path: impl Into<String>) -> Self {
        let path = path.into();
        let syntax_token = file_extension_from_path(&path);
        let mut document = Self::new(content, syntax_token);
        document.path = (!path.is_empty()).then_some(path);
        document
    }

    #[cfg(test)]
    fn new_with_cache_limits(
        content: impl AsRef<str>,
        syntax_token: impl Into<String>,
        line_cache_capacity: usize,
        _tree_cache_capacity: usize,
    ) -> Self {
        Self::with_cache_limits(content, syntax_token, line_cache_capacity)
    }

    fn with_cache_limits(
        content: impl AsRef<str>,
        syntax_token: impl Into<String>,
        line_cache_capacity: usize,
    ) -> Self {
        Self::with_detection_inputs(content, syntax_token, None, line_cache_capacity)
    }

    fn with_detection_inputs(
        content: impl AsRef<str>,
        syntax_token: impl Into<String>,
        path: Option<String>,
        line_cache_capacity: usize,
    ) -> Self {
        let content = normalize_highlight_content(content.as_ref());
        let line_offsets = compute_line_offsets(&content);
        let content_hash = hash_content(&content);
        let byte_count = content.len();

        Self {
            content: Arc::from(content.into_boxed_str()),
            line_offsets: Arc::from(line_offsets.into_boxed_slice()),
            content_hash,
            syntax_token: syntax_token.into(),
            path,
            byte_count,
            lazy_state: Rc::new(RefCell::new(LazyHighlightState::with_limit(
                line_cache_capacity,
            ))),
        }
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    fn line_start_offset(&self, line_number: u32) -> Option<usize> {
        if line_number == 0 {
            return None;
        }
        self.line_offsets.get((line_number - 1) as usize).copied()
    }

    fn line_range(&self, line_number: u32) -> Option<Range<usize>> {
        if line_number == 0 {
            return None;
        }
        let idx = (line_number - 1) as usize;
        let start = self.line_start_offset(line_number)?;
        let mut end = self
            .line_offsets
            .get(idx + 1)
            .copied()
            .unwrap_or_else(|| self.content.len());
        let bytes = self.content.as_bytes();
        if end > start && bytes[end - 1] == b'\n' {
            end -= 1;
        }
        if end > start && bytes[end - 1] == b'\r' {
            end -= 1;
        }
        Some(start..end)
    }

    pub fn line(&self, line_number: u32) -> Option<&str> {
        self.line_range(line_number)
            .map(|range| &self.content[range])
    }

    pub fn content_hash(&self) -> u64 {
        self.content_hash
    }

    pub fn syntax_token(&self) -> &str {
        &self.syntax_token
    }

    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    pub fn byte_count(&self) -> usize {
        self.byte_count
    }

    pub fn line_count(&self) -> usize {
        self.line_offsets.len()
    }

    pub fn highlight_stats(&self) -> HighlightLazyStats {
        self.lazy_state.borrow().stats()
    }

    pub fn highlight_generation_id(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.content_hash.hash(&mut hasher);
        self.syntax_token.hash(&mut hasher);
        self.path.hash(&mut hasher);
        hasher.finish()
    }
}

#[derive(Debug, Clone)]
pub struct HighlightedLine {
    pub line_number: u32,
    spans: Arc<[SyntaxHighlightedSpan]>,
}

impl HighlightedLine {
    #[cfg(test)]
    pub fn spans(&self) -> &[SyntaxHighlightedSpan] {
        &self.spans
    }

    pub fn into_spans(self) -> Arc<[SyntaxHighlightedSpan]> {
        self.spans
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HighlightLazyStats {
    pub parsed_lines: usize,
    pub parsed_trees: usize,
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub parse_hits: usize,
    pub parse_misses: usize,
    pub cached_lines: usize,
}

fn normalize_highlight_content(content: &str) -> String {
    content.replace('\t', "    ")
}

fn compute_line_offsets(content: &str) -> Vec<usize> {
    if content.is_empty() {
        return Vec::new();
    }

    let mut offsets = vec![0];
    for (idx, byte) in content.bytes().enumerate() {
        if byte == b'\n' && idx + 1 < content.len() {
            offsets.push(idx + 1);
        }
    }
    offsets
}

fn hash_content(content: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

fn normalize_language_key(language: &str) -> String {
    language.trim().to_ascii_lowercase()
}

#[derive(Debug, Clone, PartialEq)]
pub struct SyntaxHighlightedSpan {
    pub text: String,
    pub style: SyntaxStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SyntaxStyle {
    pub color: Option<Color>,
    pub bold: bool,
    pub italic: bool,
}

impl SyntaxStyle {
    fn colored(color: Color) -> Self {
        Self {
            color: Some(color),
            bold: false,
            italic: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HighlightedFile {
    spans: Vec<SyntaxHighlightedSpan>,
    line_ranges: Vec<(u32, u32)>,
}

impl HighlightedFile {
    pub fn spans(&self) -> &[SyntaxHighlightedSpan] {
        &self.spans
    }

    pub fn line(&self, line_number: u32) -> &[SyntaxHighlightedSpan] {
        if line_number == 0 {
            return &[];
        }
        let idx = (line_number - 1) as usize;
        match self.line_ranges.get(idx) {
            Some(&(s, e)) => &self.spans[s as usize..e as usize],
            None => &[],
        }
    }

    pub fn line_count(&self) -> usize {
        self.line_ranges.len()
    }

    pub fn span_count(&self) -> usize {
        self.spans.len()
    }
}

pub fn get_syntax_service() -> Arc<SyntaxHighlightService> {
    if let Ok(guard) = SYNTAX_SERVICE.read() {
        if let Some(svc) = guard.as_ref() {
            return Arc::clone(svc);
        }
    }
    let mut guard = SYNTAX_SERVICE
        .write()
        .expect("syntax service lock poisoned");
    if let Some(svc) = guard.as_ref() {
        return Arc::clone(svc);
    }
    let svc = Arc::new(SyntaxHighlightService::new());
    *guard = Some(Arc::clone(&svc));
    svc
}

pub fn release_syntax_caches() {
    if let Ok(mut cache) = HIGHLIGHT_CACHE.lock() {
        *cache = HighlightCache::new();
    }
    if let Ok(mut cache) = PARSE_TREE_CACHE.lock() {
        *cache = ParseTreeCache::new();
    }
    if let Ok(mut cache) = HIGHLIGHT_SPAN_CACHE.lock() {
        *cache = HighlightSpanCache::new();
    }
    queries::clear_query_cache();
    clear_injection_cache();
}

pub fn refresh_runtime_grammar_registry_from_path(
    path: impl AsRef<Path>,
) -> Result<RegistryFetchOutcome, GrammarInstallError> {
    let service = runtime_grammar_installation_service()?;
    let source = RegistryFetchSource::new(path.as_ref().to_string_lossy());
    let outcome = service.refresh_registry(
        &source,
        &LocalGrammarTransport,
        &AcceptingRegistryVerifier,
        true,
    )?;
    reset_syntax_service_after_runtime_asset_change();
    Ok(outcome)
}

pub fn install_runtime_grammar(
    language: &str,
) -> Result<GrammarLanguageInstallStatus, GrammarInstallError> {
    let service = runtime_grammar_installation_service()?;
    service.queue_install_for_language(language);
    let status = service.install_queued_grammar(
        language,
        &RuntimeGrammarTransport,
        &RuntimePackageDecoder,
    )?;
    reset_syntax_service_after_runtime_asset_change();
    Ok(status)
}

pub fn update_runtime_grammar(
    language: &str,
) -> Result<GrammarLanguageInstallStatus, GrammarInstallError> {
    let service = runtime_grammar_installation_service()?;
    let status =
        service.update_grammar(language, &RuntimeGrammarTransport, &RuntimePackageDecoder)?;
    reset_syntax_service_after_runtime_asset_change();
    Ok(status)
}

pub fn uninstall_runtime_grammar(
    language: &str,
) -> Result<GrammarLanguageInstallStatus, GrammarInstallError> {
    let service = runtime_grammar_installation_service()?;
    let status = service.uninstall_grammar(language)?;
    reset_syntax_service_after_runtime_asset_change();
    Ok(status)
}

fn runtime_grammar_installation_service() -> Result<GrammarInstallationService, GrammarInstallError>
{
    default_runtime_grammar_dir()
        .map(GrammarInstallationService::new)
        .ok_or_else(|| {
            GrammarInstallError::runtime_directory_unavailable(
                "runtime grammar directory is unavailable",
            )
        })
}

fn reset_syntax_service_after_runtime_asset_change() {
    release_syntax_caches();
    if let Ok(mut guard) = SYNTAX_SERVICE.write() {
        *guard = None;
    }
}

pub fn highlight_document(document: &HighlightDocument) -> Arc<HighlightedFile> {
    let service = get_syntax_service();
    let detection = service.detect_language(document);
    let syntax = service.syntax_for_detection(&detection);
    let key = HighlightFileCacheKey::new(
        service.highlight_document_cache_key(document, &detection, syntax),
    );
    if let Ok(mut cache) = HIGHLIGHT_CACHE.lock() {
        if let Some(hit) = cache.get(&key) {
            return hit;
        }
    }

    let result = Arc::new(service.highlight_document(document));
    if let Ok(mut cache) = HIGHLIGHT_CACHE.lock() {
        cache.insert(key, Arc::clone(&result));
    }
    result
}

fn default_runtime_grammar_dir() -> Option<PathBuf> {
    dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .map(|base| base.join("git_leviathan").join("tree-sitter"))
}

#[cfg(test)]
fn highlight_file(code: &str, file_extension: &str) -> Arc<HighlightedFile> {
    let document = HighlightDocument::new(code, file_extension);
    highlight_document(&document)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span_text(spans: &[SyntaxHighlightedSpan]) -> String {
        spans.iter().map(|span| span.text.as_str()).collect()
    }

    #[test]
    fn runtime_grammar_listing_api_lists_bootstrap_grammars_as_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let service =
            SyntaxHighlightService::with_runtime_and_query_override_dirs(Some(tmp.path()), None);
        let report: GrammarInventoryReport = service.list_grammar_inventory(tmp.path());

        assert!(report.built_in.is_empty());
        assert!(report.installed.is_empty());
        for language in ["rust", "go", "twig", "json", "typescript", "tsx"] {
            assert!(
                report
                    .missing
                    .iter()
                    .any(|missing| missing.entry.language == language),
                "expected {language} in missing list",
            );
        }
        assert!(report.errors.is_empty());
    }

    #[test]
    fn invalid_wasm_runtime_grammar_falls_back_without_affecting_builtins() {
        let tmp = tempfile::tempdir().unwrap();
        let package_dir = tmp.path().join("parsers").join("badwasm");
        std::fs::create_dir_all(package_dir.join("wasm")).unwrap();
        std::fs::create_dir_all(package_dir.join("queries")).unwrap();
        std::fs::write(package_dir.join("wasm").join("badwasm.wasm"), "not wasm").unwrap();
        std::fs::write(package_dir.join("queries").join("highlights.scm"), "").unwrap();
        let manifest = registry::GrammarPackageManifest {
            language: "badwasm".to_string(),
            version: semver::Version::new(1, 0, 0),
            parser_abi: tree_sitter::LANGUAGE_VERSION,
            runtime: registry::GrammarRuntime::Wasm,
            platform: "wasm".to_string(),
            source: registry::GrammarPackageSource::Community,
            source_url: None,
            signature: None,
            files: registry::GrammarPackageFiles {
                parser: None,
                wasm: Some("wasm/badwasm.wasm".to_string()),
                highlights: Some("queries/highlights.scm".to_string()),
                injections: None,
                locals: None,
            },
            filetypes: vec!["badwasm".to_string()],
            extensions: vec!["badwasm".to_string()],
            filenames: Vec::new(),
            first_line_regex: None,
            content_regex: None,
            app_version_req: None,
            sha256: Default::default(),
        };
        std::fs::write(
            package_dir.join(registry::PACKAGE_MANIFEST_FILENAME),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let service =
            SyntaxHighlightService::with_runtime_and_query_override_dirs(Some(tmp.path()), None);
        let plain_document = HighlightDocument::new("plain text\n", "badwasm");

        assert_plain_line(
            service.highlight_line(&plain_document, 1).unwrap().spans(),
            "plain text",
        );
    }

    #[test]
    fn unsupported_syntax_returns_plain_spans() {
        let service = SyntaxHighlightService::new();
        let document = HighlightDocument::new("one\ntwo\n", "txt");

        let highlighted = service.highlight_line(&document, 2).unwrap();

        assert_eq!(highlighted.line_number, 2);
        assert_plain_line(highlighted.spans(), "two");
    }

    #[test]
    fn detected_languages_without_parsers_return_plain_spans() {
        let service = SyntaxHighlightService::with_runtime_and_query_override_dirs(None, None);
        let cases = [
            (
                HighlightDocument::from_path("{{ name }}\n", "templates/card.html.twig"),
                "{{ name }}",
            ),
            (
                HighlightDocument::from_path("@color: #fff;\n", "styles/theme.less"),
                "@color: #fff;",
            ),
            (
                HighlightDocument::from_path("FROM alpine\n", "Dockerfile"),
                "FROM alpine",
            ),
        ];

        for (document, expected) in cases {
            let highlighted = service.highlight_line(&document, 1).unwrap();
            assert_plain_line(highlighted.spans(), expected);
        }
    }

    #[test]
    fn missing_grammar_status_reports_registry_installability_without_blocking() {
        let tmp = tempfile::tempdir().unwrap();
        let registry_dir = tmp.path().join("registry");
        std::fs::create_dir_all(&registry_dir).unwrap();
        let registry_file = registry::GrammarRegistryFile {
            schema_version: registry::REGISTRY_SCHEMA_VERSION,
            cache: registry::RegistryCacheMetadata::with_default_ttl(100),
            grammars: vec![registry::GrammarRegistryEntry {
                language: "twig".to_string(),
                version: semver::Version::new(1, 0, 0),
                parser_abi: tree_sitter::LANGUAGE_VERSION,
                app_version_req: None,
                runtime: Some(registry::GrammarRuntime::Native),
                platforms: Vec::new(),
                filetypes: vec!["twig".to_string(), "html.twig".to_string()],
                extensions: vec!["twig".to_string()],
                filenames: Vec::new(),
                first_line_regex: None,
                content_regex: None,
                packages: vec![registry::GrammarPackageDownload {
                    url: "https://example.invalid/twig.pkg".to_string(),
                    sha256: None,
                    signature: Some("test-signature".to_string()),
                    source: registry::GrammarPackageSource::Official,
                    runtime: Some(registry::GrammarRuntime::Native),
                    platform: Some(parser_loading::current_platform().to_string()),
                }],
            }],
        };
        std::fs::write(
            registry_dir.join("registry.json"),
            serde_json::to_string_pretty(&registry_file).unwrap(),
        )
        .unwrap();
        let service = SyntaxHighlightService::with_runtime_policy_and_query_override_dirs(
            Some(tmp.path()),
            None,
            RuntimeGrammarSecurityPolicy {
                native_package_source_allowlist: vec!["https://example.invalid/".to_string()],
                ..RuntimeGrammarSecurityPolicy::default()
            },
        );
        let document = HighlightDocument::from_path("{{ name }}\n", "templates/card.html.twig");

        let status = service.syntax_status_for_document(&document);
        let highlighted = service.highlight_line(&document, 1).unwrap();

        assert_eq!(
            status,
            SyntaxGrammarStatus::Missing {
                language_id: "twig".to_string(),
                syntax_key: "twig".to_string(),
                registry_checked: true,
                installable: true,
                install_status: GrammarInstallStatus::Missing,
            }
        );
        assert_plain_line(highlighted.spans(), "{{ name }}");
        assert_eq!(document.highlight_stats().parsed_trees, 0);
        let after_highlight_status = service.syntax_status_for_document(&document);
        assert!(
            matches!(
                after_highlight_status,
                SyntaxGrammarStatus::Missing {
                    install_status: GrammarInstallStatus::Queued,
                    ..
                }
            ),
            "{after_highlight_status:?}"
        );
    }

    #[test]
    fn lazy_line_cache_is_bounded_without_dropping_active_range() {
        let service = SyntaxHighlightService::new();
        let mut code = String::new();
        for idx in 0..20 {
            code.push_str(&format!("let value_{idx} = {idx};\n"));
        }
        let document = HighlightDocument::new_with_cache_limits(&code, "rs", 4, 8);

        let highlighted = service.highlight_range(&document, 9, 12);
        let after_range = document.highlight_stats();

        assert_eq!(
            highlighted
                .iter()
                .map(|line| line.line_number)
                .collect::<Vec<_>>(),
            vec![9, 10, 11, 12]
        );
        assert_eq!(after_range.cached_lines, 4);

        let after_cached = service.highlight_range(&document, 9, 12);
        let final_stats = document.highlight_stats();

        assert_eq!(after_cached.len(), 4);
        assert_eq!(final_stats.cached_lines, 4);
        assert!(final_stats.cache_hits >= after_range.cache_hits + 4);
    }

    #[test]
    fn whole_file_highlighting_ignores_lazy_line_cache_bounds() {
        let service = SyntaxHighlightService::new();
        let mut code = String::new();
        for idx in 0..12 {
            code.push_str(&format!("let value_{idx} = {idx};\n"));
        }
        let document = HighlightDocument::new_with_cache_limits(&code, "rs", 4, 2);

        let highlighted = service.highlight_document(&document);
        let stats = document.highlight_stats();

        assert_eq!(highlighted.line_count(), 12);
        assert_eq!(stats.cached_lines, 0);
    }

    #[test]
    fn highlight_document_exposes_line_offsets() {
        let document = HighlightDocument::new("alpha\nbeta\ngamma\n", "rs");

        assert_eq!(document.line_count(), 3);
        assert_eq!(document.line_start_offset(1), Some(0));
        assert_eq!(document.line_start_offset(2), Some(6));
        assert_eq!(document.line_start_offset(3), Some(11));
        assert_eq!(document.line_start_offset(4), None);
        assert_eq!(document.line(2), Some("beta"));
    }

    #[test]
    fn highlight_document_keeps_last_line_without_trailing_newline() {
        let document = HighlightDocument::new("first\nlast", "txt");

        assert_eq!(document.line_count(), 2);
        assert_eq!(document.line_start_offset(2), Some(6));
        assert_eq!(document.line(2), Some("last"));
    }

    #[test]
    fn highlight_document_content_hash_uses_normalized_content_identity() {
        let a = HighlightDocument::new("let x = 1;\n", "rs");
        let b = HighlightDocument::new("let x = 1;\n", "txt");
        let c = HighlightDocument::new("let x = 2;\n", "rs");

        assert_eq!(a.content_hash(), b.content_hash());
        assert_ne!(a.content_hash(), c.content_hash());
        assert_eq!(a.syntax_token(), "rs");
        assert_eq!(b.syntax_token(), "txt");
    }

    #[test]
    fn highlight_document_tab_normalization_matches_existing_expansion() {
        let raw = "fn main() {\n\tprintln!(\"hi\");\n}\n";
        let expanded = raw.replace('\t', "    ");
        let raw_document = HighlightDocument::new(raw, "rs");
        let expanded_document = HighlightDocument::new(&expanded, "rs");

        assert_eq!(raw_document.content(), expanded);
        assert_eq!(raw_document.byte_count(), expanded.len());
        assert_eq!(
            raw_document.content_hash(),
            expanded_document.content_hash()
        );

        let raw_highlight = highlight_file(raw, "rs");
        let expanded_highlight = highlight_file(&expanded, "rs");
        assert_eq!(raw_highlight.spans(), expanded_highlight.spans());
    }

    #[test]
    fn extract_spans_for_line_can_return_the_last_line() {
        let file = highlight_file("<?php\nfinal class LastLine {}", "php");
        let spans = file.line(2);

        assert_eq!(span_text(spans), "final class LastLine {}");
        assert!(!spans.is_empty());
    }

    #[test]
    fn overlong_line_falls_back_to_plain_span() {
        let service = SyntaxHighlightService::new();
        let long_line = format!(
            "let value = \"{}\";",
            "x".repeat(highlighting::MAX_HIGHLIGHT_LINE_BYTES)
        );
        let document = HighlightDocument::new(&long_line, "rs");

        let highlighted = service.highlight_line(&document, 1).unwrap();

        assert_plain_line(highlighted.spans(), &long_line);
    }

    #[test]
    fn highlight_document_exposes_full_line_ranges() {
        let service = SyntaxHighlightService::new();
        let document = HighlightDocument::new("one\ntwo\nthree\n", "txt");

        let highlighted = service.highlight_document(&document);

        assert_eq!(highlighted.line_count(), 3);
        assert_eq!(span_text(highlighted.line(1)), "one");
        assert_eq!(span_text(highlighted.line(2)), "two");
        assert_eq!(span_text(highlighted.line(3)), "three");
    }

    fn assert_plain_line(spans: &[SyntaxHighlightedSpan], expected_text: &str) {
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, expected_text);
        assert_eq!(spans[0].style, SyntaxStyle::default());
    }
}
