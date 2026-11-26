use arranger_core::{cdr,Document};
use serde_json::json;

#[test]
fn native_coordinates_reconstruct_original_path_spelling_and_rectangular_clip_scalars() {
    for d in ["M0 20 C12 -15 42 35 70 0","M0 0H35V35H0Z M10 10H25V25H10Z","M 0 0 C 1 20 30 -10 40 20 L 10 30 Z M 50 0 Q 70 20 90 0"] {
        let doc:Document=serde_json::from_value(json!({"id":"geometry-scalars","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"path","page_id":"page-1","x_mm":25.,"y_mm":35.,"width_mm":100.,"height_mm":50.,"geometry":{"kind":"path","d":d},"clip":{"kind":"rect","rect_mm":[0.,0.,65.,32.]}}]})).unwrap();
        for version in [14,21] {
            let raw=if version==14{cdr::encode_document(&doc)}else{cdr::encode_document21(&doc)}.unwrap().0;let actual=cdr::decode(&raw,"copy").unwrap().0;let object=actual.object("path").unwrap();assert_eq!(object.geometry,doc.objects[0].geometry);assert_eq!(object.clip,doc.objects[0].clip);
        }
    }
}
