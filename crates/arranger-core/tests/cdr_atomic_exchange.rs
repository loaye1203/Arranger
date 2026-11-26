use arranger_core::{cdr, native, Command, Document, Service, command::ExportFormat};
use serde_json::{json, Value};
use std::{fs, path::{Path, PathBuf}, sync::atomic::{AtomicU64, Ordering}};

fn directory() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../artifacts/cdr-atomic-exchange/{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir_all(&path).unwrap(); path.canonicalize().unwrap()
}
fn sidecar(path: &Path, suffix: &str) -> PathBuf { PathBuf::from(format!("{}{suffix}", path.display())) }
fn scene(x: f64, opacity: f64) -> Document {
    serde_json::from_value(json!({"id":"restored","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"objects":[
        {"id":"group","page_id":"page-1","x_mm":0.,"y_mm":0.,"width_mm":0.,"height_mm":0.,"style":{"fill":"none","stroke":"none","stroke_width_mm":0.,"opacity":opacity},"geometry":{"kind":"group"}},
        {"id":"rect","parent_id":"group","page_id":"page-1","x_mm":x,"y_mm":30.,"width_mm":40.,"height_mm":20.,"geometry":{"kind":"rect"}}
    ],"resources":{}})).unwrap()
}
fn encoded(document: &Document, version: u32) -> Vec<u8> {
    if version == 14 { cdr::encode_document(document) } else { cdr::encode_document21(document) }.unwrap().0
}
fn api(service: &Service, command: Value) -> arranger_core::Response {
    service.execute_json(&json!({"protocol_version":1,"command":command}).to_string())
}

#[test]
fn cdr14_and_isolated_group21_atomic_replace_keep_exact_previous_bytes() {
    for (version, opacity) in [(14, 1.), (21, 1.), (21, 0.7)] {
        let path = directory().join("交换.cdr");
        let first = encoded(&scene(20., opacity), version);
        let next = encoded(&scene(22., opacity), version);
        native::write_export(&path, &first, false).unwrap();
        assert!(!sidecar(&path, ".bak").exists());
        assert_eq!(native::write_export(&path, &next, false).unwrap_err().code, "OUTPUT_EXISTS");
        native::write_export(&path, &next, true).unwrap();
        assert_eq!(fs::read(&path).unwrap(), next);
        assert_eq!(fs::read(sidecar(&path, ".bak")).unwrap(), first);
        assert!(!sidecar(&path, ".pending").exists()); assert!(!sidecar(&path, ".bak.pending").exists());
        let (restored, notices) = native::recover_cdr_backup(&path, "restored").unwrap();
        assert_eq!(restored, scene(20., opacity));
        assert!(notices.iter().any(|n| n.code == "CDR_BACKUP_RECOVERED"));
    }
}

#[test]
fn invalid_current_or_recovery_bytes_are_preserved_before_lock_or_pending_writes() {
    let valid = encoded(&scene(20., 1.), 21); let next = encoded(&scene(22., 1.), 21);
    for suffix in ["", ".bak", ".pending", ".bak.pending"] {
        let path = directory().join("keep.cdr"); fs::write(&path, &valid).unwrap();
        fs::write(sidecar(&path, suffix), b"keep damaged evidence").unwrap();
        let before: Vec<_> = ["", ".bak", ".pending", ".bak.pending"].iter().map(|s| fs::read(sidecar(&path, s)).ok()).collect();
        assert!(native::write_export(&path, &next, true).is_err());
        let after: Vec<_> = ["", ".bak", ".pending", ".bak.pending"].iter().map(|s| fs::read(sidecar(&path, s)).ok()).collect();
        assert_eq!(before, after); assert!(!sidecar(&path, ".lock").exists());
    }
}

#[test]
fn directory_sidecars_and_invalid_targets_fail_without_replacing_primary() {
    let valid = encoded(&scene(20., 1.), 14); let next = encoded(&scene(22., 1.), 14);
    for suffix in [".bak", ".pending", ".bak.pending", ".lock"] {
        let path = directory().join("keep.cdr"); fs::write(&path, &valid).unwrap();
        fs::create_dir(sidecar(&path, suffix)).unwrap();
        assert_eq!(native::write_export(&path, &next, true).unwrap_err().code, "IO_ERROR");
        assert_eq!(fs::read(&path).unwrap(), valid); assert!(sidecar(&path, suffix).is_dir());
    }
    let path = directory().join("directory.cdr"); fs::create_dir(&path).unwrap();
    assert_eq!(native::write_export(&path, &next, true).unwrap_err().code, "IO_ERROR");
    assert_eq!(fs::read_dir(&path).unwrap().count(), 0);
}

#[test]
fn unsupported_source_and_invalid_new_encoding_touch_no_primary_or_sidecar() {
    let path = directory().join("keep.cdr");
    for suffix in ["", ".bak", ".pending", ".bak.pending", ".lock"] { fs::write(sidecar(&path, suffix), format!("evidence{suffix}")).unwrap(); }
    assert_eq!(cdr::export_document(&scene(20., 0.7), &path, true).unwrap_err().code, "CDR_UNSUPPORTED");
    assert!(native::write_export(&path, b"broken new encoding", true).is_err());
    for suffix in ["", ".bak", ".pending", ".bak.pending", ".lock"] { assert_eq!(fs::read_to_string(sidecar(&path, suffix)).unwrap(), format!("evidence{suffix}")); }
}

