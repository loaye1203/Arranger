use arranger_core::{Command, Geometry, Object, Operation, Service, Style, TextAlign, TextAnchor,
    TextLayoutOptions, TextMode, TextOnPath, command::ObjectPatch, native, render::{self,Renderer}};
use serde_json::Value;
use std::{fs,path::PathBuf,sync::atomic::{AtomicU64,Ordering}};
use unicode_segmentation::UnicodeSegmentation;

fn service()->Service {let s=Service::new();s.command(Command::DocumentNew{document_id:"main".into(),width_mm:300.,height_mm:200.,unit:"mm".into()}).unwrap();s}
fn label(id:&str,content:&str,area:bool)->Object {
    Object {id:id.into(),page_id:"page-1".into(),parent_id:None,x_mm:15.,y_mm:15.,width_mm:65.,height_mm:150.,transform:[1.,0.,0.,1.,0.,0.],style:Style::default(),visible:true,locked:false,clip:None,
        geometry:Geometry::Text {content:content.into(),font_family:"Microsoft YaHei".into(),font_size_mm:6.,text_path:None,
            layout:TextLayoutOptions{mode:if area {TextMode::Area} else {TextMode::Point},..Default::default()}}}
}
fn settings(o:&mut Object)->&mut TextLayoutOptions {match &mut o.geometry {Geometry::Text{layout,..}=>layout,_=>unreachable!()}}
fn batch(s:&Service,operations:Vec<Operation>)->arranger_core::Result<Value> {s.command(Command::Batch{document_id:"main".into(),expected_revision:s.snapshot("main")?.revision,unit:"mm".into(),operations})}
fn create(object:Object)->Operation {Operation::ObjectCreate{object}}
fn update(id:&str,patch:ObjectPatch)->Operation {Operation::ObjectUpdate{object_id:id.into(),patch}}
fn measure(s:&Service,id:&str)->Value {s.command(Command::TextMeasure{document_id:"main".into(),object_id:id.into()}).unwrap()}
fn near(a:f64,b:f64) {assert!((a-b).abs()<2e-4,"{a} != {b}");}
fn pixels(s:&Service)->Vec<u8> {Renderer::new().render(&s.snapshot("main").unwrap().document,"page-1",144.,None).unwrap().pixels.data().to_vec()}
fn temp()->PathBuf {static NEXT:AtomicU64=AtomicU64::new(1);let p=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../artifacts/core-tests/paragraph-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));fs::create_dir_all(&p).unwrap();p}

#[test]
fn hard_breaks_blank_lines_original_byte_ranges_and_unicode_are_preserved() {
    let s=service();let content="中文 AV\r\n\r\nEnglish e\u{301}\r尾行\u{2028}末行\n";
    batch(&s,vec![create(label("text",content,false))]).unwrap();let before=s.snapshot("main").unwrap();let m=measure(&s,"text");
    assert_eq!(m["line_count"],6);assert_eq!(m["lines"][1]["text"],"");assert_eq!(m["lines"][5]["text"],"");
    for line in m["lines"].as_array().unwrap() {
        let r=&line["source_range_bytes"];assert_eq!(&content[r[0].as_u64().unwrap() as usize..r[1].as_u64().unwrap() as usize],line["text"].as_str().unwrap());
    }
    assert!(m["fonts"].as_array().unwrap().len()>0);assert!(m["glyphs"].as_array().unwrap().iter().all(|g|g["glyph_id"].as_u64().unwrap()>0));
    assert!(!m["warnings"].as_array().unwrap().iter().any(|n|n["code"]=="TEXT_WHITESPACE_NORMALIZED"));
    assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
}

#[test]
fn mixed_area_wrap_uses_shaped_line_advance_and_preserves_source() {
    let s=service();let content="中文排版 Arranger supports English words 和中文自动换行。 AV fi e\u{301} 可编辑。";
    batch(&s,vec![create(label("text",content,true))]).unwrap();let m=measure(&s,"text");assert!(m["line_count"].as_u64().unwrap()>3);assert_eq!(m["overflow"],false);
    let mut next=0;
    for (i,line) in m["lines"].as_array().unwrap().iter().enumerate() {
        let range=&line["source_range_bytes"];assert_eq!(range[0],next);next=range[1].as_u64().unwrap();
        assert!(line["advance_mm"].as_f64().unwrap()<=65.+1e-4);
        let id=format!("line-{i}");batch(&s,vec![create(label(&id,line["text"].as_str().unwrap(),false))]).unwrap();
        near(measure(&s,&id)["advance_mm"].as_f64().unwrap(),line["advance_mm"].as_f64().unwrap());
    }
    assert_eq!(next as usize,content.len());let snap=s.snapshot("main").unwrap();
    assert_eq!(serde_json::to_value(&snap.document.object("text").unwrap().geometry).unwrap()["content"],content);
}

