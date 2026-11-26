use arranger_core::{cdr,Document};
use serde_json::json;

fn source()->Document {
    serde_json::from_value(json!({"id":"frames","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"objects":[
        {"id":"group","page_id":"page-1","x_mm":10.,"y_mm":20.,"width_mm":123.,"height_mm":45.,"transform":[1.,0.2,-0.1,1.2,3.,4.],"geometry":{"kind":"group"}},
        {"id":"rect","parent_id":"group","page_id":"page-1","x_mm":5.25,"y_mm":6.5,"width_mm":12.,"height_mm":8.,"transform":[1.,0.12,0.3,0.9,0.75,2.],"geometry":{"kind":"rect","radius_mm":0.}},
        {"id":"text","page_id":"page-1","x_mm":28.25,"y_mm":80.5,"width_mm":100.,"height_mm":40.,"geometry":{"kind":"text","content":"中文 / English","font_family":"Microsoft YaHei","font_size_mm":4.1}}
    ],"resources":{}})).unwrap()
}

#[test]
fn native_static_ids_recover_interleaving_between_page_and_workspace_owners() {
    let mut source=source();source.objects.clear();
    for (id,owner) in [("first","page-1"),("shared","_workspace"),("last","page-1")] {
        source.objects.push(serde_json::from_value(json!({"id":id,"page_id":owner,"x_mm":10.,"y_mm":20.,"width_mm":12.,"height_mm":8.,"geometry":{"kind":"rect","radius_mm":0.}})).unwrap());
    }
    for version in [14,21] {
        let bytes=if version==14{cdr::encode_document(&source)}else{cdr::encode_document21(&source)}.unwrap().0;let actual=cdr::decode(&bytes,"copy").unwrap().0;
        assert_eq!(actual.objects.iter().map(|o|&o.id).collect::<Vec<_>>(),source.objects.iter().map(|o|&o.id).collect::<Vec<_>>());
    }
}

#[test]
fn source_local_frames_survive_native14_and21_without_flattening_the_group_chart() {
    let source=source();
    for version in [14,21] {
        let bytes=if version==14{cdr::encode_document(&source)}else{cdr::encode_document21(&source)}.unwrap().0;
        let actual=cdr::decode(&bytes,"copy").unwrap().0;
        for expected in &source.objects {
            let o=actual.object(&expected.id).unwrap();
            assert_eq!([o.x_mm,o.y_mm,o.width_mm,o.height_mm],[expected.x_mm,expected.y_mm,expected.width_mm,expected.height_mm],"{} v{version}",o.id);
            assert_eq!(o.transform,expected.transform,"{} v{version}",o.id);
            assert_eq!(o.parent_id,expected.parent_id);
        }
    }
}
