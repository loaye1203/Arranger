use arranger_core::{cdr, Command, Document, Geometry, ImageFit, Object, Resource, Service, Style, command::{ExportFormat, Operation, ObjectPatch}};
use kurbo::Point;
use serde_json::json;
use std::{fs,path::PathBuf,sync::atomic::{AtomicU64,Ordering}};
fn root()->PathBuf {PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")}
fn asset(path:&str)->Vec<u8>{fs::read(root().join(path)).unwrap()}
fn object(id:&str,g:Geometry)->Object {Object{id:id.into(),page_id:"page-1".into(),parent_id:None,x_mm:12.,y_mm:34.,width_mm:43.,height_mm:18.,transform:[1.,0.,0.,1.,0.,0.],style:Style{fill:"#123456".into(),stroke:"#987654".into(),stroke_width_mm:0.2,..Default::default()},visible:true,locked:false,clip:None,geometry:g}}
fn scene()->Document {
    let mut d=Document::new("source".into(),210.,297.).unwrap();
    let mut r=object("rect",Geometry::Rect{radius_mm:0.});r.transform=[0.8,0.6,-0.3,0.4,4.,-7.];d.objects.push(r);
    let mut e=object("ellipse",Geometry::Ellipse);e.x_mm=80.;e.transform=[-1.,0.2,0.4,0.9,170.,5.];d.objects.push(e);
    d.objects.push(object("curve",Geometry::Path{d:"M 0 0 C 1 20 30 -10 40 20 L 10 30 Z M 50 0 Q 70 20 90 0".into(),fill_rule:Default::default()}));d
}
fn near(a:f64,b:f64){assert!((a-b).abs()<0.00021,"{a} != {b}");}
fn dir()->PathBuf{static N:AtomicU64=AtomicU64::new(0);let p=root().join(format!("artifacts/cdr-tests/{}-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),N.fetch_add(1,Ordering::Relaxed)));fs::create_dir_all(&p).unwrap();fs::canonicalize(p).unwrap()}
#[test]
fn native_vectors_roundtrip_world_geometry_order_paint_and_parametric_types() {
    let d=scene();let (raw,warnings)=cdr::encode(&d,"page-1").unwrap();assert!(raw.starts_with(b"PK"));assert_eq!(warnings[0].code,"CDR_EXPERIMENTAL");
    assert_eq!(raw,cdr::encode(&d,"page-1").unwrap().0);
    let (out,_)=cdr::decode(&raw,"imported").unwrap();assert_eq!(out.id,"imported");assert_eq!(out.objects.iter().map(|o|o.id.as_str()).collect::<Vec<_>>(),vec!["rect","ellipse","curve"]);
    for (old,new) in d.objects.iter().zip(&out.objects){assert_eq!(old.style,new.style);for (a,b) in d.bounds(old).unwrap().into_iter().zip(out.bounds(new).unwrap()){near(a,b);}}
    assert!(matches!(out.objects[0].geometry,Geometry::Rect{radius_mm:0.}));assert!(matches!(out.objects[1].geometry,Geometry::Ellipse));
    let Geometry::Path{d:input,..}=&d.objects[2].geometry else{unreachable!()};let Geometry::Path{d:output,..}=&out.objects[2].geometry else{unreachable!()};
    let a=kurbo::BezPath::from_svg(input).unwrap();let b=kurbo::BezPath::from_svg(output).unwrap();
    let av=a.segments().collect::<Vec<_>>();let bv=b.segments().collect::<Vec<_>>();assert_eq!(av.len(),bv.len());
    use kurbo::ParamCurve;for (a,b) in av.iter().zip(&bv){for t in [0.,0.25,0.5,0.75,1.] {let p=a.eval(t);let q=b.eval(t);near(p.x,q.x);near(p.y,q.y);}}
}
#[test]
fn chinese_latin_multiline_baseline_and_size_remain_live_text() {
    let mut d=Document::new("source".into(),210.,297.).unwrap();let mut t=object("text",Geometry::Text{content:"中文 ¥éÿ €“” 42\n第二行".into(),font_family:"Microsoft YaHei".into(),font_size_mm:6.35,text_path:None,layout:Default::default()});t.style.stroke="none".into();t.style.stroke_width_mm=0.;d.objects.push(t);
    let (raw,_)=cdr::encode(&d,"page-1").unwrap();let (out,_)=cdr::decode(&raw,"imported").unwrap();
    let t=&out.objects[0];let Geometry::Text{content,font_size_mm,font_family,..}=&t.geometry else{panic!("text was flattened")};assert_eq!(content,"中文 ¥éÿ €“” 42\n第二行");near(*font_size_mm,6.35);assert_eq!(font_family,"Microsoft YaHei");
    assert_eq!([t.x_mm,t.y_mm],[d.objects[0].x_mm,d.objects[0].y_mm]);assert_eq!(t.transform,d.objects[0].transform);
    let baseline=out.world_transform(t).unwrap()*Point::new(0.,*font_size_mm);near(baseline.x,12.);near(baseline.y,40.35);
}
#[test]
fn rgba_pixels_including_transparent_rgb_and_shared_instances_survive() {
    let mut d=Document::new("source".into(),210.,297.).unwrap();
    let r=Resource::image(&asset("examples/assets/cdr/content14/source-assets/alpha-37x29.png"));
    // The checked-in resource path is selected from the public content manifest.
    let r=match r {Ok(r)=>r,Err(_)=>unreachable!()};d.resources.insert("shared".into(),r.clone());
    for id in ["a","b"] {let mut o=object(id,Geometry::Image{resource_id:"shared".into(),clip_mm:None,fit:ImageFit::Stretch});o.style.stroke="none".into();o.style.stroke_width_mm=0.;if id=="b" {o.transform=[1.2,0.,0.,0.8,30.,40.];}d.objects.push(o);}
    let (raw,warnings)=cdr::encode(&d,"page-1").unwrap();assert!(warnings.iter().any(|w|w.code=="CDR_RESOURCE_ORIGINAL"));
    let (out,warnings)=cdr::decode(&raw,"pixels").unwrap();assert_eq!(out.resources,d.resources);assert!(warnings.iter().any(|w|w.code=="CDR_RESOURCE_ORIGINAL"));
    use base64::{Engine as _,engine::general_purpose::STANDARD};
    let before=image::load_from_memory(&STANDARD.decode(&r.data_base64).unwrap()).unwrap().to_rgba8();let after=image::load_from_memory(&STANDARD.decode(&out.resources.values().next().unwrap().data_base64).unwrap()).unwrap().to_rgba8();assert_eq!(before,after);
    let ids=out.objects.iter().map(|o|match &o.geometry {Geometry::Image{resource_id,..}=>resource_id,_=>panic!()}).collect::<Vec<_>>();assert_eq!(ids[0],ids[1]);for (a,b) in d.objects.iter().zip(&out.objects){for (x,y) in d.bounds(a).unwrap().into_iter().zip(out.bounds(b).unwrap()){near(x,y);}}
}
#[test]
fn import_actual_native_corel_geometry_and_content_fixtures() {
    for (file,count) in [("geometry-profile-v14.cdr",3),("content-profile-v14.cdr",3)] {
        let (d,warnings)=cdr::decode(&asset(&format!("examples/assets/cdr/corel2019/{file}")),"native").unwrap();assert_eq!(d.objects.len(),count);assert!(!warnings.is_empty());d.validate().unwrap();
    }
}
#[test]
fn missing_inherited_paint_reference_is_rejected_even_without_text_objects() {
    // This original Mac output passed the old self-reader but failed to open
    // reliably in Corel2019: its inherited style references absent fill 2.
    let raw=asset("examples/assets/cdr/rust14/RUST-vectors-EXPERIMENTAL-VECTOR-polyline-v14-zip.cdr");
    let error=cdr::decode(&raw,"broken-native-reference").unwrap_err();
    assert_eq!(error.code,"INVALID_CDR");assert!(error.message.contains("inherited text style"));
}
#[test]
fn unsupported_export_never_modifies_destination_pending_backup_or_source() {
    let p=dir().join("keep.cdr");for ext in ["",".pending",".bak",".lock"]{fs::write(format!("{}{ext}",p.display()),b"keep").unwrap();}
    let mut d=scene();d.objects[0].style.fill="#12345680".into();d.objects[0].style.opacity=0.5;let original=d.hash();
    let err=cdr::export(&d,"page-1",&p,true).unwrap_err();assert_eq!(err.code,"CDR_UNSUPPORTED");assert_eq!(d.hash(),original);
    for ext in ["",".pending",".bak",".lock"]{assert_eq!(fs::read(format!("{}{ext}",p.display())).unwrap(),b"keep");}
}
#[test]
fn imported_service_is_dirty_and_edit_undo_native_save_leave_cdr_unchanged() {
    let p=dir();let path=p.join("source.cdr");let raw=cdr::encode(&scene(),"page-1").unwrap().0;fs::write(&path,&raw).unwrap();
    let s=Service::new();s.command(Command::CdrImport{document_id:"main".into(),path:path.to_string_lossy().into()}).unwrap();let before=s.snapshot("main").unwrap();assert!(before.dirty);assert!(before.saved_path.is_none());
    s.command(Command::Batch{document_id:"main".into(),expected_revision:0,unit:"mm".into(),operations:vec![Operation::ObjectUpdate{object_id:"rect".into(),patch:ObjectPatch{x_mm:Some(8.),..Default::default()}}]}).unwrap();
    s.command(Command::Undo{document_id:"main".into(),expected_revision:1}).unwrap();assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
    let native=p.join("copy.arranger");s.command(Command::Save{document_id:"main".into(),expected_revision:2,path:native.to_string_lossy().into(),overwrite:false}).unwrap();assert_eq!(arranger_core::native::open(&native).unwrap().document.hash(),before.document_hash);assert_eq!(fs::read(path).unwrap(),raw);
}
#[test]
fn malformed_import_does_not_insert_a_session_or_touch_existing_state() {
    let p=dir().join("bad.cdr");fs::write(&p,b"not a cdr").unwrap();let s=Service::new();s.command(Command::DocumentNew{document_id:"old".into(),width_mm:210.,height_mm:297.,unit:"mm".into()}).unwrap();let before=s.snapshot("old").unwrap().document_hash;
    assert!(s.command(Command::CdrImport{document_id:"bad".into(),path:p.to_string_lossy().into()}).is_err());assert!(s.snapshot("bad").is_err());assert_eq!(s.snapshot("old").unwrap().document_hash,before);
    assert!(s.command(Command::CdrImport{document_id:"old".into(),path:root().join("examples/assets/cdr/corel2019/geometry-profile-v14.cdr").to_string_lossy().into()}).is_err());assert_eq!(s.snapshot("old").unwrap().document_hash,before);
}
#[test]
fn rejected_features_are_not_flattened_or_omitted() {
    let mut d=scene();
    for change in 0..5 {let mut bad=d.clone();match change {0=>{bad.objects[0].style.fill="#12345680".into();bad.objects[0].style.stroke="#000000".into();bad.objects[0].style.opacity=0.5;},1=>{bad.objects[0].geometry=Geometry::Group;bad.objects[0].style.opacity=0.5;},2=>bad.objects[0].style.stroke_miterlimit=10000.,3=>{bad.objects[2].style.stroke="#00000080".into();bad.objects[2].style.fill="#12345640".into();bad.objects[2].style.opacity=0.5;},_=>bad.pages[0].width_mm=0.000001};assert!(cdr::encode(&bad,"page-1").is_err(),"case {change}");}
    d.objects[0].page_id=arranger_core::WORKSPACE_PAGE.into();let out=cdr::decode(&cdr::encode_document(&d).unwrap().0,"workspace").unwrap().0;assert_eq!(out.object("rect").unwrap().page_id,arranger_core::WORKSPACE_PAGE);
}
#[test]
fn truncated_zip_and_riff_corpus_never_panics_or_returns_partial_documents() {
    let raw=asset("examples/assets/cdr/corel2019/geometry-profile-v14.cdr");for cut in (0..raw.len()).step_by(67) {let result=std::panic::catch_unwind(||cdr::decode(&raw[..cut],"test"));assert!(result.is_ok(),"panic at {cut}");assert!(result.unwrap().is_err(),"partial accepted at {cut}");}
    for byte in [0u8,1,255] {for len in [0,1,3,4,7,8,12,22,100,400] {assert!(cdr::decode(&vec![byte;len],"test").is_err());}}
    let mut wrong=raw.clone();wrong[0]^=1;assert!(cdr::decode(&wrong,"test").is_err());
    let riff=asset("examples/assets/cdr/vector14/EXPERIMENTAL-VECTOR-combined-v14-riff.cdr");
    for cut in (0..riff.len()).step_by(61) {assert!(std::panic::catch_unwind(||cdr::decode(&riff[..cut],"test")).unwrap().is_err());}
    let mut stored=cdr::encode(&scene(),"page-1").unwrap().0;
    let at=stored.windows(4).position(|v|v==b"RIFF").unwrap();stored[at+20]^=1;
    assert_eq!(cdr::decode(&stored,"bad-crc").unwrap_err().code,"INVALID_CDR");
}
#[test]
fn api_export_check_overwrite_and_schema_are_executable() {
    let p=dir().join("api.cdr");let s=Service::new();s.command(Command::DocumentNew{document_id:"main".into(),width_mm:210.,height_mm:297.,unit:"mm".into()}).unwrap();
    let r=s.execute_json(&json!({"protocol_version":1,"command":{"type":"save_check","document_id":"main","format":"cdr14"}}).to_string());assert!(r.ok);let result=serde_json::to_value(r).unwrap();assert_eq!(result["result"]["can_save"],false);assert_eq!(result["result"]["interchange"]["can_export"],true);
    let command=||Command::Export{document_id:"main".into(),page_id:"page-1".into(),format:ExportFormat::Cdr14,path:p.to_string_lossy().into(),overwrite:false,dpi:96.};s.command(command()).unwrap();assert_eq!(s.command(command()).unwrap_err().code,"OUTPUT_EXISTS");
    assert!(cdr::decode(&fs::read(p).unwrap(),"empty").unwrap().0.objects.is_empty());
}

fn pages_scene()->Document {
    let mut d=scene();
    d.pages.push(arranger_core::Page{id:"blank-middle".into(),width_mm:210.,height_mm:297.});
    d.pages.push(arranger_core::Page{id:"third".into(),width_mm:210.,height_mm:297.});
    d.pages.push(arranger_core::Page{id:"blank-last".into(),width_mm:210.,height_mm:297.});
    let mut a=object("third-bottom",Geometry::Ellipse);a.page_id="third".into();a.x_mm=70.;
    let mut b=object("third-top",Geometry::Rect{radius_mm:0.});b.page_id="third".into();b.x_mm=80.;
    // Deliberately interleave pages in the source object's global stack.
    d.objects.insert(1,a);d.objects.push(b);d
}
#[test]
fn document_interchange_keeps_empty_pages_page_order_and_each_page_stack() {
    let d=pages_scene();let (raw,warnings)=cdr::encode_document(&d).unwrap();assert_eq!(raw,cdr::encode_document(&d).unwrap().0);assert!(!warnings.iter().any(|n|n.code=="CDR_PAGE_ONLY"));
    let (out,warnings)=cdr::decode(&raw,"pages").unwrap();assert_eq!(out.pages,d.pages);assert!(!warnings.iter().any(|n|n.code=="CDR_PAGES_ASSIGNED"));
    for (i,p) in d.pages.iter().enumerate() {
        assert_eq!((out.pages[i].width_mm,out.pages[i].height_mm),(210.,297.));
        let before=d.objects.iter().filter(|o|o.page_id==p.id).collect::<Vec<_>>();let after=out.objects.iter().filter(|o|o.page_id==out.pages[i].id).collect::<Vec<_>>();
        assert_eq!(before.iter().map(|o|&o.id).collect::<Vec<_>>(),after.iter().map(|o|&o.id).collect::<Vec<_>>());
        for (a,b) in before.iter().zip(after){for (x,y) in d.bounds(a).unwrap().into_iter().zip(out.bounds(b).unwrap()){near(x,y);}}
    }
    let again=cdr::decode(&cdr::encode_document(&out).unwrap().0,"pages").unwrap().0;assert_eq!(out.hash(),again.hash());
}
#[test]
fn selected_page_is_explicit_and_does_not_include_other_pages_or_workspace() {
    let mut d=pages_scene();let mut outside=object("outside",Geometry::Rect{radius_mm:0.});outside.page_id=arranger_core::WORKSPACE_PAGE.into();outside.x_mm=-50.;d.objects.push(outside);
    assert_eq!(cdr::decode(&cdr::encode_document(&d).unwrap().0,"workspace").unwrap().0.objects.iter().filter(|o|o.page_id==arranger_core::WORKSPACE_PAGE).count(),1);
    let (raw,warnings)=cdr::encode(&d,"third").unwrap();assert!(warnings.iter().any(|n|n.code=="CDR_PAGE_ONLY"));let out=cdr::decode(&raw,"one").unwrap().0;assert_eq!(out.pages.len(),1);assert_eq!(out.objects.iter().map(|o|o.id.as_str()).collect::<Vec<_>>(),vec!["third-bottom","third-top"]);
    let blank=cdr::decode(&cdr::encode(&d,"blank-middle").unwrap().0,"blank").unwrap().0;assert!(blank.objects.is_empty());
}
#[test]
fn cross_page_bitmap_resource_remains_one_shared_native_resource() {
    let mut d=pages_scene();let r=Resource::image(&asset("examples/assets/cdr/content14/source-assets/alpha-37x29.png")).unwrap();d.resources.insert("shared".into(),r);
    for (id,page) in [("image-first","page-1"),("image-third","third")] {let mut o=object(id,Geometry::Image{resource_id:"shared".into(),clip_mm:None,fit:ImageFit::Stretch});o.page_id=page.into();o.style.stroke="none".into();o.style.stroke_width_mm=0.;d.objects.push(o);}
    let out=cdr::decode(&cdr::encode_document(&d).unwrap().0,"shared").unwrap().0;assert_eq!(out.resources.len(),1);
    let images=out.objects.iter().filter_map(|o|if let Geometry::Image{resource_id,..}=&o.geometry {Some((&o.page_id,resource_id))}else{None}).collect::<Vec<_>>();assert_eq!(images.len(),2);assert_ne!(images[0].0,images[1].0);assert_eq!(images[0].1,images[1].1);
}
#[test]
fn actual_corel_two_page_cmyk_text_retains_native_channels_without_rgb_substitution() {
    for file in ["two-pages-v14.cdr","two-pages-v14-edited.cdr"] {
        let (doc,notices)=cdr::decode(&asset(&format!("examples/assets/cdr/corel2019/references/{file}")),"actual").unwrap();
        assert_eq!(doc.pages.len(),2);assert!(notices.iter().any(|n|n.code=="CMYK_RGB_APPROXIMATION"));
        assert!(doc.objects.iter().any(|o|o.style.fill=="cmyk(0,0,0,100)"));
        let out=cdr::decode(&cdr::encode_document(&doc).unwrap().0,"actual").unwrap().0;
        for o in &doc.objects {assert_eq!(o.style,out.object(&o.id).unwrap().style);}
    }
}
#[test]
fn all_pages_api_export_keeps_revision_dirty_binding_and_undo_history() {
    let p=dir();let input=p.join("pages.arranger");arranger_core::native::save(&pages_scene(),0,&input,None,false).unwrap();let s=Service::new();s.command(Command::DocumentOpen{document_id:"main".into(),path:input.to_string_lossy().into()}).unwrap();let before=s.snapshot("main").unwrap();
    let path=p.join("all.cdr");let r=s.execute_json(&json!({"protocol_version":1,"command":{"type":"cdr_export","document_id":"main","path":path}}).to_string());assert!(r.ok,"{r:?}");let after=s.snapshot("main").unwrap();assert_eq!((after.document_hash,after.revision,after.dirty,after.saved_path),(before.document_hash,0,false,before.saved_path));assert_eq!(cdr::decode(&fs::read(&path).unwrap(),"out").unwrap().0.pages.len(),4);
    assert!(s.command(Command::Undo{document_id:"main".into(),expected_revision:0}).is_err());assert_eq!(s.command(Command::CdrExport{document_id:"main".into(),path:path.to_string_lossy().into(),format:"cdr14".into(),overwrite:false,mode:None,page_id:None}).unwrap_err().code,"OUTPUT_EXISTS");
}
#[test]
fn late_page_unsupported_object_is_atomic_before_any_destination_write() {
    let mut d=pages_scene();d.objects.last_mut().unwrap().style.fill="#12345680".into();d.objects.last_mut().unwrap().style.opacity=0.4;let p=dir().join("keep.cdr");fs::write(&p,b"original").unwrap();let hash=d.hash();assert_eq!(cdr::export_document(&d,&p,true).unwrap_err().code,"CDR_UNSUPPORTED");assert_eq!(fs::read(&p).unwrap(),b"original");assert_eq!(d.hash(),hash);
    d.objects.last_mut().unwrap().style.fill="#123456".into();d.objects.last_mut().unwrap().style.opacity=1.;d.pages[3].width_mm=0.000001;assert!(cdr::encode_document(&d).is_err());assert!(cdr::encode(&d,"page-1").is_ok());
}
