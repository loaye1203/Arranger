use arranger_core::{cdr,Document,Geometry,Object,Style,WORKSPACE_PAGE};
use kurbo::Point;

#[test]
fn actual_corel_resaved_hidden_markers_and_uniform_text_flags_are_readable() {
 let root=std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
 let states=cdr::decode(&std::fs::read(root.join("examples/assets/cdr/corel2019/hidden-state-marker-v14.cdr")).unwrap(),"states").unwrap().0;
 for (id,visible,locked) in [("hidden",false,false),("locked",true,true),("state-group",false,true),("state-member",false,true),("desktop-state",false,true)] {
  let o=states.object(id).unwrap();assert_eq!((o.visible,o.locked),(visible,locked));
 }
 assert_eq!(states.object("desktop-state").unwrap().page_id,WORKSPACE_PAGE);
 let mixed=cdr::decode(&std::fs::read(root.join("examples/assets/cdr/corel2019/uniform-text-flags-v14.cdr")).unwrap(),"mixed").unwrap().0;
 assert_eq!(mixed.pages.len(),3);
 assert!(matches!(&mixed.object("text").unwrap().geometry,Geometry::Text{content,font_family,..} if content=="中文 / live 42"&&font_family=="宋体"));
 let reopened=cdr::decode(&cdr::encode_document(&mixed).unwrap().0,"mixed").unwrap().0;
 assert_eq!(mixed.objects.len(),reopened.objects.len());
}
fn object(id:&str,page:&str)->Object {Object{id:id.into(),page_id:page.into(),parent_id:None,x_mm:12.5,y_mm:23.75,width_mm:31.,height_mm:17.,transform:[0.9,0.2,-0.1,1.1,3.,-2.],style:Style{fill:"#123456".into(),stroke:"none".into(),..Default::default()},visible:true,locked:false,clip:None,geometry:Geometry::Ellipse}}
fn scene()->Document {
    let mut d=Document::new("source".into(),297.,210.).unwrap();d.pages[0].id="landscape".into();
    d.pages.push(arranger_core::Page{id:"label".into(),width_mm:60.,height_mm:14.});
    d.pages.push(arranger_core::Page{id:"blank".into(),width_mm:123.4567,height_mm:234.5678});
    d.objects.push(object("drawing-a","landscape"));
    d.objects.push(object("drawing-b","label"));
    d.objects.push(object("desktop",WORKSPACE_PAGE));d
}
fn near(a:f64,b:f64){assert!((a-b).abs()<0.00021,"{a} != {b}");}
#[test]
fn mixed_page_sizes_native_names_and_workspace_coordinates_survive() {
    let d=scene();let (raw,w)=cdr::encode_document(&d).unwrap();assert!(w.iter().any(|n|n.code=="CDR_WORKSPACE"));
    let (out,w)=cdr::decode(&raw,"source").unwrap();assert_eq!(out.pages,d.pages);assert!(!w.iter().any(|n|n.code=="CDR_PAGES_ASSIGNED"));
    for a in &d.objects {let b=out.object(&a.id).unwrap();assert_eq!(a.page_id,b.page_id);for (p,q) in d.bounds(a).unwrap().into_iter().zip(out.bounds(b).unwrap()){near(p,q);}}
    assert_eq!(out.objects.iter().filter(|o|o.page_id==WORKSPACE_PAGE).count(),1);
    let again=cdr::decode(&cdr::encode_document(&out).unwrap().0,"source").unwrap().0;assert_eq!(out.hash(),again.hash());
}
#[test]
fn explicit_selected_page_excludes_desktop_and_retains_its_size_and_name() {
    let d=scene();let (raw,w)=cdr::encode(&d,"label").unwrap();assert!(w.iter().any(|n|n.code=="CDR_PAGE_ONLY"));
    let out=cdr::decode(&raw,"selected").unwrap().0;assert_eq!(out.pages,vec![d.pages[1].clone()]);assert_eq!(out.objects.len(),1);assert_eq!(out.objects[0].id,"drawing-b");
    for (a,b) in d.bounds(d.object("drawing-b").unwrap()).unwrap().into_iter().zip(out.bounds(&out.objects[0]).unwrap()){near(a,b);}
}
#[test]
fn locked_hidden_group_and_clipped_children_keep_state_and_edit_guards() {
    let mut d=scene();let mut g=object("locked-group",WORKSPACE_PAGE);g.geometry=Geometry::Group;g.locked=true;d.objects.push(g);
    d.objects[2].parent_id=Some("locked-group".into());d.objects[2].visible=false;
    d.objects[2].clip=Some(arranger_core::Clip::Rect{rect_mm:[0.,0.,20.,10.]});
    d.objects[1].locked=true;d.objects[1].visible=false;
    let out=cdr::decode(&cdr::encode_document(&d).unwrap().0,"states").unwrap().0;
    for id in ["locked-group","desktop","drawing-b"] {let a=d.object(id).unwrap();let b=out.object(id).unwrap();assert_eq!((a.visible,a.locked),(b.visible,b.locked));assert_eq!(out.require_editable(id).unwrap_err().code,"OBJECT_LOCKED");}
    assert_eq!(out.object("desktop").unwrap().parent_id.as_deref(),Some("locked-group"));assert!(out.object("desktop").unwrap().clip.is_some());
    let a=d.world_transform(d.object("desktop").unwrap()).unwrap()*Point::new(5.,7.);
    let b=out.world_transform(out.object("desktop").unwrap()).unwrap()*Point::new(5.,7.);near(a.x,b.x);near(a.y,b.y);
}
#[test]
fn text_and_clip_use_their_own_page_coordinate_origin() {
    let mut d=scene();d.objects[0].geometry=Geometry::Text{content:"中文 / live".into(),font_family:"Arial".into(),font_size_mm:5.,text_path:None,layout:Default::default()};
    d.objects[1].clip=Some(arranger_core::Clip::Rect{rect_mm:[1.,2.,14.,8.]});
    let out=cdr::decode(&cdr::encode_document(&d).unwrap().0,"origin").unwrap().0;
    let a=d.world_transform(&d.objects[0]).unwrap()*Point::new(0.,5.);let b=out.world_transform(out.object("drawing-a").unwrap()).unwrap()*Point::new(0.,5.);near(a.x,b.x);near(a.y,b.y);
    let o=out.object("drawing-b").unwrap();let clip=o.clip.as_ref().unwrap();let expected=d.world_transform(&d.objects[1]).unwrap()*d.objects[1].clip.as_ref().unwrap().path().unwrap();
    let actual=out.world_transform(o).unwrap()*clip.path().unwrap();use kurbo::Shape;let a=expected.bounding_box();let b=actual.bounding_box();for (a,b) in [a.x0,a.y0,a.x1,a.y1].into_iter().zip([b.x0,b.y0,b.x1,b.y1]){near(a,b);}
}
#[test]
fn page_sizes_below_native_precision_and_coordinate_range_reject() {
    let mut d=scene();for value in [0.000001,300000.] {d.pages[1].width_mm=value;let hash=d.hash();assert!(cdr::encode_document(&d).is_err());assert_eq!(hash,d.hash());}
    // Selected-page export still validates only its explicit output scope.
    assert!(cdr::encode(&d,"landscape").is_ok());
}
