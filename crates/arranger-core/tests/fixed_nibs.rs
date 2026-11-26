use arranger_core::{Document,Geometry,FixedStrokeNib,cdr,native,render::Renderer};
use serde_json::json;
use std::{fs,time::{SystemTime,UNIX_EPOCH}};
fn doc()->Document{serde_json::from_value(json!({"id":"fixed-nib","unit":"mm","pages":[{"id":"page-1","width_mm":50.,"height_mm":40.}],"resources":{},"objects":[{"id":"line","page_id":"page-1","x_mm":0.,"y_mm":0.,"width_mm":10.,"height_mm":1.,"transform":[2.,0.,0.5,0.5,8.,15.],"geometry":{"kind":"path","d":"M0 0L10 0"},"style":{"fill":"none","stroke":"#000000","stroke_width_mm":2.,"opacity":1.,"stroke_scale_with_object":false,"stroke_fixed_nib":{"stretch_percent":50,"angle_millionth_degrees":90000000}}}]})).unwrap()}
#[test]
fn page_elliptical_pen_matches_manual_affine_reference_and_hit_distance(){
 let d=doc();let mut reference=d.clone();let o=&mut reference.objects[0];o.transform=[1.,0.,0.,0.5,0.,0.];o.geometry=Geometry::Path{d:"M8 30L28 30".into(),fill_rule:Default::default()};o.style.stroke_scale_with_object=true;o.style.stroke_fixed_nib=None;
 let renderer=Renderer::new();let a=renderer.render(&d,"page-1",254.,None).unwrap();let b=renderer.render(&reference,"page-1",254.,None).unwrap();assert_eq!(a.pixels.data(),b.pixels.data());
 assert_eq!(d.hit_test("page-1",[18.,15.4],0.),Some("line".into()));assert_eq!(d.hit_test("page-1",[18.,15.6],0.),None);
 let mut thin=d.clone();thin.objects[0].style.stroke_fixed_nib.as_mut().unwrap().stretch_percent=1;thin.objects[0].style.stroke_linecap=arranger_core::LineCap::Round;assert_eq!(thin.hit_test("page-1",[29.2,15.],0.05),None,"page tolerance must not expand along the wide axis by100x");
 let mut vertical=d.clone();vertical.objects[0].style.stroke_fixed_nib.as_mut().unwrap().angle_millionth_degrees=0;assert_ne!(a.pixels.data(),renderer.render(&vertical,"page-1",254.,None).unwrap().pixels.data());
}
#[test]
fn both_cdr_versions_keep_primitive_identity_fixed_ellipse_and_complete_source(){
 for version in [14,21]{for kind in 0..4{for angle in [-90_000_000,0,30_000_000,70_542_347]{for parent in [false,true]{
  let mut d=doc();let o=&mut d.objects[0];o.width_mm=20.;o.height_mm=10.;o.x_mm=1.25;o.y_mm=2.5;o.transform=[1.17,0.23,0.31,0.91,8.,10.];o.style.fill="#123456".into();o.style.stroke_width_mm=0.1234567;o.style.stroke_fixed_nib=Some(FixedStrokeNib{stretch_percent:70,angle_millionth_degrees:angle});o.geometry=match kind{0=>Geometry::Rect{radius_mm:0.},1=>Geometry::Ellipse,2=>Geometry::Rect{radius_mm:0.75},_=>Geometry::Path{d:"M0.1234567 0C3.1234567 9.9876543 9.1111111 -2.3333333 20 10Z".into(),fill_rule:Default::default()}};
  if parent{let mut g:arranger_core::Object=serde_json::from_value(json!({"id":"parent","page_id":"page-1","x_mm":0.,"y_mm":0.,"width_mm":0.,"height_mm":0.,"geometry":{"kind":"group"}})).unwrap();g.transform=d.objects[0].transform;d.objects[0].transform=[1.,0.,0.,1.,0.,0.];d.objects[0].parent_id=Some("parent".into());d.objects.insert(0,g);}
  let encode=if version==14{cdr::encode_document}else{cdr::encode_document21};let a=cdr::decode(&encode(&d).unwrap().0,&d.id).unwrap().0;assert_eq!(a,d,"v{version} kind{kind} angle{angle} parent{parent}");assert_eq!(cdr::decode(&encode(&a).unwrap().0,&d.id).unwrap().0,d);
 }}}}
}
#[test]
fn inactive_nib_and_group_inert_paint_keep_exact_values(){
 for group in [false,true]{let mut d=doc();let o=&mut d.objects[0];o.style.stroke="none".into();if group{o.geometry=Geometry::Group;o.width_mm=0.;o.height_mm=0.;}
  for encode in [cdr::encode_document,cdr::encode_document21]{assert_eq!(cdr::decode(&encode(&d).unwrap().0,&d.id).unwrap().0,d);}
 }
}
#[test]
fn native5_rejects_downgraded_headers_before_validation_and_backup_recovery(){
 let dir=std::env::temp_dir().join(format!("arranger-nib-{}-{}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));fs::create_dir(&dir).unwrap();let path=dir.join("nib.arranger");let d=doc();native::save(&d,7,&path,None,false).unwrap();let bytes=fs::read(&path).unwrap();let mut v:serde_json::Value=serde_json::from_slice(&bytes).unwrap();assert_eq!(v["format_version"],5);assert_eq!(native::open(&path).unwrap().document,d);fs::write(path.with_extension("arranger.bak"),&bytes).unwrap();
 for version in 1..=4{v["format_version"]=json!(version);v["document"]["objects"][0]["transform"]=json!([0.,0.,0.,0.,0.,0.]);let raw=serde_json::to_vec(&v).unwrap();fs::write(&path,&raw).unwrap();assert_eq!(native::open(&path).err().unwrap().code,"UNSUPPORTED_FILE_VERSION");assert_eq!(fs::read(&path).unwrap(),raw);assert_eq!(fs::read(path.with_extension("arranger.bak")).unwrap(),bytes);}
 fs::remove_dir_all(dir).unwrap();
}
#[test]
fn nib_validation_rejects_invalid_modes_and_old_default_has_no_new_field(){
 let d=doc();for stretch in [0,100,101]{let mut bad=d.clone();bad.objects[0].style.stroke_fixed_nib.as_mut().unwrap().stretch_percent=stretch;assert_eq!(bad.validate().unwrap_err().code,"INVALID_STYLE");}let mut bad=d.clone();bad.objects[0].style.stroke_scale_with_object=true;assert_eq!(bad.validate().unwrap_err().code,"INVALID_STYLE");
 let mut old=d;old.objects[0].style.stroke_fixed_nib=None;let raw=serde_json::to_value(old).unwrap();assert!(raw["objects"][0]["style"].get("stroke_fixed_nib").is_none());
}
