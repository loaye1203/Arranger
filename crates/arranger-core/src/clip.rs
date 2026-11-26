//! Non-destructive object-local clips, independent of object paint/geometry.
use crate::{Document,Error,FillRule,Geometry,ImageFit,Object,Result};
use kurbo::{BezPath,Point,Rect,Shape,PathEl};
use serde::{Serialize,Deserialize};

#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]
#[serde(tag="kind",rename_all="snake_case",deny_unknown_fields)]
pub enum Clip {
    Rect {rect_mm:[f64;4]},
    Path {d:String,#[serde(default,skip_serializing_if="FillRule::is_nonzero")] fill_rule:FillRule},
}
impl Clip {
    pub fn path(&self)->Result<BezPath> {match self {
        Self::Rect{rect_mm:[x,y,w,h]}=>Ok(Rect::new(*x,*y,x+w,y+h).to_path(1e-4)),
        Self::Path{d,..}=>crate::path::parse(d).map_err(|e|Error::new("INVALID_CLIP",e.message)),
    }}
    pub fn rule(&self)->FillRule {match self {Self::Path{fill_rule,..}=>*fill_rule,_=>FillRule::NonZero}}
    pub fn validate(&self)->Result<()> {
        if let Self::Rect{rect_mm:r}=self {
            if !r.iter().all(|v|v.is_finite()) || r[2]<0. || r[3]<0. {return Err(Error::new("INVALID_CLIP","Clip rectangle needs finite coordinates and nonnegative width/height"));}
        }
        let path=self.path()?;
        if !path.is_finite() {return Err(Error::new("INVALID_CLIP","Clip coordinates must be finite"));}
        let mut open=false;
        for el in path.elements() {match el {
            PathEl::MoveTo(_)=>{if open {return Err(Error::new("INVALID_CLIP","Close every clip subpath"));}open=true;},
            PathEl::ClosePath=>open=false,_=>{},
        }}
        if open {return Err(Error::new("INVALID_CLIP","Close every clip subpath"));}
        let r=path.bounding_box();
        if ![r.x0,r.y0,r.x1,r.y1,r.width(),r.height()].iter().all(|v|v.is_finite() && (*v as f32).is_finite()) {
            return Err(Error::new("INVALID_CLIP","Clip exceeds the renderer's finite coordinate range"));
        }
        Ok(())
    }
    pub fn empty(&self)->Result<bool> {let path=self.path()?;let r=path.bounding_box();Ok(path.elements().is_empty() || r.width()==0. || r.height()==0.)}
    pub fn contains(&self,p:Point)->Result<bool> {let path=self.path()?;let r=path.bounding_box();Ok(!path.elements().is_empty() && r.width()!=0. && r.height()!=0. && crate::path::contains(&path,self.rule(),p))}
    pub fn svg(&self)->String {match self {
        Self::Rect{rect_mm:[x,y,w,h]}=>format!("<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" fill=\"black\"/>"),
        Self::Path{d,fill_rule}=>format!("<path d=\"{}\" clip-rule=\"{}\" fill=\"black\"/>",d.replace('&',"&amp;").replace('"',"&quot;").replace('<',"&lt;"),fill_rule.svg_name()),
    }}
}

#[derive(Serialize)]
pub struct ActiveClip {
    pub object_id:String,
    pub source:&'static str,
    pub clip:Clip,
    pub world_transform:[f64;6],
}
impl Document {
    fn visit_clips(&self, object:&Object, mut visit:impl FnMut(&Object,&Clip,&'static str,kurbo::Affine)->Result<bool>)->Result<bool> {
        let mut current=object;
        loop {
            let mut world=None;
            let mut check=|clip:&Clip,source| {
                let transform=match world {Some(t)=>t,None=>{let t=self.world_transform(current)?;world=Some(t);t}};
                visit(current,clip,source,transform)
            };
            if let Some(clip)=&current.clip && !check(clip,"object")? {return Ok(false);}
            if let Geometry::Image{clip_mm,fit,..}=&current.geometry {
                if let Some(rect_mm)=clip_mm && !check(&Clip::Rect{rect_mm:*rect_mm},"legacy_image")? {return Ok(false);}
                if *fit==ImageFit::Cover && !check(&Clip::Rect{rect_mm:[0.,0.,current.width_mm,current.height_mm]},"cover")? {return Ok(false);}
            }
            match &current.parent_id {Some(id)=>current=self.object(id)?,None=>return Ok(true)}
        }
    }
    pub fn active_clips(&self,object:&Object)->Result<Vec<ActiveClip>> {
        let mut clips=vec![];
        self.visit_clips(object,|o,clip,source,world| {
            clips.push(ActiveClip{object_id:o.id.clone(),source,clip:clip.clone(),world_transform:world.as_coeffs()});Ok(true)
        })?;
        Ok(clips)
    }
    pub fn inside_clips(&self,object:&Object,point:Point)->Result<bool> {
        self.visit_clips(object,|_,clip,_,transform|{
            if transform.determinant().abs()<f64::EPSILON {Ok(false)} else {clip.contains(transform.inverse()*point)}
        })
    }
}
