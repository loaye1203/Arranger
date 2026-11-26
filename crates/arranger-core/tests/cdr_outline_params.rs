use arranger_core::{cdr,Document,Geometry,save_check::{self,SaveFormat}};
use serde_json::json;

fn source(kind:&str,width:f64)->Document {
    let geometry=match kind{"rect"=>json!({"kind":"rect","radius_mm":0.125}),"ellipse"=>json!({"kind":"ellipse"}),"path"=>json!({"kind":"path","d":"M0 0L12 0L12 8Z"}),_=>unreachable!()};
    serde_json::from_value(json!({"id":"outline-precision","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"shape","page_id":"page-1","x_mm":23.25,"y_mm":34.5,"width_mm":12.,"height_mm":8.,"geometry":geometry,"style":{"fill":"#123456","stroke":"#000000","stroke_width_mm":width,"opacity":1.}}]})).unwrap()
}
fn encode(doc:&Document,version:u32)->Vec<u8>{if version==14{cdr::encode_document(doc)}else{cdr::encode_document21(doc)}.unwrap().0}
fn format(version:u32)->SaveFormat{if version==14{SaveFormat::Cdr14}else{SaveFormat::Cdr21}}

#[test]
fn native14_and21_preserve_guarded_active_primitive_widths_and_their_second_exchange() {
    for kind in ["rect","ellipse","path"] {for width in [0.1234567,1000.1234567] {
        let source=source(kind,width);
        for version in [14,21] {
            let actual=cdr::decode(&encode(&source,version),&source.id).unwrap().0;
            assert_eq!(actual,source,"native{version} {kind} width={width}");
            let second=cdr::decode(&encode(&actual,version),&source.id).unwrap().0;assert_eq!(second,source);
            let report=save_check::analyze(&source,format(version)).unwrap();assert_eq!(report["interchange_fidelity"]["roundtrip_source_fields"]["exact"],true);
            assert_eq!(report["can_save"],false);assert_eq!(report["interchange_fidelity"]["native_compatibility"],"not_tested_by_this_query");
        }
    }}
}

#[test]
fn active_width_recovery_uses_the_final_world_budget_instead_of_local_scalar_error() {
    for kind in ["rect","ellipse","path"] {for scale in [2.,100.] {
        let mut source=source(kind,0.1234567);source.objects[0].transform=[scale,0.,0.,scale,0.,0.];
        for version in [14,21] {
            let adaptive=true;
            let actual=cdr::decode(&encode(&source,version),&source.id).unwrap().0;assert_eq!(actual.objects[0].style.stroke_width_mm,if adaptive{0.1234567}else{0.1235},"native{version}, {kind}, scale{scale}");
            let report=save_check::analyze(&source,format(version)).unwrap();assert_eq!(report["interchange_fidelity"]["roundtrip_source_fields"]["exact"],adaptive);
            assert_eq!(report["interchange_fidelity"]["roundtrip_source_fields"]["differences"].as_array().unwrap().iter().any(|d|d["path"]=="/objects/0/style/stroke_width_mm"),!adaptive);
        }
    }}
}

#[test]
fn curve_and_pen_restoration_share_one_budget_and_retain_the_complete_native_chart_on_failure() {
    let mut source=source("rect",0.1234567);source.objects[0].geometry=Geometry::Rect{radius_mm:0.1234567};
    for version in [14,21] {
        let actual=cdr::decode(&encode(&source,version),&source.id).unwrap().0;
        assert_eq!(actual.objects[0].geometry,Geometry::Rect{radius_mm:0.1234567});assert_eq!(actual.objects[0].style.stroke_width_mm,0.1234567);
    }
}

#[test]
fn source_widths_below_native_grid_keep_explicit_precision_differences() {
    for kind in ["rect","path"] {let source=source(kind,0.00004);
        for version in [14,21] {
            let actual=cdr::decode(&encode(&source,version),&source.id).unwrap().0;
            let adaptive=true;
            assert_eq!(actual.objects[0].style.stroke_width_mm,if adaptive{0.00004}else{0.});
            let report=save_check::analyze(&source,format(version)).unwrap();assert_eq!(report["interchange_fidelity"]["roundtrip_source_fields"]["exact"],adaptive);
            assert_eq!(report["interchange_fidelity"]["roundtrip_source_fields"]["differences"].as_array().unwrap().iter().any(|d|d["path"]=="/objects/0/style/stroke_width_mm"),!adaptive);
        }
    }
}

#[test]
fn grid_path_active_width_is_guarded_with_actual_parent_and_clip_ownership() {
    for parent in [false,true] {for clip in [false,true] {
        let mut source=source("path",0.1234567);
        if clip{source.objects[0].clip=Some(arranger_core::Clip::Rect{rect_mm:[1.,1.,8.,5.]});}
        if parent {
            source.objects[0].parent_id=Some("parent".into());
            source.objects.push(serde_json::from_value(json!({"id":"parent","page_id":"page-1","x_mm":0.,"y_mm":0.,"width_mm":0.,"height_mm":0.,"transform":[0.5,0.1,-0.1,0.5,0.,0.],"geometry":{"kind":"group"}})).unwrap());
        }
        for version in [14,21] {
            let actual=cdr::decode(&encode(&source,version),&source.id).unwrap().0;assert_eq!(actual,source,"native{version} parent={parent}, clip={clip}");
            assert_eq!(cdr::decode(&encode(&actual,version),&source.id).unwrap().0,source);
        }
    }}
}

#[test]
fn native_fixed_world_pen_budget_survives_small_shape_matrices() {
    for kind in ["rect","ellipse","path"] {
        let mut source=source(kind,0.1234567);source.objects[0].x_mm=0.;source.objects[0].y_mm=0.;source.objects[0].transform=[0.001,0.,0.,0.001,0.,0.];source.objects[0].style.stroke_miterlimit=1000.;
        for version in [14,21] {
            let adaptive=true;
            let actual=cdr::decode(&encode(&source,version),&source.id).unwrap().0;assert_eq!(actual.objects[0].style.stroke_width_mm,if adaptive{0.1234567}else{0.1235},"native{version} {kind}");
            let report=save_check::analyze(&source,format(version)).unwrap();assert_eq!(report["interchange_fidelity"]["roundtrip_source_fields"]["exact"],adaptive);
        }
    }
}
