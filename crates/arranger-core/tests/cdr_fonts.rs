use arranger_core::{cdr,Document,Geometry};
use resvg::usvg::fontdb;
use serde_json::json;
fn source(family:&str)->Document {
 serde_json::from_value(json!({"id":"main","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"objects":[{"id":"artistic","page_id":"page-1","parent_id":null,"x_mm":20.,"y_mm":30.,"width_mm":80.,"height_mm":8.,"transform":[1.,0.,0.,1.,0.,0.],"style":{"fill":"#123456","stroke":"none","stroke_width_mm":0.,"opacity":1.},"visible":true,"locked":false,"geometry":{"kind":"text","content":"Native font / Cafe 42","font_family":family,"font_size_mm":6.35}}],"resources":{}})).unwrap()
}
fn installed()->String {
 let db=arranger_core::text::font_database();["Times New Roman","Helvetica","DejaVu Sans","Liberation Sans","Calibri"].into_iter().find(|f|db.query(&fontdb::Query{families:&[fontdb::Family::Name(f)],..Default::default()}).is_some()).expect("platform needs one common TrueType font").into()
}
#[test]
fn installed_fonts_keep_native_names_live_text_and_area_defaults() {
 let family=installed();let mut d=source(&family);let mut second=d.objects[0].clone();second.id="area".into();second.y_mm=70.;second.height_mm=40.;if let Geometry::Text{layout,content,..}=&mut second.geometry {layout.mode=arranger_core::TextMode::Area;layout.line_height_mm=Some(8.);layout.letter_spacing_mm=0.2;*content="Native paragraph\n\nBlank line\n".into();}d.objects.push(second);
 let hash=d.hash();let encoded=cdr::encode_document(&d).unwrap().0;assert_eq!(d.hash(),hash);let out=cdr::decode(&encoded,"roundtrip").unwrap().0;
 for name in ["artistic","area"] {let a=d.object(name).unwrap();let b=out.object(name).unwrap();let Geometry::Text{font_family,content,layout,..}=&b.geometry else{panic!("font converted to outlines")};assert_eq!(font_family,&family);let Geometry::Text{content:original,layout:before,..}=&a.geometry else{panic!()};assert_eq!(content,original);assert_eq!(layout.mode,before.mode);assert!((layout.letter_spacing_mm-before.letter_spacing_mm).abs()<0.000001);}
 let stable=cdr::decode(&cdr::encode_document(&out).unwrap().0,"roundtrip").unwrap().0;assert_eq!(stable,out);
}
#[test]
fn unresolved_font_is_a_concrete_preflight_blocker_and_source_is_unchanged() {
 let d=source("Arranger Missing Font 6673f2d0");let hash=d.hash();let check=cdr::check_document(&d);assert_eq!(check["can_export"],false);assert_eq!(check["blockers"][0]["object_id"],"artistic");assert!(check["blockers"][0]["error"]["message"].as_str().unwrap().contains("no installed face"));assert!(cdr::encode_document(&d).is_err());assert_eq!(d.hash(),hash);
}
