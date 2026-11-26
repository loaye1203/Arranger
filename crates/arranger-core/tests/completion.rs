use arranger_core::{Service,Geometry,native};
use serde_json::{Value,json};
use std::{fs,path::PathBuf,sync::atomic::{AtomicU64,Ordering}};

fn call(s:&Service,c:Value)->arranger_core::command::Response {
    s.execute_json(&json!({"protocol_version":1,"command":c}).to_string())
}
fn ok(s:&Service,c:Value)->Value {let r=call(s,c);assert!(r.ok,"{:?}",r.error);r.result.unwrap()}
fn service()->Service {let s=Service::new();ok(&s,json!({"type":"document_new","document_id":"main","width_mm":300,"height_mm":200,"unit":"mm"}));s}
fn object(id:&str,geometry:Value,parent:Option<&str>)->Value {
    json!({"id":id,"page_id":"page-1","parent_id":parent,"x_mm":0,"y_mm":0,"width_mm":20,"height_mm":10,"geometry":geometry})
}
fn create(o:Value)->Value {json!({"type":"object_create","object":o})}
fn batch(s:&Service,ops:Value)->Value {ok(s,json!({"type":"batch","document_id":"main","expected_revision":s.snapshot("main").unwrap().revision,"unit":"mm","operations":ops}))}
fn temp()->PathBuf {
    static NEXT:AtomicU64=AtomicU64::new(0);
    let root=PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf();
    let base=root.join("artifacts/core-tests");
    fs::create_dir_all(&base).unwrap();
    // Windows can reuse a PID from an earlier validation. Keep that evidence
    // and claim a fresh directory atomically instead of reusing its outputs.
    loop {
        let dir=base.join(format!("completion-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));
        match fs::create_dir(&dir) {
            Ok(())=>return dir,
            Err(error) if error.kind()==std::io::ErrorKind::AlreadyExists=>continue,
            Err(error)=>panic!("Could not create test output {}: {error}",dir.display()),
        }
    }
}
fn fixture()->Value {json!([
    create(object("group",json!({"kind":"group"}),None)),
    create(object("nested",json!({"kind":"group"}),Some("group"))),
    create(object("guide",json!({"kind":"path","d":"M0 0L200 0"}),Some("nested"))),
    create(object("text",json!({"kind":"text","content":"AB","font_family":"sans-serif","font_size_mm":3,"text_path":{"path_id":"guide"}}),Some("nested")))
])}

#[test]
fn group_copy_remaps_hierarchy_and_internal_text_guide_in_one_undo() {
    let s=service();batch(&s,fixture());let before=s.snapshot("main").unwrap();
    batch(&s,json!([{"type":"object_duplicate","object_id":"group","new_id":"copy","descendant_ids":{"guide":"copy-guide"}}]));
    let copied=s.snapshot("main").unwrap();assert_eq!(copied.document.objects.len(),8);
    assert_eq!(copied.document.object("copy.1").unwrap().parent_id.as_deref(),Some("copy"));
    assert_eq!(copied.document.object("copy-guide").unwrap().parent_id.as_deref(),Some("copy.1"));
    let Geometry::Text{text_path:Some(binding),..}=&copied.document.object("copy.3").unwrap().geometry else{panic!("editable bound text")};
    assert_eq!(binding.path_id,"copy-guide");
    ok(&s,json!({"type":"undo","document_id":"main","expected_revision":copied.revision}));
    assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
    let undone=s.snapshot("main").unwrap();ok(&s,json!({"type":"redo","document_id":"main","expected_revision":undone.revision}));
    assert_eq!(s.snapshot("main").unwrap().document_hash,copied.document_hash);
}

#[test]
fn copy_collision_bad_mapping_and_locked_descendant_rollback_everything() {
    let s=service();batch(&s,fixture());
    for op in [json!({"type":"object_duplicate","object_id":"group","new_id":"guide"}),json!({"type":"object_duplicate","object_id":"group","new_id":"copy","descendant_ids":{"missing":"x"}}),json!({"type":"object_duplicate","object_id":"group","new_id":"copy","descendant_ids":{"guide":"same","text":"same"}})] {
        let before=s.snapshot("main").unwrap();
        let r=call(&s,json!({"type":"batch","document_id":"main","expected_revision":before.revision,"unit":"mm","operations":[{"type":"object_update","object_id":"group","patch":{"x_mm":50}},op]}));
        assert!(!r.ok);assert_eq!(r.error.unwrap().rolled_back,Some(true));assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
    }
    batch(&s,json!([{"type":"object_lock","object_id":"guide","locked":true}]));let before=s.snapshot("main").unwrap();
    let r=call(&s,json!({"type":"batch","document_id":"main","expected_revision":before.revision,"unit":"mm","operations":[{"type":"object_duplicate","object_id":"group","new_id":"copy"}]}));
    assert_eq!(r.error.unwrap().code,"OBJECT_LOCKED");assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
}

#[test]
fn copied_external_guide_keeps_reference_and_new_children_survive_native_roundtrip() {
    let s=service();batch(&s,fixture());
    batch(&s,json!([{"type":"object_duplicate","object_id":"text","new_id":"independent"}]));
    let snap=s.snapshot("main").unwrap();let dir=temp();let path=dir.join("group.arranger");
    native::save(&snap.document,snap.revision,&path,None,false).unwrap();let loaded=native::open(&path).unwrap();
    assert_eq!(loaded.document.hash(),snap.document_hash);
    let Geometry::Text{text_path:Some(binding),..}=&loaded.document.object("independent").unwrap().geometry else{panic!("text")};assert_eq!(binding.path_id,"guide");
    batch(&s,json!([{"type":"object_duplicate","object_id":"nested","new_id":"nested-copy"}]));
    assert_eq!(s.snapshot("main").unwrap().document.object("nested-copy").unwrap().parent_id.as_deref(),Some("group"));
}

#[test]
fn page_lifecycle_rejects_nonempty_and_last_page_then_restores_with_undo() {
    let s=service();batch(&s,json!([{"type":"page_add","page":{"id":"page-2","width_mm":210,"height_mm":297}},{"type":"page_resize","page_id":"page-1","width_mm":123.456789012345,"height_mm":200}]));
    let before=s.snapshot("main").unwrap();batch(&s,json!([{"type":"page_delete","page_id":"page-2"}]));
    let snap=s.snapshot("main").unwrap();let r=call(&s,json!({"type":"batch","document_id":"main","expected_revision":snap.revision,"unit":"mm","operations":[{"type":"page_delete","page_id":"page-1"}]}));
    assert_eq!(r.error.unwrap().code,"PAGE_NOT_EMPTY");ok(&s,json!({"type":"undo","document_id":"main","expected_revision":snap.revision}));
    assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
    let mut o=object("rect",json!({"kind":"rect"}),None);o["page_id"]=json!("page-2");batch(&s,json!([create(o)]));
    let snap=s.snapshot("main").unwrap();let r=call(&s,json!({"type":"batch","document_id":"main","expected_revision":snap.revision,"unit":"mm","operations":[{"type":"page_delete","page_id":"page-2"}]}));assert_eq!(r.error.unwrap().code,"PAGE_NOT_EMPTY");
}

#[test]
fn affine_ellipse_bounds_match_analytic_extrema_instead_of_curve_tolerance() {
    let s=service();let mut o=object("ellipse",json!({"kind":"ellipse"}),None);
    o["width_mm"]=json!(36.);o["height_mm"]=json!(24.);o["transform"]=json!([1.1,0.25,0.35,0.9,90.,60.]);
    batch(&s,json!([create(o)]));let snap=s.snapshot("main").unwrap();let b=snap.document.bounds(snap.document.object("ellipse").unwrap()).unwrap();
    let rx=19.8_f64.hypot(4.2);let ry=4.5_f64.hypot(10.8);
    for (actual,expected) in b.into_iter().zip([114.-rx,75.3-ry,2.*rx,2.*ry]) {assert!((actual-expected).abs()<1e-12);}
}

#[test]
fn save_as_keeps_both_files_independent_and_conflicts_do_not_overwrite() {
    let s=service();batch(&s,json!([create(object("rect",json!({"kind":"rect"}),None))]));let a=s.snapshot("main").unwrap();let dir=temp();let first=dir.join("a.arranger");let second=dir.join("b.arranger");
    let binding=native::save(&a.document,a.revision,&first,None,false).unwrap();
    let mut changed=a.document.as_ref().clone();changed.objects[0].x_mm=33.;
    native::save(&changed,a.revision+1,&second,Some(&binding),false).unwrap();
    assert_eq!(native::open(&first).unwrap().document.hash(),a.document_hash);assert_eq!(native::open(&second).unwrap().document.hash(),changed.hash());
    fs::write(&first,b"externally changed").unwrap();
    assert_eq!(native::save(&changed,2,&first,Some(&binding),true).err().unwrap().code,"DISK_CONFLICT");assert_eq!(fs::read(first).unwrap(),b"externally changed");
}

#[test]
fn pdf_is_real_vector_page_rejects_wrong_path_and_preserves_document() {
    let s=service();batch(&s,fixture());let snap=s.snapshot("main").unwrap();let dir=temp();let path=dir.join("page.pdf");
    let result=ok(&s,json!({"type":"export","document_id":"main","page_id":"page-1","format":"pdf","path":path}));
    assert!(result["warnings"].as_array().unwrap().iter().all(|v|v["code"]!="PDF_RASTERIZED"));
    let data=fs::read(&path).unwrap();assert!(data.starts_with(b"%PDF-"));assert!(data.ends_with(b"%%EOF\n"));assert_eq!(s.snapshot("main").unwrap().document_hash,snap.document_hash);
    assert_eq!(call(&s,json!({"type":"export","document_id":"main","page_id":"page-1","format":"pdf","path":path})).error.unwrap().code,"OUTPUT_EXISTS");
    assert_eq!(call(&s,json!({"type":"export","document_id":"main","page_id":"page-1","format":"pdf","path":dir.join("bad.svg")})).error.unwrap().code,"INVALID_PATH");
    assert_eq!(call(&s,json!({"type":"export","document_id":"main","page_id":"page-1","format":"pdf","path":dir.join("bad.pdf"),"dpi":0})).error.unwrap().code,"INVALID_DPI");
    batch(&s,json!([{"type":"page_resize","page_id":"page-1","width_mm":6000,"height_mm":100}]));
    assert_eq!(call(&s,json!({"type":"export","document_id":"main","page_id":"page-1","format":"pdf","path":dir.join("oversize.pdf")})).error.unwrap().code,"PDF_PAGE_RANGE");
    assert!(!dir.join("oversize.pdf").exists());
}
