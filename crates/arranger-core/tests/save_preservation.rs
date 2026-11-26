use arranger_core::{Command, Geometry, Service, command::{ObjectPatch, Operation}, native, save_check::{self, SaveFormat}};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::PathBuf, sync::atomic::{AtomicU64, Ordering}};

fn source() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/assets/cdr/all-content.arranger") }
fn directory() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../artifacts/save-preservation/{}-{}", std::process::id(), NEXT.fetch_add(1,Ordering::Relaxed)));
    fs::create_dir_all(&p).unwrap(); fs::canonicalize(p).unwrap()
}
fn service() -> Service {
    let s=Service::new(); s.command(Command::DocumentOpen{document_id:"main".into(),path:source().canonicalize().unwrap().to_string_lossy().into()}).unwrap(); s
}
fn query(s:&Service, format:&str)->Value {
    let r=s.execute_json(&json!({"protocol_version":1,"request_id":"check","command":{"type":"save_check","document_id":"main","format":format}}).to_string());
    assert!(r.ok,"{:?}",r.error); serde_json::to_value(r).unwrap()["result"].clone()
}
fn paths(v:&Value,path:&str,out:&mut BTreeSet<String>) {
    match v {
        Value::Object(m) if !m.is_empty()=>for(k,v)in m{paths(v,&format!("{path}/{}",k.replace('~',"~0").replace('/',"~1")),out);},
        Value::Array(a) if !a.is_empty()=>for(i,v)in a.iter().enumerate(){paths(v,&format!("{path}/{i}"),out);},
        _=>{out.insert(path.into());},
    }
}

#[test]
fn complete_fixture_is_inventoried_without_blobs_or_omitted_fields() {
    let s=service(); let before=s.snapshot("main").unwrap(); let check=query(&s,"cdr14");
    assert_eq!(check["counts"]["pages"],2); assert_eq!(check["counts"]["objects"],24);
    assert_eq!(check["counts"]["resources"],2); assert_eq!(check["counts"]["workspace_objects"],3);
    let mut expected=BTreeSet::new(); paths(&serde_json::to_value(&before.document).unwrap(),"",&mut expected);
    let rows=check["requirements"].as_array().unwrap();
    let actual:BTreeSet<_>=rows.iter().map(|r|r["path"].as_str().unwrap().to_string()).collect();
    assert_eq!(expected,actual); assert_eq!(rows.len(),actual.len());
    assert!(!check.to_string().contains("iVBOR"));
    for resource in check["resources"].as_array().unwrap() {
        let original=&before.document.resources[resource["id"].as_str().unwrap()];
        assert_eq!(resource["sha256"],original.sha256);
    }
    let features:BTreeSet<_>=rows.iter().map(|r|r["feature"].as_str().unwrap()).collect();
    for name in ["original_image_resource","paragraph_layout","live_text_path","group_hierarchy","affine_transform","non_destructive_clip","visibility_and_lock","stable_object_identity","pages_and_ownership","outline_details"]{assert!(features.contains(name),"{name}");}
}

#[test]
fn format_implementation_does_not_pretend_to_decide_format_capacity() {
    let s=service();
    for format in ["cdr14","cdr21"] {
        let check=query(&s,format); assert_eq!(check["can_save"],false);assert_eq!(check["full_document_save_available"],false);
        assert_eq!(check["format_capacity"],"not_determined_by_implementation_inventory");
        assert_eq!(check["default_save_format"],"arranger");
        assert_eq!(check["blocking_codes"],json!(["FORMAL_CDR_CODEC_NOT_IMPLEMENTED"]));
        assert!(check["requirements"].as_array().unwrap().iter().all(|r|r["status"]!="implemented"));
    }
    let check=query(&s,"arranger");assert_eq!(check["can_save"],true);assert_eq!(check["full_document_save_available"],true);
    assert!(check["requirements"].as_array().unwrap().iter().all(|r|r["status"]=="implemented"));
}

