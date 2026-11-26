use arranger_core::{Command, Error, Geometry, Object, Operation, Page, Service, Style, TextAnchor, TextOnPath,
    command::ObjectPatch, native, render::{self, Renderer}, text};
use kurbo::{BezPath, Shape};
use resvg::usvg::fontdb;
use serde_json::{Value,json};
use std::{fs,path::PathBuf,sync::atomic::{AtomicU64,Ordering}};

fn new() -> Service {
    let s=Service::new();s.command(Command::DocumentNew{document_id:"main".into(),width_mm:300.,height_mm:200.,unit:"mm".into()}).unwrap();s
}
fn object(id:&str,geometry:Geometry)->Object {
    Object{id:id.into(),page_id:"page-1".into(),parent_id:None,x_mm:0.,y_mm:0.,width_mm:200.,height_mm:20.,transform:[1.,0.,0.,1.,0.,0.],style:Style::default(),visible:true,locked:false,clip:None,geometry}
}
fn label(id:&str,content:&str,family:&str,size:f64)->Object {
    object(id,Geometry::Text{content:content.into(),font_family:family.into(),font_size_mm:size,text_path:None,layout:Default::default()})
}
fn path(id:&str,d:&str)->Object {object(id,Geometry::Path{d:d.into(),fill_rule:Default::default()})}
fn create(object:Object)->Operation {Operation::ObjectCreate{object}}
fn batch(s:&Service,operations:Vec<Operation>)->Result<Value,Error> {
    s.command(Command::Batch{document_id:"main".into(),expected_revision:s.snapshot("main")?.revision,unit:"mm".into(),operations})
}
fn measure(s:&Service,id:&str)->Value {s.command(Command::TextMeasure{document_id:"main".into(),object_id:id.into()}).unwrap()}
fn binding(path_id:&str,start:f64)->TextOnPath {TextOnPath{path_id:path_id.into(),start_offset_mm:start,baseline_offset_mm:0.,anchor:TextAnchor::Start}}
fn set_path(id:&str,binding:Option<TextOnPath>)->Operation {Operation::TextSetPath{object_id:id.into(),text_path:binding}}
fn temp()->PathBuf {
    static NEXT:AtomicU64=AtomicU64::new(1);let root=PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_owned();
    let path=root.join(format!("artifacts/core-tests/text-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));fs::create_dir_all(&path).unwrap();path
}
fn pixels(s:&Service)->Vec<u8> {Renderer::new().render(&s.snapshot("main").unwrap().document,"page-1",144.,None).unwrap().pixels.data().to_vec()}
fn near(a:f64,b:f64){assert!((a-b).abs()<1e-4,"{a} != {b}");}

#[test]
fn mixed_measurement_reports_real_glyphs_ink_and_fonts_not_declared_frame() {
    let s=new();batch(&s,vec![create(label("mixed","中文 Arranger AV fi e\u{301}","Arial",8.))]).unwrap();
    let before=s.snapshot("main").unwrap();let m=measure(&s,"mixed");
    assert!(m["advance_mm"].as_f64().unwrap()>40.);assert!(m["advance_mm"].as_f64().unwrap()<130.);
    assert!(m["ink_bounds_local_mm"][2].as_f64().unwrap()>40.);assert_ne!(m["bounds_mm"][2],json!(200.));
    let glyphs=m["glyphs"].as_array().unwrap();assert!(glyphs.len()>10);assert!(glyphs.iter().all(|g|g["glyph_id"].as_u64().unwrap()>0));
    assert!(glyphs.iter().any(|g|g["cluster_text"].as_str().unwrap().contains('中')));
    assert!(!m["fonts"].as_array().unwrap().is_empty());
    assert!(m["fonts"].as_array().unwrap().iter().all(|f|f["sha256"].as_str().unwrap().len()==64));
    assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
    assert_eq!(s.snapshot("main").unwrap().revision,before.revision);
    let info=s.command(Command::ObjectGet{document_id:"main".into(),object_id:"mixed".into()}).unwrap();
    assert_eq!(info["bounds_mm"],m["bounds_mm"]);
    println!("mixed: advance={} mm, ink={}, glyphs={}, fonts={}",m["advance_mm"],m["ink_bounds_local_mm"],glyphs.len(),m["fonts"]);
}

#[test]
fn font_size_kerning_combining_marks_and_whitespace_are_measured() {
    // A generic "monospace" request may substitute a proportional face when
    // the platform has no font matching fontdb's default generic-family name.
    let fonts=text::font_database();
    let mono=fonts.faces().filter(|face|face.monospaced).find_map(|face| {
        let family=&face.families.first()?.0;
        let id=fonts.query(&fontdb::Query{families:&[fontdb::Family::Name(family)],..Default::default()})?;
        fonts.face(id).filter(|resolved|resolved.monospaced).map(|resolved|(family.clone(),resolved.post_script_name.clone()))
    }).expect("The whitespace regression requires an installed monospaced font");
    let s=new();batch(&s,vec![create(label("a","AV 中文","Arial",8.)),create(label("b","AV 中文","Arial",16.)),create(label("pair","AV","Arial",8.)),
        create(label("single-a","A","Arial",8.)),create(label("single-v","V","Arial",8.)),create(label("mark","e\u{301}","Arial",8.)),
        create(label("spaces","   ","Arial",8.)),create(label("one-space"," ","Arial",8.)),create(label("empty","","Arial",8.)),
        create(label("mono-space"," ",&mono.0,8.)),create(label("mono-m","M",&mono.0,8.))]).unwrap();
    near(measure(&s,"b")["advance_mm"].as_f64().unwrap(),2.*measure(&s,"a")["advance_mm"].as_f64().unwrap());
    assert!(measure(&s,"pair")["advance_mm"].as_f64().unwrap()<measure(&s,"single-a")["advance_mm"].as_f64().unwrap()+measure(&s,"single-v")["advance_mm"].as_f64().unwrap());
    assert_eq!(measure(&s,"mark")["placed_glyph_count"],1);
    let spaces=measure(&s,"spaces");let one=measure(&s,"one-space");
    assert!(spaces["advance_mm"].as_f64().unwrap()>0.);near(spaces["advance_mm"].as_f64().unwrap(),3.*one["advance_mm"].as_f64().unwrap());
    // Proportional fonts can kern A+space and space+A; use the actual equal-
    // advance invariant of a monospaced face for an independent marker check.
    let mono_space=measure(&s,"mono-space");let mono_m=measure(&s,"mono-m");
    assert_eq!(mono_space["fonts"],mono_m["fonts"]);
    assert_eq!(mono_m["fonts"][0]["post_script_name"],mono.1);
    near(mono_space["advance_mm"].as_f64().unwrap(),mono_m["advance_mm"].as_f64().unwrap());
    assert!(spaces["ink_bounds_local_mm"].is_null());assert_eq!(spaces["placed_glyph_count"],3);
    assert_eq!(measure(&s,"empty")["advance_mm"],0.);
    batch(&s,vec![Operation::TextToPath{object_id:"spaces".into()},Operation::TextToPath{object_id:"empty".into()}]).unwrap();
    assert!(s.snapshot("main").unwrap().document.object("spaces").unwrap().local_path().unwrap().elements().is_empty());
}

#[test]
fn mixed_text_outlines_preserve_visuals_transforms_style_and_whole_history() {
    let s=new();let mut text=label("mixed","中文 A&B <vector> fi","Arial",10.);text.x_mm=30.25;text.y_mm=35.;
    text.transform=[1.,0.1,0.2,1.,0.,0.];text.style.fill="#bd487f".into();text.style.stroke="#562244".into();text.style.stroke_width_mm=0.2;
    batch(&s,vec![create(text)]).unwrap();let before=s.snapshot("main").unwrap();let measurement=measure(&s,"mixed");let image=pixels(&s);
    let result=batch(&s,vec![Operation::TextToPath{object_id:"mixed".into()}]).unwrap();
    assert!(result["warnings"].is_array());
    let after=s.snapshot("main").unwrap();let outlined=after.document.object("mixed").unwrap();
    assert!(matches!(outlined.geometry,Geometry::Path{..}));assert_eq!(outlined.transform,before.document.object("mixed").unwrap().transform);
    assert_eq!(outlined.style,before.document.object("mixed").unwrap().style);assert_eq!(outlined.x_mm,30.25);
    assert!(image==pixels(&s),"Source text and its outlines must use exactly the same placed glyph contours");
    for (a,b) in after.document.bounds(outlined).unwrap().into_iter().zip(measurement["bounds_mm"].as_array().unwrap()) {near(a,b.as_f64().unwrap());}
    assert_eq!(after.undo_depth,before.undo_depth+1);
    s.command(Command::Undo{document_id:"main".into(),expected_revision:after.revision}).unwrap();assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
    s.command(Command::Redo{document_id:"main".into(),expected_revision:after.revision+1}).unwrap();assert_eq!(s.snapshot("main").unwrap().document_hash,after.document_hash);
}

#[test]
fn path_text_tracks_guide_parent_transform_and_own_drag_offset() {
    let s=new();let mut group=object("group",Geometry::Group);group.transform=[2.,0.,0.,2.,20.,10.];
    let mut line=path("guide","M10 20L110 20");line.parent_id=Some("group".into());
    batch(&s,vec![create(group),create(line),create(label("text","中文 AB","Microsoft YaHei",8.)),set_path("text",Some(binding("guide",10.)))]).unwrap();
    let m=measure(&s,"text");near(m["path_length_mm"].as_f64().unwrap(),200.);
    let x0=m["glyphs"][0]["origin_mm"][0].as_f64().unwrap();
    assert!((x0-50.).abs()<0.5);near(m["glyphs"][0]["origin_mm"][1].as_f64().unwrap(),50.);
    assert!(m["glyphs"].as_array().unwrap().iter().all(|g|g["angle_degrees"].as_f64().unwrap().abs()<1e-4));
    let mut end=binding("guide",80.);end.anchor=TextAnchor::End;end.baseline_offset_mm=3.;
    batch(&s,vec![set_path("text",Some(end))]).unwrap();let shifted=measure(&s,"text");
    near(shifted["glyphs"][0]["origin_mm"][0].as_f64().unwrap()-x0,70.-m["advance_mm"].as_f64().unwrap());
    assert!(shifted["glyphs"].as_array().unwrap().iter().all(|g|(g["origin_mm"][1].as_f64().unwrap()-53.).abs()<1e-4));
    batch(&s,vec![set_path("text",Some(binding("guide",10.)))]).unwrap();
    batch(&s,vec![Operation::ObjectUpdate{object_id:"guide".into(),patch:ObjectPatch{x_mm:Some(11.),..Default::default()}},
        Operation::ObjectUpdate{object_id:"text".into(),patch:ObjectPatch{x_mm:Some(5.),y_mm:Some(4.),..Default::default()}}]).unwrap();
    let changed=measure(&s,"text");near(changed["glyphs"][0]["origin_mm"][0].as_f64().unwrap()-x0,22.);
    let b=changed["bounds_mm"].as_array().unwrap();let a=m["bounds_mm"].as_array().unwrap();near(b[0].as_f64().unwrap()-a[0].as_f64().unwrap(),27.);near(b[1].as_f64().unwrap()-a[1].as_f64().unwrap(),4.);
    batch(&s,vec![set_path("text",None)]).unwrap();assert!(measure(&s,"text")["path_length_mm"].is_null());
}

#[test]
fn circle_mixed_text_anchor_normal_offset_conversion_and_native_roundtrip() {
    let s=new();let mut circle=object("circle",Geometry::Ellipse);circle.width_mm=150.;circle.height_mm=150.;circle.x_mm=60.;circle.y_mm=20.;
    let mut on_circle=binding("circle",100.);on_circle.anchor=TextAnchor::Middle;on_circle.baseline_offset_mm=-2.;
    batch(&s,vec![create(circle),Operation::ObjectToPath{object_id:"circle".into()},create(label("title","Arranger 中英文混排 fi","Arial",9.)),set_path("title",Some(on_circle))]).unwrap();
    let original=s.snapshot("main").unwrap();let m=measure(&s,"title");assert!(m["path_length_mm"].as_f64().unwrap()>470.);
    let angles:Vec<_>=m["glyphs"].as_array().unwrap().iter().map(|g|g["angle_degrees"].as_f64().unwrap()).collect();
    assert!(angles.windows(2).any(|v|(v[0]-v[1]).abs()>1.));assert!(!m["overflow"].as_bool().unwrap());
    let dir=temp();let native=dir.join("混排路径.arranger");
    s.command(Command::Save{document_id:"main".into(),expected_revision:original.revision,path:native.to_string_lossy().into_owned(),overwrite:false}).unwrap();
    let loaded=native::open(&native).unwrap();assert_eq!(*original.document,loaded.document);
    let svg=render::svg(&loaded.document,"page-1").unwrap();assert!(svg.contains("data-arranger-text-path=\"true\""));assert!(svg.contains("Arranger 中英文混排 fi"));
    Renderer::new().render(&loaded.document,"page-1",144.,None).unwrap().pixels.save_png(dir.join("before.png")).unwrap();
    let image=pixels(&s);batch(&s,vec![Operation::TextToPath{object_id:"title".into()}]).unwrap();
    Renderer::new().render(&s.snapshot("main").unwrap().document,"page-1",144.,None).unwrap().pixels.save_png(dir.join("after.png")).unwrap();
    let after=pixels(&s);
    let changed=image.iter().zip(&after).filter(|(a,b)|a!=b).count();
    let max_delta=image.iter().zip(&after).map(|(a,b)|a.abs_diff(*b)).max().unwrap();
    println!("circle pixel changed channels: {changed}, max delta: {max_delta}");
    assert!(image==after,"circle conversion changed {changed} channels, max delta {max_delta}");assert!(!render::svg(&s.snapshot("main").unwrap().document,"page-1").unwrap().contains("data-arranger-text-path"));
    let info=s.command(Command::PathGet{document_id:"main".into(),object_id:"title".into()}).unwrap();assert!(info["elements"].as_array().unwrap().len()>100);
    fs::write(dir.join("editable.svg"),svg).unwrap();
}

#[test]
fn overflowing_guide_or_text_edit_rolls_back_all_staged_work() {
    let s=new();batch(&s,vec![create(path("guide","M0 60L200 60")),create(label("text","中文 Arranger","Arial",10.)),set_path("text",Some(binding("guide",10.)))]).unwrap();
    let before=s.snapshot("main").unwrap();
    for operation in [
        Operation::PathNodeUpdate{object_id:"guide".into(),index:1,element:arranger_core::PathElement::LineTo{to:[12.,60.]}},
        set_path("text",Some(binding("guide",195.))),
        Operation::ObjectUpdate{object_id:"text".into(),patch:ObjectPatch{geometry:Some(Geometry::Text{content:"中文 Arranger".repeat(20),font_family:"Arial".into(),font_size_mm:10.,text_path:Some(binding("guide",10.)),layout:Default::default()}),..Default::default()}},
    ] {
        let err=batch(&s,vec![Operation::ObjectUpdate{object_id:"guide".into(),patch:ObjectPatch{x_mm:Some(5.),..Default::default()}},operation]).unwrap_err();
        assert_eq!(err.code,"TEXT_PATH_OVERFLOW");assert_eq!(err.failed_operation,Some(2));assert_eq!(err.rolled_back,Some(true));
        assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
    }
}

#[test]
fn guide_reference_locks_invalid_ranges_and_cycles_never_publish_partial_edits() {
    let s=new();let mut group=object("group",Geometry::Group);group.width_mm=0.;group.height_mm=0.;
    let mut other=path("cross-page","M0 0L100 0");other.page_id="page-2".into();
    batch(&s,vec![create(group),create(path("guide","M0 50L200 50")),create(path("compound","M0 0L10 0 M20 0L100 0")),create(path("empty","")),
        create(label("text","ABC 中文","Arial",8.)),Operation::PageAdd{page:Page{id:"page-2".into(),width_mm:300.,height_mm:200.}},create(other)]).unwrap();
    for (op,code) in [
        (set_path("guide",Some(binding("guide",0.))),"NOT_TEXT"),(set_path("text",Some(binding("group",0.))),"INVALID_TEXT_PATH"),
        (set_path("text",Some(binding("empty",0.))),"INVALID_TEXT_PATH"),(set_path("text",Some(binding("compound",0.))),"INVALID_TEXT_PATH"),
        (set_path("text",Some(binding("cross-page",0.))),"CROSS_PAGE"),(set_path("text",Some(binding("missing",0.))),"OBJECT_NOT_FOUND"),
        (set_path("text",Some(binding("guide",f64::NAN))),"INVALID_TEXT_PATH"),
        (Operation::ObjectUpdate{object_id:"text".into(),patch:ObjectPatch{geometry:Some(Geometry::Text{content:"test".into(),font_family:"Arial".into(),font_size_mm:1e100,text_path:None,layout:Default::default()}),..Default::default()}},"TEXT_RANGE"),
        (Operation::ObjectReparent{object_id:"group".into(),parent_id:Some("group".into())},"CYCLIC_GROUP"),
    ] {
        let before=s.snapshot("main").unwrap();let e=batch(&s,vec![op]).unwrap_err();assert_eq!(e.code,code);assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
    }
    batch(&s,vec![Operation::ObjectLock{object_id:"guide".into(),locked:true},set_path("text",Some(binding("guide",0.)))]).unwrap(); // Reading a locked guide is allowed.
    let before=s.snapshot("main").unwrap();let e=batch(&s,vec![Operation::ObjectDelete{object_id:"guide".into()}]).unwrap_err();assert_eq!(e.code,"OBJECT_LOCKED");assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
    batch(&s,vec![Operation::ObjectLock{object_id:"guide".into(),locked:false}]).unwrap();let before=s.snapshot("main").unwrap();
    assert_eq!(batch(&s,vec![Operation::ObjectDelete{object_id:"guide".into()}]).unwrap_err().code,"OBJECT_NOT_FOUND");assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
    batch(&s,vec![Operation::ObjectLock{object_id:"text".into(),locked:true}]).unwrap();assert_eq!(batch(&s,vec![Operation::TextToPath{object_id:"text".into()}]).unwrap_err().code,"OBJECT_LOCKED");
}

#[test]
fn substitution_is_reported_and_missing_glyph_conversion_is_atomic() {
    let s=new();batch(&s,vec![create(label("fallback","中文 ABC","Arranger-Nonexistent-Font",8.)),create(label("missing","A\u{10ffff}B","Arial",8.))]).unwrap();
    let m=measure(&s,"fallback");assert!(m["warnings"].as_array().unwrap().iter().any(|w|w["code"]=="FONT_SUBSTITUTED"));assert!(m["fonts"].as_array().unwrap().len()>=1);
    let result=batch(&s,vec![Operation::TextToPath{object_id:"fallback".into()}]).unwrap();assert!(result["warnings"].as_array().unwrap().iter().any(|w|w["code"]=="FONT_SUBSTITUTED"));
    let m=measure(&s,"missing");assert!(m["warnings"].as_array().unwrap().iter().any(|w|w["code"]=="MISSING_GLYPH"));
    let before=s.snapshot("main").unwrap();let err=batch(&s,vec![Operation::ObjectUpdate{object_id:"fallback".into(),patch:ObjectPatch{x_mm:Some(30.),..Default::default()}},Operation::TextToPath{object_id:"missing".into()}]).unwrap_err();
    assert_eq!(err.code,"MISSING_GLYPH");assert_eq!(err.failed_operation,Some(2));assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
}

#[test]
fn explicit_mixed_text_task_cancel_and_success_are_one_transaction() {
    let s=new();batch(&s,vec![create(path("guide","M0 90C0 0 200 0 200 90")),create(label("text","中英文 Arranger","Arial",8.))]).unwrap();let before=s.snapshot("main").unwrap();
    for commit in [false,true] {
        let token=s.command(Command::Begin{document_id:"main".into(),expected_revision:before.revision,unit:"mm".into(),lease_ms:30000}).unwrap()["transaction_id"].as_str().unwrap().to_owned();
        for operation in [set_path("text",Some(binding("guide",10.))),Operation::TextToPath{object_id:"text".into()}] {
            s.command(Command::Execute{document_id:"main".into(),transaction_id:token.clone(),operation}).unwrap();
        }
        assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
        assert!(measure(&s,"text")["path_length_mm"].is_null()); // Readers never see staged text.
        s.command(if commit {Command::Commit{document_id:"main".into(),transaction_id:token}} else {Command::Rollback{document_id:"main".into(),transaction_id:token}}).unwrap();
        if !commit {assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);}
    }
    let after=s.snapshot("main").unwrap();assert_eq!(after.undo_depth,before.undo_depth+1);s.command(Command::Undo{document_id:"main".into(),expected_revision:after.revision}).unwrap();assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
}

#[test]
fn previous_native_text_checksums_and_outline_glyph_positions_are_reusable() {
    let root=PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_owned();
    let loaded=native::open(&root.join("crates/arranger-core/tests/fixtures/v1-minimum-legacy.arranger")).unwrap();
    let text=loaded.document.objects.iter().find(|o|matches!(o.geometry,Geometry::Text{..})).unwrap();
    assert!(serde_json::to_value(&text.geometry).unwrap().get("text_path").is_none());
    let layout=text::layout(&loaded.document,text).unwrap();assert!(layout.glyphs.len()>0);
    let b=layout.outline.bounding_box();assert!(b.width()>0. && b.height()>0.);
    let normalized=BezPath::from_svg(&layout.outline.to_svg()).unwrap();assert_eq!(normalized,layout.outline);
}

#[test]
fn malformed_text_path_payload_rolls_back_its_owned_transaction() {
    let s=new();batch(&s,vec![create(label("text","中文 Test","Arial",8.))]).unwrap();let before=s.snapshot("main").unwrap();
    let token=s.command(Command::Begin{document_id:"main".into(),expected_revision:before.revision,unit:"mm".into(),lease_ms:30000}).unwrap()["transaction_id"].as_str().unwrap().to_owned();
    s.command(Command::Execute{document_id:"main".into(),transaction_id:token.clone(),operation:Operation::ObjectUpdate{object_id:"text".into(),patch:ObjectPatch{x_mm:Some(99.),..Default::default()}}}).unwrap();
    let bad=s.execute_json(&json!({"protocol_version":1,"command":{"type":"execute","document_id":"main","transaction_id":token,"operation":{"type":"text_set_path","object_id":"text"}}}).to_string());
    let error=bad.error.unwrap();assert_eq!(error.code,"INVALID_REQUEST");assert_eq!(error.failed_operation,Some(2));assert_eq!(error.rolled_back,Some(true));
    assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);assert!(!s.snapshot("main").unwrap().busy);
}

