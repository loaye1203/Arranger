use arranger_core::{cdr,Geometry,TextAlign};
use std::{fs,path::PathBuf};
fn root()->PathBuf{PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")}
#[test]
fn actual_native21_simple_fonts_and_area_controls_decode_as_editable_model_objects() {
 let folders=[("fonts-native",vec!["font-arial","font-calibri","font-times"]),("layout-native",vec!["area-basic","area-width","area-height","area-align2","area-align3","area-spacing","area-leading"])];
 for(folder,names)in folders {for name in names {for suffix in ["","-edited"] {
  let p=root().join(format!("reports/windows/WIN-SAVE-ROUND-20261024-01/{folder}/{name}-v21{suffix}.cdr"));let raw=fs::read(&p).unwrap();let d=cdr::decode(&raw,"native21").unwrap_or_else(|e|panic!("{}: {e}",p.display())).0;assert_eq!(d.objects.len(),1);
  let Geometry::Text{content,font_family,font_size_mm,layout,..}=&d.objects[0].geometry else{panic!("native21 text lost")};assert_eq!(content.ends_with(" edited")||content.ends_with(" 已编辑"),!suffix.is_empty());assert!(*font_size_mm>0.);
  if folder=="fonts-native" {assert_eq!(font_family,match name {"font-arial"=>"Arial","font-calibri"=>"Calibri",_=>"Times New Roman"});assert!(layout.letter_spacing_mm>0.);}else{assert_eq!(layout.align,match name {"area-align2"=>TextAlign::Right,"area-align3"=>TextAlign::Center,_=>TextAlign::Left});assert_eq!(layout.letter_spacing_mm>0.,name=="area-spacing");assert_eq!(d.objects[0].width_mm,if name=="area-width" {120.}else{80.});}
 }}}
}
#[test]
fn actual_native21_identity_keeps_hidden_locked_objects_and_stable_names() {
 for suffix in ["","-edited"] {let p=root().join(format!("reports/windows/WIN-CDR-SAVE-20261022-01/focused/native-preservation/identity-v21{suffix}.cdr"));let d=cdr::decode(&fs::read(p).unwrap(),"identity21").unwrap().0;assert_eq!(d.objects.len(),3);assert!(!d.object("arr-hidden").unwrap().visible);assert!(d.object("arr-locked").unwrap().locked);assert!(matches!(d.object("arr-stable-name").unwrap().geometry,Geometry::Text{..}));}
}
#[test]
fn real21_model_encoding_keeps_live_geometry_text_images_and_clip_graph() {
 use arranger_core::{Document,Resource};use serde_json::json;
 let mut d=Document::new("model21".into(),210.,297.).unwrap();d.pages.push(arranger_core::Page{id:"second".into(),width_mm:148.,height_mm:210.});
 for (id,x,y,geometry,fill) in [("rounded",20.,30.,json!({"kind":"rect","radius_mm":2.}),"cmyk255(51,102,153,204)"),("oval",80.,30.,json!({"kind":"ellipse"}),"#123456"),("curve",20.,80.,json!({"kind":"path","d":"M0 0 L30 0 L30 20 L0 20 Z M5 5 L25 5 L25 15 L5 15 Z"}),"#287ac7"),("point",20.,120.,json!({"kind":"text","content":"Native21 / live\n\nLast\n","font_family":"Arial","font_size_mm":6.35,"layout":{"mode":"point","letter_spacing_mm":-0.08,"line_height_mm":7.5}}),"#123456"),("area",90.,120.,json!({"kind":"text","content":"Editable21 area\n\nThird line\n","font_family":"Arial","font_size_mm":6.35,"layout":{"mode":"area","align":"center","line_height_mm":8.}}),"#123456")]{
  let o=serde_json::from_value(json!({"id":id,"page_id":"page-1","x_mm":x,"y_mm":y,"width_mm":40.,"height_mm":30.,"geometry":geometry,"style":{"fill":fill,"stroke":"none","stroke_width_mm":0.,"opacity":1.}})).unwrap();d.objects.push(o);
 }
 let raw=fs::read(root().join("examples/assets/images/quadrants.png")).unwrap();d.resources.insert("pixels".into(),Resource::png(&raw).unwrap());d.objects.push(serde_json::from_value(json!({"id":"image","page_id":"second","x_mm":20.,"y_mm":30.,"width_mm":40.,"height_mm":25.,"geometry":{"kind":"image","resource_id":"pixels","fit":"stretch"},"clip":{"kind":"rect","rect_mm":[2.,2.,30.,18.]},"style":{"fill":"none","stroke":"none","stroke_width_mm":0.,"opacity":1.}})).unwrap());
 let hash=d.hash();let (bytes,warnings)=cdr::encode_document21(&d).unwrap();assert_eq!(d.hash(),hash);assert!(warnings.iter().any(|n|n.message.contains("CDR21")));let out=cdr::decode(&bytes,"roundtrip21").unwrap_or_else(|e|panic!("native21 generated: {e}")).0;assert_eq!(out.pages,d.pages);assert_eq!(out.objects.len(),6);
 for o in &d.objects {assert_eq!(std::mem::discriminant(&out.object(&o.id).unwrap().geometry),std::mem::discriminant(&o.geometry));}
 assert_eq!(out.object("rounded").unwrap().style.fill,"cmyk255(51,102,153,204)");assert!(out.object("image").unwrap().clip.is_some());let again=cdr::decode(&cdr::encode_document21(&out).unwrap().0,"roundtrip21").unwrap().0;assert_eq!(again,out);
 let target=root().join(format!("artifacts/cdr21-model-{}.cdr",std::process::id()));fs::write(&target,bytes).unwrap();eprintln!("native21 model candidate: {}",target.display());
}

#[test]
fn api_explicit21_and_default14_stay_readonly_and_full_save_remains_false() {
 use arranger_core::{Command,Service};use serde_json::json;
 let service=Service::new();service.command(Command::DocumentNew{document_id:"api21".into(),width_mm:210.,height_mm:297.,unit:"mm".into()}).unwrap();let before=service.snapshot("api21").unwrap().document_hash;
 let folder=root().join(format!("artifacts/cdr21-api-{}",std::process::id()));fs::create_dir_all(&folder).unwrap();
 for format in [None,Some("cdr21")] {let path=folder.join(if format.is_some(){"explicit21.cdr"}else{"default14.cdr"});let mut command=json!({"type":"cdr_export","document_id":"api21","path":path});if let Some(f)=format{command["format"]=json!(f);}let response=service.execute_json(&json!({"protocol_version":1,"command":command}).to_string());assert!(response.ok);assert_eq!(service.snapshot("api21").unwrap().document_hash,before);
 let raw=fs::read(path).unwrap();assert_eq!(raw.windows(4).find(|r|*r==b"CDRM"||*r==b"CDRE").unwrap(),if format.is_some(){b"CDRM"}else{b"CDRE"});
 }
 let status=arranger_core::save_check::analyze(&service.snapshot("api21").unwrap().document,arranger_core::save_check::SaveFormat::Cdr21).unwrap();assert_eq!(status["can_save"],false);assert_eq!(status["document_interchange"]["can_export"],true);
}

#[test]
fn unsupported21_export_protects_existing_output_and_recovery_files() {
 use arranger_core::Document;use serde_json::json;
 let mut doc=Document::new("opaque".into(),210.,297.).unwrap();doc.objects.push(serde_json::from_value(json!({"id":"partial","page_id":"page-1","x_mm":20.,"y_mm":20.,"width_mm":40.,"height_mm":20.,"geometry":{"kind":"rect","radius_mm":0.},"style":{"fill":"#12345680","stroke":"#000000","stroke_width_mm":1.,"opacity":0.5}})).unwrap());
 doc.objects[0].style.stroke_miterlimit=10000.;
 let path=root().join(format!("artifacts/cdr21-protected-{}.cdr",std::process::id()));for suffix in ["",".pending",".bak",".lock"]{fs::write(format!("{}{suffix}",path.display()),b"keep-original").unwrap();}let hash=doc.hash();assert_eq!(cdr::export_document21(&doc,&path,true).unwrap_err().code,"CDR_UNSUPPORTED");assert_eq!(hash,doc.hash());for suffix in ["",".pending",".bak",".lock"]{assert_eq!(fs::read(format!("{}{suffix}",path.display())).unwrap(),b"keep-original");}
}

#[test]
fn native21_explicit_evenodd_and_nonzero_keep_different_live_fill_rules() {
 use arranger_core::{Document,FillRule};use serde_json::json;
 for rule in ["evenodd","nonzero"] {let mut doc=Document::new("fill-rule".into(),210.,297.).unwrap();doc.objects.push(serde_json::from_value(json!({"id":"compound","page_id":"page-1","x_mm":20.,"y_mm":20.,"width_mm":40.,"height_mm":30.,"geometry":{"kind":"path","d":"M0 0 L40 0 L40 30 L0 30 Z M5 5 L35 5 L35 25 L5 25 Z","fill_rule":rule},"style":{"fill":"#123456","stroke":"none","stroke_width_mm":0.,"opacity":1.}})).unwrap());let hash=doc.hash();let raw=cdr::encode_document21(&doc).unwrap().0;let out=cdr::decode(&raw,"fill-rule").unwrap().0;let Geometry::Path{fill_rule,..}=out.objects[0].geometry else{panic!("path flattened")};assert_eq!(fill_rule,if rule=="evenodd"{FillRule::EvenOdd}else{FillRule::NonZero});assert_eq!(hash,doc.hash());}
}
