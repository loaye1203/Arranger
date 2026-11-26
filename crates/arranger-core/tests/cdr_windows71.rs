use arranger_core::{cdr,Geometry};
use serde_json::Value;
use std::{fs,path::PathBuf};
fn root()->PathBuf {PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reports/windows/WIN-53-20261025-01")}

#[test]
fn all_eighteen_real_dpi_papers_match_the_actual_sdk_thirty_six_object_positions() {
    let folder=root().join("page-coordinate-controls");let receipt:Value=serde_json::from_slice(&fs::read(folder.join("result.json")).unwrap()).unwrap();let mut count=0;
    for case in receipt["cases"].as_array().unwrap() {
        let path=folder.join(case["file"].as_str().unwrap());let doc=cdr::decode(&fs::read(&path).unwrap(),"actual").unwrap_or_else(|e|panic!("{}: {e}",path.display())).0;
        for native in case["reopened"]["objects"].as_array().unwrap() {
            let object=doc.object(native["name"].as_str().unwrap()).unwrap();let actual=doc.bounds(object).unwrap();
            for (a,b) in actual.into_iter().zip(native["bounds_mm"].as_array().unwrap()){assert!((a-b.as_f64().unwrap()).abs()<=0.00021,"{} {actual:?} {}",path.display(),native["bounds_mm"]);}
            count+=1;
        }
    }assert_eq!(count,36);
}

#[test]
fn actual_cff_resaves_keep_uniform_live_text_in_both_native_versions() {
    for name in ["font-cff-declaration-corel-resaved.cdr","font-cff-declaration-v21-corel-resaved.cdr"] {
        let raw=fs::read(root().join("fonts/native").join(name)).unwrap();let doc=cdr::decode(&raw,"cff").unwrap().0;
        for id in ["area","artistic"] {
            let Geometry::Text{font_family,content,..}=&doc.object(id).unwrap().geometry else{panic!("CFF story lost")};
            assert_eq!(font_family,"Source Han Sans JP");assert!(content.contains("中文"));
        }
        for version in [14,21] {let raw=if version==14 {cdr::encode_document(&doc)}else{cdr::encode_document21(&doc)}.unwrap().0;let again=cdr::decode(&raw,"cff").unwrap().0;
            for id in ["area","artistic"] {assert_eq!(again.object(id).unwrap().geometry,doc.object(id).unwrap().geometry);}
        }
    }
}

#[test]
fn fresh_model_path_relations_keep_each_anchor_after_curve_quantization() {
    use arranger_core::{Document,TextAnchor};use serde_json::json;
    for anchor in ["start","middle","end"] {
        let mut doc=Document::new("fresh-path".into(),210.,297.).unwrap();
        doc.objects.push(serde_json::from_value(json!({"id":"guide","page_id":"page-1","x_mm":5.,"y_mm":8.,"width_mm":90.,"height_mm":40.,"transform":[0.9,0.2,-0.1,1.1,20.,30.],"geometry":{"kind":"path","d":"M0 0 Q40.123456 55.654321 90 0"},"style":{"fill":"none","stroke":"#0044cc","stroke_width_mm":0.2,"opacity":1.}})).unwrap());
        doc.objects.push(serde_json::from_value(json!({"id":"text","page_id":"page-1","x_mm":0.,"y_mm":0.,"width_mm":50.,"height_mm":10.,"geometry":{"kind":"text","content":"Live / 中文 42","font_family":"Arial","font_size_mm":5.,"text_path":{"path_id":"guide","start_offset_mm":35.125,"baseline_offset_mm":-1.25,"anchor":anchor}},"style":{"fill":"#123456","stroke":"none","stroke_width_mm":0.,"opacity":1.}})).unwrap());
        let before=doc.hash();
        for version in [14,21] {let raw=if version==14 {cdr::encode_document(&doc)}else{cdr::encode_document21(&doc)}.unwrap().0;let written=cdr::decode(&raw,"fresh").unwrap().0;
            let Geometry::Text{text_path:Some(binding),content,..}=&written.object("text").unwrap().geometry else{panic!("path flattened")};
            assert_eq!(content,"Live / 中文 42");assert_eq!(binding.anchor,match anchor {"start"=>TextAnchor::Start,"middle"=>TextAnchor::Middle,_=>TextAnchor::End});assert!((binding.start_offset_mm-35.125).abs()<0.0001);assert_eq!(binding.baseline_offset_mm,-1.25);
        }
        assert_eq!(doc.hash(),before);
        doc.objects[1].transform[0]=2.;
        assert_eq!(cdr::encode_document(&doc).unwrap_err().code,"CDR_UNSUPPORTED");
        assert_eq!(cdr::encode_document21(&doc).unwrap_err().code,"CDR_UNSUPPORTED");
    }
}
