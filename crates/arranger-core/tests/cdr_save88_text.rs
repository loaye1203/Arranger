use arranger_core::{cdr,Geometry};
#[test]
fn actual_corel21_empty_and_outlined_stories_import_without_losing_live_text(){
    for (name,raw) in [
        ("empty",include_bytes!("../../../examples/assets/cdr/corel2019/save88-text/empty-text-v21-corel-resaved.cdr").as_slice()),
        ("outline",include_bytes!("../../../examples/assets/cdr/corel2019/save88-text/outlined-text-v21-corel-resaved.cdr").as_slice()),
        ("path",include_bytes!("../../../examples/assets/cdr/corel2019/save88-text/similarity21-clip-outline-corel-resaved.cdr").as_slice()),
    ]{
        let doc=cdr::decode(raw,"actual").unwrap_or_else(|e|panic!("{name}: {e:?}")).0;
        let text=doc.object(if name=="path"{"text"}else{"content"}).unwrap();
        let Geometry::Text{content,text_path,..}=&text.geometry else{panic!("real story replaced")};
        if name=="empty"{assert!(content.is_empty());}else{assert!(!content.is_empty());assert_eq!(text.style.stroke,"#000000");assert!(text.style.stroke_width_mm>0.);}
        if name=="path"{assert_eq!(text_path.as_ref().unwrap().path_id,"guide");assert_eq!(text.parent_id.as_deref(),Some("parent"));assert!(text.clip.is_some());}
        doc.validate().unwrap();
    }
}