#[test]
fn left_center_right_alignment_uses_area_or_max_point_line_width() {
    let s=service();
    for area in [false,true] {for (i,align) in [TextAlign::Left,TextAlign::Center,TextAlign::Right].into_iter().enumerate() {
        let id=format!("text-{area}-{i}");let mut o=label(&id,"AB 中文\nX",area);settings(&mut o).align=align;batch(&s,vec![create(o)]).unwrap();let m=measure(&s,&id);
        let width=if area {65.} else {m["advance_mm"].as_f64().unwrap()};
        for line in m["lines"].as_array().unwrap() {let leftover=width-line["advance_mm"].as_f64().unwrap();let expected=match align {TextAlign::Left=>0.,TextAlign::Center=>leftover/2.,TextAlign::Right=>leftover};near(line["baseline_origin_mm"][0].as_f64().unwrap(),expected);}
    }}
}

#[test]
fn letter_spacing_changes_real_glyph_positions_spaces_and_wraps() {
    let s=service();
    for (id,spacing,content) in [("base",0.,"HHHH"),("spaced",1.25,"HHHH"),("tight",-0.1,"HHHH"),("spaces",1.25,"   "),("plain-spaces",0.,"   ")] {
        let mut o=label(id,content,false);settings(&mut o).letter_spacing_mm=spacing;batch(&s,vec![create(o)]).unwrap();
    }
    let a=measure(&s,"base");let b=measure(&s,"spaced");near(b["advance_mm"].as_f64().unwrap()-a["advance_mm"].as_f64().unwrap(),3.*1.25);
    for i in 0..4 {near(b["glyphs"][i]["origin_mm"][0].as_f64().unwrap()-a["glyphs"][i]["origin_mm"][0].as_f64().unwrap(),i as f64*1.25);}
    near(measure(&s,"tight")["advance_mm"].as_f64().unwrap()-a["advance_mm"].as_f64().unwrap(),-0.3);
    near(measure(&s,"spaces")["advance_mm"].as_f64().unwrap()-measure(&s,"plain-spaces")["advance_mm"].as_f64().unwrap(),2.*1.25);
    let mut area=label("area","中文中文中文中文中文中文",true);area.width_mm=30.;batch(&s,vec![create(area.clone())]).unwrap();let first=measure(&s,"area");
    settings(&mut area).letter_spacing_mm=3.;batch(&s,vec![update("area",ObjectPatch{geometry:Some(area.geometry),..Default::default()})]).unwrap();assert!(measure(&s,"area")["line_count"].as_u64().unwrap()>first["line_count"].as_u64().unwrap());
}

#[test]
fn line_height_is_baseline_distance_including_empty_lines() {
    let s=service();let mut o=label("text","中\n\nAB\n末",false);settings(&mut o).line_height_mm=Some(11.5);batch(&s,vec![create(o)]).unwrap();let m=measure(&s,"text");
    near(m["line_height_mm"].as_f64().unwrap(),11.5);
    for i in 0..4 {near(m["lines"][i]["baseline_origin_mm"][1].as_f64().unwrap(),6.+i as f64*11.5);}
    assert!(m["layout_bounds_local_mm"][3].as_f64().unwrap()>=46.);
}

