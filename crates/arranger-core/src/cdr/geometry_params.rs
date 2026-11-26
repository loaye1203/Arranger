//! Bounded clip scalars and coordinate-free curve notation.
use super::{invalid,unsupported,Result,path_notation::Notation};
use crate::{Clip,Document,Geometry};
use kurbo::{Affine,BezPath};
use serde::{Deserialize,Serialize};
use std::collections::{BTreeMap,BTreeSet};
const PREFIX:&str="ArrangerGeometryParametersV1|sha256:";
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Parameters{version:u32,objects:BTreeMap<String,ParametersForObject>,#[serde(skip)]unrebased_paths:BTreeSet<String>,#[serde(skip)]native_clips:BTreeMap<String,String>}
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct ParametersForObject{#[serde(skip_serializing_if="Option::is_none")]path:Option<Notation>,#[serde(skip_serializing_if="Option::is_none")]clip:Option<ClipParameters>,#[serde(default,skip_serializing_if="Option::is_none")]path_units:Option<u32>,#[serde(default,skip_serializing_if="Option::is_none")]native_clip_sha256:Option<String>,#[serde(default,skip_serializing_if="Option::is_none")]native_clip_owner:Option<String>}
#[derive(Serialize,Deserialize)]
#[serde(tag="kind",rename_all="snake_case",deny_unknown_fields)]
enum ClipParameters{Rect{rect_mm:[f64;4]},Path{notation:Notation}}
#[cfg(test)]
pub(super) fn notes(doc:&Document)->Result<Option<String>> {notes_in_version(doc,false)}
fn clip_notation(d:&str)->Result<Option<Notation>> {
    // This scalar describes the source-local notation's grid, independently
    // of the frame's encoded chart. Every coordinate still comes from the
    // actual native curve after collapse/local-frame restoration.
    for units in std::iter::once(1).chain(super::path_precision::UNITS) {
        // A larger scalar may exceed the real signed point-table domain;
        // it is a failed candidate, not evidence that an exchange must fail.
        if let Ok(Some(notation))=Notation::from_source_in_units(d,units) {return Ok(Some(notation));}
    }Ok(None)
}
pub(super) fn notes_in_version(doc:&Document,modern:bool)->Result<Option<String>> {
    let mut objects=BTreeMap::new();
    // Hash the actual world-space point table emitted after normalization and
    // adaptive-unit lowering, rather than the source's pre-rounding clip.
    let native=if doc.objects.iter().any(|o|o.clip.is_some()) {let mut value=super::write::normalized(doc)?;super::path_precision::lower(doc,&mut value)?;Some(value)}else{None};
    for o in &doc.objects {
        let path_units=super::path_precision::plan(doc,o)?;
        let path=if let Geometry::Path{d,..}=&o.geometry{Notation::from_source_in_units(d,path_units.unwrap_or(1))?}else{None};
        let clip=match &o.clip{Some(Clip::Rect{rect_mm})=>Some(ClipParameters::Rect{rect_mm:*rect_mm}),Some(Clip::Path{d,..})=>clip_notation(d)?.map(|notation|ClipParameters::Path{notation}),None=>None};
        let(mut native_clip_sha256,mut native_clip_owner)=(None,None);
        if clip.is_some() {if let Some(native)=&native {
            let image=native.object(&o.id)?;
            // Cover/legacy image viewports introduce two distinct clips: the
            // bitmap's rectangular viewport, and the source clip carried by a
            // generated outer group. Bind the source clip's own native owner,
            // never the unrelated inner viewport (which can have another rule).
            let wrapped=matches!(&o.geometry,Geometry::Image{fit,clip_mm,..} if *fit==crate::ImageFit::Cover||clip_mm.is_some());
            let owner=if wrapped {native.object(image.parent_id.as_deref().ok_or_else(||invalid("normalized image clip owner absent"))?)?}else{image};
            if owner.clip.is_some() {
                let projected=super::clip_precision::projected_native_clip(doc,native,owner,modern)?;
                native_clip_sha256=Some(clip_hash(&projected,owner.clip.as_ref().unwrap().rule()));
                if owner.id!=o.id{native_clip_owner=Some(owner.id.clone());}
            }
        }}
        if path.is_some()||clip.is_some()||path_units.is_some(){objects.insert(o.id.clone(),ParametersForObject{path,clip,path_units,native_clip_sha256,native_clip_owner});}
    }
    if objects.is_empty(){return Ok(None);}super::metadata::encode(PREFIX,&Parameters{version:1,objects,unrebased_paths:BTreeSet::new(),native_clips:BTreeMap::new()}).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn fine_clip_metadata_contains_only_number_formats_units_and_projection_identity() {
        let d="M0.1234567 0 C1.1234567 2.7654321 3.3333333 -1.1111111 5 0 L5 6 L0 6 Z";
        let source:Document=serde_json::from_value(json!({"id":"notation-only","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"shape","page_id":"page-1","x_mm":20.25,"y_mm":30.5,"width_mm":12.,"height_mm":8.,"geometry":{"kind":"rect","radius_mm":0.},"clip":{"kind":"path","d":d}}]})).unwrap();
        for modern in [false,true] {let raw=notes_in_version(&source,modern).unwrap().unwrap();let value:serde_json::Value=super::super::metadata::decode(PREFIX,&raw).unwrap();let object=&value["objects"]["shape"];let notation=&object["clip"]["notation"];
            assert_eq!(notation["units"],1000);assert!(!notation["syntax"].as_str().unwrap().chars().any(|c|c.is_ascii_digit()));
            assert!(!object.to_string().contains("0.1234567"));assert!(object["clip"].get("d").is_none());assert_eq!(object["native_clip_sha256"].as_str().unwrap().len(),64);
        }
    }
    fn wrapped_image_source(fit:&str)->Document {
        let mut source:Document=serde_json::from_value(json!({"id":"image-clip-binding","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[
            {"id":"image-image-frame","page_id":"page-1","x_mm":0.,"y_mm":0.,"width_mm":0.,"height_mm":0.,"geometry":{"kind":"group"}},
            {"id":"image","parent_id":"image-image-frame","page_id":"page-1","x_mm":20.,"y_mm":30.,"width_mm":45.,"height_mm":40.,"geometry":{"kind":"image","resource_id":"source","fit":fit,"clip_mm":[0.,0.,45.,40.]},"clip":{"kind":"path","d":"M0 0H45V40H0Z M12 8H32V32H12Z","fill_rule":"evenodd"}}
        ]})).unwrap();source.resources.insert("source".into(),crate::Resource::image(include_bytes!("../../../../examples/assets/images/quadrants.jpg")).unwrap());source
    }
    #[test]
    fn generated_image_wrapper_hash_tracks_source_clip_instead_of_the_inner_viewport_mask() {
        for fit in ["cover","contain"] {let source=wrapped_image_source(fit);for version in [14,21] {
            let raw=if version==14{crate::cdr::encode_document(&source)}else{crate::cdr::encode_document21(&source)}.unwrap().0;let actual=crate::cdr::decode(&raw,&source.id).unwrap().0;assert_eq!(actual,source,"version{version} fit{fit}");
        }}
    }
    #[test]
    fn generated_image_wrapper_actual_native_point_and_double_frame_edits_remain_authoritative() {
        use super::super::binary::*;
        fn edit(nodes:&mut[Node],point:bool)->bool {for node in nodes {
            if node.kind()==Some(b"obj ") {let(kind,mut args)=super::super::write::arguments(&node.find(b"loda").unwrap().data).unwrap();
                if kind==3&&args.iter().any(|(k,_)|*k==8005)&&!args.iter().any(|(k,_)|*k==14001) {
                    if point {let data=&mut args.iter_mut().find(|(k,_)|*k==30).unwrap().1;let n=u16_at(data,0).unwrap() as usize;for i in 0..n{let at=4+8*i;let x=i32_at(data,at).unwrap();set32(data,at,(x+1) as u32).unwrap();}node.find_mut(b"loda").unwrap().data=super::super::write::loda(kind,&args);}
                    else{let data=&mut node.find_mut(b"trfd").unwrap().data;let at=u32_at(data,u32_at(data,8).unwrap() as usize).unwrap() as usize+16+2*8;let tx=f64_at(data,at).unwrap()+0.001;data[at..at+8].copy_from_slice(&tx.to_le_bytes());}return true;
                }
            }if let Some(children)=&mut node.children{if edit(children,point){return true;}}
        }false}
        fn pack(native:&mut NativeRecords)->Vec<u8>{if native.version==2100{repack21(native).unwrap()}else{native.members.insert("content/riffData.cdr".into(),write(&native.nodes).unwrap());zip(&native.members.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>())}}
        let source=wrapped_image_source("cover");for version in [14,21]{for point in [true,false] {
            let raw=if version==14{crate::cdr::encode_document(&source)}else{crate::cdr::encode_document21(&source)}.unwrap().0;let mut native=read_native(&raw).unwrap();assert!(edit(&mut native.nodes,point));let with_notes=pack(&mut native);
            native.members.remove("metadata/metadata.xml");native.members.remove("META-INF/metadata.xml");let reference=crate::cdr::decode(&pack(&mut native),"reference").unwrap().0;let(actual,warnings)=crate::cdr::decode(&with_notes,"actual").unwrap();
            let a=actual.object("image").unwrap();let b=reference.object("image-image-frame-1").unwrap();let(pa,pb)=(actual.world_transform(a).unwrap()*a.clip.as_ref().unwrap().path().unwrap(),reference.world_transform(b).unwrap()*b.clip.as_ref().unwrap().path().unwrap());
            assert!(super::super::path_precision::same_world(&pa,Affine::IDENTITY,&pb,Affine::IDENTITY));assert_eq!(pa.elements().len(),pb.elements().len());for(a,b)in pa.elements().iter().zip(pb.elements()){match(a,b){(kurbo::PathEl::MoveTo(a),kurbo::PathEl::MoveTo(b))|(kurbo::PathEl::LineTo(a),kurbo::PathEl::LineTo(b))=>assert!((a.x-b.x).abs()<1e-10&&(a.y-b.y).abs()<1e-10),(kurbo::PathEl::ClosePath,kurbo::PathEl::ClosePath)=>{},_=>panic!("unexpected edited clip segment")}}
            assert!(warnings.iter().any(|w|w.code=="CDR_GEOMETRY_PARAMETERS_NATIVE"));assert_ne!(a.clip,source.object("image").unwrap().clip);
        }}
    }
    #[test]
    fn legacy_v1_clip_parameters_without_native_projection_hash_remain_readable() {
        let source:Document=serde_json::from_value(json!({"id":"legacy-clip","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"shape","page_id":"page-1","x_mm":0.,"y_mm":0.,"width_mm":12.,"height_mm":8.,"geometry":{"kind":"rect","radius_mm":0.},"clip":{"kind":"rect","rect_mm":[1.,2.,3.,4.]}}]})).unwrap();
        let raw=notes(&source).unwrap().unwrap();let mut value:serde_json::Value=super::super::metadata::decode(PREFIX,&raw).unwrap();assert_eq!(value["objects"]["shape"]["native_clip_sha256"].as_str().unwrap().len(),64);
        value["objects"]["shape"].as_object_mut().unwrap().remove("native_clip_sha256");let params:Parameters=serde_json::from_value(value).unwrap();let mut native=source.clone();native.objects[0].clip=Some(Clip::Path{d:"M1 2L4 2L4 6L1 6L1 2Z".into(),fill_rule:Default::default()});assert_eq!(restore(&mut native,params).unwrap(),(1,0));assert_eq!(native,source);
    }
    #[test]
    fn amplified_native_clip_edits_are_checked_in_world_coordinates_before_notation_restore() {
        let source:Document=serde_json::from_value(json!({"id":"world-clip-edit","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"shape","page_id":"page-1","x_mm":0.,"y_mm":0.,"width_mm":1.,"height_mm":1.,"transform":[100.,0.,0.,100.,0.,0.],"geometry":{"kind":"rect","radius_mm":0.},"clip":{"kind":"path","d":"M0 0L1 0L1 1L0 1Z"}}]})).unwrap();
        let params=super::super::metadata::decode(PREFIX,&notes(&source).unwrap().unwrap()).unwrap();let mut native=source.clone();native.objects[0].clip=Some(Clip::Path{d:"M0 0L1.00001 0L1.00001 1L0 1Z".into(),fill_rule:Default::default()});let expected=native.clone();
        assert_eq!(restore(&mut native,params).unwrap(),(0,1));assert_eq!(native.objects,expected.objects);
    }
}
pub(super) fn read(members:&BTreeMap<String,Vec<u8>>)->Result<Option<Parameters>> {
    super::metadata::find(members,PREFIX)?.map(|s|super::metadata::decode(PREFIX,&s)).transpose()
}
fn clip_hash(path:&BezPath,rule:crate::FillRule)->String {
    // No quantization here: a real double-valued frame move, even below one
    // point-table unit, must prevent old clip scalars from replacing that edit.
    crate::model::sha256(format!("{}|{}",u8::from(rule.is_nonzero()),path.to_svg()).as_bytes())
}
/// Capture canonical native frame geometry before PowerClip collapse changes
/// its coordinate basis, and before source local-frame parameters are restored.
/// Only hashes cross that boundary; no source or native coordinate snapshot is
/// retained in Notes or in this runtime map.
pub(super) fn capture_native_clips(doc:&Document,frames:&[String],params:Option<&mut Parameters>)->Result<()> {
    let Some(params)=params else{return Ok(())};params.native_clips.clear();
    for id in frames {
        let frame=doc.object(id)?;let roots=doc.objects.iter().filter(|o|o.parent_id.as_deref()==Some(id)).collect::<Vec<_>>();
        if id.starts_with(super::clipping::PREFIX)&&roots.len()==1&&frame.style.fill=="none"&&frame.style.stroke=="none"&&frame.style.opacity==1. {
            let world=Affine::new(frame.transform)*Affine::translate((frame.x_mm,frame.y_mm));
            params.native_clips.insert(roots[0].id.clone(),clip_hash(&(world*frame.local_path()?),frame.fill_rule()));
        }
    }Ok(())
}
pub(super) fn restore_path_units(doc:&mut Document,params:Option<&mut Parameters>,scaled:&BTreeSet<String>,widths:&BTreeMap<String,f64>)->Result<(usize,usize)> {
    let Some(params)=params else{return Ok((0,0))};if params.version!=1{return Err(unsupported("unknown geometry parameter version"));}
    params.unrebased_paths.clear();
    let(mut rebased,mut native)=(0,0);
    for(id,p)in &params.objects {if let Some(units)=p.path_units {
        if !crate::model::valid_id(id){return Err(invalid("invalid native path unit target"));}
        if super::path_precision::rebase_with_outline(doc,id,units,widths.get(id).copied(),scaled.contains(id))?{rebased+=1;}else{native+=1;params.unrebased_paths.insert(id.clone());}
    }}Ok((rebased,native))
}
pub(super) fn restored_units(params:Option<&Parameters>)->BTreeMap<String,u32>{params.map(|p|p.objects.iter().filter(|(id,_)|!p.unrebased_paths.contains(*id)).filter_map(|(id,o)|o.path_units.map(|u|(id.clone(),u))).collect()).unwrap_or_default()}
pub(super) fn restore(doc:&mut Document,params:Parameters)->Result<(usize,usize)> {
    if params.version!=1{return Err(unsupported("unknown geometry parameter version"));}let mut restored=0;let mut native=0;
    for(id,p)in params.objects {
        if !crate::model::valid_id(&id){return Err(invalid("invalid geometry parameter name"));}
        let Some(index)=doc.objects.iter().position(|o|o.id==id)else{continue};
        let world=doc.world_transform(&doc.objects[index])?;
        if params.unrebased_paths.contains(&id){native+=usize::from(p.path.is_some());}
        else if let (Some(notation),Geometry::Path{d,..})=(p.path,&mut doc.objects[index].geometry){if let Some(source)=notation.restore(d)?{if super::path_notation::same_world_geometry(d,&source,world)?{*d=source;restored+=1;}else{native+=1;}}else{native+=1;}}
        if let(Some(parameters),Some(actual))=(p.clip,doc.objects[index].clip.clone()) {
            // Old V1 metadata has no hash and keeps the previous bounded check.
            // New files require the exact original native world projection;
            // changed native points or a double-valued frame move stay native.
            let owner=p.native_clip_owner.as_ref().unwrap_or(&id);if !crate::model::valid_id(owner){return Err(invalid("invalid native clip projection owner"));}
            if p.native_clip_sha256.as_ref().is_some_and(|expected|params.native_clips.get(owner)!=Some(expected)){native+=1;continue;}
            match parameters {
                ClipParameters::Path{notation}=>{let path=actual.path()?.to_svg();if let Some(d)=notation.restore(&path)?{if super::path_notation::same_world_geometry(&path,&d,world)?{doc.objects[index].clip=Some(Clip::Path{d,fill_rule:actual.rule()});restored+=1;}else{native+=1;}}else{native+=1;}},
                ClipParameters::Rect{rect_mm}=>{
                    let expected=Clip::Rect{rect_mm};expected.validate()?;
                    let world=doc.world_transform(&doc.objects[index])?;let a=world*actual.path()?;let b=world*expected.path()?;
                    if actual.rule().is_nonzero()&&super::image_params::same_rectangle(&a,&b){doc.objects[index].clip=Some(expected);restored+=1;}else{native+=1;}
                },
            }
        }
    }doc.validate()?;Ok((restored,native))
}
