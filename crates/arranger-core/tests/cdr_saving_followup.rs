use arranger_core::{Document,Geometry,Style};
use serde_json::json;
fn root()->std::path::PathBuf{std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")}
#[test]
fn genuine_resaved_overflow_and_multipage_ids_import_without_loosening_geometry() {
    let folder=root().join("reports/windows/WIN-SAVE-CLOSURE-20261024-01/paragraphs/native");
    for name in ["area-left","area-center","area-right","area-tracking","area-overflow","area-pages-workspace"] {
        let raw=std::fs::read(folder.join(format!("{name}-corel-resaved.cdr"))).unwrap();let doc=arranger_core::cdr::decode(&raw,name).unwrap().0;
        assert!(doc.objects.iter().any(|o|matches!(o.geometry,Geometry::Text{..})));
        let again=arranger_core::cdr::decode(&arranger_core::cdr::encode_document(&doc).unwrap().0,name).unwrap().0;assert_eq!(again,doc,"{name}");
    }
    let affine=std::fs::read(folder.join("area-affine-corel-resaved.cdr")).unwrap();let affine=arranger_core::cdr::decode(&affine,"affine-frame").unwrap().0;
    let text=affine.object("area-text").unwrap();assert_eq!((text.width_mm,text.height_mm),(65.,55.));
    for (value,want) in text.transform.into_iter().zip([-0.79,0.2,0.39,1.135,127.45,62.225]) {assert!((value-want).abs()<1e-10);}
}
#[test]
fn eight_genuine_uniform_transparency_files_keep_per_child_not_isolated_group_opacity() {
    let folder=root().join("reports/windows/WIN-SAVE-CLOSURE-20261024-01/controls");
    for version in [14,21] {for name in ["opacity-rect","opacity-group-overlap"] {for suffix in ["","-edited"] {
        let raw=std::fs::read(folder.join(format!("{name}-v{version}{suffix}.cdr"))).unwrap();let doc=arranger_core::cdr::decode(&raw,"opacity").unwrap().0;
        assert_eq!(doc.object("alpha-a").unwrap().style.opacity,0.7);
        if name=="opacity-group-overlap" {assert_eq!(doc.object("alpha-b").unwrap().style.opacity,0.7);assert_eq!(doc.object("alpha-group").unwrap().style.opacity,1.);}
        for output in [14,21] {let encoded=if output==14{arranger_core::cdr::encode_document(&doc)}else{arranger_core::cdr::encode_document21(&doc)}.unwrap().0;
            let restored=arranger_core::cdr::decode(&encoded,"opacity").unwrap().0;assert_eq!(restored,doc,"{name}/{version}/{output}/{suffix}");}
    }}}
}
#[test]
fn uniform_shape_opacity_roundtrips_and_unsupported_composition_is_explicit() {
    for opacity in [0.,0.25,0.5,0.7,0.75] {
        let mut doc=Document::new("opacity-model".into(),210.,297.).unwrap();
        for (i,geometry) in [json!({"kind":"rect","radius_mm":0.}),json!({"kind":"ellipse"}),json!({"kind":"path","d":"M0 0 L40 0 L40 20 L0 20 Z"})].into_iter().enumerate(){
            doc.objects.push(serde_json::from_value(json!({"id":format!("shape-{i}"),"page_id":"page-1","x_mm":20.,"y_mm":30.+i as f64*40.,"width_mm":40.,"height_mm":20.,"style":{"fill":"#1e5ab4","stroke":"none","stroke_width_mm":0.,"opacity":opacity},"geometry":geometry})).unwrap());
        }
        for version in [14,21] {let raw=if version==14{arranger_core::cdr::encode_document(&doc)}else{arranger_core::cdr::encode_document21(&doc)}.unwrap().0;let restored=arranger_core::cdr::decode(&raw,&doc.id).unwrap().0;
            for o in &restored.objects {assert_eq!(o.style.opacity,opacity);let a=doc.bounds(doc.object(&o.id).unwrap()).unwrap();let b=restored.bounds(o).unwrap();assert!(a.iter().zip(b).all(|(x,y)|(x-y).abs()<1e-8));}
            let again=if version==14{arranger_core::cdr::encode_document(&restored)}else{arranger_core::cdr::encode_document21(&restored)}.unwrap().0;assert_eq!(arranger_core::cdr::decode(&again,&doc.id).unwrap().0,restored);}
        doc.objects[0].style=Style{opacity:0.12345,..doc.objects[0].style.clone()};let raw=arranger_core::cdr::encode_document(&doc).unwrap().0;assert_eq!(arranger_core::cdr::decode(&raw,&doc.id).unwrap().0.objects[0].style.opacity,0.12345);
        doc.objects[0].style.opacity=opacity;doc.objects[0].style.stroke="#00000080".into();assert!(arranger_core::cdr::encode_document(&doc).is_err());
    }
}

#[test]
fn default_variable_and_cff_fonts_keep_declared_family_live_text_and_local_pixels() {
    use arranger_core::render::Renderer;
    let db=arranger_core::text::font_database();let mut local=0;
    for family in ["Bahnschrift","STIX Two Text","Skia","PingFang SC","Hiragino Sans","Source Han Sans JP"] {
        let content=if ["PingFang SC","Hiragino Sans","Source Han Sans JP"].contains(&family){"Native CFF 中文 ABC 42"}else{"Default variable ABC 42"};
        let mut doc=Document::new("native-font".into(),210.,297.).unwrap();
        doc.objects.push(serde_json::from_value(json!({"id":"text","page_id":"page-1","x_mm":20.,"y_mm":30.,"width_mm":100.,"height_mm":10.,"geometry":{"kind":"text","content":content,"font_family":family,"font_size_mm":6.35},"style":{"fill":"#123456","stroke":"none","stroke_width_mm":0.,"opacity":1.}})).unwrap());
        let installed=db.faces().any(|f|f.families.iter().any(|(name,_)|name==family));
        if family!="Bahnschrift"&&family!="Source Han Sans JP"&&!installed{continue;}if installed{local+=1;}
        let renderer=Renderer::new();let source=renderer.render(&doc,"page-1",72.,None).unwrap().pixels;
        for version in [14,21] {
            let raw=if version==14{arranger_core::cdr::encode_document(&doc)}else{arranger_core::cdr::encode_document21(&doc)}.unwrap().0;let copy=arranger_core::cdr::decode(&raw,&doc.id).unwrap().0;
            let Geometry::Text{font_family,content,..}=&copy.objects[0].geometry else{panic!("text flattened")};assert_eq!(font_family,family);assert_eq!(content,if ["PingFang SC","Hiragino Sans","Source Han Sans JP"].contains(&family){"Native CFF 中文 ABC 42"}else{"Default variable ABC 42"});
            assert_eq!(renderer.render(&copy,"page-1",72.,None).unwrap().pixels.data(),source.data());
        }
    }
    println!("VARIABLE_DEFAULT_RENDERED_LOCAL_FAMILIES={local}");
}
