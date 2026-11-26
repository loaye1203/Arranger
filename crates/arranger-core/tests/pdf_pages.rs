use arranger_core::{Service,Command};
use serde_json::{json,Value};
use std::{path::PathBuf,fs};
fn cmd(s:&Service,v:Value)->arranger_core::Result<Value>{s.command(serde_json::from_value(v).unwrap())}
fn fixture(name:&str)->(Service,PathBuf){
    let dir=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../artifacts/pdf-pages-{}-{name}",std::process::id()));fs::create_dir_all(&dir).unwrap();
    let s=Service::new();cmd(&s,json!({"type":"document_new","document_id":"main","width_mm":210,"height_mm":297,"unit":"mm"})).unwrap();
    cmd(&s,json!({"type":"batch","document_id":"main","expected_revision":0,"unit":"mm","operations":[{"type":"page_add","page":{"id":"blank","width_mm":297,"height_mm":210}},{"type":"page_add","page":{"id":"label","width_mm":60,"height_mm":14}}]})).unwrap();(s,dir.canonicalize().unwrap().join("pages.pdf"))
}
fn export(s:&Service,p:&std::path::Path,pages:Value)->arranger_core::Result<Value>{cmd(s,json!({"type":"pdf_export","document_id":"main","path":p,"page_ids":pages,"expected_revision":1,"overwrite":true}))}
#[test]fn all_and_explicit_pages_export_without_mutating_the_session(){
    let(s,p)=fixture("all");let before=s.snapshot("main").unwrap();let r=export(&s,&p,Value::Null).unwrap();assert_eq!(r["page_ids"],json!(["page-1","blank","label"]));assert_eq!(r["page_count"],3);assert!(fs::read(&p).unwrap().starts_with(b"%PDF-"));
    let r=export(&s,&p,json!(["label","page-1"])).unwrap();assert_eq!(r["page_ids"],json!(["label","page-1"]));let after=s.snapshot("main").unwrap();assert_eq!(before.document,after.document);assert_eq!(before.revision,after.revision);assert_eq!(before.undo_depth,after.undo_depth);assert_eq!(before.saved_path,after.saved_path);assert_eq!(before.dirty,after.dirty);
}
#[test]fn invalid_selection_revision_path_and_overwrite_never_replace_existing_output(){
    let(s,p)=fixture("invalid");let sentinel=b"existing PDF";fs::write(&p,sentinel).unwrap();
    for pages in[json!([]),json!(["page-1","page-1"]),json!(["missing"])]{assert!(export(&s,&p,pages).is_err());assert_eq!(fs::read(&p).unwrap(),sentinel);}
    for(extra,code)in[(json!({"expected_revision":0}),"REVISION_CONFLICT"),(json!({"overwrite":false}),"OUTPUT_EXISTS"),(json!({"path":p.with_extension("png")}),"INVALID_PATH")]{
        let mut v=json!({"type":"pdf_export","document_id":"main","path":p,"expected_revision":1,"overwrite":true});for(k,vv)in extra.as_object().unwrap(){v[k]=vv.clone();}assert_eq!(cmd(&s,v).unwrap_err().code,code);assert_eq!(fs::read(&p).unwrap(),sentinel);
    }
}
#[test]fn active_transaction_and_oversize_page_fail_before_publication(){
    let(s,p)=fixture("busy");let t=cmd(&s,json!({"type":"begin","document_id":"main","expected_revision":1,"unit":"mm"})).unwrap();assert_eq!(export(&s,&p,Value::Null).unwrap_err().code,"DOCUMENT_BUSY");assert!(!p.exists());cmd(&s,json!({"type":"rollback","document_id":"main","transaction_id":t["transaction_id"]})).unwrap();
    cmd(&s,json!({"type":"batch","document_id":"main","expected_revision":1,"unit":"mm","operations":[{"type":"page_resize","page_id":"blank","width_mm":6000,"height_mm":210}]})).unwrap();
    assert_eq!(cmd(&s,json!({"type":"pdf_export","document_id":"main","path":p})).unwrap_err().code,"PDF_PAGE_RANGE");assert!(!p.exists());
    cmd(&s,json!({"type":"pdf_export","document_id":"main","path":p,"page_ids":["label"]})).unwrap();assert!(p.exists());
}