#[test]
fn query_keeps_revision_history_binding_dirty_and_transaction_unchanged() {
    let s=service();let before=serde_json::to_value(s.snapshot("main").unwrap()).unwrap();
    query(&s,"cdr14");query(&s,"cdr21");query(&s,"arranger");
    assert_eq!(before,serde_json::to_value(s.snapshot("main").unwrap()).unwrap());
    let rev=before["revision"].as_u64().unwrap();
    let token=s.command(Command::Begin{document_id:"main".into(),expected_revision:rev,unit:"mm".into(),lease_ms:60000}).unwrap()["transaction_id"].as_str().unwrap().to_string();
    let check=query(&s,"cdr14");assert_eq!(check["busy"],true);assert_eq!(check["revision"],rev);assert_eq!(check["document_hash"],before["document_hash"]);
    s.command(Command::Rollback{document_id:"main".into(),transaction_id:token}).unwrap();
}

#[test]
fn rejected_cdr_save_does_not_touch_output_backup_pending_lock_or_session() {
    let s=service();let before=serde_json::to_value(s.snapshot("main").unwrap()).unwrap();let dir=directory();let path=dir.join("保留.CDR");
    for suffix in ["",".bak",".pending",".lock"]{fs::write(format!("{}{suffix}",path.display()),format!("keep{suffix}")).unwrap();}
    for overwrite in [false,true]{
        let e=s.command(Command::Save{document_id:"main".into(),expected_revision:before["revision"].as_u64().unwrap(),path:path.to_string_lossy().into(),overwrite}).unwrap_err();
        assert_eq!(e.code,"CDR_SAVE_NOT_READY");
        assert_eq!(before,serde_json::to_value(s.snapshot("main").unwrap()).unwrap());
        for suffix in ["",".bak",".pending",".lock"]{assert_eq!(fs::read_to_string(format!("{}{suffix}",path.display())).unwrap(),format!("keep{suffix}"));}
    }
    let absent=dir.join("new.cdr");assert!(native::save(&s.snapshot("main").unwrap().document,0,&absent,None,true).is_err());
    assert_eq!(fs::read_dir(&dir).unwrap().count(),4);
}

#[test]
fn cdr_open_never_recovers_an_unrelated_native_backup_or_accepts_renamed_json() {
    let dir=directory();let path=dir.join("not-native.cdr");fs::write(&path,b"RIFFbroken").unwrap();fs::copy(source(),dir.join("not-native.cdr.bak")).unwrap();
    assert_eq!(native::open(&path).err().unwrap().code,"CDR_IMPORT_NOT_READY");
    fs::copy(source(),&path).unwrap();assert_eq!(native::open(&path).err().unwrap().code,"CDR_IMPORT_NOT_READY");
    let s=Service::new();let e=s.command(Command::DocumentOpen{document_id:"main".into(),path:path.to_string_lossy().into()}).unwrap_err();
    assert_eq!(e.code,"CDR_IMPORT_NOT_READY");assert_eq!(s.command(Command::Documents).unwrap()["documents"],json!([]));
}

#[test]
fn complete_native_roundtrip_preserves_source_bytes_links_and_edit_undo() {
    let s=service();let before=s.snapshot("main").unwrap();let path=directory().join("完整保存.arranger");
    s.command(Command::Save{document_id:"main".into(),expected_revision:before.revision,path:path.to_string_lossy().into(),overwrite:false}).unwrap();
    let reopened=native::open(&path).unwrap();assert_eq!(reopened.document,*before.document);assert_eq!(reopened.revision,before.revision);
    let second=Service::new();second.command(Command::DocumentOpen{document_id:"main".into(),path:path.to_string_lossy().into()}).unwrap();
    let mut geometry=before.document.object("point-text").unwrap().geometry.clone();if let Geometry::Text{content,..}=&mut geometry{*content="保存后继续编辑 / 中文".into();}
    let change=vec![Operation::ObjectUpdate{object_id:"rectangle".into(),patch:ObjectPatch{width_mm:Some(64.),..Default::default()}},Operation::ObjectUpdate{object_id:"point-text".into(),patch:ObjectPatch{geometry:Some(geometry),..Default::default()}}];
    second.command(Command::Batch{document_id:"main".into(),expected_revision:before.revision,unit:"mm".into(),operations:change}).unwrap();
    let edited=second.snapshot("main").unwrap();assert_eq!(edited.undo_depth,1);assert_eq!(edited.document.resources,before.document.resources);
    second.command(Command::Save{document_id:"main".into(),expected_revision:edited.revision,path:path.to_string_lossy().into(),overwrite:false}).unwrap();
    assert_eq!(native::open(&path).unwrap().document,*edited.document);
    second.command(Command::Undo{document_id:"main".into(),expected_revision:edited.revision}).unwrap();
    assert_eq!(*second.snapshot("main").unwrap().document,*before.document);
}

