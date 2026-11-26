//! Fitted text stays one editable story with one original guide and one effect.
use super::{binary::*,*};
use crate::{Document,Clip};

pub(super) fn source(parent:bool,curve:bool,own:f64,part:u8)->Document{
    let mut doc:Document=serde_json::from_value(serde_json::json!({"id":"path-appearance","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[
        {"id":"guide","page_id":"page-1","x_mm":35.,"y_mm":70.,"width_mm":120.,"height_mm":20.,"geometry":{"kind":"path","d":if curve{"M0 0C40 16 80 -16 120 0"}else{"M0 0L120 0"}},"style":{"fill":"none","stroke":"#888888","stroke_width_mm":0.2,"opacity":1.}},
        {"id":"text","page_id":"page-1","x_mm":0.,"y_mm":0.,"width_mm":60.,"height_mm":12.,"transform":[own,0.,0.,own,0.,0.],"geometry":{"kind":"text","content":"Live 42","font_family":"Arial","font_size_mm":4.,"layout":{"letter_spacing_mm":0.125,"line_height_mm":5.25},"text_path":{"path_id":"guide","start_offset_mm":25.,"baseline_offset_mm":0.125,"anchor":"start"}},"style":{"fill":if part==0{"rgba(30,90,180,0.37654321)"}else{"#1e5ab4"},"stroke":if part==1{"rgba(220,80,40,0.5)"}else{"#dc5028"},"stroke_width_mm":0.1234567,"opacity":1.}}
    ]})).unwrap();
    if parent{let group=serde_json::from_value(serde_json::json!({"id":"parent","page_id":"page-1","x_mm":3.,"y_mm":4.,"width_mm":0.,"height_mm":0.,"transform":[0.,1.7,-1.7,0.,0.,0.],"geometry":{"kind":"group"}})).unwrap();doc.objects.insert(0,group);doc.objects.iter_mut().find(|o|o.id=="text").unwrap().parent_id=Some("parent".into());}
    // Scale about the existing guide start, so this test does not introduce
    // a distinct endpoint-clipping regime when shrinking a parented story.
    let text=doc.objects.iter_mut().find(|o|o.id=="text").unwrap();let(e,f)=if parent{((1.-own)*(70.-4.)/1.7,-(1.-own)*(35.-3.)/1.7)}else{((1.-own)*35.,(1.-own)*70.)};text.transform[4]=e;text.transform[5]=f;doc
}
fn pack(native:NativeRecords,version:u32)->Vec<u8>{if version==14{write(&native.nodes).unwrap()}else{write21(&native.nodes,native.members).unwrap()}}
#[test]
fn scoped_path_paint_and_fine_pen_keep_one_story_original_guide_and_complete_source(){
    for parent in[false,true]{for curve in[false,true]{for part in[0,1,2]{for version in[14,21]{
        let source=source(parent,curve,1.,part);let encode=if version==14{encode_document}else{encode_document21};let bytes=encode(&source).unwrap().0;let restored=decode(&bytes,&source.id).unwrap().0;assert_eq!(restored,source,"parent{parent} curve{curve} part{part} v{version}");assert_eq!(decode(&encode(&restored).unwrap().0,&source.id).unwrap().0,source);
        let native=read_native(&bytes).unwrap();assert_eq!(all(&native.nodes).into_iter().filter(|n|n.tag==*b"txsm").count(),1);assert_eq!(path_text::Links::new(&native.nodes).unwrap().by_text.len(),1);
        let effects=all(&native.nodes).into_iter().filter_map(|n|if n.kind()==Some(b"obj "){Some(write::arguments(&read::own_record(n,b"loda").unwrap().data).unwrap().1)}else{None}).flatten().filter(|(k,_)|*k==8000).map(|(_,raw)|u32_at(&raw,89).unwrap()).collect::<Vec<_>>();assert_eq!(effects,if part<2{vec![u32::from(part)]}else{vec![]});
        let actual=read::decode_geometry(&bytes,"actual").unwrap().0;assert_eq!(actual.objects.len(),source.objects.len());assert!(path_outline::preserves(&actual,actual.object("text").unwrap(),&source,source.object("text").unwrap()).unwrap());
    }}}}
}
#[test]
fn scoped_path_paint_composes_with_existing_own_scale_without_copying_the_guide(){
    for parent in[false,true]{for own in[0.5,1.5]{for part in[0,1]{for encode in[encode_document,encode_document21]{
        let source=source(parent,false,own,part);let bytes=encode(&source).unwrap().0;assert_eq!(decode(&bytes,&source.id).unwrap().0,source);let actual=read::decode_geometry(&bytes,"actual").unwrap().0;assert_eq!(actual.objects.len(),source.objects.len());assert!(path_outline::preserves(&actual,actual.object("text").unwrap(),&source,source.object("text").unwrap()).unwrap());
    }}}}
}
#[test]
fn native_fitted_pen_and_paint_edits_are_not_replaced_by_old_scalars(){
    for parent in[false,true]{for version in[14,21]{for mode in 0..6{
        let source=source(parent,true,1.,0);let encode=if version==14{encode_document}else{encode_document21};let old=read_native(&encode(&source).unwrap().0).unwrap();let mut model=write::normalized(&source).unwrap();let o=model.objects.iter_mut().find(|o|o.id=="text").unwrap();o.style.fill=source.object("text").unwrap().style.fill.clone();o.style.opacity=1.;
        match mode{0=>o.style.stroke_width_mm+=0.0001,1=>{o.style.opacity=1.;o.style.fill="rgba(30,90,180,0.5)".into();},2=>{o.style.opacity=1.;o.style.fill="#abcdef".into();},3=>{o.style.stroke="none".into();o.style.stroke_width_mm=0.;},4=>{o.style.fill="#1e5ab4".into();o.style.stroke="rgba(220,80,40,0.5)".into();},_=>if let crate::Geometry::Text{text_path:Some(p),..}=&mut o.geometry{p.start_offset_mm+=0.0001;}else{panic!()}}
        // Re-encode a consistent native edit, then retain the original Notes.
        let mut changed=read_native(&encode(&model).unwrap().0).unwrap();for key in["metadata/metadata.xml","META-INF/metadata.xml"]{if let Some(raw)=old.members.get(key){changed.members.insert(key.into(),raw.clone());}else{changed.members.remove(key);}}
        let bytes=pack(changed,version);let restored=decode(&bytes,"edited").unwrap().0;let actual=read::decode_geometry(&bytes,"actual").unwrap().0;let(a,b)=(restored.object("text").unwrap(),actual.object("text").unwrap());assert_eq!(a.style.stroke,b.style.stroke);assert!(path_outline::preserves(&restored,a,&actual,b).unwrap());
        if mode==0{assert_ne!(a.style.stroke_width_mm,source.object("text").unwrap().style.stroke_width_mm);}if mode==1{assert_eq!(crate::paint::parse(&a.style.fill).unwrap().alpha(),0.5);}if mode==2{assert_eq!(crate::paint::parse(&a.style.fill).unwrap().rgba(),[171,205,239,255]);assert_eq!(a.style.opacity,1.);}if mode==3{assert_eq!(a.style.stroke,"none");assert_eq!(a.style.stroke_width_mm,0.);}
        if mode==4{assert_eq!(crate::paint::parse(&a.style.fill).unwrap().alpha(),1.);assert_eq!(crate::paint::parse(&a.style.stroke).unwrap().alpha(),0.5);}if mode==5{assert_ne!(a.style.stroke_width_mm,source.object("text").unwrap().style.stroke_width_mm,"an edited fit keeps the native pen chart");}
    }}}
}
#[test]
fn unsupported_paint_composition_and_collapsed_fitted_pen_still_fail_closed(){
    for mode in 0..4{let mut doc=source(false,false,1.,0);let o=doc.objects.iter_mut().find(|o|o.id=="text").unwrap();match mode{0=>o.style.stroke="rgba(220,80,40,0.5)".into(),1=>o.style.opacity=0.5,2=>o.clip=Some(Clip::Rect{rect_mm:[0.,0.,200.,200.]}),_=>o.style.stroke_width_mm=0.0000001};for encode in[encode_document,encode_document21]{assert!(encode(&doc).is_err(),"mode{mode}");}}
}

#[test]
fn fitted_native21_binary_and_typed_paint_scopes_must_still_agree(){
    fn flip(nodes:&mut[Node])->bool{for n in nodes{
        if n.kind()==Some(b"obj "){let(kind,mut args)=write::arguments(&read::own_record(n,b"loda").unwrap().data).unwrap();if args.iter().any(|(k,v)|*k==1000&&read_wide(v).unwrap()=="text"){
            let raw=&mut args.iter_mut().find(|(k,_)|*k==8000).unwrap().1;let old=u32_at(raw,89).unwrap();set32(raw,89,1-old).unwrap();n.find_mut(b"loda").unwrap().data=write::loda(kind,&args);return true;
        }}if let Some(c)=&mut n.children{if flip(c){return true;}}
    }false}
    for part in[0,1]{let source=source(true,true,1.,part);let mut native=read_native(&encode_document21(&source).unwrap().0).unwrap();assert!(flip(&mut native.nodes));let error=decode(&pack(native,21),"bad-scope").unwrap_err();assert_eq!(error.code,"CDR_UNSUPPORTED");assert!(error.message.contains("scope"),"{error:?}");}
}
