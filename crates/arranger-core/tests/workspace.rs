use arranger_core::{Command,Geometry,Object,Page,Service,Style,WORKSPACE_PAGE,command::{Operation,ObjectPatch},render::{Renderer,svg},native};

fn rect(id:&str,x:f64,y:f64)->Object{Object{id:id.into(),page_id:"page-1".into(),parent_id:None,x_mm:x,y_mm:y,width_mm:20.,height_mm:10.,transform:[1.,0.,0.,1.,0.,0.],style:Style{fill:"#ff0000".into(),..Default::default()},visible:true,locked:false,clip:None,geometry:Geometry::Rect{radius_mm:0.}}}
fn service()->Service{let s=Service::new();s.command(Command::DocumentNew{document_id:"main".into(),width_mm:210.,height_mm:297.,unit:"mm".into()}).unwrap();s}
fn batch(s:&Service,operations:Vec<Operation>)->arranger_core::Result<serde_json::Value>{s.command(Command::Batch{document_id:"main".into(),expected_revision:s.snapshot("main")?.revision,unit:"mm".into(),operations})}
fn place(id:&str,page:&str)->Operation{Operation::ObjectPlace{object_id:id.into(),page_id:page.into()}}
fn update(id:&str,x:f64)->Operation{Operation::ObjectUpdate{object_id:id.into(),patch:ObjectPatch{x_mm:Some(x),..Default::default()}}}

