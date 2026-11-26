use arranger_core::{cdr,Document};
use serde_json::json;
fn source(d:&str)->Document {serde_json::from_value(json!({"id":"fine-path","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"path","page_id":"page-1","x_mm":20.25,"y_mm":30.5,"width_mm":12.,"height_mm":8.,"geometry":{"kind":"path","d":d,"fill_rule":"nonzero"}}]})).unwrap()}
#[test]
fn ordinary_seven_decimal_native_paths_recover_source_spelling_from_active_points() {
    for d in ["M0.1234567 0 C1.1234567 2.7654321 3.3333333 -1.1111111 5 0","m +.1234567 -.7654321 l 1.1111111 2.2222222 h 3.3333333 v -4.4444444 z","M0.1234567 0 Q1.2345678 2.3456789 3.4567891 0"] {
        let source=source(d);
        for version in [14,21] {
            let bytes=if version==14{cdr::encode_document(&source)}else{cdr::encode_document21(&source)}.unwrap().0;
            let actual=cdr::decode(&bytes,&source.id).unwrap().0;assert_eq!(actual.objects,source.objects,"native{version} {d}");
        }
    }
}
#[test]
fn scaled_native_paths_preserve_own_and_ancestor_clips_with_flattened_parent_coordinates() {
    for mode in [1,2,3] {
        let mut source=source("M0.1234567 0 C1.1234567 2.7654321 3.3333333 -1.1111111 5 0");
        source.objects[0].transform=[1.1,0.2,0.1,0.9,0.5,-0.25];
        if mode==1||mode==3{source.objects[0].clip=Some(arranger_core::Clip::Rect{rect_mm:[0.125,-1.75,4.5,3.]});}
        if mode>=2 {
            let mut parent=source.objects[0].clone();parent.id="parent".into();parent.x_mm=3.;parent.y_mm=4.;parent.transform=[0.9,0.1,-0.1,1.1,4.,5.];parent.geometry=arranger_core::Geometry::Group;parent.clip=if mode==3{Some(arranger_core::Clip::Rect{rect_mm:[0.,0.,60.,60.]})}else{None};parent.style=Default::default();
            source.objects[0].parent_id=Some("parent".into());source.objects.push(parent);
        }
        if mode==3 {source.objects[0].style.opacity=0.8;}
        for version in [14,21] {
            let bytes=if version==14{cdr::encode_document(&source)}else{cdr::encode_document21(&source)}.unwrap().0;
            let actual=cdr::decode(&bytes,&source.id).unwrap().0;assert_eq!(actual,source,"mode{mode} native{version}");
        }
    }
}
#[test]
fn unchanged_precision_native_reexchange_and_model_edits_stay_exact() {
    let source=source("M0.1234567 0 C1.1234567 2.7654321 3.3333333 -1.1111111 5 0");
    for version in [14,21] {
        let mut doc=source.clone();for _ in 0..3 {let bytes=if version==14{cdr::encode_document(&doc)}else{cdr::encode_document21(&doc)}.unwrap().0;doc=cdr::decode(&bytes,&source.id).unwrap().0;assert_eq!(doc,source);}
        let mut changed=doc.clone();changed.objects[0].geometry=arranger_core::Geometry::Path{d:"M0.1234568 0 L1.2345678 2.3456789".into(),fill_rule:Default::default()};
        let bytes=if version==14{cdr::encode_document(&changed)}else{cdr::encode_document21(&changed)}.unwrap().0;assert_eq!(cdr::decode(&bytes,&source.id).unwrap().0,changed);
    }
}
#[test]
fn range_syntax_and_live_semantic_boundaries_remain_explicit_source_field_differences() {
    for mode in [0,3,4,5] {
        let mut source=source("M0.1234567 0 C1.1234567 2.7654321 3.3333333 -1.1111111 5 0");
        match mode {
            0=>source.objects[0].style.stroke="#000000".into(),
            1=>source.objects[0].clip=Some(arranger_core::Clip::Rect{rect_mm:[0.,0.,3.,3.]}),
            2=>{let mut parent=source.objects[0].clone();parent.id="parent".into();parent.geometry=arranger_core::Geometry::Group;source.objects[0].parent_id=Some("parent".into());source.objects.push(parent);},
            3=>source.objects[0].geometry=arranger_core::Geometry::Path{d:"M200000.1234567 0 L200010.1234567 1".into(),fill_rule:Default::default()},
            4=>source.objects[0].transform=[0.000001,0.,0.,0.000001,0.,0.],
            5=>source.objects[0].geometry=arranger_core::Geometry::Path{d:"M0.123456789012345 0 L1 1".into(),fill_rule:Default::default()},
            _=>source.objects[0].geometry=arranger_core::Geometry::Path{d:"M0.1234567 0 A1 1 0 0 1 2 0".into(),fill_rule:Default::default()},
        }
        for version in [14,21] {
            let format=if version==14{arranger_core::save_check::SaveFormat::Cdr14}else{arranger_core::save_check::SaveFormat::Cdr21};let report=arranger_core::save_check::analyze(&source,format).unwrap();
            assert_eq!(report["can_save"],false);assert_eq!(report["document_interchange"]["can_export"],true,"mode{mode} native{version}: {report}");
            assert_eq!(report["interchange_fidelity"]["roundtrip_source_fields"]["exact"],false,"mode{mode} native{version}");
            assert!(report["interchange_fidelity"]["roundtrip_source_fields"]["differences"].as_array().unwrap().iter().any(|d|d["path"]=="/objects/0/geometry/d"),"mode{mode} native{version}: {report}");
        }
    }
}
