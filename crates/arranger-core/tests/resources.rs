use arranger_core::{Clip,Command,Document,Geometry,ImageFit,LineCap,LineJoin,Object,Operation,Resource,Service,Style,FillRule,
    command::ObjectPatch,native,render::{self,Renderer},resource};
use serde_json::{Value,json};
use std::{fs,path::PathBuf,sync::atomic::{AtomicU64,Ordering}};

fn service()->Service {let s=Service::new();s.command(Command::DocumentNew{document_id:"main".into(),width_mm:300.,height_mm:200.,unit:"mm".into()}).unwrap();s}
fn object(id:&str,geometry:Geometry)->Object {Object{id:id.into(),page_id:"page-1".into(),parent_id:None,x_mm:0.,y_mm:0.,width_mm:60.,height_mm:40.,transform:[1.,0.,0.,1.,0.,0.],style:Style::default(),visible:true,locked:false,clip:None,geometry}}
fn rect(id:&str)->Object {object(id,Geometry::Rect{radius_mm:0.})}
fn image(id:&str,resource:&str)->Object {object(id,Geometry::Image{resource_id:resource.into(),clip_mm:None,fit:ImageFit::Stretch})}
fn asset(name:&str)->PathBuf {PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/assets/images").join(name).canonicalize().unwrap()}
fn import(object:Object,name:&str)->Operation {Operation::ImageImport{path:asset(name).to_string_lossy().into_owned(),object}}
fn create(object:Object)->Operation {Operation::ObjectCreate{object}}
fn update(id:&str,patch:ObjectPatch)->Operation {Operation::ObjectUpdate{object_id:id.into(),patch}}
fn batch(s:&Service,operations:Vec<Operation>)->arranger_core::Result<Value> {s.command(Command::Batch{document_id:"main".into(),expected_revision:s.snapshot("main")?.revision,unit:"mm".into(),operations})}
fn render(s:&Service)->render::Pixmap {Renderer::new().render(&s.snapshot("main").unwrap().document,"page-1",127.,None).unwrap().pixels}
fn pixel(p:&render::Pixmap,x:f64,y:f64)->[u8;3] {let i=((y*5.).round() as usize*p.width() as usize+(x*5.).round() as usize)*4;[p.data()[i],p.data()[i+1],p.data()[i+2]]}
fn close(a:[u8;3],b:[u8;3],epsilon:u8) {assert!(a.iter().zip(b).all(|(x,y)|x.abs_diff(y)<=epsilon),"{a:?} != {b:?}");}
fn hit(doc:&Document,x:f64,y:f64)->Option<String> {doc.hit_test("page-1",[x,y],0.5)}
fn temp()->PathBuf {static NEXT:AtomicU64=AtomicU64::new(1);let p=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../artifacts/core-tests/resources-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));fs::create_dir_all(&p).unwrap();p.canonicalize().unwrap()}

#[test]
fn png_rgb_jpeg_and_grayscale_jpeg_retain_original_bytes_and_dimensions() {
    for (name,mime,w,h) in [("quadrants.png","image/png",120,80),("quadrants.jpg","image/jpeg",120,80),("grayscale.jpg","image/jpeg",32,16)] {
        let bytes=fs::read(asset(name)).unwrap();let r=Resource::image(&bytes).unwrap();let info=resource::validate(&r).unwrap();
        assert_eq!(r.mime,mime);assert_eq!(info.width_px,w);assert_eq!(info.height_px,h);assert_eq!(info.encoded_bytes,bytes.len());
        use base64::Engine;assert_eq!(base64::engine::general_purpose::STANDARD.decode(&r.data_base64).unwrap(),bytes);
    }
    assert_eq!(Resource::png(&fs::read(asset("quadrants.jpg")).unwrap()).unwrap_err().code,"INVALID_RESOURCE");
}

