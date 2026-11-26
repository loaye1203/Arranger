//! Windows150 measured horizontal fitted glyph shear, no derived glyph cache.
//! Raw orientations0..3 preserve the font frame in this bounded family;4 does
//! not. Their SDK enumeration differs. Never extend this to a bent guide.
use super::{invalid,unsupported,Result};
use crate::{Document,Geometry,Object,TextMode};
use kurbo::{Affine,PathEl};
use std::collections::BTreeMap;

pub(super) fn factor(text:&Object)->Option<f64>{
    let[a,b,k,d,_,_]=text.transform;
    (a==1.&&b==0.&&d==1.&&k!=0.&&k.is_finite()&&k.abs()<=1.).then_some(k)
}
fn baseline(doc:&Document,text:&Object)->Result<(f64,f64)>{
    let Geometry::Text{content,layout,text_path:Some(binding),..}=&text.geometry else{return Err(unsupported("native shear requires an editable fitted point story"))};
    if layout.mode!=TextMode::Point||content.is_empty()||content.contains(['\r','\n'])||text.clip.is_some()||text.style.stroke!="none"||text.style.opacity!=1.||text.style.fill=="none"||text.style.fill.len()==9{
        return Err(unsupported("native fitted shear needs one opaque filled point paragraph without an outline or clip"));
    }
    let guide=doc.object(&binding.path_id)?;
    if guide.parent_id!=text.parent_id{return Err(unsupported("native fitted shear text and guide must share a parent"));}
    let Geometry::Path{d,..}=&guide.geometry else{return Err(unsupported("native fitted shear guide must be a curve"))};
    let path=crate::path::parse(d)?;let[PathEl::MoveTo(p),PathEl::LineTo(q)]=path.elements()else{return Err(unsupported("native fitted shear requires one horizontal open line"))};
    let m=Affine::new(guide.transform)*Affine::translate((guide.x_mm,guide.y_mm));let(p,q)=(m* *p,m* *q);
    if p.y!=q.y||q.x<=p.x{return Err(unsupported("native fitted shear requires a left-to-right horizontal guide"));}
    let parent=text.parent_id.as_ref().map(|id|doc.world_transform(doc.object(id)?)).transpose()?.unwrap_or(Affine::IDENTITY);
    let[a,b,c,d,_,f]=parent.as_coeffs();
    if a!=d||b!=0.||c!=0.||a<=0.||!a.is_finite(){return Err(unsupported("native fitted shear parent must be a positive axis-aligned uniform scale"));}
    let mut parent_id=text.parent_id.as_deref();while let Some(id)=parent_id{let o=doc.object(id)?;if o.style.opacity!=1.||o.clip.is_some(){return Err(unsupported("native fitted shear ancestor transparency/clip is not verified"));}parent_id=o.parent_id.as_deref();}
    let local=p.y+binding.baseline_offset_mm;let world=a*local+f;
    if !local.is_finite()||!world.is_finite(){return Err(invalid("native fitted shear baseline is not finite"));}
    let run=crate::text::layout(doc,text)?;
    let advance=run.advance_mm;let length=q.x-p.x;let start=binding.start_offset_mm-match binding.anchor{crate::TextAnchor::Start=>0.,crate::TextAnchor::Middle=>advance/2.,crate::TextAnchor::End=>advance};
    if run.overflow||start<0.||start+advance>length{return Err(unsupported("native fitted shear crosses a live guide endpoint"));}
    Ok((local,world))
}
/// Validate a source chart exactly, before emitting or normalizing anything.
pub(super) fn plan(doc:&Document,text:&Object)->Result<Option<(f64,f64)>>{
    let Some(k)=factor(text)else{return Ok(None)};let(local,world)=baseline(doc,text)?;
    if text.x_mm!=0.||text.y_mm!=0.||text.transform[4]!= -k*local||text.transform[5]!=0.{return Err(unsupported("native fitted shear must fix the declared baseline with zero local origin"));}
    Ok(Some((k,world)))
}
pub(super) fn check(doc:&Document)->Result<()>{for text in &doc.objects{if matches!(text.geometry,Geometry::Text{text_path:Some(_),..}){plan(doc,text)?;}}Ok(())}
pub(super) fn effective(k:f64,baseline:f64)->[f64;6]{[1.,0.,k,1.,-k*baseline,0.]}
pub(super) fn native_matrix(doc:&Document,text:&Object)->Result<[f64;6]>{Ok(match plan(doc,text)?{Some((k,_))=>[1.,-k,0.,0.,1.,0.],None=>[1.,0.,0.,0.,1.,0.]})}
/// Deferred native coefficient: the linked guide must be known before we can
/// prove that the measured direction/frame has the model's affine meaning.
pub(super) fn frame(story:[f64;6],matrix:[f64;6],orientation:u32)->Result<Option<f64>>{
    if orientation==4{super::path_text::check_frame(story,matrix)?;return Ok(None)}
    if orientation>3||story.into_iter().zip(matrix).any(|(a,b)|!a.is_finite()||a!=b)||matrix[0]!=1.||matrix[3]!=0.||matrix[4]!=1.||matrix[1].abs()>1.{return Err(unsupported("native fitted shear frame/direction is outside the measured profile"));}
    Ok(Some(-matrix[1]))
}
pub(super) fn validate_imports(doc:&Document,frames:&BTreeMap<String,f64>)->Result<()>{
    for(id,k)in frames{if !k.is_finite()||k.abs()>1.{return Err(invalid("non-finite native fitted shear"));}baseline(doc,doc.object(id)?)?;}Ok(())
}
pub(super) fn restore(doc:&mut Document,frames:&BTreeMap<String,f64>)->Result<()>{
    // Source charts/fit scalars have already passed their normal active-native
    // guards. Use the CURRENT coefficient and baseline, never old Notes' Own.
    for(id,k)in frames{let(local,_)=baseline(doc,doc.object(id)?)?;let text=doc.objects.iter_mut().find(|o|&o.id==id).ok_or_else(||invalid("native fitted shear target missing"))?;text.x_mm=0.;text.y_mm=0.;text.transform=effective(*k,local);}
    Ok(())
}

