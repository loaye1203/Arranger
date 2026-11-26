use arranger_core::{Command, Document, Geometry, Object, Resource, Service, Style, command::{ExportFormat, ObjectPatch, Operation}, native, render::Renderer};
use serde_json::json;
use std::{fs, path::PathBuf, sync::atomic::{AtomicU64, Ordering}, time::Duration};

fn new(service:&Service,id:&str) {
    service.command(Command::DocumentNew {document_id:id.into(),width_mm:300.,height_mm:200.,unit:"mm".into()}).unwrap();
}
fn object(id:&str,x:f64)->Object {
    Object{id:id.into(),page_id:"page-1".into(),parent_id:None,x_mm:x,y_mm:10.,width_mm:100.,height_mm:50.,transform:[1.,0.,0.,1.,0.,0.],style:Style::default(),visible:true,locked:false,clip:None,geometry:Geometry::Rect{radius_mm:2.}}
}
fn batch(service:&Service,id:&str,operations:Vec<Operation>)->arranger_core::Result<serde_json::Value> {
    service.command(Command::Batch {document_id:id.into(),expected_revision:service.snapshot(id)?.revision,unit:"mm".into(),operations})
}
fn update(id:&str,x:f64)->Operation {Operation::ObjectUpdate{object_id:id.into(),patch:ObjectPatch{x_mm:Some(x),..Default::default()}}}
fn temp(name:&str)->PathBuf {
    static NEXT:AtomicU64=AtomicU64::new(1);
    let root=PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_owned();
    let path=root.join("artifacts/core-tests").join(format!("{name}-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));
    fs::create_dir_all(&path).unwrap();path
}

#[test]
fn thousand_distinct_objects_rollback_and_single_undo_redo() {
    let service=Service::new();new(&service,"main");
    batch(&service,"main",(0..1000).map(|i|Operation::ObjectCreate{object:object(&format!("rect-{i}"),i as f64)}).collect()).unwrap();
    let before=service.snapshot("main").unwrap();
    let mut bad:Vec<_>=(0..1000).map(|i|update(&format!("rect-{i}"),i as f64+0.125)).collect();bad[799]=update("missing",0.);
    let error=batch(&service,"main",bad).unwrap_err();
    assert_eq!(error.failed_operation,Some(800));assert_eq!(error.rolled_back,Some(true));
    let failed=service.snapshot("main").unwrap();assert_eq!(failed.document_hash,before.document_hash);assert_eq!(failed.undo_depth,before.undo_depth);
    batch(&service,"main",(0..1000).map(|i|update(&format!("rect-{i}"),i as f64+0.125)).collect()).unwrap();
    let success=service.snapshot("main").unwrap();assert_eq!(success.undo_depth,before.undo_depth+1);
    for (i,o) in success.document.objects.iter().enumerate(){assert_eq!(o.x_mm,i as f64+0.125);}
    service.command(Command::Undo{document_id:"main".into(),expected_revision:success.revision}).unwrap();
    let undone=service.snapshot("main").unwrap();assert_eq!(undone.document_hash,before.document_hash);
    service.command(Command::Redo{document_id:"main".into(),expected_revision:undone.revision}).unwrap();
    assert_eq!(service.snapshot("main").unwrap().document_hash,success.document_hash);
}

#[test]
fn explicit_leases_keep_readers_and_other_documents_available() {
    let service=Service::new();new(&service,"main");new(&service,"other");
    batch(&service,"main",vec![Operation::ObjectCreate{object:object("rect",10.)}]).unwrap();
    let before=service.snapshot("main").unwrap();
    let begin=service.command(Command::Begin{document_id:"main".into(),expected_revision:before.revision,unit:"mm".into(),lease_ms:60000}).unwrap();let token=begin["transaction_id"].as_str().unwrap().to_owned();
    service.command(Command::Execute{document_id:"main".into(),transaction_id:token.clone(),operation:update("rect",100.)}).unwrap();
    assert_eq!(service.snapshot("main").unwrap().document_hash,before.document_hash);
    assert_eq!(batch(&service,"main",vec![update("rect",500.)]).unwrap_err().code,"DOCUMENT_BUSY");
    batch(&service,"other",vec![Operation::ObjectCreate{object:object("unrelated",0.)}]).unwrap();
    assert_eq!(service.command(Command::Commit{document_id:"main".into(),transaction_id:"wrong".into()}).unwrap_err().code,"TRANSACTION_NOT_FOUND");
    service.command(Command::Rollback{document_id:"main".into(),transaction_id:token}).unwrap();
    assert_eq!(service.snapshot("main").unwrap().document_hash,before.document_hash);
    let begin=service.command(Command::Begin{document_id:"main".into(),expected_revision:before.revision,unit:"mm".into(),lease_ms:10}).unwrap();
    let token=begin["transaction_id"].as_str().unwrap().into();
    service.command(Command::Execute{document_id:"main".into(),transaction_id:token,operation:update("rect",900.)}).unwrap();
    std::thread::sleep(Duration::from_millis(25));
    let recovered=service.snapshot("main").unwrap();assert!(!recovered.busy);assert_eq!(recovered.document_hash,before.document_hash);assert_eq!(recovered.warnings.last().unwrap().code,"TRANSACTION_EXPIRED");
}

#[test]
fn invalid_input_and_locked_parent_never_publish_partial_work() {
    let service=Service::new();new(&service,"main");
    let mut group=object("group",0.);group.geometry=Geometry::Group;group.locked=true;
    batch(&service,"main",vec![Operation::ObjectCreate{object:object("rect",10.)},Operation::ObjectCreate{object:group}]).unwrap();
    let hash=service.snapshot("main").unwrap().document_hash;
    let mut child=object("child",0.);child.parent_id=Some("group".into());
    for op in [update("missing",2.),update("rect",f64::NAN),Operation::ObjectCreate{object:child},Operation::ObjectUpdate{object_id:"rect".into(),patch:ObjectPatch{geometry:Some(Geometry::Path{d:"M0 bad".into(),fill_rule:Default::default()}),..Default::default()}}] {
        assert!(batch(&service,"main",vec![update("rect",200.),op]).is_err());assert_eq!(service.snapshot("main").unwrap().document_hash,hash);
    }
    let response=service.execute_json(&json!({"protocol_version":1,"request_id":"unknown","command":{"type":"batch","document_id":"main","expected_revision":1,"unit":"mm","operations":[],"unrecognized":true}}).to_string());
    assert!(!response.ok);assert_eq!(response.error.unwrap().code,"INVALID_REQUEST");assert_eq!(response.request_id,"unknown");
    assert_eq!(service.command(Command::Begin{document_id:"main".into(),expected_revision:1,unit:"px".into(),lease_ms:100}).unwrap_err().code,"UNSUPPORTED_UNIT");
    assert_eq!(service.command(Command::Undo{document_id:"main".into(),expected_revision:0}).unwrap_err().code,"REVISION_CONFLICT");
    let token=service.command(Command::Begin{document_id:"main".into(),expected_revision:1,unit:"mm".into(),lease_ms:60000}).unwrap()["transaction_id"].as_str().unwrap().to_owned();
    service.command(Command::Execute{document_id:"main".into(),transaction_id:token.clone(),operation:update("rect",900.)}).unwrap();
    let rejected=service.execute_json(&json!({"protocol_version":1,"command":{"type":"execute","document_id":"main","transaction_id":token,"operation":{"type":"object_update","object_id":"rect","patch":{"unknown_field":1}}}}).to_string());
    assert!(!rejected.ok);assert_eq!(rejected.error.unwrap().rolled_back,Some(true));assert!(!service.snapshot("main").unwrap().busy);assert_eq!(service.snapshot("main").unwrap().document_hash,hash);
}

#[test]
fn native_roundtrip_editing_disk_conflict_backup_and_future_format() {
    let dir=temp("native");let path=dir.join("editable.arranger");let service=Service::new();new(&service,"main");
    let mut text=object("text",10.123456789012345);text.geometry=Geometry::Text{content:"中文 & <Arranger>".into(),font_family:"Microsoft YaHei".into(),font_size_mm:10.,text_path:None,layout:Default::default()};
    let mut pixels=arranger_core::render::Pixmap::new(2,2).unwrap();pixels.fill(resvg::tiny_skia::Color::from_rgba8(255,40,30,255));
    let resource=Resource::png(&pixels.encode_png().unwrap()).unwrap();
    let mut image=object("image",20.);image.geometry=Geometry::Image{resource_id:"shared".into(),clip_mm:Some([0.,0.,40.,30.]),fit:Default::default()};
    let mut curve=object("curve",30.);curve.geometry=Geometry::Path{d:"M0 0 C10 0 20 20 30 0".into(),fill_rule:Default::default()};
    let mut offpage=object("offpage",330.);offpage.y_mm=-10.;
    batch(&service,"main",vec![Operation::ResourcePut{resource_id:"shared".into(),resource},Operation::ObjectCreate{object:text},Operation::ObjectCreate{object:image},Operation::ObjectDuplicate{object_id:"image".into(),new_id:"image-copy".into(),descendant_ids:Default::default()},Operation::ObjectCreate{object:curve},Operation::ObjectCreate{object:offpage}]).unwrap();
    let before=service.snapshot("main").unwrap();let pathstr=path.to_string_lossy().into_owned();
    service.command(Command::Save{document_id:"main".into(),expected_revision:before.revision,path:pathstr.clone(),overwrite:false}).unwrap();
    let second=Service::new();second.command(Command::DocumentOpen{document_id:"reopened".into(),path:pathstr.clone()}).unwrap();
    assert_eq!(*second.snapshot("reopened").unwrap().document,*before.document);
    batch(&second,"reopened",vec![update("text",50.)]).unwrap();let edited=second.snapshot("reopened").unwrap();
    second.command(Command::Save{document_id:"reopened".into(),expected_revision:edited.revision,path:pathstr.clone(),overwrite:false}).unwrap();
    assert_eq!(service.command(Command::Save{document_id:"main".into(),expected_revision:before.revision,path:pathstr,overwrite:true}).unwrap_err().code,"DISK_CONFLICT");
    assert_eq!(native::open(&path).unwrap().document, *edited.document);
    fs::write(&path,b"{broken").unwrap();let backup=native::open(&path).unwrap();assert_eq!(backup.document,*before.document);assert_eq!(backup.warnings[0].code,"RECOVERED_BACKUP");
    fs::write(&path,br#"{"format":"arranger","format_version":99,"new_document":{"future":true}}"#).unwrap();
    assert_eq!(native::open(&path).err().unwrap().code,"UNSUPPORTED_FILE_VERSION");
}

#[test]
fn real_svg_png_exports_preserve_mm_text_and_output_conflicts() {
    let dir=temp("export");let mut doc=Document::new("doc".into(),300.,200.).unwrap();
    let mut text=object("text",10.);text.geometry=Geometry::Text{content:"中文 A&B <vector>".into(),font_family:"Microsoft YaHei".into(),font_size_mm:8.,text_path:None,layout:Default::default()};
    doc.objects=vec![object("rect",10.),text];doc.validate().unwrap();
    let renderer=Renderer::new();let svgpath=dir.join("page.svg");let pngpath=dir.join("page.png");
    renderer.export(&doc,"page-1",ExportFormat::Svg,96.,&svgpath,false).unwrap();
    renderer.export(&doc,"page-1",ExportFormat::Png,96.,&pngpath,false).unwrap();
    let svg=fs::read_to_string(&svgpath).unwrap();assert!(svg.contains("<text"));assert!(svg.contains("中文 A&amp;B &lt;vector&gt;"));
    let tree=resvg::usvg::Tree::from_str(&svg,&resvg::usvg::Options::default()).unwrap();assert!((tree.size().width() as f64-300.*96./25.4).abs()<0.001);
    let png=arranger_core::render::Pixmap::load_png(&pngpath).unwrap();assert_eq!((png.width(),png.height()),(1134,756));
    assert_eq!(renderer.export(&doc,"page-1",ExportFormat::Png,96.,&pngpath,false).unwrap_err().code,"OUTPUT_EXISTS");
    assert_eq!(renderer.export(&doc,"page-1",ExportFormat::Png,f64::INFINITY,&dir.join("invalid.png"),false).unwrap_err().code,"INVALID_DPI");
    let mut missing=doc.clone();if let Geometry::Text{font_family,..}=&mut missing.objects[1].geometry{*font_family="Arranger-Missing-Font-Example".into();}
    assert_eq!(renderer.warnings(&missing)[0].code,"FONT_SUBSTITUTED");
}

#[test]
fn capability_schema_and_transformed_hit_testing_are_callable() {
    let service=Service::new();let description=service.command(Command::Describe).unwrap();
    assert_eq!(description["schema"]["properties"]["protocol_version"]["const"],1);
    assert!(description["schema"]["$defs"]["Command"]["oneOf"].as_array().unwrap().len()>=16);
    new(&service,"main");let mut o=object("rotated",10.);o.transform=[0.,1.,-1.,0.,200.,0.];
    batch(&service,"main",vec![Operation::ObjectCreate{object:o}]).unwrap();
    let hit=service.command(Command::HitTest{document_id:"main".into(),page_id:"page-1".into(),point_mm:[180.,30.]}).unwrap();assert_eq!(hit["object_id"],"rotated");
    let info=service.command(Command::ObjectGet{document_id:"main".into(),object_id:"rotated".into()}).unwrap();assert_eq!(info["bounds_mm"][2],50.);
}
