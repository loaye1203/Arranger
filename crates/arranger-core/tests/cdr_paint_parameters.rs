use arranger_core::{Document,Geometry,Style,cdr,render::Renderer};
use serde_json::json;
fn native(doc:&Document,version:u32)->Vec<u8>{if version==14{cdr::encode_document(doc)}else{cdr::encode_document21(doc)}.unwrap().0}
#[test]
fn every_eight_bit_alpha_and_precise_opacity_keep_separate_live_parameters() {
    let mut doc=Document::new("all-alpha".into(),210.,297.).unwrap();
    for alpha in 0..=255 {for stroke in [false,true] {
        let paint=format!("#287ac7{alpha:02x}");let style=if stroke{Style{fill:"none".into(),stroke:paint,stroke_width_mm:1.2,opacity:0.123456789012345,..Default::default()}}else{Style{fill:paint,stroke:"none".into(),stroke_width_mm:0.,opacity:0.7,..Default::default()}};
        doc.objects.push(serde_json::from_value(json!({"id":format!("alpha-{alpha}-{stroke}"),"page_id":"page-1","x_mm":20.,"y_mm":30.,"width_mm":40.,"height_mm":20.,"style":style,"geometry":{"kind":"ellipse"}})).unwrap());
    }}
    for version in [14,21] {
        let (copy,warnings)=cdr::decode(&native(&doc,version),&doc.id).unwrap();assert!(warnings.iter().any(|w|w.code=="CDR_PAINT_PARAMETERS"));
        for source in &doc.objects {let o=copy.object(&source.id).unwrap();assert_eq!(o.style.opacity,source.style.opacity);assert_eq!(o.style.fill,source.style.fill);assert_eq!(o.style.stroke,source.style.stroke);assert!(matches!(o.geometry,Geometry::Ellipse));}
        assert_eq!(cdr::decode(&native(&copy,version),&doc.id).unwrap().0,copy);
    }
}
#[test]
fn separate_paint_alpha_and_object_opacity_keep_exact_render_on_both_roundtrips() {
    let mut doc=Document::new("paint-render".into(),210.,297.).unwrap();
    for (i,(fill,opacity)) in [("#287ac7aa",1.),("#b4603280",0.12345),("#00663301",0.7),("#123456fe",0.3333333333333333)].into_iter().enumerate(){doc.objects.push(serde_json::from_value(json!({"id":format!("shape-{i}"),"page_id":"page-1","x_mm":20.+i as f64*25.,"y_mm":30.+i as f64*15.,"width_mm":60.,"height_mm":40.,"style":{"fill":fill,"stroke":"none","stroke_width_mm":0.,"opacity":opacity},"geometry":{"kind":"rect","radius_mm":0.}})).unwrap());}
    let renderer=Renderer::new();let source=renderer.render(&doc,"page-1",254.,None).unwrap().pixels;
    for version in [14,21] {let copy=cdr::decode(&native(&doc,version),&doc.id).unwrap().0;assert_eq!(renderer.render(&copy,"page-1",254.,None).unwrap().pixels.data(),source.data());}
    doc.objects[0].style.stroke="#00000080".into();let copy=cdr::decode(&cdr::encode_document(&doc).unwrap().0,&doc.id).unwrap().0;assert_eq!(copy,doc);
}

#[test]
fn arbitrary_model_opacity_keeps_bits_including_subnormals_and_signed_zero() {
    let mut doc=Document::new("precise".into(),210.,297.).unwrap();
    for (i,value) in [0.12345,1./3.,1e-300,f64::from_bits(1),-0.0,f64::from_bits(1f64.to_bits()-1)].into_iter().enumerate(){doc.objects.push(serde_json::from_value(json!({"id":format!("opacity-{i}"),"page_id":"page-1","x_mm":20.,"y_mm":30.,"width_mm":40.,"height_mm":20.,"style":{"fill":"#287ac7","stroke":"none","stroke_width_mm":0.,"opacity":value},"geometry":{"kind":"rect","radius_mm":0.}})).unwrap());}
    for version in [14,21]{let copy=cdr::decode(&native(&doc,version),&doc.id).unwrap().0;for source in &doc.objects{assert_eq!(copy.object(&source.id).unwrap().style.opacity.to_bits(),source.style.opacity.to_bits());}}
}
