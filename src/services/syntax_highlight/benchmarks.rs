use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use semver::Version;
use sha2::{Digest, Sha256};
use tempfile::tempdir;
use tree_sitter::LANGUAGE_VERSION;

use super::installation::{
    DirectoryPackageDecoder, GrammarInstallationService, LocalGrammarTransport,
};
use super::registry::{
    current_app_version, GrammarPackageDownload, GrammarPackageFiles, GrammarPackageManifest,
    GrammarPackageSource, GrammarRegistryEntry, GrammarRegistryFile, GrammarRuntime,
    RegistryCacheMetadata, RuntimeGrammarSecurityPolicy, PACKAGE_MANIFEST_FILENAME,
    REGISTRY_JSON_PATH, REGISTRY_SCHEMA_VERSION,
};
use super::{
    parser_loading, queries, release_syntax_caches, HighlightDocument, HighlightLineResult,
    SyntaxHighlightService, PARSE_TREE_CACHE,
};

const VISIBLE_HUNK_LINES: u32 = 32;

struct BenchmarkCase {
    name: &'static str,
    path: &'static str,
    content: String,
    target_line: u32,
}

#[test]
fn syntax_highlight_benchmark() {
    for case in benchmark_cases() {
        run_case(&case);
    }
    run_install_case();
}

fn run_case(case: &BenchmarkCase) {
    release_syntax_caches();
    let paint_start = Instant::now();
    let cold_document = HighlightDocument::from_path(&case.content, case.path);
    let first_paint = paint_start.elapsed();

    let service = SyntaxHighlightService::with_runtime_and_query_override_dirs(None, None);
    let first_highlight_start = Instant::now();
    let first_highlight = service.highlight_line_for_document(&cold_document, case.target_line);
    let first_highlight_elapsed = first_highlight_start.elapsed();
    let visible_start = case.target_line;
    let visible_end =
        (visible_start + VISIBLE_HUNK_LINES - 1).min(cold_document.line_count() as u32);
    let hunk_start = Instant::now();
    let highlighted = highlight_range(&service, &cold_document, visible_start, visible_end);
    let cold_hunk_elapsed = hunk_start.elapsed();
    let cold_stats = cold_document.highlight_stats();
    let parse_footprint = PARSE_TREE_CACHE
        .lock()
        .map(|cache| cache.footprint())
        .unwrap_or_default();

    let warm_service = SyntaxHighlightService::with_runtime_and_query_override_dirs(None, None);
    let warm_document = HighlightDocument::from_path(&case.content, case.path);
    let warm_start = Instant::now();
    let warm_highlighted =
        highlight_range(&warm_service, &warm_document, visible_start, visible_end);
    let warm_hunk_elapsed = warm_start.elapsed();
    let warm_stats = warm_document.highlight_stats();
    let query_stats = queries::query_cache_stats();

    assert!(matches!(first_highlight, HighlightLineResult::Ready(_)));
    assert_eq!(highlighted, (visible_end - visible_start + 1) as usize);
    assert_eq!(warm_highlighted, highlighted);

    println!(
        "syntax_highlight_benchmark case={} lines={} first_paint_us={} first_visible_us={} visible_hunk_us={} warm_visible_hunk_us={} parse_hit_rate={} query_hit_rate={} parse_tree_entries={} parse_tree_bytes={}",
        case.name,
        cold_document.line_count(),
        micros(first_paint),
        micros(first_highlight_elapsed),
        micros(cold_hunk_elapsed),
        micros(warm_hunk_elapsed),
        ratio(warm_stats.parse_hits, warm_stats.parse_hits + warm_stats.parse_misses),
        ratio(query_stats.hits, query_stats.hits + query_stats.misses),
        parse_footprint.entries,
        parse_footprint.bytes
    );
    assert!(cold_stats.parsed_lines <= cold_document.line_count());
}