#[test]
#[cfg(target_os="windows")]
fn color_glyphs_are_reported_and_never_silently_flattened_to_monochrome() {
    let s=new();batch(&s,vec![create(label("emoji","😀","Segoe UI Emoji",12.))]).unwrap();
    let m=measure(&s,"emoji");assert_eq!(m["outline_supported"],false,"This Windows regression requires the system Segoe UI Emoji color face");
    assert!(m["warnings"].as_array().unwrap().iter().any(|n|n["code"]=="UNSUPPORTED_GLYPH_OUTLINE"));
    let before=s.snapshot("main").unwrap();let e=batch(&s,vec![Operation::TextToPath{object_id:"emoji".into()}]).unwrap_err();assert_eq!(e.code,"UNSUPPORTED_GLYPH_OUTLINE");assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
}

#[test]
fn caret_advances_match_independently_laid_out_prefixes() {
    let doc = arranger_core::Document::new("carets".into(), 210., 297.).unwrap();
    let mut o = label("story", "", "sans-serif", 4.);
    for spacing in [0., 0.35, -0.1] {
        for story in [" \t  ", "AV office e\u{301} 中文", "العربية", "排版\r\n"] {
            if let Geometry::Text { layout, .. } = &mut o.geometry { layout.letter_spacing_mm = spacing; }
            let actual = text::caret_advances(&doc, &o, story, 0).map_err(|e|e.code);
            let expected: Result<Vec<_>, _> = story.char_indices().map(|(index, ch)| {
                if let Geometry::Text { content, .. } = &mut o.geometry {
                    *content = story[..index+ch.len_utf8()].into();
                }
                text::layout(&doc, &o).map(|l|l.advance_mm).map_err(|e|e.code)
            }).collect();
            assert_eq!(actual, expected, "{story:?} {spacing}");
        }
    }
}

