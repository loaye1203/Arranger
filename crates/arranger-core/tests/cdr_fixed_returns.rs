//! Actual Windows122 returns, including preserved native type failures.
use arranger_core::{cdr,Document,Geometry};
use serde_json::Value;
use std::{fs,path::PathBuf};
fn root()->PathBuf{PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")}
#[test]
fn real_fixed_pen_returns_restore_the_complete_source_but_real_size_edits_win(){
 let folder=root().join("examples/assets/cdr/corel2019/save122-fixed");let packet:Value=serde_json::from_slice(&fs::read(folder.join("manifest.json")).unwrap()).unwrap();let(mut restored,mut edited)=(0,0);
 for row in packet["cases"].as_array().unwrap(){
  let raw=fs::read(folder.join(row["file"].as_str().unwrap())).unwrap();let d=cdr::decode(&raw,"actual").unwrap().0;
  if row["kind"]=="unchanged_native_return"{
   let sources:Value=serde_json::from_slice(&fs::read(root().join(row["source_packet"].as_str().unwrap())).unwrap()).unwrap();let source=sources["cases"].as_array().unwrap().iter().find(|r|r["name"]==row["source_name"]&&r["native_version"]==row["version"]).unwrap();let mut expected:Document=serde_json::from_value(source["document"].clone()).unwrap();expected.id="actual".into();let marker=expected.objects.iter_mut().find(|o|o.id=="edit-target").unwrap();marker.transform[4]+=1.25;marker.transform[5]+=2.5;assert_eq!(d,expected,"{}",row["file"]);
   for encode in [cdr::encode_document,cdr::encode_document21]{assert_eq!(cdr::decode(&encode(&d).unwrap().0,"actual").unwrap().0,d);}restored+=1;
  }else{
   let o=d.object("content").unwrap();assert!(!o.style.stroke_scale_with_object);let native_type=row["native_type"].as_u64().unwrap();assert!(if native_type==1{matches!(o.geometry,Geometry::Rect{radius_mm:0.})}else{native_type==3&&matches!(o.geometry,Geometry::Path{..})},"do not restore a real returned curve as the old rectangle");
   for(a,b)in d.bounds(o).unwrap().into_iter().zip(row["sdk_bounds_mm"].as_array().unwrap()){assert!((a-b.as_f64().unwrap()).abs()<=0.00021,"actual edit {}",row["file"]);}edited+=1;
  }
 }
 assert_eq!((restored,edited),(10,8));
}
