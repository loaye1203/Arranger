//! Resolve image fit into an editable native bitmap placement and PowerClips.
//! The source frame/fit is normalized explicitly; original resources are untouched.
use super::{unsupported,Result};
use crate::{Clip,Document,Geometry,ImageFit,Object,Style};
use kurbo::{Affine,Rect};
use std::collections::BTreeSet;

pub(super) fn needs_placement(doc:&Document)->bool {
    doc.objects.iter().any(|o|matches!(&o.geometry,Geometry::Image{fit,clip_mm,..} if *fit!=ImageFit::Stretch || clip_mm.is_some()))
}
pub(super) fn normalize(source:&Document,result:&mut Document)->Result<()> {
    let mut ids=source.objects.iter().map(|o|o.id.clone()).collect::<BTreeSet<_>>();let mut objects=vec![];
    for (old,normalized) in source.objects.iter().zip(&result.objects) {
        let mut image=normalized.clone();
        if let Geometry::Image{resource_id,fit,clip_mm}=&old.geometry {
            if *fit!=ImageFit::Stretch || clip_mm.is_some() {
                let info=crate::resource::validate(&source.resources[resource_id])?;
                let (px,py)=(f64::from(info.width_px),f64::from(info.height_px));
                let (mut sx,mut sy)=(old.width_mm/px,old.height_mm/py);
                if *fit!=ImageFit::Stretch {let s=if *fit==ImageFit::Cover {sx.max(sy)}else{sx.min(sy)};sx=s;sy=s;}
                let (width,height)=(px*sx,py*sy);let (dx,dy)=((old.width_mm-width)/2.,(old.height_mm-height)/2.);
                let mut rect=if *fit==ImageFit::Cover {Some(Rect::new(0.,0.,old.width_mm,old.height_mm))}else{None};
                if let Some([x,y,w,h])=clip_mm {let legacy=Rect::new(*x,*y,x+w,y+h);rect=Some(rect.map_or(legacy,|r|r.intersect(legacy)));}
                let shifted=Affine::translate((-dx,-dy));
                image.clip=match (&old.clip,rect) {
                    (Some(clip),Some(_))=>{
                        let base=format!("{}-image-frame",&old.id[..old.id.len().min(96)]);let mut id=base.clone();let mut n=0;
                        while !ids.insert(id.clone()) {n+=1;id=format!("{base}-{n}");}
                        // The normalized parent uses page coordinates, just as
                        // existing CDR14 group members do. Keep the image's ID.
                        let wrapper=Object{id:id.clone(),page_id:old.page_id.clone(),parent_id:old.parent_id.clone(),x_mm:0.,y_mm:0.,width_mm:0.,height_mm:0.,transform:[1.,0.,0.,1.,0.,0.],style:Style::default(),visible:true,locked:false,clip:Some(Clip::Path{d:(source.world_transform(old)?*clip.path()?).to_svg(),fill_rule:clip.rule()}),geometry:Geometry::Group};
                        image.parent_id=Some(id);objects.push(wrapper);None
                    },
                    (Some(clip),None)=>{
                        Some(Clip::Path{d:(shifted*clip.path()?).to_svg(),fill_rule:clip.rule()})
                    },
                    _=>None,
                };
                if let Some(r)=rect {
                    if r.width()<=0. || r.height()<=0. {return Err(unsupported("empty image viewport intersections are not encoded"));}
                    image.clip=Some(Clip::Rect{rect_mm:[r.x0-dx,r.y0-dy,r.width(),r.height()]});
                }
                image.width_mm=width;image.height_mm=height;image.x_mm=0.;image.y_mm=0.;image.transform=(source.world_transform(old)?*Affine::translate((dx,dy))).as_coeffs();
                image.geometry=Geometry::Image{resource_id:resource_id.clone(),fit:ImageFit::Stretch,clip_mm:None};
            }
        }
        objects.push(image);
    }
    result.objects=objects;result.validate()?;
    for o in &result.objects {
        let mut current=o;let mut depth=0;
        while let Some(parent)=&current.parent_id {depth+=1;if depth>16 {return Err(unsupported("normalized image/group nesting exceeds 16 levels"));}current=result.object(parent)?;}
    }
    Ok(())
}
