use arranger_core::{cdr,Geometry};
#[test]
fn genuine_sdk_relative075_radius_and_paints_survive_two_native_exchanges(){
 let raw=include_bytes!("../../../examples/assets/cdr/corel2019/corners-20261030/sdk-rounded075-v21.cdr");let doc=cdr::decode(raw,"relative-control").unwrap().0;assert_eq!(doc.objects.len(),1);let o=&doc.objects[0];assert_eq!(o.width_mm,40.);assert_eq!(o.height_mm,10.);assert_eq!(o.geometry,Geometry::Rect{radius_mm:0.75});
 for encode in [cdr::encode_document,cdr::encode_document21]{let bytes=encode(&doc).unwrap().0;let copy=cdr::decode(&bytes,&doc.id).unwrap().0;assert_eq!(copy,doc);assert_eq!(cdr::decode(&encode(&copy).unwrap().0,&doc.id).unwrap().0,doc);}
}
