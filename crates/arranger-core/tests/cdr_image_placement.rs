use arranger_core::{cdr,Document,Geometry,ImageFit,Resource};
use serde_json::json;

#[test]
fn image_fit_frame_and_legacy_crop_survive_native14_and21_without_a_generated_parent() {
    for fit in [ImageFit::Contain,ImageFit::Cover,ImageFit::Stretch] {for compound in [false,true] {
        let mut doc:Document=serde_json::from_value(json!({"id":"image-placement","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"image","page_id":"page-1","x_mm":25.25,"y_mm":40.5,"width_mm":45.,"height_mm":40.,"geometry":{"kind":"image","resource_id":"original","fit":fit,"clip_mm":[1.,2.,40.,35.]},"style":{"fill":"none","stroke":"none","stroke_width_mm":0.,"opacity":1.}}]})).unwrap();
        doc.resources.insert("original".into(),Resource::png(include_bytes!("../../../examples/assets/images/quadrants.png")).unwrap());
        if compound {doc.objects[0].clip=Some(serde_json::from_value(json!({"kind":"path","d":"M0 0H45V40H0Z M12 8H32V32H12Z","fill_rule":"evenodd"})).unwrap());}
        for version in [14,21] {
            let bytes=if version==14{cdr::encode_document(&doc)}else{cdr::encode_document21(&doc)}.unwrap().0;let actual=cdr::decode(&bytes,"copy").unwrap().0;
            assert_eq!(actual.objects.len(),1,"generated fit wrapper leaked: {fit:?} compound={compound} v{version}");let image=actual.object("image").unwrap();
            assert_eq!(image.geometry,doc.objects[0].geometry);assert_eq!([image.x_mm,image.y_mm,image.width_mm,image.height_mm],[25.25,40.5,45.,40.]);assert_eq!(image.transform,doc.objects[0].transform);assert_eq!(image.parent_id,None);
            assert!(matches!(image.geometry,Geometry::Image{..}));
        }
    }}
}
