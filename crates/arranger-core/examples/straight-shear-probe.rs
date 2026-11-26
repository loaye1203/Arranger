//! Controlled native-frame candidates and ordinary live-text references.
use arranger_core::{Document,cdr,native,text};
use kurbo::{BezPath,PathEl};
use sha2::{Digest,Sha256};
use std::{env,fs,path::{Path,PathBuf}};
fn sha(path:&Path)->String{format!("{:x}",Sha256::digest(fs::read(path).unwrap()))}
fn controls(path:BezPath)->Vec<[f64;2]>{
    let mut result=vec![];for el in path.elements(){match el{
        PathEl::MoveTo(p)|PathEl::LineTo(p)=>result.push([p.x,p.y]),
        PathEl::QuadTo(a,p)=>result.extend([[a.x,a.y],[p.x,p.y]]),
        PathEl::CurveTo(a,b,p)=>result.extend([[a.x,a.y],[b.x,b.y],[p.x,p.y]]),
        PathEl::ClosePath=>{},
    }}result
}
fn main(){
    let args=env::args().collect::<Vec<_>>();assert_eq!(args.len(),3,"recipe fresh-output-directory");
    let mut packet:serde_json::Value=serde_json::from_slice(&fs::read(&args[1]).unwrap()).unwrap();
    assert_eq!(packet["profile"],"straight-shear-frame");assert_eq!(packet["cases"].as_array().unwrap().len(),12);
    let root=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
    let out=PathBuf::from(&args[2]);fs::create_dir_all(out.parent().unwrap()).unwrap();fs::create_dir(&out).unwrap();let out=out.canonicalize().unwrap();assert!(out.starts_with(&root));
    let relative=|p:&Path|p.strip_prefix(&root).unwrap().to_string_lossy().replace('\\',"/");
    let mut pairs=std::collections::BTreeMap::<String,Vec<Vec<[f64;2]>>>::new();
    for case in packet["cases"].as_array_mut().unwrap(){
        let doc:Document=serde_json::from_value(case["document"].clone()).unwrap();let modern=case["native_version"]==21;
        let text=doc.object("content").unwrap();let run=text::layout(&doc,text).unwrap();assert!(run.outline_supported&&!run.missing_glyphs&&!run.overflow);
        let outline=doc.world_transform(text).unwrap()*run.outline.clone();pairs.entry(case["pair"].as_str().unwrap().into()).or_default().push(controls(outline));
        let bytes=if case["variant"]=="fitted"{cdr::encode_straight_shear_research(&doc,"content",modern)}else if modern{cdr::encode_document21(&doc)}else{cdr::encode_document(&doc)}.unwrap().0;
        if case["variant"]=="fitted"&&case["shear"].as_f64()!=Some(0.){assert!(cdr::decode(&bytes,"guard").is_err());}
        let file=out.join(format!("{}.cdr",doc.id));fs::write(&file,bytes).unwrap();let source=out.join(format!("{}-source.arranger",doc.id));native::save(&doc,0,&source,None,false).unwrap();assert_eq!(native::open(&source).unwrap().document,doc);
        case["file"]=relative(&file).into();case["sha256"]=sha(&file).into();case["source_original"]=relative(&source).into();case["source_sha256"]=sha(&source).into();case["font_validation"]=true.into();
    }
    let mut proofs=vec![];for (name,pair)in pairs{assert_eq!(pair.len(),2);assert!(!pair[0].is_empty());assert_eq!(pair[0].len(),pair[1].len());let max=pair[0].iter().zip(&pair[1]).flat_map(|(a,b)|a.iter().zip(b).map(|(x,y)|(x-y).abs())).fold(0.,f64::max);assert!(max<=0.00005,"source equivalence {name}: {max}");proofs.push(serde_json::json!({"pair":name,"controls":pair[0].len(),"maximum_source_control_difference_mm":max,"passed":true}));}
    packet["source_equivalence"]=proofs.into();fs::write(out.join("manifest.json"),serde_json::to_vec_pretty(&packet).unwrap()).unwrap();println!("12 genuine native candidates/references,12 complete sources,6 baseline-fixed source equivalence proofs; Corel unverified");
}
