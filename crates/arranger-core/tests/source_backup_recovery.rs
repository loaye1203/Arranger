use arranger_core::{Service,Command,native};
use serde_json::{json,Value};
use std::{fs,path::PathBuf};
fn command(s:&Service,v:Value)->arranger_core::Result<Value>{s.command(serde_json::from_value(v).unwrap())}
fn fixture(name:&str)->(PathBuf,arranger_core::Document){
 let dir=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../artifacts/source-recovery-{}-{name}",std::process::id()));fs::create_dir_all(&dir).unwrap();
 let source=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/assets/cdr/all-content.arranger");let doc=native::open(&source.canonicalize().unwrap()).unwrap().document;
 let original=dir.canonicalize().unwrap().join("原稿.arranger");let binding=native::save(&doc,7,&original,None,false).unwrap();let mut changed=doc.clone();changed.pages[0].width_mm+=1.;native::save(&changed,8,&original,Some(&binding),false).unwrap();(original,doc)
}
fn backup(p:&std::path::Path)->PathBuf{PathBuf::from(format!("{}.bak",p.display()))}
fn recover(s:&Service,id:&str,p:&std::path::Path)->arranger_core::Result<Value>{command(s,json!({"type":"document_recover_backup","document_id":id,"path":p}))}
#[test]
fn missing_primary_recovers_the_complete_backup_unbound_and_never_rewrites_inputs(){
 let(p,doc)=fixture("missing");let b=backup(&p);let bytes=fs::read(&b).unwrap();let pending=PathBuf::from(format!("{}.pending",p.display()));fs::write(&pending,b"unfinished").unwrap();fs::remove_file(&p).unwrap();let s=Service::new();recover(&s,"recovered",&p).unwrap();let r=s.snapshot("recovered").unwrap();assert_eq!(r.document.as_ref(),&doc);assert_eq!(r.revision,7);assert!(r.dirty&&r.saved_path.is_none());assert_eq!(r.undo_depth,0);assert!(r.warnings.iter().any(|n|n.code=="SOURCE_BACKUP_RECOVERED"));assert!(!p.exists());assert_eq!(fs::read(&b).unwrap(),bytes);assert_eq!(fs::read(&pending).unwrap(),b"unfinished");
 let out=p.with_file_name("恢复副本.arranger");command(&s,json!({"type":"save","document_id":"recovered","expected_revision":7,"path":out})).unwrap();assert_eq!(native::open(&out).unwrap().document,doc);assert!(!s.snapshot("recovered").unwrap().dirty);assert!(!p.exists());assert_eq!(fs::read(b).unwrap(),bytes);
}
#[test]
fn exact_backup_validation_rejects_corrupt_future_and_wrong_paths_without_partial_sessions(){
 let(p,doc)=fixture("validation");let b=backup(&p);let bytes=fs::read(&b).unwrap();let primary=fs::read(&p).unwrap();let s=Service::new();recover(&s,"direct",&b).unwrap();assert_eq!(s.snapshot("direct").unwrap().document.as_ref(),&doc);
 fs::write(backup(&b),&bytes).unwrap();let mut file:Value=serde_json::from_slice(&bytes).unwrap();file["document_sha256"]=json!("0");fs::write(&b,serde_json::to_vec(&file).unwrap()).unwrap();assert_eq!(recover(&s,"corrupt",&b).unwrap_err().code,"CHECKSUM_MISMATCH");assert!(s.snapshot("corrupt").is_err());file["format_version"]=json!(999);fs::write(&b,serde_json::to_vec(&file).unwrap()).unwrap();assert_eq!(recover(&s,"future",&p).unwrap_err().code,"UNSUPPORTED_FILE_VERSION");assert!(s.snapshot("future").is_err());assert_eq!(recover(&s,"wrong",&p.with_extension("cdr")).unwrap_err().code,"INVALID_PATH");assert_eq!(recover(&s,"relative",std::path::Path::new("local.arranger")).unwrap_err().code,"INVALID_PATH");assert_eq!(fs::read(&p).unwrap(),primary);
}
#[test]
fn duplicate_recovery_keeps_the_existing_busy_session_and_its_transaction(){
 let(p,_)=fixture("duplicate");let s=Service::new();command(&s,json!({"type":"document_new","document_id":"main","width_mm":210,"height_mm":297,"unit":"mm"})).unwrap();let before=s.snapshot("main").unwrap();let t=command(&s,json!({"type":"begin","document_id":"main","expected_revision":before.revision,"unit":"mm"})).unwrap();assert_eq!(recover(&s,"main",&p).unwrap_err().code,"DOCUMENT_EXISTS");let after=s.snapshot("main").unwrap();assert_eq!(after.document,before.document);assert!(after.busy);command(&s,json!({"type":"rollback","document_id":"main","transaction_id":t["transaction_id"]})).unwrap();
}
#[test]
fn recovered_edits_undo_and_copy_save_are_independent_and_overwrite_requires_explicit_consent(){
 let(p,doc)=fixture("editing");let b=backup(&p);let before=fs::read(&p).unwrap();let backup_bytes=fs::read(&b).unwrap();let s=Service::new();recover(&s,"first",&b).unwrap();recover(&s,"second",&p).unwrap();let a=s.snapshot("first").unwrap();command(&s,json!({"type":"batch","document_id":"second","expected_revision":7,"unit":"mm","operations":[{"type":"page_resize","page_id":doc.pages[0].id,"width_mm":220,"height_mm":297}]})).unwrap();let changed=s.snapshot("second").unwrap();assert_eq!(changed.undo_depth,1);s.command(Command::Undo{document_id:"second".into(),expected_revision:changed.revision}).unwrap();assert_eq!(s.snapshot("second").unwrap().document.as_ref(),&doc);assert_eq!(s.snapshot("first").unwrap().document,a.document);
 assert_eq!(command(&s,json!({"type":"save","document_id":"first","expected_revision":7,"path":p})).unwrap_err().code,"OUTPUT_EXISTS");assert_eq!(command(&s,json!({"type":"save","document_id":"first","expected_revision":7,"path":b})).unwrap_err().code,"INVALID_PATH");assert_eq!(fs::read(&p).unwrap(),before);assert_eq!(fs::read(&b).unwrap(),backup_bytes);
}
