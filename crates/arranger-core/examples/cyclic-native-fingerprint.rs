//! Read-only independent reconstruction of the stored cyclic model digests.
//! Inputs are ordinary typed snapshots; this never restores an Own matrix.
use arranger_core::Document;
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{collections::{BTreeMap,BTreeSet},env,fs};
fn payload(doc:&Document,text:&str,guide:&str,scoped:bool)->Value{
    doc.validate().unwrap();let mut selected=BTreeSet::new();
    for id in[text,guide]{let mut current=Some(id);let mut depth=0;while let Some(id)=current{depth+=1;assert!(depth<=17);selected.insert(id);current=doc.object(id).unwrap().parent_id.as_deref();}}
    let objects=selected.iter().map(|id|(*id,doc.object(id).unwrap())).collect::<BTreeMap<_,_>>();
    let users=doc.objects.iter().filter_map(|o|match &o.geometry{arranger_core::Geometry::Text{text_path:Some(p),..}if p.path_id==guide=>Some(o.id.as_str()),_=>None}).collect::<BTreeSet<_>>();
    let pages=objects.values().map(|o|o.page_id.as_str()).collect::<BTreeSet<_>>();let relevant=doc.pages.iter().filter(|p|!scoped||pages.contains(p.id.as_str())).collect::<Vec<_>>();
    // Serialize typed fields before creating a Value: a Value map has a
    // different key order from the native Object/Page struct serializer.
    let bytes=serde_json::to_vec(&(&doc.unit,relevant,objects,users)).unwrap();let sha=format!("{:x}",Sha256::digest(&bytes));
    json!({"sha256":sha,"selected":serde_json::from_slice::<Value>(&bytes).unwrap()})
}
fn main(){
    let args=env::args().collect::<Vec<_>>();assert_eq!(args.len(),3,"snapshot-manifest fresh-output-json");assert!(!std::path::Path::new(&args[2]).exists());
    let packet:Value=serde_json::from_slice(&fs::read(&args[1]).unwrap()).unwrap();let mut rows=vec![];
    for task in packet["cases"].as_array().unwrap(){let mut documents=serde_json::Map::new();for(name,value)in task["documents"].as_object().unwrap(){let doc:Document=serde_json::from_value(value.clone()).unwrap();documents.insert(name.clone(),payload(&doc,task["text_id"].as_str().unwrap(),task["guide_id"].as_str().unwrap(),task["version"]==2));}
        rows.push(json!({"id":task["id"],"text_id":task["text_id"],"version":task["version"],"expected_native_sha256":task["native_sha256"],"expected_carrier_sha256":task["carrier_sha256"],"documents":documents}));
    }
    fs::write(&args[2],serde_json::to_vec_pretty(&json!({"cases":rows,"new_corel_operations":0,"restoration_attempted":false,"full_cdr_save_complete":false})).unwrap()).unwrap();
}
