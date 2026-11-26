use arranger_core::{cdr,Document};
use serde_json::json;
fn source(alpha:u8,outline:bool,kind:&str)->Document{
    let geometry=match kind{"ellipse"=>json!({"kind":"ellipse"}),"path"=>json!({"kind":"path","d":"M0 0L60 0L60 40L0 40Z"}),_=>json!({"kind":"rect","radius_mm":0.})};
    let(fill,stroke)=if outline{("#1E5ab4FF".to_string(),format!("#Dc5028{alpha:02X}"))}else{(format!("#1E5ab4{alpha:02X}"),"#Dc5028FF".to_string())};
    serde_json::from_value(json!({"id":"scoped","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"paint","page_id":"page-1","x_mm":40.,"y_mm":60.,"width_mm":60.,"height_mm":40.,"geometry":geometry,"style":{"fill":fill,"stroke":stroke,"stroke_width_mm":12.,"opacity":1.}}]})).unwrap()
}
#[test]
fn one_paint_alpha_preserves_the_other_opaque_paint_and_source_spelling(){
    for kind in ["rect","ellipse","path"]{for alpha in [0,1,64,128,192,254]{for outline in [false,true]{let doc=source(alpha,outline,kind);for encode in [cdr::encode_document,cdr::encode_document21]{let raw=encode(&doc).unwrap().0;let copy=cdr::decode(&raw,&doc.id).unwrap().0;assert_eq!(copy,doc,"{kind}/{alpha}/{outline}");assert_eq!(cdr::decode(&encode(&copy).unwrap().0,&doc.id).unwrap().0,doc);}}}}
}
#[test]
fn legacy14_combined_alpha_with_global_opacity_stays_rejected(){
    for outline in [false,true]{let mut doc=source(128,outline,"rect");doc.objects[0].style.opacity=0.5;assert!(cdr::encode_document(&doc).is_err());assert!(cdr::encode_document21(&doc).is_ok());}
}
