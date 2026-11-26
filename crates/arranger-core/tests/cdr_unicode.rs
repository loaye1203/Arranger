use arranger_core::{cdr,Document,Geometry,Page,Service,Command};
use serde_json::json;
use std::{fs,path::PathBuf};
fn root()->PathBuf{PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")}
fn directory(prefix:&str)->PathBuf{
    fs::create_dir_all(root().join("artifacts")).unwrap();
    for sequence in 0u64.. {
        let path=root().join(format!("artifacts/{prefix}-{}-{sequence}",std::process::id()));
        match fs::create_dir(&path){
            Ok(())=>return path,
            Err(error)if error.kind()==std::io::ErrorKind::AlreadyExists=>continue,
            Err(error)=>panic!("cannot create isolated test directory {}: {error}",path.display()),
        }
    }unreachable!()
}

fn scene(content:&str,area:bool)->Document {
    let mut doc=Document::new("unicode".into(),210.,297.).unwrap();
    doc.objects.push(serde_json::from_value(json!({"id":"live","page_id":"page-1","x_mm":20.,"y_mm":30.,"width_mm":90.,"height_mm":45.,"transform":[0.9,0.1,-0.1,1.05,3.,4.],"geometry":{"kind":"text","content":content,"font_family":"Microsoft YaHei","font_size_mm":6.35,"layout":{"mode":if area {"area"}else{"point"},"line_height_mm":8.}},"style":{"fill":"#123456","stroke":"none","stroke_width_mm":0.,"opacity":1.}})).unwrap());doc
}
#[test]
fn supplementary_scalars_combining_marks_and_blank_paragraphs_remain_live_in21() {
    let text="𠮷𠀀𝄞 e\u{301} A\n\n尾𩸽\n";
    for area in [false,true] {for modern in [false,true] {
        let doc=scene(text,area);let source=doc.hash();
        if !modern {assert_eq!(cdr::check_document(&doc)["can_export"],false);assert_eq!(cdr::encode_document(&doc).unwrap_err().code,"CDR_UNSUPPORTED");assert_eq!(doc.hash(),source);continue;}
        let encode=cdr::encode_document21;let (raw,warnings)=encode(&doc).unwrap();assert!(warnings.iter().any(|n|n.code=="CDR_SUPPLEMENTARY_GLYPH"),"native scalar preservation must not silently claim correct glyph rendering");assert_eq!(doc.hash(),source);
        let imported=cdr::decode(&raw,"unicode").unwrap().0;
        let Geometry::Text{content,layout,..}=&imported.objects[0].geometry else{panic!("live Unicode was outlined or rasterized")};assert_eq!(content,text);assert_eq!(layout.mode,if area {arranger_core::TextMode::Area}else{arranger_core::TextMode::Point});
        let again=cdr::decode(&encode(&imported).unwrap().0,"unicode").unwrap().0;assert_eq!(again,imported);
    }}
}
#[test]
fn actual_corel21_supplementary_area_resaves_keep_the_affine_frame_and_complete_text() {
    for edited in [false,true] {
        let file=if edited {"unicode-area-edited-v21.cdr"}else{"unicode-area-resaved-v21.cdr"};
        let raw=fs::read(root().join("examples/assets/cdr/corel2019").join(file)).unwrap();
        let doc=cdr::decode(&raw,"native-unicode").unwrap().0;let text=doc.object("unicode").unwrap();
        assert_eq!((text.width_mm,text.height_mm),(150.,60.));
        let Geometry::Text{content,layout,..}=&text.geometry else {panic!("Native text lost its editable geometry")};
        let expected=format!("𠮷 / 𠀀 / 𝄞 / e\u{301}\n\n保留𩸽和末尾段落\n{}",if edited {" / edited"}else{""});
        assert_eq!(content,&expected);assert_eq!(layout.mode,arranger_core::TextMode::Area);
    }
}
#[test]
fn limit_counts_utf16_units_and_rejected_unicode_never_overwrites_output() {
    let allowed="A".repeat(65532)+"𠮷";assert_eq!(allowed.encode_utf16().count(),65534);assert_eq!(cdr::check_document21(&scene(&allowed,true))["can_export"],true);
    assert_eq!(cdr::check_document(&scene(&"A".repeat(65534),true))["can_export"],true);
    let folder=directory("cdr-unicode");let path=folder.join("keep.cdr");
    for bad in [allowed+"A","𠮷".repeat(32768),"A\tB".into(),"A\0B".into()] {let doc=scene(&bad,false);let hash=doc.hash();for suffix in ["",".pending",".bak",".lock"]{fs::write(format!("{}{suffix}",path.display()),b"keep").unwrap();}
        for export in [cdr::export_document,cdr::export_document21] {assert_eq!(export(&doc,&path,true).unwrap_err().code,if bad.contains('\0'){"INVALID_TEXT"}else{"CDR_UNSUPPORTED"});assert_eq!(hash,doc.hash());for suffix in ["",".pending",".bak",".lock"]{assert_eq!(fs::read(format!("{}{suffix}",path.display())).unwrap(),b"keep");}}
    }
}
#[test]
fn native14_supplementary_failure_boundary_keeps_every_existing_output_unchanged() {
    let doc=scene("A𝄞B",false);let status=cdr::check_document(&doc);assert_eq!(status["can_export"],false);assert_eq!(status["blockers"][0]["object_id"],"live");
    let folder=directory("cdr-scalar-guard");let path=folder.join("keep.cdr");
    for suffix in ["",".pending",".bak",".lock"] {fs::write(format!("{}{suffix}",path.display()),b"keep").unwrap();}
    assert_eq!(cdr::export_document(&doc,&path,true).unwrap_err().code,"CDR_UNSUPPORTED");
    for suffix in ["",".pending",".bak",".lock"] {assert_eq!(fs::read(format!("{}{suffix}",path.display())).unwrap(),b"keep");}
    // Alternating scripts create more styles than the15-bit description slot.
    // They must not turn the style high bit into a surrogate-pair marker.
    let many=scene(&"A中".repeat(16384),false);let source=many.hash();
    for export in [cdr::export_document,cdr::export_document21] {
        assert_eq!(export(&many,&path,true).unwrap_err().code,"CDR_UNSUPPORTED");assert_eq!(many.hash(),source);
        for suffix in ["",".pending",".bak",".lock"] {assert_eq!(fs::read(format!("{}{suffix}",path.display())).unwrap(),b"keep");}
    }
}

#[test]
fn preflight_measures_encoded_world_coordinates_without_creating_output() {
    let mut doc=scene("𠮷",false);doc.objects[0].geometry=Geometry::Rect{radius_mm:0.};doc.objects[0].transform=[100000.,0.,0.,100000.,0.,0.];doc.validate().unwrap();let source=doc.hash();
    for page in [false,true] {let status=if page{cdr::check(&doc,"page-1")}else{cdr::check_document(&doc)};assert_eq!(status["can_export"],false);assert_eq!(status["error"]["code"],"INVALID_CDR");assert_eq!(status["blockers"][0]["scope"],"encoding");assert_eq!(source,doc.hash());}
    assert_eq!(cdr::check_document21(&doc)["can_export"],false);
}
#[test]
fn selected21_api_excludes_other_pages_and_desktop_and_preserves_empty_pages() {
    let mut doc=scene("𠮷 / page",false);doc.pages.push(Page{id:"blank".into(),width_mm:148.,height_mm:210.});let mut desktop=doc.objects[0].clone();desktop.id="shared".into();desktop.page_id="_workspace".into();doc.objects.push(desktop);
    let raw=cdr::encode_document21(&doc).unwrap().0;let folder=directory("cdr-selected21");let input=folder.join("source.cdr");fs::write(&input,raw).unwrap();
    let s=Service::new();s.command(Command::CdrImport{document_id:"source".into(),path:input.to_string_lossy().into()}).unwrap();let before=serde_json::to_value(s.snapshot("source").unwrap()).unwrap();
    for page in ["page-1","blank"] {let path=folder.join(format!("{page}.cdr"));let r=s.execute_json(&json!({"protocol_version":1,"command":{"type":"export","document_id":"source","page_id":page,"format":"cdr21","path":path}}).to_string());assert!(r.ok,"{}",serde_json::to_string(&r).unwrap());let body=serde_json::to_value(r).unwrap();assert!(body["result"]["warnings"].as_array().unwrap().iter().any(|n|n["code"]=="CDR_PAGE_ONLY"));let out=cdr::decode(&fs::read(&path).unwrap(),"selected").unwrap().0;assert_eq!(out.pages.len(),1);assert_eq!(out.pages[0].id,page);assert_eq!(out.objects.len(),if page=="blank"{0}else{1});assert_eq!(serde_json::to_value(s.snapshot("source").unwrap()).unwrap(),before);}
}
