use arranger_core::{cdr,Document,Geometry};
use serde_json::json;

fn source(kind:&str,pen:f64)->Document{
    serde_json::from_value(json!({"id":"primitive-units","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"shape","page_id":"page-1","x_mm":0.,"y_mm":0.,"width_mm":12.1234567,"height_mm":8.2345678,"transform":[2.,0.25,0.125,1.5,1.25,2.5],"geometry":if kind=="rect"{json!({"kind":"rect","radius_mm":0.1234567})}else{json!({"kind":"ellipse"})},"style":{"fill":"#123456","stroke":if pen==0.{"none"}else{"#000000"},"stroke_width_mm":pen,"opacity":1.}}]})).unwrap()
}
fn exact(doc:&Document){
    doc.validate().unwrap();
    for version in [14,21]{
        let encode=if version==14{cdr::encode_document}else{cdr::encode_document21};
        let bytes=encode(doc).unwrap().0;let actual=cdr::decode(&bytes,&doc.id).unwrap().0;
        assert_eq!(actual,*doc,"native{version}");
        assert_eq!(cdr::decode(&encode(&actual).unwrap().0,&doc.id).unwrap().0,*doc,"native{version} second exchange");
    }
}
#[test]
fn rounded_radius_dimensions_and_fine_pen_share_the_scaled_native_chart(){
    for kind in ["rect","ellipse"]{for pen in [0.,0.00004,0.1234567]{for scale in [1.,100.]{let mut doc=source(kind,pen);doc.objects[0].transform[0]*=scale;doc.objects[0].transform[3]*=scale;exact(&doc);}}}
}
#[test]
fn reflected_rounded_frames_and_own_ancestor_clips_keep_source_and_native_geometry(){
    for own in [false,true]{for parent in [false,true]{
        let mut doc=source("rect",0.1234567);doc.objects[0].transform=[-1.25,0.25,0.125,0.875,50.,60.];
        if own{doc.objects[0].clip=Some(arranger_core::Clip::Rect{rect_mm:[0.1234567,0.7654321,10.2345678,6.3456789]});}
        if parent{let mut group=doc.objects[0].clone();group.id="parent".into();group.geometry=Geometry::Group;group.width_mm=0.;group.height_mm=0.;group.transform=[0.9,0.2,-0.125,1.1,10.,15.];group.clip=Some(arranger_core::Clip::Rect{rect_mm:[-10.,-10.,200.,200.]});group.style=Default::default();doc.objects[0].parent_id=Some("parent".into());doc.objects.push(group);}
        exact(&doc);
    }}
}
#[test]
fn positive_primitive_sizes_below_the_ordinary_native_grid_remain_positive(){
    for kind in ["rect","ellipse"]{let mut doc=source(kind,0.00004);doc.objects[0].width_mm=0.00004;doc.objects[0].height_mm=0.00006;if kind=="rect"{doc.objects[0].geometry=Geometry::Rect{radius_mm:0.00001};}exact(&doc);}
}
