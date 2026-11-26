//! Offline research packet, never invoked by normal Save/export.
use arranger_core::{Document,cdr};
use std::{env,fs,path::PathBuf};
fn main(){
    let args=env::args().collect::<Vec<_>>();assert!((3..=4).contains(&args.len()),"manifest fresh-output-directory [--current-codec]");let current=args.len()==4&&args[3]=="--current-codec";assert!(args.len()==3||current);
    let root=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest:serde_json::Value=serde_json::from_slice(&fs::read(&args[1]).unwrap()).unwrap();
    let output=PathBuf::from(&args[2]);fs::create_dir(&output).unwrap();let mut results=vec![];
    for case in manifest["cases"].as_array().unwrap(){
        let requested:Document=serde_json::from_value(case["requested_document"].clone()).unwrap();
        let carrier:Document=serde_json::from_value(case["document"].clone()).unwrap();
        let modern=case["native_version"].as_u64().unwrap()==21;
        let (bytes,notices)=cdr::encode_cyclic_start_research(&requested,&carrier,modern).unwrap_or_else(|e|panic!("{}: {e:?}",case["id"]));
        let actual=cdr::decode(&bytes,&requested.id).unwrap().0;
        let mut expected=requested.clone();expected.id=actual.id.clone();assert_eq!(actual,expected,"{}",case["id"]);
        let old=fs::read(root.join(case["file"].as_str().unwrap())).unwrap();
        let ordinary=if modern{cdr::encode_document21(&carrier)}else{cdr::encode_document(&carrier)}.unwrap().0;
        let unchanged=old==ordinary;if !current{assert!(unchanged,"ordinary encoder changed {}",case["id"]);}
        assert_eq!(cdr::decode(&ordinary,&carrier.id).unwrap().0,carrier,"current carrier roundtrip {}",case["id"]);
        let file=PathBuf::from(case["file"].as_str().unwrap()).file_name().unwrap().to_owned();fs::write(output.join(file),&bytes).unwrap();
        results.push(serde_json::json!({"id":case["id"],"source_restored":true,"ordinary_bytes_unchanged":unchanged,"current_carrier_roundtrip":true,"research_notes":notices.iter().any(|n|n.code=="CDR_CYCLIC_RESEARCH")}));
    }
    fs::write(output.join("probe-result.json"),serde_json::to_vec_pretty(&results).unwrap()).unwrap();
    println!("{} complete requested sources restored; historical byte equality recorded separately",results.len());
}