#[test]
fn repeated_imports_and_one_hundred_copies_share_one_native_and_svg_original() {
    let s=service();let mut ops=vec![import(image("first","original"),"quadrants.png"),import(image("second","requested-alias"),"quadrants.png")];
    ops.extend((0..100).map(|i|Operation::ObjectDuplicate{object_id:"first".into(),new_id:format!("copy-{i}"),descendant_ids:Default::default()}));let result=batch(&s,ops).unwrap();let snap=s.snapshot("main").unwrap();
    assert_eq!(snap.document.resources.len(),1);assert_eq!(snap.document.objects.len(),102);assert!(result["warnings"].as_array().unwrap().iter().any(|w|w["code"]=="RESOURCE_REUSED"));
    assert!(snap.document.objects.iter().all(|o|matches!(&o.geometry,Geometry::Image{resource_id,..} if resource_id=="original")));
    let query=s.command(Command::ResourceGet{document_id:"main".into(),resource_id:"original".into()}).unwrap();assert_eq!(query["reference_count"],102);assert_eq!(query["image"]["width_px"],120);
    let svg=render::svg(&snap.document,"page-1").unwrap();assert_eq!(svg.matches("data:image/png;base64,").count(),1);assert_eq!(svg.matches("<use href=\"#resource-original\"").count(),102);
    let data=serde_json::to_string(&*snap.document).unwrap();assert_eq!(data.matches("data_base64").count(),1);assert_eq!(snap.undo_depth,1);
    s.command(Command::Undo{document_id:"main".into(),expected_revision:snap.revision}).unwrap();assert!(s.snapshot("main").unwrap().document.resources.is_empty());
    s.command(Command::Redo{document_id:"main".into(),expected_revision:snap.revision+1}).unwrap();assert_eq!(s.snapshot("main").unwrap().document_hash,snap.document_hash);
}

#[test]
fn file_import_failures_cancel_created_resources_and_other_edits() {
    let s=service();let before=s.snapshot("main").unwrap();
    let missing=temp().join("does-not-exist.jpg");let e=batch(&s,vec![import(image("first","new"),"quadrants.png"),Operation::ImageImport{path:missing.to_string_lossy().into_owned(),object:image("second","other")}]).unwrap_err();
    assert_eq!(e.code,"IO_ERROR");assert_eq!(e.failed_operation,Some(2));assert_eq!(e.rolled_back,Some(true));assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
    assert_eq!(batch(&s,vec![Operation::ImageImport{path:"relative.png".into(),object:image("relative","new")}]).unwrap_err().code,"INVALID_PATH");
}

#[test]
fn invalid_payload_mime_hash_bytes_and_colliding_ids_never_publish() {
    let s=service();let png=Resource::image(&fs::read(asset("quadrants.png")).unwrap()).unwrap();
    let mut mime=png.clone();mime.mime="image/jpeg".into();let mut hash=png.clone();hash.sha256="0".repeat(64);let mut encoded=png.clone();encoded.data_base64="broken base64".into();
    for r in [mime,hash,encoded] {let e=batch(&s,vec![create(rect("uncommitted")),Operation::ResourcePut{resource_id:"bad".into(),resource:r}]).unwrap_err();assert_eq!(e.code,"INVALID_RESOURCE");assert!(s.snapshot("main").unwrap().document.objects.is_empty());}
    for bytes in [b"not an image".as_slice(),&fs::read(asset("quadrants.jpg")).unwrap()[..32],&fs::read(asset("quadrants.png")).unwrap()[..35]] {assert_eq!(Resource::image(bytes).unwrap_err().code,"INVALID_RESOURCE");}
    batch(&s,vec![import(image("first","original"),"quadrants.png")]).unwrap();let before=s.snapshot("main").unwrap();
    assert_eq!(batch(&s,vec![import(image("second","original"),"quadrants.jpg")]).unwrap_err().code,"DUPLICATE_ID");assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
}

#[test]
fn resource_lifecycle_references_and_locked_parent_are_atomic() {
    let s=service();batch(&s,vec![import(image("image","source"),"quadrants.jpg"),Operation::ObjectDuplicate{object_id:"image".into(),new_id:"copy".into(),descendant_ids:Default::default()}]).unwrap();let before=s.snapshot("main").unwrap();
    let e=batch(&s,vec![Operation::ObjectDelete{object_id:"image".into()},Operation::ResourceDelete{resource_id:"source".into()}]).unwrap_err();assert_eq!(e.code,"RESOURCE_IN_USE");assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
    batch(&s,vec![Operation::ObjectDelete{object_id:"image".into()},Operation::ObjectDelete{object_id:"copy".into()},Operation::ResourceDelete{resource_id:"source".into()}]).unwrap();let empty=s.snapshot("main").unwrap();assert!(empty.document.resources.is_empty());s.command(Command::Undo{document_id:"main".into(),expected_revision:empty.revision}).unwrap();assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
    let mut group=object("locked",Geometry::Group);group.locked=true;batch(&s,vec![create(group)]).unwrap();let before=s.snapshot("main").unwrap();let mut child=image("child","new");child.parent_id=Some("locked".into());assert_eq!(batch(&s,vec![import(child,"quadrants.png")]).unwrap_err().code,"OBJECT_LOCKED");assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
}

