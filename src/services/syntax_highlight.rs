use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, LazyLock, Mutex};

use iced::Color;
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, ThemeSet};
use syntect::parsing::syntax_definition::{Pattern, SyntaxDefinition};
use syntect::parsing::{Scope, SyntaxSet, SyntaxSetBuilder};

static SYNTAX_SERVICE: LazyLock<SyntaxHighlightService> =
    LazyLock::new(SyntaxHighlightService::new);

/// Skip real highlighting above this size; fall back to plain spans.
/// Syntect's Oniguruma regexes blow up on very large or pathological files.
const MAX_HIGHLIGHT_BYTES: usize = 512 * 1024;
const MAX_HIGHLIGHT_LINES: usize = 10_000;
const HIGHLIGHT_CACHE_CAPACITY: usize = 16;

struct HighlightCache {
    map: HashMap<(u64, String), Arc<HighlightedFile>>,
    order: VecDeque<(u64, String)>,
}

impl HighlightCache {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn get(&mut self, key: &(u64, String)) -> Option<Arc<HighlightedFile>> {
        let hit = self.map.get(key).cloned();
        if hit.is_some() {
            if let Some(pos) = self.order.iter().position(|k| k == key) {
                let k = self.order.remove(pos).unwrap();
                self.order.push_back(k);
            }
        }
        hit
    }

    fn insert(&mut self, key: (u64, String), value: Arc<HighlightedFile>) {
        if self.map.contains_key(&key) {
            if let Some(pos) = self.order.iter().position(|k| k == &key) {
                let k = self.order.remove(pos).unwrap();
                self.order.push_back(k);
            }
            self.map.insert(key, value);
            return;
        }
        while self.order.len() >= HIGHLIGHT_CACHE_CAPACITY {
            if let Some(evict) = self.order.pop_front() {
                self.map.remove(&evict);
            } else {
                break;
            }
        }
        self.order.push_back(key.clone());
        self.map.insert(key, value);
    }
}

static HIGHLIGHT_CACHE: LazyLock<Mutex<HighlightCache>> =
    LazyLock::new(|| Mutex::new(HighlightCache::new()));

// Keep this sorted after Syntect's bundled PHP contexts so existing direct
// context references survive the rebuild below.
const PHP_ATTRIBUTE_CONTEXT: &str = "zzzz-php-attribute";
const PHP_ATTRIBUTE_SYNTAX_PATCH: &str = r#"
%YAML 1.2
---
name: PHP Attribute Patch
scope: source.php.attribute-patch
hidden: true
contexts:
  main:
    - match: '#\['
      scope: punctuation.definition.attribute.begin.php
      push: zzzz-php-attribute
  zzzz-php-attribute:
    - meta_scope: meta.attribute.php
    - include: string-double-quoted
    - include: string-single-quoted
    - match: '\['
      scope: punctuation.section.brackets.begin.php
      push: zzzz-php-attribute
    - match: '\]'
      scope: punctuation.definition.attribute.end.php
      pop: 1
"#;

pub struct SyntaxHighlightService {
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
}

impl SyntaxHighlightService {
    fn new() -> Self {
        let syntax_set = build_syntax_set();
        let theme_set = ThemeSet::load_defaults();
        Self {
            syntax_set,
            theme_set,
        }
    }

