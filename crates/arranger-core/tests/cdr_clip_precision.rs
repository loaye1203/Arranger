use arranger_core::{cdr,Clip,Document,Geometry,Resource};
use serde_json::json;

const CURVE:&str="M0.1234567 0 C1.1234567 2.7654321 3.3333333 -1.1111111 5 0 L5 6 L0 6 Z";
fn source()->Document{serde_json::from_value(json!({"id":"clip-precision","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"shape","page_id":"page-1","x_mm":20.25,"y_mm":30.5,"width_mm":12.,"height_mm":8.,"geometry":{"kind":"rect","radius_mm":0.},"clip":{"kind":"path","d":CURVE,"fill_rule":"nonzero"}}]})).unwrap()}
fn exact(source:&Document){for version in [14,21]{let bytes=if version==14{cdr::encode_document(source)}else{cdr::encode_document21(source)}.unwrap().0;let actual=cdr::decode(&bytes,&source.id).unwrap().0;assert_eq!(actual,*source,"native{version}");}}
#[test]
fn seven_decimal_clip_curves_recover_on_translated_and_sheared_objects(){for matrix in [[1.,0.,0.,1.,0.,0.],[1.1,0.2,-0.125,0.9,0.125,-0.25]]{let mut doc=source();doc.objects[0].transform=matrix;exact(&doc);}}
#[test]
fn independently_scaled_path_and_clip_charts_recover_together(){let mut doc=source();doc.objects[0].geometry=Geometry::Path{d:CURVE.into(),fill_rule:Default::default()};doc.objects[0].transform=[1.1,0.2,-0.125,0.9,0.125,-0.25];exact(&doc);}
#[test]
fn seven_decimal_clip_rectangles_survive_parent_and_ancestor_charts(){let mut doc=source();doc.objects[0].clip=Some(Clip::Rect{rect_mm:[0.1234567,0.2345678,5.3456789,6.4567891]});let mut parent=doc.objects[0].clone();parent.id="parent".into();parent.geometry=Geometry::Group;parent.x_mm=0.25;parent.y_mm=0.5;parent.transform=[1.1,0.2,-0.125,0.9,0.125,-0.25];parent.clip=Some(Clip::Path{d:CURVE.into(),fill_rule:Default::default()});doc.objects[0].parent_id=Some(parent.id.clone());doc.objects.insert(0,parent);exact(&doc);}
#[test]
fn seven_decimal_source_clip_survives_image_fit_and_legacy_crop(){for fit in [arranger_core::ImageFit::Contain,arranger_core::ImageFit::Cover]{let mut doc=source();doc.resources.insert("image".into(),Resource::image(include_bytes!("../../../examples/assets/images/quadrants.jpg")).unwrap());doc.objects[0].geometry=Geometry::Image{resource_id:"image".into(),fit,clip_mm:Some([0.25,0.5,8.,6.])};doc.objects[0].transform=[1.1,0.2,-0.125,0.9,0.125,-0.25];exact(&doc);}}
#[test]
fn native_clip_hash_uses_each_page_origin_including_the_shared_desktop(){let mut doc=source();doc.pages[0].width_mm=213.0001;doc.pages[0].height_mm=296.0003;let mut page=doc.pages[0].clone();page.id="page-2".into();page.width_mm=148.0003;page.height_mm=210.0001;doc.pages.push(page);let mut on_second=doc.objects[0].clone();on_second.id="second".into();on_second.page_id="page-2".into();on_second.transform=[1.1,0.2,-0.125,0.9,0.125,-0.25];let mut shared=on_second.clone();shared.id="shared".into();shared.page_id=arranger_core::WORKSPACE_PAGE.into();shared.x_mm=-25.1234567;shared.y_mm=-10.7654321;doc.objects.extend([on_second,shared]);exact(&doc);}
#[test]
fn quadratic_and_relative_clip_notation_comes_from_the_actual_fine_native_curve(){for d in ["M0.1234567 0 Q1.2345678 2.3456789 3.4567891 0 L5 6 L0 6 Z","m +.1234567 -.7654321 l 1.1111111 2.2222222 h 3.3333333 v 4.4444444 z"]{let mut doc=source();doc.objects[0].clip=Some(Clip::Path{d:d.into(),fill_rule:Default::default()});doc.objects[0].transform=[1.1,0.2,-0.125,0.9,0.125,-0.25];exact(&doc);}}
