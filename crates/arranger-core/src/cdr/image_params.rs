//! Source bitmap viewport scalars. Native pixels, masks and hierarchy own edits.
use super::{invalid,unsupported,Result};
use crate::{Clip,Document,Geometry,ImageFit};
use kurbo::{Affine,BezPath,PathEl,Point,Rect,Shape};
use serde::{Deserialize,Serialize};
use std::collections::{BTreeMap,BTreeSet};
const PREFIX:&str="ArrangerImagePlacementV1|sha256:";
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Parameters{version:u32,images:BTreeMap<String,Placement>}
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct Placement{fit:ImageFit,clip_mm:Option<[f64;4]>,position_mm:[f64;2],size_mm:[f64;2],matrix:[f64;6],had_clip:bool,#[serde(default,skip_serializing_if="Option::is_none")]generated_wrapper:Option<String>,#[serde(default,skip_serializing_if="Option::is_none")]source_index:Option<u32>}
pub(super) fn notes(doc:&Document)->Result<Option<String>> {
    let mut images=BTreeMap::new();let mut ids=doc.objects.iter().map(|o|o.id.clone()).collect::<BTreeSet<_>>();
    for (index,o) in doc.objects.iter().enumerate() {if let Geometry::Image{fit,clip_mm,..}=&o.geometry {if *fit!=ImageFit::Stretch||clip_mm.is_some(){
        let generated_wrapper=if o.clip.is_some()&&(*fit==ImageFit::Cover||clip_mm.is_some()){
            let base=format!("{}-image-frame",&o.id[..o.id.len().min(96)]);let mut id=base.clone();let mut n=0;while !ids.insert(id.clone()){n+=1;id=format!("{base}-{n}");}Some(id)
        }else{None};
        images.insert(o.id.clone(),Placement{fit:*fit,clip_mm:*clip_mm,position_mm:[o.x_mm,o.y_mm],size_mm:[o.width_mm,o.height_mm],matrix:o.transform,had_clip:o.clip.is_some(),generated_wrapper,source_index:Some(index as u32+1)});
    }}}
    if images.is_empty(){return Ok(None);}super::metadata::encode(PREFIX,&Parameters{version:1,images}).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn extracted_image_never_folds_a_real_source_group_with_a_generated_looking_name() {
        let mut source:Document=serde_json::from_value(json!({"id":"wrapper-collision","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[
            {"id":"image-image-frame","page_id":"page-1","x_mm":0.,"y_mm":0.,"width_mm":0.,"height_mm":0.,"geometry":{"kind":"group"},"clip":{"kind":"rect","rect_mm":[0.,0.,100.,100.]}},
            {"id":"image","parent_id":"image-image-frame","page_id":"page-1","x_mm":0.,"y_mm":0.,"width_mm":45.,"height_mm":40.,"geometry":{"kind":"image","resource_id":"source","fit":"cover"},"clip":{"kind":"rect","rect_mm":[0.,0.,45.,40.]}}
        ]})).unwrap();source.resources.insert("source".into(),crate::Resource::png(include_bytes!("../../../../examples/assets/images/quadrants.png")).unwrap());
        let params=super::super::metadata::decode(PREFIX,&notes(&source).unwrap().unwrap()).unwrap();let mut native=super::super::write::normalized(&source).unwrap();
        let wrapper=native.object("image").unwrap().parent_id.clone().unwrap();assert_eq!(wrapper,"image-image-frame-1");native.objects.retain(|o|o.id!=wrapper);native.objects.iter_mut().find(|o|o.id=="image").unwrap().parent_id=Some("image-image-frame".into());let expected=native.clone();
        assert_eq!(restore(&mut native,params).unwrap(),(0,1));assert_eq!(native.objects,expected.objects);
    }
}
pub(super) fn read(members:&BTreeMap<String,Vec<u8>>)->Result<Option<Parameters>> {
    super::metadata::find(members,PREFIX)?.map(|s|super::metadata::decode(PREFIX,&s)).transpose()
}
pub(super) fn source_names(params:&Parameters)->BTreeSet<String>{params.images.keys().cloned().collect()}
pub(super) fn source_indices(params:&Parameters)->BTreeMap<String,u32>{params.images.iter().filter_map(|(id,p)|p.source_index.map(|n|(id.clone(),n))).collect()}
fn close(a:Point,b:Point)->bool{(a.x-b.x).abs()<=0.00021&&(a.y-b.y).abs()<=0.00021}
fn corners(m:Affine,w:f64,h:f64)->[Point;4]{[(0.,0.),(w,0.),(w,h),(0.,h)].map(|p|m*Point::from(p))}
pub(super) fn same_rectangle(a:&BezPath,b:&BezPath)->bool {
    fn points(path:&BezPath)->Option<Vec<Point>> {
        let mut points=vec![];let mut closed=false;
        for el in path.elements(){match *el {
            PathEl::MoveTo(p) if points.is_empty()=>points.push(p),PathEl::LineTo(p) if !closed=>points.push(p),PathEl::ClosePath if !closed=>closed=true,_=>return None,
        }}
        if points.len()==5&&close(points[0],points[4]){points.pop();}
        (closed&&points.len()==4).then_some(points)
    }
    let (Some(a),Some(b))=(points(a),points(b))else{return false};
    (0..4).any(|shift|(0..4).all(|i|close(a[i],b[(i+shift)%4]))||(0..4).all(|i|close(a[i],b[(shift+4-i)%4])))
}
pub(super) fn restore(doc:&mut Document,params:Parameters)->Result<(usize,usize)> {
    if params.version!=1{return Err(unsupported("unknown native image placement version"));}
    let mut restored=0;let mut normalized=0;
    for(id,p)in params.images {
        if !crate::model::valid_id(&id)||p.position_mm.iter().chain(&p.size_mm).chain(&p.matrix).any(|n|!n.is_finite())||p.size_mm.iter().any(|n|*n<=0.){return Err(invalid("invalid image placement scalar"));}
        if p.clip_mm.is_some_and(|r|r.iter().any(|v|!v.is_finite())||r[2]<=0.||r[3]<=0.){return Err(invalid("invalid legacy image crop scalar"));}
        let Some(image)=doc.objects.iter().find(|o|o.id==id).cloned() else{continue};
        let Geometry::Image{resource_id,..}=&image.geometry else{continue};
        if image.style.stroke!="none"{normalized+=1;continue;}
        let info=crate::resource::validate(&doc.resources[resource_id])?;let(px,py)=(f64::from(info.width_px),f64::from(info.height_px));
        let(mut sx,mut sy)=(p.size_mm[0]/px,p.size_mm[1]/py);
        if p.fit!=ImageFit::Stretch{let s=if p.fit==ImageFit::Cover{sx.max(sy)}else{sx.min(sy)};sx=s;sy=s;}
        let(w,h)=(px*sx,py*sy);let(dx,dy)=((p.size_mm[0]-w)/2.,(p.size_mm[1]-h)/2.);
        let mut mask=(p.fit==ImageFit::Cover).then(||Rect::new(0.,0.,p.size_mm[0],p.size_mm[1]));
        if let Some([x,y,w,h])=p.clip_mm{let rect=Rect::new(x,y,x+w,y+h);mask=Some(mask.map_or(rect,|r|r.intersect(rect)));}
        if mask.is_some_and(|r|r.width()<=0.||r.height()<=0.){return Err(invalid("empty source image viewport"));}
        let wrapped=p.had_clip&&mask.is_some();let mut wrapper=None;let mut parent=image.parent_id.clone();
        if wrapped {
            let Some(candidate)=parent.as_ref().and_then(|id|doc.objects.iter().find(|o|&o.id==id))else{normalized+=1;continue};
            let generated=p.generated_wrapper.as_ref()==Some(&candidate.id);
            if !generated||!matches!(candidate.geometry,Geometry::Group)||candidate.style.opacity!=1.||!candidate.visible||candidate.locked||candidate.clip.is_none()||doc.objects.iter().filter(|o|o.parent_id.as_deref()==Some(&candidate.id)).count()!=1{normalized+=1;continue;}
            parent=candidate.parent_id.clone();wrapper=Some(candidate.clone());
        }
        let parent_world=parent.as_ref().map(|id|doc.world_transform(doc.object(id)?)).transpose()?.unwrap_or(Affine::IDENTITY);
        let source_world=parent_world*Affine::new(p.matrix)*Affine::translate((p.position_mm[0],p.position_mm[1]));
        if source_world.determinant().abs()<1e-12{return Err(invalid("singular source image viewport"));}
        let expected=source_world*Affine::translate((dx,dy));let actual=doc.world_transform(&image)?;
        let observed=actual*Affine::scale_non_uniform(image.width_mm/w,image.height_mm/h);
        let delta=if corners(expected,w,h).into_iter().zip(corners(observed,w,h)).all(|(a,b)|close(a,b)){Affine::IDENTITY}else{observed*expected.inverse()};
        if let Some(mask)=mask {
            let Some(clip)=&image.clip else{normalized+=1;continue};
            if !same_rectangle(&(actual*clip.path()?),&(delta*source_world*mask.into_path(0.00001))){normalized+=1;continue;}
        }else if image.clip.is_some()!=p.had_clip{normalized+=1;continue;}
        let recovered=delta*source_world;
        let clip=if let Some(wrapper)=&wrapper {
            let clip=wrapper.clip.as_ref().unwrap();Some(Clip::Path{d:(recovered.inverse()*doc.world_transform(wrapper)?*clip.path()?).to_svg(),fill_rule:clip.rule()})
        }else if mask.is_none(){image.clip.as_ref().map(|clip|Ok(Clip::Path{d:(recovered.inverse()*actual*clip.path()?).to_svg(),fill_rule:clip.rule()})).transpose()?}else{None};
        let object=doc.objects.iter_mut().find(|o|o.id==id).unwrap();object.parent_id=parent;object.x_mm=p.position_mm[0];object.y_mm=p.position_mm[1];object.width_mm=p.size_mm[0];object.height_mm=p.size_mm[1];
        object.transform=if delta==Affine::IDENTITY{p.matrix}else{(parent_world.inverse()*recovered*Affine::translate((-p.position_mm[0],-p.position_mm[1]))).as_coeffs()};
        object.geometry=Geometry::Image{resource_id:resource_id.clone(),fit:p.fit,clip_mm:p.clip_mm};object.clip=clip;
        if let Some(wrapper)=wrapper{doc.objects.retain(|o|o.id!=wrapper.id);}
        doc.validate()?;restored+=1;
    }Ok((restored,normalized))
}