#[test]
fn long_line_caret_positions_match_independent_prefixes_and_incremental_edits() {
    let doc = arranger_core::Document::new("long-carets".into(), 210., 297.).unwrap();
    let mut object = label("story", "", "sans-serif", 4.);
    let story = "AV office e\u{301} 中文 אבג العربية &<> ".repeat(12);
    let widths = text::caret_advances(&doc, &object, &story, 0).unwrap();
    let boundaries: Vec<_> = story.char_indices().map(|(i,c)| i+c.len_utf8()).collect();
    assert_eq!(widths.len(), boundaries.len());
    for (i, &end) in boundaries.iter().enumerate() {
        if let Geometry::Text { content, .. } = &mut object.geometry {
            story[..end].clone_into(content);
        }
        assert_eq!(widths[i], text::layout(&doc, &object).unwrap().advance_mm, "prefix {i}");
    }
    for i in [1, 73, boundaries.len()-2, boundaries.len()-1] {
        assert_eq!(text::caret_advances(&doc, &object, &story, boundaries[i]).unwrap(), widths[i+1..]);
    }
}

#[test]
fn stable_breaks_preserve_each_prefix_across_fonts_and_fallback_changes() {
    let doc = arranger_core::Document::new("prefixes".into(), 210., 297.).unwrap();
    for (pattern, font) in [
        ("曲线文字 AV fi e\u{301} 中英排版 0123 ", "sans-serif"),
        ("AV office ffi To a\u{301} &< >   XYZ ", "serif"),
        ("VA ffi Ae\u{301} ", "sans-serif"),
        ("  （图形 21）曲线，字号：16；间距《字样》。", "sans-serif"),
        ("  0123 45: AV office é ", "serif"),
        ("字样标注  Abc 中英 ", "font not installed 207"),
    ] {
        let story = if pattern.starts_with("VA") {
            pattern.repeat(14) + &"新增文字 AV ".repeat(15)
        } else {
            pattern.repeat(22)
        };
        let mut object = label("story", "", font, 3.5);
        let widths = text::caret_advances(&doc, &object, &story, 0).unwrap();
        let mut boundaries = Vec::new();
        for ((offset, ch), &width) in story.char_indices().zip(&widths) {
            let end = offset + ch.len_utf8();
            boundaries.push(end);
            if let Geometry::Text {content, ..} = &mut object.geometry {
                story[..end].clone_into(content);
            }
            assert_eq!(width, text::layout(&doc, &object).unwrap().advance_mm, "{font} {end}");
        }
        assert_eq!(widths.len(), story.chars().count());
        let cut = 16;
        assert_eq!(text::caret_advances(&doc, &object, &story, boundaries[cut]).unwrap(), widths[cut + 1..]);
    }
}

