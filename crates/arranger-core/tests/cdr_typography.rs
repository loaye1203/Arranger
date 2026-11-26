use arranger_core::{cdr,Document,Geometry,Object,Style,TextAlign,TextLayoutOptions,TextMode};
use kurbo::Point;
use std::{fs,path::PathBuf};
fn root()->PathBuf {PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")}
fn near(a:f64,b:f64){assert!((a-b).abs()<0.00021,"{a} != {b}");}
fn area()->Object {Object{id:"paragraph".into(),page_id:"page-1".into(),parent_id:None,x_mm:15.,y_mm:37.,width_mm:80.,height_mm:60.,transform:[1.,0.,0.,1.,0.,0.],style:Style{fill:"#123456".into(),stroke:"none".into(),stroke_width_mm:0.,..Default::default()},visible:true,locked:false,clip:None,geometry:Geometry::Text{content:"区域文字 / English\n\n保留空行\n".into(),font_family:"Microsoft YaHei".into(),font_size_mm:6.35,text_path:None,layout:TextLayoutOptions{mode:TextMode::Area,align:TextAlign::Center,letter_spacing_mm:0.,line_height_mm:Some(8.)}}}}
#[test]
fn area_frames_content_empty_paragraphs_alignment_and_tracking_survive_affine_roundtrip() {
    for align in [TextAlign::Left,TextAlign::Center,TextAlign::Right] {for spacing in [0.,0.4,-0.1] {for family in ["Microsoft YaHei","Arial"] {
        let mut doc=Document::new("source".into(),148.,210.).unwrap();let mut object=area();
        if let Geometry::Text{font_family,layout,..}=&mut object.geometry {*font_family=family.into();layout.align=align;layout.letter_spacing_mm=spacing;}
        object.transform=[-0.9,0.2,0.3,1.1,120.,-8.];doc.objects.push(object.clone());
        let before=serde_json::to_value(&doc).unwrap();let (data,warnings)=cdr::encode_document(&doc).unwrap();assert!(warnings.iter().any(|w|w.code=="CDR_TEXT_LAYOUT"));assert_eq!(serde_json::to_value(&doc).unwrap(),before);
        let (imported,warnings)=cdr::decode(&data,"imported").unwrap();assert!(warnings.iter().any(|w|w.code=="CDR_TEXT_LAYOUT"));let got=&imported.objects[0];assert_eq!(got.id,object.id);near(got.width_mm,80.);near(got.height_mm,60.);
        let Geometry::Text{content,font_size_mm,layout,..}=&got.geometry else{panic!("area text flattened")};assert_eq!(content,"区域文字 / English\n\n保留空行\n");near(*font_size_mm,6.35);assert_eq!(layout.mode,TextMode::Area);assert_eq!(layout.align,align);near(layout.letter_spacing_mm,spacing);near(layout.line_height_mm.unwrap(),8.);
        assert_eq!([got.x_mm,got.y_mm],[object.x_mm,object.y_mm]);assert_eq!(got.transform,object.transform);
        for p in [(0.,0.),(80.,0.),(80.,60.),(0.,60.)] {let a=doc.world_transform(&object).unwrap()*Point::from(p);let b=imported.world_transform(got).unwrap()*Point::from(p);near(a.x,b.x);near(a.y,b.y);}
        let (again,_)=cdr::decode(&cdr::encode_document(&imported).unwrap().0,"again").unwrap();assert_eq!(got.geometry,again.objects[0].geometry);
    }}}
}
#[test]
fn actual_corel_area_original_and_edited_references_keep_controlled_values() {
    for kind in ["area-basic","area-width","area-height","area-align2","area-align3","area-spacing","area-leading"] {for suffix in ["","-edited"] {
        let path=root().join(format!("reports/windows/WIN-SAVE-ROUND-20261024-01/layout-native/{kind}-v14{suffix}.cdr"));let (doc,_)=cdr::decode(&fs::read(path).unwrap(),"actual").unwrap();let o=&doc.objects[0];
        near(o.width_mm,if kind=="area-width" {120.}else{80.});near(o.height_mm,if kind=="area-height" {90.}else{60.});near(o.transform[4],15.);near(o.transform[5],37.);
        let Geometry::Text{content,font_size_mm,layout,..}=&o.geometry else{panic!("native area not editable")};assert!(content.contains("\n\n末行。"));assert_eq!(content.ends_with(" 已编辑"),suffix=="-edited");near(*font_size_mm,4.2333);near(layout.line_height_mm.unwrap(),if kind=="area-leading" {7.0556}else{5.08});
        assert_eq!(layout.align,match kind {"area-align2"=>TextAlign::Right,"area-align3"=>TextAlign::Center,_=>TextAlign::Left});assert_eq!(layout.letter_spacing_mm>0.,kind=="area-spacing");
        let (reopened,_)=cdr::decode(&cdr::encode_document(&doc).unwrap().0,"reopened").unwrap();let Geometry::Text{layout:new,..}=&reopened.objects[0].geometry else{panic!()};near(new.letter_spacing_mm,layout.letter_spacing_mm);
    }}
}
#[test]
fn multiple_paragraphs_keep_independent_styles_and_original_default_leading() {
    let mut d=Document::new("styles".into(),210.,297.).unwrap();
    for i in 0..3 {let mut o=area();o.id=format!("area-{i}");o.x_mm+=i as f64*20.;o.style.fill=["#123456","cmyk(0,100,40,0)","#ffffff"][i].into();if let Geometry::Text{font_size_mm,layout,..}=&mut o.geometry {*font_size_mm+=i as f64;layout.line_height_mm=None;layout.align=[TextAlign::Left,TextAlign::Center,TextAlign::Right][i];}d.objects.push(o);}
    let (out,_)=cdr::decode(&cdr::encode_document(&d).unwrap().0,"read").unwrap();for (a,b) in d.objects.iter().zip(&out.objects) {assert_eq!(a.id,b.id);assert_eq!(a.style.fill,b.style.fill);assert_eq!(a.geometry,b.geometry);}
}
#[test]
fn area_tracking_and_geometry_outside_native_range_reject_without_mutating_source() {
    let mut doc=Document::new("invalid".into(),210.,297.).unwrap();let mut o=area();if let Geometry::Text{layout,..}=&mut o.geometry {layout.letter_spacing_mm=10000.;}doc.objects.push(o);
    let before=serde_json::to_value(&doc).unwrap();assert_eq!(cdr::encode_document(&doc).unwrap_err().code,"CDR_UNSUPPORTED");assert_eq!(serde_json::to_value(&doc).unwrap(),before);
    if let Geometry::Text{layout,..}=&mut doc.objects[0].geometry {layout.letter_spacing_mm=0.;layout.line_height_mm=Some(0.000001);}
    assert!(cdr::encode_document(&doc).is_err());
}
#[test]
fn artistic_multiline_tracking_and_absolute_leading_survive_each_empty_paragraph() {
    for spacing in [0.,0.3,-0.1] {for leading in [None,Some(8.)] {
        let mut doc=Document::new("point".into(),210.,297.).unwrap();let mut o=area();o.transform=[-0.9,0.2,0.3,1.1,120.,-8.];o.id="point".into();
        if let Geometry::Text{content,layout,..}=&mut o.geometry {*content="\nAB 中文\n\nLast\n".into();*layout=TextLayoutOptions{mode:TextMode::Point,align:TextAlign::Left,letter_spacing_mm:spacing,line_height_mm:leading};}
        doc.objects.push(o);let raw=cdr::encode_document(&doc).unwrap().0;let imported=cdr::decode(&raw,"copy").unwrap().0;
        let Geometry::Text{content,layout,..}=&imported.objects[0].geometry else{panic!()};assert_eq!(content,"\nAB 中文\n\nLast\n");assert_eq!(layout.mode,TextMode::Point);assert_eq!(layout.letter_spacing_mm,spacing);assert_eq!(layout.line_height_mm,leading);
        let again=cdr::decode(&cdr::encode_document(&imported).unwrap().0,"copy").unwrap().0;assert_eq!(again,imported);
        let before=arranger_core::text::layout(&doc,&doc.objects[0]).unwrap();let after=arranger_core::text::layout(&imported,&imported.objects[0]).unwrap();assert_eq!(before.lines.len(),5);assert_eq!(after.lines.len(),5);assert_eq!(before.glyphs.len(),after.glyphs.len());for(a,b)in before.glyphs.iter().zip(after.glyphs.iter()){for(x,y)in a.origin_mm.iter().zip(b.origin_mm){near(*x,y);}}
    }}
}
#[test]
fn actual_corel_twenty_percent_artistic_tracking_is_not_silently_dropped() {
    for name in ["identity-v14.cdr","identity-v14-edited.cdr"] {
        let file=root().join("reports/windows/WIN-CDR-SAVE-20261022-01/focused/native-preservation").join(name);let raw=fs::read(file).unwrap();let doc=cdr::decode(&raw,"actual").unwrap().0;let o=doc.object("arr-stable-name").unwrap();
        let Geometry::Text{layout,..}=&o.geometry else{panic!()};let mut space=o.clone();if let Geometry::Text{content,layout,..}=&mut space.geometry {*content=" ".into();*layout=Default::default();}
        let advance=arranger_core::text::layout(&doc,&space).unwrap().advance_mm;near(layout.letter_spacing_mm,advance*0.2);assert!(layout.letter_spacing_mm>0.);
        let again=cdr::decode(&cdr::encode_document(&doc).unwrap().0,"again").unwrap().0;let Geometry::Text{layout:after,..}=&again.object("arr-stable-name").unwrap().geometry else{panic!()};near(after.letter_spacing_mm,layout.letter_spacing_mm);
    }
}
#[test]
fn negative_point_tracking_uses_encoded_typography_for_stable_native_bounds() {
    let mut d=Document::new("negative".into(),210.,297.).unwrap();let mut o=area();o.x_mm=20.;o.y_mm=35.;o.width_mm=100.;o.height_mm=10.;
    if let Geometry::Text{content,font_family,layout,..}=&mut o.geometry {*content="Native point 42\n\nThird line\n".into();*font_family="Arial".into();*layout=TextLayoutOptions{mode:TextMode::Point,align:TextAlign::Left,letter_spacing_mm:-0.08,line_height_mm:Some(7.5)};}
    d.objects.push(o);let a=cdr::decode(&cdr::encode_document(&d).unwrap().0,"same").unwrap().0;let b=cdr::decode(&cdr::encode_document(&a).unwrap().0,"same").unwrap().0;assert_eq!(a,b);
}
