use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use flate2::read::GzDecoder;
use semver::Version;
use serde::{Deserialize, Serialize};
use tree_sitter::LANGUAGE_VERSION;

use super::parser_loading;
use super::registry::{
    current_app_version, installed_package_can_load, read_local_registry, read_runtime_registry,
    scan_installed_packages, GrammarCompatibility, GrammarPackageDownload, GrammarPackageFiles,
    GrammarPackageManifest, GrammarPackageSource, GrammarRegistryEntry, GrammarRegistryFile,
    GrammarRuntime, InstalledGrammarPackage, RegistryCacheMetadata, RuntimeGrammarSecurityPolicy,
    PACKAGE_MANIFEST_FILENAME, REGISTRY_CACHE_TTL_SECONDS, REGISTRY_JSON_PATH,
    REGISTRY_SCHEMA_VERSION,
};
use super::util::{current_unix_seconds, normalize_language_key, sha256_hex};

pub const GRAMMAR_STATUS_SCHEMA_VERSION: u32 = 1;
pub const GRAMMAR_STATUS_PATH: &str = "registry/install-status.json";

const PARSERS_DIR: &str = "parsers";
const STAGING_DIR: &str = "staging";

static INSTALL_NONCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryFetchSource {
    pub url: String,
    pub signature: Option<String>,
}

impl RegistryFetchSource {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            signature: None,
        }
    }
}

pub trait GrammarRegistryFetcher {
    fn fetch_registry(&self, source: &RegistryFetchSource) -> Result<Vec<u8>, String>;
}

pub trait GrammarRegistryVerifier {
    fn verify_registry(&self, source: &RegistryFetchSource, bytes: &[u8]) -> Result<(), String>;
}

#[derive(Debug, Default)]
pub struct AcceptingRegistryVerifier;

impl GrammarRegistryVerifier for AcceptingRegistryVerifier {
    fn verify_registry(&self, _source: &RegistryFetchSource, _bytes: &[u8]) -> Result<(), String> {
        Ok(())
    }
}

pub trait GrammarPackageDownloader {
    fn download_package(&self, source: &GrammarPackageDownload) -> Result<Vec<u8>, String>;
}

pub trait GrammarPackageDecoder {
    fn decode_package(
        &self,
        bytes: &[u8],
        staging_dir: &Path,
        target_language: &str,
    ) -> Result<(), String>;
}

#[derive(Debug, Default)]
pub struct LocalGrammarTransport;

impl GrammarRegistryFetcher for LocalGrammarTransport {
    fn fetch_registry(&self, source: &RegistryFetchSource) -> Result<Vec<u8>, String> {
        fs::read(local_path_from_url(&source.url)?).map_err(|err| err.to_string())
    }
}

#[derive(Debug, Default)]
pub struct RuntimeGrammarTransport;

impl GrammarRegistryFetcher for RuntimeGrammarTransport {
    fn fetch_registry(&self, source: &RegistryFetchSource) -> Result<Vec<u8>, String> {
        fetch_runtime_bytes(&source.url)
    }
}

impl GrammarPackageDownloader for RuntimeGrammarTransport {
    fn download_package(&self, source: &GrammarPackageDownload) -> Result<Vec<u8>, String> {
        fetch_runtime_bytes(&source.url)
    }
}

fn fetch_runtime_bytes(url: &str) -> Result<Vec<u8>, String> {
    if url.starts_with("http://") || url.starts_with("https://") {
        return fetch_via_curl(url);
    }

    let path = local_path_from_url(url)?;
    if path.is_dir() {
        return Ok(path.to_string_lossy().as_bytes().to_vec());
    }
    fs::read(&path).map_err(|err| err.to_string())
}

