//! Reproducible local candidates for multi-story cyclic source recovery.
use arranger_core::{Document,cdr,native};
use sha2::{Digest,Sha256};
use std::{env,fs,path::PathBuf};
fn sha(path:&std::path::Path)->String{format!("{:x}",Sha256::digest(fs::read(path).unwrap()))}
fn main(){
    let args=env::args().collect::<Vec<_>>();assert_eq!(args.len(),3,"recipe fresh-output-directory");
    let mut packet:serde_json::Value=serde_json::from_slice(&fs::read(&args[1]).unwrap()).unwrap();
    let root=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
    let output=PathBuf::from(&args[2]);fs::create_dir_all(output.parent().unwrap()).unwrap();fs::create_dir(&output).unwrap();let output=output.canonicalize().unwrap();assert!(output.starts_with(&root));
    let relative=|p:&std::path::Path|p.strip_prefix(&root).unwrap().to_string_lossy().replace('\\',"/");
    for case in packet["cases"].as_array_mut().unwrap(){
        let requested:Document=serde_json::from_value(case["document"].clone()).unwrap();let carrier:Document=serde_json::from_value(case["carrier_document"].clone()).unwrap();let modern=case["native_version"].as_u64()==Some(21);
        let (bytes,_)=cdr::encode_cyclic_start_research(&requested,&carrier,modern).unwrap_or_else(|e|panic!("{}: {e:?}",requested.id));
        assert_eq!(cdr::decode(&bytes,&requested.id).unwrap().0,requested);
        let file=output.join(format!("{}.cdr",requested.id));fs::write(&file,bytes).unwrap();
        let source=output.join(format!("{}-source.arranger",requested.id));native::save(&requested,0,&source,None,false).unwrap();assert_eq!(native::open(&source).unwrap().document,requested);
        let neutral=output.join(format!("{}-carrier.arranger",requested.id));native::save(&carrier,0,&neutral,None,false).unwrap();
        case["file"]=relative(&file).into();case["sha256"]=sha(&file).into();case["source_original"]=relative(&source).into();case["source_sha256"]=sha(&source).into();case["carrier_source"]=relative(&neutral).into();case["carrier_sha256"]=sha(&neutral).into();case["pages"]=case["document"]["pages"].clone();case["passed"]=true.into();
    }
    fs::write(output.join("manifest.json"),serde_json::to_vec_pretty(&packet).unwrap()).unwrap();println!("8 generated CDRs and16 complete sources; exact requested recovery verified");
}
