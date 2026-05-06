use std::fmt;

use super::registry::{BuiltinSyntaxRegistry, LanguageMatch};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LanguageId(String);

impl LanguageId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for LanguageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanguageDetectionConfidence {
    Exact,
    High,
    Medium,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanguageDetectionReason {
    Filename,
    CompoundExtension,
    Extension,
    SyntaxToken,
    FirstLine,
    ContentHint,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageDetection {
    pub language_id: LanguageId,
    pub confidence: LanguageDetectionConfidence,
    pub reason: LanguageDetectionReason,
    syntax_key: Option<&'static str>,
}

impl LanguageDetection {
    fn detected(
        language_id: &'static str,
        syntax_key: &'static str,
        confidence: LanguageDetectionConfidence,
        reason: LanguageDetectionReason,
    ) -> Self {
        Self {
            language_id: LanguageId::new(language_id),
            confidence,
            reason,
            syntax_key: Some(syntax_key),
        }
    }

    fn plain() -> Self {
        Self {
            language_id: LanguageId::new("plain"),
            confidence: LanguageDetectionConfidence::None,
            reason: LanguageDetectionReason::Unknown,
            syntax_key: None,
        }
    }

    pub fn syntax_key(&self) -> Option<&'static str> {
        self.syntax_key
    }

    pub(super) fn cache_key(&self) -> &str {
        self.syntax_key.unwrap_or_else(|| self.language_id.as_str())
    }
}

pub(super) fn detect_language(
    content: &str,
    path: Option<&str>,
    syntax_token: &str,
    registry: &BuiltinSyntaxRegistry,
) -> LanguageDetection {
    let trimmed_token = syntax_token.trim();

    if let Some(path) = path.filter(|path| !path.trim().is_empty()) {
        if let Some(detection) = detect_from_path(content, path, registry) {
            return detection;
        }
    }

    if let Some(detection) = detect_from_legacy_token(content, trimmed_token, registry) {
        return detection;
    }

    detect_from_content(content, registry).unwrap_or_else(LanguageDetection::plain)
}

fn detect_from_path(
    content: &str,
    path: &str,
    registry: &BuiltinSyntaxRegistry,
) -> Option<LanguageDetection> {
    let filename = filename_from_path(path);

    if let Some(language_match) = registry.language_for_filename(filename) {
        return Some(detection_from_match(
            content,
            language_match,
            LanguageDetectionConfidence::Exact,
            LanguageDetectionReason::Filename,
        ));
    }

    if let Some(language_match) = registry.language_for_compound_extension(filename) {
        return Some(detection_from_match(
            content,
            language_match,
            LanguageDetectionConfidence::Exact,
            LanguageDetectionReason::CompoundExtension,
        ));
    }

    let extension = extension_from_filename(filename);
    extension.and_then(|extension| {
        registry
            .language_for_extension(extension)
            .map(|language_match| {
                detection_from_match(
                    content,
                    language_match,
                    LanguageDetectionConfidence::Exact,
                    LanguageDetectionReason::Extension,
                )
            })
    })
}

fn detect_from_legacy_token(
    content: &str,
    syntax_token: &str,
    registry: &BuiltinSyntaxRegistry,
) -> Option<LanguageDetection> {
    if syntax_token.is_empty() {
        return None;
    }

    if syntax_token.contains(['/', '\\', '.']) {
        if let Some(detection) = detect_from_path(content, syntax_token, registry) {
            return Some(detection);
        }
    }

    registry
        .language_for_filename(syntax_token)
        .map(|language_match| {
            detection_from_match(
                content,
                language_match,
                LanguageDetectionConfidence::Exact,
                LanguageDetectionReason::Filename,
            )
        })
        .or_else(|| {
            registry
                .language_for_compound_extension(syntax_token)
                .map(|language_match| {
                    detection_from_match(
                        content,
                        language_match,
                        LanguageDetectionConfidence::Exact,
                        LanguageDetectionReason::CompoundExtension,
                    )
                })
        })
        .or_else(|| {
            let token = syntax_token.trim_start_matches('.');
            registry
                .language_for_extension(token)
                .map(|language_match| {
                    detection_from_match(
                        content,
                        language_match,
                        LanguageDetectionConfidence::Exact,
                        LanguageDetectionReason::SyntaxToken,
                    )
                })
        })
}

fn detect_from_content(
    content: &str,
    registry: &BuiltinSyntaxRegistry,
) -> Option<LanguageDetection> {
    let first_line = content.lines().next().unwrap_or("").trim();
    let first_line_lower = first_line.to_ascii_lowercase();

    if let Some(language_match) = registry.language_for_first_line(&first_line_lower) {
        return Some(detection_from_match(
            content,
            language_match,
            LanguageDetectionConfidence::High,
            LanguageDetectionReason::FirstLine,
        ));
    }

    let trimmed = content.trim_start();
    if trimmed.starts_with("<?php") || trimmed.starts_with("<?=") {
        return Some(LanguageDetection::detected(
            "php",
            "php",
            LanguageDetectionConfidence::Medium,
            LanguageDetectionReason::ContentHint,
        ));
    }
    if trimmed.starts_with("<!doctype html") || trimmed.starts_with("<!DOCTYPE html") {
        return Some(LanguageDetection::detected(
            "html",
            "html",
            LanguageDetectionConfidence::Medium,
            LanguageDetectionReason::ContentHint,
        ));
    }
    if first_meaningful_line(trimmed).is_some_and(is_dockerfile_instruction) {
        return Some(LanguageDetection::detected(
            "dockerfile",
            "dockerfile",
            LanguageDetectionConfidence::Medium,
            LanguageDetectionReason::ContentHint,
        ));
    }

    None
}

fn detection_from_match(
    content: &str,
    language_match: LanguageMatch,
    confidence: LanguageDetectionConfidence,
    reason: LanguageDetectionReason,
) -> LanguageDetection {
    let syntax_key =
        if language_match.language_id == "php" && language_match.matched_pattern == "php" {
            if content.trim_start().starts_with("<?") {
                "php"
            } else {
                "php-only"
            }
        } else {
            language_match.syntax_key
        };

    LanguageDetection::detected(language_match.language_id, syntax_key, confidence, reason)
}

fn filename_from_path(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn extension_from_filename(filename: &str) -> Option<&str> {
    let (stem, extension) = filename.rsplit_once('.')?;
    if stem.is_empty() || extension.is_empty() {
        return None;
    }
    Some(extension)
}

fn first_meaningful_line(content: &str) -> Option<&str> {
    content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
}

fn is_dockerfile_instruction(line: &str) -> bool {
    let first = line
        .split_ascii_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    matches!(
        first.as_str(),
        "FROM"
            | "ARG"
            | "ENV"
            | "RUN"
            | "CMD"
            | "COPY"
            | "ADD"
            | "ENTRYPOINT"
            | "WORKDIR"
            | "EXPOSE"
            | "VOLUME"
            | "USER"
            | "LABEL"
            | "SHELL"
            | "STOPSIGNAL"
            | "HEALTHCHECK"
            | "ONBUILD"
    )
}

pub fn file_extension_from_path(path: &str) -> String {
    extension_from_filename(filename_from_path(path))
        .unwrap_or("")
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::super::registry::BuiltinSyntaxRegistry;
    use super::*;

    fn detect(content: &str, path: Option<&str>, syntax_token: &str) -> LanguageDetection {
        let registry = BuiltinSyntaxRegistry::new();
        detect_language(content, path, syntax_token, &registry)
    }

    #[test]
    fn detects_twig_extensions() {
        let twig = detect("", Some("templates/card.twig"), "");
        assert_eq!(twig.language_id.as_str(), "twig");
        assert_eq!(twig.syntax_key(), Some("twig"));
        assert_eq!(twig.reason, LanguageDetectionReason::Extension);

        let html_twig = detect("", Some("templates/card.html.twig"), "");
        assert_eq!(html_twig.language_id.as_str(), "twig");
        assert_eq!(html_twig.syntax_key(), Some("twig"));
        assert_eq!(html_twig.reason, LanguageDetectionReason::CompoundExtension);
    }

    #[test]
    fn detects_yaml_extensions() {
        let yaml = detect("", Some(".github/workflows/ci.yaml"), "");
        assert_eq!(yaml.language_id.as_str(), "yaml");
        assert_eq!(yaml.syntax_key(), Some("yaml"));
        assert_eq!(yaml.reason, LanguageDetectionReason::Extension);

        let yml = detect("", Some("docker-compose.yml"), "");
        assert_eq!(yml.language_id.as_str(), "yaml");
        assert_eq!(yml.syntax_key(), Some("yaml"));
        assert_eq!(yml.reason, LanguageDetectionReason::Extension);
    }

    #[test]
    fn detects_go_extensions() {
        let go = detect("", Some("cmd/server/main.go"), "");
        assert_eq!(go.language_id.as_str(), "go");
        assert_eq!(go.syntax_key(), Some("go"));
        assert_eq!(go.reason, LanguageDetectionReason::Extension);
    }

    #[test]
    fn detects_json_jsonc_and_toml_extensions() {
        let json = detect("", Some("package.json"), "");
        assert_eq!(json.language_id.as_str(), "json");
        assert_eq!(json.syntax_key(), Some("json"));
        assert_eq!(json.reason, LanguageDetectionReason::Extension);

        let jsonc = detect("", Some(".vscode/settings.jsonc"), "");
        assert_eq!(jsonc.language_id.as_str(), "jsonc");
        assert_eq!(jsonc.syntax_key(), Some("json"));
        assert_eq!(jsonc.reason, LanguageDetectionReason::Extension);

        let cargo_toml = detect("", Some("Cargo.toml"), "");
        assert_eq!(cargo_toml.language_id.as_str(), "toml");
        assert_eq!(cargo_toml.syntax_key(), Some("toml"));
        assert_eq!(cargo_toml.reason, LanguageDetectionReason::Extension);
    }

    #[test]
    fn detects_dockerfile_without_extension() {
        let detection = detect("FROM rust:latest\n", Some("deploy/Dockerfile"), "");
        assert_eq!(detection.language_id.as_str(), "dockerfile");
        assert_eq!(detection.syntax_key(), Some("dockerfile"));
        assert_eq!(detection.reason, LanguageDetectionReason::Filename);
    }

    #[test]
    fn compound_extensions_win_before_simple_extensions() {
        let blade = detect("", Some("resources/views/app.blade.php"), "");
        assert_eq!(blade.language_id.as_str(), "blade");
        assert_eq!(blade.syntax_key(), Some("blade"));
        assert_eq!(blade.reason, LanguageDetectionReason::CompoundExtension);

        let tsx_test = detect("", Some("components/Button.test.tsx"), "");
        assert_eq!(tsx_test.language_id.as_str(), "tsx");
        assert_eq!(tsx_test.syntax_key(), Some("tsx"));
        assert_eq!(tsx_test.reason, LanguageDetectionReason::CompoundExtension);
    }

    #[test]
    fn legacy_extension_tokens_still_detect() {
        let detection = detect("fn main() {}\n", None, "rs");
        assert_eq!(detection.language_id.as_str(), "rust");
        assert_eq!(detection.syntax_key(), Some("rust"));
        assert_eq!(detection.reason, LanguageDetectionReason::SyntaxToken);
    }

    #[test]
    fn first_line_and_content_hints_detect_languages() {
        let shebang = detect("#!/usr/bin/env node\nconsole.log('hi');\n", None, "");
        assert_eq!(shebang.language_id.as_str(), "javascript");
        assert_eq!(shebang.syntax_key(), Some("javascript"));
        assert_eq!(shebang.reason, LanguageDetectionReason::FirstLine);

        let dockerfile = detect("FROM alpine\nRUN echo ok\n", None, "");
        assert_eq!(dockerfile.language_id.as_str(), "dockerfile");
        assert_eq!(dockerfile.syntax_key(), Some("dockerfile"));
        assert_eq!(dockerfile.reason, LanguageDetectionReason::ContentHint);
    }

    #[test]
    fn unknown_files_detect_as_plain_text() {
        let detection = detect("plain text\n", Some("notes.unknown"), "");
        assert_eq!(detection.language_id.as_str(), "plain");
        assert_eq!(detection.syntax_key(), None);
        assert_eq!(detection.confidence, LanguageDetectionConfidence::None);
        assert_eq!(detection.reason, LanguageDetectionReason::Unknown);
    }
}
