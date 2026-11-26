use arranger_core::{Document,Geometry};
use kurbo::{Affine,PathEl,Point};
use serde_json::json;

fn source(scale:f64,position:[f64;2],family:&str)->Document{
    serde_json::from_value(json!({"id":"chart-precision","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[
        {"id":"parent","page_id":"page-1","x_mm":3.,"y_mm":4.,"width_mm":0.,"height_mm":0.,"transform":[0.,scale,-scale,0.,0.,0.],"geometry":{"kind":"group"}},
        {"id":"guide","page_id":"page-1","x_mm":position[0],"y_mm":position[1],"width_mm":110.,"height_mm":20.,"geometry":{"kind":"path","d":"M0 0 C30 10 70 -10 100 0"}},
        {"id":"text","page_id":"page-1","parent_id":"parent","x_mm":0.,"y_mm":0.,"width_mm":80.,"height_mm":25.,"geometry":{"kind":"text","content":"Edited office 42","font_family":family,"font_size_mm":12./scale,"text_path":{"path_id":"guide","start_offset_mm":8./scale,"baseline_offset_mm":0.25/scale,"anchor":"start"}}}
    ]})).unwrap()
}
fn controls(path:&kurbo::BezPath,m:Affine)->Vec<Point>{path.elements().iter().flat_map(|el|match *el{PathEl::MoveTo(p)|PathEl::LineTo(p)=>vec![m*p],PathEl::QuadTo(c,p)=>vec![m*c,m*p],PathEl::CurveTo(a,b,p)=>vec![m*a,m*b,m*p],PathEl::ClosePath=>vec![]}).collect()}

#[test]
fn equivalent_live_path_charts_keep_world_glyphs_without_a_larger_tolerance(){
    let mut tested=0;
    for family in ["Arial","PingFang SC"]{
        if arranger_core::text::font_database().query(&resvg::usvg::fontdb::Query{families:&[resvg::usvg::fontdb::Family::Name(family)],..Default::default()}).is_none(){continue;}
        tested+=1;
        for position in [[10.,20.],[12345.6789,8732.3456]]{
            let mut reference=source(1.,position,family);reference.objects[2].parent_id=None;
            let text=reference.object("text").unwrap();let expected=controls(&reference.object_path(text).unwrap(),reference.world_transform(text).unwrap());assert!(!expected.is_empty());
            for scale in [0.125,0.3,0.7,2.,3.,8.]{
                let doc=source(scale,position,family);let text=doc.object("text").unwrap();let actual=controls(&doc.object_path(text).unwrap(),doc.world_transform(text).unwrap());assert_eq!(actual.len(),expected.len());
                let delta=actual.iter().zip(&expected).map(|(a,b)|(a.x-b.x).abs().max((a.y-b.y).abs())).fold(0.,f64::max);
                assert!(delta<=0.00021,"{family} scale={scale} position={position:?} actual world glyph deviation={delta}mm");
                let Geometry::Text{text_path:Some(binding),..}=&text.geometry else{panic!()};assert_eq!(binding.path_id,"guide");
            }
        }
    }
    assert!(tested>0,"The path chart regression requires an installed Arial or PingFang SC face");
}
