use arranger_core::{BooleanOp, Command, FillRule, Geometry, Object, Operation, Page, PathElement, Service, Style,
    command::{ExportFormat, ObjectPatch}, native, render::{self, Renderer}};
use kurbo::{BezPath, ParamCurve, ParamCurveNearest, Point, Shape};
use serde_json::{Value, json};
use std::{fs, path::PathBuf, sync::atomic::{AtomicU64, Ordering}};

fn new() -> Service {
    let s = Service::new();
    s.command(Command::DocumentNew { document_id:"main".into(), width_mm:300., height_mm:200., unit:"mm".into() }).unwrap(); s
}
fn rect(id: &str, x: f64, y: f64, w: f64, h: f64) -> Object {
    Object { id:id.into(), page_id:"page-1".into(), parent_id:None, x_mm:x, y_mm:y, width_mm:w, height_mm:h,
        transform:[1.,0.,0.,1.,0.,0.], style:Style::default(), visible:true, locked:false,clip:None, geometry:Geometry::Rect {radius_mm:0.} }
}
fn path(id: &str, d: &str, fill_rule: FillRule) -> Object {
    Object { geometry:Geometry::Path {d:d.into(),fill_rule}, ..rect(id,0.,0.,100.,100.) }
}
fn batch(s: &Service, operations: Vec<Operation>) -> arranger_core::Result<Value> {
    s.command(Command::Batch { document_id:"main".into(), expected_revision:s.snapshot("main")?.revision, unit:"mm".into(), operations })
}
fn create(object: Object) -> Operation { Operation::ObjectCreate {object} }
fn boolean(left: &str, right: &str, result: &str, operation: BooleanOp, remove_inputs: bool) -> Operation {
    Operation::PathBoolean {left_id:left.into(),right_id:right.into(),result_id:result.into(),operation,remove_inputs}
}
fn get(s: &Service, id: &str) -> Value {
    s.command(Command::PathGet {document_id:"main".into(),object_id:id.into()}).unwrap()
}
fn result_path(s: &Service, id: &str) -> BezPath { s.snapshot("main").unwrap().document.object(id).unwrap().local_path().unwrap() }
fn inside(s: &Service, id: &str, x: f64, y: f64) -> bool {
    let snap=s.snapshot("main").unwrap();let o=snap.document.object(id).unwrap();
    // A separate fine polyline projection avoids the cubic root solver's
    // numerical degeneracy; the model itself must retain cubic commands.
    let mut outline=BezPath::new();
    kurbo::flatten(o.local_path().unwrap().elements().iter().copied(),1e-6,|e|outline.push(e));
    o.fill_rule().is_inside(outline.winding(Point::new(x,y)))
}
fn near(a: f64, b: f64) { assert!((a-b).abs() < 1e-6, "{a} != {b}"); }
fn temp() -> PathBuf {
    static NEXT:AtomicU64=AtomicU64::new(1);
    let root=PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_owned();
    let dir=root.join(format!("artifacts/core-tests/paths-{}-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),NEXT.fetch_add(1,Ordering::Relaxed)));
    fs::create_dir_all(&dir).unwrap();dir
}

#[test]
fn square_corner_rectangle_conversion_has_four_distinct_anchors_and_no_zero_length_curves() {
    let s=new();batch(&s,vec![create(rect("frame",10.,20.,60.,14.))]).unwrap();
    let before=s.snapshot("main").unwrap();
    batch(&s,vec![Operation::ObjectToPath{object_id:"frame".into()}]).unwrap();
    let path=result_path(&s,"frame");
    assert!(path.iter().all(|e|matches!(e,kurbo::PathEl::MoveTo(_)|kurbo::PathEl::LineTo(_)|kurbo::PathEl::ClosePath)));
    assert_eq!(path.segments().count(),4);
    assert!(path.segments().all(|s|s.eval(0.).distance(s.eval(1.))>0.));
    near(path.bounding_box().width(),60.);near(path.bounding_box().height(),14.);
    let after=s.snapshot("main").unwrap();assert_eq!(after.undo_depth,before.undo_depth+1);
    s.command(Command::Undo{document_id:"main".into(),expected_revision:after.revision}).unwrap();
    assert_eq!(s.snapshot("main").unwrap().document,before.document);
}

#[test]
fn structured_nodes_handles_subpaths_and_history_share_one_transaction() {
    let s=new();let mut o=path("curve","m10 20 c10 -15 40 -15 50 0 q20 30 40 0",FillRule::NonZero);
    o.transform=[0.,1.,-1.,0.,200.,0.];batch(&s,vec![create(o)]).unwrap();
    let original=s.snapshot("main").unwrap();
    assert_eq!(get(&s,"curve")["elements"][1],json!({"type":"cubic_to","control1":[20.,5.],"control2":[50.,5.],"to":[60.,20.]}));
    let token=s.command(Command::Begin {document_id:"main".into(),expected_revision:original.revision,unit:"mm".into(),lease_ms:30000}).unwrap()["transaction_id"].as_str().unwrap().to_owned();
    let ops=vec![
        Operation::PathNodeUpdate {object_id:"curve".into(),index:0,element:PathElement::MoveTo{to:[11.,21.]}},
        Operation::PathNodeUpdate {object_id:"curve".into(),index:1,element:PathElement::CubicTo{control1:[25.,0.],control2:[55.,0.],to:[65.,21.]}},
        Operation::PathNodeInsert {object_id:"curve".into(),index:3,element:PathElement::LineTo{to:[110.,30.]}},
        Operation::PathNodeDelete {object_id:"curve".into(),index:3},
        Operation::PathSubpathSetClosed {object_id:"curve".into(),subpath_index:0,closed:true},
    ];
    for operation in ops { s.command(Command::Execute {document_id:"main".into(),transaction_id:token.clone(),operation}).unwrap(); }
    assert_eq!(s.snapshot("main").unwrap().document_hash,original.document_hash);
    assert_eq!(get(&s,"curve")["elements"][0]["to"],json!([10.,20.])); // Queries see committed state.
    s.command(Command::Commit {document_id:"main".into(),transaction_id:token}).unwrap();
    let changed=s.snapshot("main").unwrap();assert_eq!(changed.undo_depth,original.undo_depth+1);
    let info=get(&s,"curve");assert_eq!(info["elements"][1]["control1"],json!([25.,0.]));assert_eq!(info["subpaths"][0]["closed"],true);
    assert_eq!(info["world_transform"],json!([0.,1.,-1.,0.,200.,0.]));
    s.command(Command::Undo {document_id:"main".into(),expected_revision:changed.revision}).unwrap();
    assert_eq!(s.snapshot("main").unwrap().document_hash,original.document_hash);
    s.command(Command::Redo {document_id:"main".into(),expected_revision:changed.revision+1}).unwrap();
    assert_eq!(s.snapshot("main").unwrap().document_hash,changed.document_hash);
    batch(&s,vec![Operation::PathSubpathSetClosed{object_id:"curve".into(),subpath_index:0,closed:false}]).unwrap();
    assert_eq!(get(&s,"curve")["subpaths"][0]["closed"],false);
}

#[test]
fn split_line_quadratic_and_cubic_preserves_the_curve_not_a_polyline() {
    for (id,d) in [("line","M3 7 L90 20"),("quad","M3 7 Q40 -30 90 20"),("cubic","M3 7 C-20 40 70 -30 90 20")] {
        let s=new();batch(&s,vec![create(path(id,d,FillRule::NonZero))]).unwrap();
        let before=result_path(&s,id);let segment=before.get_seg(1).unwrap();let t=0.37;
        batch(&s,vec![Operation::PathSplitSegment {object_id:id.into(),index:1,t}]).unwrap();
        let after=result_path(&s,id);assert_eq!(after.elements().len(),3);
        for i in 0..=100 {
            let u=i as f64/100.;let expected=segment.eval(u);
            let actual=if u<=t {after.get_seg(1).unwrap().eval(u/t)} else {after.get_seg(2).unwrap().eval((u-t)/(1.-t))};
            assert!(expected.distance(actual)<1e-10,"{id} at {u}");
        }
        assert_eq!(std::mem::discriminant(&before.elements()[1]),std::mem::discriminant(&after.elements()[1]));
    }
}

#[test]
fn typed_path_set_compound_empty_and_svg_after_close_are_editable() {
    let s=new();batch(&s,vec![create(path("p","M0 0L20 0L20 20ZL30 30",FillRule::NonZero))]).unwrap();
    let info=get(&s,"p");assert_eq!(info["elements"][4],json!({"type":"move_to","to":[0.,0.]}));
    assert_eq!(info["subpaths"].as_array().unwrap().len(),2);
    batch(&s,vec![Operation::PathSet {object_id:"p".into(),elements:vec![
        PathElement::MoveTo{to:[0.,0.]},PathElement::LineTo{to:[100.,0.]},PathElement::LineTo{to:[100.,100.]},PathElement::LineTo{to:[0.,100.]},PathElement::Close,
        PathElement::MoveTo{to:[20.,20.]},PathElement::LineTo{to:[80.,20.]},PathElement::LineTo{to:[80.,80.]},PathElement::LineTo{to:[20.,80.]},PathElement::Close,
    ],fill_rule:Some(FillRule::EvenOdd)}]).unwrap();
    assert!(inside(&s,"p",10.,10.));assert!(!inside(&s,"p",50.,50.));
    assert_eq!(get(&s,"p")["fill_rule"],"evenodd");
    batch(&s,vec![Operation::PathSet{object_id:"p".into(),elements:vec![],fill_rule:None}]).unwrap();
    assert_eq!(get(&s,"p")["elements"],json!([]));assert_eq!(get(&s,"p")["bounds_mm"],json!([0.,0.,0.,0.]));
    batch(&s,vec![Operation::PathNodeInsert {object_id:"p".into(),index:0,element:PathElement::MoveTo{to:[12.,13.]}}]).unwrap();
    batch(&s,vec![Operation::PathNodeDelete {object_id:"p".into(),index:0}]).unwrap();
    assert_eq!(get(&s,"p")["elements"],json!([]));
}

#[test]
fn four_booleans_have_analytic_areas_and_editable_output() {
    let s=new();batch(&s,vec![create(rect("a",10.,10.,100.,50.)),create(rect("b",60.,10.,100.,50.))]).unwrap();
    for (id,op,area) in [("union",BooleanOp::Union,7500.),("intersection",BooleanOp::Intersection,2500.),("difference",BooleanOp::Difference,2500.),("xor",BooleanOp::Xor,5000.)] {
        batch(&s,vec![boolean("a","b",id,op,false)]).unwrap();near(result_path(&s,id).area().abs(),area);
        assert_eq!(get(&s,id)["fill_rule"],"evenodd");
        assert!(get(&s,id)["elements"].as_array().unwrap().len()>=4);
        let old=get(&s,id)["elements"][0].clone();
        batch(&s,vec![Operation::PathNodeUpdate{object_id:id.into(),index:0,element:serde_json::from_value(old).unwrap()}]).unwrap();
    }
    assert!(s.snapshot("main").unwrap().document.object("a").is_ok());
    assert!(s.snapshot("main").unwrap().document.object("b").is_ok());
}

#[test]
fn curve_boolean_retains_cubics_and_matches_symmetric_area_oracle() {
    let s=new();batch(&s,vec![create(path("cap","M10 60 C10 10 110 10 110 60 L110 110 L10 110 Z",FillRule::NonZero)),create(rect("clip",60.,0.,100.,150.))]).unwrap();
    // Integral of (110-y(t))*x'(t) is 8000; the vertical centre bisects it.
    near(result_path(&s,"cap").area().abs(),8000.);
    for (id,op,area) in [("u",BooleanOp::Union,19000.),("i",BooleanOp::Intersection,4000.),("d",BooleanOp::Difference,4000.),("x",BooleanOp::Xor,15000.)] {
        batch(&s,vec![boolean("cap","clip",id,op,false)]).unwrap();
        let output=result_path(&s,id);
        // eps is a distance (mm), not an area. This area bound is stricter than
        // perimeter * 1e-6 mm for these specimens, and rejects GoodEnough mode's
        // observed 0.1624 mm² deviation. Check boundary displacement separately.
        let error=(output.area().abs()-area).abs();assert!(error<0.001,"{id} area error {error}");
        let snap=s.snapshot("main").unwrap();let clip=snap.document.object("clip").unwrap();
        let inputs=[result_path(&s,"cap"),snap.document.world_transform(clip).unwrap()*clip.local_path().unwrap()];
        for segment in output.segments() { for i in 0..=20 {
            let point=segment.eval(i as f64/20.);
            let distance=inputs.iter().flat_map(|p|p.segments()).map(|seg|seg.nearest(point,1e-9).distance_sq).fold(f64::INFINITY,f64::min).sqrt();
            assert!(distance<5e-6,"{id} boundary moved {distance} mm");
        }}
        println!("{id}: area error {error} mm^2; sampled boundary < 0.000005 mm");
        assert!(get(&s,id)["elements"].as_array().unwrap().iter().any(|el|el["type"]=="cubic_to"));
    }
    // Independent membership oracle at points well away from all boundaries.
    for x in (13..160).step_by(7) { for y in (3..150).step_by(7) {
        let (a,b)=(inside(&s,"cap",x as f64,y as f64),x>60 && x<160 && y>0 && y<150);
        for (id,expected) in [("u",a||b),("i",a&&b),("d",a&&!b),("x",a!=b)] {assert_eq!(inside(&s,id,x as f64,y as f64),expected,"{id} {x},{y}: {}",result_path(&s,id).to_svg());}
    }}
    // The near-quadratic cubic in this result previously selected empty space.
    // Isolate it from the other overlapping fixtures for a real core hit query.
    batch(&s,vec![Operation::ObjectDelete{object_id:"cap".into()},Operation::ObjectDelete{object_id:"clip".into()},Operation::ObjectDelete{object_id:"u".into()},Operation::ObjectDelete{object_id:"d".into()},Operation::ObjectDelete{object_id:"x".into()}]).unwrap();
    assert_eq!(s.snapshot("main").unwrap().document.hit_test("page-1",[76.,23.],0.01),None);
    assert_eq!(s.snapshot("main").unwrap().document.hit_test("page-1",[76.,27.],0.01),Some("i".into()));
}

#[test]
fn empty_coincident_touching_containment_and_mixed_fill_rules() {
    let s=new();batch(&s,vec![create(rect("a",10.,10.,100.,100.)),create(rect("same",10.,10.,100.,100.)),create(rect("touch",110.,10.,20.,100.)),create(rect("apart",150.,10.,20.,20.)),create(path("empty","",FillRule::NonZero))]).unwrap();
    for (id,a,b,op,area) in [
        ("same-d","a","same",BooleanOp::Difference,0.),("same-x","a","same",BooleanOp::Xor,0.),("same-i","a","same",BooleanOp::Intersection,10000.),
        ("touch-i","a","touch",BooleanOp::Intersection,0.),("touch-u","a","touch",BooleanOp::Union,12000.),
        ("apart-i","a","apart",BooleanOp::Intersection,0.),("apart-x","a","apart",BooleanOp::Xor,10400.),
        ("empty-u","empty","a",BooleanOp::Union,10000.),("empty-i","empty","a",BooleanOp::Intersection,0.),
        ("self-x","a","a",BooleanOp::Xor,0.)] {
        batch(&s,vec![boolean(a,b,id,op,false)]).unwrap();near(result_path(&s,id).area().abs(),area);
        if area==0. { assert!(get(&s,id)["elements"].as_array().unwrap().is_empty()); }
    }
    let donut="M10 10H110V110H10Z M30 30H90V90H30Z";
    batch(&s,vec![create(path("evenodd",donut,FillRule::EvenOdd)),create(path("nonzero",donut,FillRule::NonZero)),create(rect("inner",40.,40.,20.,20.))]).unwrap();
    batch(&s,vec![boolean("evenodd","inner","hole-empty",BooleanOp::Intersection,false),boolean("nonzero","inner","hole-full",BooleanOp::Intersection,false),boolean("nonzero","inner","cut-hole",BooleanOp::Difference,false),boolean("evenodd","nonzero","mixed",BooleanOp::Xor,false)]).unwrap();
    assert!(get(&s,"hole-empty")["elements"].as_array().unwrap().is_empty());near(result_path(&s,"hole-full").area().abs(),400.);
    assert!(!inside(&s,"cut-hole",50.,50.));assert!(inside(&s,"cut-hole",20.,20.));
    assert!(inside(&s,"mixed",50.,50.));assert!(!inside(&s,"mixed",20.,20.));
}

#[test]
fn transformed_group_operands_become_page_space_paths_and_convert_keeps_identity() {
    let s=new();let mut group=rect("group",20.,30.,0.,0.);group.geometry=Geometry::Group;group.transform=[2.,0.,0.,2.,0.,0.];
    let mut child=rect("child",5.,5.,20.,10.);child.parent_id=Some("group".into());
    let mut ellipse=rect("ellipse",5.,5.,20.,10.);ellipse.geometry=Geometry::Ellipse;ellipse.parent_id=Some("group".into());
    batch(&s,vec![create(group),create(child),create(rect("other",60.,70.,20.,20.)),create(ellipse)]).unwrap();
    let original=s.snapshot("main").unwrap();
    batch(&s,vec![boolean("child","other","result",BooleanOp::Intersection,true),Operation::ObjectToPath{object_id:"ellipse".into()}]).unwrap();
    let info=get(&s,"result");assert_eq!(info["bounds_mm"],json!([60.,70.,20.,20.]));near(result_path(&s,"result").area().abs(),400.);
    let snap=s.snapshot("main").unwrap();let result=snap.document.object("result").unwrap();assert_eq!(result.parent_id,None);assert_eq!(result.transform,[1.,0.,0.,1.,0.,0.]);
    let converted=snap.document.object("ellipse").unwrap();assert_eq!(converted.parent_id.as_deref(),Some("group"));assert_eq!(converted.x_mm,5.);assert!(get(&s,"ellipse")["elements"].as_array().unwrap().iter().any(|e|e["type"]=="cubic_to"));
    assert!(snap.document.object("child").is_err());assert_eq!(snap.undo_depth,original.undo_depth+1);
    s.command(Command::Undo{document_id:"main".into(),expected_revision:snap.revision}).unwrap();assert_eq!(s.snapshot("main").unwrap().document_hash,original.document_hash);
}

#[test]
fn invalid_nodes_and_boolean_failures_roll_back_prior_edits_and_input_removal() {
    let s=new();let mut locked=rect("locked",0.,0.,0.,0.);locked.geometry=Geometry::Group;locked.locked=true;
    // Create the child before locking its parent in the same initial transaction.
    locked.locked=false;let mut child=path("child","M0 0H20V20Z",FillRule::NonZero);child.parent_id=Some("locked".into());
    let mut other=rect("other-page",0.,0.,20.,20.);other.page_id="page-2".into();
    let mut huge=rect("huge",1e20,0.,20.,20.);huge.transform=[1e100,0.,0.,1.,0.,0.];
    batch(&s,vec![create(path("p","M0 0C10 0 10 20 20 20Z",FillRule::NonZero)),create(path("open","M0 0L20 20",FillRule::NonZero)),create(rect("r",0.,0.,30.,30.)),create(locked),create(child),Operation::ObjectLock{object_id:"locked".into(),locked:true},Operation::PageAdd{page:Page{id:"page-2".into(),width_mm:200.,height_mm:200.}},create(other),create(huge)]).unwrap();
    for (op,code) in [
        (Operation::PathNodeDelete{object_id:"p".into(),index:0},"INVALID_PATH"),
        (Operation::PathNodeUpdate{object_id:"p".into(),index:9,element:PathElement::MoveTo{to:[0.,0.]}},"INVALID_INDEX"),
        (Operation::PathNodeUpdate{object_id:"p".into(),index:1,element:PathElement::CubicTo{control1:[f64::NAN,0.],control2:[0.,1.],to:[1.,1.]}},"INVALID_PATH"),
        (Operation::PathSplitSegment{object_id:"p".into(),index:1,t:0.},"INVALID_PARAMETER"),
        (Operation::PathSplitSegment{object_id:"p".into(),index:2,t:0.5},"INVALID_INDEX"),
        (Operation::PathSubpathSetClosed{object_id:"p".into(),subpath_index:2,closed:true},"INVALID_INDEX"),
        (Operation::PathSet{object_id:"r".into(),elements:vec![],fill_rule:None},"NOT_A_PATH"),
        (Operation::PathSet{object_id:"child".into(),elements:vec![],fill_rule:None},"OBJECT_LOCKED"),
        (boolean("open","r","bad",BooleanOp::Union,true),"OPEN_PATH"),
        (boolean("p","r","r",BooleanOp::Union,true),"DUPLICATE_ID"),
        (boolean("p","other-page","bad",BooleanOp::Union,true),"CROSS_PAGE"),
        (boolean("p","child","bad",BooleanOp::Union,true),"OBJECT_LOCKED"),
        (boolean("p","huge","bad",BooleanOp::Union,true),"BOOLEAN_RANGE"),
        (boolean("p","missing","bad",BooleanOp::Union,true),"OBJECT_NOT_FOUND"),
    ] {
        let before=s.snapshot("main").unwrap();
        let err=batch(&s,vec![Operation::ObjectUpdate{object_id:"r".into(),patch:ObjectPatch{x_mm:Some(123.),..Default::default()}},op]).unwrap_err();
        assert_eq!(err.code,code);assert_eq!(err.failed_operation,Some(2));assert_eq!(err.rolled_back,Some(true));
        let after=s.snapshot("main").unwrap();assert_eq!(after.document_hash,before.document_hash);assert_eq!(after.undo_depth,before.undo_depth);assert!(!after.busy);
    }
}

#[test]
fn malformed_path_execute_cancels_owned_transaction_and_explicit_rollback_works() {
    let s=new();batch(&s,vec![create(path("p","M0 0L20 20",FillRule::NonZero))]).unwrap();let before=s.snapshot("main").unwrap();
    for malformed in [true,false] {
        let token=s.command(Command::Begin{document_id:"main".into(),expected_revision:before.revision,unit:"mm".into(),lease_ms:30000}).unwrap()["transaction_id"].as_str().unwrap().to_owned();
        s.command(Command::Execute{document_id:"main".into(),transaction_id:token.clone(),operation:Operation::PathNodeUpdate{object_id:"p".into(),index:1,element:PathElement::LineTo{to:[55.,55.]}}}).unwrap();
        if malformed {
            let response=s.execute_json(&json!({"protocol_version":1,"command":{"type":"execute","document_id":"main","transaction_id":token,"operation":{"type":"path_node_update","object_id":"p","index":1,"element":{"type":"cubic_to","to":[1,2],"control1":[0,1]}}}}).to_string());
            let error=response.error.unwrap();assert_eq!(error.code,"INVALID_REQUEST");assert_eq!(error.rolled_back,Some(true));
        } else {s.command(Command::Rollback{document_id:"main".into(),transaction_id:token}).unwrap();}
        assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
    }
}

#[test]
fn native_reopen_svg_png_and_hit_test_keep_boolean_holes_and_editable_nodes() {
    let s=new();batch(&s,vec![create(rect("outer",10.,10.,100.,100.)),create(rect("inner",40.,40.,40.,40.)),boolean("outer","inner","ring",BooleanOp::Difference,true)]).unwrap();
    let before=s.snapshot("main").unwrap();let dir=temp();let file=dir.join("孔洞与曲线.arranger");
    s.command(Command::Save{document_id:"main".into(),expected_revision:before.revision,path:file.to_string_lossy().into_owned(),overwrite:false}).unwrap();
    let loaded=native::open(&file).unwrap();assert_eq!(*before.document,loaded.document);
    let text=render::svg(&loaded.document,"page-1").unwrap();assert!(text.contains("fill-rule=\"evenodd\""));assert!(!text.contains("<image"));
    let renderer=Renderer::new();let image=renderer.render(&loaded.document,"page-1",254.,None).unwrap().pixels;
    let color=|x,y| image.pixel(x,y).unwrap();assert_eq!(color(600,600).red(),255);assert_eq!(color(200,200).red(),40);
    assert_eq!(loaded.document.hit_test("page-1",[60.,60.],0.1),None);assert_eq!(loaded.document.hit_test("page-1",[20.,20.],0.1),Some("ring".into()));
    for (format,name) in [(ExportFormat::Svg,"ring.svg"),(ExportFormat::Png,"ring.png")] {renderer.export(&loaded.document,"page-1",format,96.,&dir.join(name),false).unwrap();}
    s.command(Command::DocumentOpen{document_id:"reopened".into(),path:file.to_string_lossy().into_owned()}).unwrap();
    let response=s.command(Command::Batch{document_id:"reopened".into(),expected_revision:before.revision,unit:"mm".into(),operations:vec![Operation::PathSplitSegment{object_id:"ring".into(),index:1,t:0.5}]}).unwrap();
    assert_eq!(response["revision"],before.revision+1);
}

#[test]
fn previous_native_path_default_keeps_its_canonical_checksum() {
    // The prior release's retained specimen is an independent, pre-change file.
    let root=PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_owned();
    let file=root.join("crates/arranger-core/tests/fixtures/v1-minimum-legacy.arranger");
    assert!(file.exists(),"Historical fixture must remain in the repository");
    let loaded=native::open(&file).unwrap();
    assert!(loaded.document.objects.iter().any(|o|matches!(o.geometry,Geometry::Path{..})));
    let raw:Value=serde_json::from_slice(&fs::read(file).unwrap()).unwrap();
    assert_eq!(loaded.document.hash(),raw["document_sha256"].as_str().unwrap());
    for o in loaded.document.objects { if matches!(o.geometry,Geometry::Path{..}) {assert!(serde_json::to_value(o.geometry).unwrap().get("fill_rule").is_none());} }
}

#[test]
fn ellipse_round_rectangle_and_self_intersection_use_the_same_boolean_adapter() {
    let s=new();let mut ellipse=rect("ellipse",10.,10.,100.,60.);ellipse.geometry=Geometry::Ellipse;
    let mut rounded=rect("rounded",10.,10.,100.,60.);rounded.geometry=Geometry::Rect{radius_mm:10.};
    batch(&s,vec![create(ellipse),create(rounded),create(rect("clip",60.,0.,100.,100.)),
        create(path("bowtie","M10 10L110 70L10 70L110 10Z",FillRule::EvenOdd)),create(path("degenerate","M10 10L110 70Z",FillRule::NonZero))]).unwrap();
    for id in ["ellipse","rounded","bowtie"] {
        let snap=s.snapshot("main").unwrap();let object=snap.document.object(id).unwrap();
        let input=snap.document.world_transform(object).unwrap()*object.local_path().unwrap();
        let original=input.area().abs();
        for (suffix,op) in [("u",BooleanOp::Union),("i",BooleanOp::Intersection),("d",BooleanOp::Difference),("x",BooleanOp::Xor)] {
            let output=format!("{id}-{suffix}");batch(&s,vec![boolean(id,"clip",&output,op,false)]).unwrap();
            if id!="bowtie" && suffix=="i" {assert!((result_path(&s,&output).area().abs()-original/2.).abs()<0.001);}
            for x in (7..160).step_by(9) {for y in (3..100).step_by(11) {
                let p=Point::new(x as f64,y as f64);
                if input.segments().any(|seg|seg.nearest(p,1e-7).distance_sq<0.01) {continue;}
                let mut outline=BezPath::new();kurbo::flatten(input.elements().iter().copied(),1e-6,|e|outline.push(e));
                let a=object.fill_rule().is_inside(outline.winding(p));let b=x>60 && x<160 && y>0 && y<100;
                let expected=match op {BooleanOp::Union=>a||b,BooleanOp::Intersection=>a&&b,BooleanOp::Difference=>a&&!b,BooleanOp::Xor=>a!=b};
                assert_eq!(inside(&s,&output,p.x,p.y),expected,"{output} at {x},{y}");
            }}
        }
    }
    batch(&s,vec![boolean("degenerate","clip","zero",BooleanOp::Intersection,false)]).unwrap();
    assert!(get(&s,"zero")["elements"].as_array().unwrap().is_empty());
}