fn highlight_range(
    service: &SyntaxHighlightService,
    document: &HighlightDocument,
    start: u32,
    end: u32,
) -> usize {
    (start..=end)
        .filter(|line| {
            matches!(
                service.highlight_line_for_document(document, *line),
                HighlightLineResult::Ready(_)
            )
        })
        .count()
}

fn run_install_case() {
    let runtime = tempdir().unwrap();
    let source_root = tempdir().unwrap();
    let package_dir = write_install_package(source_root.path());
    write_registry(runtime.path(), package_dir.to_string_lossy().as_ref());
    let service = GrammarInstallationService::with_policy(
        runtime.path(),
        RuntimeGrammarSecurityPolicy {
            allow_native_community_grammars: true,
            ..RuntimeGrammarSecurityPolicy::default()
        },
    );
    service.queue_install_for_language("benchlang");
    let start = Instant::now();
    let status = service
        .install_queued_grammar(
            "benchlang",
            &LocalGrammarTransport,
            &DirectoryPackageDecoder,
        )
        .unwrap();
    println!(
        "syntax_highlight_benchmark case=grammar_install language={} status={:?} install_us={}",
        status.language,
        status.status,
        micros(start.elapsed())
    );
}

fn benchmark_cases() -> Vec<BenchmarkCase> {
    vec![
        BenchmarkCase {
            name: "large_scss_deep_hunk",
            path: "assets/admin/css/_chat.scss",
            content: generate_scss(2_400),
            target_line: 2_250,
        },
        BenchmarkCase {
            name: "large_php_injections",
            path: "templates/page.php",
            content: generate_php(420),
            target_line: 1_400,
        },
        BenchmarkCase {
            name: "large_markdown_fences",
            path: "docs/notes.md",
            content: generate_markdown(260),
            target_line: 1_100,
        },
        BenchmarkCase {
            name: "large_typescript",
            path: "src/app/state.ts",
            content: generate_typescript(2_200),
            target_line: 2_000,
        },
        BenchmarkCase {
            name: "large_twig_runtime_missing",
            path: "templates/admin/list.html.twig",
            content: generate_twig(1_600),
            target_line: 1_450,
        },
        BenchmarkCase {
            name: "large_plain_unknown",
            path: "logs/trace.unknown",
            content: generate_plain(3_200),
            target_line: 3_000,
        },
    ]
}

fn generate_scss(lines: usize) -> String {
    let mut out = String::new();
    for idx in 0..lines {
        out.push_str(&format!(
            ".chat-row-{idx} {{ color: #{:06x}; .meta {{ padding: {}px; }} }}\n",
            (idx * 977) % 0xff_ffff,
            idx % 17
        ));
    }
    out
}

fn generate_php(blocks: usize) -> String {
    let mut out = String::from("<?php $items = [];\n");
    for idx in 0..blocks {
        out.push_str(&format!(
            "$items[] = ['id' => {idx}, 'name' => 'Item {idx}'];\n"
        ));
        out.push_str("?>\n");
        out.push_str(&format!("<article class=\"card card-{idx}\">\n"));
        out.push_str("<style>.card { display: grid; color: #fff; }</style>\n");
        out.push_str("<script>const value = document.querySelector('.card');</script>\n");
        out.push_str("<?php echo htmlspecialchars($items[count($items) - 1]['name']); ?>\n");
    }
    out
}

fn generate_markdown(blocks: usize) -> String {
    let mut out = String::new();
    for idx in 0..blocks {
        out.push_str(&format!("## Section {idx}\n\n"));
        out.push_str("```typescript\n");
        out.push_str(&format!("const value{idx}: number = {idx};\n"));
        out.push_str("```\n\n");
        out.push_str("```scss\n");
        out.push_str(&format!(".section-{idx} {{ color: #abcdef; }}\n"));
        out.push_str("```\n\n");
    }
    out
}

fn generate_typescript(lines: usize) -> String {
    let mut out = String::new();
    for idx in 0..lines {
        out.push_str(&format!(
            "export const selector{idx}: Record<string, number> = {{ value: {idx} }};\n"
        ));
    }
    out
}

