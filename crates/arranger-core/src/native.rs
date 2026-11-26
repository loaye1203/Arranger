use crate::{Document, Error, Result, command::Notice, model::sha256};
use serde::{Deserialize, Serialize};
use std::{fs::{self, File, OpenOptions}, io::Write, path::{Path, PathBuf}};

#[derive(Clone)]
pub struct FileBinding { pub path: PathBuf, pub disk_hash: String }
pub struct Loaded {
    pub document: Document,
    pub revision: u64,
    pub binding: FileBinding,
    pub warnings: Vec<Notice>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeFile {
    format: String,
    format_version: u32,
    saved_revision: u64,
    document_sha256: String,
    document: Document,
}

pub fn normalize(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() { return Err(Error::new("INVALID_PATH", "File paths must be absolute")); }
    if path.exists() { return fs::canonicalize(path).map_err(|e| Error::io("Resolve file", e)); }
    let parent = path.parent().ok_or_else(|| Error::new("INVALID_PATH", "Missing parent directory"))?;
    let parent = fs::canonicalize(parent).map_err(|e| Error::io("Resolve parent directory", e))?;
    Ok(parent.join(path.file_name().ok_or_else(|| Error::new("INVALID_PATH", "Missing file name"))?))
}
fn suffix(path: &Path, text: &str) -> PathBuf {
    let mut p = path.as_os_str().to_owned(); p.push(text); PathBuf::from(p)
}
fn file_lock(path: &Path) -> Result<File> {
    let file = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(suffix(path, ".lock"))
        .map_err(|e| Error::io("Open output lock", e))?;
    file.try_lock().map_err(|_| Error::new("FILE_BUSY", "Another process is writing this file"))?;
    Ok(file)
}
fn synced_write(path: &Path, data: &[u8]) -> Result<()> {
    let mut file = File::create(path).map_err(|e| Error::io("Create temporary file", e))?;
    file.write_all(data).and_then(|_| file.sync_all()).map_err(|e| Error::io("Write and sync temporary file", e))
}
fn decode(data: &[u8]) -> Result<NativeFile> {
    let header: serde_json::Value = serde_json::from_slice(data).map_err(|_| Error::new("CORRUPT_FILE", "Native file is not valid JSON"))?;
    if header["format"] != "arranger" || !matches!(header["format_version"].as_u64(),Some(1|2|3|4|5)) {
        return Err(Error::new("UNSUPPORTED_FILE_VERSION", "This is not a supported Arranger native format version"));
    }
    let file: NativeFile = serde_json::from_value(header).map_err(|e| Error::new("CORRUPT_FILE", e.to_string()))?;
    if crate::paint::has_cmyk(&file.document) && file.format_version<3 {return Err(Error::new("UNSUPPORTED_FILE_VERSION","CMYK paints require Arranger native version 3 or later; older readers must not recover an RGB backup"));}
    if file.document.objects.iter().any(|o|!o.style.stroke_scale_with_object)&&file.format_version<4{return Err(Error::new("UNSUPPORTED_FILE_VERSION","Fixed-world strokes require Arranger native version 4"));}
    if file.document.objects.iter().any(|o|o.style.stroke_fixed_nib.is_some())&&file.format_version<5{return Err(Error::new("UNSUPPORTED_FILE_VERSION","Fixed elliptical nibs require Arranger native version5"));}
    file.document.validate()?;
    if file.document.hash() != file.document_sha256 { return Err(Error::new("CHECKSUM_MISMATCH", "Native document checksum does not match")); }
    Ok(file)
}

pub fn open(path: &Path) -> Result<Loaded> {
    let path = normalize(path)?;
    let data = fs::read(&path).map_err(|e| Error::io("Open native document", e))?;
    if path.extension().and_then(|s| s.to_str()).is_some_and(|s| s.eq_ignore_ascii_case("cdr")) {
        return Err(Error::new("CDR_IMPORT_NOT_READY", "Formal CDR import is not implemented; this file was not loaded or recovered from an Arranger backup"));
    }
    let mut warnings = vec![];
    let file = match decode(&data) {
        Ok(file) => file,
        Err(e) if e.code == "UNSUPPORTED_FILE_VERSION" => return Err(e),
        Err(original) => {
            let backup = fs::read(suffix(&path, ".bak")).map_err(|_| Error::new("UNRECOVERABLE_FILE", format!("{original}; no readable backup")))?;
            let recovered = decode(&backup).map_err(|e| Error::new("UNRECOVERABLE_FILE", format!("{original}; backup: {e}")))?;
            warnings.push(Notice::new("RECOVERED_BACKUP", "The primary file is damaged. A verified backup was loaded; unsaved edits may be lost."));
            recovered
        }
    };
    if suffix(&path, ".pending").exists() {
        warnings.push(Notice::new("INTERRUPTED_SAVE", "An unfinished save was found. The primary file is the commit point; the pending file was ignored."));
    }
    Ok(Loaded { document: file.document, revision: file.saved_revision, binding: FileBinding { path, disk_hash: sha256(&data) }, warnings })
}

/// Explicit source recovery is detached from both the original and its backup.
/// Read exactly the selected .arranger.bak (or the backup of an .arranger path),
/// using the same complete-model/version/checksum validation as a normal open.
pub fn recover_source_backup(path:&Path)->Result<(Document,u64,Vec<Notice>)>{
    let name=path.file_name().unwrap_or_default().to_string_lossy().to_ascii_lowercase();
    let backup=if name.ends_with(".arranger.bak"){normalize(path)?}
        else if path.extension().and_then(|e|e.to_str()).is_some_and(|e|e.eq_ignore_ascii_case("arranger")){suffix(&normalize(path)?,".bak")}
        else{return Err(Error::new("INVALID_PATH","Choose an .arranger original or its .arranger.bak backup"));};
    let bytes=fs::read(&backup).map_err(|e|Error::io("Read source backup",e))?;
    let file=decode(&bytes)?;
    Ok((file.document,file.saved_revision,vec![Notice::new("SOURCE_BACKUP_RECOVERED","A verified source backup was loaded as an unsaved, unbound document. Original, backup and pending files were not changed; save a new .arranger source.")]))
}

pub fn save(document: &Document, revision: u64, path: &Path, binding: Option<&FileBinding>, overwrite: bool) -> Result<FileBinding> {
    if path.extension().and_then(|s| s.to_str()).is_some_and(|s| s.eq_ignore_ascii_case("cdr")) {
        document.validate()?;
        return Err(Error::new("CDR_SAVE_NOT_READY", "Complete editable source saves as .arranger. For a CDR delivery copy use cdr_preflight and cdr_export with explicit editable/appearance mode; legacy save_check describes the strict native contract."));
    }
    if path.extension().and_then(|s| s.to_str()).is_none_or(|s| !s.eq_ignore_ascii_case("arranger")) {
        return Err(Error::new("INVALID_PATH", "Native documents use the .arranger extension"));
    }
    document.validate()?;
    let path = normalize(path)?;
    let _lock = file_lock(&path)?;
    let existing = read_optional(&path)?;
    let same_binding = binding.filter(|b| b.path == path);
    if let Some(binding) = same_binding {
        if existing.as_ref().map(|d| sha256(d)) != Some(binding.disk_hash.clone()) {
            return Err(Error::new("DISK_CONFLICT", "The file changed or disappeared after it was opened; save to a different path"));
        }
    } else if existing.is_some() && !overwrite {
        return Err(Error::new("OUTPUT_EXISTS", "The output already exists; explicitly allow overwrite or choose another path"));
    }
    let file = NativeFile { format: "arranger".into(), format_version: if document.objects.iter().any(|o|o.style.stroke_fixed_nib.is_some()){5}else if document.objects.iter().any(|o|!o.style.stroke_scale_with_object){4}else if crate::paint::has_cmyk(document){3}else if document.objects.iter().any(|o|o.page_id==crate::WORKSPACE_PAGE){2}else{1}, saved_revision: revision, document_sha256: document.hash(), document: document.clone() };
    let data = serde_json::to_vec_pretty(&file).map_err(|e| Error::new("SERIALIZATION_ERROR", e.to_string()))?;
    let pending = suffix(&path, ".pending");
    synced_write(&pending, &data)?;
    checkpoint("pending");
    if let Some(previous) = existing.as_ref().filter(|bytes| decode(bytes).is_ok()) {
        let temp = suffix(&path, ".bak.pending");
        synced_write(&temp, previous)?;
        fs::rename(&temp, suffix(&path, ".bak")).map_err(|e| Error::io("Replace backup", e))?;
    }
    checkpoint("backup");
    fs::rename(&pending, &path).map_err(|e| Error::io("Commit native save", e))?;
    checkpoint("committed");
    Ok(FileBinding { path, disk_hash: sha256(&data) })
}

pub fn write_export(path: &Path, data: &[u8], overwrite: bool) -> Result<PathBuf> {
    if path.extension().and_then(|s| s.to_str()).is_some_and(|s| s.eq_ignore_ascii_case("cdr")) {
        return write_cdr_exchange(path, data, overwrite);
    }
    let path = normalize(path)?;
    let _lock = file_lock(&path)?;
    if path.exists() && !overwrite { return Err(Error::new("OUTPUT_EXISTS", "Output exists; overwrite must be explicit")); }
    let pending = suffix(&path, ".pending");
    synced_write(&pending, data)?;
    fs::rename(&pending, &path).map_err(|e| Error::io("Commit export", e))?;
    Ok(path)
}

fn cdr_bytes(data: &[u8]) -> Result<()> {
    // This is a structural subset check, never a Corel compatibility verdict.
    crate::cdr::decode(data, "exchange-verification").map(|_| ())
}
fn exchange_files(path: &Path, overwrite: bool) -> Result<Vec<Option<Vec<u8>>>> {
    let current = read_optional(path)?;
    if current.is_some() && !overwrite {
        return Err(Error::new("OUTPUT_EXISTS", "Output exists; overwrite must be explicit"));
    }
    let mut files = vec![];
    for (index, candidate) in [path.to_path_buf(), suffix(path, ".bak"), suffix(path, ".pending"), suffix(path, ".bak.pending")].iter().enumerate() {
        let bytes = if index == 0 { current.clone() } else { read_optional(candidate)? };
        if let Some(bytes) = &bytes {
            cdr_bytes(bytes).map_err(|e| Error::new(&e.code, format!("Existing CDR file {} was left unchanged: {e}", candidate.display())))?;
        }
        files.push(bytes);
    }
    Ok(files)
}
fn write_cdr_exchange(path: &Path, data: &[u8], overwrite: bool) -> Result<PathBuf> {
    // Finish encoding and verification before creating even the output lock.
    cdr_bytes(data)?;
    let path = normalize(path)?;
    let before = exchange_files(&path, overwrite)?;
    let _lock = file_lock(&path)?;
    if exchange_files(&path, overwrite)? != before {
        return Err(Error::new("DISK_CONFLICT", "The CDR or a recovery file changed while the output lock was being acquired"));
    }
    let pending = suffix(&path, ".pending");
    synced_write(&pending, data)?;
    checkpoint("cdr_pending");
    if let Some(previous) = &before[0] {
        let temporary_backup = suffix(&path, ".bak.pending");
        synced_write(&temporary_backup, previous)?;
        fs::rename(&temporary_backup, suffix(&path, ".bak")).map_err(|e| Error::io("Replace CDR exchange backup", e))?;
    }
    checkpoint("cdr_backup");
    fs::rename(&pending, &path).map_err(|e| Error::io("Commit CDR exchange", e))?;
    checkpoint("cdr_committed");
    Ok(path)
}

pub fn recover_cdr_backup(path: &Path, document_id: &str) -> Result<(Document, Vec<Notice>)> {
    if path.extension().and_then(|s| s.to_str()).is_none_or(|s| !s.eq_ignore_ascii_case("cdr")) {
        return Err(Error::new("INVALID_PATH", "Select the original .cdr path to recover its .cdr.bak file"));
    }
    let path = normalize(path)?;
    let backup = suffix(&path, ".bak");
    let bytes = fs::read(&backup).map_err(|e| Error::io("Read CDR exchange backup", e))?;
    let (document, mut warnings) = crate::cdr::decode(&bytes, document_id)
        .map_err(|e| Error::new("UNRECOVERABLE_FILE", format!("CDR backup is not readable: {e}")))?;
    warnings.push(Notice::new("CDR_BACKUP_RECOVERED", "The verified previous CDR exchange was imported into an unsaved, unbound document. The primary and backup files were not changed; save an .arranger source or explicitly export to a new path."));
    Ok((document, warnings))
}
fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::io("Read current output", e)),
    }
}

// Deterministic process-termination probes are absent from normal releases.
#[cfg(not(feature="test-hooks"))]
fn checkpoint(_: &str) {}
#[cfg(feature="test-hooks")]
fn checkpoint(stage: &str) {
    if std::env::var("ARRANGER_TEST_SAVE_CHECKPOINT").ok().as_deref()!=Some(stage) {return;}
    if let Ok(marker)=std::env::var("ARRANGER_TEST_SAVE_MARKER") {
        if fs::write(&marker,stage).is_ok() {
            // The harness kills this process after observing the checkpoint.
            // Bound the wait so a forgotten test environment cannot hang forever.
            for _ in 0..300 {
                if Path::new(&format!("{marker}.resume")).exists() {break;}
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
}
