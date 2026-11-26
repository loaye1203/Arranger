use arranger_core::{Command,Document,Geometry,Object,Service,Style,cdr::{self,ExportMode}};
use serde_json::{Value,json};
use std::{fs,path::PathBuf,sync::atomic::{AtomicU64,Ordering}};

fn directory()->PathBuf{static N:AtomicU64=AtomicU64::new(0);let p=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../artifacts/cdr-delivery-tests/{}-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),N.fetch_add(1,Ordering::Relaxed)));fs::create_dir_all(&p).unwrap();fs::canonicalize(p).unwrap()}
fn rect(id:&str,page:&str)->Object{serde_json::from_value(json!({"id":id,"page_id":page,"x_mm":20.,"y_mm":30.,"width_mm":40.,"height_mm":20.,"geometry":{"kind":"rect","radius_mm":0.}})).unwrap()}
fn source()->Document{
    let mut d=Document::new("source".into(),210.,297.).unwrap();
    let mut t=rect("text","page-1");t.geometry=serde_json::from_value(json!({"kind":"text","content":"Valve HAM42","font_family":"Arial","font_size_mm":4.})).unwrap();t.style=Style{fill:"#123456".into(),stroke:"none".into(),..Default::default()};d.objects.push(t);d
}
fn loaded(d:&Document,p:&std::path::Path)->Service{let original=p.join("source.arranger");arranger_core::native::save(d,0,&original,None,false).unwrap();let s=Service::new();s.command(Command::DocumentOpen{document_id:"main".into(),path:original.to_string_lossy().into()}).unwrap();s}
fn request(s:&Service,command:Value)->Value{let r=s.execute_json(&json!({"protocol_version":1,"command":command}).to_string());assert!(r.ok,"{r:?}");r.result.unwrap()}
fn state(s:&Service)->Value{serde_json::to_value(s.snapshot("main").unwrap()).unwrap()}

#[test]fn explicit_curves_are_document_objects_in_arranger_and_both_cdr_versions(){
    let mut d=source();d.objects[0].geometry=serde_json::from_value(json!({"kind":"text","content":"曲线25 A","font_family":if cfg!(target_os="windows"){"FZHei-B01S"}else{"PingFang SC"},"font_size_mm":4.})).unwrap();
    let mut other=d.objects[0].clone();other.id="still-text".into();other.y_mm=60.;d.objects.push(other);
    let p=directory();let s=loaded(&d,&p);let before=s.snapshot("main").unwrap();
    let pixels=|d:&Document|arranger_core::render::Renderer::new().render(d,"page-1",96.,None).unwrap().pixels.data().to_vec();
    request(&s,json!({"type":"batch","document_id":"main","expected_revision":0,"unit":"mm","operations":[{"type":"text_to_path","object_id":"text"}]}));
    let curved=s.snapshot("main").unwrap();assert_eq!(curved.undo_depth,1);assert_eq!(pixels(&before.document),pixels(&curved.document));
    assert!(matches!(curved.document.object("text").unwrap().geometry,Geometry::Path{..}));
    let native=p.join("converted.arranger");request(&s,json!({"type":"save","document_id":"main","expected_revision":1,"path":native}));
    request(&s,json!({"type":"document_open","document_id":"reopened","path":native}));assert_eq!(s.snapshot("reopened").unwrap().document.objects,curved.document.objects);
    for format in["cdr14","cdr21"]{
        let path=p.join(format!("converted-{format}.cdr"));let report=request(&s,json!({"type":"cdr_export","document_id":"main","mode":"editable","format":format,"path":path}));assert_eq!(report["converted_text_ids"],json!([]));
        let(read,_)=cdr::import(&path,"read").unwrap();assert!(matches!(read.object("text").unwrap().geometry,Geometry::Path{..}));assert!(matches!(read.object("still-text").unwrap().geometry,Geometry::Text{..}));
    }
    request(&s,json!({"type":"undo","document_id":"main","expected_revision":1}));assert_eq!(s.snapshot("main").unwrap().document.objects,before.document.objects);
    assert!(matches!(s.snapshot("reopened").unwrap().document.object("text").unwrap().geometry,Geometry::Path{..}));
    request(&s,json!({"type":"redo","document_id":"main","expected_revision":2}));assert_eq!(s.snapshot("main").unwrap().document.objects,curved.document.objects);
}

