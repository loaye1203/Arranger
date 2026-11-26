//! Native PowerClip references. Contents stay editable; no bitmap flattening.
use super::{binary::*, invalid, unsupported, Result};
use crate::{Clip, Document, Geometry, Object, Style};
use kurbo::{Affine, BezPath, Rect, Shape};
use std::collections::{BTreeMap, BTreeSet};

pub(super) const PREFIX: &str = "arranger-clip-";
pub(super) fn args(node:&Node)->Result<BTreeMap<u32,Vec<u8>>> {
    Ok(super::write::arguments(&super::read::own_record(node,b"loda")?.data)?.1.into_iter().collect())
}
pub(super) fn key(node:&Node)->Result<u32>{u32_at(&super::read::own_record(node,b"spnd")?.data,0)}
pub(super) fn reference(node:&Node)->Result<Option<u32>> {
    let a=args(node)?;
    match a.get(&8005) {
        None=>{if a.contains_key(&40200){return Err(unsupported("PowerClip properties without a content reference"));}Ok(None)},
        Some(v)=>{
            if v.len()!=16 || u32_at(v,4)?!=1 || u32_at(v,8)?!=0 || u32_at(v,12)?!=1 || a.get(&40200).is_none_or(|v|v.as_slice()!=[0,0,0,0]) {
                return Err(unsupported("unknown PowerClip reference options"));
            }
            Ok(Some(u32_at(v,0)?))
        }
    }
}
pub(super) struct Links<'a> { wrappers:BTreeMap<u32,&'a Node>, used:BTreeSet<u32> }
impl<'a> Links<'a> {
    pub fn new(tree:&'a [Node])->Result<Self> {
        let mut wrappers=BTreeMap::new();let mut ids=BTreeSet::new();
        for n in all(tree) {
            if matches!(n.kind(),Some(b"obj "|b"grp ")) && !ids.insert(key(n)?) {return Err(invalid("duplicate native object key"));}
            if n.kind()==Some(b"clpt") {
                for child in n.children.as_deref().unwrap_or_default() {
                    if child.kind()!=Some(b"grp ") || super::read::flags(child)?!=0x10800800 {return Err(unsupported("non-PowerClip vector table is unsupported"));}
                    let a=args(child)?;
                    // loda 100 is an additional ID; spnd is the stream key.
                    // Real nested SaveAs14 files renumber these independently.
                    // Associations still use spnd/3500 and are checked below.
                    if a.get(&3500).is_none_or(|v|v.len()!=4) || a.iter().any(|(k,v)|![3500,100].contains(k)&&!(*k==201&&super::modern::style_record(v).is_ok_and(|v|v==serde_json::json!({})))) || a.get(&100).is_some_and(|v|v.len()!=4) {return Err(unsupported("unknown PowerClip content envelope"));}
                    if wrappers.insert(key(child)?,child).is_some(){return Err(invalid("duplicate PowerClip content key"));}
                }
            }
        }
        Ok(Self{wrappers,used:BTreeSet::new()})
    }
    pub fn scene(&mut self,nodes:&'a [Node],parent:Option<usize>,out:&mut Vec<(&'a Node,Option<usize>)>,depth:usize)->Result<()> {
        if depth>16 {return Err(unsupported("native group/PowerClip nesting exceeds 16 levels"));}
        for n in nodes.iter().rev() {
            match n.kind() {
                Some(b"grp ")=>{let index=out.len();out.push((n,parent));self.scene(n.children.as_deref().unwrap_or_default(),Some(index),out,depth+1)?;},
                Some(b"obj ")=>{
                    let index=out.len();out.push((n,parent));
                    if let Some(link)=reference(n)? {
                        let wrapper=*self.wrappers.get(&link).ok_or_else(||invalid("PowerClip content reference missing"))?;
                        if u32_at(&args(wrapper)?[&3500],0)?!=key(n)? || !self.used.insert(link) {return Err(invalid("PowerClip reference is cyclic, reused or points to a different frame"));}
                        self.scene(wrapper.children.as_deref().unwrap_or_default(),Some(index),out,depth+1)?;
                    }
                },
                Some(b"gobj"|b"layr")=>self.scene(n.children.as_deref().unwrap_or_default(),parent,out,depth)?,
                Some(b"lnkg"|b"clpt")=>return Err(unsupported("linked groups or misplaced PowerClip table")),
                _=>{},
            }
        }Ok(())
    }
    pub fn finish(&self)->Result<()> {if self.used.len()!=self.wrappers.len(){Err(invalid("unreferenced PowerClip contents"))}else{Ok(())}}
}
fn geometry(o:&Object)->Result<BezPath> {
    match &o.geometry {
        Geometry::Rect{radius_mm} if *radius_mm==0.=>Ok(Rect::new(0.,0.,o.width_mm,o.height_mm).to_path(1e-6)),
        Geometry::Ellipse=>{let mut path=kurbo::Ellipse::new((o.width_mm/2.,o.height_mm/2.),(o.width_mm/2.,o.height_mm/2.),0.).to_path(1e-6);if !matches!(path.elements().last(),Some(kurbo::PathEl::ClosePath)){path.close_path();}Ok(path)},
        Geometry::Path{d,..}=>crate::path::parse(d),
        _=>Err(unsupported("PowerClip frame must be a supported closed vector shape")),
    }
}
fn clip(path:BezPath,fill_rule:crate::FillRule)->Result<Clip>{let c=Clip::Path{d:path.to_svg(),fill_rule};c.validate()?;Ok(c)}
fn unique(doc:&Document,base:&str)->String {let mut id=base.to_string();while doc.objects.iter().any(|o|o.id==id){id.push('_');}id}
fn group(id:String,frame:&Object,parent:Option<String>)->Object {
    Object{id,page_id:frame.page_id.clone(),parent_id:parent,x_mm:0.,y_mm:0.,width_mm:0.,height_mm:0.,transform:[1.,0.,0.,1.,0.,0.],style:Style::default(),visible:frame.visible,locked:frame.locked,clip:None,geometry:Geometry::Group}
}
/// Frame paint is separate from its clipped contents, so an outline remains
/// above the contents and its outer half is not accidentally clipped away.
pub(super) fn restore(doc:&mut Document,frames:&[String])->Result<()> {
    for id in frames.iter().rev() {
        let at=doc.objects.iter().position(|o|&o.id==id).ok_or_else(||invalid("decoded PowerClip frame missing"))?;
        let frame=doc.objects[at].clone();let world=Affine::new(frame.transform)*Affine::translate((frame.x_mm,frame.y_mm));let path=world*geometry(&frame)?;
        let roots=doc.objects.iter().enumerate().filter(|(_,o)|o.parent_id.as_deref()==Some(id)).map(|(i,_)|i).collect::<Vec<_>>();
        if roots.is_empty(){return Err(unsupported("empty PowerClip contents"));}
        // Our native empty frame encloses exactly one original editable object.
        // Validate the geometry/paint and graph before collapsing that envelope.
        if id.starts_with(PREFIX) && roots.len()==1 && frame.style.fill=="none" && frame.style.stroke=="none" && frame.style.opacity==1. {
            let index=roots[0];let child=doc.objects[index].clone();let inverse=(Affine::new(child.transform)*Affine::translate((child.x_mm,child.y_mm))).inverse();
            if child.clip.is_some(){return Err(invalid("canonical PowerClip envelope duplicates an existing clip"));}
            doc.objects[index].clip=Some(clip(inverse*path,frame.fill_rule())?);doc.objects[index].parent_id=frame.parent_id;doc.objects[index].visible&=frame.visible;doc.objects[index].locked|=frame.locked;doc.objects.remove(at);continue;
        }
        let content_id=unique(doc,&format!("{id}-contents"));
        for &index in &roots {doc.objects[index].parent_id=Some(content_id.clone());}
        let mut content=group(content_id,&frame,Some(id.clone()));content.clip=Some(clip(path,frame.fill_rule())?);
        doc.objects[at]=group(id.clone(),&frame,frame.parent_id.clone());
        let mut additions=vec![];
        if frame.style.fill!="none" {let mut bg=frame.clone();bg.id=unique(doc,&format!("{id}-fill"));bg.parent_id=Some(id.clone());bg.style.stroke="none".into();additions.push(bg);}
        additions.push(content);doc.objects.splice(at+1..at+1,additions);
        if frame.style.stroke!="none" {
            let mut border=frame.clone();border.id=unique(doc,&format!("{id}-outline"));border.parent_id=Some(id.clone());border.style.fill="none".into();
            // Traversal emitted each subtree contiguously, in bottom-first order.
            let mut descendants=BTreeSet::from([id.clone()]);let mut end=at+1;
            for (i,o) in doc.objects.iter().enumerate().skip(at+1) {if o.parent_id.as_ref().is_some_and(|p|descendants.contains(p)){descendants.insert(o.id.clone());end=i+1;}else{break;}}
            doc.objects.insert(end,border);
        }
    }Ok(())
}
pub(super) fn world_clip(doc:&Document,o:&Object)->Result<Option<BezPath>> {
    o.clip.as_ref().map(|c|{
        if c.empty()? {return Err(unsupported("empty native clips are not encoded"));}
        Ok(doc.world_transform(o)?*c.path()?)
    }).transpose()
}
pub(super) fn wrapper(key:u32,frame:u32,bbox:Vec<u8>,content:Node)->Node {
    let mut obbx=bbox.clone();obbx.extend(&bbox);
    Node::list(b"grp ",vec![Node::leaf(b"spnd",key.to_le_bytes().to_vec()),Node::leaf(b"flgs",0x10800800u32.to_le_bytes().to_vec()),Node::leaf(b"bbox",bbox),Node::leaf(b"obbx",obbx),Node::list(b"lgob",vec![Node::leaf(b"loda",super::write::loda(0,&[(3500,frame.to_le_bytes().to_vec())]))]),content])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn noncanonical_native21_frame_keeps_evenodd_and_unknown_winding_is_rejected() {
        let mut doc=Document::new("clip-rule".into(),210.,297.).unwrap();
        doc.objects.push(serde_json::from_value(serde_json::json!({"id":"content","page_id":"page-1","x_mm":20.,"y_mm":20.,"width_mm":60.,"height_mm":50.,"geometry":{"kind":"rect","radius_mm":0.},"clip":{"kind":"path","d":"M0 0 L60 0 L60 50 L0 50 Z M10 10 L50 10 L50 40 L10 40 Z","fill_rule":"evenodd"}})).unwrap());
        let source=super::super::write21::encode(&doc).unwrap().0;
        for winding in ["0","2"] {
            let native=read_native(&source).unwrap();let mut nodes=native.nodes;
            fn edit(nodes:&mut [Node],winding:&str)->bool {
                for n in nodes {
                    if n.kind()==Some(b"obj ") && reference(n).unwrap().is_some() {
                        let (kind,mut args)=super::super::write::arguments(&super::super::read::own_record(n,b"loda").unwrap().data).unwrap();
                        args.iter_mut().find(|(k,_)|*k==1000).unwrap().1=wide("external-frame");
                        let style=&mut args.iter_mut().find(|(k,_)|*k==201).unwrap().1;
                        let mut value=super::super::modern::style_record(style).unwrap();value["fill"]["winding"]=serde_json::json!(winding);*style=super::super::modern::encoded_style(&value).unwrap();
                        n.find_mut(b"loda").unwrap().data=super::super::write::loda(kind,&args);return true;
                    }
                    if let Some(children)=&mut n.children {if edit(children,winding){return true;}}
                }false
            }
            assert!(edit(&mut nodes,winding));let raw=write21(&nodes,native.members).unwrap();
            let decoded=crate::cdr::decode(&raw,"external");
            if winding=="0" {let d=decoded.unwrap().0;assert!(matches!(d.object("external-frame").unwrap().geometry,Geometry::Group));let clips=d.objects.iter().filter_map(|o|o.clip.as_ref()).collect::<Vec<_>>();assert_eq!(clips.len(),1);assert_eq!(clips[0].rule(),crate::FillRule::EvenOdd);}else{assert!(decoded.is_err());}
        }
    }
    #[test]
    fn broken_native_powerclip_links_are_rejected_atomically() {
        let original=include_bytes!("../../../../reports/windows/WIN-CDR-GROUPS-20261023-01/references/clip-nested-v14.cdr");
        for mode in 0..4 {
            let mut tree=read(original).unwrap();let mut changed=false;
            fn edit(nodes:&mut [Node],mode:u8,changed:&mut bool) {
                for n in nodes {
                    if !*changed && n.tag==*b"loda" {
                        let Ok((kind,mut a))=super::super::write::arguments(&n.data) else {continue};
                        let tag=if mode<2 {3500}else{8005};
                        if let Some((_,data))=a.iter_mut().find(|(tag2,_)|*tag2==tag) {
                            if mode==0 || mode==2 {set32(data,0,9999).unwrap();}else if mode==1 {set32(data,0,0).unwrap();}else{set32(data,4,9).unwrap();}
                            n.data=super::super::write::loda(kind,&a);*changed=true;
                        }
                    }
                    if let Some(children)=&mut n.children {edit(children,mode,changed);}
                }
            }
            edit(&mut tree,mode,&mut changed);assert!(changed);
            let bad=write(&tree).unwrap();assert!(crate::cdr::decode(&bad,"broken").is_err());
        }
    }
}
