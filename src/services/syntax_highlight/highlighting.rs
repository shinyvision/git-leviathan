use std::collections::{HashMap, VecDeque};
use std::ops::Range;
use std::sync::{Arc, LazyLock, Mutex};

use iced::Color;
use tree_sitter::{QueryCursor, StreamingIterator, Tree};

use super::injections::{Injection, InjectionPlanner};
use super::parser_loading::ParserLoader;
use super::registry::{BuiltinSyntaxRegistry, TreeSitterSyntax};
use super::{HighlightDocument, SyntaxHighlightedSpan, SyntaxStyle};

pub(super) const MAX_HIGHLIGHT_LINE_BYTES: usize = 16 * 1024;

const MAX_HIGHLIGHT_LINE_CHARS: usize = 8 * 1024;
const MAX_HIGHLIGHT_SPANS_PER_LINE: usize = 256;
const MAX_INJECTION_DEPTH: u8 = 5;
const INJECTION_CACHE_CAPACITY: usize = 32;

static INJECTION_CACHE: LazyLock<Mutex<InjectionCache>> =
    LazyLock::new(|| Mutex::new(InjectionCache::new()));

pub(super) struct TreeSitterHighlighter {
    injections: InjectionPlanner,
}

impl TreeSitterHighlighter {
    pub(super) fn new() -> Self {
        Self {
            injections: InjectionPlanner::new(),
        }
    }

    pub(super) fn highlight_line_from_tree(
        &self,
        document: &HighlightDocument,
        syntax: &TreeSitterSyntax,
        tree: &Tree,
        registry: &BuiltinSyntaxRegistry,
        parser_loader: &ParserLoader,
        line_number: u32,
    ) -> Option<Arc<[SyntaxHighlightedSpan]>> {
        self.highlight_range_from_tree(
            document,
            syntax,
            tree,
            registry,
            parser_loader,
            line_number..=line_number,
        )
        .into_iter()
        .find_map(|(highlighted_line, spans)| (highlighted_line == line_number).then_some(spans))
    }

    pub(super) fn highlight_range_from_tree(
        &self,
        document: &HighlightDocument,
        syntax: &TreeSitterSyntax,
        tree: &Tree,
        registry: &BuiltinSyntaxRegistry,
        parser_loader: &ParserLoader,
        lines: std::ops::RangeInclusive<u32>,
    ) -> Vec<(u32, Arc<[SyntaxHighlightedSpan]>)> {
        let start_line = *lines.start();
        let end_line = *lines.end();
        let Some(first_range) = document.line_range(start_line) else {
            return Vec::new();
        };
        let Some(last_range) = document.line_range(end_line) else {
            return Vec::new();
        };
        let target_range = first_range.start..last_range.end;
        let mut ancestors = vec![syntax.name()];
        let document_range = 0..document.content().len();
        let mut walk = HighlightWalk {
            planner: &self.injections,
            registry,
            parser_loader,
            content: document.content(),
            content_hash: document.content_hash(),
            target_range: &target_range,
            order: 0,
        };
        let styled_ranges =
            walk.collect_recursive(syntax, tree, &[document_range], 0, &mut ancestors);

        (start_line..=end_line)
            .filter_map(|line_number| {
                let line = document.line(line_number)?;
                if line_exceeds_highlight_limits(line) {
                    return Some((line_number, plain_spans_for_line(line)));
                }
                let line_range = document.line_range(line_number)?;
                let line_styles = styled_ranges
                    .iter()
                    .copied()
                    .filter_map(|range| {
                        let start = range.start.max(line_range.start);
                        let end = range.end.min(line_range.end);
                        (start < end).then(|| StyledRange {
                            start,
                            end,
                            len: end - start,
                            ..range
                        })
                    })
                    .collect();
                Some((
                    line_number,
                    spans_for_line(document.content(), line_range, line_styles),
                ))
            })
            .collect()
    }
}

struct ChildLayer<'a> {
    syntax: &'a TreeSitterSyntax,
    tree: Tree,
    ranges: Vec<Range<usize>>,
}

struct HighlightWalk<'a> {
    planner: &'a InjectionPlanner,
    registry: &'a BuiltinSyntaxRegistry,
    parser_loader: &'a ParserLoader,
    content: &'a str,
    content_hash: u64,
    target_range: &'a Range<usize>,
    order: usize,
}

#[derive(Debug, Clone, Copy)]
struct StyledRange {
    start: usize,
    end: usize,
    style: SyntaxStyle,
    priority: u8,
    len: usize,
    depth: u8,
    order: usize,
}