#[test]
fn partial_edits_keep_exact_carets_inside_ligatures_marks_and_kerning_pairs() {
    let doc = arranger_core::Document::new("joins".into(), 210., 297.).unwrap();
    let mut obj = label("line", "", "sans-serif", 3.37);
    let mut story = (0..28).map(|n| format!("厂{n} AV ffi e\u{301} ")).collect::<String>();
    let edit = story.char_indices().nth(126).unwrap().0;
    for replacement in ["铭", "f", "e\u{301}", "AV", ""] {
        story.insert_str(edit, replacement);
        let tail = text::caret_advances(&doc, &obj, &story, edit).unwrap();
        let full = text::caret_advances(&doc, &obj, &story, 0).unwrap();
        assert_eq!(tail, full[126..]);
        for remaining in [1, 2, 4, 8, 32, 128, 191, 192] {
            let start = full.len() - remaining;
            let from = story.char_indices().nth(start).unwrap().0;
            assert_eq!(text::caret_advances(&doc, &obj, &story, from).unwrap(), full[start..]);
        }
        for (width, (i, c)) in tail.iter().zip(story[edit..].char_indices()) {
            if let Geometry::Text { content, .. } = &mut obj.geometry {
                story[..edit + i + c.len_utf8()].clone_into(content);
            }
            assert_eq!(*width, text::layout(&doc, &obj).unwrap().advance_mm);
        }
        story.replace_range(edit..edit + replacement.len(), "");
    }
}
