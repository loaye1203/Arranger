//! Report120 actual native failures must never become silent format success.
use arranger_core::{cdr,native,Document,Geometry,Page};
use serde_json::Value;
use std::{fs,path::PathBuf};
fn root()->PathBuf{PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")}
fn source(version:u64,name:&str)->Document{
 let packet:Value=serde_json::from_slice(&fs::read(root().join("examples/assets/cdr/affine-primitives-20261101/manifest.json")).unwrap()).unwrap();
 serde_json::from_value(packet["cases"].as_array().unwrap().iter().find(|r|r["native_version"]==version&&r["name"]==name).unwrap()["document"].clone()).unwrap()
}
#[test]
fn fault_profiles_reject_all_page_and_selected_exports_before_any_destination_or_backup_write(){
 let folder=root().join(format!("artifacts/quarantine-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));fs::create_dir_all(&folder).unwrap();
 for(version,name)in[(14,"rect-fractional-1")]{
  let mut d=source(version,name);d.pages.push(Page{id:"safe-page".into(),width_mm:210.,height_mm:297.});let before=d.clone();
  // The offline reader remains available for diagnosis; this never opens
  // these quarantined bytes in Corel or claims its native type is accepted.
  let old=fs::read(root().join(format!("examples/assets/cdr/affine-primitives-20261101/affineprimitive{version}-{name}.cdr"))).unwrap();
  assert_eq!(cdr::decode(&old,&d.id).unwrap().0.objects,d.objects);
  let path=folder.join(format!("keep{version}.cdr"));for suffix in["",".bak",".pending",".lock"]{fs::write(format!("{}{suffix}",path.display()),b"keep-original").unwrap();}
  let e=if version==14{cdr::export_document(&d,&path,true)}else{cdr::export_document21(&d,&path,true)}.unwrap_err();assert_eq!(e.code,"CDR_NATIVE_PROFILE_QUARANTINED");assert_eq!(d,before);
  let e=if version==14{cdr::encode(&d,"page-1")}else{cdr::encode21(&d,"page-1")}.unwrap_err();assert_eq!(e.code,"CDR_NATIVE_PROFILE_QUARANTINED");
  assert!(if version==14{cdr::encode(&d,"safe-page")}else{cdr::encode21(&d,"safe-page")}.is_ok(),"unselected fault page must not block a clean selected page");
  for suffix in["",".bak",".pending",".lock"]{assert_eq!(fs::read(format!("{}{suffix}",path.display())).unwrap(),b"keep-original");}
  let full=folder.join(format!("complete{version}.arranger"));native::save(&d,0,&full,None,false).unwrap();let s=arranger_core::Service::new();s.command(arranger_core::Command::DocumentOpen{document_id:format!("complete{version}"),path:full.to_string_lossy().into()}).unwrap();let reopened=s.snapshot(&format!("complete{version}")).unwrap();assert_eq!(reopened.document.objects,d.objects);assert_eq!(reopened.document.pages,d.pages);
 }
}
#[test]
fn successful14_native_profiles_still_emit_original_primitives_and_same_bytes(){
 let packet:Value=serde_json::from_slice(&fs::read(root().join("examples/assets/cdr/affine-primitives-20261101/manifest.json")).unwrap()).unwrap();let mut accepted=0;
 for row in packet["cases"].as_array().unwrap().iter().filter(|r|r["native_version"]==14){
  let d:Document=serde_json::from_value(row["document"].clone()).unwrap();let name=row["name"].as_str().unwrap();
  let bad=name.starts_with("rect-fractional-")||name.starts_with("rect-equal-columns-")||name.starts_with("rect-near-orthogonal-");
  if bad{assert_eq!(cdr::encode_document(&d).unwrap_err().code,"CDR_NATIVE_PROFILE_QUARANTINED");continue;}
  let raw=cdr::encode_document(&d).unwrap().0;assert_eq!(raw,fs::read(root().join(row["file"].as_str().unwrap())).unwrap());let actual=cdr::decode(&raw,&d.id).unwrap().0;assert_eq!(actual,d);assert!(matches!(actual.object("content").unwrap().geometry,Geometry::Rect{radius_mm:0.}|Geometry::Ellipse));accepted+=1;
 }
 assert_eq!(accepted,48);
}
