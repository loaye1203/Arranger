use arranger_core::{cdr,Document,Geometry,LineCap,LineJoin};
use serde_json::json;
use std::{fs,path::PathBuf};
fn source()->Document {
 serde_json::from_value(json!({"id":"main","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"objects":[{"id":"rounded","page_id":"page-1","parent_id":null,"x_mm":25.,"y_mm":30.,"width_mm":40.,"height_mm":20.,"transform":[-1.,0.,0.,1.,100.,0.],"style":{"fill":"#123456","stroke":"#abcdef","stroke_width_mm":0.4,"opacity":1.},"visible":true,"locked":false,"geometry":{"kind":"rect","radius_mm":2.}}],"resources":{}})).unwrap()
}
#[test]
fn rounded_rectangles_and_all_solid_cap_join_pairs_stay_parametric() {
 for cap in [LineCap::Butt,LineCap::Round,LineCap::Square] {for join in [LineJoin::Miter,LineJoin::Round,LineJoin::Bevel] {
  let mut d=source();d.objects[0].style.stroke_linecap=cap;d.objects[0].style.stroke_linejoin=join;d.objects[0].style.stroke_miterlimit=if join==LineJoin::Miter {8.}else{2.};
  let before=d.hash();let bytes=cdr::encode_document(&d).unwrap().0;let out=cdr::decode(&bytes,"roundtrip").unwrap().0;
  assert_eq!(d.hash(),before);let o=out.object("rounded").unwrap();assert_eq!(o.geometry,Geometry::Rect{radius_mm:2.});assert_eq!(o.style,d.objects[0].style);
  let a=d.bounds(&d.objects[0]).unwrap();let b=out.bounds(o).unwrap();for(x,y)in a.into_iter().zip(b){assert!((x-y).abs()<0.00011);}
  assert_eq!(cdr::decode(&cdr::encode_document(&out).unwrap().0,"roundtrip").unwrap().0,out);
 }}
}
#[test]
fn actual_corel_compound_references_keep_two_subpaths_round_caps_and_radius() {
 let base=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reports/windows/WIN-CDR-SAVE-20261022-01/focused/native-preservation");
 for suffix in ["","-edited"] {
  let raw=fs::read(base.join(format!("compound-v14{suffix}.cdr"))).unwrap();let d=cdr::decode(&raw,"native").unwrap().0;
  assert_eq!(d.objects.len(),2);assert_eq!(d.object("arr-rounded").unwrap().geometry,Geometry::Rect{radius_mm:2.});
  let compound=d.object("arr-compound").unwrap();assert_eq!(compound.style.stroke_linecap,LineCap::Round);
  let Geometry::Path{d:path,..}=&compound.geometry else{panic!()};assert_eq!(path.matches('M').count(),2);
  let again=cdr::decode(&cdr::encode_document(&d).unwrap().0,"native").unwrap().0;
  assert_eq!(again.object("arr-rounded").unwrap().geometry,Geometry::Rect{radius_mm:2.});
  assert_eq!(again.object("arr-compound").unwrap().style,compound.style);
 }
}
