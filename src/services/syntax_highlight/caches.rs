use std::mem;
use std::num::NonZeroUsize;
use std::sync::Arc;

use lru::LruCache;
use tree_sitter::Tree;

use super::queries::QuerySetIdentity;
use super::registry::ParserCacheIdentity;
use super::{HighlightLazyStats, HighlightedFile, LanguageDetection, SyntaxHighlightedSpan};

pub(super) const DOCUMENT_LINE_CACHE_CAPACITY: usize = 2_048;

const HIGHLIGHT_CACHE_CAPACITY: usize = 16;
const PARSE_TREE_CACHE_CAPACITY: usize = 32;
const PARSE_TREE_CACHE_BYTES: usize = 32 * 1024 * 1024;
const SPAN_CACHE_CAPACITY: usize = 8_192;
const SPAN_CACHE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct ParseTreeCacheKey {
    path: Option<String>,
    content_hash: u64,
    syntax_key: String,
    parser_identity: ParserCacheIdentity,
}

impl ParseTreeCacheKey {
    pub(super) fn new(
        path: Option<String>,
        content_hash: u64,
        syntax_key: impl Into<String>,
        parser_identity: ParserCacheIdentity,
    ) -> Self {
        Self {
            path,
            content_hash,
            syntax_key: syntax_key.into(),
            parser_identity,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct HighlightDocumentCacheKey {
    path: Option<String>,
    content_hash: u64,
    language_key: String,
    syntax_key: Option<String>,
    parser_identity: Option<ParserCacheIdentity>,
    query_identity: Option<QuerySetIdentity>,
    registry_identity: u64,
}

impl HighlightDocumentCacheKey {
    pub(super) fn new(
        path: Option<String>,
        content_hash: u64,
        language_key: impl Into<String>,
        syntax_key: Option<String>,
        parser_identity: Option<ParserCacheIdentity>,
        query_identity: Option<QuerySetIdentity>,
        registry_identity: u64,
    ) -> Self {
        Self {
            path,
            content_hash,
            language_key: language_key.into(),
            syntax_key,
            parser_identity,
            query_identity,
            registry_identity,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct HighlightSpanCacheKey {
    document: HighlightDocumentCacheKey,
    start_line: u32,
    end_line: u32,
}

impl HighlightSpanCacheKey {
    pub(super) fn new(document: HighlightDocumentCacheKey, start_line: u32, end_line: u32) -> Self {
        Self {
            document,
            start_line,
            end_line,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct HighlightFileCacheKey {
    document: HighlightDocumentCacheKey,
}

impl HighlightFileCacheKey {
    pub(super) fn new(document: HighlightDocumentCacheKey) -> Self {
        Self { document }
    }
}

pub(super) struct HighlightCache {
    map: LruCache<HighlightFileCacheKey, Arc<HighlightedFile>>,
}

impl HighlightCache {
    pub(super) fn new() -> Self {
        Self {
            map: LruCache::new(nonzero_capacity(HIGHLIGHT_CACHE_CAPACITY)),
        }
    }

    pub(super) fn get(&mut self, key: &HighlightFileCacheKey) -> Option<Arc<HighlightedFile>> {
        self.map.get(key).cloned()
    }

    pub(super) fn insert(&mut self, key: HighlightFileCacheKey, value: Arc<HighlightedFile>) {
        self.map.put(key, value);
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheFootprint {
    pub entries: usize,
    pub bytes: usize,
}

pub(super) struct ParseTreeCache {
    map: LruCache<ParseTreeCacheKey, ParseTreeEntry>,
    max_count: usize,
    max_bytes: usize,
    current_bytes: usize,
}

struct ParseTreeEntry {
    tree: Tree,
    bytes: usize,
}

impl ParseTreeCache {
    pub(super) fn new() -> Self {
        Self::with_limits(PARSE_TREE_CACHE_CAPACITY, PARSE_TREE_CACHE_BYTES)
    }

    fn with_limits(max_count: usize, max_bytes: usize) -> Self {
        Self {
            map: LruCache::new(nonzero_capacity(max_count)),
            max_count,
            max_bytes,
            current_bytes: 0,
        }
    }

    pub(super) fn get(&mut self, key: &ParseTreeCacheKey) -> Option<Tree> {
        self.map.get(key).map(|entry| entry.tree.clone())
    }

    pub(super) fn insert(&mut self, key: ParseTreeCacheKey, tree: Tree, bytes: usize) {
        if self.max_count == 0 || self.max_bytes == 0 || bytes > self.max_bytes {
            return;
        }

        if let Some((_, entry)) = self.map.push(key, ParseTreeEntry { tree, bytes }) {
            self.current_bytes = self.current_bytes.saturating_sub(entry.bytes);
        }
        self.current_bytes = self.current_bytes.saturating_add(bytes);
        self.evict_to_budget();
    }

    fn evict_to_budget(&mut self) {
        while self.map.len() > self.max_count || self.current_bytes > self.max_bytes {
            let Some((_, entry)) = self.map.pop_lru() else {
                self.current_bytes = 0;
                break;
            };
            self.current_bytes = self.current_bytes.saturating_sub(entry.bytes);
        }
    }

    #[cfg(test)]
    pub(super) fn footprint(&self) -> CacheFootprint {
        CacheFootprint {
            entries: self.map.len(),
            bytes: self.current_bytes,
        }
    }
}

pub(super) struct HighlightSpanCache {
    map: LruCache<HighlightSpanCacheKey, SpanEntry>,
    max_count: usize,
    max_bytes: usize,
    current_bytes: usize,
}

struct SpanEntry {
    spans: Arc<[SyntaxHighlightedSpan]>,
    bytes: usize,
}

impl HighlightSpanCache {
    pub(super) fn new() -> Self {
        Self::with_limits(SPAN_CACHE_CAPACITY, SPAN_CACHE_BYTES)
    }

    fn with_limits(max_count: usize, max_bytes: usize) -> Self {
        Self {
            map: LruCache::new(nonzero_capacity(max_count)),
            max_count,
            max_bytes,
            current_bytes: 0,
        }
    }

    pub(super) fn get(
        &mut self,
        key: &HighlightSpanCacheKey,
    ) -> Option<Arc<[SyntaxHighlightedSpan]>> {
        self.map.get(key).map(|entry| Arc::clone(&entry.spans))
    }

    pub(super) fn insert(
        &mut self,
        key: HighlightSpanCacheKey,
        spans: Arc<[SyntaxHighlightedSpan]>,
    ) {
        let bytes = estimated_span_bytes(&spans);
        if self.max_count == 0 || self.max_bytes == 0 || bytes > self.max_bytes {
            return;
        }

        if let Some((_, entry)) = self.map.push(key, SpanEntry { spans, bytes }) {
            self.current_bytes = self.current_bytes.saturating_sub(entry.bytes);
        }
        self.current_bytes = self.current_bytes.saturating_add(bytes);
        self.evict_to_budget();
    }

    fn evict_to_budget(&mut self) {
        while self.map.len() > self.max_count || self.current_bytes > self.max_bytes {
            let Some((_, entry)) = self.map.pop_lru() else {
                self.current_bytes = 0;
                break;
            };
            self.current_bytes = self.current_bytes.saturating_sub(entry.bytes);
        }
    }
}

#[derive(Debug)]
pub(super) struct LazyHighlightState {
    tree: Option<(ParseTreeCacheKey, Tree)>,
    line_cache: LruCache<HighlightSpanCacheKey, Arc<[SyntaxHighlightedSpan]>>,
    line_cache_capacity: usize,
    detection: Option<(u64, LanguageDetection)>,
    stats: HighlightLazyStats,
}

impl LazyHighlightState {
    pub(super) fn with_limit(line_cache_capacity: usize) -> Self {
        Self {
            tree: None,
            line_cache: LruCache::new(nonzero_capacity(line_cache_capacity)),
            line_cache_capacity,
            detection: None,
            stats: HighlightLazyStats::default(),
        }
    }

    pub(super) fn cached_detection(&self, registry_identity: u64) -> Option<LanguageDetection> {
        self.detection
            .as_ref()
            .filter(|(identity, _)| *identity == registry_identity)
            .map(|(_, detection)| detection.clone())
    }

    pub(super) fn store_detection(&mut self, registry_identity: u64, detection: LanguageDetection) {
        self.detection = Some((registry_identity, detection));
    }

    pub(super) fn stats(&self) -> HighlightLazyStats {
        HighlightLazyStats {
            cached_lines: self.line_cache.len(),
            ..self.stats
        }
    }

    pub(super) fn cached_line(
        &mut self,
        key: &HighlightSpanCacheKey,
    ) -> Option<Arc<[SyntaxHighlightedSpan]>> {
        let spans = self.line_cache.get(key).cloned();
        if spans.is_some() {
            self.stats.cache_hits += 1;
        } else {
            self.stats.cache_misses += 1;
        }
        spans
    }

    pub(super) fn record_span_cache_hit(&mut self) {
        self.stats.cache_hits += 1;
    }

    pub(super) fn insert_line(
        &mut self,
        key: HighlightSpanCacheKey,
        spans: Arc<[SyntaxHighlightedSpan]>,
    ) {
        if self.line_cache_capacity == 0 {
            return;
        }

        self.line_cache.put(key, spans);
    }

    pub(super) fn cached_tree(&mut self, key: &ParseTreeCacheKey) -> Option<Tree> {
        match &self.tree {
            Some((cached_key, tree)) if cached_key == key => {
                self.stats.parse_hits += 1;
                Some(tree.clone())
            }
            _ => None,
        }
    }

    pub(super) fn store_cached_tree(&mut self, key: ParseTreeCacheKey, tree: Tree) {
        self.tree = Some((key, tree));
        self.stats.parse_hits += 1;
    }

    pub(super) fn store_parsed_tree(
        &mut self,
        key: ParseTreeCacheKey,
        tree: Tree,
        line_count: usize,
    ) {
        self.tree = Some((key, tree));
        self.stats.parse_misses += 1;
        self.stats.parsed_trees += 1;
        self.stats.parsed_lines += line_count;
    }
}

pub(super) fn estimated_tree_bytes(content_len: usize, line_count: usize) -> usize {
    content_len
        .saturating_add(line_count.saturating_mul(mem::size_of::<usize>()))
        .max(1)
}

fn estimated_span_bytes(spans: &[SyntaxHighlightedSpan]) -> usize {
    spans
        .iter()
        .map(|span| mem::size_of::<SyntaxHighlightedSpan>().saturating_add(span.text.len()))
        .sum::<usize>()
        .max(1)
}

fn nonzero_capacity(capacity: usize) -> NonZeroUsize {
    NonZeroUsize::new(capacity.max(1)).expect("cache capacity is forced non-zero")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::syntax_highlight::SyntaxStyle;

    fn document_key(name: &str, content_hash: u64) -> HighlightDocumentCacheKey {
        HighlightDocumentCacheKey::new(
            Some(format!("src/{name}.rs")),
            content_hash,
            "rust",
            Some("rust".to_string()),
            Some(ParserCacheIdentity {
                runtime: crate::services::syntax_highlight::registry::ParserRuntime::Native,
                language_abi: tree_sitter::LANGUAGE_VERSION,
                tree_sitter_language_version: tree_sitter::LANGUAGE_VERSION,
                source: "test".to_string(),
            }),
            Some(QuerySetIdentity {
                version: 1,
                highlights: Some(11),
                injections: None,
                locals: None,
            }),
            99,
        )
    }

    fn plain_spans(text: &str) -> Arc<[SyntaxHighlightedSpan]> {
        Arc::from(
            vec![SyntaxHighlightedSpan {
                text: text.to_string(),
                style: SyntaxStyle::default(),
            }]
            .into_boxed_slice(),
        )
    }

    #[test]
    fn highlight_span_cache_evicts_lru_by_count() {
        let mut cache = HighlightSpanCache::with_limits(2, 1024 * 1024);
        let a = HighlightSpanCacheKey::new(document_key("a", 1), 1, 1);
        let b = HighlightSpanCacheKey::new(document_key("b", 2), 1, 1);
        let c = HighlightSpanCacheKey::new(document_key("c", 3), 1, 1);
        cache.insert(a.clone(), plain_spans("a"));
        cache.insert(b.clone(), plain_spans("b"));

        assert_eq!(cache.get(&a).unwrap()[0].text, "a");
        cache.insert(c.clone(), plain_spans("c"));

        assert!(cache.get(&a).is_some());
        assert!(cache.get(&b).is_none());
        assert_eq!(cache.get(&c).unwrap()[0].text, "c");
    }
}