    fn get_syntax_for_code(
        &self,
        code: &str,
        file_extension: &str,
    ) -> &syntect::parsing::SyntaxReference {
        self.syntax_set
            .find_syntax_by_token(file_extension)
            .or_else(|| {
                code.lines()
                    .next()
                    .and_then(|line| self.syntax_set.find_syntax_by_first_line(line))
            })
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text())
    }

    fn get_theme(&self) -> &syntect::highlighting::Theme {
        self.theme_set
            .themes
            .get("base16-ocean.dark")
            .unwrap_or_else(|| self.theme_set.themes.values().next().unwrap())
    }

    pub fn highlight(&self, code: &str, file_extension: &str) -> HighlightedFile {
        if code.len() > MAX_HIGHLIGHT_BYTES {
            return plain_highlighted_file(code);
        }

        let syntax = self.get_syntax_for_code(code, file_extension);
        let theme = self.get_theme();
        let mut highlight_lines = HighlightLines::new(syntax, theme);
        let mut spans = Vec::new();
        let mut line_ranges = Vec::new();

        for (line_count, line) in code.lines().enumerate() {
            if line_count >= MAX_HIGHLIGHT_LINES {
                return plain_highlighted_file(code);
            }

            let line_start = spans.len() as u32;
            let ranges = highlight_lines
                .highlight_line(line, &self.syntax_set)
                .unwrap_or_else(|_| vec![(Default::default(), line)]);

            for (style, text) in ranges {
                let fg = style.foreground;
                spans.push(SyntaxHighlightedSpan {
                    text: text.to_string(),
                    style: SyntaxStyle {
                        color: Some(Color {
                            r: fg.r as f32 / 255.0,
                            g: fg.g as f32 / 255.0,
                            b: fg.b as f32 / 255.0,
                            a: 1.0,
                        }),
                        bold: style.font_style.contains(FontStyle::BOLD),
                        italic: style.font_style.contains(FontStyle::ITALIC),
                    },
                });
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
                text: code.to_string(),
                style: SyntaxStyle::default(),
            });
        }

        HighlightedFile { spans, line_ranges }
    }
}

fn build_syntax_set() -> SyntaxSet {
    let syntax_set = two_face::syntax::extra_no_newlines();
    let source_php = Scope::new("source.php").unwrap();
    let builder = syntax_set.into_builder();
    let mut patched_builder = SyntaxSetBuilder::new();

    for syntax in builder.syntaxes() {
        let mut syntax = syntax.clone();
        if syntax.scope == source_php {
            patch_php_attribute_syntax(&mut syntax);
        }
        patched_builder.add(syntax);
    }

    patched_builder.build()
}

fn patch_php_attribute_syntax(syntax: &mut SyntaxDefinition) {
    if syntax.contexts.contains_key(PHP_ATTRIBUTE_CONTEXT) {
        return;
    }

    let mut patch = SyntaxDefinition::load_from_str(PHP_ATTRIBUTE_SYNTAX_PATCH, false, None)
        .expect("PHP attribute syntax patch should parse");
    let attribute_context = patch
        .contexts
        .remove(PHP_ATTRIBUTE_CONTEXT)
        .expect("PHP attribute syntax patch should include attribute context");
    let attribute_start_pattern = patch
        .contexts
        .remove("main")
        .and_then(|mut context| context.patterns.pop())
        .expect("PHP attribute syntax patch should include start pattern");

    syntax
        .contexts
        .insert(PHP_ATTRIBUTE_CONTEXT.to_string(), attribute_context);

    if let Some(comments) = syntax.contexts.get_mut("comments") {
        let insert_at = comments
            .patterns
            .iter()
            .position(|pattern| {
                matches!(
                    pattern,
                    Pattern::Match(pattern) if pattern.regex.regex_str() == "#"
                )
            })
            .unwrap_or(0);
        comments.patterns.insert(insert_at, attribute_start_pattern);
    }
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

/// Fully-highlighted file with a precomputed per-line span index so that
/// rendering can look up line N's spans in O(1).
#[derive(Debug, Clone)]
pub struct HighlightedFile {
    spans: Vec<SyntaxHighlightedSpan>,
    /// `line_ranges[N-1] = (start, end)` into `spans`, excluding the trailing `\n` span.
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
}

fn plain_highlighted_file(code: &str) -> HighlightedFile {
    let mut spans = Vec::new();
    let mut line_ranges = Vec::new();
    for line in code.lines() {
        let start = spans.len() as u32;
        spans.push(SyntaxHighlightedSpan {
            text: line.to_string(),
            style: SyntaxStyle::default(),
        });
        let end = spans.len() as u32;
        line_ranges.push((start, end));
        spans.push(SyntaxHighlightedSpan {
            text: "\n".to_string(),
            style: SyntaxStyle::default(),
        });
    }
    if spans.is_empty() {
        spans.push(SyntaxHighlightedSpan {
            text: code.to_string(),
            style: SyntaxStyle::default(),
        });
    }
    HighlightedFile { spans, line_ranges }
}

pub fn get_syntax_service() -> &'static SyntaxHighlightService {
    &SYNTAX_SERVICE
}

