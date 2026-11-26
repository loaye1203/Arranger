use arranger_core::{cdr,Clip,Document,Geometry};
use serde_json::json;
fn source(d:&str)->Document{serde_json::from_value(json!({"id":"smooth-source","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"shape","page_id":"page-1","x_mm":20.25,"y_mm":30.5,"width_mm":12.,"height_mm":8.,"geometry":{"kind":"path","d":d}}]})).unwrap()}
fn exact(source:&Document){for version in [14,21]{let bytes=if version==14{cdr::encode_document(source)}else{cdr::encode_document21(source)}.unwrap().0;let actual=cdr::decode(&bytes,&source.id).unwrap().0;assert_eq!(actual,*source,"native{version}");}}
#[test]
fn smooth_cubic_and_quadratic_notation_survives_repeated_relative_and_reset_commands(){for d in [
 "M0 0 C1 2 3 2 4 0 S7 -2 8 0 11 2 12 0",
 "m0 0 c1 2 3 2 4 0 s3 -2 4 0 3 2 4 0",
 "M0 0 Q2 3 4 0 T8 0 12 0",
 "m0 0 q2 3 4 0 t4 0 4 0",
 "M0 0 S2 3 4 0 L5 1 S7 3 9 0 M10 0 S12 2 14 0",
 "M0 0 T4 0 Q6 3 8 0 L9 1 T12 0 M14 0 T16 0",
 "M0 0 C1 2 3 2 4 0 T8 0 S10 2 12 0",
 "M0 0 Q2 3 4 0 S6 2 8 0 T12 0",
 "m1 2 q1 2 2 0z s1 2 2 0",
]{exact(&source(d));}}
#[test]
fn exponent_mantissa_sign_case_padding_and_signed_zero_are_source_formats(){for d in [
 "M+1.25e-01 -0.E+00 L+2.500E+00 .75e+001",
 "m .125e+1 -.5E-00 c 1e0 2E+00 3.0e00 2.e0 4e0 0e-00 s 3e0 -2E0 4E0 0e0",
 "M0e0 0E+00 Q2e+0 3e00 4e0 0e-0 T8.0E+0 0e0",
 "M1.234567e-1 0e0 L1.2345678e+0 2.3456789e0",
]{exact(&source(d));}}
#[test]
fn smooth_fine_paths_and_clips_recover_in_independent_parent_charts(){let d="M0.1234567 0 C1.1234567 2.7654321 3.3333333 -1.1111111 5 0 S8.1234567 2.3456789 10 0 L10 6 L0 6 Z";let mut doc=source(d);doc.objects[0].clip=Some(Clip::Path{d:d.into(),fill_rule:Default::default()});doc.objects[0].transform=[1.1,0.2,-0.125,0.9,0.125,-0.25];let mut parent=doc.objects[0].clone();parent.id="parent".into();parent.geometry=Geometry::Group;parent.clip=Some(Clip::Path{d:"M0 0 Q6 9 12 0 T24 0 L24 18 L0 18 Z".into(),fill_rule:Default::default()});parent.x_mm=1.25;parent.y_mm=2.5;parent.transform=[1.25,0.125,0.25,0.875,5.,6.];doc.objects[0].parent_id=Some(parent.id.clone());doc.objects.insert(0,parent);exact(&doc);}
#[test]
fn unrepresentable_scientific_points_still_report_losses(){for d in ["M1e-40 0 L1 1"]{let doc=source(d);for format in [arranger_core::save_check::SaveFormat::Cdr14,arranger_core::save_check::SaveFormat::Cdr21]{let result=arranger_core::save_check::analyze(&doc,format).unwrap();assert_eq!(result["can_save"],false);assert_eq!(result["interchange_fidelity"]["roundtrip_source_fields"]["exact"],false);}}}