impl<'a> HighlightWalk<'a> {
    fn collect_recursive(
        &mut self,
        syntax: &TreeSitterSyntax,
        tree: &Tree,
        scope_ranges: &[Range<usize>],
        depth: u8,
        ancestors: &mut Vec<&'static str>,
    ) -> Vec<StyledRange> {
        if !ranges_intersect(scope_ranges, self.target_range) {
            return Vec::new();
        }

        let children = self.child_layers(syntax, tree, depth, ancestors);
        let child_owned_ranges = children
            .iter()
            .flat_map(|child| child.ranges.iter().cloned())
            .collect::<Vec<_>>();
        let mut ranges =
            self.collect_styled_ranges(syntax, tree, scope_ranges, &child_owned_ranges, depth);

        for child in children {
            ancestors.push(child.syntax.name());
            ranges.extend(self.collect_recursive(
                child.syntax,
                &child.tree,
                &child.ranges,
                depth + 1,
                ancestors,
            ));
            ancestors.pop();
        }

        ranges
    }

    fn child_layers(
        &self,
        syntax: &TreeSitterSyntax,
        tree: &Tree,
        depth: u8,
        ancestors: &[&'static str],
    ) -> Vec<ChildLayer<'a>> {
        if depth >= MAX_INJECTION_DEPTH {
            return Vec::new();
        }

        let injections = if depth == 0 {
            cached_injections_for_tree(self.planner, self.content_hash, syntax, tree, self.content)
        } else {
            Arc::from(
                self.planner
                    .injections_for_tree(syntax, tree, self.content)
                    .into_boxed_slice(),
            )
        };

        injections
            .iter()
            .filter(|injection| ranges_intersect(&injection.ranges, self.target_range))
            .cloned()
            .filter_map(|injection| self.child_layer_from_injection(ancestors, injection))
            .collect()
    }

    fn child_layer_from_injection(
        &self,
        ancestors: &[&'static str],
        injection: Injection,
    ) -> Option<ChildLayer<'a>> {
        let syntax = self
            .registry
            .syntax_for_injection_language(&injection.language)?;
        if ancestors.contains(&syntax.name()) {
            return None;
        }

        let tree =
            self.parser_loader
                .parse_tree_for_ranges(syntax, self.content, &injection.ranges)?;
        Some(ChildLayer {
            syntax,
            tree,
            ranges: injection.ranges,
        })
    }

    fn collect_styled_ranges(
        &mut self,
        syntax: &TreeSitterSyntax,
        tree: &Tree,
        scope_ranges: &[Range<usize>],
        excluded_ranges: &[Range<usize>],
        depth: u8,
    ) -> Vec<StyledRange> {
        if self.target_range.is_empty() {
            return Vec::new();
        }

        let Some(query) = syntax.highlights_query() else {
            return Vec::new();
        };
        let mut cursor = QueryCursor::new();
        let mut captures = cursor.captures(query, tree.root_node(), self.content.as_bytes());
        captures.set_byte_range(self.target_range.clone());
        let names = query.capture_names();
        let mut ranges = Vec::new();

        while let Some((query_match, capture_index)) = captures.next() {
            let capture = query_match.captures[*capture_index];
            let Some(name) = names.get(capture.index as usize) else {
                continue;
            };
            let Some((style, priority)) = style_for_capture(name) else {
                continue;
            };
            let start = capture.node.start_byte().max(self.target_range.start);
            let end = capture.node.end_byte().min(self.target_range.end);
            if start >= end
                || !self.content.is_char_boundary(start)
                || !self.content.is_char_boundary(end)
            {
                continue;
            }
            for scoped in intersect_with_ranges(start..end, scope_ranges) {
                for segment in subtract_ranges(scoped, excluded_ranges) {
                    if segment.start >= segment.end
                        || !self.content.is_char_boundary(segment.start)
                        || !self.content.is_char_boundary(segment.end)
                    {
                        continue;
                    }
                    ranges.push(StyledRange {
                        start: segment.start,
                        end: segment.end,
                        style,
                        priority,
                        len: segment.end - segment.start,
                        depth,
                        order: self.order,
                    });
                    self.order += 1;
                }
            }
        }

        ranges
    }
}

pub(super) fn clear_injection_cache() {
    if let Ok(mut cache) = INJECTION_CACHE.lock() {
        *cache = InjectionCache::new();
    }
}