#[test]
fn png_alpha_rgba_paints_and_group_opacity_match_independent_compositing() {
    let s=service();let mut img=image("png","source");img.x_mm=10.;img.y_mm=10.;batch(&s,vec![import(img,"quadrants.png")]).unwrap();let p=render(&s);
    close(pixel(&p,20.,20.),[239,74,59],0);close(pixel(&p,55.,20.),[154,209,178],1);close(pixel(&p,55.,40.),[255,255,255],0);
    let mut group=object("group",Geometry::Group);group.x_mm=100.;group.style.opacity=0.5;
    let mut a=rect("red");a.parent_id=Some("group".into());a.style.fill="#ff000080".into();a.style.stroke="#0000ff80".into();a.style.stroke_width_mm=4.;
    let mut b=rect("blue");b.parent_id=Some("group".into());b.x_mm=30.;b.style.fill="#0000ff".into();batch(&s,vec![create(group),create(a),create(b)]).unwrap();let p=render(&s);
    close(pixel(&p,110.,20.),[255,191,191],2);close(pixel(&p,145.,20.),[127,127,255],2);
    let svg=render::svg(&s.snapshot("main").unwrap().document,"page-1").unwrap();assert!(svg.contains("fill-opacity=\"0.5019607843137255\""));assert!(svg.contains("stroke-opacity=\"0.5019607843137255\""));
}

#[test]
fn butt_round_square_caps_change_real_stroke_pixels() {
    let s=service();for (i,cap) in [LineCap::Butt,LineCap::Round,LineCap::Square].into_iter().enumerate() {let mut o=object(&format!("line-{i}"),Geometry::Path{d:"M20 20L40 20".into(),fill_rule:Default::default()});o.y_mm=i as f64*30.;o.style.fill="none".into();o.style.stroke="#111111".into();o.style.stroke_width_mm=8.;o.style.stroke_linecap=cap;batch(&s,vec![create(o)]).unwrap();}
    let p=render(&s);close(pixel(&p,17.,20.),[255,255,255],0);close(pixel(&p,17.,50.),[17,17,17],1);close(pixel(&p,17.,80.),[17,17,17],1);
    close(pixel(&p,17.,53.),[255,255,255],0);close(pixel(&p,17.,83.),[17,17,17],1);
}

#[test]
fn miter_round_bevel_joins_and_miter_limit_change_real_pixels() {
    let s=service();for (i,(join,limit)) in [(LineJoin::Miter,4.),(LineJoin::Round,4.),(LineJoin::Bevel,4.),(LineJoin::Miter,1.)].into_iter().enumerate() {let mut o=object(&format!("join-{i}"),Geometry::Path{d:"M20 45L40 15L60 45".into(),fill_rule:Default::default()});o.x_mm=i as f64*70.;o.style.fill="none".into();o.style.stroke="#111111".into();o.style.stroke_width_mm=8.;o.style.stroke_linejoin=join;o.style.stroke_miterlimit=limit;batch(&s,vec![create(o)]).unwrap();}
    let p=render(&s);close(pixel(&p,40.,9.),[17,17,17],1);for x in [110.,180.,250.] {close(pixel(&p,x,9.),[255,255,255],0);}
    close(pixel(&p,110.,11.5),[17,17,17],5);close(pixel(&p,180.,11.5),[255,255,255],0);close(pixel(&p,250.,11.5),[255,255,255],0);
}

#[test]
fn object_and_legacy_image_rect_clips_intersect_and_gate_hit_testing() {
    let s=service();let mut o=image("image","source");o.x_mm=10.;o.y_mm=10.;o.clip=Some(Clip::Rect{rect_mm:[0.,0.,30.,40.]});if let Geometry::Image{clip_mm,..}=&mut o.geometry {*clip_mm=Some([10.,0.,50.,40.]);}
    batch(&s,vec![import(o,"quadrants.jpg")]).unwrap();let snap=s.snapshot("main").unwrap();let p=render(&s);
    close(pixel(&p,15.,20.),[255,255,255],0);close(pixel(&p,45.,20.),[255,255,255],0);close(pixel(&p,25.,20.),[239,74,59],3);
    assert_eq!(hit(&snap.document,25.,20.),Some("image".into()));assert_eq!(hit(&snap.document,15.,20.),None);assert_eq!(hit(&snap.document,45.,20.),None);
    let q=s.command(Command::ObjectGet{document_id:"main".into(),object_id:"image".into()}).unwrap();assert_eq!(q["active_clips"].as_array().unwrap().len(),2);
    batch(&s,vec![update("image",ObjectPatch{clip:Some(None),geometry:Some(Geometry::Image{resource_id:"source".into(),clip_mm:Some([-50.,-50.,200.,200.]),fit:ImageFit::Stretch}),..Default::default()})]).unwrap();
    let snap=s.snapshot("main").unwrap();assert_eq!(hit(&snap.document,5.,20.),None);assert_eq!(snap.document.bounds(snap.document.object("image").unwrap()).unwrap(),[10.,10.,60.,40.]);
}

