use arranger_core::{Document,Geometry,Object,cdr,native,render::Renderer};
use serde_json::json;
use std::{fs,path::{Path,PathBuf},sync::atomic::{AtomicU64,Ordering},time::{SystemTime,UNIX_EPOCH}};
fn doc()->Document{serde_json::from_value(json!({"id":"fixed","unit":"mm","pages":[{"id":"page-1","width_mm":50.,"height_mm":40.}],"resources":{},"objects":[{"id":"line","page_id":"page-1","x_mm":0.,"y_mm":0.,"width_mm":10.,"height_mm":1.,"transform":[2.,0.,0.5,0.5,8.,15.],"geometry":{"kind":"path","d":"M0 0L10 0"},"style":{"fill":"none","stroke":"#000000","stroke_width_mm":2.,"opacity":1.,"stroke_scale_with_object":false}}]})).unwrap()}
static TEMP_SEQUENCE:AtomicU64=AtomicU64::new(0);
fn temp_in(base:&Path,stamp:u128,sequence:&AtomicU64)->PathBuf{
 loop{let n=sequence.fetch_add(1,Ordering::Relaxed);let p=base.join(format!("arranger-fixed-{}-{stamp}-{n}",std::process::id()));
  match fs::create_dir(&p){Ok(())=>return p,Err(e)if e.kind()==std::io::ErrorKind::AlreadyExists=>continue,Err(e)=>panic!("create fixed-stroke test directory: {e}")}
 }
}
fn temp()->PathBuf{temp_in(&std::env::temp_dir(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),&TEMP_SEQUENCE)}
#[test]
fn concurrent_temp_names_retry_collisions_without_overwriting_existing_files(){
 let root=temp();let sequence=AtomicU64::new(0);let first=root.join(format!("arranger-fixed-{}-42-0",std::process::id()));fs::create_dir(&first).unwrap();fs::write(first.join("sentinel"),b"keep").unwrap();
 let paths=std::thread::scope(|scope|{let handles=(0..16).map(|_|scope.spawn(||(0..8).map(|_|temp_in(&root,42,&sequence)).collect::<Vec<_>>())).collect::<Vec<_>>();handles.into_iter().flat_map(|h|h.join().unwrap()).collect::<std::collections::BTreeSet<_>>()});
 assert_eq!(paths.len(),128);assert!(paths.iter().all(|p|p.is_dir()&&p!=&first));assert_eq!(fs::read(first.join("sentinel")).unwrap(),b"keep");fs::remove_dir_all(root).unwrap();
}

#[test]
fn fixed_stroke_render_and_hit_are_in_page_units_under_nonuniform_shear(){
 let d=doc();let mut flat=d.clone();let o=&mut flat.objects[0];o.transform=[1.,0.,0.,1.,0.,0.];o.geometry=Geometry::Path{d:"M8 15L28 15".into(),fill_rule:Default::default()};o.style.stroke_scale_with_object=true;
 let renderer=Renderer::new();let a=renderer.render(&d,"page-1",254.,None).unwrap().pixels;let b=renderer.render(&flat,"page-1",254.,None).unwrap().pixels;assert_eq!(a.data(),b.data());
 assert_eq!(d.hit_test("page-1",[18.,15.9],0.),Some("line".into()));assert_eq!(d.hit_test("page-1",[18.,16.1],0.),None);
 let mut scaled=d.clone();scaled.objects[0].style.stroke_scale_with_object=true;let c=renderer.render(&scaled,"page-1",254.,None).unwrap().pixels;assert_ne!(a.data(),c.data(),"fixed and transformed widths must differ");
}

