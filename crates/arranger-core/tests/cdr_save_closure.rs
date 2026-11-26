use arranger_core::{cdr,Document,Geometry};
use serde_json::json;
#[cfg(target_os="macos")]
const FONT:&str="PingFang SC";
#[cfg(not(target_os="macos"))]
const FONT:&str="Microsoft YaHei";
const FINE:&str="M0.1234567 0 C10.1234567 2.7654321 30.3333333 -1.1111111 100 0";
fn text()->Document{serde_json::from_value(json!({"id":"save-closure","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"text","page_id":"page-1","x_mm":20.,"y_mm":30.,"width_mm":80.,"height_mm":25.,"geometry":{"kind":"text","content":"Live 42","font_family":FONT,"font_size_mm":4.}}]})).unwrap()}
fn exact(doc:&Document){doc.validate().unwrap();for modern in [false,true]{let encode=if modern{cdr::encode_document21}else{cdr::encode_document};let raw=encode(doc).unwrap().0;let actual=cdr::decode(&raw,&doc.id).unwrap().0;assert_eq!(actual,*doc,"modern={modern}");let second=encode(&actual).unwrap().0;assert_eq!(cdr::decode(&second,&doc.id).unwrap().0,*doc);}}
#[test]
fn fine_live_guides_keep_source_controls_and_anchor_distances(){for anchor in ["start","middle","end"]{let mut doc=text();let mut guide=doc.objects[0].clone();guide.id="guide".into();guide.geometry=Geometry::Path{d:FINE.into(),fill_rule:Default::default()};guide.style.fill="none".into();doc.objects.push(guide);doc.objects[0].x_mm=0.;doc.objects[0].y_mm=0.;if let Geometry::Text{text_path,..}=&mut doc.objects[0].geometry{*text_path=Some(serde_json::from_value(json!({"path_id":"guide","start_offset_mm":43.1234567,"baseline_offset_mm":0.1234567,"anchor":anchor})).unwrap());}exact(&doc);}}
#[test]
fn empty_text_remains_an_editable_story(){let mut doc=text();if let Geometry::Text{content,..}=&mut doc.objects[0].geometry{content.clear();}exact(&doc);}
#[test]
fn text_outlines_keep_uniform_native_paint_without_converting_glyphs(){for fill in ["#123456","none"]{let mut doc=text();doc.objects[0].style.fill=fill.into();doc.objects[0].style.stroke="#000000".into();doc.objects[0].style.stroke_width_mm=0.25;doc.objects[0].style.stroke_linecap=arranger_core::LineCap::Round;exact(&doc);}}
#[test]
fn empty_groups_remain_real_native_groups(){let mut doc=text();doc.objects[0].geometry=Geometry::Group;exact(&doc);}

#[test]
fn point_alignment_uses_advance_anchor_with_multiline_and_affine_frames(){for align in ["center","right"]{for affine in [false,true]{let mut doc=text();if affine{doc.objects[0].transform=[1.1,0.25,-0.125,0.9,1.25,2.5];}if let Geometry::Text{content,layout,..}=&mut doc.objects[0].geometry{*content="Live 42\nA\n\nX".into();layout.align=serde_json::from_value(json!(align)).unwrap();}exact(&doc);}}}
#[test]
fn source_line_ending_formats_do_not_replace_native_story_characters(){let mut doc=text();if let Geometry::Text{content,..}=&mut doc.objects[0].geometry{*content="A\r\nB\rC\n\nD".into();}exact(&doc);}

#[test]
fn adaptive_active_curve_pen_scales_with_the_real_native_chart(){for matrix in [[1.,0.,0.,1.,0.,0.],[2.,0.25,0.125,1.5,1.25,2.5]]{let mut doc=text();doc.objects[0].geometry=Geometry::Path{d:FINE.into(),fill_rule:Default::default()};doc.objects[0].transform=matrix;doc.objects[0].style.stroke="#000000".into();doc.objects[0].style.stroke_width_mm=0.1234567;exact(&doc);}}

#[test]
fn affine_primitive_outline_uses_live_scaled_pen_in_both_directions(){for geometry in [Geometry::Rect{radius_mm:0.},Geometry::Ellipse]{let mut doc=text();doc.objects[0].geometry=geometry;doc.objects[0].transform=[2.,0.25,0.125,1.5,1.25,2.5];doc.objects[0].style.stroke="#000000".into();doc.objects[0].style.stroke_width_mm=0.1234567;doc.objects[0].style.stroke_miterlimit=6.25;exact(&doc);}}

#[test]
fn arc_local_curvature_and_flags_use_native_endpoints(){for d in ["M0 0 A30 20 30 0 1 100 0","m1 2 a20 30 -15 1 0 40 5 30 20 60 0 1 20 10","M0 0 A4 4 0 0110 0 L12 5Z","M0 0 A0 3 0 0 1 10 5"]{let mut doc=text();doc.objects[0].geometry=Geometry::Path{d:d.into(),fill_rule:Default::default()};exact(&doc);}}