#[test]fn a_failed_multi_object_conversion_has_no_partial_document_or_history(){
    let mut d=source();let mut locked=d.objects[0].clone();locked.id="locked".into();locked.locked=true;d.objects.push(locked);
    let p=directory();let s=loaded(&d,&p);let before=state(&s);
    let result=s.execute_json(&json!({"protocol_version":1,"command":{"type":"batch","document_id":"main","expected_revision":0,"unit":"mm","operations":[{"type":"text_to_path","object_id":"text"},{"type":"text_to_path","object_id":"locked"}]}}).to_string());
    assert!(!result.ok);assert_eq!(result.error.unwrap().code,"OBJECT_LOCKED");let mut after=state(&s);let mut before=before;after.as_object_mut().unwrap().remove("warnings");before.as_object_mut().unwrap().remove("warnings");assert_eq!(after,before);
}

#[test]fn actual_corel_edited_mixed_text_reopens_with_complete_characters_and_uniform_font(){
    for(version,raw,content)in[
        (14,include_bytes!("../../../examples/assets/cdr/corel2019/delivery-14-area-edited.cdr").as_slice(),"曲线练习 25mm\nCurve sketch 42 A"),
        (21,include_bytes!("../../../examples/assets/cdr/corel2019/delivery-21-area-edited.cdr").as_slice(),"曲线练习 25mm\nCurve sketch 42 A"),
        (21,include_bytes!("../../../examples/assets/cdr/corel2019/delivery-21-point-edited.cdr").as_slice(),"曲线25 A A"),
    ]{
        let(d,_)=cdr::decode(raw,"edited").unwrap();let o=d.object("content").unwrap();let Geometry::Text{content:actual,font_family,font_size_mm,layout,..}=&o.geometry else{panic!("native text was flattened");};assert_eq!(actual,content);assert_eq!(*font_size_mm,4.);assert!(!font_family.is_empty());assert_eq!(layout.mode,if content.contains('\n'){arranger_core::TextMode::Area}else{arranger_core::TextMode::Point});
        let encoded=if version==14{cdr::encode_document(&d)}else{cdr::encode_document21(&d)}.unwrap().0;let(again,_)=cdr::decode(&encoded,"edited").unwrap();assert_eq!(again,d);
    }
}

#[test]fn modes_export_real_native_content_and_leave_source_binding_history_and_bytes_intact(){
    let p=directory();let s=loaded(&source(),&p);let before=state(&s);let original=fs::read(p.join("source.arranger")).unwrap();
    for format in["cdr14","cdr21"]{for mode in["editable","appearance"]{
        let report=request(&s,json!({"type":"cdr_preflight","document_id":"main","format":format,"mode":mode}));assert_eq!(report["can_export"],true);assert_eq!(state(&s),before);
        let path=p.join(format!("{format}-{mode}.cdr"));let saved=request(&s,json!({"type":"cdr_export","document_id":"main","format":format,"mode":mode,"path":path}));assert_eq!(saved["mode"],mode);assert_eq!(saved["atomic_exchange"],true);assert_eq!(saved["complete_save"],false);assert_eq!(saved["source_document_unchanged"],true);
        let(doc,_)=cdr::import(&path,"copy").unwrap();assert_eq!(doc.objects.len(),1);assert!(if mode=="editable"{matches!(doc.objects[0].geometry,Geometry::Text{..})}else{matches!(doc.objects[0].geometry,Geometry::Path{..})});assert_eq!(state(&s),before);assert_eq!(fs::read(p.join("source.arranger")).unwrap(),original);
    }}assert_eq!(s.command(Command::Undo{document_id:"main".into(),expected_revision:0}).unwrap_err().code,"NOTHING_TO_UNDO");
}