#[test]
fn fixed_stroke_cdr14_and21_keep_exact_source_for_primitives_curves_parents_and_fine_units(){
 for version in [14,21]{for kind in 0..4{for parent in [false,true]{
  let mut d=doc();let o=&mut d.objects[0];o.width_mm=20.;o.height_mm=10.;o.transform=[1.17,0.23,-0.31,0.91,15.,12.];o.style.fill="#123456".into();o.style.stroke_width_mm=0.1234567;
  o.geometry=match kind{0=>Geometry::Rect{radius_mm:0.},1=>Geometry::Rect{radius_mm:0.75},2=>Geometry::Ellipse,_=>Geometry::Path{d:"M0.1234567 0C3.1234567 9.9876543 9.1111111 -2.3333333 20 10Z".into(),fill_rule:Default::default()}};
  if parent{let mut g:Object=serde_json::from_value(json!({"id":"parent","page_id":"page-1","x_mm":0.,"y_mm":0.,"width_mm":0.,"height_mm":0.,"geometry":{"kind":"group"}})).unwrap();g.transform=d.objects[0].transform;d.objects[0].transform=[1.,0.,0.,1.,0.,0.];d.objects[0].parent_id=Some("parent".into());d.objects.insert(0,g);}
  let encode=if version==14{cdr::encode_document}else{cdr::encode_document21};let bytes=encode(&d).unwrap().0;let r=cdr::decode(&bytes,&d.id).unwrap().0;assert_eq!(r,d,"v{version} kind{kind} parent{parent}");assert_eq!(cdr::decode(&encode(&r).unwrap().0,&d.id).unwrap().0,d);
 }}}
}

#[test]
fn actual_fixed_controls_retain_world_mode_and_existing_vector_types(){
 let root=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/assets/cdr/corel2019/rectangle-scales");
 for version in [14,21]{for profile in ["identity","uniform2","nonuniform2x1"]{let p=root.join(format!("outline-fixed-{profile}-v{version}.cdr"));let d=cdr::decode(&fs::read(p).unwrap(),"native-fixed").unwrap().0;assert!(d.objects.iter().all(|o|o.style.stroke=="none"||!o.style.stroke_scale_with_object));assert!(d.objects.iter().all(|o|matches!(o.geometry,Geometry::Rect{..}|Geometry::Path{..})));for encode in [cdr::encode_document,cdr::encode_document21]{let r=cdr::decode(&encode(&d).unwrap().0,&d.id).unwrap().0;assert_eq!(r,d);}}}
}

#[test]
fn native_v4_roundtrip_and_version_guard_prevent_recovery_into_an_old_contract(){
 let path=temp().join("fixed.arranger");let d=doc();native::save(&d,7,&path,None,false).unwrap();let bytes=fs::read(&path).unwrap();let mut value:serde_json::Value=serde_json::from_slice(&bytes).unwrap();assert_eq!(value["format_version"],4);let loaded=native::open(&path).unwrap();assert_eq!(loaded.document,d);assert_eq!(loaded.revision,7);
 fs::write(path.with_extension("arranger.bak"),&bytes).unwrap();value["format_version"]=json!(3);fs::write(&path,serde_json::to_vec(&value).unwrap()).unwrap();assert_eq!(native::open(&path).err().unwrap().code,"UNSUPPORTED_FILE_VERSION");
 // An incompatible declared contract takes precedence over validation and
 // recovery, even if the new field also exposes an invalid world transform.
 value["document"]["objects"][0]["transform"]=json!([0.,0.,0.,0.,8.,15.]);let primary=serde_json::to_vec(&value).unwrap();fs::write(&path,&primary).unwrap();assert_eq!(native::open(&path).err().unwrap().code,"UNSUPPORTED_FILE_VERSION");assert_eq!(fs::read(&path).unwrap(),primary);assert_eq!(fs::read(path.with_extension("arranger.bak")).unwrap(),bytes);fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn default_scaling_still_omits_the_new_field_and_uses_old_native_versions(){
 let mut d=doc();d.objects[0].style.stroke_scale_with_object=true;let raw=serde_json::to_value(&d).unwrap();assert!(raw["objects"][0]["style"].get("stroke_scale_with_object").is_none());let p=temp().join("ordinary.arranger");native::save(&d,0,&p,None,false).unwrap();assert_eq!(serde_json::from_slice::<serde_json::Value>(&fs::read(&p).unwrap()).unwrap()["format_version"],1);fs::remove_dir_all(p.parent().unwrap()).unwrap();
}