/// Highlight a whole file, reusing a cached result when the `(content_hash, extension)`
/// pair has been seen before. Returns an [`Arc`] so callers can cheaply share the
/// result across UI frames and messages.
pub fn highlight_file(code: &str, file_extension: &str) -> Arc<HighlightedFile> {
    let mut hasher = DefaultHasher::new();
    code.hash(&mut hasher);
    let key = (hasher.finish(), file_extension.to_string());

    if let Ok(mut cache) = HIGHLIGHT_CACHE.lock() {
        if let Some(hit) = cache.get(&key) {
            return hit;
        }
    }

    let result = Arc::new(get_syntax_service().highlight(code, file_extension));

    if let Ok(mut cache) = HIGHLIGHT_CACHE.lock() {
        cache.insert(key, Arc::clone(&result));
    }

    result
}

pub fn file_extension_from_path(path: &str) -> String {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use syntect::easy::ScopeRegionIterator;
    use syntect::parsing::{ParseState, ScopeStack};

    #[test]
    fn whole_file_highlighting_carries_php_state_to_later_lines() {
        let file = highlight_file(
            "<?php\n\nreadonly class User {\n    public function __construct(public string $name) {}\n}\n",
            "php",
        );
        let class_line = file.line(3);
        let colors: HashSet<_> = class_line
            .iter()
            .filter_map(|span| span.style.color)
            .map(|color| {
                (
                    color.r.to_bits(),
                    color.g.to_bits(),
                    color.b.to_bits(),
                    color.a.to_bits(),
                )
            })
            .collect();

        assert!(
            colors.len() > 1,
            "expected PHP syntax scopes to produce more than one style on the class line"
        );
    }

    #[test]
    fn php_attributes_do_not_enter_comment_scope() {
        let service = SyntaxHighlightService::new();
        let code = "<?php\n#[Route(path: '/users', methods: ['GET'])]\nfinal class User {}\n";

        let attribute_scopes = scopes_for_line(&service, code, 2);
        let class_scopes = scopes_for_line(&service, code, 3);

        assert!(
            attribute_scopes
                .iter()
                .any(|scope| scope.contains("meta.attribute.php")),
            "expected PHP attribute line to use attribute scope"
        );
        assert!(
            attribute_scopes
                .iter()
                .all(|scope| !scope.contains("comment.line")),
            "expected PHP attribute line to avoid line comment scope"
        );
        assert!(
            class_scopes
                .iter()
                .all(|scope| !scope.contains("comment.line")),
            "expected PHP attribute line not to leak comment scope"
        );
    }

    #[test]
    fn extract_spans_for_line_can_return_the_last_line() {
        let file = highlight_file("<?php\nfinal class LastLine {}", "php");
        let last_line = file.line(2);

        assert!(!last_line.is_empty());
        assert_eq!(
            last_line
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>(),
            "final class LastLine {}"
        );
    }

    #[test]
    fn oversized_file_falls_back_to_plain_spans() {
        let huge = "x".repeat(MAX_HIGHLIGHT_BYTES + 1);
        let file = highlight_file(&huge, "rs");
        let line = file.line(1);
        assert_eq!(line.len(), 1);
        assert!(line[0].style.color.is_none());
    }

    #[test]
    fn cache_returns_same_arc_for_repeated_highlight() {
        let code = "fn main() { println!(\"hi\"); }\n";
        let a = highlight_file(code, "rs");
        let b = highlight_file(code, "rs");
        assert!(Arc::ptr_eq(&a, &b));
    }

    fn scopes_for_line(
        service: &SyntaxHighlightService,
        code: &str,
        target_line_number: usize,
    ) -> Vec<String> {
        let syntax = service.get_syntax_for_code(code, "php");
        let mut parse_state = ParseState::new(syntax);
        let mut stack = ScopeStack::new();
        let mut scopes = Vec::new();

        for (line_index, line) in code.lines().enumerate() {
            let ops = parse_state.parse_line(line, &service.syntax_set).unwrap();
            for (text, op) in ScopeRegionIterator::new(&ops, line) {
                stack.apply(op).unwrap();
                if line_index + 1 == target_line_number && !text.is_empty() {
                    scopes.push(stack.to_string());
                }
            }
        }

        scopes
    }
}
