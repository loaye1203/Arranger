use arranger_core::{cdr,Document,Geometry};
use serde_json::json;
const ROOT:&str=concat!(env!("CARGO_MANIFEST_DIR"),"/../../examples/assets/cdr/corel2019/save100-controls");
#[test]
fn actual_resized_rectangles_keep_native_bounds_and_the_effective_binary_pen(){
    for (file,w,h) in [("native-x-resized21.cdr",42.,11.),("native-y-resized21.cdr",22.,21.),("product-x-resized21.cdr",42.,11.),("product-y-resized21.cdr",22.,21.)]{
        let doc=cdr::decode(&std::fs::read(format!("{ROOT}/{file}")).unwrap(),"native").unwrap().0;let o=doc.object("content").unwrap();assert!(matches!(o.geometry,Geometry::Rect{radius_mm:0.}));let bounds=doc.bounds(o).unwrap();assert!((bounds[2]-w).abs()<0.00021&&(bounds[3]-h).abs()<0.00021);
        let m=doc.world_transform(o).unwrap().as_coeffs();let wx=m[0].hypot(m[2])*o.style.stroke_width_mm;let wy=m[1].hypot(m[3])*o.style.stroke_width_mm;let expected=if file.contains("x-"){[2.*1.0499999523162842,1.100000023841858]}else{[1.100000023841858,2.*1.0499999523162842]};assert_eq!([wx,wy],expected);
        for encode in [cdr::encode_document,cdr::encode_document21]{assert_eq!(cdr::decode(&encode(&doc).unwrap().0,"native").unwrap().0,doc);}
    }
}
#[test]
fn actual_whole_object_transparency_retains_both_live_paints(){
    for version in [14,21]{let doc=cdr::decode(&std::fs::read(format!("{ROOT}/combined-opacity{version}.cdr")).unwrap(),"native").unwrap().0;let o=doc.objects.iter().find(|o|o.style.fill!="none"&&o.style.stroke!="none").unwrap();assert_eq!(o.style.opacity,0.5);for encode in [cdr::encode_document,cdr::encode_document21]{assert_eq!(cdr::decode(&encode(&doc).unwrap().0,"native").unwrap().0,doc);}}
}
#[test]
fn combined_paints_and_whole_opacity_round_trip_without_decomposing_into_children(){
    for opacity in [0.,0.5,0.37654321]{let doc:Document=serde_json::from_value(json!({"id":"whole","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"paint-target","page_id":"page-1","x_mm":40.,"y_mm":60.,"width_mm":60.,"height_mm":40.,"geometry":{"kind":"rect","radius_mm":0.},"style":{"fill":"#1E5ab4","stroke":"#dc5028","stroke_width_mm":12.,"opacity":opacity}}]})).unwrap();for encode in [cdr::encode_document,cdr::encode_document21]{let copy=cdr::decode(&encode(&doc).unwrap().0,"whole").unwrap().0;assert_eq!(copy,doc);assert_eq!(copy.objects.len(),1);}}
}