#[test]
fn emergency_wrap_never_splits_combining_or_emoji_graphemes() {
    let s=service();
    for (family_index,family) in ["Microsoft YaHei","Arranger-Nonexistent-Font"].into_iter().enumerate() {
        for (id,content) in [("combining","e\u{301}e\u{301}e\u{301}"),("emoji","👩‍👩‍👧‍👦👩‍👩‍👧‍👦")] {
            let graphemes:Vec<_>=content.grapheme_indices(true).collect();
            let id=format!("{id}-{family_index}");let probe_id=format!("{id}-probe");
            let mut probe=label(&probe_id,graphemes[0].1,false);
            if let Geometry::Text{font_family,..}=&mut probe.geometry {*font_family=family.into();}
            batch(&s,vec![create(probe)]).unwrap();let natural=measure(&s,&probe_id);
            let advance=natural["advance_mm"].as_f64().unwrap();assert!(advance.is_finite() && advance>0.);
            // Force an emergency break using the actual shaped glyph width.
            // A fixed 9 mm frame fits all three combining graphemes on macOS.
            let mut o=label(&id,content,true);o.width_mm=advance/2.;
            if let Geometry::Text{font_family,..}=&mut o.geometry {*font_family=family.into();}
            batch(&s,vec![create(o)]).unwrap();let m=measure(&s,&id);
            assert_eq!(m["line_count"].as_u64().unwrap() as usize,graphemes.len(),"{family}: {m}");
            for (line,(start,grapheme)) in m["lines"].as_array().unwrap().iter().zip(graphemes) {
                assert_eq!(line["text"],grapheme);
                assert_eq!(line["source_range_bytes"],serde_json::json!([start,start+grapheme.len()]));
            }
            if family=="Arranger-Nonexistent-Font" {
                assert!(m["warnings"].as_array().unwrap().iter().any(|n|n["code"]=="FONT_SUBSTITUTED"));
            }
        }
    }
}

#[test]
fn basic_cjk_punctuation_and_english_words_stay_together_when_they_fit() {
    let s=service();let content="中文（排版），自动换行。 English words 中文。";
    let mut o=label("text",content,true);o.width_mm=27.;batch(&s,vec![create(o)]).unwrap();let m=measure(&s,"text");
    for line in m["lines"].as_array().unwrap() {let t=line["text"].as_str().unwrap();assert!(!t.ends_with('（'));assert!(!t.starts_with(['，','。','）']));}
    let words:Vec<_>=m["lines"].as_array().unwrap().iter().map(|l|l["text"].as_str().unwrap()).collect();assert!(words.iter().any(|s|s.contains("English")));assert!(words.iter().any(|s|s.contains("words")));
}

#[test]
fn repeated_area_paragraph_rebases_breaks_ranges_and_alignment() {
    let s = service();
    let paragraph = "曲线 AV equipment e\u{301} automatic 排版 100mm";
    let mut first = label("first", paragraph, true);
    first.width_mm = 33.;
    batch(&s, vec![create(first)]).unwrap();
    let original = measure(&s, "first");
    let rows = original["lines"].as_array().unwrap();
    assert!(rows.len() > 1);

    let prefix = "甲\r\n\u{2029}";
    let content = format!("{prefix}{paragraph}\n{paragraph}");
    let mut repeated = label("repeated", &content, true);
    repeated.width_mm = 33.;
    repeated.height_mm = 8.;
    settings(&mut repeated).align = TextAlign::Right;
    batch(&s, vec![create(repeated)]).unwrap();
    let result = measure(&s, "repeated");
    assert_eq!(result["overflow"], true);
    assert_eq!(result["line_count"], 2 + rows.len() * 2);
    for (copy, offset) in [prefix.len(), prefix.len() + paragraph.len() + 1].into_iter().enumerate() {
        for (i, reference) in rows.iter().enumerate() {
            let line = &result["lines"][2 + copy * rows.len() + i];
            assert_eq!(line["text"], reference["text"]);
            let range = &reference["source_range_bytes"];
            assert_eq!(line["source_range_bytes"], serde_json::json!([
                offset + range[0].as_u64().unwrap() as usize,
                offset + range[1].as_u64().unwrap() as usize
            ]));
            let last = i + 1 == rows.len();
            assert_eq!(line["break_after"], if !last { "soft" } else if copy == 0 { "hard" } else { "end" });
            near(line["baseline_origin_mm"][0].as_f64().unwrap(), 33. - reference["advance_mm"].as_f64().unwrap());
        }
    }
}

