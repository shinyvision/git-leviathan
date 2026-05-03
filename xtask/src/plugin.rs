mod lint;
mod template;

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use git_leviathan_plugin_api::descriptor::api;
use git_leviathan_plugin_api::manifest::PluginManifest;
use mlua::{Lua, Table};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use lint::{lint_path, print_lint_report, LintReport};
use template::Template;

type DynResult<T> = Result<T, Box<dyn Error>>;

const PACKAGE_FORMAT: &str = "git-leviathan-plugin-package-v1";
const PACKAGE_EXT: &str = "glplugin";
const TRUST_ROOT_FORMAT: &str = "git-leviathan-plugin-trust-roots-v1";
const REGISTRY_FORMAT: &str = "git-leviathan-plugin-registry-v1";
const SIGNATURE_ALGORITHM: &str = "keyed-sha256-v1";
const INSTALL_LOCK_FORMAT: &str = "git-leviathan-plugin-install-lock-v1";

pub fn run(args: &[String]) -> DynResult<()> {
    match args.first().map(String::as_str) {
        Some("new") => plugin_new(&args[1..]),
        Some("test") => plugin_test(&args[1..]),
        Some("lint") => plugin_lint(&args[1..]).map(|_| ()),
        Some("package") => plugin_package(&args[1..]),
        Some("inspect") => plugin_inspect(&args[1..]),
        Some("install") => plugin_install(&args[1..]),
        Some("publish") => plugin_publish(&args[1..]),
        Some("upgrade-plan") => plugin_upgrade_plan(&args[1..]),
        _ => {
            eprintln!(
                "usage: cargo xtask plugin <new|test|lint|package|inspect|install|publish|upgrade-plan> ..."
            );
            std::process::exit(2);
        }
    }
}

fn plugin_new(args: &[String]) -> DynResult<()> {
    let mut id = None;
    let mut template = "main-bar".to_string();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--template" => {
                i += 1;
                template = args
                    .get(i)
                    .ok_or("missing value after --template")?
                    .to_string();
            }
            value if value.starts_with("--") => return Err(format!("unknown flag: {value}").into()),
            value => {
                if id.replace(value.to_string()).is_some() {
                    return Err("plugin new accepts exactly one <id>".into());
                }
            }
        }
        i += 1;
    }

    let id = id.ok_or("usage: cargo xtask plugin new <id> [--template <name>]")?;
    validate_plugin_id(&id)?;
    let template = Template::by_name(&template)?;
    let dir = PathBuf::from(&id);
    if dir.exists() {
        return Err(format!("{} already exists", dir.display()).into());
    }

    fs::create_dir_all(dir.join("tests"))?;
    fs::create_dir_all(dir.join("doc"))?;
    fs::write(dir.join("plugin.toml"), template.manifest(&id))?;
    fs::write(dir.join("init.lua"), template.init_lua(&id))?;
    fs::write(dir.join("README.md"), template.readme(&id))?;
    fs::write(dir.join("tests").join("smoke.lua"), template.test_lua(&id))?;
    fs::write(
        dir.join("doc").join("usage.md"),
        format!("# {id}\n\nDevelopment notes for the `{id}` plugin.\n"),
    )?;
    println!(
        "created {} using template {}",
        dir.display(),
        template.name()
    );
    Ok(())
}

fn plugin_test(args: &[String]) -> DynResult<()> {
    let path = single_optional_path(args, "usage: cargo xtask plugin test [path]")?;
    let report = lint_path(&path);
    print_lint_report(&report);
    if report.has_errors() {
        return Err("plugin test stopped because lint failed".into());
    }

    let lua_files = lua_files_for_test(&path)?;
    if lua_files.is_empty() {
        return Err("no Lua files found to test".into());
    }

    let lua = Lua::new();
    install_test_stubs(&lua, &path)?;
    set_package_path(&lua, &path)?;

    for file in lua_files {
        let source = fs::read_to_string(&file)?;
        lua.load(&source)
            .set_name(path_label(&file))
            .exec()
            .map_err(|e| format!("{} failed: {e}", file.display()))?;
        println!("ok {}", file.display());
    }
    Ok(())
}

fn plugin_lint(args: &[String]) -> DynResult<LintReport> {
    let path = single_optional_path(args, "usage: cargo xtask plugin lint [path]")?;
    let report = lint_path(&path);
    print_lint_report(&report);
    if report.has_errors() {
        Err("plugin lint failed".into())
    } else {
        Ok(report)
    }
}

fn plugin_package(args: &[String]) -> DynResult<()> {
    let mut path = None;
    let mut out_dir = PathBuf::from("target/plugin-packages");
    let mut signing_key = None;
    let mut trust_root = None;
    let mut key_id = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                i += 1;
                out_dir = PathBuf::from(args.get(i).ok_or("missing value after --out")?);
            }
            "--signing-key" => {
                i += 1;
                signing_key = Some(PathBuf::from(
                    args.get(i).ok_or("missing value after --signing-key")?,
                ));
            }
            "--trust-root" => {
                i += 1;
                trust_root = Some(
                    args.get(i)
                        .ok_or("missing value after --trust-root")?
                        .clone(),
                );
            }
            "--key-id" => {
                i += 1;
                key_id = Some(args.get(i).ok_or("missing value after --key-id")?.clone());
            }
            value if value.starts_with("--") => return Err(format!("unknown flag: {value}").into()),
            value => {
                if path.replace(PathBuf::from(value)).is_some() {
                    return Err("plugin package accepts at most one [path]".into());
                }
            }
        }
        i += 1;
    }

    let path = path.unwrap_or_else(|| PathBuf::from("."));
    let report = lint_path(&path);
    print_lint_report(&report);
    if report.has_errors() {
        return Err("plugin package stopped because lint failed".into());
    }

    let mut package = build_package(&path)?;
    if signing_key.is_some() || trust_root.is_some() || key_id.is_some() {
        let signing_key = signing_key.ok_or("plugin package signing requires --signing-key")?;
        let trust_root = trust_root.ok_or("plugin package signing requires --trust-root <id>")?;
        let key = fs::read(&signing_key)?;
        let key_id = key_id.unwrap_or_else(|| short_key_id(&key));
        sign_package(&mut package, &key, &trust_root, &key_id)?;
    }
    fs::create_dir_all(&out_dir)?;
    let filename = format!(
        "{}-{}.{}",
        package.plugin.id, package.plugin.version, PACKAGE_EXT
    );
    let out_path = out_dir.join(filename);
    fs::write(&out_path, serde_json::to_vec_pretty(&package)?)?;
    println!("wrote {}", out_path.display());
    Ok(())
}

