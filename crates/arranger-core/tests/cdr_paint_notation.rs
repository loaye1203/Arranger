use arranger_core::{cdr,Document};
use serde_json::json;
#[test]
fn source_hex_spelling_and_explicit_opaque_alpha_survive_both_native_exchanges(){
    for(fill,stroke)in [("#AbCdEf","none"),("#123456FF","#aBcDeFFF"),("#aBcDeF80","none"),("cmyk(1,2,3,4)","#Ab12eF")]{
        let source:Document=serde_json::from_value(json!({"id":"spelling","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"shape","page_id":"page-1","x_mm":20.,"y_mm":30.,"width_mm":40.,"height_mm":20.,"geometry":{"kind":"rect","radius_mm":0.},"style":{"fill":fill,"stroke":stroke,"stroke_width_mm":0.25,"opacity":1.}}]})).unwrap();
        for encode in [cdr::encode_document,cdr::encode_document21]{let actual=cdr::decode(&encode(&source).unwrap().0,&source.id).unwrap().0;assert_eq!(actual,source);assert_eq!(cdr::decode(&encode(&actual).unwrap().0,&source.id).unwrap().0,source);}
    }
}
