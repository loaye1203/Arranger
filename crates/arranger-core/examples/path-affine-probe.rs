//! Research source witnesses only. Ordinary production encoding is unchanged.
use arranger_core::{Document,Geometry,cdr,native,text};
use kurbo::{Affine,BezPath,PathEl,Point};
use sha2::{Digest,Sha256};
use std::{env,fs,path::{Path,PathBuf}};
fn sha(p:&Path)->String{format!("{:x}",Sha256::digest(fs::read(p).unwrap()))}
fn paths(path:&BezPath)->serde_json::Value{
    let mut result=vec![];let mut segments=vec![];let mut start=Point::ZERO;let mut at=Point::ZERO;
    for el in path.elements(){match *el{
        PathEl::MoveTo(p)=>{assert!(segments.is_empty());start=p;at=p;},
        PathEl::LineTo(p)=>{segments.push(serde_json::json!({"kind":"line","points":[[at.x,at.y],[p.x,p.y]]}));at=p;},
        PathEl::QuadTo(a,p)=>{let b=at+(a-at)*(2./3.);let c=p+(a-p)*(2./3.);segments.push(serde_json::json!({"kind":"cubic","points":[[at.x,at.y],[b.x,b.y],[c.x,c.y],[p.x,p.y]]}));at=p;},
        PathEl::CurveTo(a,b,p)=>{segments.push(serde_json::json!({"kind":"cubic","points":[[at.x,at.y],[a.x,a.y],[b.x,b.y],[p.x,p.y]]}));at=p;},
        PathEl::ClosePath=>{if at!=start{segments.push(serde_json::json!({"kind":"line","points":[[at.x,at.y],[start.x,start.y]]}));}result.push(serde_json::json!({"start":[start.x,start.y],"closed":true,"segments":segments}));segments=vec![];at=start;},
    }}assert!(segments.is_empty()&&!result.is_empty());result.into()
}
fn main(){
    let args=env::args().collect::<Vec<_>>();assert_eq!(args.len(),3,"recipe fresh-output-directory");
    let mut packet:serde_json::Value=serde_json::from_slice(&fs::read(&args[1]).unwrap()).unwrap();assert_eq!(packet["profile"],"path-affine-frame-research");assert_eq!(packet["seeds"].as_array().unwrap().len(),4);assert_eq!(packet["variants"].as_array().unwrap().len(),10);
    let root=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();let out=PathBuf::from(&args[2]);fs::create_dir_all(out.parent().unwrap()).unwrap();fs::create_dir(&out).unwrap();let out=out.canonicalize().unwrap();assert!(out.starts_with(root.join("artifacts")));
    let relative=|p:&Path|p.strip_prefix(&root).unwrap().to_string_lossy().replace('\\',"/");let mut cases=vec![];
    for seed in packet["seeds"].as_array().unwrap(){
        let doc:Document=serde_json::from_value(seed["document"].clone()).unwrap();let modern=seed["native_version"]==21;let base=out.join(format!("{}-neutral.cdr",doc.id));let bytes=if modern{cdr::encode_document21(&doc)}else{cdr::encode_document(&doc)}.unwrap().0;fs::write(&base,bytes).unwrap();assert_eq!(cdr::decode(&fs::read(&base).unwrap(),&doc.id).unwrap().0,doc);
        let neutral=text::layout(&doc,doc.object("content").unwrap()).unwrap();assert!(neutral.outline_supported&&!neutral.missing_glyphs&&!neutral.overflow);
        for variant in packet["variants"].as_array().unwrap(){
            let id=format!("{}-{}",doc.id,variant["id"].as_str().unwrap());let mut requested=doc.clone();requested.id=id.clone();let m: [f64;6]=serde_json::from_value(variant["affine"].clone()).unwrap();let t=requested.objects.iter_mut().find(|o|o.id=="content").unwrap();t.transform=m;assert!(matches!(t.geometry,Geometry::Text{text_path:Some(_),..}));requested.validate().unwrap();let source=out.join(format!("{id}-source.arranger"));native::save(&requested,0,&source,None,false).unwrap();assert_eq!(native::open(&source).unwrap().document,requested);
            let run=text::layout(&requested,requested.object("content").unwrap()).unwrap();assert!(run.outline_supported&&!run.missing_glyphs&&!run.overflow);let source_world=requested.world_transform(requested.object("content").unwrap()).unwrap()*run.outline.clone();let predicted=Affine::new(m)*neutral.outline.clone();assert_eq!(paths(&source_world),paths(&predicted));
            cases.push(serde_json::json!({"id":id,"seed_id":doc.id,"native_version":seed["native_version"],"guide_family":seed["guide_family"],"variant":variant,"neutral_file":relative(&base),"neutral_sha256":sha(&base),"source_original":relative(&source),"source_sha256":sha(&source),"document":requested,"source_world_paths":paths(&source_world),"source_fonts":neutral.fonts}));
        }
    }
    packet["cases"]=cases.into();fs::write(out.join("manifest.json"),serde_json::to_vec_pretty(&packet).unwrap()).unwrap();println!("40 complete requested sources and4 neutral ordinary-codec carriers; no Corel run, no production affine promotion");
}