#[cfg(test)]mod tests{
    use super::*;
    use super::super::binary::*;
    fn source(k:f64,parent:bool,offset:f64)->Document{
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/assets/cdr/straight-shear-20261106/recipe.json");
        let recipe:serde_json::Value=serde_json::from_str(&std::fs::read_to_string(root).unwrap()).unwrap();
        let mut doc:Document=serde_json::from_value(recipe["cases"][2]["document"].clone()).unwrap();
        let text=doc.objects.iter_mut().find(|o|o.id=="content").unwrap();if let Geometry::Text{text_path:Some(p),..}=&mut text.geometry{p.baseline_offset_mm=offset;}text.transform=effective(k,80.+offset);
        if parent{for o in &mut doc.objects{o.parent_id=Some("parent".into());}let mut p=doc.objects[2].clone();p.id="parent".into();p.parent_id=None;p.geometry=Geometry::Group;p.transform=[2.,0.,0.,2.,3.25,1.5];p.x_mm=0.;p.y_mm=0.;p.width_mm=10.;p.height_mm=10.;p.style=crate::Style::default();doc.objects.push(p);}doc
    }
    fn encode(doc:&Document,modern:bool)->Vec<u8>{if modern{crate::cdr::encode_document21(doc)}else{crate::cdr::encode_document(doc)}.unwrap().0}
    fn text_mut(doc:&mut Document)->&mut Object{doc.objects.iter_mut().find(|o|o.id=="content").unwrap()}
    fn edit(nodes:&mut[Node],modern:bool,k:f64,orientation:u32,both:bool){
        for n in nodes{if n.kind()==Some(b"obj "){
            let(_,args)=super::super::write::arguments(&n.find(b"loda").unwrap().data).unwrap();if args.iter().find(|(k,_)|*k==1000).is_some_and(|(_,v)|read_wide(v).unwrap()=="content"){
                let raw=&mut n.find_mut(b"trfd").unwrap().data;let at=u32_at(raw,u32_at(raw,8).unwrap()as usize).unwrap()as usize+16;raw[at+8..at+16].copy_from_slice(&(-k).to_le_bytes());
                let t=&mut n.find_mut(b"txsm").unwrap().data;let at=if modern{49}else{44};if both{t[at+8..at+16].copy_from_slice(&(-k).to_le_bytes());}set32(t,at+56,orientation).unwrap();
            }
        }if let Some(c)=&mut n.children{edit(c,modern,k,orientation,both);}}
    }
    fn pack(mut n:NativeRecords)->Vec<u8>{if n.version==2100{repack21(&n).unwrap()}else{n.members.insert("content/riffData.cdr".into(),write(&n.nodes).unwrap());zip(&n.members.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>())}}
    #[test]fn complete_horizontal_shear_preserves_source_and_two_exchanges(){
        for modern in[false,true]{for parent in[false,true]{for k in[-0.3249196962329004,0.25,0.3249196962329004]{for offset in[0.,1.25]{
            let source=source(k,parent,offset);let bytes=encode(&source,modern);let decoded=crate::cdr::decode(&bytes,&source.id).unwrap().0;assert_eq!(decoded,source,"modern{modern}parent{parent}k{k}offset{offset}");
            let second=encode(&decoded,modern);assert_eq!(crate::cdr::decode(&second,&source.id).unwrap().0,source);
            let bare=super::super::read::decode_geometry(&bytes,&source.id).unwrap().0;assert_eq!(bare.object("content").unwrap().transform[2],k);assert!(matches!(bare.object("content").unwrap().geometry,Geometry::Text{text_path:Some(_),..}));
        }}}}
    }
    #[test]fn native_coefficient_and_clear_always_override_old_source_own(){
        for modern in[false,true]{for parent in[false,true]{let source=source(0.25,parent,1.25);let bytes=encode(&source,modern);
            for (k,orientation)in[(0.25+1e-12,0),(0.,0),(0.,4)]{let mut n=read_native(&bytes).unwrap();edit(&mut n.nodes,modern,k,orientation,true);let doc=crate::cdr::decode(&pack(n),&source.id).unwrap().0;assert_eq!(doc.object("content").unwrap().transform[2],k);if k==0.{assert_eq!(doc.object("content").unwrap().transform,Affine::IDENTITY.as_coeffs());}}
        }}
    }
    #[test]fn missing_notes_keeps_the_effective_native_shear(){
        for modern in[false,true]{let source=source(-0.25,false,1.25);let mut n=read_native(&encode(&source,modern)).unwrap();n.members.remove("metadata/metadata.xml");n.members.remove("META-INF/metadata.xml");let doc=crate::cdr::decode(&pack(n),&source.id).unwrap().0;assert_eq!(doc.object("content").unwrap().transform,effective(-0.25,81.25));}
    }
    #[test]fn original_direction_four_and_mismatched_frames_still_reject(){
        for modern in[false,true]{let source=source(0.25,false,0.);let bytes=encode(&source,modern);for(mode,orientation,both)in[(0,4,true),(1,5,true),(2,0,false)]{let mut n=read_native(&bytes).unwrap();edit(&mut n.nodes,modern,if both{0.25}else{0.25+1e-12},orientation,both);assert!(crate::cdr::decode(&pack(n),"reject").is_err(),"mode{mode}");}}
    }
    #[test]fn unproved_source_guide_origin_paint_and_parent_are_rejected(){
        for mode in 0..9{let mut doc=source(0.25,true,0.);match mode{
            0=>text_mut(&mut doc).transform[4]+=1e-9,
            1=>text_mut(&mut doc).x_mm=0.0001,
            2=>text_mut(&mut doc).style.stroke="#123456".into(),
            3=>text_mut(&mut doc).style.opacity=0.5,
            4=>doc.objects.iter_mut().find(|o|o.id=="guide").unwrap().geometry=Geometry::Path{d:"M20 80L180 80.0001".into(),fill_rule:Default::default()},
            5=>doc.objects.iter_mut().find(|o|o.id=="guide").unwrap().geometry=Geometry::Path{d:"M20 80C40 80 100 80 180 80".into(),fill_rule:Default::default()},
            6=>doc.objects.iter_mut().find(|o|o.id=="parent").unwrap().transform=[0.,2.,-2.,0.,3.25,1.5],
            7=>doc.objects.iter_mut().find(|o|o.id=="parent").unwrap().style.opacity=0.5,
            _=>if let Geometry::Text{text_path:Some(p),..}=&mut text_mut(&mut doc).geometry{p.start_offset_mm=159.;},
        }for modern in[false,true]{assert!(if modern{crate::cdr::encode_document21(&doc)}else{crate::cdr::encode_document(&doc)}.is_err(),"mode{mode}");}}
    }
    fn glyph_controls(doc:&Document)->Vec<[f64;2]>{let text=doc.object("content").unwrap();let path=doc.world_transform(text).unwrap()*crate::text::layout(doc,text).unwrap().outline.clone();let mut p=vec![];for e in path.elements(){match *e{PathEl::MoveTo(a)|PathEl::LineTo(a)=>p.push([a.x,a.y]),PathEl::QuadTo(a,b)=>p.extend([[a.x,a.y],[b.x,b.y]]),PathEl::CurveTo(a,b,c)=>p.extend([[a.x,a.y],[b.x,b.y],[c.x,c.y]]),PathEl::ClosePath=>{}}}p}
    #[test]fn a_native_subgrid_guide_move_changes_the_current_shear_baseline(){
        for modern in[false,true]{let source=source(0.25,true,1.25);let mut n=read_native(&encode(&source,modern)).unwrap();
            fn shift(nodes:&mut[Node]){for n in nodes{if n.kind()==Some(b"obj "){let(_,a)=super::super::write::arguments(&n.find(b"loda").unwrap().data).unwrap();if a.iter().find(|(k,_)|*k==1000).is_some_and(|(_,v)|read_wide(v).unwrap()=="guide"){let raw=&mut n.find_mut(b"trfd").unwrap().data;let at=u32_at(raw,u32_at(raw,8).unwrap()as usize).unwrap()as usize+16+40;let y=f64_at(raw,at).unwrap();raw[at..at+8].copy_from_slice(&(y+0.001).to_le_bytes());}}if let Some(c)=&mut n.children{shift(c);}}}
            shift(&mut n.nodes);let bytes=pack(n);let doc=crate::cdr::decode(&bytes,&source.id).unwrap().0;assert_ne!(doc.object("content").unwrap().transform[4],source.object("content").unwrap().transform[4]);let native=super::super::read::decode_geometry(&bytes,"bare").unwrap().0;let a=glyph_controls(&doc);let b=glyph_controls(&native);assert_eq!(a.len(),b.len());assert!(a.iter().zip(b).all(|(a,b)|a.into_iter().zip(b).all(|(a,b)|(a-b).abs()<=0.00021)));
        }
    }
    #[test]fn losing_only_the_text_local_hint_keeps_native_world_glyph_controls(){
        for modern in[false,true]{let source=source(-0.25,true,1.25);let mut n=read_native(&encode(&source,modern)).unwrap();let prefix="ArrangerLocalFramesV1|sha256:";let old=super::super::metadata::find(&n.members,prefix).unwrap().unwrap();let mut hint:serde_json::Value=super::super::metadata::decode(prefix,&old).unwrap();hint["frames"].as_object_mut().unwrap().remove("content");let new=super::super::metadata::encode(prefix,&hint).unwrap();let key=if modern{"META-INF/metadata.xml"}else{"metadata/metadata.xml"};let xml=String::from_utf8(n.members[key].clone()).unwrap().replacen(&old,&new,1);n.members.insert(key.into(),xml.into_bytes());let bytes=pack(n);let doc=crate::cdr::decode(&bytes,&source.id).unwrap().0;let native=super::super::read::decode_geometry(&bytes,"bare").unwrap().0;let a=glyph_controls(&doc);let b=glyph_controls(&native);assert_eq!(a.len(),b.len());assert!(a.iter().zip(b).all(|(a,b)|a.into_iter().zip(b).all(|(a,b)|(a-b).abs()<=0.00021)));
        }
    }

}