#[test]
fn transformed_group_and_evenodd_path_hole_clip_gate_pixels_and_hits() {
    let s=service();let mut group=object("group",Geometry::Group);group.transform=[1.2,0.15,0.1,1.,80.,20.];group.clip=Some(Clip::Path{d:"M0 0H60V60H0Z M20 20H40V40H20Z".into(),fill_rule:FillRule::EvenOdd});
    let mut child=rect("child");child.parent_id=Some("group".into());child.width_mm=90.;child.height_mm=80.;child.clip=Some(Clip::Rect{rect_mm:[0.,0.,50.,70.]});batch(&s,vec![create(group),create(child)]).unwrap();let snap=s.snapshot("main").unwrap();let p=render(&s);
    let world=|x:f64,y:f64|(1.2*x+0.1*y+80.,0.15*x+y+20.);
    for (x,y,visible) in [(10.,10.,true),(30.,30.,false),(55.,10.,false),(65.,10.,false)] {let (x,y)=world(x,y);assert_eq!(hit(&snap.document,x,y).is_some(),visible);if visible {close(pixel(&p,x,y),[40,122,199],1);}else {close(pixel(&p,x,y),[255,255,255],0);}}
}

#[test]
fn empty_clips_hide_fully_and_null_patch_restores_original_geometry() {
    let s=service();let mut a=rect("a");a.clip=Some(Clip::Path{d:"".into(),fill_rule:Default::default()});let mut b=rect("b");b.x_mm=80.;b.clip=Some(Clip::Rect{rect_mm:[0.,0.,0.,40.]});batch(&s,vec![create(a),create(b)]).unwrap();let p=render(&s);close(pixel(&p,10.,10.),[255,255,255],0);close(pixel(&p,90.,10.),[255,255,255],0);
    let patch=ObjectPatch{x_mm:Some(5.),..Default::default()};assert!(serde_json::to_value(&patch).unwrap().get("clip").is_none());batch(&s,vec![update("a",patch)]).unwrap();assert!(s.snapshot("main").unwrap().document.object("a").unwrap().clip.is_some());
    let before=s.snapshot("main").unwrap();let r=s.execute_json(&json!({"protocol_version":1,"command":{"type":"batch","document_id":"main","expected_revision":before.revision,"unit":"mm","operations":[{"type":"object_update","object_id":"a","patch":{"clip":null}}]}}).to_string());assert!(r.ok);
    assert!(s.snapshot("main").unwrap().document.object("a").unwrap().clip.is_none());close(pixel(&render(&s),10.,10.),[40,122,199],0);s.command(Command::Undo{document_id:"main".into(),expected_revision:before.revision+1}).unwrap();assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
}

#[test]
fn contain_stretch_cover_place_original_pixels_without_destructive_crop() {
    let s=service();for (i,fit) in [ImageFit::Contain,ImageFit::Stretch,ImageFit::Cover].into_iter().enumerate() {let mut o=image(&format!("fit-{i}"),&format!("requested-{i}"));o.width_mm=60.;o.height_mm=60.;o.x_mm=10.+i as f64*90.;o.y_mm=10.;if let Geometry::Image{fit:f,..}=&mut o.geometry {*f=fit;}batch(&s,vec![import(o,"quadrants.jpg")]).unwrap();}
    let p=render(&s);close(pixel(&p,15.,15.),[255,255,255],0);close(pixel(&p,105.,15.),[239,74,59],3);close(pixel(&p,195.,15.),[239,74,59],3);close(pixel(&p,185.,20.),[255,255,255],0);close(pixel(&p,255.,20.),[255,255,255],0);assert_eq!(s.snapshot("main").unwrap().document.resources.len(),1);
}