#[test]
fn area_overflow_keeps_all_lines_and_atomic_conversion_requires_a_fit() {
    let s=service();let mut o=label("text","中文\nEnglish\n尾行",true);o.height_mm=8.;batch(&s,vec![create(o)]).unwrap();let m=measure(&s,"text");assert_eq!(m["line_count"],3);assert_eq!(m["overflow"],true);
    assert!(m["warnings"].as_array().unwrap().iter().any(|w|w["code"]=="TEXT_AREA_OVERFLOW"));
    let before=s.snapshot("main").unwrap();let e=batch(&s,vec![update("text",ObjectPatch{x_mm:Some(88.),..Default::default()}),Operation::TextToPath{object_id:"text".into()}]).unwrap_err();assert_eq!(e.code,"TEXT_AREA_OVERFLOW");assert_eq!(e.failed_operation,Some(2));assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
    batch(&s,vec![update("text",ObjectPatch{height_mm:Some(40.),..Default::default()}),Operation::TextToPath{object_id:"text".into()}]).unwrap();assert!(matches!(s.snapshot("main").unwrap().document.object("text").unwrap().geometry,Geometry::Path{..}));
}

#[test]
fn invalid_spacing_incompatible_path_and_locks_roll_back_everything() {
    let s=service();batch(&s,vec![create(label("text","中文 AB",false))]).unwrap();
    for opt in [TextLayoutOptions{line_height_mm:Some(0.),..Default::default()},TextLayoutOptions{letter_spacing_mm:f64::NAN,..Default::default()},TextLayoutOptions{letter_spacing_mm:-100.,..Default::default()}] {
        let mut o=label("text","AB",false);*settings(&mut o)=opt;let before=s.snapshot("main").unwrap();let err=batch(&s,vec![update("text",ObjectPatch{x_mm:Some(99.),..Default::default()}),update("text",ObjectPatch{geometry:Some(o.geometry),..Default::default()})]).unwrap_err();assert_eq!(err.code,"INVALID_TEXT_LAYOUT");assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
    }
    let mut guide=label("guide","",false);guide.geometry=Geometry::Path{d:"M0 50L200 50".into(),fill_rule:Default::default()};batch(&s,vec![create(guide)]).unwrap();
    for (content,area) in [("中\n文",false),("中文",true)] {
        let mut o=label("text",content,area);if let Geometry::Text{text_path,..}=&mut o.geometry {*text_path=Some(TextOnPath{path_id:"guide".into(),start_offset_mm:0.,baseline_offset_mm:0.,anchor:TextAnchor::Start});}
        assert_eq!(batch(&s,vec![update("text",ObjectPatch{geometry:Some(o.geometry),..Default::default()})]).unwrap_err().code,"INVALID_TEXT_LAYOUT");
    }
    let natural=measure(&s,"text");let mut spaced=s.snapshot("main").unwrap().document.object("text").unwrap().clone();settings(&mut spaced).letter_spacing_mm=0.35;
    batch(&s,vec![update("text",ObjectPatch{geometry:Some(spaced.geometry),..Default::default()}),Operation::TextSetPath{object_id:"text".into(),text_path:Some(TextOnPath{path_id:"guide".into(),start_offset_mm:10.,baseline_offset_mm:0.,anchor:TextAnchor::Start})}]).unwrap();
    let bound=measure(&s,"text");assert!(bound["advance_mm"].as_f64().unwrap()>natural["advance_mm"].as_f64().unwrap());assert!(bound["lines"][0]["baseline_origin_mm"].is_null());
    let image=pixels(&s);batch(&s,vec![Operation::TextToPath{object_id:"text".into()}]).unwrap();assert!(image==pixels(&s),"Spaced path text must share its conversion contours");
    batch(&s,vec![Operation::ObjectLock{object_id:"text".into(),locked:true}]).unwrap();assert_eq!(batch(&s,vec![update("text",ObjectPatch{width_mm:Some(80.),..Default::default()})]).unwrap_err().code,"OBJECT_LOCKED");
}