fn plugin_inspect(args: &[String]) -> DynResult<()> {
    let mut path = None;
    let mut trust_roots = None;
    let mut registry = None;
    let mut require_signature = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--trust-roots" => {
                i += 1;
                trust_roots = Some(PathBuf::from(
                    args.get(i).ok_or("missing value after --trust-roots")?,
                ));
            }
            "--registry" => {
                i += 1;
                registry = Some(PathBuf::from(
                    args.get(i).ok_or("missing value after --registry")?,
                ));
            }
            "--require-signature" => require_signature = true,
            value if value.starts_with("--") => return Err(format!("unknown flag: {value}").into()),
            value => {
                if path.replace(PathBuf::from(value)).is_some() {
                    return Err("plugin inspect accepts exactly one <package-or-dir>".into());
                }
            }
        }
        i += 1;
    }
    let path = path.ok_or(
        "usage: cargo xtask plugin inspect <package-or-dir> [--trust-roots <file>] [--registry <dir>] [--require-signature]",
    )?;
    let verify = VerifyContext::from_paths(
        trust_roots.as_deref(),
        registry.as_deref(),
        require_signature,
    )?;
    if path.is_dir() {
        let package = build_package(&path)?;
        verify_package_with_context(&package, &verify)?;
        print_package_summary(&package, "directory");
    } else {
        let package = read_package(&path)?;
        verify_package_with_context(&package, &verify)?;
        verify_package_file_against_registry(&path, &package, &verify)?;
        print_package_summary(&package, "package");
    }
    Ok(())
}

fn plugin_install(args: &[String]) -> DynResult<()> {
    let mut package_path = None;
    let mut plugins_dir = PathBuf::from("plugins");
    let mut lockfile = None;
    let mut trust_roots = None;
    let mut registry = None;
    let mut require_signature = false;
    let mut force = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--plugins-dir" => {
                i += 1;
                plugins_dir =
                    PathBuf::from(args.get(i).ok_or("missing value after --plugins-dir")?);
            }
            "--lockfile" => {
                i += 1;
                lockfile = Some(PathBuf::from(
                    args.get(i).ok_or("missing value after --lockfile")?,
                ));
            }
            "--trust-roots" => {
                i += 1;
                trust_roots = Some(PathBuf::from(
                    args.get(i).ok_or("missing value after --trust-roots")?,
                ));
            }
            "--registry" => {
                i += 1;
                registry = Some(PathBuf::from(
                    args.get(i).ok_or("missing value after --registry")?,
                ));
            }
            "--require-signature" => require_signature = true,
            "--force" => force = true,
            value if value.starts_with("--") => return Err(format!("unknown flag: {value}").into()),
            value => {
                if package_path.replace(PathBuf::from(value)).is_some() {
                    return Err("plugin install accepts exactly one <package>".into());
                }
            }
        }
        i += 1;
    }

    let package_path = package_path
        .ok_or("usage: cargo xtask plugin install <package> [--plugins-dir <dir>] [--lockfile <file>] [--trust-roots <file>] [--registry <dir>] [--require-signature] [--force]")?;
    let package = read_package(&package_path)?;
    let verify = VerifyContext::from_paths(
        trust_roots.as_deref(),
        registry.as_deref(),
        require_signature,
    )?;
    verify_package_with_context(&package, &verify)?;
    verify_package_file_against_registry(&package_path, &package, &verify)?;
    install_package(&package, &plugins_dir, force)?;
    let lockfile = lockfile.unwrap_or_else(|| plugins_dir.join("plugins.lock"));
    update_install_lock(&lockfile, &package, &package_path, registry.as_deref())?;
    println!(
        "installed {} {} to {}",
        package.plugin.id,
        package.plugin.version,
        plugins_dir.join(&package.plugin.id).display()
    );
    Ok(())
}

fn plugin_publish(args: &[String]) -> DynResult<()> {
    let mut package_path = None;
    let mut registry = None;
    let mut trust_roots = None;
    let mut revoke = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--registry" => {
                i += 1;
                registry = Some(PathBuf::from(
                    args.get(i).ok_or("missing value after --registry")?,
                ));
            }
            "--trust-roots" => {
                i += 1;
                trust_roots = Some(PathBuf::from(
                    args.get(i).ok_or("missing value after --trust-roots")?,
                ));
            }
            "--revoke" => {
                i += 1;
                revoke = Some(args.get(i).ok_or("missing value after --revoke")?.clone());
            }
            value if value.starts_with("--") => return Err(format!("unknown flag: {value}").into()),
            value => {
                if package_path.replace(PathBuf::from(value)).is_some() {
                    return Err("plugin publish accepts exactly one <package>".into());
                }
            }
        }
        i += 1;
    }

    let registry = registry.ok_or("plugin publish requires --registry <dir>")?;
    if let Some(spec) = revoke {
        revoke_registry_version(&registry, &spec, "revoked by local registry tooling")?;
        println!("revoked {spec} in {}", registry.display());
        return Ok(());
    }
    let package_path = package_path.ok_or(
        "usage: cargo xtask plugin publish <package> --registry <dir> [--trust-roots <file>] [--revoke <id@version>]",
    )?;
    let package = read_package(&package_path)?;
    let verify = VerifyContext::from_paths(trust_roots.as_deref(), None, trust_roots.is_some())?;
    verify_package_with_context(&package, &verify)?;

    let target_dir = registry
        .join(&package.plugin.id)
        .join(&package.plugin.version);
    fs::create_dir_all(&target_dir)?;
    let target_path = target_dir.join(
        package_path
            .file_name()
            .and_then(OsStr::to_str)
            .ok_or("package path has no file name")?,
    );
    fs::copy(&package_path, &target_path)?;
    update_registry_index(&registry, &package, &target_path)?;
    if let Some(trust_roots_path) = trust_roots {
        fs::copy(&trust_roots_path, registry.join("trust-roots.json"))?;
    }
    println!("published {}", target_path.display());
    Ok(())
}

