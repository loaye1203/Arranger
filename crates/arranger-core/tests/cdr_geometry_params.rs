use arranger_core::{cdr,Document,Geometry};
use serde_json::json;

fn rectangle(radius:f64)->Document {
    serde_json::from_value(json!({"id":"precision","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"shape","page_id":"page-1","x_mm":23.25,"y_mm":34.5,"width_mm":12.,"height_mm":8.,"geometry":{"kind":"rect","radius_mm":radius}}]})).unwrap()
}
#[test]
fn native14_and21_restore_off_grid_radius_without_storing_geometry_coordinates() {
    for radius in [0.1234567,0.00004,3.999999] {
        let source=rectangle(radius);
        for version in [14,21] {
            let bytes=if version==14{cdr::encode_document(&source)}else{cdr::encode_document21(&source)}.unwrap().0;
            let actual=cdr::decode(&bytes,"copy").unwrap().0;
            assert_eq!(actual.object("shape").unwrap().geometry,source.object("shape").unwrap().geometry,"radius={radius}, native{version}");
        }
    }
}
#[test]
fn rounded_source_frames_recover_dimension_precision_without_changing_their_units() {
    let mut source=rectangle(0.1234567);source.objects[0].width_mm=12.1234567;source.objects[0].height_mm=8.7654321;
    for version in [14,21] {
        let bytes=if version==14{cdr::encode_document(&source)}else{cdr::encode_document21(&source)}.unwrap().0;
        let actual=cdr::decode(&bytes,"copy").unwrap().0;
        assert_eq!(actual.objects,source.objects,"native{version}");
    }
}
#[test]
fn high_precision_path_loss_has_an_exact_source_field_diagnostic() {
    let mut source=rectangle(0.);
    source.objects[0].geometry=Geometry::Path{d:"M0.123456789012345 0 L1 1".into(),fill_rule:Default::default()};
    // Precision beyond the measured native local-unit domain stays explicit.
    source.objects[0].style.stroke="#000000".into();source.objects[0].style.stroke_width_mm=0.25;
    for version in [14,21] {
        let bytes=if version==14{cdr::encode_document(&source)}else{cdr::encode_document21(&source)}.unwrap().0;
        let actual=cdr::decode(&bytes,"copy").unwrap().0;
        assert_ne!(actual.object("shape").unwrap().geometry,source.object("shape").unwrap().geometry);
        let format=if version==14{arranger_core::save_check::SaveFormat::Cdr14}else{arranger_core::save_check::SaveFormat::Cdr21};
        let hash=source.hash();let report=arranger_core::save_check::analyze(&source,format).unwrap();
        assert_eq!(report["can_save"],false);assert_eq!(report["document_interchange"]["can_export"],true);
        assert_eq!(report["interchange_fidelity"]["roundtrip_source_fields"]["exact"],false);
        assert!(report["interchange_fidelity"]["roundtrip_source_fields"]["differences"].as_array().unwrap().iter().any(|d|d["path"]=="/objects/0/geometry/d"&&d["object_id"]=="shape"));
        assert_eq!(source.hash(),hash);
    }
}
#[test]
fn exact_source_fields_do_not_unlock_formal_saving_or_claim_native_acceptance() {
    let source=rectangle(0.1234567);
    for format in [arranger_core::save_check::SaveFormat::Cdr14,arranger_core::save_check::SaveFormat::Cdr21] {
        let report=arranger_core::save_check::analyze(&source,format).unwrap();
        assert_eq!(report["interchange_fidelity"]["roundtrip_source_fields"]["exact"],true);
        assert_eq!(report["interchange_fidelity"]["native_compatibility"],"not_tested_by_this_query");
        assert_eq!(report["can_save"],false);assert_eq!(report["full_document_save_available"],false);
    }
}
#[test]
fn fidelity_query_preserves_encoding_rejection_instead_of_an_empty_success() {
    let mut source=rectangle(0.);source.objects[0].width_mm=300_000.;
    let report=arranger_core::save_check::analyze(&source,arranger_core::save_check::SaveFormat::Cdr14).unwrap();
    assert_eq!(report["interchange_fidelity"]["encoding_status"],"rejected");
    assert_eq!(report["interchange_fidelity"]["roundtrip_source_fields"]["exact"],serde_json::Value::Null);
    assert_eq!(report["can_save"],false);
}
#[test]
fn outlined_tiny_rectangles_keep_physical_pens_through_real_scaling_units() {
    let mut source=rectangle(0.);source.objects[0].width_mm=0.000149;source.objects[0].transform=[10.,0.,0.,10.,0.,0.];source.objects[0].style.stroke="#000000".into();source.objects[0].style.stroke_width_mm=1.;
    for version in [14,21] {
        let bytes=if version==14{cdr::encode_document(&source)}else{cdr::encode_document21(&source)}.unwrap().0;
        let actual=cdr::decode(&bytes,"copy").unwrap().0;let object=actual.object("shape").unwrap();
        assert_eq!(object.width_mm,0.000149,"native{version} preserves the source chart through a native scaling pen");
        assert_eq!([object.transform[0],object.transform[1],object.transform[2],object.transform[3]],[10.,0.,0.,10.]);
        assert_eq!(object.style.stroke_width_mm,1.);
    }
}
#[test]
fn radius_restore_is_bounded_in_world_units_including_parent_scale_and_shear() {
    for(matrix,exact)in [([4.,0.,0.,4.,0.,0.],true),([5.,0.,0.,5.,0.,0.],true),([100.,0.,0.,100.,0.,0.],true),([4.,0.,4.,1.,0.,0.],true)] {
        let mut source=rectangle(0.1234567);source.objects[0].transform=matrix;
        for version in [14,21] {
            let bytes=if version==14{cdr::encode_document(&source)}else{cdr::encode_document21(&source)}.unwrap().0;
            let actual=cdr::decode(&bytes,"copy").unwrap().0;
            assert_eq!(actual.object("shape").unwrap().geometry,Geometry::Rect{radius_mm:if exact{0.1234567}else{0.1235}},"native{version} matrix{matrix:?}");
            let format=if version==14{arranger_core::save_check::SaveFormat::Cdr14}else{arranger_core::save_check::SaveFormat::Cdr21};let report=arranger_core::save_check::analyze(&source,format).unwrap();
            assert_eq!(report["interchange_fidelity"]["roundtrip_source_fields"]["exact"],exact);
            if !exact {assert!(report["interchange_fidelity"]["roundtrip_source_fields"]["differences"].as_array().unwrap().iter().any(|d|d["path"]=="/objects/0/geometry/radius_mm"));}
        }
    }
}
#[test]
fn dimension_and_radius_errors_share_one_world_curve_budget() {
    let mut source=rectangle(0.1234567);source.objects[0].width_mm=12.123449;source.objects[0].transform=[4.,0.,0.,4.,0.,0.];
    for version in [14,21] {
        let bytes=if version==14{cdr::encode_document(&source)}else{cdr::encode_document21(&source)}.unwrap().0;
        let actual=cdr::decode(&bytes,"copy").unwrap().0;let object=actual.object("shape").unwrap();
        assert_eq!(object.width_mm,12.123449);assert_eq!(object.geometry,Geometry::Rect{radius_mm:0.1234567},"native{version}: adaptive units keep the combined curve budget");
    }
}
#[test]
fn inherited_group_scale_also_bounds_round_corner_restoration() {
    let mut source=rectangle(0.1234567);source.objects[0].parent_id=Some("parent".into());
    source.objects.push(serde_json::from_value(json!({"id":"parent","page_id":"page-1","x_mm":0.,"y_mm":0.,"width_mm":0.,"height_mm":0.,"transform":[100.,0.,0.,100.,0.,0.],"geometry":{"kind":"group"}})).unwrap());
    for version in [14,21] {
        let bytes=if version==14{cdr::encode_document(&source)}else{cdr::encode_document21(&source)}.unwrap().0;
        let actual=cdr::decode(&bytes,"copy").unwrap().0;
        assert_eq!(actual.object("shape").unwrap().geometry,Geometry::Rect{radius_mm:0.1234567});
    }
}

#[test]
fn maximal_rounded_corners_use_a_valid_native_chart_before_either_version_is_written() {
    for (width,height,radius) in [(0.0007,0.00105,0.00035),(0.0011,0.00165,0.00055),(8.0003,12.00045,4.00015)] {
        let mut source=rectangle(radius);source.objects[0].width_mm=width;source.objects[0].height_mm=height;
        source.validate().unwrap();
        for encode in [cdr::encode_document,cdr::encode_document21] {
            let bytes=encode(&source).unwrap().0;let actual=cdr::decode(&bytes,&source.id).unwrap().0;
            assert_eq!(actual,source,"maximal radius {radius} must not round above half the native width");
            assert_eq!(cdr::decode(&encode(&actual).unwrap().0,&source.id).unwrap().0,source);
        }
    }
}

#[test]
fn unrepresentable_joint_primitive_precision_rejects_before_any_target_or_recovery_file_write() {
    let mut source=rectangle(0.125);let object=&mut source.objects[0];object.width_mm=12.1234551;
    object.style.stroke="#000000".into();object.style.stroke_width_mm=0.123451;
    object.transform=[30.,0.,0.,1.1e-10/30.,0.,0.];source.validate().unwrap();let before=source.hash();
    let path=std::env::temp_dir().join(format!("arranger-primitive-budget-{}.cdr",std::process::id()));
    for suffix in ["",".pending",".bak",".lock"] {std::fs::write(format!("{}{suffix}",path.display()),b"preserve-existing").unwrap();}
    for export in [cdr::export_document,cdr::export_document21] {
        let error=export(&source,&path,true).unwrap_err();assert_eq!(error.code,"CDR_UNSUPPORTED");
        assert!(error.message.contains("physical bound"));assert_eq!(source.hash(),before);
        for suffix in ["",".pending",".bak",".lock"] {assert_eq!(std::fs::read(format!("{}{suffix}",path.display())).unwrap(),b"preserve-existing");}
    }
    for suffix in ["",".pending",".bak",".lock"] {std::fs::remove_file(format!("{}{suffix}",path.display())).unwrap();}
}

#[test]
fn a_curve_cannot_fall_back_to_a_coarse_chart_after_all_joint_pen_candidates_fail() {
    let mut source=rectangle(0.);let object=&mut source.objects[0];object.x_mm=0.;object.y_mm=0.;
    object.geometry=Geometry::Path{d:"M0.1234551 0 L12.1234551 8".into(),fill_rule:Default::default()};
    object.style.stroke="#000000".into();object.style.stroke_width_mm=0.123451;
    object.transform=[30.,0.,0.,1.1e-10/30.,0.,0.];source.validate().unwrap();
    for encode in [cdr::encode_document,cdr::encode_document21] {let e=encode(&source).unwrap_err();assert_eq!(e.code,"CDR_UNSUPPORTED");assert!(e.message.contains("joint geometry/pen"));}
    // A coarser chart still remains available when it really fits the joint
    // physical budget; exact spelling and formal Save are separate contracts.
    source.objects[0].transform=[0.5,0.,0.,2.2e-11,0.,0.];
    for encode in [cdr::encode_document,cdr::encode_document21] {assert!(cdr::decode(&encode(&source).unwrap().0,"coarse").is_ok());}
}
