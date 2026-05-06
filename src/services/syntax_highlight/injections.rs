use std::collections::BTreeMap;
use std::ops::Range;

use tree_sitter::{QueryCapture, QueryCursor, QueryMatch, QueryProperty, StreamingIterator, Tree};

use super::registry::TreeSitterSyntax;

#[derive(Debug, Clone)]
pub(super) struct Injection {
    pub(super) language: String,
    pub(super) ranges: Vec<Range<usize>>,
}

pub(super) struct InjectionPlanner;

impl InjectionPlanner {
    pub(super) fn new() -> Self {
        Self
    }

    pub(super) fn injections_for_tree(
        &self,
        syntax: &TreeSitterSyntax,
        tree: &Tree,
        content: &str,
    ) -> Vec<Injection> {
        let Some(query) = syntax.injections_query() else {
            let _ = syntax.locals_query();
            return Vec::new();
        };

        let capture_names = query.capture_names();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(query, tree.root_node(), content.as_bytes());
        let mut injections = Vec::new();
        let mut combined = BTreeMap::<String, Vec<Range<usize>>>::new();

        while let Some(query_match) = matches.next() {
            let properties = query.property_settings(query_match.pattern_index);
            let Some(language) =
                injection_language(properties, query_match, capture_names, content)
            else {
                continue;
            };
            let ranges = content_ranges(query_match, capture_names, content);
            if ranges.is_empty() {
                continue;
            }

            if properties
                .iter()
                .any(|property| property.key.as_ref() == "injection.combined")
            {
                combined.entry(language).or_default().extend(ranges);
            } else {
                let ranges = normalize_ranges(content, ranges);
                if !ranges.is_empty() {
                    injections.push(Injection { language, ranges });
                }
            }
        }

        injections.extend(combined.into_iter().filter_map(|(language, ranges)| {
            let ranges = normalize_ranges(content, ranges);
            (!ranges.is_empty()).then_some(Injection { language, ranges })
        }));
        injections.sort_by(|left, right| {
            let left_start = left.ranges.first().map_or(usize::MAX, |range| range.start);
            let right_start = right.ranges.first().map_or(usize::MAX, |range| range.start);
            (left_start, &left.language).cmp(&(right_start, &right.language))
        });
        injections
    }
}

fn injection_language(
    properties: &[QueryProperty],
    query_match: &QueryMatch<'_, '_>,
    capture_names: &[&str],
    content: &str,
) -> Option<String> {
    for property in properties {
        if property.key.as_ref() != "injection.language" {
            continue;
        }

        if let Some(capture_id) = property.capture_id {
            if let Some(language) =
                language_from_capture_id(query_match.captures, capture_id, content)
            {
                return Some(language);
            }
        } else if let Some(value) = property.value.as_deref().and_then(normalize_language_text) {
            return Some(value);
        }
    }

    query_match.captures.iter().find_map(|capture| {
        capture_name(capture_names, capture)
            .filter(|name| *name == "injection.language")
            .and_then(|_| node_text(capture, content))
            .and_then(normalize_language_text)
    })
}

fn language_from_capture_id(
    captures: &[QueryCapture<'_>],
    capture_id: usize,
    content: &str,
) -> Option<String> {
    captures
        .iter()
        .find(|capture| capture.index as usize == capture_id)
        .and_then(|capture| node_text(capture, content))
        .and_then(normalize_language_text)
}

fn normalize_language_text(language: &str) -> Option<String> {
    let language = language.trim();
    (!language.is_empty()).then(|| language.to_string())
}

fn content_ranges(
    query_match: &QueryMatch<'_, '_>,
    capture_names: &[&str],
    content: &str,
) -> Vec<Range<usize>> {
    query_match
        .captures
        .iter()
        .filter(|capture| {
            capture_name(capture_names, capture).is_some_and(|name| name == "injection.content")
        })
        .filter_map(|capture| node_range(capture, content))
        .collect()
}

fn capture_name<'a>(capture_names: &'a [&str], capture: &QueryCapture<'_>) -> Option<&'a str> {
    capture_names.get(capture.index as usize).copied()
}

fn node_text<'a>(capture: &QueryCapture<'_>, content: &'a str) -> Option<&'a str> {
    let range = node_range(capture, content)?;
    content.get(range)
}

fn node_range(capture: &QueryCapture<'_>, content: &str) -> Option<Range<usize>> {
    let start = capture.node.start_byte();
    let end = capture.node.end_byte();
    (start < end
        && end <= content.len()
        && content.is_char_boundary(start)
        && content.is_char_boundary(end))
    .then_some(start..end)
}

fn normalize_ranges(content: &str, ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    let mut ranges = ranges
        .into_iter()
        .filter(|range| {
            range.start < range.end
                && range.end <= content.len()
                && content.is_char_boundary(range.start)
                && content.is_char_boundary(range.end)
        })
        .collect::<Vec<_>>();
    ranges.sort_unstable_by_key(|range| (range.start, range.end));

    let mut normalized: Vec<Range<usize>> = Vec::new();
    for range in ranges {
        match normalized.last_mut() {
            Some(last) if range.start <= last.end => {
                last.end = last.end.max(range.end);
            }
            _ => normalized.push(range),
        }
    }
    normalized
}

