use arranger_core::{cdr,Document,Object,Geometry,Style,Clip,ImageFit,Resource,Service,Command,command::{ObjectPatch,Operation}};
use std::{path::PathBuf,fs,sync::atomic::{AtomicU64,Ordering}};
fn root()->PathBuf{PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")}
fn object(id:&str,parent:Option<&str>,geometry:Geometry)->Object{Object{id:id.into(),page_id:"page-1".into(),parent_id:parent.map(str::to_string),x_mm:3.,y_mm:7.,width_mm:20.,height_mm:10.,transform:[1.,0.,0.,1.,0.,0.],style:Style{fill:"#123456".into(),stroke:"none".into(),..Default::default()},visible:true,locked:false,clip:None,geometry}}
fn scene()->Document{
 let mut d=Document::new("scene".into(),210.,297.).unwrap();
 let mut group=object("outer",None,Geometry::Group);group.transform=[0.9,0.2,-0.3,1.1,40.,55.];
 let mut nested=object("inner",Some("outer"),Geometry::Group);nested.transform=[-1.,0.1,0.4,0.8,60.,20.];
 let rect=object("rect",Some("outer"),Geometry::Rect{radius_mm:0.});let ellipse=object("ellipse",Some("inner"),Geometry::Ellipse);
 let mut curve=object("curve",Some("inner"),Geometry::Path{d:"M0 0 C10 0 20 10 15 20 L0 10 Z".into(),fill_rule:Default::default()});curve.x_mm=25.;
 // Parent IDs are independent of the global array's ordering.
 d.objects=vec![ellipse,group,rect,nested,curve,object("top",None,Geometry::Rect{radius_mm:0.})];d
}
fn dir()->PathBuf{static N:AtomicU64=AtomicU64::new(0);let p=root().join(format!("artifacts/cdr-groups/{}-{}",std::process::id(),N.fetch_add(1,Ordering::Relaxed)));fs::create_dir_all(&p).unwrap();fs::canonicalize(p).unwrap()}
fn near(a:f64,b:f64){assert!((a-b).abs()<0.0003,"{a} != {b}")}
#[test]
fn nested_group_hierarchy_world_geometry_and_sibling_order_survive(){
 let d=scene();let hash=d.hash();let (bytes,warnings)=cdr::encode_document(&d).unwrap();assert!(warnings.iter().any(|w|w.code=="CDR_GROUP_NORMALIZED"));assert_eq!(d.hash(),hash);assert_eq!(bytes,cdr::encode_document(&d).unwrap().0);
 let (out,warnings)=cdr::decode(&bytes,"out").unwrap();assert!(warnings.iter().any(|w|w.code=="CDR_GROUP_NORMALIZED"));assert_eq!(out.objects.len(),6);
 for o in &d.objects {let imported=out.object(&o.id).unwrap();assert_eq!(imported.parent_id,o.parent_id);for (a,b) in d.bounds(o).unwrap().into_iter().zip(out.bounds(imported).unwrap()){near(a,b);}}
 for parent in [None,Some("outer"),Some("inner")] {assert_eq!(d.objects.iter().filter(|o|o.parent_id.as_deref()==parent).map(|o|&o.id).collect::<Vec<_>>(),out.objects.iter().filter(|o|o.parent_id.as_deref()==parent).map(|o|&o.id).collect::<Vec<_>>());}
 let again=cdr::decode(&cdr::encode_document(&out).unwrap().0,"out").unwrap().0;assert_eq!(out.hash(),again.hash());
}
#[test]
fn imported_group_translation_is_one_edit_and_undo_restores_all_members(){
 let path=dir().join("groups.cdr");fs::write(&path,cdr::encode_document(&scene()).unwrap().0).unwrap();let s=Service::new();s.command(Command::CdrImport{document_id:"main".into(),path:path.to_string_lossy().into()}).unwrap();let before=s.snapshot("main").unwrap();
 s.command(Command::Batch{document_id:"main".into(),expected_revision:0,unit:"mm".into(),operations:vec![Operation::ObjectUpdate{object_id:"outer".into(),patch:ObjectPatch{x_mm:Some(5.),y_mm:Some(8.),..Default::default()}}]}).unwrap();let after=s.snapshot("main").unwrap();
 let group=before.document.object("outer").unwrap();assert_eq!([group.x_mm,group.y_mm],[3.,7.]);
 let dx=5.-group.x_mm;let dy=8.-group.y_mm;let delta=[group.transform[0]*dx+group.transform[2]*dy,group.transform[1]*dx+group.transform[3]*dy];
 for id in ["rect","ellipse","curve"] {let a=before.document.bounds(before.document.object(id).unwrap()).unwrap();let b=after.document.bounds(after.document.object(id).unwrap()).unwrap();near(b[0]-a[0],delta[0]);near(b[1]-a[1],delta[1]);}
 s.command(Command::Undo{document_id:"main".into(),expected_revision:1}).unwrap();assert_eq!(s.snapshot("main").unwrap().document_hash,before.document_hash);
}
#[test]
fn actual_corel_resaved_negative_width_remains_an_editable_mirrored_rectangle(){
 let bytes=fs::read(root().join("examples/assets/cdr/corel2019/group-negative-width-v14.cdr")).unwrap();
 let d=cdr::decode(&bytes,"native").unwrap().0;assert_eq!(d.objects.len(),8);
 let rect=d.object("v-sheared-rectangle").unwrap();assert_eq!(rect.parent_id.as_deref(),Some("nested"));assert!(matches!(rect.geometry,Geometry::Rect{radius_mm:0.}));
 near(rect.width_mm,34.6455);near(rect.height_mm,15.2946);
 // Actual Corel 2019 SDK bounds after moving the full group and SaveAs14.
 let expected=[-0.0915,224.04,37.9215,19.6605];
 for (a,b) in d.bounds(rect).unwrap().into_iter().zip(expected){near(a,b);}
 let reopened=cdr::decode(&cdr::encode_document(&d).unwrap().0,"native").unwrap().0;
 for (a,b) in reopened.bounds(reopened.object("v-sheared-rectangle").unwrap()).unwrap().into_iter().zip(expected){near(a,b);}
}
#[test]
fn actual_corel_plain_group_without_bookkeeping_transform_is_imported(){
 let bytes=fs::read(root().join("examples/assets/cdr/corel2019/group-identity-no-matrix-v14.cdr")).unwrap();
 let d=cdr::decode(&bytes,"native").unwrap().0;assert_eq!(d.objects.len(),3);assert!(matches!(d.object("outer").unwrap().geometry,Geometry::Group));
 // Corel SDK measurements from the saved file, not intended creation inputs.
 for (id,expected) in [("rect",[19.9287,77.0053,60.,24.]),("ellipse",[49.9287,83.0053,30.,18.])] {
  let o=d.object(id).unwrap();assert_eq!(o.parent_id.as_deref(),Some("outer"));
  for (a,b) in d.bounds(o).unwrap().into_iter().zip(expected){near(a,b);}
 }
 let reopened=cdr::decode(&cdr::encode_document(&d).unwrap().0,"native").unwrap().0;
 for id in ["rect","ellipse"] {assert_eq!(reopened.object(id).unwrap().parent_id,d.object(id).unwrap().parent_id);for (a,b) in d.bounds(d.object(id).unwrap()).unwrap().into_iter().zip(reopened.bounds(reopened.object(id).unwrap()).unwrap()){near(a,b);}}
}
#[test]
fn translated_group_text_and_cross_group_image_instances_remain_live_shared(){
 let mut d=Document::new("scene".into(),210.,297.).unwrap();let mut a=object("a",None,Geometry::Group);a.x_mm=40.;let mut b=object("b",None,Geometry::Group);b.y_mm=80.;
 let text=object("text",Some("a"),Geometry::Text{content:"分组 中文 42".into(),font_family:"Microsoft YaHei".into(),font_size_mm:5.,text_path:None,layout:Default::default()});
 let r=Resource::image(&fs::read(root().join("examples/assets/cdr/content14/source-assets/alpha-37x29.png")).unwrap()).unwrap();d.resources.insert("shared".into(),r);
 d.objects=vec![a,b,text,object("one",Some("a"),Geometry::Image{resource_id:"shared".into(),clip_mm:None,fit:ImageFit::Stretch}),object("two",Some("b"),Geometry::Image{resource_id:"shared".into(),clip_mm:None,fit:ImageFit::Stretch})];
 let out=cdr::decode(&cdr::encode_document(&d).unwrap().0,"out").unwrap().0;assert_eq!(out.resources.len(),1);assert!(matches!(&out.object("text").unwrap().geometry,Geometry::Text{content,..} if content=="分组 中文 42"));
 for id in ["text","one","two"] {for (a,b) in d.bounds(d.object(id).unwrap()).unwrap().into_iter().zip(out.bounds(out.object(id).unwrap()).unwrap()){near(a,b);}}
}
#[test]
fn affine_group_text_and_image_keep_baseline_pixels_and_world_matrix(){
 let mut d=scene();let r=Resource::image(&fs::read(root().join("examples/assets/cdr/content14/source-assets/alpha-37x29.png")).unwrap()).unwrap();d.resources.insert("image".into(),r);
 for (id,geometry) in [("live-text",Geometry::Text{content:"中文 ABC é\n第二行".into(),font_family:"Microsoft YaHei".into(),font_size_mm:5.,text_path:None,layout:Default::default()}),("live-image",Geometry::Image{resource_id:"image".into(),clip_mm:None,fit:ImageFit::Stretch})] {d.objects.push(object(id,Some("inner"),geometry));}
 let out=cdr::decode(&cdr::encode_document(&d).unwrap().0,"out").unwrap().0;
 for id in ["live-text","live-image"] {let a=d.object(id).unwrap();let b=out.object(id).unwrap();for (a,b) in d.world_transform(a).unwrap().as_coeffs().into_iter().zip(out.world_transform(b).unwrap().as_coeffs()){near(a,b);}assert_eq!(std::mem::discriminant(&a.geometry),std::mem::discriminant(&b.geometry));}
}
#[test]
fn clipped_group_or_member_preserves_editable_contents_and_world_clip(){
 for target in ["outer","inner","rect","ellipse"] {
  let mut d=scene();d.objects.iter_mut().find(|o|o.id==target).unwrap().clip=Some(Clip::Rect{rect_mm:[0.,0.,10.,10.]});let hash=d.hash();
  let reopened=cdr::decode(&cdr::encode_document(&d).unwrap().0,"out").unwrap().0;assert_eq!(d.hash(),hash);assert_eq!(d.objects.len(),reopened.objects.len());
  let a=d.object(target).unwrap();let b=reopened.object(target).unwrap();assert!(b.clip.is_some());assert_eq!(a.parent_id,b.parent_id);
  let expected=d.world_transform(a).unwrap()*a.clip.as_ref().unwrap().path().unwrap();let actual=reopened.world_transform(b).unwrap()*b.clip.as_ref().unwrap().path().unwrap();

  use kurbo::Shape;let x=expected.bounding_box();let y=actual.bounding_box();for (a,b) in [x.x0,x.y0,x.x1,x.y1].into_iter().zip([y.x0,y.y0,y.x1,y.y1]) {near(a,b);}
  for id in ["rect","ellipse","curve"] {for (a,b) in d.bounds(d.object(id).unwrap()).unwrap().into_iter().zip(reopened.bounds(reopened.object(id).unwrap()).unwrap()){near(a,b);}}
 }
}
#[test]
fn actual_native_powerclip_frames_contents_and_nested_clips_remain_editable(){
 for name in ["clip-rect","clip-path","clip-nested","clip-image"] {
  let path=root().join(format!("reports/windows/WIN-CDR-GROUPS-20261023-01/references/{name}-v14.cdr"));
  let (d,warnings)=cdr::decode(&fs::read(path).unwrap(),"native").unwrap();assert!(warnings.iter().any(|w|w.code=="CDR_CLIP_NORMALIZED"));
  assert_eq!(d.objects.iter().filter(|o|o.clip.is_some()).count(),if name=="clip-nested" {2}else{1});
  let again=cdr::decode(&cdr::encode_document(&d).unwrap().0,"out").unwrap().0;assert_eq!(d.objects.len(),again.objects.len());
  assert_eq!(d.objects.iter().filter(|o|o.clip.is_some()).count(),again.objects.iter().filter(|o|o.clip.is_some()).count());
 }
}
#[test]
fn transparent_singular_or_too_deep_groups_are_explicitly_unsupported(){
 let mut d=scene();for change in 0..2 {let mut bad=d.clone();let o=bad.objects.iter_mut().find(|o|o.id=="outer").unwrap();match change{0=>o.style.opacity=0.7,_=>o.transform=[0.;6]};assert!(cdr::encode_document(&bad).is_err());}
 d.objects.push(object("empty",None,Geometry::Group));let restored=cdr::decode(&cdr::encode_document(&d).unwrap().0,&d.id).unwrap().0;assert!(matches!(restored.object("empty").unwrap().geometry,Geometry::Group));
 let mut deep=Document::new("deep".into(),210.,297.).unwrap();for i in 0..18 {let parent=if i>0 {Some(format!("g{}",i-1))}else{None};deep.objects.push(object(&format!("g{i}"),parent.as_deref(),Geometry::Group));}deep.objects.push(object("leaf",Some("g17"),Geometry::Ellipse));assert!(cdr::encode_document(&deep).unwrap_err().message.contains("16 levels"));
}

#[test]
fn actual_corel_nested_save_uses_static_ids_independent_of_stream_keys(){
 let bytes=fs::read(root().join("examples/assets/cdr/corel2019/nested-clips-independent-ids-v14.cdr")).unwrap();
 let d=cdr::decode(&bytes,"native").unwrap().0;
 assert_eq!(d.objects.iter().filter(|o|o.clip.is_some()).count(),3);
 for (id,parent) in [("outer",None),("inner",Some("outer")),("content",Some("inner"))] {
  assert_eq!(d.object(id).unwrap().parent_id.as_deref(),parent);
  assert!(d.object(id).unwrap().clip.is_some());
 }
 let again=cdr::decode(&cdr::encode_document(&d).unwrap().0,"native").unwrap().0;
 assert_eq!(again.objects.iter().filter(|o|o.clip.is_some()).count(),3);
 for id in ["outer","inner","content"] {for (a,b) in d.bounds(d.object(id).unwrap()).unwrap().into_iter().zip(again.bounds(again.object(id).unwrap()).unwrap()){near(a,b);}}
}
