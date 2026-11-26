use arranger_core::{cdr,Document,Geometry};
use serde_json::json;

fn scene(group_opacity:f64)->Document {
    serde_json::from_value(json!({"id":"isolated","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"objects":[
        {"id":"alpha-group","page_id":"page-1","x_mm":0.,"y_mm":0.,"width_mm":0.,"height_mm":0.,"style":{"fill":"none","stroke":"none","stroke_width_mm":0.,"opacity":group_opacity},"geometry":{"kind":"group"}},
        {"id":"alpha-a","page_id":"page-1","parent_id":"alpha-group","x_mm":20.,"y_mm":57.,"width_mm":40.,"height_mm":30.,"style":{"fill":"#1e5ab4","stroke":"none","stroke_width_mm":0.,"opacity":1.},"geometry":{"kind":"rect"}},
        {"id":"alpha-b","page_id":"page-1","parent_id":"alpha-group","x_mm":40.,"y_mm":67.,"width_mm":40.,"height_mm":30.,"style":{"fill":"#dc5028","stroke":"none","stroke_width_mm":0.,"opacity":1.},"geometry":{"kind":"rect"}}
    ],"resources":{}})).unwrap()
}

#[test]
fn actual_corel21_isolated_group_retains_parent_alpha_and_opaque_children() {
    let raw=include_bytes!("../../../examples/assets/cdr/corel2019/group-isolated-opacity-v21.cdr");
    let d=cdr::decode(raw,"native").unwrap().0;
    assert_eq!(d.objects.len(),3);
    assert_eq!(d.object("alpha-group").unwrap().geometry,Geometry::Group);
    assert_eq!(d.object("alpha-group").unwrap().style.opacity,0.7);
    for id in ["alpha-a","alpha-b"] {assert_eq!(d.object(id).unwrap().style.opacity,1.);assert_eq!(d.object(id).unwrap().parent_id.as_deref(),Some("alpha-group"));}
}

#[test]
fn independent21_writer_keeps_isolation_and_exact_group_and_child_alpha_parameters() {
    for opacity in [0.7,0.123456789] {
        let mut d=scene(opacity);d.object("alpha-group").unwrap();
        d.objects[1].style.fill="#1e5ab480".into();
        let before=d.hash();assert!(cdr::encode_document(&d).is_err());
        let bytes=cdr::encode_document21(&d).unwrap().0;let out=cdr::decode(&bytes,"imported").unwrap().0;
        assert_eq!(d.hash(),before);assert_eq!(out.objects.len(),3);
        assert_eq!(out.object("alpha-group").unwrap().style,d.object("alpha-group").unwrap().style);
        for id in ["alpha-a","alpha-b"] {assert_eq!(out.object(id).unwrap().style,d.object(id).unwrap().style);assert_eq!(out.object(id).unwrap().parent_id.as_deref(),Some("alpha-group"));}
    }
}

#[test]
fn native_group_keeps_its_nonpainting_source_style_in_local_scalar_fields() {
    let mut doc=scene(1.);let group=&mut doc.objects[0];
    group.style.fill="cmyk(10,20,30,40)".into();group.style.stroke="#123456".into();group.style.stroke_width_mm=0.375;
    group.style.stroke_linecap=arranger_core::LineCap::Round;group.style.stroke_linejoin=arranger_core::LineJoin::Bevel;group.style.stroke_miterlimit=7.25;
    for version in [14,21]{let bytes=if version==14{cdr::encode_document(&doc)}else{cdr::encode_document21(&doc)}.unwrap().0;let actual=cdr::decode(&bytes,"groups").unwrap().0;assert_eq!(actual.objects[0].style,doc.objects[0].style);}
}
