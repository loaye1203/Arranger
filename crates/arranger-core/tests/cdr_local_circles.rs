//! Actual Corel122 references and the changed21 representation, no native run.
use arranger_core::{cdr,Document,Geometry};
use serde_json::Value;
use std::{fs,path::PathBuf};
fn root()->PathBuf{PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")}
#[test]
fn actual_corel_local_circle_groups_keep_native_parameters_and_separately_observe_sdk_boxes(){
 let folder=root().join("examples/assets/cdr/corel2019/save122-local-circles");let p:Value=serde_json::from_slice(&fs::read(folder.join("manifest.json")).unwrap()).unwrap();assert_eq!(p["cases"].as_array().unwrap().len(),8);let mut ellipse_box_differences=0;
 for c in p["cases"].as_array().unwrap(){let d=cdr::decode(&fs::read(folder.join(c["file"].as_str().unwrap())).unwrap(),"native").unwrap().0;
  for name in["content","curve-reference"]{let o=d.object(name).unwrap();assert!(o.style.stroke_scale_with_object);assert_eq!(o.style.stroke_width_mm,0.5);let expected=&c["sdk"][if name=="content"{"live"}else{"curve"}]["bounds_mm"];
   let error=d.bounds(o).unwrap().into_iter().zip(expected.as_array().unwrap()).map(|(a,b)|(a-b.as_f64().unwrap()).abs()).fold(0.,f64::max);
   if matches!(o.geometry,Geometry::Ellipse){let fields=&c["independent_binary_primitive"];assert_eq!([o.width_mm,o.height_mm],[fields["size_mm"][0].as_f64().unwrap(),fields["size_mm"][1].as_f64().unwrap()]);let m=d.world_transform(o).unwrap().as_coeffs();for(a,b)in m.into_iter().zip(fields["page_matrix"].as_array().unwrap()){assert!((a-b.as_f64().unwrap()).abs()<1e-11);}
    if error>0.00021{ellipse_box_differences+=1;}
   }else{assert!(error<=0.00021,"{} {name}: {error}",c["file"]);}
  }
  let raw=cdr::encode_document21(&d).unwrap().0;assert_eq!(cdr::decode(&raw,"native").unwrap().0,d,"{}",c["file"]);
 }
 assert_eq!(ellipse_box_differences,3,"SDK/model box differences are observed, not claimed exact");
}
#[test]
fn new21_affine_source_uses_changed_bytes_while_old_crash_files_stay_immutable(){
 let p:Value=serde_json::from_slice(&fs::read(root().join("examples/assets/cdr/affine-primitives-20261101/manifest.json")).unwrap()).unwrap();let mut count=0;
 for c in p["cases"].as_array().unwrap().iter().filter(|c|c["native_version"]==21){let d:Document=serde_json::from_value(c["document"].clone()).unwrap();let raw=cdr::encode_document21(&d).unwrap().0;assert_ne!(raw,fs::read(root().join(c["file"].as_str().unwrap())).unwrap());let actual=cdr::decode(&raw,&d.id).unwrap().0;assert_eq!(actual,d);assert!(matches!(actual.object("content").unwrap().geometry,Geometry::Rect{radius_mm:0.}|Geometry::Ellipse));count+=1;}
 assert_eq!(count,64);
}
#[test]
fn actual124_converted_returns_keep_curve_type_and_joint_physical_bound_through_two_exchanges(){
 let folder=root().join("examples/assets/cdr/corel2019/save124-converted");let p:Value=serde_json::from_slice(&fs::read(folder.join("manifest.json")).unwrap()).unwrap();
 assert_eq!(p["cases"].as_array().unwrap().len(),4);
 for c in p["cases"].as_array().unwrap(){let d=cdr::decode(&fs::read(folder.join(c["file"].as_str().unwrap())).unwrap(),"native").unwrap().0;
  assert!(matches!(d.object("content").unwrap().geometry,Geometry::Path{..}),"A lost native rectangle is never restored from Notes");
  let mut current=d.clone();for _ in 0..2{current=cdr::decode(&cdr::encode_document21(&current).unwrap().0,"native").unwrap().0;
   let a=d.object("content").unwrap();let b=current.object("content").unwrap();assert!(matches!(b.geometry,Geometry::Path{..}));assert_eq!(a.style,b.style);
   // An imported elliptical nib recharts the actual native point table.
   // Its irrational coordinates are normalized to the native grid on export;
   // this is a physical proof, not exact source-spelling acceptance.
   let pa=a.local_path().unwrap();let pb=b.local_path().unwrap();assert_eq!(pa.elements().len(),pb.elements().len());let ma=d.world_transform(a).unwrap();let mb=current.world_transform(b).unwrap();
   for(e,f)in pa.elements().iter().zip(pb.elements()){match(e,f){
    (kurbo::PathEl::MoveTo(p),kurbo::PathEl::MoveTo(q))|(kurbo::PathEl::LineTo(p),kurbo::PathEl::LineTo(q))=>{
     let pad=a.style.stroke_width_mm*a.style.stroke_miterlimit;for(x,y)in[(-1.,-1.),(1.,-1.),(1.,1.),(-1.,1.)]{let u=ma*(*p+kurbo::Vec2::new(x*pad,y*pad));let v=mb*(*q+kurbo::Vec2::new(x*pad,y*pad));assert!((u.x-v.x).abs()<=0.00021&&(u.y-v.y).abs()<=0.00021);}
    },(kurbo::PathEl::ClosePath,kurbo::PathEl::ClosePath)=>{},_=>panic!("unexpected actual124 polygon")}}
  }
 }
}