#[test]
fn page_contact_includes_edges_and_partial_overlap_but_all_four_outside_regions_are_shared(){
    let s=service();batch(&s,vec![Operation::ObjectCreate{object:rect("rect",25.,35.)}]).unwrap();
    for (x,y,owner) in [(-21.,35.,WORKSPACE_PAGE),(-20.,35.,"page-1"),(-10.,35.,"page-1"),(210.,35.,"page-1"),(210.001,35.,WORKSPACE_PAGE),(25.,-10.,"page-1"),(25.,-10.001,WORKSPACE_PAGE),(25.,297.,"page-1"),(25.,297.001,WORKSPACE_PAGE)]{
        batch(&s,vec![Operation::ObjectUpdate{object_id:"rect".into(),patch:ObjectPatch{x_mm:Some(x),y_mm:Some(y),..Default::default()}},place("rect","page-1")]).unwrap();
        assert_eq!(s.snapshot("main").unwrap().document.object("rect").unwrap().page_id,owner);
    }
}
#[test]
fn shared_roots_are_visible_and_hittable_on_both_pages_while_page_owned_objects_are_isolated(){
    let s=service();batch(&s,vec![Operation::PageAdd{page:Page{id:"second".into(),width_mm:210.,height_mm:297.}},Operation::ObjectCreate{object:rect("shared",-30.,20.)},place("shared","page-1"),Operation::ObjectCreate{object:rect("private",-10.,50.)},place("private","page-1")]).unwrap();
    let d=s.snapshot("main").unwrap().document;
    for page in ["page-1","second"]{assert_eq!(d.hit_test(page,[-20.,25.],0.),Some("shared".into()));let image=Renderer::new().render_viewport(&d,page,1.,[-40.,0.,80.,80.]).unwrap().pixels;assert_eq!(image.pixel(20,25).unwrap().red(),255);assert_eq!(image.pixel(20,25).unwrap().green(),0);}
    assert_eq!(d.hit_test("second",[-5.,55.],0.),None);assert_eq!(d.hit_test("page-1",[-5.,55.],0.),Some("private".into()));
    let export=svg(&d,"page-1").unwrap();assert!(!export.contains("object-shared")&&export.contains("object-private"));
    let paper=Renderer::new().render(&d,"page-1",25.4,None).unwrap().pixels;assert_eq!((paper.width(),paper.height()),(210,297));assert_eq!(paper.pixel(2,55).unwrap().green(),0);
    assert_eq!(Renderer::new().render(&d,"second",25.4,None).unwrap().pixels.pixel(2,55).unwrap().green(),255);
}
#[test]
fn moving_shared_content_into_another_page_is_one_transaction_and_one_undo_restores_scope_and_geometry(){
    let s=service();batch(&s,vec![Operation::PageAdd{page:Page{id:"second".into(),width_mm:210.,height_mm:297.}},Operation::ObjectCreate{object:rect("rect",-30.,20.)},place("rect","page-1")]).unwrap();let before=s.snapshot("main").unwrap();
    batch(&s,vec![update("rect",25.),place("rect","second")]).unwrap();let after=s.snapshot("main").unwrap();assert_eq!(after.undo_depth,before.undo_depth+1);assert_eq!(after.revision,before.revision+1);assert_eq!(after.document.object("rect").unwrap().page_id,"second");assert_eq!(after.document.hit_test("page-1",[30.,25.],0.),None);
    s.command(Command::Undo{document_id:"main".into(),expected_revision:after.revision}).unwrap();assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
}
#[test]
fn affine_group_placement_changes_the_whole_subtree_without_rewriting_child_geometry(){
    let s=service();let mut group=rect("group",0.,0.);group.geometry=Geometry::Group;group.transform=[1.,0.2,0.1,1.,-80.,25.];let mut child=rect("child",0.,0.);child.parent_id=Some("group".into());let saved=child.clone();
    batch(&s,vec![Operation::ObjectCreate{object:group},Operation::ObjectCreate{object:child},place("child","page-1")]).unwrap();let d=s.snapshot("main").unwrap().document;assert!(d.objects.iter().all(|o|o.page_id==WORKSPACE_PAGE));let mut actual=d.object("child").unwrap().clone();actual.page_id="page-1".into();assert_eq!(actual,saved);
}
#[test]
fn grouping_page_and_workspace_members_reconciles_membership_atomically(){
    let s=service();let mut g=rect("group",0.,0.);g.geometry=Geometry::Group;
    batch(&s,vec![Operation::ObjectCreate{object:rect("inside",25.,20.)},Operation::ObjectCreate{object:rect("outside",-30.,20.)},place("outside","page-1")]).unwrap();
    batch(&s,vec![Operation::ObjectCreate{object:g},Operation::ObjectReparent{object_id:"inside".into(),parent_id:Some("group".into())},Operation::ObjectReparent{object_id:"outside".into(),parent_id:Some("group".into())},place("group","page-1")]).unwrap();let d=s.snapshot("main").unwrap().document;d.validate().unwrap();assert!(d.objects.iter().all(|o|o.page_id=="page-1"));
}
#[test]
fn page_deletion_preserves_shared_content_and_native_roundtrip_keeps_ownership(){
    let s=service();batch(&s,vec![Operation::PageAdd{page:Page{id:"second".into(),width_mm:210.,height_mm:297.}},Operation::ObjectCreate{object:rect("shared",-30.,20.)},place("shared","page-1"),Operation::PageDelete{page_id:"page-1".into()}]).unwrap();let state=s.snapshot("main").unwrap();
    let dir=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/core-tests").join(format!("workspace-{}",std::process::id()));std::fs::create_dir_all(&dir).unwrap();let file=std::fs::canonicalize(&dir).unwrap().join("shared.arranger");native::save(&state.document,state.revision,&file,None,true).unwrap();assert_eq!(native::open(&file).unwrap().document,*state.document);assert_eq!(state.document.object("shared").unwrap().page_id,WORKSPACE_PAGE);
}
#[test]
fn invalid_page_and_locked_placement_roll_back_every_geometry_edit_and_workspace_cannot_be_a_paper_page(){
    let s=service();batch(&s,vec![Operation::ObjectCreate{object:rect("rect",25.,35.)}]).unwrap();let before=s.snapshot("main").unwrap();assert!(batch(&s,vec![update("rect",-50.),place("rect","missing")]).is_err());assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
    batch(&s,vec![Operation::ObjectLock{object_id:"rect".into(),locked:true}]).unwrap();let before=s.snapshot("main").unwrap();assert_eq!(batch(&s,vec![place("rect","page-1")]).unwrap_err().code,"OBJECT_LOCKED");assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
    assert!(batch(&s,vec![Operation::PageAdd{page:Page{id:WORKSPACE_PAGE.into(),width_mm:210.,height_mm:297.}}]).is_err());
}
#[test]
fn empty_text_uses_its_editable_frame_for_placement_instead_of_empty_ink_at_origin(){
    let s=service();let mut o=rect("empty",-40.,20.);o.geometry=Geometry::Text{content:String::new(),font_family:"sans-serif".into(),font_size_mm:5.,text_path:None,layout:Default::default()};
    batch(&s,vec![Operation::ObjectCreate{object:o},place("empty","page-1")]).unwrap();assert_eq!(s.snapshot("main").unwrap().document.object("empty").unwrap().page_id,WORKSPACE_PAGE);
}

