use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) const READ_FILE_BYTE_LIMIT: u64 = 8 * 1024 * 1024;
pub(super) const WRITE_FILE_BYTE_LIMIT: u64 = 8 * 1024 * 1024;
pub(super) const APPEND_FILE_BYTE_LIMIT: u64 = 8 * 1024 * 1024;
pub(super) const COPY_BYTE_LIMIT: u64 = 8 * 1024 * 1024;

pub(super) fn read_file(path: &str) -> Result<String, String> {
    let p = Path::new(path);
    let meta = fs::metadata(p).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err(format!("not a regular file: {path}"));
    }
    if meta.len() > READ_FILE_BYTE_LIMIT {
        return Err(format!(
            "file exceeds {READ_FILE_BYTE_LIMIT}-byte read_file limit: {path}"
        ));
    }
    fs::read_to_string(p).map_err(|e| e.to_string())
}

pub(super) fn copy_file(src: &str, dst: &str) -> Result<(), String> {
    let meta = fs::symlink_metadata(src).map_err(|e| e.to_string())?;
    if !meta.file_type().is_file() {
        return Err(format!("not a regular file: {src}"));
    }
    if meta.len() > COPY_BYTE_LIMIT {
        return Err(format!(
            "source exceeds {COPY_BYTE_LIMIT}-byte copy limit: {src}"
        ));
    }
    fs::copy(src, dst).map(|_| ()).map_err(|e| e.to_string())
}

pub(super) fn write_file(path: &str, content: &str) -> Result<(), String> {
    if content.len() as u64 > WRITE_FILE_BYTE_LIMIT {
        return Err(format!(
            "content exceeds {WRITE_FILE_BYTE_LIMIT}-byte write_file limit: {path}"
        ));
    }
    fs::write(path, content).map_err(|e| e.to_string())
}

pub(super) fn append_file(path: &str, content: &str) -> Result<(), String> {
    if content.len() as u64 > APPEND_FILE_BYTE_LIMIT {
        return Err(format!(
            "content exceeds {APPEND_FILE_BYTE_LIMIT}-byte append_file limit: {path}"
        ));
    }
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    f.write_all(content.as_bytes()).map_err(|e| e.to_string())
}

pub(super) fn touch(path: &str) -> Result<(), String> {
    let f = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
        .map_err(|e| e.to_string())?;
    let times = fs::FileTimes::new().set_modified(SystemTime::now());
    f.set_times(times).map_err(|e| e.to_string())
}

#[cfg_attr(test, derive(Debug))]
pub(super) struct Entry {
    pub(super) name: String,
    pub(super) path: String,
    pub(super) is_dir: bool,
    pub(super) is_symlink: bool,
    pub(super) size: u64,
    pub(super) modified: i64,
}

pub(super) fn metadata(path: &str) -> Result<Entry, String> {
    let p = Path::new(path);
    let meta = fs::symlink_metadata(p).map_err(|e| e.to_string())?;
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let file_type = meta.file_type();
    let is_symlink = file_type.is_symlink();
    let is_dir = file_type.is_dir();
    let size = if is_dir { 0 } else { meta.len() };
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    Ok(Entry {
        name,
        path: path.to_string(),
        is_dir,
        is_symlink,
        size,
        modified,
    })
}

pub(super) fn list_dir(path: &str) -> Result<Vec<Entry>, String> {
    let p = PathBuf::from(path);
    let rd = fs::read_dir(&p).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for entry in rd.flatten() {
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let full = entry.path().to_string_lossy().into_owned();
        let file_type = meta.file_type();
        let is_symlink = file_type.is_symlink();
        let is_dir = file_type.is_dir();
        let size = if is_dir { 0 } else { meta.len() };
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        out.push(Entry {
            name,
            path: full,
            is_dir,
            is_symlink,
            size,
            modified,
        });
    }
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
    Ok(out)
}