fn fetch_via_curl(url: &str) -> Result<Vec<u8>, String> {
    use std::process::Command;

    const MAX_SECONDS: &str = "60";

    let mut command = Command::new("curl");
    let output = crate::utils::configure_background_command(&mut command)
        .arg("-sSLf")
        .arg("--max-time")
        .arg(MAX_SECONDS)
        .arg("--")
        .arg(url)
        .output()
        .map_err(|err| {
            if err.kind() == ErrorKind::NotFound {
                "curl is required to download grammars but was not found on PATH".to_string()
            } else {
                format!("failed to invoke curl: {err}")
            }
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        let detail = if stderr.is_empty() {
            match output.status.code() {
                Some(code) => format!("exit status {code}"),
                None => "terminated by signal".to_string(),
            }
        } else {
            stderr.to_string()
        };
        return Err(format!("curl failed for {url}: {detail}"));
    }

    Ok(output.stdout)
}

impl GrammarPackageDownloader for LocalGrammarTransport {
    fn download_package(&self, source: &GrammarPackageDownload) -> Result<Vec<u8>, String> {
        let path = local_path_from_url(&source.url)?;
        if !path.exists() {
            return Err(format!(
                "grammar package path does not exist: {}",
                path.display()
            ));
        }
        Ok(path.to_string_lossy().as_bytes().to_vec())
    }
}

pub trait GrammarPackageVerifier {
    fn verify_package(&self, source: &GrammarPackageDownload, bytes: &[u8]) -> Result<(), String>;
}

#[derive(Debug, Default)]
pub struct CommunityOnlyPackageVerifier;

impl GrammarPackageVerifier for CommunityOnlyPackageVerifier {
    fn verify_package(&self, source: &GrammarPackageDownload, _bytes: &[u8]) -> Result<(), String> {
        match source.source {
            GrammarPackageSource::Official => {
                Err("official package verifier is not configured".to_string())
            }
            GrammarPackageSource::Community => Ok(()),
        }
    }
}

#[cfg(test)]
#[derive(Debug, Default)]
pub struct DirectoryPackageDecoder;

#[cfg(test)]
impl GrammarPackageDecoder for DirectoryPackageDecoder {
    fn decode_package(
        &self,
        bytes: &[u8],
        staging_dir: &Path,
        _target_language: &str,
    ) -> Result<(), String> {
        let source = std::str::from_utf8(bytes)
            .map_err(|err| format!("package source path is not UTF-8: {err}"))?;
        let source = Path::new(source.trim());
        copy_dir_all(source, staging_dir)
    }
}

#[derive(Debug, Default)]
pub struct RuntimePackageDecoder;

impl GrammarPackageDecoder for RuntimePackageDecoder {
    fn decode_package(
        &self,
        bytes: &[u8],
        staging_dir: &Path,
        target_language: &str,
    ) -> Result<(), String> {
        if let Ok(source) = std::str::from_utf8(bytes) {
            let source = Path::new(source.trim());
            if source.is_dir() {
                return copy_dir_all(source, staging_dir);
            }
            if source.is_file() {
                let bytes = fs::read(source).map_err(|err| err.to_string())?;
                return decode_npm_wasm_package(&bytes, staging_dir, target_language);
            }
        }
        decode_npm_wasm_package(bytes, staging_dir, target_language)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum GrammarInstallStatus {
    Missing,
    Queued,
    Installing,
    Installed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GrammarLanguageInstallStatus {
    pub language: String,
    pub status: GrammarInstallStatus,
    #[serde(default)]
    pub installed_version: Option<Version>,
    #[serde(default)]
    pub available_version: Option<Version>,
    #[serde(default)]
    pub last_error: Option<String>,
    pub updated_at_unix_seconds: u64,
}

impl GrammarLanguageInstallStatus {
    fn missing(language: String, available_version: Option<Version>) -> Self {
        Self {
            language,
            status: GrammarInstallStatus::Missing,
            installed_version: None,
            available_version,
            last_error: None,
            updated_at_unix_seconds: current_unix_seconds(),
        }
    }

    fn with_status(
        language: String,
        status: GrammarInstallStatus,
        installed_version: Option<Version>,
        available_version: Option<Version>,
        last_error: Option<String>,
    ) -> Self {
        Self {
            language,
            status,
            installed_version,
            available_version,
            last_error,
            updated_at_unix_seconds: current_unix_seconds(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryFetchOutcome {
    pub registry: GrammarRegistryFile,
    pub source: RegistryFetchOutcomeSource,
    pub fallback_error: Option<GrammarInstallError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryFetchOutcomeSource {
    FreshCache,
    Network,
    StaleFallback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrammarInstallError {
    pub kind: GrammarInstallErrorKind,
    pub language: Option<String>,
    pub path: Option<PathBuf>,
    pub message: String,
}

impl GrammarInstallError {
    pub fn runtime_directory_unavailable(message: impl Into<String>) -> Self {
        Self::new(
            GrammarInstallErrorKind::RuntimeDirectoryUnavailable,
            None,
            None,
            message,
        )
    }

    fn new(
        kind: GrammarInstallErrorKind,
        language: Option<String>,
        path: Option<PathBuf>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            language,
            path,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrammarInstallErrorKind {
    RuntimeDirectoryUnavailable,
    RuntimeDownloadsDisabled,
    FetchRegistry,
    VerifyRegistry,
    ParseRegistry,
    WriteRegistry,
    NoRegistryEntry,
    IncompatibleGrammar,
    NoPackageSource,
    DownloadPackage,
    PackageHashMismatch,
    VerifyPackage,
    UnsignedOfficialPackage,
    DecodePackage,
    ReadManifest,
    ParseManifest,
    ManifestMismatch,
    InvalidPackagePath,
    MissingPackageFile,
    MissingFileHash,
    FileHashMismatch,
    DisallowedNativePackageSource,
    AtomicInstall,
    RemoveInstalled,
    AlreadyInstalling,
    NotQueued,
}

#[derive(Debug, Clone)]
pub struct GrammarInstallationService {
    runtime_dir: PathBuf,
    policy: RuntimeGrammarSecurityPolicy,
}

impl GrammarInstallationService {
    pub fn new(runtime_dir: impl Into<PathBuf>) -> Self {
        Self::with_policy(runtime_dir, RuntimeGrammarSecurityPolicy::from_env())
    }

    pub fn with_policy(
        runtime_dir: impl Into<PathBuf>,
        policy: RuntimeGrammarSecurityPolicy,
    ) -> Self {
        Self {
            runtime_dir: runtime_dir.into(),
            policy,
        }
    }

    pub fn refresh_registry(
        &self,
        source: &RegistryFetchSource,
        fetcher: &impl GrammarRegistryFetcher,
        verifier: &impl GrammarRegistryVerifier,
        force: bool,
    ) -> Result<RegistryFetchOutcome, GrammarInstallError> {
        let local = read_local_registry(&self.runtime_dir);
        if !self.policy.runtime_downloads_enabled {
            return match local.registry {
                Some(registry) => Ok(RegistryFetchOutcome {
                    registry,
                    source: RegistryFetchOutcomeSource::StaleFallback,
                    fallback_error: None,
                }),
                None => Err(GrammarInstallError::new(
                    GrammarInstallErrorKind::RuntimeDownloadsDisabled,
                    None,
                    None,
                    "runtime grammar downloads are disabled",
                )),
            };
        }
        if !force {
            if let Some(registry) = local.registry.as_ref() {
                if registry.cache.is_fresh_at(current_unix_seconds()) {
                    return Ok(RegistryFetchOutcome {
                        registry: registry.clone(),
                        source: RegistryFetchOutcomeSource::FreshCache,
                        fallback_error: None,
                    });
                }
            }
        }

        match self.fetch_registry_from_source(source, fetcher, verifier) {
            Ok(registry) => Ok(RegistryFetchOutcome {
                registry,
                source: RegistryFetchOutcomeSource::Network,
                fallback_error: None,
            }),
            Err(err) => match local.registry {
                Some(registry) => Ok(RegistryFetchOutcome {
                    registry,
                    source: RegistryFetchOutcomeSource::StaleFallback,
                    fallback_error: Some(err),
                }),
                None => Err(err),
            },
        }
    }

    pub fn status_for_language(&self, language: &str) -> GrammarLanguageInstallStatus {
        let language_key = normalize_language_key(language);
        let available_version = self
            .registry_entry_for_language(&language_key)
            .map(|entry| entry.version);
        if let Some(package) = self.installed_package_for_language(&language_key) {
            return GrammarLanguageInstallStatus::with_status(
                language_key,
                GrammarInstallStatus::Installed,
                Some(package.manifest.version),
                available_version,
                None,
            );
        }

        let cached = read_status_cache(&self.runtime_dir)
            .languages
            .remove(&language_key)
            .unwrap_or_else(|| {
                GrammarLanguageInstallStatus::missing(language_key, available_version)
            });
        if cached.status == GrammarInstallStatus::Installed {
            GrammarLanguageInstallStatus::missing(cached.language, cached.available_version)
        } else {
            cached
        }
    }

    pub fn queue_install_for_language(&self, language: &str) -> GrammarLanguageInstallStatus {
        let language_key = normalize_language_key(language);
        let current = self.status_for_language(&language_key);
        match current.status {
            GrammarInstallStatus::Installed
            | GrammarInstallStatus::Queued
            | GrammarInstallStatus::Installing
            | GrammarInstallStatus::Failed => return current,
            GrammarInstallStatus::Missing => {}
        }

        let Some(entry) = self.registry_entry_for_language(&language_key) else {
            return current;
        };
        if !self.policy.runtime_downloads_enabled
            || !entry_is_installable(&entry)
            || select_package_source(&entry, &self.policy).is_none()
        {
            return current;
        }

        let status = GrammarLanguageInstallStatus::with_status(
            language_key,
            GrammarInstallStatus::Queued,
            None,
            Some(entry.version),
            None,
        );
        self.persist_status(status)
    }

    pub fn install_queued_grammar(
        &self,
        language: &str,
        downloader: &impl GrammarPackageDownloader,
        decoder: &impl GrammarPackageDecoder,
    ) -> Result<GrammarLanguageInstallStatus, GrammarInstallError> {
        self.install_queued_grammar_verified(
            language,
            downloader,
            decoder,
            &CommunityOnlyPackageVerifier,
        )
    }

    pub fn install_queued_grammar_verified(
        &self,
        language: &str,
        downloader: &impl GrammarPackageDownloader,
        decoder: &impl GrammarPackageDecoder,
        verifier: &impl GrammarPackageVerifier,
    ) -> Result<GrammarLanguageInstallStatus, GrammarInstallError> {
        let current = self.status_for_language(language);
        match current.status {
            GrammarInstallStatus::Installed => return Ok(current),
            GrammarInstallStatus::Queued => {}
            GrammarInstallStatus::Installing => {
                return Err(GrammarInstallError::new(
                    GrammarInstallErrorKind::AlreadyInstalling,
                    Some(normalize_language_key(language)),
                    None,
                    "grammar installation is already in progress",
                ));
            }
            GrammarInstallStatus::Missing | GrammarInstallStatus::Failed => {
                return Err(GrammarInstallError::new(
                    GrammarInstallErrorKind::NotQueued,
                    Some(normalize_language_key(language)),
                    None,
                    "grammar is not queued for installation",
                ));
            }
        }
        self.install_language(language, downloader, decoder, verifier)
    }

    pub fn update_grammar(
        &self,
        language: &str,
        downloader: &impl GrammarPackageDownloader,
        decoder: &impl GrammarPackageDecoder,
    ) -> Result<GrammarLanguageInstallStatus, GrammarInstallError> {
        self.update_grammar_verified(language, downloader, decoder, &CommunityOnlyPackageVerifier)
    }

    pub fn update_grammar_verified(
        &self,
        language: &str,
        downloader: &impl GrammarPackageDownloader,
        decoder: &impl GrammarPackageDecoder,
        verifier: &impl GrammarPackageVerifier,
    ) -> Result<GrammarLanguageInstallStatus, GrammarInstallError> {
        let current = self.status_for_language(language);
        if current.status == GrammarInstallStatus::Installing {
            return Err(GrammarInstallError::new(
                GrammarInstallErrorKind::AlreadyInstalling,
                Some(normalize_language_key(language)),
                None,
                "grammar installation is already in progress",
            ));
        }
        self.install_language(language, downloader, decoder, verifier)
    }

    pub fn uninstall_grammar(
        &self,
        language: &str,
    ) -> Result<GrammarLanguageInstallStatus, GrammarInstallError> {
        let language_key = normalize_language_key(language);
        let target = self.runtime_dir.join(PARSERS_DIR).join(&language_key);
        if target.exists() {
            fs::remove_dir_all(&target).map_err(|err| {
                GrammarInstallError::new(
                    GrammarInstallErrorKind::RemoveInstalled,
                    Some(language_key.clone()),
                    Some(target.clone()),
                    err.to_string(),
                )
            })?;
        }
        let available_version = self
            .registry_entry_for_language(&language_key)
            .map(|entry| entry.version);
        Ok(self.persist_status(GrammarLanguageInstallStatus::missing(
            language_key,
            available_version,
        )))
    }

    fn fetch_registry_from_source(
        &self,
        source: &RegistryFetchSource,
        fetcher: &impl GrammarRegistryFetcher,
        verifier: &impl GrammarRegistryVerifier,
    ) -> Result<GrammarRegistryFile, GrammarInstallError> {
        let bytes = fetcher.fetch_registry(source).map_err(|err| {
            GrammarInstallError::new(GrammarInstallErrorKind::FetchRegistry, None, None, err)
        })?;
        verifier.verify_registry(source, &bytes).map_err(|err| {
            GrammarInstallError::new(GrammarInstallErrorKind::VerifyRegistry, None, None, err)
        })?;
        let mut registry =
            serde_json::from_slice::<GrammarRegistryFile>(&bytes).map_err(|err| {
                GrammarInstallError::new(
                    GrammarInstallErrorKind::ParseRegistry,
                    None,
                    None,
                    err.to_string(),
                )
            })?;
        if registry.schema_version != REGISTRY_SCHEMA_VERSION {
            return Err(GrammarInstallError::new(
                GrammarInstallErrorKind::ParseRegistry,
                None,
                None,
                format!("unsupported registry schema {}", registry.schema_version),
            ));
        }

        let ttl_seconds = if registry.cache.ttl_seconds == 0 {
            REGISTRY_CACHE_TTL_SECONDS
        } else {
            registry.cache.ttl_seconds
        };
        registry.cache = RegistryCacheMetadata::new(current_unix_seconds(), ttl_seconds);
        write_json_atomic(&self.runtime_dir.join(REGISTRY_JSON_PATH), &registry).map_err(
            |err| {
                GrammarInstallError::new(
                    GrammarInstallErrorKind::WriteRegistry,
                    None,
                    Some(self.runtime_dir.join(REGISTRY_JSON_PATH)),
                    err,
                )
            },
        )?;
        Ok(registry)
    }

    fn install_language(
        &self,
        language: &str,
        downloader: &impl GrammarPackageDownloader,
        decoder: &impl GrammarPackageDecoder,
        verifier: &impl GrammarPackageVerifier,
    ) -> Result<GrammarLanguageInstallStatus, GrammarInstallError> {
        let language_key = normalize_language_key(language);
        if !self.policy.runtime_downloads_enabled {
            return Err(self.fail_language(
                &language_key,
                None,
                GrammarInstallErrorKind::RuntimeDownloadsDisabled,
                "runtime grammar downloads are disabled",
            ));
        }
        let entry = self
            .registry_entry_for_language(&language_key)
            .ok_or_else(|| {
                GrammarInstallError::new(
                    GrammarInstallErrorKind::NoRegistryEntry,
                    Some(language_key.clone()),
                    None,
                    "grammar is not present in the registry cache",
                )
            })?;
        if !entry_is_installable(&entry) {
            return Err(self.fail_language(
                &language_key,
                Some(entry.version),
                GrammarInstallErrorKind::IncompatibleGrammar,
                "grammar is not compatible with this app or parser runtime",
            ));
        }
        if entry_has_disallowed_native_package(&entry, &self.policy) {
            return Err(self.fail_language(
                &language_key,
                Some(entry.version),
                GrammarInstallErrorKind::DisallowedNativePackageSource,
                "native grammar package source is not allowed",
            ));
        }
        let package_source = select_package_source(&entry, &self.policy)
            .cloned()
            .ok_or_else(|| {
                self.fail_language(
                    &language_key,
                    Some(entry.version.clone()),
                    GrammarInstallErrorKind::NoPackageSource,
                    "registry entry has no package for this runtime",
                )
            })?;

        self.persist_status(GrammarLanguageInstallStatus::with_status(
            language_key.clone(),
            GrammarInstallStatus::Installing,
            None,
            Some(entry.version.clone()),
            None,
        ));

        let bytes = match downloader.download_package(&package_source) {
            Ok(bytes) => bytes,
            Err(err) => {
                return Err(self.fail_language(
                    &language_key,
                    Some(entry.version),
                    GrammarInstallErrorKind::DownloadPackage,
                    err,
                ));
            }
        };
        if let Err(err) = verify_download_hash(&package_source, &bytes) {
            return Err(self.fail_language_with_error(&language_key, Some(entry.version), err));
        }
        if let Err(err) = verify_package_signature(&package_source, &bytes, verifier) {
            return Err(self.fail_language_with_error(&language_key, Some(entry.version), err));
        }

        let staging_dir = self.unique_staging_dir(&language_key);
        if let Err(err) = fs::create_dir_all(&staging_dir) {
            return Err(self.fail_language(
                &language_key,
                Some(entry.version),
                GrammarInstallErrorKind::AtomicInstall,
                err.to_string(),
            ));
        }
        if let Err(err) = decoder.decode_package(&bytes, &staging_dir, &language_key) {
            let _ = fs::remove_dir_all(&staging_dir);
            return Err(self.fail_language(
                &language_key,
                Some(entry.version),
                GrammarInstallErrorKind::DecodePackage,
                err,
            ));
        }

        let mut manifest = match read_staged_manifest(&staging_dir, &language_key) {
            Ok(manifest) => manifest,
            Err(err) => {
                let _ = fs::remove_dir_all(&staging_dir);
                return Err(self.fail_language_with_error(&language_key, Some(entry.version), err));
            }
        };
        if let Err(err) = validate_staged_manifest(&staging_dir, &entry, &package_source, &manifest)
        {
            let _ = fs::remove_dir_all(&staging_dir);
            return Err(self.fail_language_with_error(&language_key, Some(entry.version), err));
        }
        if let Err(err) = stamp_staged_manifest(&staging_dir, &package_source, &mut manifest) {
            let _ = fs::remove_dir_all(&staging_dir);
            return Err(self.fail_language_with_error(&language_key, Some(entry.version), err));
        }

        if let Err(err) = self.atomic_install_staged_package(&language_key, &staging_dir) {
            let _ = fs::remove_dir_all(&staging_dir);
            return Err(self.fail_language_with_error(&language_key, Some(entry.version), err));
        }

        Ok(
            self.persist_status(GrammarLanguageInstallStatus::with_status(
                language_key,
                GrammarInstallStatus::Installed,
                Some(manifest.version),
                Some(entry.version),
                None,
            )),
        )
    }

    fn persist_status(&self, status: GrammarLanguageInstallStatus) -> GrammarLanguageInstallStatus {
        let mut cache = read_status_cache(&self.runtime_dir);
        cache
            .languages
            .insert(status.language.clone(), status.clone());
        let _ = write_json_atomic(&self.runtime_dir.join(GRAMMAR_STATUS_PATH), &cache);
        status
    }

    fn fail_language(
        &self,
        language: &str,
        available_version: Option<Version>,
        kind: GrammarInstallErrorKind,
        message: impl Into<String>,
    ) -> GrammarInstallError {
        self.fail_language_with_error(
            language,
            available_version,
            GrammarInstallError::new(kind, Some(language.to_string()), None, message),
        )
    }

    fn fail_language_with_error(
        &self,
        language: &str,
        available_version: Option<Version>,
        err: GrammarInstallError,
    ) -> GrammarInstallError {
        self.persist_status(GrammarLanguageInstallStatus::with_status(
            language.to_string(),
            GrammarInstallStatus::Failed,
            None,
            available_version,
            Some(err.message.clone()),
        ));
        err
    }

    fn registry_entry_for_language(&self, language: &str) -> Option<GrammarRegistryEntry> {
        let read = read_runtime_registry(&self.runtime_dir);
        let app_version = current_app_version();
        read.registry.and_then(|registry| {
            registry
                .grammars
                .into_iter()
                .filter(|entry| normalize_language_key(&entry.language) == language)
                .filter(|entry| {
                    GrammarCompatibility::for_requirements(
                        entry.parser_abi,
                        entry.app_version_req.as_ref(),
                        &app_version,
                    )
                    .is_compatible()
                })
                .max_by(|left, right| left.version.cmp(&right.version))
        })
    }

    fn installed_package_for_language(&self, language: &str) -> Option<InstalledGrammarPackage> {
        let app_version = current_app_version();
        scan_installed_packages(&self.runtime_dir, &app_version)
            .packages
            .into_iter()
            .filter(|package| normalize_language_key(&package.manifest.language) == language)
            .filter(|package| installed_package_can_load(package, &self.policy))
            .max_by(|left, right| left.manifest.version.cmp(&right.manifest.version))
    }

    fn unique_staging_dir(&self, language: &str) -> PathBuf {
        let nonce = INSTALL_NONCE.fetch_add(1, Ordering::Relaxed);
        self.runtime_dir
            .join(STAGING_DIR)
            .join(format!("{language}.installing-{nonce}"))
    }

    fn atomic_install_staged_package(
        &self,
        language: &str,
        staging_dir: &Path,
    ) -> Result<(), GrammarInstallError> {
        let parsers_dir = self.runtime_dir.join(PARSERS_DIR);
        fs::create_dir_all(&parsers_dir).map_err(|err| {
            GrammarInstallError::new(
                GrammarInstallErrorKind::AtomicInstall,
                Some(language.to_string()),
                Some(parsers_dir.clone()),
                err.to_string(),
            )
        })?;
        let target = parsers_dir.join(language);
        if !target.exists() {
            fs::rename(staging_dir, &target).map_err(|err| {
                GrammarInstallError::new(
                    GrammarInstallErrorKind::AtomicInstall,
                    Some(language.to_string()),
                    Some(target.clone()),
                    err.to_string(),
                )
            })?;
            return Ok(());
        }

        let backup = self.runtime_dir.join(STAGING_DIR).join(format!(
            "{language}.previous-{}",
            INSTALL_NONCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::rename(&target, &backup).map_err(|err| {
            GrammarInstallError::new(
                GrammarInstallErrorKind::AtomicInstall,
                Some(language.to_string()),
                Some(target.clone()),
                err.to_string(),
            )
        })?;
        match fs::rename(staging_dir, &target) {
            Ok(()) => {
                let _ = fs::remove_dir_all(&backup);
                Ok(())
            }
            Err(err) => {
                let _ = fs::rename(&backup, &target);
                Err(GrammarInstallError::new(
                    GrammarInstallErrorKind::AtomicInstall,
                    Some(language.to_string()),
                    Some(target),
                    err.to_string(),
                ))
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GrammarStatusCacheFile {
    schema_version: u32,
    #[serde(default)]
    languages: BTreeMap<String, GrammarLanguageInstallStatus>,
}

impl Default for GrammarStatusCacheFile {
    fn default() -> Self {
        Self {
            schema_version: GRAMMAR_STATUS_SCHEMA_VERSION,
            languages: BTreeMap::new(),
        }
    }
}

fn read_status_cache(runtime_dir: &Path) -> GrammarStatusCacheFile {
    let path = runtime_dir.join(GRAMMAR_STATUS_PATH);
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            return GrammarStatusCacheFile::default();
        }
        Err(_) => return GrammarStatusCacheFile::default(),
    };
    match serde_json::from_str::<GrammarStatusCacheFile>(&raw) {
        Ok(cache) if cache.schema_version == GRAMMAR_STATUS_SCHEMA_VERSION => cache,
        _ => GrammarStatusCacheFile::default(),
    }
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(value).map_err(|err| err.to_string())?;
    let tmp_path = path.with_extension(format!(
        "tmp-{}",
        INSTALL_NONCE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&tmp_path, bytes).map_err(|err| err.to_string())?;
    fs::rename(&tmp_path, path).map_err(|err| {
        let _ = fs::remove_file(&tmp_path);
        err.to_string()
    })
}

fn verify_download_hash(
    source: &GrammarPackageDownload,
    bytes: &[u8],
) -> Result<(), GrammarInstallError> {
    let Some(expected) = source.sha256.as_deref() else {
        return Ok(());
    };
    let actual = sha256_hex(bytes);
    if actual.eq_ignore_ascii_case(expected) {
        return Ok(());
    }
    Err(GrammarInstallError::new(
        GrammarInstallErrorKind::PackageHashMismatch,
        None,
        None,
        format!("package hash {actual} did not match expected {expected}"),
    ))
}

fn verify_package_signature(
    source: &GrammarPackageDownload,
    bytes: &[u8],
    verifier: &impl GrammarPackageVerifier,
) -> Result<(), GrammarInstallError> {
    if source.source == GrammarPackageSource::Official && source.signature.is_none() {
        return Err(GrammarInstallError::new(
            GrammarInstallErrorKind::UnsignedOfficialPackage,
            None,
            None,
            "official grammar package is unsigned",
        ));
    }
    verifier.verify_package(source, bytes).map_err(|err| {
        GrammarInstallError::new(GrammarInstallErrorKind::VerifyPackage, None, None, err)
    })
}

fn read_staged_manifest(
    staging_dir: &Path,
    language: &str,
) -> Result<GrammarPackageManifest, GrammarInstallError> {
    let manifest_path = staging_dir.join(PACKAGE_MANIFEST_FILENAME);
    let raw = fs::read_to_string(&manifest_path).map_err(|err| {
        GrammarInstallError::new(
            GrammarInstallErrorKind::ReadManifest,
            Some(language.to_string()),
            Some(manifest_path.clone()),
            err.to_string(),
        )
    })?;
    serde_json::from_str::<GrammarPackageManifest>(&raw).map_err(|err| {
        GrammarInstallError::new(
            GrammarInstallErrorKind::ParseManifest,
            Some(language.to_string()),
            Some(manifest_path),
            err.to_string(),
        )
    })
}

fn validate_staged_manifest(
    staging_dir: &Path,
    entry: &GrammarRegistryEntry,
    source: &GrammarPackageDownload,
    manifest: &GrammarPackageManifest,
) -> Result<(), GrammarInstallError> {
    let language = normalize_language_key(&entry.language);
    if normalize_language_key(&manifest.language) != language
        || manifest.version != entry.version
        || manifest.parser_abi != entry.parser_abi
    {
        return Err(GrammarInstallError::new(
            GrammarInstallErrorKind::ManifestMismatch,
            Some(language),
            Some(staging_dir.join(PACKAGE_MANIFEST_FILENAME)),
            "package manifest does not match the registry entry",
        ));
    }
    if let Some(runtime) = entry.runtime {
        if manifest.runtime != runtime {
            return Err(GrammarInstallError::new(
                GrammarInstallErrorKind::ManifestMismatch,
                Some(normalize_language_key(&entry.language)),
                Some(staging_dir.join(PACKAGE_MANIFEST_FILENAME)),
                "package runtime does not match the registry entry",
            ));
        }
    }
    if manifest.runtime == GrammarRuntime::Native
        && manifest.platform != parser_loading::current_platform()
    {
        return Err(GrammarInstallError::new(
            GrammarInstallErrorKind::ManifestMismatch,
            Some(normalize_language_key(&entry.language)),
            Some(staging_dir.join(PACKAGE_MANIFEST_FILENAME)),
            "package platform does not match this runtime",
        ));
    }
    let package_runtime = source.runtime.or(entry.runtime).unwrap_or(manifest.runtime);
    if manifest.runtime != package_runtime {
        return Err(GrammarInstallError::new(
            GrammarInstallErrorKind::ManifestMismatch,
            Some(normalize_language_key(&entry.language)),
            Some(staging_dir.join(PACKAGE_MANIFEST_FILENAME)),
            "package runtime does not match the selected package source",
        ));
    }

    validate_manifest_file_paths(staging_dir, &manifest.files, &manifest.language)?;
    verify_manifest_file_hashes(staging_dir, manifest)?;
    validate_staged_parser_loads(staging_dir, manifest)
}

fn validate_staged_parser_loads(
    staging_dir: &Path,
    manifest: &GrammarPackageManifest,
) -> Result<(), GrammarInstallError> {
    match manifest.runtime {
        GrammarRuntime::Native => Ok(()),
        GrammarRuntime::Wasm => validate_staged_wasm_parser_loads(staging_dir, manifest),
    }
}

fn validate_staged_wasm_parser_loads(
    staging_dir: &Path,
    manifest: &GrammarPackageManifest,
) -> Result<(), GrammarInstallError> {
    let package = InstalledGrammarPackage {
        package_dir: staging_dir.to_path_buf(),
        manifest_path: staging_dir.join(PACKAGE_MANIFEST_FILENAME),
        manifest: manifest.clone(),
        compatibility: GrammarCompatibility::for_requirements(
            manifest.parser_abi,
            manifest.app_version_req.as_ref(),
            &current_app_version(),
        ),
    };
    parser_loading::load_wasm_language(&package)
        .map(|_| ())
        .map_err(|err| {
            GrammarInstallError::new(
                GrammarInstallErrorKind::VerifyPackage,
                Some(normalize_language_key(&manifest.language)),
                manifest
                    .files
                    .wasm
                    .as_ref()
                    .map(|path| staging_dir.join(path)),
                err.message().to_string(),
            )
        })
}

fn stamp_staged_manifest(
    staging_dir: &Path,
    source: &GrammarPackageDownload,
    manifest: &mut GrammarPackageManifest,
) -> Result<(), GrammarInstallError> {
    manifest.source = source.source;
    manifest.source_url = Some(source.url.clone());
    manifest.signature = source.signature.clone();
    write_json_atomic(&staging_dir.join(PACKAGE_MANIFEST_FILENAME), manifest).map_err(|err| {
        GrammarInstallError::new(
            GrammarInstallErrorKind::WriteRegistry,
            Some(normalize_language_key(&manifest.language)),
            Some(staging_dir.join(PACKAGE_MANIFEST_FILENAME)),
            err,
        )
    })
}

fn validate_manifest_file_paths(
    staging_dir: &Path,
    files: &GrammarPackageFiles,
    language: &str,
) -> Result<(), GrammarInstallError> {
    for relative in manifest_paths(files) {
        let path = safe_package_path(staging_dir, relative, language)?;
        if !path.is_file() {
            return Err(GrammarInstallError::new(
                GrammarInstallErrorKind::MissingPackageFile,
                Some(normalize_language_key(language)),
                Some(path),
                "manifest references a file that is not present",
            ));
        }
    }
    Ok(())
}

fn verify_manifest_file_hashes(
    staging_dir: &Path,
    manifest: &GrammarPackageManifest,
) -> Result<(), GrammarInstallError> {
    for relative in manifest_paths(&manifest.files) {
        if !manifest.sha256.contains_key(relative) {
            return Err(GrammarInstallError::new(
                GrammarInstallErrorKind::MissingFileHash,
                Some(normalize_language_key(&manifest.language)),
                Some(safe_package_path(
                    staging_dir,
                    relative,
                    &manifest.language,
                )?),
                "manifest is missing a SHA-256 hash for a package file",
            ));
        }
    }

    for (relative, expected) in &manifest.sha256 {
        let path = safe_package_path(staging_dir, relative, &manifest.language)?;
        let bytes = fs::read(&path).map_err(|err| {
            GrammarInstallError::new(
                GrammarInstallErrorKind::MissingPackageFile,
                Some(normalize_language_key(&manifest.language)),
                Some(path.clone()),
                err.to_string(),
            )
        })?;
        let actual = sha256_hex(&bytes);
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(GrammarInstallError::new(
                GrammarInstallErrorKind::FileHashMismatch,
                Some(normalize_language_key(&manifest.language)),
                Some(path),
                format!("file hash {actual} did not match expected {expected}"),
            ));
        }
    }
    Ok(())
}

fn manifest_paths(files: &GrammarPackageFiles) -> impl Iterator<Item = &String> {
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

fn safe_package_path(
    package_dir: &Path,
    relative_path: &str,
    language: &str,
) -> Result<PathBuf, GrammarInstallError> {
    let trimmed = relative_path.trim();
    if trimmed.is_empty() || trimmed.contains('\\') {
        return Err(GrammarInstallError::new(
            GrammarInstallErrorKind::InvalidPackagePath,
            Some(normalize_language_key(language)),
            None,
            "manifest path is empty or uses unsupported separators",
        ));
    }
    let path = Path::new(trimmed);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(GrammarInstallError::new(
            GrammarInstallErrorKind::InvalidPackagePath,
            Some(normalize_language_key(language)),
            None,
            "manifest path must stay inside the grammar package",
        ));
    }
    Ok(package_dir.join(path))
}

fn entry_is_installable(entry: &GrammarRegistryEntry) -> bool {
    let compatibility = GrammarCompatibility::for_requirements(
        entry.parser_abi,
        entry.app_version_req.as_ref(),
        &current_app_version(),
    );
    compatibility.is_compatible()
        && match entry.runtime {
            Some(GrammarRuntime::Native) => {
                entry.platforms.is_empty()
                    || entry
                        .platforms
                        .iter()
                        .any(|platform| platform == parser_loading::current_platform())
            }
            Some(GrammarRuntime::Wasm) | None => true,
        }
}

fn select_package_source<'a>(
    entry: &'a GrammarRegistryEntry,
    policy: &RuntimeGrammarSecurityPolicy,
) -> Option<&'a GrammarPackageDownload> {
    entry.packages.iter().find(|package| {
        let runtime = package.runtime.or(entry.runtime);
        let runtime_ok = runtime.is_none_or(|runtime| match runtime {
            GrammarRuntime::Native => {
                platform_matches(package) && policy.allows_native_download(package)
            }
            GrammarRuntime::Wasm => true,
        });
        let platform_ok = package.platform.as_deref().is_none_or(|platform| {
            platform == "wasm" || platform == parser_loading::current_platform()
        });
        runtime_ok && platform_ok
    })
}

fn entry_has_disallowed_native_package(
    entry: &GrammarRegistryEntry,
    policy: &RuntimeGrammarSecurityPolicy,
) -> bool {
    entry.packages.iter().any(|package| {
        let runtime = package.runtime.or(entry.runtime);
        runtime == Some(GrammarRuntime::Native)
            && platform_matches(package)
            && !policy.allows_native_download(package)
    })
}

fn platform_matches(package: &GrammarPackageDownload) -> bool {
    package
        .platform
        .as_deref()
        .is_none_or(|platform| platform == parser_loading::current_platform())
}

fn copy_dir_all(source: &Path, destination: &Path) -> Result<(), String> {
    let metadata = fs::metadata(source)
        .map_err(|err| format!("failed to read package source {}: {err}", source.display()))?;
    if !metadata.is_dir() {
        return Err(format!(
            "package source must be a directory: {}",
            source.display()
        ));
    }
    fs::create_dir_all(destination).map_err(|err| err.to_string())?;
    for entry in fs::read_dir(source).map_err(|err| err.to_string())? {
        let entry = entry.map_err(|err| err.to_string())?;
        let file_type = entry.file_type().map_err(|err| err.to_string())?;
        let target = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), target).map_err(|err| err.to_string())?;
        } else {
            return Err(format!(
                "unsupported package entry type: {}",
                entry.path().display()
            ));
        }
    }
    Ok(())
}

fn decode_npm_wasm_package(
    bytes: &[u8],
    staging_dir: &Path,
    target_language: &str,
) -> Result<(), String> {
    let decoder = GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);
    let mut package_name = None;
    let mut package_version = None;
    let mut wasm_files = BTreeMap::<String, Vec<u8>>::new();
    let mut query_files = BTreeMap::<String, Vec<u8>>::new();
    let mut jsx_highlights: Option<Vec<u8>> = None;

    for entry in archive.entries().map_err(|err| err.to_string())? {
        let mut entry = entry.map_err(|err| err.to_string())?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path().map_err(|err| err.to_string())?.into_owned();
        let relative = strip_npm_package_prefix(&path)?;
        let Some(relative) = relative else {
            continue;
        };

        let mut entry_bytes = Vec::new();
        entry
            .read_to_end(&mut entry_bytes)
            .map_err(|err| err.to_string())?;

        if relative == Path::new("package.json") {
            let metadata = serde_json::from_slice::<NpmPackageMetadata>(&entry_bytes)
                .map_err(|err| format!("invalid npm package metadata: {err}"))?;
            package_name = Some(metadata.name);
            package_version = Some(metadata.version);
            continue;
        }

        if let Some(file_name) = relative
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| name.starts_with("tree-sitter-") && name.ends_with(".wasm"))
        {
            wasm_files.insert(file_name.to_string(), entry_bytes);
            continue;
        }

        if relative.starts_with("queries") {
            let file_name = relative
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            match file_name {
                "highlights.scm" | "injections.scm" | "locals.scm" => {
                    query_files.insert(format!("queries/{file_name}"), entry_bytes);
                }
                "highlights-jsx.scm" => {
                    jsx_highlights = Some(entry_bytes);
                }
                _ => {}
            }
        }
    }

    let package_name =
        package_name.ok_or_else(|| "npm package is missing package.json".to_string())?;
    let version = package_version.ok_or_else(|| "npm package is missing version".to_string())?;
    let target_key = normalize_language_key(target_language);
    let (wasm_name, wasm_bytes) = bundled_wasm_for_language(&target_key)
        .map(|(name, bytes)| (name.to_string(), bytes.to_vec()))
        .or_else(|| pick_wasm_for_language(wasm_files, &target_key))
        .ok_or_else(|| format!("npm package {package_name} has no WASM parser for {target_key}"))?;

    if target_key == "javascript" {
        if let Some(jsx) = jsx_highlights {
            let combined = match query_files.get("queries/highlights.scm") {
                Some(highlights) => {
                    let mut combined = highlights.clone();
                    combined.push(b'\n');
                    combined.extend_from_slice(&jsx);
                    combined
                }
                None => jsx,
            };
            query_files.insert("queries/highlights.scm".to_string(), combined);
        }
    }

    for (path, query) in fallback_queries_for_package(&target_key, &package_name) {
        query_files
            .entry(path.to_string())
            .or_insert_with(|| query.as_bytes().to_vec());
    }
    normalize_query_files_for_package(&target_key, &mut query_files);

    let wasm_path = format!("wasm/{wasm_name}");
    fs::create_dir_all(staging_dir.join("wasm")).map_err(|err| err.to_string())?;
    fs::write(staging_dir.join(&wasm_path), &wasm_bytes).map_err(|err| err.to_string())?;

    let mut files = GrammarPackageFiles {
        parser: None,
        wasm: Some(wasm_path.clone()),
        highlights: None,
        injections: None,
        locals: None,
    };
    for (path, bytes) in &query_files {
        let full_path = staging_dir.join(path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        fs::write(&full_path, bytes).map_err(|err| err.to_string())?;
        match Path::new(path).file_name().and_then(|name| name.to_str()) {
            Some("highlights.scm") => files.highlights = Some(path.clone()),
            Some("injections.scm") => files.injections = Some(path.clone()),
            Some("locals.scm") => files.locals = Some(path.clone()),
            _ => {}
        }
    }

    let mut sha256 = BTreeMap::new();
    sha256.insert(wasm_path, sha256_hex(&wasm_bytes));
    for (path, bytes) in &query_files {
        sha256.insert(path.clone(), sha256_hex(bytes));
    }

    let manifest = GrammarPackageManifest {
        language: target_key.clone(),
        version,
        parser_abi: LANGUAGE_VERSION,
        runtime: GrammarRuntime::Wasm,
        platform: "wasm".to_string(),
        source: GrammarPackageSource::Community,
        source_url: None,
        signature: None,
        files,
        filetypes: vec![target_key.clone()],
        extensions: vec![target_key.clone()],
        filenames: Vec::new(),
        first_line_regex: None,
        content_regex: None,
        app_version_req: None,
        sha256,
    };
    fs::write(
        staging_dir.join(PACKAGE_MANIFEST_FILENAME),
        serde_json::to_vec_pretty(&manifest).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())
}

fn pick_wasm_for_language(
    mut wasm_files: BTreeMap<String, Vec<u8>>,
    target_language: &str,
) -> Option<(String, Vec<u8>)> {
    let preferred = format!("tree-sitter-{}.wasm", target_language.replace('-', "_"));
    if let Some(bytes) = wasm_files.remove(&preferred) {
        return Some((preferred, bytes));
    }
    let plain = format!("tree-sitter-{target_language}.wasm");
    if let Some(bytes) = wasm_files.remove(&plain) {
        return Some((plain, bytes));
    }
    wasm_files.into_iter().next()
}

fn bundled_wasm_for_language(language: &str) -> Option<(&'static str, &'static [u8])> {
    match language {
        "scss" => Some((
            "tree-sitter-scss.wasm",
            include_bytes!("../../../assets/tree-sitter/scss/tree-sitter-scss.wasm"),
        )),
        _ => None,
    }
}

fn normalize_query_files_for_package(language: &str, query_files: &mut BTreeMap<String, Vec<u8>>) {
    if language == "cpp" {
        if let Some(highlights) = query_files.get_mut("queries/highlights.scm") {
            if !query_has_c_base_highlights(highlights) {
                let mut combined = CPP_C_BASE_HIGHLIGHTS.as_bytes().to_vec();
                combined.push(b'\n');
                combined.extend_from_slice(highlights);
                *highlights = combined;
            }
        }
    }
}

fn query_has_c_base_highlights(query: &[u8]) -> bool {
    std::str::from_utf8(query).is_ok_and(|query| {
        query.contains("(preproc_directive)") && query.contains("(primitive_type)")
    })
}

#[derive(Deserialize)]
struct NpmPackageMetadata {
    name: String,
    version: Version,
}

fn strip_npm_package_prefix(path: &Path) -> Result<Option<PathBuf>, String> {
    let mut components = path.components();
    match components.next() {
        Some(Component::Normal(prefix)) if prefix == "package" => {}
        Some(_) => return Ok(None),
        None => return Ok(None),
    }
    let mut relative = PathBuf::new();
    for component in components {
        match component {
            Component::Normal(part) => relative.push(part),
            _ => return Err("npm package contains an unsafe path".to_string()),
        }
    }
    Ok((!relative.as_os_str().is_empty()).then_some(relative))
}

fn fallback_queries_for_package(
    language: &str,
    package_name: &str,
) -> &'static [(&'static str, &'static str)] {
    match (
        language,
        package_name.rsplit('/').next().unwrap_or_default(),
    ) {
        ("twig", "tree-sitter-shopware-twig") => &[
            (
                "queries/highlights.scm",
                r#"
(twig_comment) @comment
(variable) @variable
(function_identifier) @function
(tag) @keyword
(conditional) @keyword
(html_tag_name) @tag
(html_attribute_name) @attribute
(html_attribute_value) @string
[
  "{{"
  "}}"
  "{%"
  "%}"
] @punctuation.bracket
"#,
            ),
            (
                "queries/injections.scm",
                r#"; extends

((content) @injection.content
  (#set! injection.language "html")
  (#set! injection.combined))
"#,
            ),
        ],
        ("twig", _) => &[
            (
                "queries/highlights.scm",
                r#"
(comment) @comment
(variable) @variable
(property) @property
(function) @function
(string) @string
(interpolated_string) @string
(number) @number
(boolean) @constant.builtin
(null) @constant.builtin
(embedded_begin) @punctuation.bracket
(embedded_end) @punctuation.bracket
"#,
            ),
            (
                "queries/injections.scm",
                r#"
((content) @injection.content
  (#set! injection.language "html")
  (#set! injection.combined))
"#,
            ),
        ],
        ("xml", _) => &[("queries/highlights.scm", XML_FALLBACK_HIGHLIGHTS)],
        ("markdown", _) => &[
            ("queries/highlights.scm", MARKDOWN_FALLBACK_HIGHLIGHTS),
            ("queries/injections.scm", MARKDOWN_FALLBACK_INJECTIONS),
        ],
        _ => &[],
    }
}

const CPP_C_BASE_HIGHLIGHTS: &str = r##"(identifier) @variable

((identifier) @constant
 (#match? @constant "^[A-Z][A-Z\\d_]*$"))

"break" @keyword
"case" @keyword
"const" @keyword
"continue" @keyword
"default" @keyword
"do" @keyword
"else" @keyword
"enum" @keyword
"extern" @keyword
"for" @keyword
"if" @keyword
"inline" @keyword
"return" @keyword
"sizeof" @keyword
"static" @keyword
"struct" @keyword
"switch" @keyword
"typedef" @keyword
"union" @keyword
"volatile" @keyword
"while" @keyword

"#define" @keyword
"#elif" @keyword
"#else" @keyword
"#endif" @keyword
"#if" @keyword
"#ifdef" @keyword
"#ifndef" @keyword
"#include" @keyword
(preproc_directive) @keyword

"--" @operator
"-" @operator
"-=" @operator
"->" @operator
"=" @operator
"!=" @operator
"*" @operator
"&" @operator
"&&" @operator
"+" @operator
"++" @operator
"+=" @operator
"<" @operator
"==" @operator
">" @operator
"||" @operator

"." @delimiter
";" @delimiter

(string_literal) @string
(system_lib_string) @string

(null) @constant
(number_literal) @number
(char_literal) @number

(field_identifier) @property
(statement_identifier) @label
(type_identifier) @type
(primitive_type) @type
(sized_type_specifier) @type

(call_expression
  function: (identifier) @function)
(call_expression
  function: (field_expression
    field: (field_identifier) @function))
(function_declarator
  declarator: (identifier) @function)
(preproc_function_def
  name: (identifier) @function.special)

(comment) @comment
"##;

const MARKDOWN_FALLBACK_HIGHLIGHTS: &str = r#"(atx_heading (inline) @text.title)
(setext_heading (paragraph) @text.title)

[
  (atx_h1_marker)
  (atx_h2_marker)
  (atx_h3_marker)
  (atx_h4_marker)
  (atx_h5_marker)
  (atx_h6_marker)
  (setext_h1_underline)
  (setext_h2_underline)
] @punctuation.special

[
  (link_title)
  (indented_code_block)
  (fenced_code_block)
] @text.literal

[
  (fenced_code_block_delimiter)
] @punctuation.delimiter

(code_fence_content) @none

[
  (link_destination)
] @text.uri

[
  (link_label)
] @text.reference

[
  (list_marker_plus)
  (list_marker_minus)
  (list_marker_star)
  (list_marker_dot)
  (list_marker_parenthesis)
  (thematic_break)
] @punctuation.special

[
  (block_continuation)
  (block_quote_marker)
] @punctuation.special

[
  (backslash_escape)
] @string.escape
"#;

const MARKDOWN_FALLBACK_INJECTIONS: &str = r#"(fenced_code_block
  (info_string
    (language) @injection.language)
  (code_fence_content) @injection.content)

((html_block) @injection.content (#set! injection.language "html"))
"#;

const XML_FALLBACK_HIGHLIGHTS: &str = r##"
"xml" @keyword

[ "version" "encoding" "standalone" ] @property

(EncName) @string.special
(VersionNum) @number

[ "yes" "no" ] @boolean

(PI) @embedded
(PI (PITarget) @keyword)

(elementdecl
  "ELEMENT" @keyword
  (Name) @tag)

(contentspec
  (_ (Name) @property))

"#PCDATA" @type.builtin

[ "EMPTY" "ANY" ] @string.special.symbol
[ "*" "?" "+" ] @operator

(GEDecl
  "ENTITY" @keyword
  (Name) @constant)
(GEDecl (EntityValue) @string)

(NDataDecl
  "NDATA" @keyword
  (Name) @label)

(PEDecl
  "ENTITY" @keyword
  "%" @operator
  (Name) @constant)
(PEDecl (EntityValue) @string)

(NotationDecl
  "NOTATION" @keyword
  (Name) @constant)
(NotationDecl
  (ExternalID
    (SystemLiteral (URI) @string.special)))

(AttlistDecl
  "ATTLIST" @keyword
  (Name) @tag)
(AttDef (Name) @property)
(AttDef (Enumeration (Nmtoken) @string))
(DefaultDecl (AttValue) @string)

[
  (StringType)
  (TokenizedType)
] @type.builtin

(NotationType "NOTATION" @type.builtin)

[ "#REQUIRED" "#IMPLIED" "#FIXED" ] @attribute

(EntityRef) @constant
((EntityRef) @constant.builtin
  (#any-of? @constant.builtin "&amp;" "&lt;" "&gt;" "&quot;" "&apos;"))
(CharRef) @constant
(PEReference) @constant

[ "PUBLIC" "SYSTEM" ] @keyword

(PubidLiteral) @string.special
(SystemLiteral (URI) @markup.link)

(XmlModelPI "xml-model" @keyword)
(StyleSheetPI "xml-stylesheet" @keyword)
(PseudoAtt (Name) @property)
(PseudoAtt (PseudoAttValue) @string)

(doctypedecl "DOCTYPE" @keyword)
(doctypedecl (Name) @type)

(STag (Name) @tag)
(ETag (Name) @tag)
(EmptyElemTag (Name) @tag)

(Attribute (Name) @property)
(Attribute (AttValue) @string)

[
  "<?" "?>"
  "<!" "]]>"
  "<" ">"
  "</" "/>"
] @punctuation.delimiter

[ "(" ")" "[" "]" ] @punctuation.bracket
[ "\"" "'" ] @punctuation.delimiter
[ "," "|" "=" ] @operator

(CharData) @markup

(CDSect
  (CDStart) @markup.heading
  (CData) @markup.raw
  "]]>" @markup.heading)

(Comment) @comment
"##;

fn local_path_from_url(url: &str) -> Result<PathBuf, String> {
    let Some(path) = url.strip_prefix("file://") else {
        let path = PathBuf::from(url);
        if path.is_absolute() || path.exists() {
            return Ok(path);
        }
        return Err(format!("unsupported grammar transport URL: {url}"));
    };
    Ok(PathBuf::from(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use semver::VersionReq;
    use std::io::Write;
    use tar::Header;
    use tempfile::tempdir;
    use tree_sitter::LANGUAGE_VERSION;

    struct StaticRegistryFetcher {
        result: Result<Vec<u8>, String>,
    }

    impl GrammarRegistryFetcher for StaticRegistryFetcher {
        fn fetch_registry(&self, _source: &RegistryFetchSource) -> Result<Vec<u8>, String> {
            self.result.clone()
        }
    }

    struct StaticDownloader {
        result: Result<Vec<u8>, String>,
    }

    impl GrammarPackageDownloader for StaticDownloader {
        fn download_package(&self, _source: &GrammarPackageDownload) -> Result<Vec<u8>, String> {
            self.result.clone()
        }
    }

    struct FailingDecoder;

    impl GrammarPackageDecoder for FailingDecoder {
        fn decode_package(
            &self,
            _bytes: &[u8],
            staging_dir: &Path,
            _target_language: &str,
        ) -> Result<(), String> {
            fs::write(staging_dir.join("partial.txt"), "partial").unwrap();
            Err("decode failed".to_string())
        }
    }

    struct PanickingRegistryFetcher;

    impl GrammarRegistryFetcher for PanickingRegistryFetcher {
        fn fetch_registry(&self, _source: &RegistryFetchSource) -> Result<Vec<u8>, String> {
            panic!("offline refresh attempted a fetch")
        }
    }

    fn registry_entry(language: &str, version: Version, url: &str) -> GrammarRegistryEntry {
        GrammarRegistryEntry {
            language: language.to_string(),
            version,
            parser_abi: LANGUAGE_VERSION,
            app_version_req: Some(VersionReq::parse(">=0.1.0").unwrap()),
            runtime: Some(GrammarRuntime::Native),
            platforms: vec![parser_loading::current_platform().to_string()],
            filetypes: vec![language.to_string()],
            extensions: vec![language.to_string()],
            filenames: Vec::new(),
            first_line_regex: None,
            content_regex: None,
            packages: vec![GrammarPackageDownload {
                url: url.to_string(),
                sha256: None,
                signature: None,
                source: GrammarPackageSource::Community,
                runtime: Some(GrammarRuntime::Native),
                platform: Some(parser_loading::current_platform().to_string()),
            }],
        }
    }

    fn wasm_registry_entry(language: &str, version: Version, url: &str) -> GrammarRegistryEntry {
        let mut entry = registry_entry(language, version, url);
        entry.runtime = Some(GrammarRuntime::Wasm);
        entry.platforms.clear();
        let package = &mut entry.packages[0];
        package.runtime = Some(GrammarRuntime::Wasm);
        package.platform = Some("wasm".to_string());
        entry
    }

    fn official_registry_entry(language: &str, signature: Option<&str>) -> GrammarRegistryEntry {
        let mut entry = registry_entry(
            language,
            Version::new(1, 0, 0),
            &format!("https://example.invalid/{language}.pkg"),
        );
        let package = &mut entry.packages[0];
        package.source = GrammarPackageSource::Official;
        package.signature = signature.map(str::to_string);
        entry
    }

    fn registry_file(entries: Vec<GrammarRegistryEntry>, fetched_at: u64) -> GrammarRegistryFile {
        GrammarRegistryFile {
            schema_version: REGISTRY_SCHEMA_VERSION,
            cache: RegistryCacheMetadata::new(fetched_at, 1),
            grammars: entries,
        }
    }

    fn write_registry(runtime_dir: &Path, entries: Vec<GrammarRegistryEntry>, fetched_at: u64) {
        write_json_atomic(
            &runtime_dir.join(REGISTRY_JSON_PATH),
            &registry_file(entries, fetched_at),
        )
        .unwrap();
    }

    fn community_service(runtime_dir: &Path) -> GrammarInstallationService {
        GrammarInstallationService::with_policy(
            runtime_dir,
            RuntimeGrammarSecurityPolicy {
                allow_native_community_grammars: true,
                ..RuntimeGrammarSecurityPolicy::default()
            },
        )
    }

    fn write_package_source(root: &Path, language: &str, version: Version) -> PathBuf {
        let package_dir = root.join(format!("{language}-{version}"));
        fs::create_dir_all(
            package_dir
                .join("parser")
                .join(parser_loading::current_platform()),
        )
        .unwrap();
        fs::create_dir_all(package_dir.join("queries")).unwrap();
        fs::write(
            package_dir
                .join("parser")
                .join(parser_loading::current_platform())
                .join(format!("{language}.so")),
            "not a real shared library",
        )
        .unwrap();
        fs::write(package_dir.join("queries").join("highlights.scm"), "").unwrap();
        let parser_path = format!(
            "parser/{}/{language}.so",
            parser_loading::current_platform()
        );
        let highlights_path = "queries/highlights.scm".to_string();
        let mut hashes = BTreeMap::new();
        hashes.insert(
            parser_path.clone(),
            sha256_hex(b"not a real shared library"),
        );
        hashes.insert(highlights_path.clone(), sha256_hex(b""));
        let manifest = GrammarPackageManifest {
            language: language.to_string(),
            version,
            parser_abi: LANGUAGE_VERSION,
            runtime: GrammarRuntime::Native,
            platform: parser_loading::current_platform().to_string(),
            source: GrammarPackageSource::Community,
            source_url: None,
            signature: None,
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
            app_version_req: Some(VersionReq::parse(">=0.1.0").unwrap()),
            sha256: hashes,
        };
        write_json_atomic(&package_dir.join(PACKAGE_MANIFEST_FILENAME), &manifest).unwrap();
        package_dir
    }

    fn npm_wasm_package(
        package_name: &str,
        version: &str,
        wasm_filename: &str,
        highlights: Option<&str>,
    ) -> Vec<u8> {
        let mut tar_bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_bytes);
            append_tar_file(
                &mut builder,
                "package/package.json",
                format!(r#"{{"name":"{package_name}","version":"{version}"}}"#).as_bytes(),
            );
            append_tar_file(
                &mut builder,
                &format!("package/{wasm_filename}"),
                b"\0asm fake test bytes",
            );
            if let Some(highlights) = highlights {
                append_tar_file(
                    &mut builder,
                    "package/queries/highlights.scm",
                    highlights.as_bytes(),
                );
            }
            builder.finish().unwrap();
        }

        let mut encoder = GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&tar_bytes).unwrap();
        encoder.finish().unwrap()
    }

    fn npm_multi_wasm_package(
        package_name: &str,
        version: &str,
        wasm_files: &[(&str, &[u8])],
        highlights: Option<&str>,
    ) -> Vec<u8> {
        let mut tar_bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_bytes);
            append_tar_file(
                &mut builder,
                "package/package.json",
                format!(r#"{{"name":"{package_name}","version":"{version}"}}"#).as_bytes(),
            );
            for (filename, bytes) in wasm_files {
                append_tar_file(&mut builder, &format!("package/{filename}"), bytes);
            }
            if let Some(highlights) = highlights {
                append_tar_file(
                    &mut builder,
                    "package/queries/highlights.scm",
                    highlights.as_bytes(),
                );
            }
            builder.finish().unwrap();
        }

        let mut encoder = GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&tar_bytes).unwrap();
        encoder.finish().unwrap()
    }

    fn append_tar_file(builder: &mut tar::Builder<&mut Vec<u8>>, path: &str, bytes: &[u8]) {
        let mut header = Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, path, bytes).unwrap();
    }

    fn read_manifest(package_dir: &Path) -> GrammarPackageManifest {
        serde_json::from_slice(&fs::read(package_dir.join(PACKAGE_MANIFEST_FILENAME)).unwrap())
            .unwrap()
    }

    #[test]
    fn registry_fetch_uses_stale_cache_when_network_fails() {
        let tmp = tempdir().unwrap();
        write_registry(
            tmp.path(),
            vec![registry_entry("zig", Version::new(1, 0, 0), "memory://zig")],
            0,
        );
        let service = GrammarInstallationService::new(tmp.path());
        let fetcher = StaticRegistryFetcher {
            result: Err("offline".to_string()),
        };

        let outcome = service
            .refresh_registry(
                &RegistryFetchSource::new("https://example.invalid/registry.json"),
                &fetcher,
                &AcceptingRegistryVerifier,
                false,
            )
            .unwrap();

        assert_eq!(outcome.source, RegistryFetchOutcomeSource::StaleFallback);
        assert_eq!(outcome.registry.grammars[0].language, "zig");
        assert_eq!(
            outcome.fallback_error.unwrap().kind,
            GrammarInstallErrorKind::FetchRegistry
        );
    }

    #[test]
    fn offline_policy_never_attempts_registry_fetch() {
        let tmp = tempdir().unwrap();
        let service = GrammarInstallationService::with_policy(
            tmp.path(),
            RuntimeGrammarSecurityPolicy {
                runtime_downloads_enabled: false,
                ..RuntimeGrammarSecurityPolicy::default()
            },
        );

        let err = service
            .refresh_registry(
                &RegistryFetchSource::new("https://example.invalid/registry.json"),
                &PanickingRegistryFetcher,
                &AcceptingRegistryVerifier,
                true,
            )
            .unwrap_err();

        assert_eq!(err.kind, GrammarInstallErrorKind::RuntimeDownloadsDisabled);
    }

    #[test]
    fn runtime_decoder_converts_npm_wasm_package_to_manifest() {
        let tmp = tempdir().unwrap();
        let bytes = npm_wasm_package(
            "tree-sitter-go",
            "0.25.0",
            "tree-sitter-go.wasm",
            Some("(identifier) @variable\n"),
        );

        RuntimePackageDecoder
            .decode_package(&bytes, tmp.path(), "go")
            .unwrap();

        let manifest = read_manifest(tmp.path());
        assert_eq!(manifest.language, "go");
        assert_eq!(manifest.version, Version::new(0, 25, 0));
        assert_eq!(manifest.parser_abi, LANGUAGE_VERSION);
        assert_eq!(manifest.runtime, GrammarRuntime::Wasm);
        assert_eq!(
            manifest.files.wasm.as_deref(),
            Some("wasm/tree-sitter-go.wasm")
        );
        assert_eq!(
            manifest.files.highlights.as_deref(),
            Some("queries/highlights.scm")
        );
        assert!(tmp.path().join("wasm/tree-sitter-go.wasm").exists());
        assert!(manifest.sha256.contains_key("wasm/tree-sitter-go.wasm"));
        assert!(manifest.sha256.contains_key("queries/highlights.scm"));
    }

    #[test]
    fn runtime_decoder_expands_cpp_queries_with_c_base_highlights() {
        let tmp = tempdir().unwrap();
        let bytes = npm_wasm_package(
            "tree-sitter-cpp",
            "0.23.4",
            "tree-sitter-cpp.wasm",
            Some("(qualified_identifier name: (identifier) @function)\n"),
        );

        RuntimePackageDecoder
            .decode_package(&bytes, tmp.path(), "cpp")
            .unwrap();

        let highlights = fs::read_to_string(tmp.path().join("queries/highlights.scm")).unwrap();
        assert!(highlights.contains("(preproc_directive) @keyword"));
        assert!(highlights.contains("(primitive_type) @type"));
        assert!(highlights.contains("(qualified_identifier name: (identifier) @function)"));
    }

    #[test]
    fn runtime_decoder_adds_twig_queries_when_package_has_none() {
        let tmp = tempdir().unwrap();
        let bytes = npm_wasm_package("tree-sitter-twig", "0.8.2", "tree-sitter-twig.wasm", None);

        RuntimePackageDecoder
            .decode_package(&bytes, tmp.path(), "twig")
            .unwrap();

        let manifest = read_manifest(tmp.path());
        assert_eq!(manifest.language, "twig");
        assert_eq!(
            manifest.files.highlights.as_deref(),
            Some("queries/highlights.scm")
        );
        assert_eq!(
            manifest.files.injections.as_deref(),
            Some("queries/injections.scm")
        );
        assert!(
            fs::read_to_string(tmp.path().join("queries/highlights.scm"))
                .unwrap()
                .contains("(variable) @variable")
        );
        assert!(
            fs::read_to_string(tmp.path().join("queries/injections.scm"))
                .unwrap()
                .contains("injection.language \"html\"")
        );
    }

    #[test]
    fn runtime_decoder_uses_bundled_scss_wasm_with_npm_queries() {
        let tmp = tempdir().unwrap();
        let bytes = npm_multi_wasm_package(
            "tree-sitter-scss",
            "1.0.0",
            &[],
            Some("(identifier) @variable\n"),
        );

        RuntimePackageDecoder
            .decode_package(&bytes, tmp.path(), "scss")
            .unwrap();

        let manifest = read_manifest(tmp.path());
        assert_eq!(manifest.language, "scss");
        assert_eq!(
            manifest.files.wasm.as_deref(),
            Some("wasm/tree-sitter-scss.wasm")
        );
        assert_eq!(
            manifest.files.highlights.as_deref(),
            Some("queries/highlights.scm")
        );
        assert_eq!(
            fs::read(tmp.path().join("wasm/tree-sitter-scss.wasm")).unwrap(),
            bundled_wasm_for_language("scss").unwrap().1
        );
    }

    #[test]
    fn runtime_decoder_picks_language_specific_wasm_for_multi_parser_package() {
        let tmp = tempdir().unwrap();
        let bytes = npm_multi_wasm_package(
            "tree-sitter-php",
            "0.24.2",
            &[
                ("tree-sitter-php.wasm", b"php-wasm-bytes"),
                ("tree-sitter-php_only.wasm", b"php-only-wasm-bytes"),
            ],
            Some("(identifier) @variable\n"),
        );

        RuntimePackageDecoder
            .decode_package(&bytes, tmp.path(), "php-only")
            .unwrap();

        let manifest = read_manifest(tmp.path());
        assert_eq!(manifest.language, "php-only");
        assert_eq!(
            manifest.files.wasm.as_deref(),
            Some("wasm/tree-sitter-php_only.wasm")
        );
    }

    #[test]
    fn invalid_wasm_package_is_rejected_before_install() {
        let tmp = tempdir().unwrap();
        let service = GrammarInstallationService::new(tmp.path());
        write_registry(
            tmp.path(),
            vec![wasm_registry_entry(
                "go",
                Version::new(0, 25, 0),
                "memory://go",
            )],
            current_unix_seconds(),
        );
        service.queue_install_for_language("go");

        let err = service
            .install_queued_grammar(
                "go",
                &StaticDownloader {
                    result: Ok(npm_wasm_package(
                        "tree-sitter-go",
                        "0.25.0",
                        "tree-sitter-go.wasm",
                        Some("(identifier) @variable\n"),
                    )),
                },
                &RuntimePackageDecoder,
            )
            .unwrap_err();

        assert_eq!(err.kind, GrammarInstallErrorKind::VerifyPackage);
        assert!(!tmp.path().join(PARSERS_DIR).join("go").exists());
        assert_eq!(
            service.status_for_language("go").status,
            GrammarInstallStatus::Failed
        );
    }

    #[test]
    fn stale_installed_status_is_ignored_when_wasm_package_cannot_load() {
        let tmp = tempdir().unwrap();
        let package_dir = tmp.path().join(PARSERS_DIR).join("go");
        fs::create_dir_all(package_dir.join("wasm")).unwrap();
        fs::create_dir_all(package_dir.join("queries")).unwrap();
        fs::write(package_dir.join("wasm/tree-sitter-go.wasm"), "not wasm").unwrap();
        fs::write(package_dir.join("queries/highlights.scm"), "").unwrap();
        let mut sha256 = BTreeMap::new();
        sha256.insert(
            "wasm/tree-sitter-go.wasm".to_string(),
            sha256_hex(b"not wasm"),
        );
        sha256.insert("queries/highlights.scm".to_string(), sha256_hex(b""));
        write_json_atomic(
            &package_dir.join(PACKAGE_MANIFEST_FILENAME),
            &GrammarPackageManifest {
                language: "go".to_string(),
                version: Version::new(0, 25, 0),
                parser_abi: LANGUAGE_VERSION,
                runtime: GrammarRuntime::Wasm,
                platform: "wasm".to_string(),
                source: GrammarPackageSource::Community,
                source_url: None,
                signature: None,
                files: GrammarPackageFiles {
                    parser: None,
                    wasm: Some("wasm/tree-sitter-go.wasm".to_string()),
                    highlights: Some("queries/highlights.scm".to_string()),
                    injections: None,
                    locals: None,
                },
                filetypes: vec!["go".to_string()],
                extensions: vec!["go".to_string()],
                filenames: Vec::new(),
                first_line_regex: None,
                content_regex: None,
                app_version_req: None,
                sha256,
            },
        )
        .unwrap();
        write_json_atomic(
            &tmp.path().join(GRAMMAR_STATUS_PATH),
            &GrammarStatusCacheFile {
                schema_version: GRAMMAR_STATUS_SCHEMA_VERSION,
                languages: BTreeMap::from([(
                    "go".to_string(),
                    GrammarLanguageInstallStatus::with_status(
                        "go".to_string(),
                        GrammarInstallStatus::Installed,
                        Some(Version::new(0, 25, 0)),
                        Some(Version::new(0, 25, 0)),
                        None,
                    ),
                )]),
            },
        )
        .unwrap();
        write_registry(
            tmp.path(),
            vec![wasm_registry_entry(
                "go",
                Version::new(0, 25, 0),
                "memory://go",
            )],
            current_unix_seconds(),
        );

        let service = GrammarInstallationService::new(tmp.path());
        let status = service.status_for_language("go");

        assert_eq!(status.status, GrammarInstallStatus::Missing);
        assert_eq!(status.available_version, Some(Version::new(0, 25, 0)));
    }

    #[test]
    fn failed_download_keeps_status_failed_and_runtime_usable() {
        let tmp = tempdir().unwrap();
        let service = community_service(tmp.path());
        write_registry(
            tmp.path(),
            vec![registry_entry(
                "yaml",
                Version::new(1, 0, 0),
                "memory://yaml",
            )],
            current_unix_seconds(),
        );
        let queued = service.queue_install_for_language("yaml");
        assert_eq!(queued.status, GrammarInstallStatus::Queued);

        let err = service
            .install_queued_grammar(
                "yaml",
                &StaticDownloader {
                    result: Err("download failed".to_string()),
                },
                &DirectoryPackageDecoder,
            )
            .unwrap_err();

        assert_eq!(err.kind, GrammarInstallErrorKind::DownloadPackage);
        let status = service.status_for_language("yaml");
        assert_eq!(status.status, GrammarInstallStatus::Failed);
        assert_eq!(status.last_error.as_deref(), Some("download failed"));
        assert!(service
            .install_queued_grammar(
                "yaml",
                &StaticDownloader {
                    result: Ok(Vec::new())
                },
                &DirectoryPackageDecoder,
            )
            .is_err());
    }

    #[test]
    fn failed_decode_never_reaches_installed_parser_scan() {
        let tmp = tempdir().unwrap();
        let service = community_service(tmp.path());
        write_registry(
            tmp.path(),
            vec![registry_entry("zig", Version::new(1, 0, 0), "memory://zig")],
            current_unix_seconds(),
        );
        service.queue_install_for_language("zig");

        let err = service
            .install_queued_grammar(
                "zig",
                &StaticDownloader {
                    result: Ok(b"not used".to_vec()),
                },
                &FailingDecoder,
            )
            .unwrap_err();

        assert_eq!(err.kind, GrammarInstallErrorKind::DecodePackage);
        assert!(!tmp.path().join(PARSERS_DIR).join("zig").exists());
        let scan = scan_installed_packages(tmp.path(), &current_app_version());
        assert!(scan.packages.is_empty());
    }

    #[test]
    fn installed_package_survives_scanner_restart() {
        let tmp = tempdir().unwrap();
        let source_root = tempdir().unwrap();
        let package_dir = write_package_source(source_root.path(), "zig", Version::new(1, 0, 0));
        let service = community_service(tmp.path());
        write_registry(
            tmp.path(),
            vec![registry_entry("zig", Version::new(1, 0, 0), "memory://zig")],
            current_unix_seconds(),
        );
        service.queue_install_for_language("zig");

        let status = service
            .install_queued_grammar(
                "zig",
                &StaticDownloader {
                    result: Ok(package_dir.to_string_lossy().as_bytes().to_vec()),
                },
                &DirectoryPackageDecoder,
            )
            .unwrap();

        assert_eq!(status.status, GrammarInstallStatus::Installed);
        let restarted = community_service(tmp.path());
        let restarted_status = restarted.status_for_language("zig");
        assert_eq!(restarted_status.status, GrammarInstallStatus::Installed);
        assert_eq!(
            restarted_status.installed_version,
            Some(Version::new(1, 0, 0))
        );
        let scan = scan_installed_packages(tmp.path(), &current_app_version());
        assert_eq!(scan.packages.len(), 1);
    }

    #[test]
    fn tampered_package_file_is_rejected() {
        let tmp = tempdir().unwrap();
        let source_root = tempdir().unwrap();
        let package_dir = write_package_source(source_root.path(), "zig", Version::new(1, 0, 0));
        fs::write(
            package_dir
                .join("parser")
                .join(parser_loading::current_platform())
                .join("zig.so"),
            "tampered",
        )
        .unwrap();
        let service = community_service(tmp.path());
        write_registry(
            tmp.path(),
            vec![registry_entry("zig", Version::new(1, 0, 0), "memory://zig")],
            current_unix_seconds(),
        );
        service.queue_install_for_language("zig");

        let err = service
            .install_queued_grammar(
                "zig",
                &StaticDownloader {
                    result: Ok(package_dir.to_string_lossy().as_bytes().to_vec()),
                },
                &DirectoryPackageDecoder,
            )
            .unwrap_err();

        assert_eq!(err.kind, GrammarInstallErrorKind::FileHashMismatch);
        assert!(!tmp.path().join(PARSERS_DIR).join("zig").exists());
    }

    #[test]
    fn unsigned_official_package_is_rejected() {
        let tmp = tempdir().unwrap();
        let service = GrammarInstallationService::with_policy(
            tmp.path(),
            RuntimeGrammarSecurityPolicy {
                native_package_source_allowlist: vec!["https://example.invalid/".to_string()],
                ..RuntimeGrammarSecurityPolicy::default()
            },
        );
        write_registry(
            tmp.path(),
            vec![official_registry_entry("zig", None)],
            current_unix_seconds(),
        );
        let queued = service.queue_install_for_language("zig");
        assert_eq!(queued.status, GrammarInstallStatus::Queued);

        let err = service
            .install_queued_grammar_verified(
                "zig",
                &StaticDownloader {
                    result: Ok(Vec::new()),
                },
                &DirectoryPackageDecoder,
                &CommunityOnlyPackageVerifier,
            )
            .unwrap_err();

        assert_eq!(err.kind, GrammarInstallErrorKind::UnsignedOfficialPackage);
    }

    #[test]
    fn update_and_uninstall_replace_installed_package_status() {
        let tmp = tempdir().unwrap();
        let source_root = tempdir().unwrap();
        let v1_source = write_package_source(source_root.path(), "zig", Version::new(1, 0, 0));
        let v2_source = write_package_source(source_root.path(), "zig", Version::new(2, 0, 0));
        let service = community_service(tmp.path());
        write_registry(
            tmp.path(),
            vec![registry_entry(
                "zig",
                Version::new(1, 0, 0),
                "memory://zig-v1",
            )],
            current_unix_seconds(),
        );
        service.queue_install_for_language("zig");
        service
            .install_queued_grammar(
                "zig",
                &StaticDownloader {
                    result: Ok(v1_source.to_string_lossy().as_bytes().to_vec()),
                },
                &DirectoryPackageDecoder,
            )
            .unwrap();
        write_registry(
            tmp.path(),
            vec![registry_entry(
                "zig",
                Version::new(2, 0, 0),
                "memory://zig-v2",
            )],
            current_unix_seconds(),
        );

        let updated = service
            .update_grammar(
                "zig",
                &StaticDownloader {
                    result: Ok(v2_source.to_string_lossy().as_bytes().to_vec()),
                },
                &DirectoryPackageDecoder,
            )
            .unwrap();
        assert_eq!(updated.installed_version, Some(Version::new(2, 0, 0)));

        let removed = service.uninstall_grammar("zig").unwrap();
        assert_eq!(removed.status, GrammarInstallStatus::Missing);
        assert!(!tmp.path().join(PARSERS_DIR).join("zig").exists());
    }

    #[test]
    fn failed_update_keeps_existing_installed_package() {
        let tmp = tempdir().unwrap();
        let source_root = tempdir().unwrap();
        let v1_source = write_package_source(source_root.path(), "zig", Version::new(1, 0, 0));
        let v2_source = write_package_source(source_root.path(), "zig", Version::new(2, 0, 0));
        fs::write(
            v2_source
                .join("parser")
                .join(parser_loading::current_platform())
                .join("zig.so"),
            "tampered",
        )
        .unwrap();
        let service = community_service(tmp.path());
        write_registry(
            tmp.path(),
            vec![registry_entry(
                "zig",
                Version::new(1, 0, 0),
                "memory://zig-v1",
            )],
            current_unix_seconds(),
        );
        service.queue_install_for_language("zig");
        service
            .install_queued_grammar(
                "zig",
                &StaticDownloader {
                    result: Ok(v1_source.to_string_lossy().as_bytes().to_vec()),
                },
                &DirectoryPackageDecoder,
            )
            .unwrap();
        write_registry(
            tmp.path(),
            vec![registry_entry(
                "zig",
                Version::new(2, 0, 0),
                "memory://zig-v2",
            )],
            current_unix_seconds(),
        );

        let err = service
            .update_grammar(
                "zig",
                &StaticDownloader {
                    result: Ok(v2_source.to_string_lossy().as_bytes().to_vec()),
                },
                &DirectoryPackageDecoder,
            )
            .unwrap_err();
        let status = service.status_for_language("zig");
        let scan = scan_installed_packages(tmp.path(), &current_app_version());

        assert_eq!(err.kind, GrammarInstallErrorKind::FileHashMismatch);
        assert_eq!(status.status, GrammarInstallStatus::Installed);
        assert_eq!(status.installed_version, Some(Version::new(1, 0, 0)));
        assert_eq!(scan.packages.len(), 1);
        assert_eq!(scan.packages[0].manifest.version, Version::new(1, 0, 0));
    }
}