fn generate_twig(lines: usize) -> String {
    let mut out = String::new();
    for idx in 0..lines {
        out.push_str(&format!(
            "{{% if item.visible %}}<span class=\"row-{idx}\">{{{{ item.name }}}}</span>{{% endif %}}\n"
        ));
    }
    out
}

fn generate_plain(lines: usize) -> String {
    let mut out = String::new();
    for idx in 0..lines {
        out.push_str(&format!("plain diagnostic line {idx} with repeated text\n"));
    }
    out
}

fn write_registry(runtime_dir: &std::path::Path, package_path: &str) {
    let registry = GrammarRegistryFile {
        schema_version: REGISTRY_SCHEMA_VERSION,
        cache: RegistryCacheMetadata::new(1, 60),
        grammars: vec![GrammarRegistryEntry {
            language: "benchlang".to_string(),
            version: Version::new(1, 0, 0),
            parser_abi: LANGUAGE_VERSION,
            app_version_req: Some(format!(">={}", current_app_version()).parse().unwrap()),
            runtime: Some(GrammarRuntime::Native),
            platforms: vec![parser_loading::current_platform().to_string()],
            filetypes: vec!["benchlang".to_string()],
            extensions: vec!["benchlang".to_string()],
            filenames: Vec::new(),
            first_line_regex: None,
            content_regex: None,
            packages: vec![GrammarPackageDownload {
                url: package_path.to_string(),
                sha256: None,
                signature: None,
                source: GrammarPackageSource::Community,
                runtime: Some(GrammarRuntime::Native),
                platform: Some(parser_loading::current_platform().to_string()),
            }],
        }],
    };
    let path = runtime_dir.join(REGISTRY_JSON_PATH);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_string_pretty(&registry).unwrap()).unwrap();
}

fn write_install_package(root: &std::path::Path) -> std::path::PathBuf {
    let package_dir = root.join("benchlang");
    let parser_path = format!("parser/{}/benchlang.so", parser_loading::current_platform());
    let query_path = "queries/highlights.scm".to_string();
    std::fs::create_dir_all(
        package_dir
            .join("parser")
            .join(parser_loading::current_platform()),
    )
    .unwrap();
    std::fs::create_dir_all(package_dir.join("queries")).unwrap();
    std::fs::write(package_dir.join(&parser_path), "not a shared library").unwrap();
    std::fs::write(package_dir.join(&query_path), "").unwrap();
    let mut sha256 = BTreeMap::new();
    sha256.insert(
        parser_path.clone(),
        sha256_hex(&std::fs::read(package_dir.join(&parser_path)).unwrap()),
    );
    sha256.insert(
        query_path.clone(),
        sha256_hex(&std::fs::read(package_dir.join(&query_path)).unwrap()),
    );
    let manifest = GrammarPackageManifest {
        language: "benchlang".to_string(),
        version: Version::new(1, 0, 0),
        parser_abi: LANGUAGE_VERSION,
        runtime: GrammarRuntime::Native,
        platform: parser_loading::current_platform().to_string(),
        source: GrammarPackageSource::Community,
        source_url: None,
        signature: None,
        files: GrammarPackageFiles {
            parser: Some(parser_path),
            wasm: None,
            highlights: Some(query_path),
            injections: None,
            locals: None,
        },
        filetypes: vec!["benchlang".to_string()],
        extensions: vec!["benchlang".to_string()],
        filenames: Vec::new(),
        first_line_regex: None,
        content_regex: None,
        app_version_req: Some(format!(">={}", current_app_version()).parse().unwrap()),
        sha256,
    };
    std::fs::write(
        package_dir.join(PACKAGE_MANIFEST_FILENAME),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    package_dir
}

fn micros(duration: Duration) -> u128 {
    duration.as_micros()
}

fn ratio(numerator: usize, denominator: usize) -> String {
    if denominator == 0 {
        return "0.000".to_string();
    }
    format!("{:.3}", numerator as f64 / denominator as f64)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}
