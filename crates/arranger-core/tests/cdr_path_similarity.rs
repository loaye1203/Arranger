use arranger_core::{cdr,Document,Geometry};
use serde_json::json;
fn source(matrix:[f64;6])->Document{
    serde_json::from_value(json!({"id":"path-parent","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"parent","page_id":"page-1","x_mm":0.,"y_mm":0.,"width_mm":0.,"height_mm":0.,"transform":matrix,"geometry":{"kind":"group"}},{"id":"guide","page_id":"page-1","x_mm":10.,"y_mm":20.,"width_mm":110.,"height_mm":20.,"geometry":{"kind":"path","d":"M0.1234567 0 C30.1234567 10.7654321 70.3333333 -10.1111111 100 0"}},{"id":"text","page_id":"page-1","parent_id":"parent","x_mm":0.,"y_mm":0.,"width_mm":80.,"height_mm":25.,"geometry":{"kind":"text","content":"Live 42","font_family":if cfg!(target_os="macos"){"PingFang SC"}else{"Microsoft YaHei"},"font_size_mm":4.1234567,"layout":{"letter_spacing_mm":0.1234567},"text_path":{"path_id":"guide","start_offset_mm":3.1234567,"baseline_offset_mm":0.1234567,"anchor":"start"}}}]})).unwrap()
}
#[test]
fn conformal_text_parent_keeps_the_same_real_guide_font_and_offsets(){
    for matrix in [[2.,0.,0.,2.,3.,4.],[0.,2.,-2.,0.,3.,4.],[0.6,0.8,-0.8,0.6,3.,4.],[0.3,0.4,-0.4,0.3,3.,4.]]{for anchor in ["start","middle","end"]{
        let mut source=source(matrix);let Geometry::Text{text_path:Some(binding),..}=&mut source.objects[2].geometry else{unreachable!()};binding.anchor=serde_json::from_value(json!(anchor)).unwrap();if anchor!="start"{binding.start_offset_mm=30.1234567;}
        for version in [14,21]{let encode=if version==14{cdr::encode_document}else{cdr::encode_document21};let actual=cdr::decode(&encode(&source).unwrap().0,&source.id).unwrap().0;assert_eq!(actual,source,"native{version} parent{matrix:?} anchor{anchor}");assert_eq!(cdr::decode(&encode(&actual).unwrap().0,&source.id).unwrap().0,source);}
    }}
}
#[test]
fn nested_conformal_groups_and_original_clips_stay_editable(){
    let mut source=source([0.,2.,-2.,0.,3.,4.]);let mut parent=source.objects[0].clone();parent.id="outer".into();parent.transform=[0.5,0.,0.,0.5,7.,8.];source.objects[0].parent_id=Some("outer".into());source.objects.push(parent);source.objects[2].clip=Some(arranger_core::Clip::Rect{rect_mm:[-10.,-10.,200.,200.]});
    for version in [14,21]{let bytes=if version==14{cdr::encode_document(&source)}else{cdr::encode_document21(&source)}.unwrap().0;assert_eq!(cdr::decode(&bytes,&source.id).unwrap().0,source);}
}
#[test]
fn nonconformal_and_reflected_parents_remain_explicitly_rejected(){
    for matrix in [[2.,0.,0.,1.,0.,0.],[1.,0.,0.25,1.,0.,0.],[-1.,0.,0.,1.,0.,0.]]{let source=source(matrix);assert!(cdr::encode_document(&source).is_err());assert!(cdr::encode_document21(&source).is_err());}
}
#[test]
fn native21_isolation_and_live_text_outline_share_the_similarity_chart(){
    let mut source=source([0.,2.,-2.,0.,3.,4.]);source.objects[0].style.opacity=0.5;source.objects[2].style.stroke="#000000".into();source.objects[2].style.stroke_width_mm=0.25;source.objects[2].clip=Some(arranger_core::Clip::Rect{rect_mm:[-10.,-10.,200.,200.]});
    assert!(cdr::encode_document(&source).is_err());let bytes=cdr::encode_document21(&source).unwrap().0;assert_eq!(cdr::decode(&bytes,&source.id).unwrap().0,source);
}
#[test]
fn three_nonbinary_parent_scales_restore_the_exact_original_text_parameters(){
    let mut source=source([1.1,0.7,-0.7,1.1,3.,4.]);source.objects[0].parent_id=Some("middle".into());let mut middle=source.objects[0].clone();middle.id="middle".into();middle.transform=[0.1,0.2,-0.2,0.1,5.,6.];middle.parent_id=Some("outer".into());let mut outer=middle.clone();outer.id="outer".into();outer.parent_id=None;outer.transform=[0.1,0.2,-0.2,0.1,7.,8.];source.objects.extend([middle,outer]);
    for version in [14,21]{let bytes=if version==14{cdr::encode_document(&source)}else{cdr::encode_document21(&source)}.unwrap().0;assert_eq!(cdr::decode(&bytes,&source.id).unwrap().0,source,"native{version}: parent association must match Document::world_transform");}
}
