use arranger_core::{cdr,Document,FillRule,Resource};
use serde_json::json;
use std::{fs,path::PathBuf};
fn root()->PathBuf{PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")}
fn scene(rule:&str)->Document {
 let mut doc=Document::new("clip-rules".into(),210.,297.).unwrap();
 for value in [
  json!({"id":"arranger-clip-5","page_id":"page-1","x_mm":0.,"y_mm":0.,"width_mm":70.,"height_mm":50.,"transform":[-1.,0.2,0.1,1.,100.,40.],"geometry":{"kind":"group"},"clip":{"kind":"path","d":"M0 0 L70 0 L70 50 L0 50 Z M10 10 L60 10 L60 40 L10 40 Z","fill_rule":rule}}),
  json!({"id":"member","page_id":"page-1","parent_id":"arranger-clip-5","x_mm":0.,"y_mm":0.,"width_mm":70.,"height_mm":50.,"geometry":{"kind":"rect","radius_mm":0.},"style":{"fill":"#123456","stroke":"none","stroke_width_mm":0.,"opacity":1.}}),
  json!({"id":"image","page_id":"page-1","x_mm":20.,"y_mm":130.,"width_mm":60.,"height_mm":40.,"geometry":{"kind":"image","resource_id":"pixels","fit":"cover","clip_mm":[2.,2.,55.,35.]},"clip":{"kind":"path","d":"M0 0 L60 0 L60 40 L0 40 Z M10 10 L50 10 L50 30 L10 30 Z","fill_rule":rule},"style":{"fill":"none","stroke":"none","stroke_width_mm":0.,"opacity":1.}})
 ] {doc.objects.push(serde_json::from_value(value).unwrap());}
 doc.resources.insert("pixels".into(),Resource::png(&fs::read(root().join("examples/assets/images/quadrants.png")).unwrap()).unwrap());doc
}
#[test]
fn native14_and21_keep_clip_rules_through_mirrored_groups_and_fitted_image_viewports() {
 for modern in [false,true] {
 for (name,rule) in [("evenodd",FillRule::EvenOdd),("nonzero",FillRule::NonZero)] {
  let source=scene(name);let hash=source.hash();let encode=if modern {cdr::encode_document21}else{cdr::encode_document};let bytes=encode(&source).unwrap().0;assert_eq!(source.hash(),hash);
  let out=cdr::decode(&bytes,"roundtrip").unwrap().0;assert_eq!(out.object("arranger-clip-5").unwrap().clip.as_ref().unwrap().rule(),rule);
  let image=out.object("image").unwrap();assert_eq!(image.clip.as_ref().unwrap().rule(),rule);
  assert_eq!(image.geometry,source.object("image").unwrap().geometry);assert_eq!(image.parent_id,None);
  assert_eq!(out.objects.iter().filter(|o|o.clip.is_some()).count(),2,"source custom clip and source group clip survive; generated image viewport is represented by fit/clip_mm");
  let again=cdr::decode(&encode(&out).unwrap().0,"roundtrip").unwrap().0;assert_eq!(out,again);
 }}
}
#[test]
fn native14_preserves_existing_files_when_another_unsupported_effect_is_present() {
 let mut doc=scene("evenodd");doc.objects[0].style.opacity=0.5;let hash=doc.hash();let check=cdr::check_document(&doc);assert_eq!(check["can_export"],false);
 let path=root().join(format!("artifacts/clip-rule-protection-{}.cdr",std::process::id()));
 for suffix in ["",".pending",".bak",".lock"]{fs::write(format!("{}{suffix}",path.display()),b"unchanged").unwrap();}
 assert_eq!(cdr::export_document(&doc,&path,true).unwrap_err().code,"CDR_UNSUPPORTED");assert_eq!(hash,doc.hash());
 for suffix in ["",".pending",".bak",".lock"]{assert_eq!(fs::read(format!("{}{suffix}",path.display())).unwrap(),b"unchanged");}
 assert!(cdr::encode_document(&scene("nonzero")).is_ok());
}
