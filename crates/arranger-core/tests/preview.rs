use arranger_core::{Document, Geometry, Object, Snapshot, model::DocumentPreview};
use serde_json::json;
use std::sync::Arc;

fn scene() -> Snapshot {
    let document: Document = serde_json::from_value(json!({
        "id":"preview","unit":"mm","pages":[{"id":"p","width_mm":210.,"height_mm":297.}],
        "resources":{},"objects":[
            {"id":"group","page_id":"p","width_mm":10.,"height_mm":10.,"geometry":{"kind":"group"}},
            {"id":"guide","parent_id":"group","page_id":"p","width_mm":10.,"height_mm":10.,"geometry":{"kind":"path","d":"M0 0L10 10"}},
            {"id":"label","page_id":"p","width_mm":10.,"height_mm":10.,"geometry":{"kind":"text","content":"中文","font_family":"Arial","font_size_mm":3.,"text_path":{"path_id":"guide"}}}
        ]
    })).unwrap();
    document.validate().unwrap();
    Snapshot {document_id:"preview".into(),revision:1,document_hash:document.hash(),document:Arc::new(document),dirty:false,busy:false,undo_depth:0,redo_depth:0,saved_path:None,warnings:vec![]}
}

#[test]
fn invalid_guide_or_ancestor_restores_the_last_preview_and_preserves_the_source() {
    let snapshot=scene();
    for id in ["guide","group"] {
        let initial=snapshot.document.object(id).unwrap();
        let mut preview=DocumentPreview::new(&snapshot,std::slice::from_ref(initial)).unwrap();
        let mut moved=initial.clone();moved.x_mm+=7.1;
        assert!(preview.update(vec![moved.clone()]).unwrap());
        assert!(!preview.update(vec![moved.clone()]).unwrap());
        let good=preview.document().hash();
        let mut invalid=moved.clone();invalid.transform=[0.;6];
        let mut oracle=(*snapshot.document).clone();*oracle.objects.iter_mut().find(|o|o.id==id).unwrap()=invalid.clone();
        assert_eq!(preview.update(vec![invalid]).unwrap_err().code,oracle.validate().unwrap_err().code);
        assert_eq!(preview.document().hash(),good);
        assert!(preview.update(vec![initial.clone()]).unwrap());assert!(preview.at_source());
    }
    assert_eq!(snapshot.document.hash(),snapshot.document_hash);
}

#[test]
fn text_draft_rejects_an_invalid_path_story_without_touching_other_objects() {
    let snapshot=scene();let object=snapshot.document.object("label").unwrap();
    let mut geometry=object.geometry.clone();
    let Geometry::Text {content,..}=&mut geometry else{unreachable!()};content.push('\n');
    assert_eq!(snapshot.document.draft_text("label",&geometry).unwrap_err().code,"INVALID_TEXT_LAYOUT");
    let mut corrected=geometry;
    if let Geometry::Text {content,..}=&mut corrected{*content="新词".into();}
    let edited:Object=snapshot.document.draft_text("label",&corrected).unwrap();
    assert_eq!(edited.geometry,corrected);assert_eq!(snapshot.document.hash(),snapshot.document_hash);
}

#[test]
fn nested_bounds_match_world_geometry_in_document_order_with_empty_groups() {
    let mut doc: Document = serde_json::from_value(json!({
        "id":"nested","unit":"mm","pages":[{"id":"p","width_mm":210.,"height_mm":297.}],"resources":{},
        "objects":[
            {"id":"ellipse","page_id":"p","parent_id":"inner","width_mm":7.2,"height_mm":4.8,"geometry":{"kind":"ellipse"}},
            {"id":"empty","width_mm":0.,"height_mm":0.,"page_id":"p","parent_id":"outer","geometry":{"kind":"group"}},
            {"id":"inner","width_mm":0.,"height_mm":0.,"page_id":"p","parent_id":"outer","x_mm":7.8,"transform":[0.3,0.72,0.8,0.3,9.5,0.42],"geometry":{"kind":"group"}},
            {"id":"path","width_mm":2.,"height_mm":12.,"page_id":"p","parent_id":"inner","geometry":{"kind":"path","d":"M 0 0 Q 6 10 2 12"}},
            {"id":"outer","width_mm":0.,"height_mm":0.,"page_id":"p","transform":[0.88,0.76,-0.3,1.2,30.,15.],"geometry":{"kind":"group"}}
        ]
    })).unwrap();
    doc.validate().unwrap();
    for x in [0., 19.25, -73.61] {
        doc.objects[4].x_mm=x;
        let rect = |id:&str| {
            let [x,y,w,h] = doc.bounds(doc.object(id).unwrap()).unwrap();
            kurbo::Rect::new(x,y,x+w,y+h)
        };
        let r=rect("ellipse").union(rect("path"));
        assert_eq!(doc.bounds(&doc.objects[2]).unwrap(),[r.x0,r.y0,r.width(),r.height()]);
        let r=rect("inner").union(kurbo::Rect::ZERO);
        assert_eq!(doc.bounds(&doc.objects[4]).unwrap(),[r.x0,r.y0,r.width(),r.height()]);
    }
}
