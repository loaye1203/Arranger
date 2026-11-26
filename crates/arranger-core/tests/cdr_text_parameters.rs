use arranger_core::{cdr, Document};
use serde_json::json;

fn source()->Document {
    serde_json::from_value(json!({"id":"text-parameters","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[
        {"id":"point","page_id":"page-1","x_mm":20.25,"y_mm":30.5,"width_mm":150.,"height_mm":20.,"geometry":{"kind":"text","content":"中文 / English","font_family":"Microsoft YaHei","font_size_mm":4.1234567,"layout":{"mode":"point","letter_spacing_mm":0.12345678}}},
        {"id":"area","page_id":"page-1","x_mm":20.25,"y_mm":70.5,"width_mm":65.,"height_mm":55.,"geometry":{"kind":"text","content":"中英文 mixed\n\nSecond line","font_family":"Microsoft YaHei","font_size_mm":3.9876543,"layout":{"mode":"area","align":"center","letter_spacing_mm":0.23456789,"line_height_mm":7.2345678}}}
    ]})).unwrap()
}

#[test]
fn precise_path_offsets_and_anchor_survive_the_native_scalar_projection() {
    for anchor in ["start","middle","end"] {
        let source:Document=serde_json::from_value(json!({"id":"path-scalars","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[
            {"id":"guide","page_id":"page-1","x_mm":20.,"y_mm":70.,"width_mm":90.,"height_mm":30.,"geometry":{"kind":"path","d":"M0 20 C20 -10 65 -10 90 20"}},
            {"id":"path-text","page_id":"page-1","x_mm":0.,"y_mm":0.,"width_mm":100.,"height_mm":20.,"geometry":{"kind":"text","content":"沿路径 / English","font_family":"Microsoft YaHei","font_size_mm":4.1234567,"text_path":{"path_id":"guide","start_offset_mm":51.2345678,"baseline_offset_mm":-1.2345678,"anchor":anchor}}}
        ]})).unwrap();
        for version in [14,21] {
            let bytes=if version==14{cdr::encode_document(&source)}else{cdr::encode_document21(&source)}.unwrap().0;
            let actual=cdr::decode(&bytes,"copy").unwrap().0;
            assert_eq!(actual.object("path-text").unwrap().geometry,source.object("path-text").unwrap().geometry,"{anchor} v{version}");
        }
    }
}

#[test]
fn source_font_alias_and_precise_layout_survive_native14_and21() {
    let source=source();
    for version in [14,21] {
        let bytes=if version==14{cdr::encode_document(&source)}else{cdr::encode_document21(&source)}.unwrap().0;
        let actual=cdr::decode(&bytes,"copy").unwrap().0;
        for expected in &source.objects {
            assert_eq!(actual.object(&expected.id).unwrap().geometry,expected.geometry,"{} v{version}",expected.id);
        }
    }
}

#[test]
fn actual_corel_created_latin_font_edit_remains_regular_live_text() {
    let (doc,_)=cdr::decode(include_bytes!("../../../examples/assets/cdr/corel2019/latin-font-edited-v21.cdr"),"native-edit").unwrap();
    let arranger_core::Geometry::Text{content,font_family,font_size_mm,layout,..}=&doc.object("probe").unwrap().geometry else{panic!("native text flattened")};
    assert_eq!(content,"Live text ABC / edited");assert_eq!(font_family,"Calibri");assert!((*font_size_mm-6.25).abs()<0.00021);assert!((layout.line_height_mm.unwrap()-10.25).abs()<0.00021);
}

#[test]
fn actual_corel_legacy_cjk_font_is_editable_without_substitution() {
    let (doc,_)=cdr::decode(include_bytes!("../../../examples/assets/cdr/corel2019/fz-legacy-uniform-v21.cdr"),"native-legacy").unwrap();
    let arranger_core::Geometry::Text{font_family,content,..}=&doc.object("probe").unwrap().geometry else{panic!("legacy text flattened")};assert_eq!(font_family,"方正黑体简体");assert_eq!(content,"中文 mixed\n\nSecond line");
}

#[test]
fn legacy_font_alias_point_and_area_write_both_native_versions() {
    let mut source=source();for o in &mut source.objects {if let arranger_core::Geometry::Text{font_family,..}=&mut o.geometry{*font_family="FZHei-B01S".into();}}
    for version in [14,21] {let bytes=if version==14{cdr::encode_document(&source)}else{cdr::encode_document21(&source)}.unwrap().0;let actual=cdr::decode(&bytes,"copy").unwrap().0;for o in &source.objects{assert_eq!(actual.object(&o.id).unwrap().geometry,o.geometry);}}
}

#[test]
fn actual_corel_regular_area_style4904_keeps_live_mixed_text_and_scalars() {
    let (doc,_)=cdr::decode(include_bytes!("../../../examples/assets/cdr/corel2019/area-regular-style4904-v21.cdr"),"native-area").unwrap();
    let arranger_core::Geometry::Text{content,font_family,font_size_mm,..}=&doc.object("probe").unwrap().geometry else{panic!("area text flattened")};
    assert_eq!(content,"中文 mixed\n\nSecond line");assert_eq!(font_family,"Microsoft YaHei");assert_eq!(*font_size_mm,4.1234567);
}
