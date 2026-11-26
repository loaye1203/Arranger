use arranger_core::{cdr,Document};
use serde_json::json;
fn source(alpha:f64,outline:bool,kind:&str)->Document{
    let geometry=match kind{"ellipse"=>json!({"kind":"ellipse"}),"path"=>json!({"kind":"path","d":"M0 0L60 0L60 40L0 40Z"}),_=>json!({"kind":"rect","radius_mm":0.})};
    let(fill,stroke)=if outline{("rgba(30,90,180,1)".to_string(),format!("rgba(220,80,40,{alpha})"))}else{(format!("rgba(30,90,180,{alpha})"),"rgba(220,80,40,1)".to_string())};
    serde_json::from_value(json!({"id":"floating-paint","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"paint","page_id":"page-1","x_mm":40.,"y_mm":60.,"width_mm":60.,"height_mm":40.,"geometry":geometry,"style":{"fill":fill,"stroke":stroke,"stroke_width_mm":12.,"opacity":1.}}]})).unwrap()
}
#[test]
fn continuous_alpha_source_fields_survive_two_exchanges_for_each_native_primitive(){
    for kind in ["rect","ellipse","path"]{for alpha in [0.,1.,0.5,0.1,0.12345678901234568,128./255.,f64::EPSILON,f64::MIN_POSITIVE]{for outline in [false,true]{let source=source(alpha,outline,kind);source.validate().unwrap();for encode in [cdr::encode_document,cdr::encode_document21]{let raw=encode(&source).unwrap().0;let copy=cdr::decode(&raw,&source.id).unwrap().0;assert_eq!(copy,source,"{kind}/{alpha}/{outline}");assert_eq!(cdr::decode(&encode(&copy).unwrap().0,&source.id).unwrap().0,source);}}}}
}
#[test]
fn floating_paints_with_opacity_and_dual_paints_use_existing_editable_layers(){
    for encode in [cdr::encode_document,cdr::encode_document21]{let mut source=source(0.5,false,"path");source.objects[0].style.stroke="rgba(220,80,40,0.25)".into();assert_eq!(cdr::decode(&encode(&source).unwrap().0,&source.id).unwrap().0,source);}
    let mut source=source(0.5,false,"rect");source.objects[0].style.opacity=0.7;assert!(cdr::encode_document(&source).is_err());assert_eq!(cdr::decode(&cdr::encode_document21(&source).unwrap().0,&source.id).unwrap().0,source);
}
#[test]
fn ordinary_text_uses_one_story_and_preserves_scoped_byte_or_continuous_paints(){
    for mode in ["point","area"]{for outline in [false,true]{for alpha in [0.5,128./255.]{let mut source=source(alpha,outline,"rect");source.objects[0].geometry=serde_json::from_value(json!({"kind":"text","content":"One editable\nstory","font_family":"Arial","font_size_mm":8.,"layout":{"mode":mode,"line_height_mm":10.}})).unwrap();source.objects[0].style.stroke_width_mm=0.1234567;
        for encode in [cdr::encode_document,cdr::encode_document21]{let copy=cdr::decode(&encode(&source).unwrap().0,&source.id).unwrap().0;assert_eq!(copy,source);assert_eq!(copy.objects.len(),1);}
    }}}
}
#[test]
fn svg_and_native_document_retain_exact_non_byte_alpha(){
    let source=source(0.5,false,"rect");let svg=arranger_core::render::svg(&source,"page-1").unwrap();assert!(svg.contains("fill-opacity=\"0.5\""));assert!(!svg.contains("0.501960"));assert!(svg.contains("stroke-opacity=\"1\""));
    let serialized=serde_json::to_vec(&source).unwrap();assert_eq!(serde_json::from_slice::<Document>(&serialized).unwrap(),source);
}