#[test]
fn both_cdr_export_api_routes_reject_owned_transactions_then_release_save_lease_on_errors() {
    let service = Service::new(); service.command(Command::DocumentNew { document_id: "main".into(), width_mm: 210., height_mm: 297., unit: "mm".into() }).unwrap();
    let token = service.command(Command::Begin { document_id: "main".into(), expected_revision: 0, unit: "mm".into(), lease_ms: 60000 }).unwrap()["transaction_id"].as_str().unwrap().to_owned();
    let path = directory().join("busy.cdr");
    for version in ["cdr14", "cdr21"] {
        let reply = api(&service, json!({"type":"cdr_export","document_id":"main","path":path,"format":version}));
        assert_eq!(reply.error.unwrap().code, "DOCUMENT_BUSY");
    }
    for format in [ExportFormat::Cdr14, ExportFormat::Cdr21] {
        let error = service.command(Command::Export { document_id: "main".into(), page_id: "page-1".into(), format, path: path.to_string_lossy().into(), overwrite: false, dpi: 96. }).unwrap_err();
        assert_eq!(error.code, "DOCUMENT_BUSY");
    }
    assert!(!path.exists()); assert!(!sidecar(&path, ".lock").exists());
    service.command(Command::Rollback { document_id: "main".into(), transaction_id: token }).unwrap();
    let before = serde_json::to_value(service.snapshot("main").unwrap()).unwrap();
    assert!(!api(&service, json!({"type":"cdr_export","document_id":"main","path":path,"format":"invalid"})).ok);
    assert_eq!(before, serde_json::to_value(service.snapshot("main").unwrap()).unwrap());
    for version in ["cdr14", "cdr21"] {
        let path = directory().join("ok.cdr");
        let reply = api(&service, json!({"type":"cdr_export","document_id":"main","path":path,"format":version})); assert!(reply.ok, "{reply:?}");
        assert_eq!(before, serde_json::to_value(service.snapshot("main").unwrap()).unwrap());
    }
}

#[test]
fn explicit_backup_recovery_keeps_files_unmodified_and_document_unsaved_unbound() {
    for (version, opacity) in [(14, 1.), (21, 0.7)] {
        let path = directory().join("damaged.cdr"); let bytes = encoded(&scene(20., opacity), version);
        fs::write(&path, b"damaged primary").unwrap(); fs::write(sidecar(&path, ".bak"), &bytes).unwrap();
        fs::write(sidecar(&path, ".pending"), encoded(&scene(99., opacity), version)).unwrap();
        let service = Service::new();
        let response = api(&service, json!({"type":"cdr_recover_backup","document_id":"restored","path":path})); assert!(response.ok, "{response:?}");
        let snapshot = service.snapshot("restored").unwrap(); assert!(snapshot.dirty); assert!(snapshot.saved_path.is_none()); assert_eq!(*snapshot.document, scene(20., opacity));
        assert_eq!(fs::read(&path).unwrap(), b"damaged primary"); assert_eq!(fs::read(sidecar(&path, ".bak")).unwrap(), bytes);
        assert!(!sidecar(&path, ".lock").exists());
        assert_eq!(service.command(Command::Save { document_id: "restored".into(), expected_revision: 0, path: path.to_string_lossy().into(), overwrite: true }).unwrap_err().code, "CDR_SAVE_NOT_READY");
        assert!(!api(&service, json!({"type":"document_close","document_id":"restored","expected_revision":0})).ok);
    }
}

#[test]
fn failed_backup_recovery_is_atomic_and_never_recovers_native_json_or_pending_file() {
    let path = directory().join("damaged.cdr"); let valid = encoded(&scene(20., 1.), 21);
    fs::write(&path, &valid).unwrap(); fs::write(sidecar(&path, ".pending"), &valid).unwrap();
    let service = Service::new();
    assert!(!api(&service, json!({"type":"cdr_recover_backup","document_id":"missing","path":path})).ok);
    let native_json = directory().join("source.arranger"); native::save(&scene(20., 1.), 0, &native_json, None, false).unwrap();
    fs::copy(native_json, sidecar(&path, ".bak")).unwrap();
    let response = api(&service, json!({"type":"cdr_recover_backup","document_id":"bad","path":path})); assert_eq!(response.error.unwrap().code, "UNRECOVERABLE_FILE");
    assert_eq!(service.command(Command::Documents).unwrap()["documents"], json!([])); assert_eq!(fs::read(&path).unwrap(), valid);
}

#[test]
fn cooperative_writer_lock_rejects_then_releases_without_modifying_primary() {
    let path = directory().join("locked.cdr"); let valid = encoded(&scene(20., 1.), 21); fs::write(&path, &valid).unwrap();
    let lock = fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(sidecar(&path, ".lock")).unwrap(); lock.try_lock().unwrap();
    assert_eq!(native::write_export(&path, &valid, true).unwrap_err().code, "FILE_BUSY"); assert_eq!(fs::read(&path).unwrap(), valid);
    drop(lock); native::write_export(&path, &valid, true).unwrap(); assert_eq!(fs::read(sidecar(&path, ".bak")).unwrap(), valid);
}