#[test]
fn new_styles_clips_and_shared_blobs_roundtrip_and_old_native_hashes_still_open() {
    let s=service();let mut o=image("image","source");o.clip=Some(Clip::Path{d:"M0 0H50V35H0Z".into(),fill_rule:FillRule::EvenOdd});o.style.opacity=0.75;o.style.stroke_linecap=LineCap::Round;o.style.stroke_linejoin=LineJoin::Bevel;o.style.stroke_miterlimit=2.;batch(&s,vec![import(o,"quadrants.jpg"),Operation::ObjectDuplicate{object_id:"image".into(),new_id:"copy".into(),descendant_ids:Default::default()}]).unwrap();let before=s.snapshot("main").unwrap();let png=render(&s);
    let file=temp().join("资源样式裁剪.arranger");s.command(Command::Save{document_id:"main".into(),expected_revision:before.revision,path:file.to_string_lossy().into_owned(),overwrite:false}).unwrap();let loaded=native::open(&file).unwrap();assert_eq!(loaded.document,*before.document);assert!(png.data()==Renderer::new().render(&loaded.document,"page-1",127.,None).unwrap().pixels.data());
    let old=native::open(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v1-minimum-legacy.arranger")).unwrap();
    for o in &old.document.objects {let j=serde_json::to_value(o).unwrap();assert!(j.get("clip").is_none());assert!(j["style"].get("stroke_linecap").is_none());if matches!(o.geometry,Geometry::Image{..}) {assert!(j["geometry"].get("fit").is_none());}}
}

#[test]
fn mixed_import_style_clip_transactions_publish_once_or_cancel_completely() {
    let s=service();let before=s.snapshot("main").unwrap();
    for commit in [false,true] {
        let token=s.command(Command::Begin{document_id:"main".into(),expected_revision:before.revision,unit:"mm".into(),lease_ms:30000}).unwrap()["transaction_id"].as_str().unwrap().to_owned();
        for operation in [import(image("image","source"),"quadrants.png"),update("image",ObjectPatch{clip:Some(Some(Clip::Rect{rect_mm:[0.,0.,30.,30.]})),style:Some(Style{opacity:0.6,stroke_linejoin:LineJoin::Round,..Default::default()}),..Default::default()})] {s.command(Command::Execute{document_id:"main".into(),transaction_id:token.clone(),operation}).unwrap();}
        assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);assert_eq!(s.command(Command::ResourceGet{document_id:"main".into(),resource_id:"source".into()}).unwrap_err().code,"RESOURCE_NOT_FOUND");
        s.command(if commit {Command::Commit{document_id:"main".into(),transaction_id:token}}else{Command::Rollback{document_id:"main".into(),transaction_id:token}}).unwrap();
        if !commit {assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);}
    }
    let after=s.snapshot("main").unwrap();assert_eq!(after.undo_depth,1);s.command(Command::Undo{document_id:"main".into(),expected_revision:after.revision}).unwrap();assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
}

#[test]
fn invalid_style_clip_and_owned_bad_schema_roll_back_prior_imports() {
    let s=service();
    for (patch,code) in [
        (ObjectPatch{style:Some(Style{stroke_miterlimit:0.5,..Default::default()}),..Default::default()},"INVALID_STYLE"),
        (ObjectPatch{clip:Some(Some(Clip::Rect{rect_mm:[0.,0.,-1.,10.]})),..Default::default()},"INVALID_CLIP"),
        (ObjectPatch{clip:Some(Some(Clip::Path{d:"M0 0L10 10".into(),fill_rule:Default::default()})),..Default::default()},"INVALID_CLIP"),
    ] {let e=batch(&s,vec![import(image("image","new"),"quadrants.jpg"),update("image",patch)]).unwrap_err();assert_eq!(e.code,code);assert!(s.snapshot("main").unwrap().document.resources.is_empty());}
    let before=s.snapshot("main").unwrap();let token=s.command(Command::Begin{document_id:"main".into(),expected_revision:before.revision,unit:"mm".into(),lease_ms:30000}).unwrap()["transaction_id"].as_str().unwrap().to_owned();s.command(Command::Execute{document_id:"main".into(),transaction_id:token.clone(),operation:import(image("image","new"),"quadrants.jpg")}).unwrap();
    let r=s.execute_json(&json!({"protocol_version":1,"command":{"type":"execute","document_id":"main","transaction_id":token,"operation":{"type":"object_update","object_id":"image","patch":{"clip":{"kind":"unknown"}}}}}).to_string());assert_eq!(r.error.unwrap().code,"INVALID_REQUEST");assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);assert!(!s.snapshot("main").unwrap().busy);
}

