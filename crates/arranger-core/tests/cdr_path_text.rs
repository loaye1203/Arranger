use arranger_core::{cdr,Geometry,TextAnchor};
use kurbo::ParamCurveArclen;
use std::path::PathBuf;
fn folder()->PathBuf{PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reports/windows/WIN-SAVE-CLOSURE-20261024-01/controls")}
#[test]
fn twenty_actual_corel_path_stories_import_live_links_offsets_and_anchors() {
    for version in [14,21] {for kind in ["control","offset","distance","center","right"] {for edit in ["","-edited"] {
        let file=folder().join(format!("path-{kind}-v{version}{edit}.cdr"));let bytes=std::fs::read(&file).unwrap();
        let (doc,_)=cdr::decode(&bytes,"path").unwrap_or_else(|e|panic!("{}: {e:?}",file.display()));
        let text=doc.object("control-text").unwrap();let guide=doc.object("control-guide").unwrap();
        let Geometry::Text{content,text_path:Some(binding),..}=&text.geometry else{panic!("native live relation lost")};
        assert_eq!(content,if edit.is_empty(){"沿路径 / Vector"}else{"沿路径 / Vector 已编辑"});
        assert_eq!(binding.path_id,"control-guide");assert_eq!(text.transform,[1.,0.,0.,1.,0.,0.]);
        let path=doc.world_transform(guide).unwrap()*guide.local_path().unwrap();let len:f64=path.segments().map(|s|s.arclen(1e-6)).sum();
        let (anchor,start)=match kind {"center"=>(TextAnchor::Middle,len/2.),"right"=>(TextAnchor::End,len),"offset"=>(TextAnchor::Start,7.5),_=>(TextAnchor::Start,0.)};
        assert_eq!(binding.anchor,anchor);assert!((binding.start_offset_mm-start).abs()<1e-6);assert_eq!(binding.baseline_offset_mm,if kind=="distance"{2.}else{0.});
        doc.validate().unwrap();let restored:arranger_core::Document=serde_json::from_value(serde_json::to_value(&doc).unwrap()).unwrap();assert_eq!(restored,doc);
        for output_version in [14,21] {
            let raw=if output_version==14 {cdr::encode_document(&doc)}else{cdr::encode_document21(&doc)}.unwrap().0;
            let written=cdr::decode(&raw,"written").unwrap().0;
            let Geometry::Text{content:actual,text_path:Some(path),..}=&written.object("control-text").unwrap().geometry else{panic!("written path relation lost")};
            assert_eq!(actual,content);assert_eq!(path.path_id,binding.path_id);assert_eq!(path.anchor,binding.anchor);
            assert!((path.start_offset_mm-binding.start_offset_mm).abs()<0.0001);assert_eq!(path.baseline_offset_mm,binding.baseline_offset_mm);
            let again=if output_version==14 {cdr::encode_document(&written)}else{cdr::encode_document21(&written)}.unwrap().0;
            let next=cdr::decode(&again,"written").unwrap().0;
            assert_eq!(next.object("control-text").unwrap().geometry,written.object("control-text").unwrap().geometry);
        }
        assert_eq!(std::fs::read(file).unwrap(),bytes);
    }}}
}

#[test]
fn four_legacy_path_defaults_with_unmeasured_distance_mode_stay_atomic_rejections() {
    let root=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reports/windows/WIN-CDR-SAVE-20261022-01/focused/native-preservation");
    for version in [14,21] {for edit in ["","-edited"] {
        let source=std::fs::read(root.join(format!("path-text-v{version}{edit}.cdr"))).unwrap();
        assert_eq!(cdr::decode(&source,"legacy").unwrap_err().code,"CDR_UNSUPPORTED");
    }}
}