#[test]
fn editable_native_roundtrip_and_old_default_hashes_are_preserved() {
    let s=service();let mut o=label("text","中文\r\nEnglish AV e\u{301}\n",true);settings(&mut o).align=TextAlign::Right;settings(&mut o).letter_spacing_mm=0.3;settings(&mut o).line_height_mm=Some(9.);
    batch(&s,vec![create(o)]).unwrap();let before=s.snapshot("main").unwrap();let path=temp().join("多行排版.arranger");s.command(Command::Save{document_id:"main".into(),expected_revision:before.revision,path:path.to_string_lossy().into_owned(),overwrite:false}).unwrap();let loaded=native::open(&path).unwrap();assert_eq!(*before.document,loaded.document);
    let old=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v1-minimum-legacy.arranger");let old=native::open(&old).unwrap();
    for object in &old.document.objects {if matches!(object.geometry,Geometry::Text{..}) {assert!(serde_json::to_value(&object.geometry).unwrap().get("layout").is_none());}}
}

#[test]
fn aligned_paragraph_outlines_svg_and_png_share_identical_contours() {
    let s=service();
    for (i,align) in [TextAlign::Left,TextAlign::Center,TextAlign::Right].into_iter().enumerate() {
        let mut o=label(&format!("text-{i}"),"中文 AV fi\nEnglish e\u{301}\n区域排版",true);o.x_mm=12.+i as f64*85.;o.transform=[1.,0.03,0.08,1.,0.,0.];o.style.stroke="#17334b".into();o.style.stroke_width_mm=0.1;
        settings(&mut o).align=align;settings(&mut o).letter_spacing_mm=0.25;settings(&mut o).line_height_mm=Some(10.);batch(&s,vec![create(o)]).unwrap();
    }
    let before=s.snapshot("main").unwrap();let image=pixels(&s);let measurements:Vec<_>=(0..3).map(|i|measure(&s,&format!("text-{i}"))).collect();
    batch(&s,(0..3).map(|i|Operation::TextToPath{object_id:format!("text-{i}")}).collect()).unwrap();let after=s.snapshot("main").unwrap();let changed=image.iter().zip(pixels(&s)).filter(|(a,b)|**a!=*b).count();assert_eq!(changed,0,"Outlining must preserve placed contours at 144 DPI");
    for (i,m) in measurements.iter().enumerate() {let obj=after.document.object(&format!("text-{i}")).unwrap();for (a,b) in after.document.bounds(obj).unwrap().into_iter().zip(m["bounds_mm"].as_array().unwrap()) {near(a,b.as_f64().unwrap());}}
    let svg=render::svg(&after.document,"page-1").unwrap();assert!(!svg.contains("<text"));assert!(!svg.contains("<image"));
    s.command(Command::Undo{document_id:"main".into(),expected_revision:after.revision}).unwrap();assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
}

#[test]
fn staged_area_typography_and_resize_publish_once_or_cancel_completely() {
    let s=service();batch(&s,vec![create(label("text","中英文 Arranger 自动换行 English words",true))]).unwrap();let before=s.snapshot("main").unwrap();let measurement=measure(&s,"text");
    for commit in [false,true] {
        let token=s.command(Command::Begin{document_id:"main".into(),expected_revision:before.revision,unit:"mm".into(),lease_ms:30000}).unwrap()["transaction_id"].as_str().unwrap().to_owned();
        let mut o=label("text","中英文 Arranger\n修改后的区域排版 English words",true);settings(&mut o).align=TextAlign::Center;settings(&mut o).letter_spacing_mm=0.5;settings(&mut o).line_height_mm=Some(10.);
        for operation in [update("text",ObjectPatch{geometry:Some(o.geometry),width_mm:Some(40.),..Default::default()}),update("text",ObjectPatch{x_mm:Some(30.),..Default::default()})] {s.command(Command::Execute{document_id:"main".into(),transaction_id:token.clone(),operation}).unwrap();}
        assert_eq!(measure(&s,"text"),measurement);assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
        s.command(if commit {Command::Commit{document_id:"main".into(),transaction_id:token}} else {Command::Rollback{document_id:"main".into(),transaction_id:token}}).unwrap();
        if !commit {assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);}
    }
    let after=s.snapshot("main").unwrap();assert_eq!(after.undo_depth,before.undo_depth+1);assert_ne!(measure(&s,"text"),measurement);
    s.command(Command::Undo{document_id:"main".into(),expected_revision:after.revision}).unwrap();assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
    s.command(Command::Redo{document_id:"main".into(),expected_revision:after.revision+1}).unwrap();assert_eq!(s.snapshot("main").unwrap().document_hash,after.document_hash);
}