#[test]
fn clipping_survives_shape_conversion_and_booleans_refuse_clipped_operands() {
    let s=service();let mut a=rect("a");a.clip=Some(Clip::Rect{rect_mm:[0.,0.,20.,20.]});batch(&s,vec![create(a),create(rect("b")),Operation::ObjectToPath{object_id:"a".into()}]).unwrap();let before=s.snapshot("main").unwrap();assert!(before.document.object("a").unwrap().clip.is_some());
    let e=batch(&s,vec![Operation::PathBoolean{left_id:"a".into(),right_id:"b".into(),result_id:"result".into(),operation:arranger_core::BooleanOp::Union,remove_inputs:true}]).unwrap_err();assert_eq!(e.code,"UNSUPPORTED_GEOMETRY");assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
    let mut text=object("text",Geometry::Text{content:"中文 Text".into(),font_family:"Microsoft YaHei".into(),font_size_mm:8.,text_path:None,layout:Default::default()});
    text.x_mm=100.;text.y_mm=100.;text.clip=Some(Clip::Rect{rect_mm:[0.,0.,16.,20.]});batch(&s,vec![create(text)]).unwrap();let image=render(&s);
    batch(&s,vec![Operation::TextToPath{object_id:"text".into()}]).unwrap();assert!(s.snapshot("main").unwrap().document.object("text").unwrap().clip.is_some());assert!(image.data()==render(&s).data(),"Clipped text conversion must preserve source clipping and pixels");
}

#[test]
fn original_metadata_inventory_matches_exact_payloads_and_native_save_keeps_bytes() {
    use base64::{Engine as _,engine::general_purpose::STANDARD};
    let fixture=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/assets/cdr/all-content.arranger");let mut d=native::open(&fixture).unwrap().document;
    // The old complete fixture has plain containers. Add a real PNG tEXt
    // chunk with a valid CRC so this test exercises actual metadata bytes.
    let r=d.resources.values_mut().find(|r|r.mime=="image/png").unwrap();let plain=STANDARD.decode(&r.data_base64).unwrap();let payload=b"Arranger-test\0retain original metadata";let mut chunk=b"tEXt".to_vec();chunk.extend(payload);let mut crc=0xffffffffu32;
    for &byte in &chunk {crc^=u32::from(byte);for _ in 0..8 {crc=(crc>>1)^if crc&1!=0 {0xedb88320}else{0};}}
    let mut raw=plain[..33].to_vec();raw.extend((payload.len() as u32).to_be_bytes());raw.extend(chunk);raw.extend((!crc).to_be_bytes());raw.extend(&plain[33..]);*r=Resource::image(&raw).unwrap();
    let before=d.hash();let mut records=0;
    for (id,r) in &d.resources {
        let raw=STANDARD.decode(&r.data_base64).unwrap();let info=resource::validate(r).unwrap();let query=resource::query(&d,id).unwrap();assert_eq!(query["image"]["metadata"],serde_json::to_value(&info.metadata).unwrap());
        for record in &info.metadata.records {assert_eq!(arranger_core::model::sha256(&raw[record.offset..record.offset+record.encoded_bytes]),record.sha256);records+=1;}
    }
    assert!(records>0);assert_eq!(d.hash(),before);let path=temp().join("metadata.arranger");native::save(&d,0,&path,None,false).unwrap();assert_eq!(native::open(&path).unwrap().document.resources,d.resources);
}

#[test]
fn metadata_checks_reject_truncation_without_panicking_or_guessing_colors() {
    for mime in ["image/png","image/jpeg"] {
        for n in 0..120 {let mut raw=vec![255;n];if n>8 && mime=="image/png"{raw[8..n.min(12)].fill(0xff);}
            let result=std::panic::catch_unwind(||resource::metadata(&raw,mime));assert!(result.is_ok());
        }
    }
    // Metadata provenance is not ICC parsing or EXIF interpretation.
    let payload=b"Exif\0\0uninterpreted";let mut raw=vec![0xff,0xd8,0xff,0xe1];raw.extend(((payload.len()+2) as u16).to_be_bytes());raw.extend(payload);raw.extend([0xff,0xd9]);
    let info=resource::metadata(&raw,"image/jpeg").unwrap();assert_eq!(info.records[0].kind,"jpeg_exif");assert!(info.needs_managed_interpretation());assert_eq!(info.records[0].sha256,arranger_core::model::sha256(payload));assert_eq!(info.jpeg_components,None);
}