fn plugin_upgrade_plan(args: &[String]) -> DynResult<()> {
    let mut paths = Vec::new();
    let mut trust_roots = None;
    let mut registry = None;
    let mut require_signature = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--trust-roots" => {
                i += 1;
                trust_roots = Some(PathBuf::from(
                    args.get(i).ok_or("missing value after --trust-roots")?,
                ));
            }
            "--registry" => {
                i += 1;
                registry = Some(PathBuf::from(
                    args.get(i).ok_or("missing value after --registry")?,
                ));
            }
            "--require-signature" => require_signature = true,
            value if value.starts_with("--") => return Err(format!("unknown flag: {value}").into()),
            value => paths.push(PathBuf::from(value)),
        }
        i += 1;
    }
    if paths.len() != 2 {
        return Err("usage: cargo xtask plugin upgrade-plan <installed-package-or-dir> <candidate-package-or-dir> [--trust-roots <file>] [--registry <dir>] [--require-signature]".into());
    }
    let verify = VerifyContext::from_paths(
        trust_roots.as_deref(),
        registry.as_deref(),
        require_signature,
    )?;
    let installed = package_from_path(&paths[0], &VerifyContext::default())?;
    let candidate = package_from_path(&paths[1], &verify)?;
    print_upgrade_plan(&installed, &candidate);
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct PluginPackage {
    format: String,
    plugin: PackagePlugin,
    #[serde(default)]
    metadata: PackageMetadata,
    root_checksum: String,
    files: Vec<PackageFile>,
    #[serde(default)]
    signatures: Vec<PackageSignature>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PackagePlugin {
    id: String,
    name: String,
    version: String,
    api_version: String,
    description: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PackageMetadata {
    manifest_sha256: String,
    checksum_algorithm: String,
    package_tool: String,
    capabilities: Vec<String>,
    dependencies: BTreeMap<String, String>,
    optional_dependencies: BTreeMap<String, String>,
    provides_services: Vec<String>,
    consumes_services: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PackageSignature {
    algorithm: String,
    trust_root: String,
    key_id: String,
    signature: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct PackageFile {
    path: String,
    size: u64,
    sha256: String,
    content_base64: String,
}

fn build_package(path: &Path) -> DynResult<PluginPackage> {
    let manifest_raw = fs::read_to_string(path.join("plugin.toml"))?;
    let manifest: PluginManifest = toml::from_str(&manifest_raw)?;
    let metadata = PackageMetadata {
        manifest_sha256: sha256_hex(manifest_raw.as_bytes()),
        checksum_algorithm: "sha256".to_string(),
        package_tool: "cargo xtask plugin package".to_string(),
        capabilities: sorted_capabilities(&manifest),
        dependencies: version_req_map(&manifest.dependencies),
        optional_dependencies: version_req_map(&manifest.optional_dependencies),
        provides_services: sorted_service_keys(&manifest.provides_services),
        consumes_services: sorted_service_keys(&manifest.consumes_services),
    };
    let mut files = Vec::new();
    for file in package_files(path)? {
        let bytes = fs::read(&file)?;
        let rel = normalize_relative(path, &file)?;
        files.push(PackageFile {
            path: rel,
            size: bytes.len() as u64,
            sha256: sha256_hex(&bytes),
            content_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        });
    }
    let root_checksum = root_checksum(&files);
    Ok(PluginPackage {
        format: PACKAGE_FORMAT.to_string(),
        plugin: PackagePlugin {
            id: manifest.id,
            name: manifest.name,
            version: manifest.version.to_string(),
            api_version: format!(
                "{}.{}",
                manifest.api_version.major, manifest.api_version.minor
            ),
            description: manifest.description,
        },
        metadata,
        root_checksum,
        files,
        signatures: Vec::new(),
    })
}

fn read_package(path: &Path) -> DynResult<PluginPackage> {
    let raw = fs::read(path)?;
    let package: PluginPackage = serde_json::from_slice(&raw)?;
    Ok(package)
}

fn verify_package_with_context(package: &PluginPackage, context: &VerifyContext) -> DynResult<()> {
    if package.format != PACKAGE_FORMAT {
        return Err(format!("unsupported package format `{}`", package.format).into());
    }
    let mut paths = BTreeSet::new();
    let mut manifest_raw = None;
    for file in &package.files {
        if !paths.insert(&file.path) {
            return Err(format!("duplicate package path `{}`", file.path).into());
        }
        validate_package_path(&file.path)?;
        let content = base64::engine::general_purpose::STANDARD
            .decode(&file.content_base64)
            .map_err(|e| format!("{} has invalid base64 content: {e}", file.path))?;
        if content.len() as u64 != file.size {
            return Err(format!("{} size mismatch", file.path).into());
        }
        let got = sha256_hex(&content);
        if got != file.sha256 {
            return Err(format!("{} checksum mismatch", file.path).into());
        }
        if file.path == "plugin.toml" {
            manifest_raw = Some(String::from_utf8(content).map_err(|e| {
                format!("plugin.toml is not valid UTF-8 for manifest verification: {e}")
            })?);
        }
    }
    let got_root = root_checksum(&package.files);
    if got_root != package.root_checksum {
        return Err("package root checksum mismatch".into());
    }
    let manifest_raw = manifest_raw.ok_or("package is missing plugin.toml")?;
    verify_manifest_matches_package(package, &manifest_raw)?;
    verify_registry_metadata(package, context)?;
    verify_package_signatures(package, context)?;
    Ok(())
}

fn install_package(package: &PluginPackage, plugins_dir: &Path, force: bool) -> DynResult<()> {
    let target_dir = plugins_dir.join(&package.plugin.id);
    if target_dir.exists() {
        if !force {
            return Err(format!(
                "{} already exists; pass --force to replace it",
                target_dir.display()
            )
            .into());
        }
        fs::remove_dir_all(&target_dir)?;
    }

    fs::create_dir_all(&target_dir)?;
    for file in &package.files {
        validate_package_path(&file.path)?;
        let target = target_dir.join(&file.path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = base64::engine::general_purpose::STANDARD
            .decode(&file.content_base64)
            .map_err(|e| format!("{} has invalid base64 content: {e}", file.path))?;
        fs::write(target, content)?;
    }
    Ok(())
}

fn print_package_summary(package: &PluginPackage, kind: &str) {
    println!("{kind}: {}", package.plugin.id);
    println!("name: {}", package.plugin.name);
    println!("version: {}", package.plugin.version);
    println!("api_version: {}", package.plugin.api_version);
    println!(
        "capabilities: {}",
        comma_list(&package.metadata.capabilities)
    );
    println!("dependencies: {}", map_list(&package.metadata.dependencies));
    println!(
        "optional_dependencies: {}",
        map_list(&package.metadata.optional_dependencies)
    );
    println!("files: {}", package.files.len());
    println!("root_checksum: {}", package.root_checksum);
    println!("manifest_sha256: {}", package.metadata.manifest_sha256);
    if package.signatures.is_empty() {
        println!("signatures: none");
    } else {
        println!("signatures:");
        for signature in &package.signatures {
            println!(
                "  {} {}:{} {}",
                signature.algorithm, signature.trust_root, signature.key_id, signature.signature
            );
        }
    }
    for file in &package.files {
        println!("  {}  {} bytes  {}", file.sha256, file.size, file.path);
    }
}

fn root_checksum(files: &[PackageFile]) -> String {
    let mut hasher = Sha256::new();
    let mut ordered = files.iter().collect::<Vec<_>>();
    ordered.sort_by(|a, b| a.path.cmp(&b.path));
    for file in ordered {
        hasher.update(file.path.as_bytes());
        hasher.update([0]);
        hasher.update(file.size.to_string().as_bytes());
        hasher.update([0]);
        hasher.update(file.sha256.as_bytes());
        hasher.update([b'\n']);
    }
    hex_lower(&hasher.finalize())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_lower(&hasher.finalize())
}

fn short_key_id(key: &[u8]) -> String {
    sha256_hex(key).chars().take(16).collect()
}

fn keyed_sha256_hex(key: &[u8], payload: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key);
    hasher.update([0]);
    hasher.update(payload);
    hex_lower(&hasher.finalize())
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[derive(Default)]
struct VerifyContext {
    trust_roots: Option<TrustRoots>,
    registry: Option<RegistryIndex>,
    require_signature: bool,
}

impl VerifyContext {
    fn from_paths(
        trust_roots: Option<&Path>,
        registry: Option<&Path>,
        require_signature: bool,
    ) -> DynResult<Self> {
        let trust_roots = trust_roots.map(read_trust_roots).transpose()?;
        let registry = registry.map(read_registry_index).transpose()?;
        Ok(Self {
            trust_roots,
            registry,
            require_signature,
        })
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct TrustRoots {
    #[serde(default = "trust_root_format")]
    format: String,
    #[serde(default)]
    keys: Vec<TrustKey>,
    #[serde(default)]
    revoked_signatures: Vec<RevokedSignature>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TrustKey {
    trust_root: String,
    key_id: String,
    key_base64: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct RevokedSignature {
    trust_root: String,
    key_id: String,
    #[serde(default)]
    reason: String,
}

fn trust_root_format() -> String {
    TRUST_ROOT_FORMAT.to_string()
}

fn registry_format() -> String {
    REGISTRY_FORMAT.to_string()
}

fn install_lock_format() -> String {
    INSTALL_LOCK_FORMAT.to_string()
}

fn read_trust_roots(path: &Path) -> DynResult<TrustRoots> {
    let roots: TrustRoots = serde_json::from_slice(&fs::read(path)?)?;
    if roots.format != TRUST_ROOT_FORMAT {
        return Err(format!("unsupported trust roots format `{}`", roots.format).into());
    }
    Ok(roots)
}

fn sign_package(
    package: &mut PluginPackage,
    key: &[u8],
    trust_root: &str,
    key_id: &str,
) -> DynResult<()> {
    let payload = package_signing_payload(package)?;
    let signature = keyed_sha256_hex(key, &payload);
    package
        .signatures
        .retain(|existing| existing.trust_root != trust_root || existing.key_id != key_id);
    package.signatures.push(PackageSignature {
        algorithm: SIGNATURE_ALGORITHM.to_string(),
        trust_root: trust_root.to_string(),
        key_id: key_id.to_string(),
        signature,
    });
    package.signatures.sort_by(|a, b| {
        (&a.trust_root, &a.key_id, &a.algorithm).cmp(&(&b.trust_root, &b.key_id, &b.algorithm))
    });
    Ok(())
}

fn package_signing_payload(package: &PluginPackage) -> DynResult<Vec<u8>> {
    #[derive(Serialize)]
    struct SigningPayload<'a> {
        format: &'a str,
        plugin: &'a PackagePlugin,
        metadata: &'a PackageMetadata,
        root_checksum: &'a str,
        files: Vec<SigningFile<'a>>,
    }

    #[derive(Serialize)]
    struct SigningFile<'a> {
        path: &'a str,
        size: u64,
        sha256: &'a str,
    }

    let mut files = package
        .files
        .iter()
        .map(|file| SigningFile {
            path: &file.path,
            size: file.size,
            sha256: &file.sha256,
        })
        .collect::<Vec<_>>();
    files.sort_by(|a, b| a.path.cmp(b.path));
    Ok(serde_json::to_vec(&SigningPayload {
        format: &package.format,
        plugin: &package.plugin,
        metadata: &package.metadata,
        root_checksum: &package.root_checksum,
        files,
    })?)
}

fn verify_package_signatures(package: &PluginPackage, context: &VerifyContext) -> DynResult<()> {
    if package.signatures.is_empty() {
        if context.require_signature {
            return Err("package signature required but package is unsigned".into());
        }
        return Ok(());
    }
    let Some(trust_roots) = &context.trust_roots else {
        if context.require_signature {
            return Err("package signature verification requires --trust-roots".into());
        }
        return Ok(());
    };
    let payload = package_signing_payload(package)?;
    let mut verified = false;
    for signature in &package.signatures {
        if signature.algorithm != SIGNATURE_ALGORITHM {
            return Err(
                format!("unsupported signature algorithm `{}`", signature.algorithm).into(),
            );
        }
        if trust_roots.revoked_signatures.iter().any(|revoked| {
            revoked.trust_root == signature.trust_root && revoked.key_id == signature.key_id
        }) {
            return Err(format!(
                "signature key {}:{} is revoked",
                signature.trust_root, signature.key_id
            )
            .into());
        }
        let key = trust_roots
            .keys
            .iter()
            .find(|key| key.trust_root == signature.trust_root && key.key_id == signature.key_id)
            .ok_or_else(|| {
                format!(
                    "unknown signature trust root {}:{}",
                    signature.trust_root, signature.key_id
                )
            })?;
        let key_bytes = base64::engine::general_purpose::STANDARD
            .decode(&key.key_base64)
            .map_err(|e| {
                format!(
                    "trust key {}:{} is invalid base64: {e}",
                    key.trust_root, key.key_id
                )
            })?;
        let expected = keyed_sha256_hex(&key_bytes, &payload);
        if expected != signature.signature {
            return Err(format!(
                "package signature mismatch for {}:{}",
                signature.trust_root, signature.key_id
            )
            .into());
        }
        verified = true;
    }
    if context.require_signature && !verified {
        return Err("package signature required but no signature verified".into());
    }
    Ok(())
}

fn verify_registry_metadata(package: &PluginPackage, context: &VerifyContext) -> DynResult<()> {
    let Some(registry) = &context.registry else {
        return Ok(());
    };
    let key = package_key(package);
    if registry
        .revocations
        .iter()
        .any(|revoked| revoked.id == package.plugin.id && revoked.version == package.plugin.version)
    {
        return Err(format!("{key} is revoked by registry").into());
    }
    if let Some(entry) = registry.packages.get(&key) {
        if entry.root_checksum != package.root_checksum {
            return Err(format!("{key} root checksum differs from registry index").into());
        }
        if !entry.capabilities.is_empty() && entry.capabilities != package.metadata.capabilities {
            return Err(format!("{key} capabilities differ from registry index").into());
        }
        if !entry.dependencies.is_empty() && entry.dependencies != package.metadata.dependencies {
            return Err(format!("{key} dependencies differ from registry index").into());
        }
    }
    Ok(())
}

fn verify_package_file_against_registry(
    path: &Path,
    package: &PluginPackage,
    context: &VerifyContext,
) -> DynResult<()> {
    let Some(registry) = &context.registry else {
        return Ok(());
    };
    let key = package_key(package);
    let Some(entry) = registry.packages.get(&key) else {
        return Ok(());
    };
    if entry.package_sha256.is_empty() {
        return Ok(());
    }
    let got = sha256_hex(&fs::read(path)?);
    if got != entry.package_sha256 {
        return Err(format!("{key} package file checksum differs from registry index").into());
    }
    Ok(())
}

fn verify_manifest_matches_package(package: &PluginPackage, manifest_raw: &str) -> DynResult<()> {
    let manifest: PluginManifest = toml::from_str(manifest_raw)?;
    let api_version = format!(
        "{}.{}",
        manifest.api_version.major, manifest.api_version.minor
    );
    if manifest.id != package.plugin.id {
        return Err("manifest id differs from package metadata".into());
    }
    if manifest.name != package.plugin.name {
        return Err("manifest name differs from package metadata".into());
    }
    if manifest.version.to_string() != package.plugin.version {
        return Err("manifest version differs from package metadata".into());
    }
    if api_version != package.plugin.api_version {
        return Err("manifest api_version differs from package metadata".into());
    }
    if manifest.description != package.plugin.description {
        return Err("manifest description differs from package metadata".into());
    }
    if !has_package_metadata(&package.metadata) {
        return Ok(());
    }
    let metadata = PackageMetadata {
        manifest_sha256: sha256_hex(manifest_raw.as_bytes()),
        checksum_algorithm: "sha256".to_string(),
        package_tool: package.metadata.package_tool.clone(),
        capabilities: sorted_capabilities(&manifest),
        dependencies: version_req_map(&manifest.dependencies),
        optional_dependencies: version_req_map(&manifest.optional_dependencies),
        provides_services: sorted_service_keys(&manifest.provides_services),
        consumes_services: sorted_service_keys(&manifest.consumes_services),
    };
    if metadata.manifest_sha256 != package.metadata.manifest_sha256 {
        return Err("manifest checksum differs from package metadata".into());
    }
    if metadata.capabilities != package.metadata.capabilities {
        return Err("manifest capabilities differ from package metadata".into());
    }
    if metadata.dependencies != package.metadata.dependencies {
        return Err("manifest dependencies differ from package metadata".into());
    }
    if metadata.optional_dependencies != package.metadata.optional_dependencies {
        return Err("manifest optional dependencies differ from package metadata".into());
    }
    if metadata.provides_services != package.metadata.provides_services {
        return Err("manifest provided services differ from package metadata".into());
    }
    if metadata.consumes_services != package.metadata.consumes_services {
        return Err("manifest consumed services differ from package metadata".into());
    }
    Ok(())
}

fn has_package_metadata(metadata: &PackageMetadata) -> bool {
    !metadata.manifest_sha256.is_empty()
        || !metadata.capabilities.is_empty()
        || !metadata.dependencies.is_empty()
        || !metadata.optional_dependencies.is_empty()
        || !metadata.provides_services.is_empty()
        || !metadata.consumes_services.is_empty()
}

fn sorted_capabilities(manifest: &PluginManifest) -> Vec<String> {
    let mut capabilities = manifest
        .capabilities
        .iter()
        .cloned()
        .map(String::from)
        .collect::<Vec<_>>();
    capabilities.sort();
    capabilities
}

fn version_req_map<V: ToString>(
    map: &std::collections::HashMap<String, V>,
) -> BTreeMap<String, String> {
    map.iter()
        .map(|(id, req)| (id.clone(), req.to_string()))
        .collect()
}

fn sorted_service_keys(
    services: &[git_leviathan_plugin_api::manifest::ServiceDecl],
) -> Vec<String> {
    let mut keys = services
        .iter()
        .map(|service| service.key())
        .collect::<Vec<_>>();
    keys.sort();
    keys
}

fn package_key(package: &PluginPackage) -> String {
    format!("{}@{}", package.plugin.id, package.plugin.version)
}

fn update_registry_index(
    registry: &Path,
    package: &PluginPackage,
    target_path: &Path,
) -> DynResult<()> {
    fs::create_dir_all(registry)?;
    let index_path = registry.join("index.json");
    let mut index = read_registry_index_or_default(registry)?;
    let rel = normalize_relative(registry, target_path)?;
    let package_sha256 = sha256_hex(&fs::read(target_path)?);
    index.packages.insert(
        package_key(package),
        RegistryEntry {
            id: package.plugin.id.clone(),
            version: package.plugin.version.clone(),
            path: rel,
            root_checksum: package.root_checksum.clone(),
            package_sha256,
            capabilities: package.metadata.capabilities.clone(),
            dependencies: package.metadata.dependencies.clone(),
            optional_dependencies: package.metadata.optional_dependencies.clone(),
            signatures: signature_summaries(package),
        },
    );
    fs::write(index_path, serde_json::to_vec_pretty(&index)?)?;
    Ok(())
}

fn read_registry_index(path: &Path) -> DynResult<RegistryIndex> {
    let index_path = path.join("index.json");
    let index: RegistryIndex = serde_json::from_slice(&fs::read(&index_path)?)?;
    if index.format != REGISTRY_FORMAT {
        return Err(format!("unsupported registry format `{}`", index.format).into());
    }
    Ok(index)
}

fn read_registry_index_or_default(path: &Path) -> DynResult<RegistryIndex> {
    let index_path = path.join("index.json");
    if index_path.is_file() {
        read_registry_index(path)
    } else {
        Ok(RegistryIndex::default())
    }
}

fn revoke_registry_version(registry: &Path, spec: &str, reason: &str) -> DynResult<()> {
    let (id, version) = spec
        .split_once('@')
        .ok_or("revocation must use <id@version>")?;
    fs::create_dir_all(registry)?;
    let mut index = read_registry_index_or_default(registry)?;
    if !index
        .revocations
        .iter()
        .any(|revoked| revoked.id == id && revoked.version == version)
    {
        index.revocations.push(RegistryRevocation {
            id: id.to_string(),
            version: version.to_string(),
            reason: reason.to_string(),
        });
        index
            .revocations
            .sort_by(|a, b| (&a.id, &a.version).cmp(&(&b.id, &b.version)));
    }
    fs::write(
        registry.join("index.json"),
        serde_json::to_vec_pretty(&index)?,
    )?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct RegistryIndex {
    #[serde(default = "registry_format")]
    format: String,
    #[serde(default)]
    packages: BTreeMap<String, RegistryEntry>,
    #[serde(default)]
    revocations: Vec<RegistryRevocation>,
}

impl Default for RegistryIndex {
    fn default() -> Self {
        Self {
            format: REGISTRY_FORMAT.to_string(),
            packages: BTreeMap::new(),
            revocations: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct RegistryEntry {
    id: String,
    version: String,
    path: String,
    root_checksum: String,
    #[serde(default)]
    package_sha256: String,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    dependencies: BTreeMap<String, String>,
    #[serde(default)]
    optional_dependencies: BTreeMap<String, String>,
    #[serde(default)]
    signatures: Vec<RegistrySignature>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RegistryRevocation {
    id: String,
    version: String,
    #[serde(default)]
    reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegistrySignature {
    algorithm: String,
    trust_root: String,
    key_id: String,
}

fn signature_summaries(package: &PluginPackage) -> Vec<RegistrySignature> {
    package
        .signatures
        .iter()
        .map(|signature| RegistrySignature {
            algorithm: signature.algorithm.clone(),
            trust_root: signature.trust_root.clone(),
            key_id: signature.key_id.clone(),
        })
        .collect()
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct InstallLock {
    #[serde(default = "install_lock_format")]
    format: String,
    #[serde(default, rename = "plugin")]
    plugins: Vec<InstalledPackage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstalledPackage {
    id: String,
    version: String,
    source: String,
    checksum: String,
    root_checksum: String,
    package_path: String,
    capabilities: Vec<String>,
    dependencies: BTreeMap<String, String>,
    optional_dependencies: BTreeMap<String, String>,
    signatures: Vec<RegistrySignature>,
}

fn update_install_lock(
    lockfile: &Path,
    package: &PluginPackage,
    package_path: &Path,
    registry: Option<&Path>,
) -> DynResult<()> {
    let mut lock = if lockfile.is_file() {
        toml::from_str::<InstallLock>(&fs::read_to_string(lockfile)?)?
    } else {
        InstallLock {
            format: INSTALL_LOCK_FORMAT.to_string(),
            plugins: Vec::new(),
        }
    };
    let installed = InstalledPackage {
        id: package.plugin.id.clone(),
        version: package.plugin.version.clone(),
        source: registry
            .map(|path| format!("registry:{}", path.display()))
            .unwrap_or_else(|| "package".to_string()),
        checksum: format!("sha256:{}", package.root_checksum),
        root_checksum: package.root_checksum.clone(),
        package_path: package_path.display().to_string(),
        capabilities: package.metadata.capabilities.clone(),
        dependencies: package.metadata.dependencies.clone(),
        optional_dependencies: package.metadata.optional_dependencies.clone(),
        signatures: signature_summaries(package),
    };
    lock.plugins.retain(|entry| entry.id != package.plugin.id);
    lock.plugins.push(installed);
    lock.plugins.sort_by(|a, b| a.id.cmp(&b.id));
    if let Some(parent) = lockfile.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(lockfile, toml::to_string(&lock)?)?;
    Ok(())
}

fn package_from_path(path: &Path, verify: &VerifyContext) -> DynResult<PluginPackage> {
    let package = if path.is_dir() {
        build_package(path)?
    } else {
        read_package(path)?
    };
    verify_package_with_context(&package, verify)?;
    if !path.is_dir() {
        verify_package_file_against_registry(path, &package, verify)?;
    }
    Ok(package)
}

fn print_upgrade_plan(installed: &PluginPackage, candidate: &PluginPackage) {
    for line in upgrade_plan_lines(installed, candidate) {
        println!("{line}");
    }
}

fn upgrade_plan_lines(installed: &PluginPackage, candidate: &PluginPackage) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!("plugin: {}", candidate.plugin.id));
    if installed.plugin.id != candidate.plugin.id {
        lines.push(format!(
            "warning: installed id differs: {}",
            installed.plugin.id
        ));
    }
    lines.push(format!(
        "version: {} -> {}",
        installed.plugin.version, candidate.plugin.version
    ));
    lines.push(format!(
        "root_checksum: {} -> {}",
        installed.root_checksum, candidate.root_checksum
    ));
    lines.push(format!(
        "manifest_sha256: {} -> {}",
        installed.metadata.manifest_sha256, candidate.metadata.manifest_sha256
    ));
    lines.extend(set_change_lines(
        "capabilities",
        &installed.metadata.capabilities,
        &candidate.metadata.capabilities,
    ));
    lines.extend(map_change_lines(
        "dependencies",
        &installed.metadata.dependencies,
        &candidate.metadata.dependencies,
    ));
    lines.extend(map_change_lines(
        "optional_dependencies",
        &installed.metadata.optional_dependencies,
        &candidate.metadata.optional_dependencies,
    ));
    lines.push(format!(
        "signatures: {} -> {}",
        signature_list(installed),
        signature_list(candidate)
    ));
    lines
}

fn set_change_lines(label: &str, old: &[String], new: &[String]) -> Vec<String> {
    let old_set = old.iter().cloned().collect::<BTreeSet<_>>();
    let new_set = new.iter().cloned().collect::<BTreeSet<_>>();
    let added = new_set.difference(&old_set).cloned().collect::<Vec<_>>();
    let removed = old_set.difference(&new_set).cloned().collect::<Vec<_>>();
    vec![
        format!("{label}: {} -> {}", comma_list(old), comma_list(new)),
        format!("{label}_added: {}", comma_list(&added)),
        format!("{label}_removed: {}", comma_list(&removed)),
    ]
}

fn map_change_lines(
    label: &str,
    old: &BTreeMap<String, String>,
    new: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut lines = vec![format!("{label}: {} -> {}", map_list(old), map_list(new))];
    let keys = old
        .keys()
        .chain(new.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    if keys.is_empty() {
        lines.push(format!("{label}_changes: none"));
        return lines;
    }
    lines.push(format!("{label}_changes:"));
    for key in keys {
        match (old.get(&key), new.get(&key)) {
            (None, Some(value)) => lines.push(format!("  + {key} {value}")),
            (Some(value), None) => lines.push(format!("  - {key} {value}")),
            (Some(old_value), Some(new_value)) if old_value != new_value => {
                lines.push(format!("  ~ {key} {old_value} -> {new_value}"));
            }
            _ => {}
        }
    }
    lines
}

fn comma_list(items: &[String]) -> String {
    if items.is_empty() {
        "none".to_string()
    } else {
        items.join(", ")
    }
}

fn map_list(map: &BTreeMap<String, String>) -> String {
    if map.is_empty() {
        return "none".to_string();
    }
    map.iter()
        .map(|(key, value)| format!("{key} {value}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn signature_list(package: &PluginPackage) -> String {
    if package.signatures.is_empty() {
        return "none".to_string();
    }
    package
        .signatures
        .iter()
        .map(|sig| format!("{}:{}:{}", sig.algorithm, sig.trust_root, sig.key_id))
        .collect::<Vec<_>>()
        .join(", ")
}

fn package_files(root: &Path) -> DynResult<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    files.retain(|path| {
        let rel = normalize_relative(root, path).unwrap_or_default();
        !rel.starts_with("target/")
            && !rel.starts_with(".git/")
            && !rel.ends_with(&format!(".{PACKAGE_EXT}"))
    });
    files.sort_by_key(|path| normalize_relative(root, path).unwrap_or_default());
    Ok(files)
}

fn lua_source_files(root: &Path, include_tests: bool) -> DynResult<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    files.retain(|path| {
        path.extension() == Some(OsStr::new("lua"))
            && (include_tests
                || !path
                    .strip_prefix(root)
                    .is_ok_and(|rel| rel.starts_with("tests")))
    });
    files.sort();
    Ok(files)
}

fn lua_files_for_test(root: &Path) -> DynResult<Vec<PathBuf>> {
    let mut files = Vec::new();
    let init = root.join("init.lua");
    if init.is_file() {
        files.push(init);
    }
    let tests = root.join("tests");
    if tests.is_dir() {
        let mut test_files = Vec::new();
        collect_files(&tests, &tests, &mut test_files)?;
        test_files.retain(|path| path.extension() == Some(OsStr::new("lua")));
        test_files.sort();
        files.extend(test_files);
    }
    Ok(files)
}

fn collect_files(_root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> DynResult<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        if name == OsStr::new(".git") || name == OsStr::new("target") {
            continue;
        }
        if path.is_dir() {
            collect_files(_root, &path, out)?;
        } else if path.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

fn install_test_stubs(lua: &Lua, plugin_path: &Path) -> DynResult<()> {
    let globals = lua.globals();
    let leviathan = lua.create_table()?;
    globals.set("leviathan", leviathan.clone())?;

    for function in api::all_functions() {
        install_noop_function(lua, &leviathan, function.path)?;
    }

    let ui: Table = leviathan.get("ui")?;
    let region_fn = lua.create_function(|lua, _name: String| {
        let region = lua.create_table()?;
        for name in ["add", "remove", "replace"] {
            region.set(name, lua.create_function(|_, _: mlua::Value| Ok(()))?)?;
        }
        Ok(region)
    })?;
    ui.set("region", region_fn)?;

    let command: Table = leviathan.get("command")?;
    let commands = lua.create_table()?;
    globals.set("__xtask_commands", commands.clone())?;
    command.set(
        "create",
        lua.create_function(move |_, (name, _spec): (String, mlua::Value)| {
            commands.set(name, true)?;
            Ok(())
        })?,
    )?;
    command.set("invoke", lua.create_function(|_, _name: String| Ok(true))?)?;

    let fs_table: Table = leviathan.get("fs")?;
    let root = plugin_path.to_path_buf();
    fs_table.set(
        "read_file",
        lua.create_function(move |_, path: String| {
            let path = root.join(path);
            match fs::read_to_string(path) {
                Ok(content) => Ok((Some(content), Option::<String>::None)),
                Err(e) => Ok((Option::<String>::None, Some(e.to_string()))),
            }
        })?,
    )?;
    Ok(())
}

fn install_noop_function(lua: &Lua, root: &Table, path: &str) -> DynResult<()> {
    let path = path.strip_prefix("leviathan.").unwrap_or(path);
    let mut table = root.clone();
    let mut segments = path.split('.').peekable();
    while let Some(segment) = segments.next() {
        if segments.peek().is_none() {
            table.set(
                segment,
                lua.create_function(|_, _: mlua::MultiValue| Ok(true))?,
            )?;
        } else {
            let next = match table.get::<mlua::Value>(segment)? {
                mlua::Value::Table(t) => t,
                mlua::Value::Nil => {
                    let fresh = lua.create_table()?;
                    table.set(segment, fresh.clone())?;
                    fresh
                }
                _ => return Err(format!("cannot install stub at `{path}`").into()),
            };
            table = next;
        }
    }
    Ok(())
}

fn set_package_path(lua: &Lua, root: &Path) -> DynResult<()> {
    let package: Table = lua.globals().get("package")?;
    let existing: String = package.get("path")?;
    let lua_root = root.join("lua");
    let extra = format!(
        "{};{}",
        lua_root.join("?.lua").display(),
        lua_root.join("?/init.lua").display()
    );
    package.set("path", format!("{extra};{existing}"))?;
    Ok(())
}

fn has_docs(path: &Path) -> bool {
    path.join("README.md").is_file()
        || markdown_in_dir(&path.join("docs"))
        || markdown_in_dir(&path.join("doc"))
}

fn has_tests(path: &Path) -> bool {
    let tests = path.join("tests");
    if !tests.is_dir() {
        return false;
    }
    let Ok(mut files) = package_files(&tests) else {
        return false;
    };
    files.retain(|path| matches!(path.extension().and_then(OsStr::to_str), Some("lua" | "md")));
    !files.is_empty()
}

fn markdown_in_dir(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    let Ok(files) = package_files(path) else {
        return false;
    };
    files
        .iter()
        .any(|path| path.extension() == Some(OsStr::new("md")))
}

fn validate_plugin_id(id: &str) -> DynResult<()> {
    if id.is_empty() {
        return Err("plugin id cannot be empty".into());
    }
    if !id
        .bytes()
        .all(|b| b == b'_' || b == b'-' || b == b'.' || b.is_ascii_alphanumeric())
    {
        return Err(format!(
            "plugin id `{id}` may only contain ASCII letters, numbers, dot, dash, and underscore"
        )
        .into());
    }
    Ok(())
}

fn single_optional_path(args: &[String], usage: &str) -> DynResult<PathBuf> {
    if args.len() > 1 || args.first().is_some_and(|arg| arg.starts_with("--")) {
        return Err(usage.into());
    }
    Ok(args
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".")))
}

fn normalize_relative(root: &Path, path: &Path) -> DynResult<String> {
    let rel = path.strip_prefix(root)?;
    Ok(rel
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/"))
}

fn validate_package_path(path: &str) -> DynResult<()> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.split('/').any(|part| part == ".." || part.is_empty())
    {
        return Err(format!("invalid package path `{path}`").into());
    }
    Ok(())
}

fn path_label(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn title_case_id(id: &str) -> String {
    id.split(['_', '-', '.'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_checksum_changes_when_file_checksum_changes() {
        let files = vec![PackageFile {
            path: "init.lua".to_string(),
            size: 3,
            sha256: "abc".to_string(),
            content_base64: "eHl6".to_string(),
        }];
        let first = root_checksum(&files);
        let mut changed = files;
        changed[0].sha256 = "def".to_string();
        assert_ne!(first, root_checksum(&changed));
    }

    #[test]
    fn tampered_package_content_is_rejected() {
        let dir = fixture_plugin("tamper_a", "0.1.0", &[], &[]);
        let mut package = build_package(&dir).expect("package");
        let file = package
            .files
            .iter_mut()
            .find(|file| file.path == "init.lua")
            .expect("init.lua");
        file.content_base64 = base64::engine::general_purpose::STANDARD.encode("tampered");

        let err = verify_package_with_context(&package, &VerifyContext::default())
            .expect_err("tampered package must fail");
        let message = err.to_string();
        assert!(message.contains("checksum mismatch") || message.contains("size mismatch"));
        remove_fixture(dir);
    }

    #[test]
    fn revoked_registry_version_is_rejected() {
        let dir = fixture_plugin("revoked_a", "1.0.0", &[], &[]);
        let package = build_package(&dir).expect("package");
        let context = VerifyContext {
            registry: Some(RegistryIndex {
                format: REGISTRY_FORMAT.to_string(),
                packages: BTreeMap::new(),
                revocations: vec![RegistryRevocation {
                    id: "revoked_a".to_string(),
                    version: "1.0.0".to_string(),
                    reason: "test".to_string(),
                }],
            }),
            ..VerifyContext::default()
        };

        let err =
            verify_package_with_context(&package, &context).expect_err("revoked package must fail");
        assert!(err.to_string().contains("revoked"));
        remove_fixture(dir);
    }

    #[test]
    fn upgrade_plan_shows_changed_capabilities() {
        let old_dir = fixture_plugin("cap_plan", "0.1.0", &["ui:graph_decoration"], &[]);
        let new_dir = fixture_plugin(
            "cap_plan",
            "0.2.0",
            &["ui:graph_decoration", "ui:diff_decoration"],
            &[("helper", "^1")],
        );
        let old = build_package(&old_dir).expect("old package");
        let new = build_package(&new_dir).expect("new package");

        let lines = upgrade_plan_lines(&old, &new);
        assert!(lines
            .iter()
            .any(|line| line == "capabilities_added: ui:diff_decoration"));
        assert!(lines.iter().any(|line| line == "  + helper ^1"));
        remove_fixture(old_dir);
        remove_fixture(new_dir);
    }

    #[test]
    fn signed_package_requires_known_trust_root() {
        let dir = fixture_plugin("signed_a", "0.1.0", &[], &[]);
        let mut package = build_package(&dir).expect("package");
        sign_package(&mut package, b"dev-key", "local-dev", "dev").expect("sign");
        let unknown = TrustRoots {
            format: TRUST_ROOT_FORMAT.to_string(),
            keys: vec![TrustKey {
                trust_root: "other".to_string(),
                key_id: "dev".to_string(),
                key_base64: base64::engine::general_purpose::STANDARD.encode("dev-key"),
            }],
            revoked_signatures: Vec::new(),
        };
        let context = VerifyContext {
            trust_roots: Some(unknown),
            require_signature: true,
            ..VerifyContext::default()
        };

        let err = verify_package_with_context(&package, &context)
            .expect_err("unknown trust root must fail");
        assert!(err.to_string().contains("unknown signature trust root"));
        remove_fixture(dir);
    }

    fn fixture_plugin(
        id: &str,
        version: &str,
        capabilities: &[&str],
        dependencies: &[(&str, &str)],
    ) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "git-leviathan-xtask-{id}-{version}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("tests")).expect("tests dir");
        let caps = if capabilities.is_empty() {
            String::new()
        } else {
            format!(
                "capabilities = [{}]\n",
                capabilities
                    .iter()
                    .map(|cap| format!("\"{cap}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let deps = if dependencies.is_empty() {
            String::new()
        } else {
            let body = dependencies
                .iter()
                .map(|(id, req)| format!("{id} = \"{req}\""))
                .collect::<Vec<_>>()
                .join("\n");
            format!("\n[dependencies]\n{body}\n")
        };
        fs::write(
            dir.join("plugin.toml"),
            format!(
                "id = \"{id}\"\nname = \"{id}\"\nversion = \"{version}\"\napi_version = \"1.0\"\n{caps}{deps}"
            ),
        )
        .expect("manifest");
        fs::write(dir.join("init.lua"), "leviathan.log('ok')\n").expect("init");
        fs::write(dir.join("README.md"), "# fixture\n").expect("readme");
        fs::write(dir.join("tests").join("smoke.lua"), "assert(true)\n").expect("test");
        dir
    }

    fn remove_fixture(dir: PathBuf) {
        let _ = fs::remove_dir_all(dir);
    }
}
