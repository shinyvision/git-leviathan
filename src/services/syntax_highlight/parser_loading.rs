use std::collections::HashMap;
use std::ops::Range as ByteRange;
use std::path::{Component, Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use libloading::Library;
use tree_sitter::{Language, Parser, Point, Range as TreeSitterRange, Tree};

use super::registry::{
    GrammarRuntime, InstalledGrammarPackage, ParserAbiCompatibility, ParserRuntime,
    TreeSitterSyntax,
};

type NativeLanguageFn = unsafe extern "C" fn() -> *const ();

static NATIVE_LOAD_FAILURES: LazyLock<
    Mutex<HashMap<NativeParserCacheKey, NativeParserLoadErrorKind>>,
> = LazyLock::new(|| Mutex::new(HashMap::new()));
static NATIVE_LOAD_RESULTS: LazyLock<Mutex<HashMap<NativeParserCacheKey, Language>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static NATIVE_LIBRARIES: LazyLock<Mutex<Vec<&'static Library>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));
static WASM_LOAD_FAILURES: LazyLock<Mutex<HashMap<WasmParserCacheKey, WasmParserLoadErrorKind>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static WASM_LOAD_RESULTS: LazyLock<Mutex<HashMap<WasmParserCacheKey, Language>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

static WASM_ENGINE: LazyLock<tree_sitter::wasmtime::Engine> =
    LazyLock::new(tree_sitter::wasmtime::Engine::default);

pub(super) struct ParserLoader {
    state: Mutex<ParserState>,
}

struct ParserState {
    parser: Parser,
    wasm_store_attached: bool,
}

impl ParserLoader {
    pub(super) fn new() -> Self {
        Self {
            state: Mutex::new(ParserState {
                parser: Parser::new(),
                wasm_store_attached: false,
            }),
        }
    }

    pub(super) fn parse_tree(&self, syntax: &TreeSitterSyntax, content: &str) -> Option<Tree> {
        let mut state = self.state.lock().ok()?;
        if syntax.parser_runtime() == ParserRuntime::Wasm {
            attach_wasm_store(&mut state).ok()?;
        }
        state.parser.set_language(syntax.language()).ok()?;
        state.parser.set_included_ranges(&[]).ok()?;
        state.parser.parse(content, None)
    }

    pub(super) fn parse_tree_for_ranges(
        &self,
        syntax: &TreeSitterSyntax,
        content: &str,
        ranges: &[ByteRange<usize>],
    ) -> Option<Tree> {
        if ranges.is_empty() {
            return None;
        }

        let included_ranges = included_ranges_for_content(content, ranges)?;
        let mut state = self.state.lock().ok()?;
        if syntax.parser_runtime() == ParserRuntime::Wasm {
            attach_wasm_store(&mut state).ok()?;
        }
        state.parser.set_language(syntax.language()).ok()?;
        state.parser.set_included_ranges(&included_ranges).ok()?;
        let tree = state.parser.parse(content, None);
        let _ = state.parser.set_included_ranges(&[]);
        tree
    }
}

fn included_ranges_for_content(
    content: &str,
    ranges: &[ByteRange<usize>],
) -> Option<Vec<TreeSitterRange>> {
    let normalized = normalize_byte_ranges(content.len(), ranges);
    if normalized.is_empty() {
        return None;
    }

    Some(
        normalized
            .into_iter()
            .map(|range| TreeSitterRange {
                start_byte: range.start,
                end_byte: range.end,
                start_point: point_for_byte(content.as_bytes(), range.start),
                end_point: point_for_byte(content.as_bytes(), range.end),
            })
            .collect(),
    )
}

fn normalize_byte_ranges(content_len: usize, ranges: &[ByteRange<usize>]) -> Vec<ByteRange<usize>> {
    let mut ranges = ranges
        .iter()
        .filter_map(|range| {
            let start = range.start.min(content_len);
            let end = range.end.min(content_len);
            (start < end).then_some(start..end)
        })
        .collect::<Vec<_>>();
    ranges.sort_unstable_by_key(|range| (range.start, range.end));

    let mut normalized: Vec<ByteRange<usize>> = Vec::new();
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

fn point_for_byte(bytes: &[u8], byte_offset: usize) -> Point {
    let byte_offset = byte_offset.min(bytes.len());
    let mut row = 0;
    let mut line_start = 0;
    for (idx, byte) in bytes.iter().enumerate().take(byte_offset) {
        if *byte == b'\n' {
            row += 1;
            line_start = idx + 1;
        }
    }

    Point {
        row,
        column: byte_offset - line_start,
    }
}

fn attach_wasm_store(state: &mut ParserState) -> Result<(), WasmParserLoadError> {
    if state.wasm_store_attached {
        return Ok(());
    }

    let store = tree_sitter::WasmStore::new(&WASM_ENGINE).map_err(|err| {
        WasmParserLoadError::new(
            WasmParserLoadErrorKind::CreateStore,
            format!("failed to create Tree-sitter WASM store: {err}"),
        )
    })?;
    state.parser.set_wasm_store(store).map_err(|err| {
        WasmParserLoadError::new(
            WasmParserLoadErrorKind::AttachStore,
            format!("failed to attach Tree-sitter WASM store: {err}"),
        )
    })?;
    state.wasm_store_attached = true;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct NativeParserCacheKey {
    library_path: PathBuf,
    symbol: String,
    parser_abi: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct WasmParserCacheKey {
    wasm_path: PathBuf,
    language_name: String,
    parser_abi: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NativeParserLoadErrorKind {
    CachedFailure,
    UnsupportedRuntime,
    UnsupportedPlatform,
    MissingParserPath,
    UnsafePackagePath,
    IncompatibleManifestAbi,
    LibraryOpen,
    InvalidSymbolName,
    MissingSymbol,
    NullLanguage,
    AbiMismatch,
    IncompatibleLanguageAbi,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NativeParserLoadError {
    kind: NativeParserLoadErrorKind,
    message: String,
}

impl NativeParserLoadError {
    fn new(kind: NativeParserLoadErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub(super) fn kind(&self) -> NativeParserLoadErrorKind {
        self.kind
    }

    pub(super) fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WasmParserLoadErrorKind {
    CachedFailure,
    UnsupportedRuntime,
    MissingWasmPath,
    UnsafePackagePath,
    IncompatibleManifestAbi,
    ReadWasm,
    CreateStore,
    AttachStore,
    InvalidLanguageName,
    LoadLanguage,
    NotWasmLanguage,
    IncompatibleLanguageAbi,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WasmParserLoadError {
    kind: WasmParserLoadErrorKind,
    message: String,
}

impl WasmParserLoadError {
    fn new(kind: WasmParserLoadErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub(super) fn kind(&self) -> WasmParserLoadErrorKind {
        self.kind
    }

    pub(super) fn message(&self) -> &str {
        &self.message
    }
}

pub(super) fn load_native_language(
    package: &InstalledGrammarPackage,
) -> Result<Language, NativeParserLoadError> {
    if package.manifest.runtime != GrammarRuntime::Native {
        return Err(NativeParserLoadError::new(
            NativeParserLoadErrorKind::UnsupportedRuntime,
            "grammar package is not native",
        ));
    }

    if package.manifest.platform != current_platform() {
        return Err(NativeParserLoadError::new(
            NativeParserLoadErrorKind::UnsupportedPlatform,
            format!(
                "grammar platform {} does not match {}",
                package.manifest.platform,
                current_platform()
            ),
        ));
    }

    let Some(parser_path) = package.manifest.files.parser.as_deref() else {
        return Err(NativeParserLoadError::new(
            NativeParserLoadErrorKind::MissingParserPath,
            "grammar package has no native parser path",
        ));
    };
    let library_path = package_relative_path(&package.package_dir, parser_path)?;
    let Some(symbol) = tree_sitter_symbol_name(&package.manifest.language) else {
        return Err(NativeParserLoadError::new(
            NativeParserLoadErrorKind::InvalidSymbolName,
            "grammar language cannot be converted to a Tree-sitter symbol",
        ));
    };
    let key = NativeParserCacheKey {
        library_path: library_path.clone(),
        symbol: symbol.clone(),
        parser_abi: package.manifest.parser_abi,
    };

    if cached_failure(&key).is_some() {
        return Err(NativeParserLoadError::new(
            NativeParserLoadErrorKind::CachedFailure,
            "native parser loading previously failed",
        ));
    }
    if let Some(language) = cached_native_language(&key) {
        return Ok(language);
    }

    let result = load_native_language_uncached(&library_path, &symbol, package.manifest.parser_abi);
    match &result {
        Ok(language) => cache_native_language(key, language.clone()),
        Err(err) => cache_failure(key, err.kind()),
    }
    result
}

pub(super) fn load_wasm_language(
    package: &InstalledGrammarPackage,
) -> Result<Language, WasmParserLoadError> {
    if package.manifest.runtime != GrammarRuntime::Wasm {
        return Err(WasmParserLoadError::new(
            WasmParserLoadErrorKind::UnsupportedRuntime,
            "grammar package is not WASM",
        ));
    }

    let Some(wasm_path) = package.manifest.files.wasm.as_deref() else {
        return Err(WasmParserLoadError::new(
            WasmParserLoadErrorKind::MissingWasmPath,
            "grammar package has no WASM parser path",
        ));
    };
    let wasm_path = package_relative_path(&package.package_dir, wasm_path).map_err(|err| {
        WasmParserLoadError::new(WasmParserLoadErrorKind::UnsafePackagePath, err.message())
    })?;
    let Some(language_name) = wasm_language_name_from_package(package)
        .or_else(|| wasm_language_name(&package.manifest.language))
    else {
        return Err(WasmParserLoadError::new(
            WasmParserLoadErrorKind::InvalidLanguageName,
            "grammar language cannot be converted to a Tree-sitter WASM name",
        ));
    };
    let key = WasmParserCacheKey {
        wasm_path: wasm_path.clone(),
        language_name: language_name.clone(),
        parser_abi: package.manifest.parser_abi,
    };

    if cached_wasm_failure(&key).is_some() {
        return Err(WasmParserLoadError::new(
            WasmParserLoadErrorKind::CachedFailure,
            "WASM parser loading previously failed",
        ));
    }
    if let Some(language) = cached_wasm_language(&key) {
        return Ok(language);
    }

    let result =
        load_wasm_language_uncached(&wasm_path, &language_name, package.manifest.parser_abi);
    match &result {
        Ok(language) => cache_wasm_language(key, language.clone()),
        Err(err) => cache_wasm_failure(key, err.kind()),
    }
    result
}

pub(super) fn package_relative_path(
    package_dir: &Path,
    relative_path: &str,
) -> Result<PathBuf, NativeParserLoadError> {
    let trimmed = relative_path.trim();
    if trimmed.is_empty() || trimmed.contains('\\') {
        return Err(NativeParserLoadError::new(
            NativeParserLoadErrorKind::UnsafePackagePath,
            "manifest path is empty or uses unsupported separators",
        ));
    }

    let path = Path::new(trimmed);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(NativeParserLoadError::new(
            NativeParserLoadErrorKind::UnsafePackagePath,
            "manifest path must stay inside the grammar package",
        ));
    }

    Ok(package_dir.join(path))
}

pub(super) fn current_platform() -> &'static str {
    static PLATFORM: LazyLock<String> =
        LazyLock::new(|| format!("{}-{}", target_os_name(), target_arch_name()));
    &PLATFORM
}

pub(super) fn tree_sitter_symbol_name(language: &str) -> Option<String> {
    let language = language.trim();
    if language.is_empty() {
        return None;
    }

    let mut normalized = String::new();
    let mut last_was_separator = false;
    for ch in language.chars() {
        match ch {
            'a'..='z' | '0'..='9' => {
                normalized.push(ch);
                last_was_separator = false;
            }
            'A'..='Z' => {
                normalized.push(ch.to_ascii_lowercase());
                last_was_separator = false;
            }
            '+' => {
                normalized.push('p');
                last_was_separator = false;
            }
            '#' => {
                push_separator(&mut normalized, &mut last_was_separator);
                normalized.push_str("sharp");
                last_was_separator = false;
            }
            '-' | '_' | '.' | ' ' => push_separator(&mut normalized, &mut last_was_separator),
            _ => return None,
        }
    }

    let normalized = normalized.trim_matches('_');
    if normalized.is_empty()
        || !normalized
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
    {
        return None;
    }

    Some(format!("tree_sitter_{normalized}"))
}

fn wasm_language_name(language: &str) -> Option<String> {
    tree_sitter_symbol_name(language)
        .and_then(|symbol| symbol.strip_prefix("tree_sitter_").map(ToString::to_string))
}

fn wasm_language_name_from_package(package: &InstalledGrammarPackage) -> Option<String> {
    let filename = package
        .manifest
        .files
        .wasm
        .as_deref()
        .and_then(|path| Path::new(path).file_name())
        .and_then(|filename| filename.to_str())?;
    let language = filename
        .strip_prefix("tree-sitter-")
        .and_then(|filename| filename.strip_suffix(".wasm"))?;
    normalized_wasm_language_name(language)
}

fn normalized_wasm_language_name(language: &str) -> Option<String> {
    let mut normalized = String::with_capacity(language.len());
    for ch in language.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            normalized.push(ch.to_ascii_lowercase());
        } else if ch == '-' {
            normalized.push('_');
        } else {
            return None;
        }
    }
    (!normalized.is_empty()
        && normalized
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic))
    .then_some(normalized)
}

fn load_native_language_uncached(
    library_path: &Path,
    symbol: &str,
    parser_abi: usize,
) -> Result<Language, NativeParserLoadError> {
    if !ParserAbiCompatibility::check(parser_abi).is_compatible() {
        return Err(NativeParserLoadError::new(
            NativeParserLoadErrorKind::IncompatibleManifestAbi,
            "manifest parser ABI is not supported by this Tree-sitter runtime",
        ));
    }

    if !library_path.is_file() {
        return Err(NativeParserLoadError::new(
            NativeParserLoadErrorKind::MissingParserPath,
            format!(
                "native parser library does not exist: {}",
                library_path.display()
            ),
        ));
    }

    let library = unsafe { Library::new(library_path) }.map_err(|err| {
        NativeParserLoadError::new(
            NativeParserLoadErrorKind::LibraryOpen,
            format!("failed to open native parser library: {err}"),
        )
    })?;
    let language_fn = {
        let symbol =
            unsafe { library.get::<NativeLanguageFn>(symbol.as_bytes()) }.map_err(|err| {
                NativeParserLoadError::new(
                    NativeParserLoadErrorKind::MissingSymbol,
                    format!("failed to find native parser symbol: {err}"),
                )
            })?;
        *symbol
    };

    let raw_language = unsafe { language_fn() };
    if raw_language.is_null() {
        return Err(NativeParserLoadError::new(
            NativeParserLoadErrorKind::NullLanguage,
            "native parser returned a null language pointer",
        ));
    }

    let language =
        unsafe { Language::new(tree_sitter_language::LanguageFn::from_raw(language_fn)) };
    validate_language_abi(&language, parser_abi)?;
    let leaked_library = Box::leak(Box::new(library));
    if let Ok(mut libraries) = NATIVE_LIBRARIES.lock() {
        libraries.push(leaked_library);
    }
    Ok(language)
}

fn validate_language_abi(
    language: &Language,
    declared_abi: usize,
) -> Result<(), NativeParserLoadError> {
    let actual_abi = language.abi_version();
    if actual_abi != declared_abi {
        return Err(NativeParserLoadError::new(
            NativeParserLoadErrorKind::AbiMismatch,
            format!("manifest ABI {declared_abi} does not match parser ABI {actual_abi}"),
        ));
    }

    if !ParserAbiCompatibility::check(actual_abi).is_compatible() {
        return Err(NativeParserLoadError::new(
            NativeParserLoadErrorKind::IncompatibleLanguageAbi,
            "native parser ABI is not supported by this Tree-sitter runtime",
        ));
    }

    Ok(())
}

fn load_wasm_language_uncached(
    wasm_path: &Path,
    language_name: &str,
    parser_abi: usize,
) -> Result<Language, WasmParserLoadError> {
    if !ParserAbiCompatibility::check(parser_abi).is_compatible() {
        return Err(WasmParserLoadError::new(
            WasmParserLoadErrorKind::IncompatibleManifestAbi,
            "manifest parser ABI is not supported by this Tree-sitter runtime",
        ));
    }

    let bytes = std::fs::read(wasm_path).map_err(|err| {
        WasmParserLoadError::new(
            WasmParserLoadErrorKind::ReadWasm,
            format!("failed to read WASM parser file: {err}"),
        )
    })?;
    let mut store = tree_sitter::WasmStore::new(&WASM_ENGINE).map_err(|err| {
        WasmParserLoadError::new(
            WasmParserLoadErrorKind::CreateStore,
            format!("failed to create Tree-sitter WASM store: {err}"),
        )
    })?;
    let language = store.load_language(language_name, &bytes).map_err(|err| {
        WasmParserLoadError::new(
            WasmParserLoadErrorKind::LoadLanguage,
            format!("failed to load WASM parser: {err}"),
        )
    })?;

    validate_wasm_language_abi(&language)?;
    Ok(language)
}

fn validate_wasm_language_abi(language: &Language) -> Result<(), WasmParserLoadError> {
    if !language.is_wasm() {
        return Err(WasmParserLoadError::new(
            WasmParserLoadErrorKind::NotWasmLanguage,
            "WASM parser did not produce a WASM language",
        ));
    }

    let actual_abi = language.abi_version();
    if !ParserAbiCompatibility::check(actual_abi).is_compatible() {
        return Err(WasmParserLoadError::new(
            WasmParserLoadErrorKind::IncompatibleLanguageAbi,
            format!("WASM parser ABI {actual_abi} is not supported by this Tree-sitter runtime"),
        ));
    }

    Ok(())
}

fn cached_failure(key: &NativeParserCacheKey) -> Option<NativeParserLoadErrorKind> {
    NATIVE_LOAD_FAILURES
        .lock()
        .ok()
        .and_then(|cache| cache.get(key).copied())
}

fn cached_native_language(key: &NativeParserCacheKey) -> Option<Language> {
    NATIVE_LOAD_RESULTS
        .lock()
        .ok()
        .and_then(|cache| cache.get(key).cloned())
}

fn cache_native_language(key: NativeParserCacheKey, language: Language) {
    if let Ok(mut cache) = NATIVE_LOAD_RESULTS.lock() {
        cache.insert(key, language);
    }
}

fn cache_failure(key: NativeParserCacheKey, kind: NativeParserLoadErrorKind) {
    if kind == NativeParserLoadErrorKind::CachedFailure {
        return;
    }

    if let Ok(mut cache) = NATIVE_LOAD_FAILURES.lock() {
        cache.insert(key, kind);
    }
}

fn cached_wasm_failure(key: &WasmParserCacheKey) -> Option<WasmParserLoadErrorKind> {
    WASM_LOAD_FAILURES
        .lock()
        .ok()
        .and_then(|cache| cache.get(key).copied())
}

fn cached_wasm_language(key: &WasmParserCacheKey) -> Option<Language> {
    WASM_LOAD_RESULTS
        .lock()
        .ok()
        .and_then(|cache| cache.get(key).cloned())
}

fn cache_wasm_language(key: WasmParserCacheKey, language: Language) {
    if let Ok(mut cache) = WASM_LOAD_RESULTS.lock() {
        cache.insert(key, language);
    }
}

fn cache_wasm_failure(key: WasmParserCacheKey, kind: WasmParserLoadErrorKind) {
    if kind == WasmParserLoadErrorKind::CachedFailure {
        return;
    }

    if let Ok(mut cache) = WASM_LOAD_FAILURES.lock() {
        cache.insert(key, kind);
    }
}

fn target_os_name() -> &'static str {
    if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        std::env::consts::OS
    }
}

fn target_arch_name() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        std::env::consts::ARCH
    }
}

fn push_separator(normalized: &mut String, last_was_separator: &mut bool) {
    if !normalized.is_empty() && !*last_was_separator {
        normalized.push('_');
        *last_was_separator = true;
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use semver::Version;
    use tempfile::tempdir;
    use tree_sitter::LANGUAGE_VERSION;

    use super::*;
    use crate::services::syntax_highlight::registry::{
        AppVersionCompatibility, GrammarCompatibility, GrammarPackageFiles, GrammarPackageManifest,
        InstalledGrammarPackage, ParserAbiCompatibility,
    };

    fn native_package(
        package_dir: PathBuf,
        language: &str,
        parser_path: &str,
        parser_abi: usize,
    ) -> InstalledGrammarPackage {
        InstalledGrammarPackage {
            manifest_path: package_dir.join("manifest.json"),
            package_dir,
            manifest: GrammarPackageManifest {
                language: language.to_string(),
                version: Version::new(1, 0, 0),
                parser_abi,
                runtime: GrammarRuntime::Native,
                platform: current_platform().to_string(),
                source:
                    crate::services::syntax_highlight::registry::GrammarPackageSource::Community,
                source_url: None,
                signature: None,
                files: GrammarPackageFiles {
                    parser: Some(parser_path.to_string()),
                    wasm: None,
                    highlights: Some("queries/highlights.scm".to_string()),
                    injections: None,
                    locals: None,
                },
                filetypes: vec![language.to_string()],
                extensions: vec![language.to_string()],
                filenames: Vec::new(),
                first_line_regex: None,
                content_regex: None,
                app_version_req: None,
                sha256: Default::default(),
            },
            compatibility: GrammarCompatibility {
                parser_abi: ParserAbiCompatibility::check(parser_abi),
                app_version: AppVersionCompatibility::NotDeclared,
            },
        }
    }

    fn wasm_package(
        package_dir: PathBuf,
        language: &str,
        wasm_path: Option<&str>,
        parser_abi: usize,
    ) -> InstalledGrammarPackage {
        InstalledGrammarPackage {
            manifest_path: package_dir.join("manifest.json"),
            package_dir,
            manifest: GrammarPackageManifest {
                language: language.to_string(),
                version: Version::new(1, 0, 0),
                parser_abi,
                runtime: GrammarRuntime::Wasm,
                platform: "wasm".to_string(),
                source:
                    crate::services::syntax_highlight::registry::GrammarPackageSource::Community,
                source_url: None,
                signature: None,
                files: GrammarPackageFiles {
                    parser: None,
                    wasm: wasm_path.map(ToString::to_string),
                    highlights: Some("queries/highlights.scm".to_string()),
                    injections: None,
                    locals: None,
                },
                filetypes: vec![language.to_string()],
                extensions: vec![language.to_string()],
                filenames: Vec::new(),
                first_line_regex: None,
                content_regex: None,
                app_version_req: None,
                sha256: Default::default(),
            },
            compatibility: GrammarCompatibility {
                parser_abi: ParserAbiCompatibility::check(parser_abi),
                app_version: AppVersionCompatibility::NotDeclared,
            },
        }
    }

    #[test]
    fn symbol_names_are_normalized_safely() {
        assert_eq!(
            tree_sitter_symbol_name("C++").as_deref(),
            Some("tree_sitter_cpp")
        );
        assert_eq!(
            tree_sitter_symbol_name("c-sharp").as_deref(),
            Some("tree_sitter_c_sharp")
        );
        assert_eq!(
            tree_sitter_symbol_name("php only").as_deref(),
            Some("tree_sitter_php_only")
        );
        assert_eq!(tree_sitter_symbol_name("../bad"), None);
    }

    #[test]
    fn wasm_language_names_follow_tree_sitter_symbol_names() {
        assert_eq!(wasm_language_name("C++").as_deref(), Some("cpp"));
        assert_eq!(wasm_language_name("c-sharp").as_deref(), Some("c_sharp"));
        assert_eq!(wasm_language_name("../bad"), None);
    }

    #[test]
    fn wasm_language_name_can_follow_parser_filename() {
        let tmp = tempdir().unwrap();
        let package = wasm_package(
            tmp.path().join("parsers").join("twig"),
            "twig",
            Some("wasm/tree-sitter-shopware_twig.wasm"),
            LANGUAGE_VERSION,
        );

        assert_eq!(
            wasm_language_name_from_package(&package).as_deref(),
            Some("shopware_twig")
        );
    }

    #[test]
    fn native_loader_rejects_missing_parser_path_and_caches_failure() {
        let tmp = tempdir().unwrap();
        let package_dir = tmp.path().join("parsers").join("missing");
        fs::create_dir_all(&package_dir).unwrap();
        let package = native_package(
            package_dir,
            "missing",
            &format!("parser/{}/missing.so", current_platform()),
            LANGUAGE_VERSION,
        );

        let err = load_native_language(&package).unwrap_err();
        assert_eq!(err.kind(), NativeParserLoadErrorKind::MissingParserPath);

        let cached = load_native_language(&package).unwrap_err();
        assert_eq!(cached.kind(), NativeParserLoadErrorKind::CachedFailure);
    }

    #[test]
    fn native_loader_rejects_invalid_library_files() {
        let tmp = tempdir().unwrap();
        let package_dir = tmp.path().join("parsers").join("invalid");
        let parser_dir = package_dir.join("parser").join(current_platform());
        fs::create_dir_all(&parser_dir).unwrap();
        fs::write(parser_dir.join("invalid.so"), "not a shared library").unwrap();
        let package = native_package(
            package_dir,
            "invalid",
            &format!("parser/{}/invalid.so", current_platform()),
            LANGUAGE_VERSION,
        );

        let err = load_native_language(&package).unwrap_err();
        assert_eq!(err.kind(), NativeParserLoadErrorKind::LibraryOpen);
    }

    #[test]
    fn native_loader_rejects_incompatible_manifest_abi_before_loading() {
        let tmp = tempdir().unwrap();
        let package_dir = tmp.path().join("parsers").join("newer");
        fs::create_dir_all(&package_dir).unwrap();
        let package = native_package(
            package_dir,
            "newer",
            &format!("parser/{}/newer.so", current_platform()),
            LANGUAGE_VERSION + 1,
        );

        let err = load_native_language(&package).unwrap_err();
        assert_eq!(
            err.kind(),
            NativeParserLoadErrorKind::IncompatibleManifestAbi
        );
    }

    #[test]
    fn native_loader_rejects_paths_outside_package() {
        let tmp = tempdir().unwrap();
        let package_dir = tmp.path().join("parsers").join("unsafe");
        fs::create_dir_all(&package_dir).unwrap();
        let package = native_package(package_dir, "unsafe", "../unsafe.so", LANGUAGE_VERSION);

        let err = load_native_language(&package).unwrap_err();
        assert_eq!(err.kind(), NativeParserLoadErrorKind::UnsafePackagePath);
    }

    #[test]
    fn wasm_loader_rejects_missing_wasm_path() {
        let tmp = tempdir().unwrap();
        let package_dir = tmp.path().join("parsers").join("missing-wasm");
        fs::create_dir_all(&package_dir).unwrap();
        let package = wasm_package(package_dir, "missing-wasm", None, LANGUAGE_VERSION);

        let err = load_wasm_language(&package).unwrap_err();
        assert_eq!(err.kind(), WasmParserLoadErrorKind::MissingWasmPath);
    }

    #[test]
    fn wasm_loader_rejects_paths_outside_package() {
        let tmp = tempdir().unwrap();
        let package_dir = tmp.path().join("parsers").join("unsafe-wasm");
        fs::create_dir_all(&package_dir).unwrap();
        let package = wasm_package(
            package_dir,
            "unsafe-wasm",
            Some("../unsafe.wasm"),
            LANGUAGE_VERSION,
        );

        let err = load_wasm_language(&package).unwrap_err();
        assert_eq!(err.kind(), WasmParserLoadErrorKind::UnsafePackagePath);
    }

    #[test]
    fn wasm_loader_rejects_incompatible_manifest_abi_before_loading() {
        let tmp = tempdir().unwrap();
        let package_dir = tmp.path().join("parsers").join("newer-wasm");
        fs::create_dir_all(&package_dir).unwrap();
        let package = wasm_package(
            package_dir,
            "newer-wasm",
            Some("wasm/newer-wasm.wasm"),
            LANGUAGE_VERSION + 1,
        );

        let err = load_wasm_language(&package).unwrap_err();
        assert_eq!(err.kind(), WasmParserLoadErrorKind::IncompatibleManifestAbi);
    }

    #[test]
    fn wasm_loader_rejects_invalid_wasm_and_caches_failure() {
        let tmp = tempdir().unwrap();
        let package_dir = tmp.path().join("parsers").join("invalid-wasm");
        let wasm_dir = package_dir.join("wasm");
        fs::create_dir_all(&wasm_dir).unwrap();
        fs::write(wasm_dir.join("invalid-wasm.wasm"), "not wasm").unwrap();
        let package = wasm_package(
            package_dir,
            "invalid-wasm",
            Some("wasm/invalid-wasm.wasm"),
            LANGUAGE_VERSION,
        );

        let err = load_wasm_language(&package).unwrap_err();
        assert_eq!(err.kind(), WasmParserLoadErrorKind::LoadLanguage);

        let cached = load_wasm_language(&package).unwrap_err();
        assert_eq!(cached.kind(), WasmParserLoadErrorKind::CachedFailure);
    }
}