#[test]
fn attached_text_and_its_guide_keep_one_owner_when_the_guide_moves_to_workspace(){
    let s=service();let mut guide=rect("guide",25.,30.);guide.geometry=Geometry::Path{d:"M 0 0 L 80 0".into(),fill_rule:Default::default()};let mut text=rect("text",0.,0.);text.geometry=Geometry::Text{content:"ABC".into(),font_family:"sans-serif".into(),font_size_mm:5.,text_path:Some(arranger_core::TextOnPath{path_id:"guide".into(),start_offset_mm:0.,baseline_offset_mm:0.,anchor:arranger_core::TextAnchor::Start}),layout:Default::default()};
    batch(&s,vec![Operation::ObjectCreate{object:guide},Operation::ObjectCreate{object:text}]).unwrap();batch(&s,vec![update("guide",-120.),place("guide","page-1")]).unwrap();let d=s.snapshot("main").unwrap().document;d.validate().unwrap();assert!(d.objects.iter().all(|o|o.page_id==WORKSPACE_PAGE));assert!(matches!(d.object("text").unwrap().geometry,Geometry::Text{text_path:Some(_),..}));
}

#[test]
fn legacy_documents_keep_v1_and_shared_documents_write_v2_with_explicit_reader_compatibility(){
    let s=service();let dir=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/core-tests").join(format!("workspace-version-{}",std::process::id()));std::fs::create_dir_all(&dir).unwrap();let dir=std::fs::canonicalize(dir).unwrap();let plain=dir.join("legacy.arranger");let shared=dir.join("shared.arranger");let before=s.snapshot("main").unwrap();
    native::save(&before.document,before.revision,&plain,None,true).unwrap();let header:serde_json::Value=serde_json::from_slice(&std::fs::read(&plain).unwrap()).unwrap();assert_eq!(header["format_version"],1);assert_eq!(native::open(&plain).unwrap().document,*before.document);
    batch(&s,vec![Operation::ObjectCreate{object:rect("shared",-40.,20.)},place("shared","page-1")]).unwrap();let after=s.snapshot("main").unwrap();native::save(&after.document,after.revision,&shared,None,true).unwrap();let mut header:serde_json::Value=serde_json::from_slice(&std::fs::read(&shared).unwrap()).unwrap();assert_eq!(header["format_version"],2);assert_eq!(native::open(&shared).unwrap().document,*after.document);
    let backup=std::path::PathBuf::from(format!("{}.bak",shared.display()));std::fs::copy(&plain,&backup).unwrap();header["format_version"]=99.into();std::fs::write(&shared,serde_json::to_vec(&header).unwrap()).unwrap();assert!(matches!(native::open(&shared),Err(e) if e.code=="UNSUPPORTED_FILE_VERSION"));
}
