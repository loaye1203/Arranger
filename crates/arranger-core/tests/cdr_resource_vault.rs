use arranger_core::{cdr,Document,Resource};
use serde_json::json;

fn image(id:&str,resource:&str,page:&str,x:f64)->arranger_core::Object {
    serde_json::from_value(json!({"id":id,"page_id":page,"x_mm":x,"y_mm":30.,"width_mm":40.,"height_mm":20.,"style":{"fill":"none","stroke":"none","stroke_width_mm":0.,"opacity":1.},"geometry":{"kind":"image","resource_id":resource,"fit":"stretch"}})).unwrap()
}

#[test]
fn unused_original_image_bytes_survive_empty14_and21_documents() {
    let mut source=Document::new("empty-resources".into(),210.,297.).unwrap();
    source.resources.insert("unused".into(),Resource::png(include_bytes!("../../../examples/assets/images/quadrants.png")).unwrap());
    for version in [14,21] {
        let raw=if version==14{cdr::encode_document(&source)}else{cdr::encode_document21(&source)}.unwrap().0;
        let restored=cdr::decode(&raw,&source.id).unwrap().0;
        assert!(restored.objects.is_empty());assert_eq!(restored.resources,source.resources);
    }
}

#[test]
fn original_resource_map_and_desktop_sharing_do_not_depend_on_object_data_owners() {
    let mut source=Document::new("shared-resources".into(),210.,297.).unwrap();
    let raw=include_bytes!("../../../examples/assets/images/quadrants.png");
    source.resources.insert("shared".into(),Resource::png(raw).unwrap());source.resources.insert("unused".into(),Resource::png(raw).unwrap());
    source.objects=vec![image("page-image","shared","page-1",20.),image("desktop-image","shared","_workspace",-55.)];
    for version in [14,21] {
        let encoded=if version==14{cdr::encode_document(&source)}else{cdr::encode_document21(&source)}.unwrap().0;
        let restored=cdr::decode(&encoded,&source.id).unwrap().0;assert_eq!(restored.resources,source.resources);
        for id in ["page-image","desktop-image"] {assert_eq!(restored.object(id).unwrap().geometry,source.object(id).unwrap().geometry);}
    }
}