#[test]
fn unverified_typography_and_alpha_are_not_experimental_successes() {
    let s=service();let check=query(&s,"cdr14");
    for row in check["requirements"].as_array().unwrap(){
        if matches!(row["feature"].as_str(),Some("original_image_resource"|"paragraph_layout"|"live_text_path"|"non_destructive_clip")) {assert_eq!(row["status"],"not_implemented");}
    }
    let mut doc=(*s.snapshot("main").unwrap().document).clone();
    let r=doc.objects.iter_mut().find(|o|o.id=="rectangle").unwrap();r.style.fill="#11223344".into();
    let report=save_check::analyze(&doc,SaveFormat::Cdr14).unwrap();
    let row=report["requirements"].as_array().unwrap().iter().find(|r|r["object_id"]=="rectangle" && r["path"].as_str().unwrap().ends_with("/style/fill")).unwrap();
    assert_eq!(row["status"],"not_implemented");
}

#[test]
fn unknown_format_and_unknown_query_fields_reject() {
    let s=service();
    for extra in [json!({"type":"save_check","document_id":"main","format":"cdr99"}),json!({"type":"save_check","document_id":"main","format":"cdr14","ignore_losses":true})]{
        let r=s.execute_json(&json!({"protocol_version":1,"command":extra}).to_string());assert!(!r.ok);assert_eq!(r.error.unwrap().code,"INVALID_REQUEST");
    }
}

#[test]
fn full_cdr_preflight_exposes_each_blocked_object_without_mutation() {
    let s=service();
    // The committed complete sample uses the Mac host's PingFang SC. This
    // structural-blocker test needs a declared installed Windows face, while
    // product font substitution remains explicit and the source stays intact.
    if cfg!(target_os="windows") {
        let snap=s.snapshot("main").unwrap();
        let operations=snap.document.objects.iter().filter_map(|o| {
            let mut geometry=o.geometry.clone();
            if let Geometry::Text{font_family,..}=&mut geometry {
                *font_family="Microsoft YaHei".into();
                Some(Operation::ObjectUpdate{object_id:o.id.clone(),patch:ObjectPatch{geometry:Some(geometry),..Default::default()}})
            } else {None}
        }).collect();
        s.command(Command::Batch{document_id:"main".into(),expected_revision:snap.revision,unit:"mm".into(),operations}).unwrap();
    }
    let before=serde_json::to_value(s.snapshot("main").unwrap()).unwrap();
    let report=query(&s,"cdr14");
    let blockers=report["document_interchange"]["blockers"].as_array().unwrap();
    let ids=blockers.iter().filter_map(|v|v["object_id"].as_str()).collect::<BTreeSet<_>>();
    assert!(!ids.contains("on-path-text"));
    assert!(ids.contains("transformed-group"));
    assert_eq!(ids.len(),1,"{blockers:?}");
    assert!(!ids.contains("sheared-child"),"single-paint alpha now has native appearance and bounded original parameters");
    assert_eq!(before,serde_json::to_value(s.snapshot("main").unwrap()).unwrap());
    // Every blocked object really prevents an exchange; field inventory and
    // partial page output cannot claim the whole document is saveable.
    assert_eq!(report["can_save"],false);
    assert_eq!(report["document_interchange"]["can_export"],false);
    assert!(blockers.iter().all(|v|v["error"]["code"].is_string()));
}