fn cached_injections_for_tree(
    planner: &InjectionPlanner,
    content_hash: u64,
    syntax: &TreeSitterSyntax,
    tree: &Tree,
    content: &str,
) -> Arc<[Injection]> {
    let key = InjectionCacheKey {
        content_hash,
        syntax_identity: syntax.cache_identity(),
    };
    if let Ok(mut cache) = INJECTION_CACHE.lock() {
        if let Some(injections) = cache.get(&key) {
            return injections;
        }
    }

    let injections: Arc<[Injection]> = Arc::from(
        planner
            .injections_for_tree(syntax, tree, content)
            .into_boxed_slice(),
    );
    if let Ok(mut cache) = INJECTION_CACHE.lock() {
        cache.insert(key, Arc::clone(&injections));
    }
    injections
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct InjectionCacheKey {
    content_hash: u64,
    syntax_identity: u64,
}

struct InjectionCache {
    map: HashMap<InjectionCacheKey, Arc<[Injection]>>,
    order: VecDeque<InjectionCacheKey>,
}

impl InjectionCache {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn get(&mut self, key: &InjectionCacheKey) -> Option<Arc<[Injection]>> {
        let hit = self.map.get(key).cloned();
        if hit.is_some() {
            touch_key(&mut self.order, key);
        }
        hit
    }

    fn insert(&mut self, key: InjectionCacheKey, injections: Arc<[Injection]>) {
        if self.map.contains_key(&key) {
            touch_key(&mut self.order, &key);
            self.map.insert(key, injections);
            return;
        }
        while self.order.len() >= INJECTION_CACHE_CAPACITY {
            if let Some(evict) = self.order.pop_front() {
                self.map.remove(&evict);
            } else {
                break;
            }
        }
        self.order.push_back(key.clone());
        self.map.insert(key, injections);
    }
}

fn touch_key<T>(order: &mut VecDeque<T>, key: &T)
where
    T: Clone + Eq,
{
    if let Some(pos) = order.iter().position(|cached_key| cached_key == key) {
        let cached_key = order.remove(pos).unwrap();
        order.push_back(cached_key);
    }
}

fn ranges_intersect(ranges: &[Range<usize>], line_range: &Range<usize>) -> bool {
    ranges
        .iter()
        .any(|range| range.start < line_range.end && line_range.start < range.end)
}

fn intersect_with_ranges(range: Range<usize>, scopes: &[Range<usize>]) -> Vec<Range<usize>> {
    scopes
        .iter()
        .filter_map(|scope| {
            let start = range.start.max(scope.start);
            let end = range.end.min(scope.end);
            (start < end).then_some(start..end)
        })
        .collect()
}

fn subtract_ranges(range: Range<usize>, excluded: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut segments = vec![range];
    for excluded_range in excluded {
        let mut next = Vec::new();
        for segment in segments {
            if excluded_range.end <= segment.start || excluded_range.start >= segment.end {
                next.push(segment);
                continue;
            }
            if segment.start < excluded_range.start {
                next.push(segment.start..excluded_range.start.min(segment.end));
            }
            if excluded_range.end < segment.end {
                next.push(excluded_range.end.max(segment.start)..segment.end);
            }
        }
        segments = next;
        if segments.is_empty() {
            break;
        }
    }
    segments
}

fn spans_for_line(
    content: &str,
    line_range: Range<usize>,
    styled_ranges: Vec<StyledRange>,
) -> Arc<[SyntaxHighlightedSpan]> {
    if line_range.is_empty() {
        return plain_spans_for_line("");
    }
    if styled_ranges.len() > MAX_HIGHLIGHT_SPANS_PER_LINE {
        return plain_spans_for_line(&content[line_range]);
    }

    let mut boundaries = Vec::with_capacity(styled_ranges.len().saturating_mul(2) + 2);
    boundaries.push(line_range.start);
    boundaries.push(line_range.end);
    for range in &styled_ranges {
        boundaries.push(range.start);
        boundaries.push(range.end);
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    let mut spans = Vec::new();
    for pair in boundaries.windows(2) {
        let start = pair[0];
        let end = pair[1];
        if start >= end {
            continue;
        }
        let style = best_style_for_segment(&styled_ranges, start, end).unwrap_or_default();
        push_span(&mut spans, &content[start..end], style);
    }

    if spans.is_empty() {
        plain_spans_for_line(&content[line_range])
    } else {
        Arc::from(spans.into_boxed_slice())
    }
}

fn best_style_for_segment(ranges: &[StyledRange], start: usize, end: usize) -> Option<SyntaxStyle> {
    ranges
        .iter()
        .filter(|range| range.start <= start && range.end >= end)
        .max_by_key(|range| {
            (
                range.depth,
                range.priority,
                usize::MAX - range.len,
                range.order,
            )
        })
        .map(|range| range.style)
}

fn push_span(spans: &mut Vec<SyntaxHighlightedSpan>, text: &str, style: SyntaxStyle) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = spans.last_mut() {
        if last.style == style {
            last.text.push_str(text);
            return;
        }
    }
    spans.push(SyntaxHighlightedSpan {
        text: text.to_string(),
        style,
    });
}

fn style_for_capture(name: &str) -> Option<(SyntaxStyle, u8)> {
    style_for_capture_name(name).or_else(|| {
        name.rsplit_once('.')
            .and_then(|(fallback, _)| style_for_capture_name(fallback))
    })
}

fn style_for_capture_name(name: &str) -> Option<(SyntaxStyle, u8)> {
    let style =
        if name.starts_with('_') || name.starts_with("local.") || name.starts_with("injection.") {
            return None;
        } else if name.contains("comment") {
            SyntaxStyle {
                color: Some(rgb(0x7f, 0x84, 0x8e)),
                bold: false,
                italic: true,
            }
        } else if name.contains("string") {
            SyntaxStyle::colored(rgb(0x98, 0xc3, 0x79))
        } else if name.contains("number")
            || name.contains("float")
            || name.contains("constant")
            || name.contains("boolean")
        {
            SyntaxStyle::colored(rgb(0xd1, 0x9a, 0x66))
        } else if name.contains("keyword") || name.contains("operator") {
            SyntaxStyle::colored(rgb(0xc6, 0x78, 0xdd))
        } else if name.contains("function") || name.contains("method") {
            SyntaxStyle::colored(rgb(0x61, 0xaf, 0xef))
        } else if name.contains("type")
            || name.contains("constructor")
            || name.contains("class")
            || name.contains("tag")
        {
            SyntaxStyle::colored(rgb(0xe5, 0xc0, 0x7b))
        } else if name.contains("property")
            || name.contains("attribute")
            || name.contains("parameter")
            || name.contains("variable")
        {
            SyntaxStyle::colored(rgb(0xe0, 0x6c, 0x75))
        } else if name.contains("punctuation") {
            SyntaxStyle::colored(rgb(0xab, 0xb2, 0xbf))
        } else if name.contains("markup") {
            SyntaxStyle::colored(rgb(0x56, 0xb6, 0xc2))
        } else {
            return None;
        };

    Some((style, capture_priority(name)))
}

fn capture_priority(name: &str) -> u8 {
    if name.contains("comment") || name.contains("string") {
        90
    } else if name.contains("keyword") || name.contains("function") || name.contains("method") {
        80
    } else if name.contains("number") || name.contains("constant") || name.contains("boolean") {
        70
    } else if name.contains("type") || name.contains("tag") || name.contains("property") {
        60
    } else if name.contains("variable") || name.contains("attribute") {
        50
    } else {
        40
    }
}

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: 1.0,
    }
}

fn line_exceeds_highlight_limits(line: &str) -> bool {
    line.len() > MAX_HIGHLIGHT_LINE_BYTES || line.chars().count() > MAX_HIGHLIGHT_LINE_CHARS
}

pub(super) fn plain_spans_for_line(line: &str) -> Arc<[SyntaxHighlightedSpan]> {
    Arc::from(
        vec![SyntaxHighlightedSpan {
            text: line.to_string(),
            style: SyntaxStyle::default(),
        }]
        .into_boxed_slice(),
    )
}

pub(super) fn highlight_bounds(
    document: &HighlightDocument,
    start_line: u32,
    end_line: u32,
) -> Option<(u32, u32)> {
    if start_line == 0 || end_line == 0 || start_line > end_line || document.line_count() == 0 {
        return None;
    }
    let line_count = document.line_count() as u32;
    let start_line = start_line.min(line_count);
    let end_line = end_line.min(line_count);
    if start_line > end_line {
        None
    } else {
        Some((start_line, end_line))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_names_fall_back_to_parent_segments() {
        let function = style_for_capture("function").unwrap();
        let builtin = style_for_capture("function.builtin").unwrap();

        assert_eq!(builtin, function);
    }
}