#[test]fn version14_transparency_rejection_preserves_every_destination_and_suggests21(){
    let mut d=Document::new("alpha".into(),210.,297.).unwrap();let group: Object=serde_json::from_value(json!({"id":"alpha","page_id":"page-1","x_mm":0.,"y_mm":0.,"width_mm":0.,"height_mm":0.,"geometry":{"kind":"group"},"style":{"fill":"none","stroke":"none","stroke_width_mm":0.,"opacity":0.7}})).unwrap();d.objects.push(group);let mut child=rect("child","page-1");child.parent_id=Some("alpha".into());d.objects.push(child);let p=directory();let s=loaded(&d,&p);let before=state(&s);let path=p.join("target.cdr");fs::write(&path,b"untouched target").unwrap();fs::write(p.join("target.cdr.bak"),b"untouched backup").unwrap();
    for mode in["editable","appearance"]{let r=request(&s,json!({"type":"cdr_preflight","document_id":"main","format":"cdr14","mode":mode}));assert_eq!(r["can_export"],false);assert_eq!(r["suggested_format"],"cdr21");let result=s.execute_json(&json!({"protocol_version":1,"command":{"type":"cdr_export","document_id":"main","format":"cdr14","mode":mode,"path":path,"overwrite":true}}).to_string());assert!(!result.ok);assert_eq!(result.error.unwrap().code,"CDR_REQUIRES_VERSION21");assert_eq!(fs::read(&path).unwrap(),b"untouched target");assert_eq!(fs::read(p.join("target.cdr.bak")).unwrap(),b"untouched backup");assert_eq!(state(&s),before);}
}

#[test]fn transaction_and_bad_backup_guards_apply_to_both_modes(){
    let p=directory();let s=loaded(&source(),&p);let before=state(&s);let tx=request(&s,json!({"type":"begin","document_id":"main","expected_revision":0,"unit":"mm","lease_ms":60000}));
    for mode in["editable","appearance"]{let r=s.execute_json(&json!({"protocol_version":1,"command":{"type":"cdr_export","document_id":"main","mode":mode,"path":p.join("busy.cdr")}}).to_string());assert!(!r.ok);assert_eq!(r.error.unwrap().code,"DOCUMENT_BUSY");assert!(!p.join("busy.cdr").exists());}
    request(&s,json!({"type":"rollback","document_id":"main","transaction_id":tx["transaction_id"]}));assert_eq!(state(&s),before);
    let path=p.join("keep.cdr");request(&s,json!({"type":"cdr_export","document_id":"main","mode":"editable","path":path}));let primary=fs::read(&path).unwrap();let backup=p.join("keep.cdr.bak");fs::write(&backup,b"broken recovery").unwrap();
    for mode in["editable","appearance"]{let r=s.execute_json(&json!({"protocol_version":1,"command":{"type":"cdr_export","document_id":"main","mode":mode,"path":path,"overwrite":true}}).to_string());assert!(!r.ok);assert_eq!(fs::read(&path).unwrap(),primary);assert_eq!(fs::read(&backup).unwrap(),b"broken recovery");assert_eq!(state(&s),before);}
}

#[test]fn explicit_page_scope_and_workspace_do_not_depend_on_the_gui(){
    let mut d=source();d.pages.push(arranger_core::Page{id:"second".into(),width_mm:210.,height_mm:297.});d.objects.push(rect("second-rect","second"));d.objects.push(rect("desktop",arranger_core::WORKSPACE_PAGE));let p=directory();let s=loaded(&d,&p);let before=state(&s);
    for mode in[ExportMode::Editable,ExportMode::Appearance]{
        let all=p.join(format!("all-{mode:?}.cdr"));request(&s,json!({"type":"cdr_export","document_id":"main","format":"cdr21","mode":mode,"path":all}));let(doc,_)=cdr::import(&all,"all").unwrap();assert_eq!(doc.pages.len(),2);assert_eq!(doc.objects.len(),3);
        let one=p.join(format!("one-{mode:?}.cdr"));let report=request(&s,json!({"type":"cdr_export","document_id":"main","format":"cdr21","mode":mode,"page_id":"second","path":one}));assert_eq!(report["scope"],"selected_page");let(doc,_)=cdr::import(&one,"one").unwrap();assert_eq!(doc.pages.len(),1);assert_eq!(doc.objects.iter().map(|o|o.id.as_str()).collect::<Vec<_>>(),vec!["second-rect"]);assert_eq!(state(&s),before);
    }
}
