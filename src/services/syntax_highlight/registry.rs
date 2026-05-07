use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, LazyLock};
use std::time::{SystemTime, UNIX_EPOCH};

use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tree_sitter::{Language, LANGUAGE_VERSION, MIN_COMPATIBLE_LANGUAGE_VERSION};

use super::parser_loading;
use super::queries;

pub const REGISTRY_SCHEMA_VERSION: u32 = 1;
pub const REGISTRY_CACHE_TTL_SECONDS: u64 = 24 * 60 * 60;
pub const REGISTRY_JSON_PATH: &str = "registry/registry.json";
pub const PACKAGE_MANIFEST_FILENAME: &str = "manifest.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GrammarRegistryFile {
    pub schema_version: u32,
    #[serde(default, alias = "metadata")]
    pub cache: RegistryCacheMetadata,
    #[serde(default)]
    pub grammars: Vec<GrammarRegistryEntry>,
}

impl GrammarRegistryFile {
    pub fn is_supported_schema(&self) -> bool {
        self.schema_version == REGISTRY_SCHEMA_VERSION
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GrammarRegistryEntry {
    pub language: String,
    pub version: Version,
    pub parser_abi: usize,
    #[serde(default, rename = "app_version", alias = "app_version_req")]
    pub app_version_req: Option<VersionReq>,
    #[serde(default)]
    pub runtime: Option<GrammarRuntime>,
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub filetypes: Vec<String>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub filenames: Vec<String>,
    #[serde(default)]
    pub first_line_regex: Option<String>,
    #[serde(default)]
    pub content_regex: Option<String>,
    #[serde(default)]
    pub packages: Vec<GrammarPackageDownload>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GrammarPackageDownload {
    pub url: String,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub signature: Option<String>,
    #[serde(default)]
    pub source: GrammarPackageSource,
    #[serde(default)]
    pub runtime: Option<GrammarRuntime>,
    #[serde(default)]
    pub platform: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GrammarPackageManifest {
    pub language: String,
    pub version: Version,
    pub parser_abi: usize,
    pub runtime: GrammarRuntime,
    pub platform: String,
    #[serde(default)]
    pub source: GrammarPackageSource,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub signature: Option<String>,
    pub files: GrammarPackageFiles,
    #[serde(default)]
    pub filetypes: Vec<String>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub filenames: Vec<String>,
    #[serde(default)]
    pub first_line_regex: Option<String>,
    #[serde(default)]
    pub content_regex: Option<String>,
    #[serde(default, rename = "app_version", alias = "app_version_req")]
    pub app_version_req: Option<VersionReq>,
    #[serde(default)]
    pub sha256: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GrammarPackageFiles {
    #[serde(default)]
    pub parser: Option<String>,
    #[serde(default)]
    pub wasm: Option<String>,
    #[serde(default)]
    pub highlights: Option<String>,
    #[serde(default)]
    pub injections: Option<String>,
    #[serde(default)]
    pub locals: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum GrammarRuntime {
    Native,
    Wasm,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum GrammarPackageSource {
    Official,
    #[default]
    Community,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeGrammarSecurityPolicy {
    pub runtime_downloads_enabled: bool,
    pub allow_native_community_grammars: bool,
    pub native_package_source_allowlist: Vec<String>,
}

impl RuntimeGrammarSecurityPolicy {
    pub fn from_env() -> Self {
        Self {
            runtime_downloads_enabled: !env_flag("GIT_LEVIATHAN_DISABLE_RUNTIME_GRAMMAR_DOWNLOADS"),
            allow_native_community_grammars: env_flag(
                "GIT_LEVIATHAN_ALLOW_NATIVE_COMMUNITY_GRAMMARS",
            ),
            native_package_source_allowlist: env_list(
                "GIT_LEVIATHAN_NATIVE_GRAMMAR_SOURCE_ALLOWLIST",
            )
            .unwrap_or_default(),
        }
    }

    pub fn allows_native_download(&self, source: &GrammarPackageDownload) -> bool {
        match source.source {
            GrammarPackageSource::Official => self.url_is_allowlisted(&source.url),
            GrammarPackageSource::Community => self.allow_native_community_grammars,
        }
    }

    pub fn allows_installed_native_package(&self, manifest: &GrammarPackageManifest) -> bool {
        match manifest.source {
            GrammarPackageSource::Official => manifest
                .source_url
                .as_deref()
                .is_some_and(|url| self.url_is_allowlisted(url)),
            GrammarPackageSource::Community => self.allow_native_community_grammars,
        }
    }

    fn url_is_allowlisted(&self, url: &str) -> bool {
        self.native_package_source_allowlist
            .iter()
            .any(|prefix| !prefix.is_empty() && url.starts_with(prefix))
    }
}

impl Default for RuntimeGrammarSecurityPolicy {
    fn default() -> Self {
        Self {
            runtime_downloads_enabled: true,
            allow_native_community_grammars: false,
            native_package_source_allowlist: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryCacheMetadata {
    pub fetched_at_unix_seconds: u64,
    pub ttl_seconds: u64,
}

impl RegistryCacheMetadata {
    pub fn new(fetched_at_unix_seconds: u64, ttl_seconds: u64) -> Self {
        Self {
            fetched_at_unix_seconds,
            ttl_seconds,
        }
    }

    pub fn with_default_ttl(fetched_at_unix_seconds: u64) -> Self {
        Self::new(fetched_at_unix_seconds, REGISTRY_CACHE_TTL_SECONDS)
    }

    pub fn expires_at_unix_seconds(&self) -> Option<u64> {
        self.fetched_at_unix_seconds.checked_add(self.ttl_seconds)
    }

    pub fn is_expired_at(&self, now_unix_seconds: u64) -> bool {
        self.expires_at_unix_seconds()
            .is_none_or(|expires_at| now_unix_seconds >= expires_at)
    }

    pub fn is_fresh_at(&self, now_unix_seconds: u64) -> bool {
        !self.is_expired_at(now_unix_seconds)
    }
}

impl Default for RegistryCacheMetadata {
    fn default() -> Self {
        Self::with_default_ttl(0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltInGrammar {
    pub language_id: String,
    pub syntax_key: String,
    pub filenames: Vec<String>,
    pub extensions: Vec<String>,
    pub compound_extensions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledGrammarPackage {
    pub package_dir: PathBuf,
    pub manifest_path: PathBuf,
    pub manifest: GrammarPackageManifest,
    pub compatibility: GrammarCompatibility,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingGrammar {
    pub entry: GrammarRegistryEntry,
    pub compatibility: GrammarCompatibility,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrammarInventoryReport {
    pub built_in: Vec<BuiltInGrammar>,
    pub installed: Vec<InstalledGrammarPackage>,
    pub missing: Vec<MissingGrammar>,
    pub registry_cache: Option<RegistryCacheMetadata>,
    pub registry_cache_fresh: bool,
    pub errors: Vec<GrammarRegistryError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalRegistryRead {
    pub registry: Option<GrammarRegistryFile>,
    pub errors: Vec<GrammarRegistryError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledGrammarScan {
    pub packages: Vec<InstalledGrammarPackage>,
    pub errors: Vec<GrammarRegistryError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrammarRegistryError {
    pub kind: GrammarRegistryErrorKind,
    pub path: PathBuf,
    pub message: String,
}

impl GrammarRegistryError {
    fn new(kind: GrammarRegistryErrorKind, path: PathBuf, message: impl Into<String>) -> Self {
        Self {
            kind,
            path,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrammarRegistryErrorKind {
    ReadRegistry,
    ParseRegistry,
    UnsupportedRegistrySchema,
    ReadPackageDirectory,
    ReadManifest,
    ParseManifest,
    InvalidPackagePath,
    MissingPackageFile,
    MissingFileHash,
    ReadPackageFile,
    FileHashMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrammarCompatibility {
    pub parser_abi: ParserAbiCompatibility,
    pub app_version: AppVersionCompatibility,
}

impl GrammarCompatibility {
    pub fn for_requirements(
        parser_abi: usize,
        app_version_req: Option<&VersionReq>,
        app_version: &Version,
    ) -> Self {
        Self {
            parser_abi: ParserAbiCompatibility::check(parser_abi),
            app_version: AppVersionCompatibility::check(app_version_req, app_version),
        }
    }

    pub fn is_compatible(&self) -> bool {
        self.parser_abi.is_compatible() && self.app_version.is_compatible()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParserAbiCompatibility {
    Compatible {
        parser_abi: usize,
        min_supported: usize,
        max_supported: usize,
    },
    Incompatible {
        parser_abi: usize,
        min_supported: usize,
        max_supported: usize,
    },
}

impl ParserAbiCompatibility {
    pub fn check(parser_abi: usize) -> Self {
        let min_supported = MIN_COMPATIBLE_LANGUAGE_VERSION;
        let max_supported = LANGUAGE_VERSION;
        if (min_supported..=max_supported).contains(&parser_abi) {
            Self::Compatible {
                parser_abi,
                min_supported,
                max_supported,
            }
        } else {
            Self::Incompatible {
                parser_abi,
                min_supported,
                max_supported,
            }
        }
    }

    pub fn is_compatible(&self) -> bool {
        matches!(self, Self::Compatible { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppVersionCompatibility {
    Compatible,
    NotDeclared,
    Incompatible { required: String, current: String },
}

impl AppVersionCompatibility {
    pub fn check(app_version_req: Option<&VersionReq>, app_version: &Version) -> Self {
        match app_version_req {
            Some(req) if req.matches(app_version) => Self::Compatible,
            Some(req) => Self::Incompatible {
                required: req.to_string(),
                current: app_version.to_string(),
            },
            None => Self::NotDeclared,
        }
    }

    pub fn is_compatible(&self) -> bool {
        matches!(self, Self::Compatible | Self::NotDeclared)
    }
}

pub(super) struct BuiltinSyntaxRegistry {
    syntaxes: HashMap<&'static str, Arc<TreeSitterSyntax>>,
    languages: &'static [LanguageSpec],
    bootstrap_languages: &'static [LanguageSpec],
    runtime_languages: Vec<LanguageSpec>,
    query_sources: HashMap<String, queries::QuerySources>,
    query_override_dir: Option<PathBuf>,
    cache_identity: u64,
}

impl BuiltinSyntaxRegistry {
    pub(super) fn new() -> Self {
        Self::new_with_query_override_dir(None)
    }

    pub(super) fn new_with_query_override_dir(query_override_dir: Option<&Path>) -> Self {
        let query_sources = built_in_query_sources();
        let syntaxes = build_syntaxes();
        let cache_identity = registry_cache_identity(&syntaxes);
        Self {
            syntaxes,
            languages: BUILTIN_LANGUAGE_SPECS,
            bootstrap_languages: BOOTSTRAP_NPM_WASM_LANGUAGE_SPECS.as_slice(),
            runtime_languages: Vec::new(),
            query_sources,
            query_override_dir: query_override_dir.map(Path::to_path_buf),
            cache_identity,
        }
    }

    pub(super) fn get(&self, key: &str) -> Option<&TreeSitterSyntax> {
        self.syntaxes.get(key).map(Arc::as_ref)
    }

    pub(super) fn syntax_for_injection_language(
        &self,
        language: &str,
    ) -> Option<&TreeSitterSyntax> {
        let normalized = normalize_injection_language(language)?;
        self.get(normalized.as_str())
    }

    pub(super) fn cache_identity(&self) -> u64 {
        self.cache_identity
    }

    pub(super) fn built_in_grammars(&self) -> Vec<BuiltInGrammar> {
        let mut seen = BTreeSet::new();
        self.languages
            .iter()
            .filter(|spec| self.syntaxes.contains_key(spec.syntax_key))
            .filter(|spec| seen.insert(spec.language_id))
            .map(LanguageSpec::to_built_in_grammar)
            .collect()
    }

    pub(super) fn language_for_filename(&self, filename: &str) -> Option<LanguageMatch> {
        self.language_specs().find_map(|spec| {
            spec.filenames
                .iter()
                .find(|candidate| filename.eq_ignore_ascii_case(candidate))
                .map(|matched| spec.to_match(matched))
        })
    }

    pub(super) fn language_for_compound_extension(&self, filename: &str) -> Option<LanguageMatch> {
        let filename = filename.to_ascii_lowercase();
        self.language_specs().find_map(|spec| {
            spec.compound_extensions
                .iter()
                .find(|compound| matches_compound_extension(&filename, compound))
                .map(|matched| spec.to_match(matched))
        })
    }

    pub(super) fn language_for_extension(&self, extension: &str) -> Option<LanguageMatch> {
        self.language_specs().find_map(|spec| {
            spec.extensions
                .iter()
                .find(|candidate| extension.eq_ignore_ascii_case(candidate))
                .map(|matched| spec.to_match(matched))
        })
    }

    pub(super) fn language_for_first_line(&self, first_line: &str) -> Option<LanguageMatch> {
        if first_line.starts_with("#!") {
            return self.language_for_shebang(first_line);
        }

        if first_line.starts_with("# syntax=docker/dockerfile") {
            return self.language_by_id("dockerfile", "# syntax=docker/dockerfile");
        }

        None
    }

    fn language_for_shebang(&self, first_line: &str) -> Option<LanguageMatch> {
        let commands = [
            ("ts-node", "typescript"),
            ("tsx", "tsx"),
            ("node", "javascript"),
            ("deno", "javascript"),
            ("python", "python"),
            ("python3", "python"),
            ("ruby", "ruby"),
            ("bash", "bash"),
            ("zsh", "bash"),
            ("sh", "bash"),
            ("lua", "lua"),
        ];

        commands.iter().find_map(|(needle, language_id)| {
            first_line
                .contains(needle)
                .then(|| self.language_by_id(language_id, needle))
                .flatten()
        })
    }

    fn language_by_id(
        &self,
        language_id: &str,
        matched_pattern: &'static str,
    ) -> Option<LanguageMatch> {
        self.language_specs()
            .find(|spec| spec.language_id == language_id)
            .map(|spec| spec.to_match(matched_pattern))
    }

    pub(super) fn load_runtime_grammars(
        &mut self,
        packages: &[InstalledGrammarPackage],
        policy: &RuntimeGrammarSecurityPolicy,
    ) {
        for package in packages {
            if !self.runtime_package_can_load(package, policy) {
                continue;
            }
            let syntax_key = normalize_language_key(&package.manifest.language);
            if let Some(query_sources) = self.load_runtime_query_sources(package) {
                self.query_sources.insert(syntax_key, query_sources);
            }
        }

        for package in packages {
            match package.manifest.runtime {
                GrammarRuntime::Native => self.load_native_runtime_grammar(package, policy),
                GrammarRuntime::Wasm => self.load_wasm_runtime_grammar(package),
            }
        }
        self.cache_identity = registry_cache_identity(&self.syntaxes);
    }

    fn runtime_package_can_load(
        &self,
        package: &InstalledGrammarPackage,
        policy: &RuntimeGrammarSecurityPolicy,
    ) -> bool {
        if !package.compatibility.is_compatible() {
            return false;
        }

        let syntax_key = normalize_language_key(&package.manifest.language);
        !syntax_key.is_empty()
            && !self.syntaxes.contains_key(syntax_key.as_str())
            && match package.manifest.runtime {
                GrammarRuntime::Native => {
                    package.manifest.platform == parser_loading::current_platform()
                        && policy.allows_installed_native_package(&package.manifest)
                }
                GrammarRuntime::Wasm => true,
            }
    }

    fn load_native_runtime_grammar(
        &mut self,
        package: &InstalledGrammarPackage,
        policy: &RuntimeGrammarSecurityPolicy,
    ) {
        if package.manifest.runtime != GrammarRuntime::Native
            || !package.compatibility.is_compatible()
            || package.manifest.platform != parser_loading::current_platform()
            || !policy.allows_installed_native_package(&package.manifest)
        {
            return;
        }

        let syntax_key = normalize_language_key(&package.manifest.language);
        if syntax_key.is_empty() || self.syntaxes.contains_key(syntax_key.as_str()) {
            return;
        }

        let language = match parser_loading::load_native_language(package) {
            Ok(language) => language,
            Err(err) => {
                eprintln!(
                    "Failed to load native Tree-sitter parser for {}: {}",
                    package.manifest.language,
                    err.message()
                );
                return;
            }
        };

        let syntax_key = leak_static_str(syntax_key);
        let compiled_queries = match queries::compile_query_bundle(
            syntax_key,
            &language,
            &self.query_sources,
            self.query_override_dir.as_deref(),
        ) {
            Ok(queries) => queries,
            Err(err) => {
                eprintln!("{}", err.message());
                queries::CompiledQueries::disabled()
            }
        };
        self.syntaxes.insert(
            syntax_key,
            Arc::new(TreeSitterSyntax::new(
                syntax_key,
                language,
                compiled_queries,
                ParserRuntime::Native,
                ParserCacheIdentity::runtime(package),
            )),
        );
        self.runtime_languages
            .push(LanguageSpec::from_manifest(syntax_key, &package.manifest));
    }

    fn load_wasm_runtime_grammar(&mut self, package: &InstalledGrammarPackage) {
        if package.manifest.runtime != GrammarRuntime::Wasm
            || !package.compatibility.is_compatible()
        {
            return;
        }

        let syntax_key = normalize_language_key(&package.manifest.language);
        if syntax_key.is_empty() || self.syntaxes.contains_key(syntax_key.as_str()) {
            return;
        }

        let language = match parser_loading::load_wasm_language(package) {
            Ok(language) => language,
            Err(err) => {
                eprintln!(
                    "Failed to load WASM Tree-sitter parser for {}: {}",
                    package.manifest.language,
                    err.message()
                );
                return;
            }
        };

        let syntax_key = leak_static_str(syntax_key);
        let compiled_queries = match queries::compile_query_bundle(
            syntax_key,
            &language,
            &self.query_sources,
            self.query_override_dir.as_deref(),
        ) {
            Ok(queries) => queries,
            Err(err) => {
                eprintln!("{}", err.message());
                queries::CompiledQueries::disabled()
            }
        };
        self.syntaxes.insert(
            syntax_key,
            Arc::new(TreeSitterSyntax::new(
                syntax_key,
                language,
                compiled_queries,
                ParserRuntime::Wasm,
                ParserCacheIdentity::runtime(package),
            )),
        );
        self.runtime_languages
            .push(LanguageSpec::from_manifest(syntax_key, &package.manifest));
    }

    fn load_runtime_query_sources(
        &self,
        package: &InstalledGrammarPackage,
    ) -> Option<queries::QuerySources> {
        let mut sources = queries::QuerySources::default();
        let mut loaded_any = false;
        for (query_type, query_path) in [
            (
                queries::QueryType::Highlights,
                package.manifest.files.highlights.as_deref(),
            ),
            (
                queries::QueryType::Injections,
                package.manifest.files.injections.as_deref(),
            ),
            (
                queries::QueryType::Locals,
                package.manifest.files.locals.as_deref(),
            ),
        ] {
            let Some(query_path) = query_path else {
                continue;
            };
            let query_path =
                match parser_loading::package_relative_path(&package.package_dir, query_path) {
                    Ok(path) => path,
                    Err(err) => {
                        eprintln!(
                            "Failed to load Tree-sitter runtime query for {}: {}",
                            package.manifest.language,
                            err.message()
                        );
                        return None;
                    }
                };
            match fs::read_to_string(&query_path) {
                Ok(query) => {
                    sources.set(query_type, query);
                    loaded_any = true;
                }
                Err(err) => {
                    eprintln!(
                        "Failed to read Tree-sitter runtime query {}: {}",
                        query_path.display(),
                        err
                    );
                    return None;
                }
            }
        }

        if normalize_language_key(&package.manifest.language) == "php" {
            let injections = sources
                .get(queries::QueryType::Injections)
                .map(|existing| format!("{existing}\n{PHP_HTML_INJECTIONS_QUERY}"))
                .unwrap_or_else(|| PHP_HTML_INJECTIONS_QUERY.to_string());
            sources.set(queries::QueryType::Injections, injections);
            loaded_any = true;
        }
        if normalize_language_key(&package.manifest.language) == "cpp" {
            if let Some(highlights) = sources.get(queries::QueryType::Highlights) {
                let highlights = ensure_cpp_query_has_c_base(highlights);
                sources.set(queries::QueryType::Highlights, highlights);
            }
        }

        loaded_any.then_some(sources)
    }

    fn language_specs(&self) -> impl Iterator<Item = &LanguageSpec> {
        self.languages
            .iter()
            .chain(self.bootstrap_languages.iter())
            .chain(self.runtime_languages.iter())
    }
}

pub(super) fn current_app_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or_else(|_| Version::new(0, 0, 0))
}

pub fn read_local_registry(runtime_dir: &Path) -> LocalRegistryRead {
    let registry_path = runtime_dir.join(REGISTRY_JSON_PATH);
    let raw = match fs::read_to_string(&registry_path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            return LocalRegistryRead {
                registry: None,
                errors: Vec::new(),
            };
        }
        Err(err) => {
            return LocalRegistryRead {
                registry: None,
                errors: vec![GrammarRegistryError::new(
                    GrammarRegistryErrorKind::ReadRegistry,
                    registry_path,
                    err.to_string(),
                )],
            };
        }
    };

    match serde_json::from_str::<GrammarRegistryFile>(&raw) {
        Ok(registry) if registry.is_supported_schema() => LocalRegistryRead {
            registry: Some(registry),
            errors: Vec::new(),
        },
        Ok(registry) => LocalRegistryRead {
            registry: None,
            errors: vec![GrammarRegistryError::new(
                GrammarRegistryErrorKind::UnsupportedRegistrySchema,
                registry_path,
                format!("unsupported registry schema {}", registry.schema_version),
            )],
        },
        Err(err) => LocalRegistryRead {
            registry: None,
            errors: vec![GrammarRegistryError::new(
                GrammarRegistryErrorKind::ParseRegistry,
                registry_path,
                err.to_string(),
            )],
        },
    }
}

pub fn read_runtime_registry(runtime_dir: &Path) -> LocalRegistryRead {
    let mut read = read_local_registry(runtime_dir);
    let bootstrap = bootstrap_runtime_registry();
    match read.registry.as_mut() {
        Some(registry) => merge_bootstrap_grammars(registry, bootstrap),
        None => read.registry = Some(bootstrap),
    }
    read
}

fn merge_bootstrap_grammars(registry: &mut GrammarRegistryFile, bootstrap: GrammarRegistryFile) {
    let mut languages = registry
        .grammars
        .iter()
        .map(|entry| normalize_language_key(&entry.language))
        .collect::<BTreeSet<_>>();
    for entry in bootstrap.grammars {
        if languages.insert(normalize_language_key(&entry.language)) {
            registry.grammars.push(entry);
        }
    }
}

fn bootstrap_runtime_registry() -> GrammarRegistryFile {
    GrammarRegistryFile {
        schema_version: REGISTRY_SCHEMA_VERSION,
        cache: RegistryCacheMetadata::new(0, u64::MAX),
        grammars: BOOTSTRAP_NPM_WASM_GRAMMARS
            .iter()
            .map(npm_wasm_grammar)
            .collect(),
    }
}

struct NpmWasmGrammarSpec {
    language: &'static str,
    version: (u64, u64, u64),
    url: &'static str,
    filetypes: &'static [&'static str],
    extensions: &'static [&'static str],
    filenames: &'static [&'static str],
}

impl NpmWasmGrammarSpec {
    fn version(&self) -> Version {
        let (major, minor, patch) = self.version;
        Version::new(major, minor, patch)
    }
}

const BOOTSTRAP_NPM_WASM_GRAMMARS: &[NpmWasmGrammarSpec] = &[
    NpmWasmGrammarSpec {
        language: "go",
        version: (0, 25, 0),
        url: "https://registry.npmjs.org/tree-sitter-go/-/tree-sitter-go-0.25.0.tgz",
        filetypes: &["go"],
        extensions: &["go"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "twig",
        version: (1, 2, 0),
        url: "https://registry.npmjs.org/tree-sitter-shopware-twig/-/tree-sitter-shopware-twig-1.2.0.tgz",
        filetypes: &["twig", "html.twig"],
        extensions: &["twig"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "rust",
        version: (0, 24, 0),
        url: "https://registry.npmjs.org/tree-sitter-rust/-/tree-sitter-rust-0.24.0.tgz",
        filetypes: &["rust"],
        extensions: &["rs"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "yaml",
        version: (0, 7, 1),
        url: "https://registry.npmjs.org/@tree-sitter-grammars/tree-sitter-yaml/-/tree-sitter-yaml-0.7.1.tgz",
        filetypes: &["yaml"],
        extensions: &["yaml", "yml"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "json",
        version: (0, 24, 8),
        url: "https://registry.npmjs.org/tree-sitter-json/-/tree-sitter-json-0.24.8.tgz",
        filetypes: &["json"],
        extensions: &["json"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "toml",
        version: (0, 7, 0),
        url: "https://registry.npmjs.org/@tree-sitter-grammars/tree-sitter-toml/-/tree-sitter-toml-0.7.0.tgz",
        filetypes: &["toml"],
        extensions: &["toml"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "php",
        version: (0, 24, 2),
        url: "https://registry.npmjs.org/tree-sitter-php/-/tree-sitter-php-0.24.2.tgz",
        filetypes: &["php"],
        extensions: &["php"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "php-only",
        version: (0, 24, 2),
        url: "https://registry.npmjs.org/tree-sitter-php/-/tree-sitter-php-0.24.2.tgz",
        filetypes: &["php"],
        extensions: &["php"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "html",
        version: (0, 23, 2),
        url: "https://registry.npmjs.org/tree-sitter-html/-/tree-sitter-html-0.23.2.tgz",
        filetypes: &["html"],
        extensions: &["html", "htm"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "css",
        version: (0, 23, 2),
        url: "https://registry.npmjs.org/tree-sitter-css/-/tree-sitter-css-0.23.2.tgz",
        filetypes: &["css"],
        extensions: &["css"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "scss",
        version: (1, 0, 0),
        url: "https://registry.npmjs.org/tree-sitter-scss/-/tree-sitter-scss-1.0.0.tgz",
        filetypes: &["scss", "sass"],
        extensions: &["scss", "sass"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "xml",
        version: (0, 26, 1),
        url: "https://registry.npmjs.org/@lumis-sh/wasm-xml/-/wasm-xml-0.26.1.tgz",
        filetypes: &["xml"],
        extensions: &[
            "xml", "xsd", "xsl", "xslt", "rng", "rss", "atom", "svg", "plist", "wsdl", "xliff",
        ],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "javascript",
        version: (0, 23, 1),
        url: "https://registry.npmjs.org/tree-sitter-javascript/-/tree-sitter-javascript-0.23.1.tgz",
        filetypes: &["javascript"],
        extensions: &["js", "mjs", "cjs", "jsx"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "typescript",
        version: (0, 23, 2),
        url: "https://registry.npmjs.org/tree-sitter-typescript/-/tree-sitter-typescript-0.23.2.tgz",
        filetypes: &["typescript"],
        extensions: &["ts"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "tsx",
        version: (0, 23, 2),
        url: "https://registry.npmjs.org/tree-sitter-typescript/-/tree-sitter-typescript-0.23.2.tgz",
        filetypes: &["tsx"],
        extensions: &["tsx"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "markdown",
        version: (0, 26, 0),
        url: "https://registry.npmjs.org/@lumis-sh/wasm-markdown/-/wasm-markdown-0.26.0.tgz",
        filetypes: &["markdown"],
        extensions: &["md"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "lua",
        version: (0, 4, 1),
        url: "https://registry.npmjs.org/@tree-sitter-grammars/tree-sitter-lua/-/tree-sitter-lua-0.4.1.tgz",
        filetypes: &["lua"],
        extensions: &["lua"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "c",
        version: (0, 24, 1),
        url: "https://registry.npmjs.org/tree-sitter-c/-/tree-sitter-c-0.24.1.tgz",
        filetypes: &["c"],
        extensions: &["c", "h"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "cpp",
        version: (0, 23, 4),
        url: "https://registry.npmjs.org/tree-sitter-cpp/-/tree-sitter-cpp-0.23.4.tgz",
        filetypes: &["cpp", "c++"],
        extensions: &["cc", "cpp", "cxx", "c++", "hh", "hpp", "hxx", "h++", "ipp", "inl", "tpp"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "c-sharp",
        version: (0, 23, 5),
        url: "https://registry.npmjs.org/tree-sitter-c-sharp/-/tree-sitter-c-sharp-0.23.5.tgz",
        filetypes: &["csharp", "c-sharp"],
        extensions: &["cs", "csx"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "zig",
        version: (1, 1, 2),
        url: "https://registry.npmjs.org/@tree-sitter-grammars/tree-sitter-zig/-/tree-sitter-zig-1.1.2.tgz",
        filetypes: &["zig"],
        extensions: &["zig", "zon"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "python",
        version: (0, 25, 0),
        url: "https://registry.npmjs.org/tree-sitter-python/-/tree-sitter-python-0.25.0.tgz",
        filetypes: &["python"],
        extensions: &["py", "pyw", "pyi"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "java",
        version: (0, 23, 5),
        url: "https://registry.npmjs.org/tree-sitter-java/-/tree-sitter-java-0.23.5.tgz",
        filetypes: &["java"],
        extensions: &["java"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "objc",
        version: (3, 0, 2),
        url: "https://registry.npmjs.org/tree-sitter-objc/-/tree-sitter-objc-3.0.2.tgz",
        filetypes: &["objc", "objective-c"],
        extensions: &["m", "mm"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "ruby",
        version: (0, 23, 1),
        url: "https://registry.npmjs.org/tree-sitter-ruby/-/tree-sitter-ruby-0.23.1.tgz",
        filetypes: &["ruby"],
        extensions: &["rb", "rake", "gemspec"],
        filenames: &["gemfile", "rakefile", "guardfile", "capfile", "podfile", "vagrantfile"],
    },
    NpmWasmGrammarSpec {
        language: "elixir",
        version: (0, 3, 5),
        url: "https://registry.npmjs.org/tree-sitter-elixir/-/tree-sitter-elixir-0.3.5.tgz",
        filetypes: &["elixir"],
        extensions: &["ex", "exs"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "haskell",
        version: (0, 23, 1),
        url: "https://registry.npmjs.org/tree-sitter-haskell/-/tree-sitter-haskell-0.23.1.tgz",
        filetypes: &["haskell"],
        extensions: &["hs", "lhs"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "ocaml",
        version: (0, 24, 2),
        url: "https://registry.npmjs.org/tree-sitter-ocaml/-/tree-sitter-ocaml-0.24.2.tgz",
        filetypes: &["ocaml"],
        extensions: &["ml", "mli", "mll", "mly"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "scala",
        version: (0, 24, 0),
        url: "https://registry.npmjs.org/tree-sitter-scala/-/tree-sitter-scala-0.24.0.tgz",
        filetypes: &["scala"],
        extensions: &["scala", "sc"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "r",
        version: (1, 2, 0),
        url: "https://registry.npmjs.org/@davisvaughan/tree-sitter-r/-/tree-sitter-r-1.2.0.tgz",
        filetypes: &["r"],
        extensions: &["r"],
        filenames: &[".rprofile"],
    },
    NpmWasmGrammarSpec {
        language: "julia",
        version: (0, 23, 1),
        url: "https://registry.npmjs.org/tree-sitter-julia/-/tree-sitter-julia-0.23.1.tgz",
        filetypes: &["julia"],
        extensions: &["jl"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "bash",
        version: (0, 25, 1),
        url: "https://registry.npmjs.org/tree-sitter-bash/-/tree-sitter-bash-0.25.1.tgz",
        filetypes: &["bash", "shell"],
        extensions: &["sh", "bash", "zsh"],
        filenames: &[".bashrc", ".bash_profile", ".zshrc"],
    },
    NpmWasmGrammarSpec {
        language: "powershell",
        version: (0, 26, 4),
        url: "https://registry.npmjs.org/tree-sitter-powershell/-/tree-sitter-powershell-0.26.4.tgz",
        filetypes: &["powershell"],
        extensions: &["ps1", "psm1", "psd1"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "make",
        version: (1, 1, 1),
        url: "https://registry.npmjs.org/tree-sitter-make/-/tree-sitter-make-1.1.1.tgz",
        filetypes: &["make"],
        extensions: &["mk", "mak"],
        filenames: &["makefile", "gnumakefile"],
    },
    NpmWasmGrammarSpec {
        language: "starlark",
        version: (1, 3, 0),
        url: "https://registry.npmjs.org/tree-sitter-starlark/-/tree-sitter-starlark-1.3.0.tgz",
        filetypes: &["starlark", "bazel"],
        extensions: &["bzl", "star"],
        filenames: &["build", "workspace", "build.bazel", "workspace.bazel", "module.bazel"],
    },
    NpmWasmGrammarSpec {
        language: "fsharp",
        version: (0, 1, 0),
        url: "https://registry.npmjs.org/tree-sitter-fsharp/-/tree-sitter-fsharp-0.1.0.tgz",
        filetypes: &["fsharp", "f-sharp"],
        extensions: &["fs", "fsi", "fsx"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "awk",
        version: (0, 7, 2),
        url: "https://registry.npmjs.org/tree-sitter-awk/-/tree-sitter-awk-0.7.2.tgz",
        filetypes: &["awk"],
        extensions: &["awk"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "batch",
        version: (0, 11, 1),
        url: "https://registry.npmjs.org/tree-sitter-batch/-/tree-sitter-batch-0.11.1.tgz",
        filetypes: &["batch", "bat"],
        extensions: &["bat", "cmd"],
        filenames: &[],
    },
    NpmWasmGrammarSpec {
        language: "cuda",
        version: (0, 21, 1),
        url: "https://registry.npmjs.org/tree-sitter-cuda/-/tree-sitter-cuda-0.21.1.tgz",
        filetypes: &["cuda"],
        extensions: &["cu", "cuh"],
        filenames: &[],
    },
];

fn npm_wasm_grammar(spec: &NpmWasmGrammarSpec) -> GrammarRegistryEntry {
    GrammarRegistryEntry {
        language: spec.language.to_string(),
        version: spec.version(),
        parser_abi: LANGUAGE_VERSION,
        app_version_req: None,
        runtime: Some(GrammarRuntime::Wasm),
        platforms: vec!["wasm".to_string()],
        filetypes: spec
            .filetypes
            .iter()
            .map(|value| value.to_string())
            .collect(),
        extensions: spec
            .extensions
            .iter()
            .map(|value| value.to_string())
            .collect(),
        filenames: spec
            .filenames
            .iter()
            .map(|value| value.to_string())
            .collect(),
        first_line_regex: None,
        content_regex: None,
        packages: vec![GrammarPackageDownload {
            url: spec.url.to_string(),
            sha256: None,
            signature: None,
            source: GrammarPackageSource::Community,
            runtime: Some(GrammarRuntime::Wasm),
            platform: Some("wasm".to_string()),
        }],
    }
}

pub fn scan_installed_packages(runtime_dir: &Path, app_version: &Version) -> InstalledGrammarScan {
    let parsers_dir = runtime_dir.join("parsers");
    let mut errors = Vec::new();
    let read_dir = match fs::read_dir(&parsers_dir) {
        Ok(read_dir) => read_dir,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            return InstalledGrammarScan {
                packages: Vec::new(),
                errors,
            };
        }
        Err(err) => {
            return InstalledGrammarScan {
                packages: Vec::new(),
                errors: vec![GrammarRegistryError::new(
                    GrammarRegistryErrorKind::ReadPackageDirectory,
                    parsers_dir,
                    err.to_string(),
                )],
            };
        }
    };

    let mut manifest_paths = Vec::new();
    for entry in read_dir {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                errors.push(GrammarRegistryError::new(
                    GrammarRegistryErrorKind::ReadPackageDirectory,
                    parsers_dir.clone(),
                    err.to_string(),
                ));
                continue;
            }
        };

        match entry.file_type() {
            Ok(file_type) if file_type.is_dir() => {
                manifest_paths.push(entry.path().join(PACKAGE_MANIFEST_FILENAME));
            }
            Ok(_) => {}
            Err(err) => errors.push(GrammarRegistryError::new(
                GrammarRegistryErrorKind::ReadPackageDirectory,
                entry.path(),
                err.to_string(),
            )),
        }
    }

    manifest_paths.sort();
    let mut packages = Vec::new();
    for manifest_path in manifest_paths {
        let raw = match fs::read_to_string(&manifest_path) {
            Ok(raw) => raw,
            Err(err) => {
                errors.push(GrammarRegistryError::new(
                    GrammarRegistryErrorKind::ReadManifest,
                    manifest_path,
                    err.to_string(),
                ));
                continue;
            }
        };

        let manifest = match serde_json::from_str::<GrammarPackageManifest>(&raw) {
            Ok(manifest) => manifest,
            Err(err) => {
                errors.push(GrammarRegistryError::new(
                    GrammarRegistryErrorKind::ParseManifest,
                    manifest_path,
                    err.to_string(),
                ));
                continue;
            }
        };

        let compatibility = GrammarCompatibility::for_requirements(
            manifest.parser_abi,
            manifest.app_version_req.as_ref(),
            app_version,
        );
        let package_dir = manifest_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(PathBuf::new);
        if let Err(err) = validate_installed_package_files(&package_dir, &manifest, &manifest_path)
        {
            errors.push(err);
            continue;
        }
        packages.push(InstalledGrammarPackage {
            package_dir,
            manifest_path,
            manifest,
            compatibility,
        });
    }

    InstalledGrammarScan { packages, errors }
}

fn validate_installed_package_files(
    package_dir: &Path,
    manifest: &GrammarPackageManifest,
    manifest_path: &Path,
) -> Result<(), GrammarRegistryError> {
    for relative in manifest_file_paths(&manifest.files) {
        let path = safe_manifest_path(package_dir, relative, manifest_path)?;
        if !path.is_file() {
            return Err(GrammarRegistryError::new(
                GrammarRegistryErrorKind::MissingPackageFile,
                path,
                "manifest references a file that is not present",
            ));
        }
        if !manifest.sha256.contains_key(relative) {
            return Err(GrammarRegistryError::new(
                GrammarRegistryErrorKind::MissingFileHash,
                manifest_path.to_path_buf(),
                "manifest is missing a SHA-256 hash for a package file",
            ));
        }
    }

    for (relative, expected) in &manifest.sha256 {
        let path = safe_manifest_path(package_dir, relative, manifest_path)?;
        let bytes = fs::read(&path).map_err(|err| {
            GrammarRegistryError::new(
                GrammarRegistryErrorKind::ReadPackageFile,
                path.clone(),
                err.to_string(),
            )
        })?;
        let actual = sha256_hex(&bytes);
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(GrammarRegistryError::new(
                GrammarRegistryErrorKind::FileHashMismatch,
                path,
                format!("file hash {actual} did not match expected {expected}"),
            ));
        }
    }
    Ok(())
}

fn manifest_file_paths(files: &GrammarPackageFiles) -> impl Iterator<Item = &String> {
    [
        files.parser.as_ref(),
        files.wasm.as_ref(),
        files.highlights.as_ref(),
        files.injections.as_ref(),
        files.locals.as_ref(),
    ]
    .into_iter()
    .flatten()
}

fn safe_manifest_path(
    package_dir: &Path,
    relative_path: &str,
    manifest_path: &Path,
) -> Result<PathBuf, GrammarRegistryError> {
    let trimmed = relative_path.trim();
    if trimmed.is_empty() || trimmed.contains('\\') {
        return Err(GrammarRegistryError::new(
            GrammarRegistryErrorKind::InvalidPackagePath,
            manifest_path.to_path_buf(),
            "manifest path is empty or uses unsupported separators",
        ));
    }
    let path = Path::new(trimmed);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(GrammarRegistryError::new(
            GrammarRegistryErrorKind::InvalidPackagePath,
            manifest_path.to_path_buf(),
            "manifest path must stay inside the grammar package",
        ));
    }
    Ok(package_dir.join(path))
}

pub(super) fn list_grammar_inventory_for_app_version(
    runtime_dir: &Path,
    builtins: &BuiltinSyntaxRegistry,
    app_version: &Version,
    policy: &RuntimeGrammarSecurityPolicy,
) -> GrammarInventoryReport {
    let registry_read = read_runtime_registry(runtime_dir);
    let installed_scan = scan_installed_packages(runtime_dir, app_version);
    let built_in = builtins.built_in_grammars();
    let built_in_keys: BTreeSet<String> = built_in
        .iter()
        .map(|grammar| normalize_language_key(&grammar.language_id))
        .collect();
    let installed_keys: BTreeSet<String> = installed_scan
        .packages
        .iter()
        .filter(|package| installed_package_can_load(package, policy))
        .map(|package| normalize_language_key(&package.manifest.language))
        .collect();

    let registry_cache = registry_read
        .registry
        .as_ref()
        .map(|registry| registry.cache.clone());
    let registry_cache_fresh =
        registry_cache_fresh_at(registry_cache.as_ref(), current_unix_seconds());
    let missing = registry_read
        .registry
        .as_ref()
        .map(|registry| missing_grammars(registry, &built_in_keys, &installed_keys, app_version))
        .unwrap_or_default();
    let mut errors = registry_read.errors;
    errors.extend(installed_scan.errors);

    GrammarInventoryReport {
        built_in,
        installed: installed_scan.packages,
        missing,
        registry_cache,
        registry_cache_fresh,
        errors,
    }
}

pub(super) fn installed_package_can_load(
    package: &InstalledGrammarPackage,
    policy: &RuntimeGrammarSecurityPolicy,
) -> bool {
    package.compatibility.is_compatible()
        && match package.manifest.runtime {
            GrammarRuntime::Native => {
                package.manifest.platform == parser_loading::current_platform()
                    && package.manifest.files.parser.is_some()
                    && policy.allows_installed_native_package(&package.manifest)
            }
            GrammarRuntime::Wasm => wasm_package_can_load(package),
        }
}

fn wasm_package_can_load(package: &InstalledGrammarPackage) -> bool {
    parser_loading::load_wasm_language(package).is_ok()
}

fn registry_cache_fresh_at(cache: Option<&RegistryCacheMetadata>, now_unix_seconds: u64) -> bool {
    let fallback = RegistryCacheMetadata::with_default_ttl(0);
    cache.unwrap_or(&fallback).is_fresh_at(now_unix_seconds)
}

fn current_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn env_list(name: &str) -> Option<Vec<String>> {
    std::env::var(name).ok().map(|value| {
        value
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .collect()
    })
}

fn missing_grammars(
    registry: &GrammarRegistryFile,
    built_in_keys: &BTreeSet<String>,
    installed_keys: &BTreeSet<String>,
    app_version: &Version,
) -> Vec<MissingGrammar> {
    registry
        .grammars
        .iter()
        .filter(|entry| {
            let key = normalize_language_key(&entry.language);
            !built_in_keys.contains(&key) && !installed_keys.contains(&key)
        })
        .map(|entry| MissingGrammar {
            compatibility: GrammarCompatibility::for_requirements(
                entry.parser_abi,
                entry.app_version_req.as_ref(),
                app_version,
            ),
            entry: entry.clone(),
        })
        .collect()
}

fn normalize_language_key(language: &str) -> String {
    language.trim().to_ascii_lowercase()
}

fn normalize_injection_language(language: &str) -> Option<String> {
    let marker = language
        .trim()
        .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == '{')
        .next()
        .unwrap_or_default()
        .trim();
    if marker.is_empty() {
        return None;
    }

    let marker = marker.strip_prefix("source.").unwrap_or(marker);
    let normalized = normalize_language_key(marker);
    let key = match normalized.as_str() {
        "cjs" | "javascriptreact" | "js" | "jsx" | "mjs" | "node" => "javascript",
        "cts" | "mts" | "ts" => "typescript",
        "html.twig" => "twig",
        "jsonc" => "json",
        "md" => "markdown",
        other => other,
    };
    Some(key.to_string())
}

fn ensure_query_inherits(source: &str, inherited_language: &str) -> String {
    let expected = normalize_language_key(inherited_language);
    let modeline = queries::parse_modelines(source);
    if modeline
        .inherits
        .iter()
        .any(|language| language == &expected)
    {
        source.to_string()
    } else {
        format!("; inherits: {expected}\n{source}")
    }
}

fn ensure_cpp_query_has_c_base(source: &str) -> String {
    if source.contains("(preproc_directive)") && source.contains("(primitive_type)") {
        source.to_string()
    } else {
        ensure_query_inherits(source, "c")
    }
}

fn leak_static_str(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

fn leak_patterns<'a, I>(patterns: I) -> &'static [&'static str]
where
    I: IntoIterator<Item = &'a String>,
{
    leak_patterns_from_strs(patterns.into_iter().map(String::as_str))
}

fn leak_patterns_from_strs<'a, I>(patterns: I) -> &'static [&'static str]
where
    I: IntoIterator<Item = &'a str>,
{
    let mut seen = BTreeSet::new();
    let values: Vec<&'static str> = patterns
        .into_iter()
        .filter_map(normalize_detection_pattern)
        .filter(|pattern| seen.insert(pattern.clone()))
        .map(leak_static_str)
        .collect();
    Box::leak(values.into_boxed_slice())
}

fn leak_filename_patterns<'a, I>(patterns: I) -> &'static [&'static str]
where
    I: IntoIterator<Item = &'a String>,
{
    leak_filename_patterns_from_strs(patterns.into_iter().map(String::as_str))
}

fn leak_filename_patterns_from_strs<'a, I>(patterns: I) -> &'static [&'static str]
where
    I: IntoIterator<Item = &'a str>,
{
    let mut seen = BTreeSet::new();
    let values: Vec<&'static str> = patterns
        .into_iter()
        .filter_map(normalize_filename_pattern)
        .filter(|pattern| seen.insert(pattern.clone()))
        .map(leak_static_str)
        .collect();
    Box::leak(values.into_boxed_slice())
}

fn normalize_detection_pattern(pattern: &str) -> Option<String> {
    let pattern = pattern.trim().trim_start_matches('.');
    (!pattern.is_empty()).then(|| pattern.to_ascii_lowercase())
}

fn normalize_filename_pattern(pattern: &str) -> Option<String> {
    let pattern = pattern.trim();
    (!pattern.is_empty()).then(|| pattern.to_ascii_lowercase())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum ParserRuntime {
    Native,
    Wasm,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct ParserCacheIdentity {
    pub(super) runtime: ParserRuntime,
    pub(super) language_abi: usize,
    pub(super) tree_sitter_language_version: usize,
    pub(super) source: String,
}

impl ParserCacheIdentity {
    fn runtime(package: &InstalledGrammarPackage) -> Self {
        let parser_path = match package.manifest.runtime {
            GrammarRuntime::Native => package.manifest.files.parser.as_deref(),
            GrammarRuntime::Wasm => package.manifest.files.wasm.as_deref(),
        }
        .unwrap_or_default();

        Self {
            runtime: package.manifest.runtime.into(),
            language_abi: package.manifest.parser_abi,
            tree_sitter_language_version: LANGUAGE_VERSION,
            source: format!(
                "{}:{}:{}:{}",
                package.manifest.language,
                package.manifest.version,
                package.manifest_path.display(),
                parser_path
            ),
        }
    }
}

impl From<GrammarRuntime> for ParserRuntime {
    fn from(value: GrammarRuntime) -> Self {
        match value {
            GrammarRuntime::Native => Self::Native,
            GrammarRuntime::Wasm => Self::Wasm,
        }
    }
}

const PHP_HTML_INJECTIONS_QUERY: &str = r#"
((text) @injection.content
  (#set! injection.language "html")
  (#set! injection.combined))
"#;

#[derive(Debug)]
pub(super) struct TreeSitterSyntax {
    name: &'static str,
    language: Language,
    queries: queries::CompiledQueries,
    parser_runtime: ParserRuntime,
    parser_identity: ParserCacheIdentity,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct LanguageMatch {
    pub(super) language_id: &'static str,
    pub(super) syntax_key: &'static str,
    pub(super) matched_pattern: &'static str,
}

struct LanguageSpec {
    language_id: &'static str,
    syntax_key: &'static str,
    filenames: &'static [&'static str],
    extensions: &'static [&'static str],
    compound_extensions: &'static [&'static str],
}

impl LanguageSpec {
    fn from_npm_wasm_spec(spec: &NpmWasmGrammarSpec) -> Self {
        let filenames = leak_filename_patterns_from_strs(spec.filenames.iter().copied());
        let extensions = leak_patterns_from_strs(
            spec.extensions.iter().copied().chain(
                spec.filetypes
                    .iter()
                    .copied()
                    .filter(|filetype| !filetype.contains('.')),
            ),
        );
        let compound_extensions = leak_patterns_from_strs(
            spec.filetypes
                .iter()
                .copied()
                .filter(|filetype| filetype.contains('.')),
        );

        Self {
            language_id: spec.language,
            syntax_key: spec.language,
            filenames,
            extensions,
            compound_extensions,
        }
    }

    fn from_manifest(language_id: &'static str, manifest: &GrammarPackageManifest) -> Self {
        let filenames = leak_filename_patterns(manifest.filenames.iter());
        let extensions = leak_patterns(
            manifest.extensions.iter().chain(
                manifest
                    .filetypes
                    .iter()
                    .filter(|filetype| !filetype.contains('.')),
            ),
        );
        let compound_extensions = leak_patterns(
            manifest
                .filetypes
                .iter()
                .filter(|filetype| filetype.contains('.')),
        );

        Self {
            language_id,
            syntax_key: language_id,
            filenames,
            extensions,
            compound_extensions,
        }
    }

    fn to_match(&self, matched_pattern: &'static str) -> LanguageMatch {
        LanguageMatch {
            language_id: self.language_id,
            syntax_key: self.syntax_key,
            matched_pattern,
        }
    }

    fn to_built_in_grammar(&self) -> BuiltInGrammar {
        BuiltInGrammar {
            language_id: self.language_id.to_string(),
            syntax_key: self.syntax_key.to_string(),
            filenames: self
                .filenames
                .iter()
                .map(|value| value.to_string())
                .collect(),
            extensions: self
                .extensions
                .iter()
                .map(|value| value.to_string())
                .collect(),
            compound_extensions: self
                .compound_extensions
                .iter()
                .map(|value| value.to_string())
                .collect(),
        }
    }
}

fn bootstrap_npm_wasm_language_specs() -> Vec<LanguageSpec> {
    BOOTSTRAP_NPM_WASM_GRAMMARS
        .iter()
        .map(LanguageSpec::from_npm_wasm_spec)
        .collect()
}

static BOOTSTRAP_NPM_WASM_LANGUAGE_SPECS: LazyLock<Vec<LanguageSpec>> =
    LazyLock::new(bootstrap_npm_wasm_language_specs);

const BUILTIN_LANGUAGE_SPECS: &[LanguageSpec] = &[
    LanguageSpec {
        language_id: "blade",
        syntax_key: "blade",
        filenames: &[],
        extensions: &[],
        compound_extensions: &["blade.php"],
    },
    LanguageSpec {
        language_id: "twig",
        syntax_key: "twig",
        filenames: &[],
        extensions: &["twig"],
        compound_extensions: &["html.twig"],
    },
    LanguageSpec {
        language_id: "yaml",
        syntax_key: "yaml",
        filenames: &[],
        extensions: &["yaml", "yml"],
        compound_extensions: &[],
    },
    LanguageSpec {
        language_id: "json",
        syntax_key: "json",
        filenames: &[],
        extensions: &["json"],
        compound_extensions: &[],
    },
    LanguageSpec {
        language_id: "jsonc",
        syntax_key: "json",
        filenames: &[],
        extensions: &["jsonc"],
        compound_extensions: &[],
    },
    LanguageSpec {
        language_id: "toml",
        syntax_key: "toml",
        filenames: &["cargo.lock"],
        extensions: &["toml"],
        compound_extensions: &[],
    },
    LanguageSpec {
        language_id: "dockerfile",
        syntax_key: "dockerfile",
        filenames: &["dockerfile", "containerfile"],
        extensions: &[],
        compound_extensions: &[],
    },
    LanguageSpec {
        language_id: "rust",
        syntax_key: "rust",
        filenames: &[],
        extensions: &["rs"],
        compound_extensions: &[],
    },
    LanguageSpec {
        language_id: "php",
        syntax_key: "php",
        filenames: &[],
        extensions: &["php", "phtml"],
        compound_extensions: &[],
    },
    LanguageSpec {
        language_id: "html",
        syntax_key: "html",
        filenames: &[],
        extensions: &["html", "htm"],
        compound_extensions: &[],
    },
    LanguageSpec {
        language_id: "xml",
        syntax_key: "xml",
        filenames: &[],
        extensions: &[
            "xml", "xsd", "xsl", "xslt", "rng", "rss", "atom", "svg", "plist", "wsdl", "xliff",
        ],
        compound_extensions: &[],
    },
    LanguageSpec {
        language_id: "css",
        syntax_key: "css",
        filenames: &[],
        extensions: &["css"],
        compound_extensions: &[],
    },
    LanguageSpec {
        language_id: "scss",
        syntax_key: "scss",
        filenames: &[],
        extensions: &["scss", "sass"],
        compound_extensions: &[],
    },
    LanguageSpec {
        language_id: "less",
        syntax_key: "less",
        filenames: &[],
        extensions: &["less"],
        compound_extensions: &[],
    },
    LanguageSpec {
        language_id: "javascript",
        syntax_key: "javascript",
        filenames: &[],
        extensions: &["js", "mjs", "cjs", "jsx"],
        compound_extensions: &[],
    },
    LanguageSpec {
        language_id: "typescript",
        syntax_key: "typescript",
        filenames: &[],
        extensions: &["ts", "mts", "cts"],
        compound_extensions: &[],
    },
    LanguageSpec {
        language_id: "tsx",
        syntax_key: "tsx",
        filenames: &[],
        extensions: &["tsx"],
        compound_extensions: &["test.tsx", "spec.tsx"],
    },
    LanguageSpec {
        language_id: "markdown",
        syntax_key: "markdown",
        filenames: &[],
        extensions: &["md", "markdown"],
        compound_extensions: &[],
    },
    LanguageSpec {
        language_id: "go",
        syntax_key: "go",
        filenames: &[],
        extensions: &["go"],
        compound_extensions: &[],
    },
    LanguageSpec {
        language_id: "bash",
        syntax_key: "bash",
        filenames: &[".bashrc", ".bash_profile", ".zshrc"],
        extensions: &["sh", "bash", "zsh"],
        compound_extensions: &[],
    },
    LanguageSpec {
        language_id: "python",
        syntax_key: "python",
        filenames: &[],
        extensions: &["py", "pyw"],
        compound_extensions: &[],
    },
    LanguageSpec {
        language_id: "ruby",
        syntax_key: "ruby",
        filenames: &["gemfile", "rakefile"],
        extensions: &["rb"],
        compound_extensions: &[],
    },
    LanguageSpec {
        language_id: "lua",
        syntax_key: "lua",
        filenames: &[],
        extensions: &["lua"],
        compound_extensions: &[],
    },
];

impl TreeSitterSyntax {
    fn new(
        name: &'static str,
        language: Language,
        queries: queries::CompiledQueries,
        parser_runtime: ParserRuntime,
        parser_identity: ParserCacheIdentity,
    ) -> Self {
        Self {
            name,
            language,
            queries,
            parser_runtime,
            parser_identity,
        }
    }

    pub(super) fn name(&self) -> &'static str {
        self.name
    }

    pub(super) fn language(&self) -> &Language {
        &self.language
    }

    pub(super) fn highlights_query(&self) -> Option<&tree_sitter::Query> {
        self.queries.highlights()
    }

    pub(super) fn injections_query(&self) -> Option<&tree_sitter::Query> {
        self.queries.injections()
    }

    pub(super) fn locals_query(&self) -> Option<&tree_sitter::Query> {
        self.queries.locals()
    }

    pub(super) fn parser_runtime(&self) -> ParserRuntime {
        self.parser_runtime
    }

    pub(super) fn parser_identity(&self) -> &ParserCacheIdentity {
        &self.parser_identity
    }

    pub(super) fn query_identity(&self) -> queries::QuerySetIdentity {
        self.queries.identity()
    }

    pub(super) fn cache_identity(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.name.hash(&mut hasher);
        self.parser_identity.hash(&mut hasher);
        self.query_identity().hash(&mut hasher);
        hasher.finish()
    }
}

fn built_in_query_sources() -> HashMap<String, queries::QuerySources> {
    HashMap::new()
}

fn build_syntaxes() -> HashMap<&'static str, Arc<TreeSitterSyntax>> {
    HashMap::new()
}

fn registry_cache_identity(syntaxes: &HashMap<&'static str, Arc<TreeSitterSyntax>>) -> u64 {
    let mut entries = syntaxes.iter().collect::<Vec<_>>();
    entries.sort_unstable_by_key(|(key, _)| **key);

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for (key, syntax) in entries {
        key.hash(&mut hasher);
        syntax.cache_identity().hash(&mut hasher);
    }
    hasher.finish()
}

fn matches_compound_extension(filename: &str, compound: &str) -> bool {
    filename == compound
        || filename
            .strip_suffix(compound)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn app_version() -> Version {
        Version::parse("0.1.0").unwrap()
    }

    fn registry_entry(language: &str) -> GrammarRegistryEntry {
        GrammarRegistryEntry {
            language: language.to_string(),
            version: Version::new(1, 2, 3),
            parser_abi: LANGUAGE_VERSION,
            app_version_req: Some(VersionReq::parse(">=0.1.0").unwrap()),
            runtime: Some(GrammarRuntime::Native),
            platforms: vec![parser_loading::current_platform().to_string()],
            filetypes: vec![language.to_string()],
            extensions: vec![language.to_string()],
            filenames: Vec::new(),
            first_line_regex: None,
            content_regex: None,
            packages: Vec::new(),
        }
    }

    fn write_registry(runtime_dir: &Path, entries: Vec<GrammarRegistryEntry>) {
        let registry_dir = runtime_dir.join("registry");
        fs::create_dir_all(&registry_dir).unwrap();
        let registry = GrammarRegistryFile {
            schema_version: REGISTRY_SCHEMA_VERSION,
            cache: RegistryCacheMetadata::new(100, 60),
            grammars: entries,
        };
        fs::write(
            registry_dir.join("registry.json"),
            serde_json::to_string_pretty(&registry).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn bootstrap_npm_wasm_specs_match_registry_contract() {
        let registry = bootstrap_runtime_registry();
        assert_eq!(registry.grammars.len(), BOOTSTRAP_NPM_WASM_GRAMMARS.len());

        let mut seen = BTreeSet::new();
        for spec in BOOTSTRAP_NPM_WASM_GRAMMARS {
            assert!(
                seen.insert(normalize_language_key(spec.language)),
                "duplicate bootstrap grammar language: {}",
                spec.language
            );
            assert!(
                !spec.filetypes.is_empty()
                    || !spec.extensions.is_empty()
                    || !spec.filenames.is_empty(),
                "{} must have at least one detection pattern",
                spec.language
            );
            assert!(
                spec.url.starts_with("https://registry.npmjs.org/") && spec.url.ends_with(".tgz"),
                "{} must use an npm tarball URL",
                spec.language
            );
            assert!(
                spec.url.contains(&format!("-{}.tgz", spec.version())),
                "{} URL should match its declared version",
                spec.language
            );

            let entry = registry
                .grammars
                .iter()
                .find(|entry| entry.language == spec.language)
                .unwrap_or_else(|| panic!("missing bootstrap grammar: {}", spec.language));
            assert_eq!(entry.version, spec.version(), "{}", spec.language);
            assert_eq!(entry.parser_abi, LANGUAGE_VERSION, "{}", spec.language);
            assert_eq!(
                entry.runtime,
                Some(GrammarRuntime::Wasm),
                "{}",
                spec.language
            );
            assert_eq!(entry.platforms, ["wasm"], "{}", spec.language);
            assert_eq!(entry.app_version_req, None, "{}", spec.language);
            assert_eq!(
                entry.filetypes,
                spec.filetypes
                    .iter()
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>(),
                "{}",
                spec.language
            );
            assert_eq!(
                entry.extensions,
                spec.extensions
                    .iter()
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>(),
                "{}",
                spec.language
            );
            assert_eq!(
                entry.filenames,
                spec.filenames
                    .iter()
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>(),
                "{}",
                spec.language
            );
            let [package] = entry.packages.as_slice() else {
                panic!("{} must have exactly one package", spec.language);
            };
            assert_eq!(package.url, spec.url, "{}", spec.language);
            assert_eq!(
                package.source,
                GrammarPackageSource::Community,
                "{}",
                spec.language
            );
            assert_eq!(
                package.runtime,
                Some(GrammarRuntime::Wasm),
                "{}",
                spec.language
            );
            assert_eq!(
                package.platform.as_deref(),
                Some("wasm"),
                "{}",
                spec.language
            );
            assert_eq!(package.sha256, None, "{}", spec.language);
            assert_eq!(package.signature, None, "{}", spec.language);
        }
    }

    #[test]
    fn bootstrap_npm_wasm_specs_are_known_detection_languages() {
        let registry = BuiltinSyntaxRegistry::new();
        for spec in BOOTSTRAP_NPM_WASM_GRAMMARS {
            if spec.language == "php-only" {
                continue;
            }

            for extension in spec.extensions {
                assert_detection_match(
                    registry.language_for_extension(extension),
                    spec.language,
                    extension,
                );
            }
            for filetype in spec.filetypes {
                if filetype.contains('.') {
                    let filename = format!("sample.{filetype}");
                    assert_detection_match(
                        registry.language_for_compound_extension(&filename),
                        spec.language,
                        filetype,
                    );
                } else {
                    assert_detection_match(
                        registry.language_for_extension(filetype),
                        spec.language,
                        filetype,
                    );
                }
            }
            for filename in spec.filenames {
                assert_detection_match(
                    registry.language_for_filename(filename),
                    spec.language,
                    filename,
                );
            }
        }
    }

    fn assert_detection_match(
        language_match: Option<LanguageMatch>,
        expected_syntax_key: &str,
        pattern: &str,
    ) {
        let language_match = language_match.unwrap_or_else(|| {
            panic!("missing detection pattern: {expected_syntax_key} {pattern}")
        });
        assert_eq!(
            language_match.syntax_key, expected_syntax_key,
            "{expected_syntax_key} detection pattern {pattern}",
        );
    }

    #[test]
    fn cpp_runtime_query_inherits_c_when_package_only_has_cpp_layer() {
        let source = "(qualified_identifier name: (identifier) @function)\n";

        let expanded = ensure_cpp_query_has_c_base(source);

        assert!(expanded.starts_with("; inherits: c\n"));
        assert!(expanded.contains(source));
    }

    #[test]
    fn cpp_runtime_query_keeps_decoded_c_base_without_inheritance_modeline() {
        let source = "(preproc_directive) @keyword\n(primitive_type) @type\n";

        assert_eq!(ensure_cpp_query_has_c_base(source), source);
    }

    fn write_manifest(
        runtime_dir: &Path,
        language: &str,
        parser_abi: usize,
        app_version_req: Option<&str>,
    ) {
        let package_dir = runtime_dir.join("parsers").join(language);
        let parser_path = format!(
            "parser/{}/{language}.so",
            parser_loading::current_platform()
        );
        let highlights_path = "queries/highlights.scm".to_string();
        fs::create_dir_all(
            package_dir
                .join("parser")
                .join(parser_loading::current_platform()),
        )
        .unwrap();
        fs::create_dir_all(package_dir.join("queries")).unwrap();
        fs::write(package_dir.join(&parser_path), "not a real shared library").unwrap();
        fs::write(
            package_dir.join(&highlights_path),
            "(identifier) @variable\n",
        )
        .unwrap();
        let mut sha256 = BTreeMap::new();
        sha256.insert(
            parser_path.clone(),
            sha256_hex(&fs::read(package_dir.join(&parser_path)).unwrap()),
        );
        sha256.insert(
            highlights_path.clone(),
            sha256_hex(&fs::read(package_dir.join(&highlights_path)).unwrap()),
        );
        let manifest = GrammarPackageManifest {
            language: language.to_string(),
            version: Version::new(1, 2, 3),
            parser_abi,
            runtime: GrammarRuntime::Native,
            platform: parser_loading::current_platform().to_string(),
            source: GrammarPackageSource::Official,
            source_url: Some("https://example.invalid/test.pkg".to_string()),
            signature: Some("test-signature".to_string()),
            files: GrammarPackageFiles {
                parser: Some(parser_path),
                wasm: None,
                highlights: Some(highlights_path),
                injections: None,
                locals: None,
            },
            filetypes: vec![language.to_string()],
            extensions: vec![language.to_string()],
            filenames: Vec::new(),
            first_line_regex: None,
            content_regex: None,
            app_version_req: app_version_req.map(|req| VersionReq::parse(req).unwrap()),
            sha256,
        };
        fs::write(
            package_dir.join(PACKAGE_MANIFEST_FILENAME),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }

    fn write_wasm_manifest(runtime_dir: &Path, language: &str) {
        let package_dir = runtime_dir.join("parsers").join(language);
        let wasm_path = format!("wasm/{language}.wasm");
        let highlights_path = "queries/highlights.scm".to_string();
        fs::create_dir_all(package_dir.join("wasm")).unwrap();
        fs::create_dir_all(package_dir.join("queries")).unwrap();
        fs::write(package_dir.join(&wasm_path), "not wasm").unwrap();
        fs::write(package_dir.join(&highlights_path), "").unwrap();
        let mut sha256 = BTreeMap::new();
        sha256.insert(
            wasm_path.clone(),
            sha256_hex(&fs::read(package_dir.join(&wasm_path)).unwrap()),
        );
        sha256.insert(
            highlights_path.clone(),
            sha256_hex(&fs::read(package_dir.join(&highlights_path)).unwrap()),
        );
        let manifest = GrammarPackageManifest {
            language: language.to_string(),
            version: Version::new(1, 2, 3),
            parser_abi: LANGUAGE_VERSION,
            runtime: GrammarRuntime::Wasm,
            platform: "wasm".to_string(),
            source: GrammarPackageSource::Community,
            source_url: None,
            signature: None,
            files: GrammarPackageFiles {
                parser: None,
                wasm: Some(wasm_path),
                highlights: Some(highlights_path),
                injections: None,
                locals: None,
            },
            filetypes: vec![language.to_string()],
            extensions: vec![language.to_string()],
            filenames: Vec::new(),
            first_line_regex: None,
            content_regex: None,
            app_version_req: Some(VersionReq::parse(">=0.1.0").unwrap()),
            sha256,
        };
        fs::write(
            package_dir.join(PACKAGE_MANIFEST_FILENAME),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }

    fn mark_manifest_community(runtime_dir: &Path, language: &str) {
        let manifest_path = runtime_dir
            .join("parsers")
            .join(language)
            .join(PACKAGE_MANIFEST_FILENAME);
        let mut manifest = serde_json::from_str::<GrammarPackageManifest>(
            &fs::read_to_string(&manifest_path).unwrap(),
        )
        .unwrap();
        manifest.source = GrammarPackageSource::Community;
        manifest.source_url = None;
        manifest.signature = None;
        fs::write(
            manifest_path,
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn registry_cache_metadata_tracks_expiration() {
        let cache = RegistryCacheMetadata::new(100, 60);

        assert_eq!(cache.expires_at_unix_seconds(), Some(160));
        assert!(cache.is_fresh_at(159));
        assert!(cache.is_expired_at(160));
    }

    #[test]
    fn reads_local_registry_file() {
        let tmp = tempdir().unwrap();
        write_registry(tmp.path(), vec![registry_entry("go")]);

        let read = read_local_registry(tmp.path());
        let registry = read.registry.unwrap();

        assert!(read.errors.is_empty());
        assert_eq!(registry.cache.expires_at_unix_seconds(), Some(160));
        assert_eq!(registry.grammars[0].language, "go");
    }

    #[test]
    fn corrupt_registry_returns_recoverable_error() {
        let tmp = tempdir().unwrap();
        let registry_dir = tmp.path().join("registry");
        fs::create_dir_all(&registry_dir).unwrap();
        fs::write(registry_dir.join("registry.json"), "{ nope").unwrap();

        let builtins = BuiltinSyntaxRegistry::new();
        let report = list_grammar_inventory_for_app_version(
            tmp.path(),
            &builtins,
            &app_version(),
            &RuntimeGrammarSecurityPolicy {
                native_package_source_allowlist: vec!["https://example.invalid/".to_string()],
                ..RuntimeGrammarSecurityPolicy::default()
            },
        );

        assert!(report.built_in.is_empty());
        assert!(report
            .missing
            .iter()
            .any(|missing| missing.entry.language == "go"));
        assert!(report
            .missing
            .iter()
            .any(|missing| missing.entry.language == "twig"));
        assert_eq!(
            report.errors[0].kind,
            GrammarRegistryErrorKind::ParseRegistry
        );
    }

    #[test]
    fn scanner_ignores_corrupt_manifests() {
        let tmp = tempdir().unwrap();
        write_manifest(tmp.path(), "twig", LANGUAGE_VERSION, Some(">=0.1.0"));
        let broken_dir = tmp.path().join("parsers").join("broken");
        fs::create_dir_all(&broken_dir).unwrap();
        fs::write(broken_dir.join(PACKAGE_MANIFEST_FILENAME), "{ nope").unwrap();

        let scan = scan_installed_packages(tmp.path(), &app_version());

        assert_eq!(scan.packages.len(), 1);
        assert_eq!(scan.packages[0].manifest.language, "twig");
        assert!(scan.packages[0].compatibility.is_compatible());
        assert_eq!(scan.errors.len(), 1);
        assert_eq!(scan.errors[0].kind, GrammarRegistryErrorKind::ParseManifest);
    }

    #[test]
    fn scanner_recognizes_wasm_package_manifests() {
        let tmp = tempdir().unwrap();
        write_wasm_manifest(tmp.path(), "zig");

        let scan = scan_installed_packages(tmp.path(), &app_version());

        assert_eq!(scan.packages.len(), 1);
        assert_eq!(scan.packages[0].manifest.runtime, GrammarRuntime::Wasm);
        assert_eq!(
            scan.packages[0].manifest.files.wasm.as_deref(),
            Some("wasm/zig.wasm")
        );
        assert!(scan.packages[0].manifest.files.parser.is_none());
        assert!(scan.packages[0].compatibility.is_compatible());
    }

    #[test]
    fn runtime_query_loader_reads_manifest_query_assets() {
        let tmp = tempdir().unwrap();
        write_manifest(tmp.path(), "querylang", LANGUAGE_VERSION, Some(">=0.1.0"));
        let package_dir = tmp.path().join("parsers").join("querylang");
        let query_dir = package_dir.join("queries");
        fs::create_dir_all(&query_dir).unwrap();
        fs::write(query_dir.join("highlights.scm"), "(identifier) @variable\n").unwrap();
        fs::write(
            query_dir.join("injections.scm"),
            "(string_literal) @injection.content\n",
        )
        .unwrap();
        fs::write(
            query_dir.join("locals.scm"),
            "(identifier) @local.definition\n",
        )
        .unwrap();

        let mut manifest = serde_json::from_str::<GrammarPackageManifest>(
            &fs::read_to_string(package_dir.join(PACKAGE_MANIFEST_FILENAME)).unwrap(),
        )
        .unwrap();
        manifest.files.injections = Some("queries/injections.scm".to_string());
        manifest.files.locals = Some("queries/locals.scm".to_string());
        manifest.sha256.insert(
            "queries/injections.scm".to_string(),
            sha256_hex(&fs::read(query_dir.join("injections.scm")).unwrap()),
        );
        manifest.sha256.insert(
            "queries/locals.scm".to_string(),
            sha256_hex(&fs::read(query_dir.join("locals.scm")).unwrap()),
        );
        fs::write(
            package_dir.join(PACKAGE_MANIFEST_FILENAME),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        let scan = scan_installed_packages(tmp.path(), &app_version());
        let registry = BuiltinSyntaxRegistry::new();

        let sources = registry
            .load_runtime_query_sources(&scan.packages[0])
            .unwrap();

        assert_eq!(
            sources.get(queries::QueryType::Highlights),
            Some("(identifier) @variable\n")
        );
        assert_eq!(
            sources.get(queries::QueryType::Injections),
            Some("(string_literal) @injection.content\n")
        );
        assert_eq!(
            sources.get(queries::QueryType::Locals),
            Some("(identifier) @local.definition\n")
        );
    }

    #[test]
    fn scanner_rejects_installed_package_with_tampered_file() {
        let tmp = tempdir().unwrap();
        write_manifest(tmp.path(), "zig", LANGUAGE_VERSION, Some(">=0.1.0"));
        fs::write(
            tmp.path()
                .join("parsers")
                .join("zig")
                .join("queries")
                .join("highlights.scm"),
            "tampered",
        )
        .unwrap();

        let scan = scan_installed_packages(tmp.path(), &app_version());

        assert!(scan.packages.is_empty());
        assert_eq!(scan.errors.len(), 1);
        assert_eq!(
            scan.errors[0].kind,
            GrammarRegistryErrorKind::FileHashMismatch
        );
    }

    #[test]
    fn scanner_rejects_manifest_paths_outside_package() {
        let tmp = tempdir().unwrap();
        write_manifest(tmp.path(), "zig", LANGUAGE_VERSION, Some(">=0.1.0"));
        let manifest_path = tmp
            .path()
            .join("parsers")
            .join("zig")
            .join(PACKAGE_MANIFEST_FILENAME);
        let mut manifest = serde_json::from_str::<GrammarPackageManifest>(
            &fs::read_to_string(&manifest_path).unwrap(),
        )
        .unwrap();
        manifest.files.highlights = Some("../highlights.scm".to_string());
        fs::write(
            &manifest_path,
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let scan = scan_installed_packages(tmp.path(), &app_version());

        assert!(scan.packages.is_empty());
        assert_eq!(scan.errors.len(), 1);
        assert_eq!(
            scan.errors[0].kind,
            GrammarRegistryErrorKind::InvalidPackagePath
        );
    }

    #[test]
    fn inventory_lists_installed_and_missing_grammars() {
        let tmp = tempdir().unwrap();
        write_registry(
            tmp.path(),
            vec![
                registry_entry("twig"),
                registry_entry("go"),
                registry_entry("rust"),
            ],
        );
        write_manifest(tmp.path(), "twig", LANGUAGE_VERSION, Some(">=0.1.0"));

        let builtins = BuiltinSyntaxRegistry::new();
        let report = list_grammar_inventory_for_app_version(
            tmp.path(),
            &builtins,
            &app_version(),
            &RuntimeGrammarSecurityPolicy {
                native_package_source_allowlist: vec!["https://example.invalid/".to_string()],
                ..RuntimeGrammarSecurityPolicy::default()
            },
        );

        assert!(report.built_in.is_empty());
        assert!(report
            .installed
            .iter()
            .any(|package| package.manifest.language == "twig"));
        assert!(report
            .missing
            .iter()
            .any(|missing| missing.entry.language == "go"));
        assert!(report
            .missing
            .iter()
            .any(|missing| missing.entry.language == "rust"));
        assert!(!report
            .missing
            .iter()
            .any(|missing| missing.entry.language == "twig"));
        assert!(report.errors.is_empty());
    }

    #[test]
    fn unloadable_wasm_package_does_not_satisfy_registry_with_wasm_feature() {
        let tmp = tempdir().unwrap();
        write_registry(tmp.path(), vec![registry_entry("zig")]);
        write_wasm_manifest(tmp.path(), "zig");

        let builtins = BuiltinSyntaxRegistry::new();
        let report = list_grammar_inventory_for_app_version(
            tmp.path(),
            &builtins,
            &app_version(),
            &RuntimeGrammarSecurityPolicy::default(),
        );

        assert!(report
            .installed
            .iter()
            .any(|package| package.manifest.language == "zig"));
        assert!(report
            .missing
            .iter()
            .any(|missing| missing.entry.language == "zig"));
    }

    #[test]
    fn incompatible_installed_package_does_not_satisfy_registry_entry() {
        let tmp = tempdir().unwrap();
        write_registry(tmp.path(), vec![registry_entry("go")]);
        write_manifest(tmp.path(), "go", LANGUAGE_VERSION, Some(">=9.0.0"));

        let builtins = BuiltinSyntaxRegistry::new();
        let report = list_grammar_inventory_for_app_version(
            tmp.path(),
            &builtins,
            &app_version(),
            &RuntimeGrammarSecurityPolicy::default(),
        );

        assert!(!report.installed[0].compatibility.is_compatible());
        assert!(matches!(
            report.installed[0].compatibility.app_version,
            AppVersionCompatibility::Incompatible { .. }
        ));
        assert!(report
            .missing
            .iter()
            .any(|missing| missing.entry.language == "go"));
        assert!(report.errors.is_empty());
    }

    #[test]
    fn native_community_grammar_is_missing_until_policy_allows_it() {
        let tmp = tempdir().unwrap();
        write_registry(tmp.path(), vec![registry_entry("zig")]);
        write_manifest(tmp.path(), "zig", LANGUAGE_VERSION, Some(">=0.1.0"));
        mark_manifest_community(tmp.path(), "zig");
        let builtins = BuiltinSyntaxRegistry::new();

        let default_report = list_grammar_inventory_for_app_version(
            tmp.path(),
            &builtins,
            &app_version(),
            &RuntimeGrammarSecurityPolicy::default(),
        );
        let community_report = list_grammar_inventory_for_app_version(
            tmp.path(),
            &builtins,
            &app_version(),
            &RuntimeGrammarSecurityPolicy {
                allow_native_community_grammars: true,
                ..RuntimeGrammarSecurityPolicy::default()
            },
        );

        assert!(default_report
            .missing
            .iter()
            .any(|missing| missing.entry.language == "zig"));
        assert!(!community_report
            .missing
            .iter()
            .any(|missing| missing.entry.language == "zig"));
    }

    #[test]
    fn parser_abi_compatibility_uses_tree_sitter_supported_range() {
        assert!(ParserAbiCompatibility::check(LANGUAGE_VERSION).is_compatible());
        assert!(ParserAbiCompatibility::check(MIN_COMPATIBLE_LANGUAGE_VERSION).is_compatible());
        assert!(!ParserAbiCompatibility::check(LANGUAGE_VERSION + 1).is_compatible());
    }
}
