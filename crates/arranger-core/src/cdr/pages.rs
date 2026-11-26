//! Native drawing pages and the nonprinting, document-wide desktop layer.
//! Names use Corel's page/layer name records, never an opaque Arranger payload.
use super::{binary::*, invalid, unsupported, Result};
use crate::{Document, Page, WORKSPACE_PAGE, model::valid_id};
use std::collections::BTreeSet;

pub(super) const WORKSPACE_PROFILE:&[u8]=include_bytes!("../../../../reports/windows/WIN-CDR-SAVE-20261022-01/focused/native-preservation/workspace-v14.cdr");
pub(super) struct Scene<'a> {pub node:&'a Node,pub page_id:String,pub size:[f64;2]}
pub(super) fn props(page:&Node,default:[f64;2])->Result<(String,[f64;2])> {
    let mut name=String::new();let mut size=default;
    let properties=page.children.as_deref().unwrap_or_default().iter().filter(|n|n.kind()==Some(b"lgob")).collect::<Vec<_>>();
    if properties.len()>1 {return Err(invalid("ambiguous native page properties"));}
    if let Some(properties)=properties.first() {
        let raw=&properties.find(b"loda")?.data;
        if u32_at(raw,4)?==0 {
            if raw.len()!=24 || u32_at(raw,0)?!=24 || u32_at(raw,16)?!=0 {return Err(invalid("invalid empty native page properties"));}
        } else {
            let (kind,args)=super::write::arguments(raw)?;
            if kind!=0 {return Err(unsupported("unknown native page property kind"));}
            for (tag,value) in args {
                match tag {
                    1000=>name=read_wide(&value)?,
                    19130 if [8,16].contains(&value.len())=>{
                        size=[i32_at(&value,0)? as f64/10000.,i32_at(&value,4)? as f64/10000.];
                        if value.len()==16 && (u32_at(&value,12)?!=1 || !(u32_at(&value,8)?==0 || u32_at(&value,8)?==15&&size==[297.,210.])) {return Err(unsupported("unmeasured native paper preset/orientation properties"));}
                    },
                    12000 if value.as_slice()==[0,0,0,0]=>{},
                    _=>return Err(unsupported("native page effects/properties require preservation support")),
                }
            }
        }
    }
    if size.iter().any(|v|!v.is_finite() || *v<=0.) {return Err(invalid("native page dimensions must be positive"));}
    Ok((name,size))
}
pub(super) fn decode<'a>(nodes:&'a [Node],default:[f64;2])->Result<(Vec<Page>,Vec<Scene<'a>>,bool)> {
    let mut drawing=vec![];let mut master=vec![];
    for page in all(nodes).into_iter().filter(|n|n.kind()==Some(b"page")) {
        if super::read::flags(page)?&0x10000==0 {drawing.push((page,props(page,default)?));}else{master.push(page);}
    }
    if drawing.is_empty() || master.len()>1 {return Err(unsupported("requires drawing pages and at most one native master page"));}
    let names=drawing.iter().map(|(_,p)|p.0.clone()).collect::<BTreeSet<_>>();let mut used=BTreeSet::new();let mut pages=vec![];let mut scenes=vec![];let mut assigned=false;
    for (i,(node,(name,size))) in drawing.into_iter().enumerate() {
        let id=if valid_id(&name) && name!=WORKSPACE_PAGE && !used.contains(&name) {name}else{
            assigned=true;let mut id=format!("page-{}",i+1);while used.contains(&id)||names.contains(&id){id.push('_');}id
        };used.insert(id.clone());pages.push(Page{id:id.clone(),width_mm:size[0],height_mm:size[1]});scenes.push(Scene{node,page_id:id,size});
    }
    for node in master {scenes.push(Scene{node,page_id:WORKSPACE_PAGE.into(),size:default});}
    for scene in &scenes {
        let master=scene.page_id==WORKSPACE_PAGE;
        for layer in all(std::slice::from_ref(scene.node)).into_iter().filter(|n|n.kind()==Some(b"layr")) {
            let has_objects=layer.children.as_deref().unwrap_or_default().iter().any(|n|matches!(n.kind(),Some(b"obj "|b"grp "|b"lnkg")));
            if has_objects {
                let flag=super::read::flags(layer)?;
                // Corel's recorded move/SaveAs14 round-trip adds 0x40 while
                // retaining the same layer properties and object states.
                let supported=if master {[0x98010000,0x98010008].contains(&(flag&!0x40))}else{flag&!0x40==0x98000000};
                if !supported {return Err(unsupported("nonstandard populated native layer requires visibility/lock/print preservation"));}
                // This is the actual nonprinting desktop layer in the native
                // workspace reference. Other master-layer semantics stay out.
                if master {
                    let (kind,_)=super::write::arguments(&super::read::own_record(layer,b"loda")?.data)?;
                    if kind!=0 {return Err(unsupported("special master layers are not shared-workspace objects"));}
                }
            }
        }
        if all(std::slice::from_ref(scene.node)).iter().any(|n|matches!(n.kind(),Some(b"lnkg"|b"clpt"))) {return Err(unsupported("native linked or misplaced clipping group"));}
    }
    Ok((pages,scenes,assigned))
}
pub(super) fn size(doc:&Document,owner:&str)->Result<[f64;2]> {
    let p=if owner==WORKSPACE_PAGE {&doc.pages[0]}else{doc.page(owner)?};Ok([p.width_mm,p.height_mm])
}
pub(super) fn set_properties(page:&mut Node,p:&Page)->Result<()> {
    // Corel reads all four native words. The old eight-byte form left the
    // preset/orientation tail uninitialized on its next SaveAs.
    let values=[coord(p.width_mm)?,coord(p.height_mm)?,0,1].into_iter().flat_map(i32::to_le_bytes).collect();
    let properties=Node::list(b"lgob",vec![Node::leaf(b"loda",super::write::loda(0,&[(1000,wide(&p.id)),(19130,values)]))]);
    let children=page.children.as_mut().ok_or_else(||invalid("missing native page children"))?;
    children.retain(|n|n.kind()!=Some(b"lgob"));children.push(properties);Ok(())
}
pub(super) fn workspace_layer()->Result<Node> {
    let profile=read(WORKSPACE_PROFILE)?;
    let page=all(&profile).into_iter().find(|n|n.kind()==Some(b"page")&&super::read::flags(n).is_ok_and(|f|f&0x10000!=0)).ok_or_else(||invalid("missing native master profile"))?;
    let mut layer=all(std::slice::from_ref(page)).into_iter().find(|n|n.kind()==Some(b"layr")&&super::read::flags(n).is_ok_and(|f|f==0x98010000)).ok_or_else(||invalid("missing nonprinting native desktop layer"))?.clone();
    layer.children.as_mut().unwrap().retain(|n|!matches!(n.kind(),Some(b"obj "|b"grp ")));
    // The reference also has per-page overrides. A transplanted master layer
    // needs the explicit nonprinting bit; otherwise Corel opens it printable.
    let flag=super::read::flags(&layer)?;
    layer.find_mut(b"flgs")?.data=(flag|0x8).to_le_bytes().to_vec();
    let data=&layer.find(b"loda")?.data;let (kind,mut args)=super::write::arguments(data)?;
    args.retain(|(tag,_)|*tag!=40050);for (tag,v) in &mut args {if *tag==1000 {*v=wide("Arranger workspace");}}
    layer.find_mut(b"loda")?.data=super::write::loda(kind,&args);Ok(layer)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn emitted_page_properties_initialize_the_complete_native_paper_record() {
        let mut page=Node::list(b"page",vec![]);
        set_properties(&mut page,&Page{id:"page-1".into(),width_mm:210.,height_mm:297.}).unwrap();
        let (_,args)=super::super::write::arguments(&page.find(b"loda").unwrap().data).unwrap();
        let raw=&args.iter().find(|(tag,_)|*tag==19130).unwrap().1;
        assert_eq!(raw.len(),16,"Corel reads a complete paper record when opening independent output");
        assert_eq!(u32_at(raw,8).unwrap(),0);assert_eq!(u32_at(raw,12).unwrap(),1);
    }
    #[test]
    fn actual_corel_workspace_is_one_master_scene_with_two_private_pages() {
        let tree=read(WORKSPACE_PROFILE).unwrap();let (pages,scenes,assigned)=decode(&tree,[210.,297.]).unwrap();
        assert_eq!(pages.len(),2);assert_eq!(scenes.len(),3);assert!(assigned);
        let shared=scenes.iter().find(|s|s.page_id==WORKSPACE_PAGE).unwrap();
        let objects=all(std::slice::from_ref(shared.node)).into_iter().filter(|n|n.kind()==Some(b"obj ")).collect::<Vec<_>>();assert_eq!(objects.len(),1);
        let (_,args)=super::super::write::arguments(&super::super::read::own_record(objects[0],b"loda").unwrap().data).unwrap();
        assert_eq!(read_wide(&args.into_iter().find(|(tag,_)|*tag==1000).unwrap().1).unwrap(),"arr-shared-circle");
    }
    #[test]
    fn page_names_are_collision_safe_and_unknown_properties_reject() {
        let empty=||Node::list(b"page",vec![Node::leaf(b"flgs",0x90000000u32.to_le_bytes().to_vec())]);
        let mut a=empty();let mut b=empty();let mut c=empty();
        for (n,id) in [(&mut a,"page-1"),(&mut b,"page-1"),(&mut c,"中文")] {set_properties(n,&Page{id:id.into(),width_mm:123.,height_mm:45.}).unwrap();}
        let nodes=vec![a,b,c];let (p,_,assigned)=decode(&nodes,[210.,297.]).unwrap();assert!(assigned);assert_eq!(p.iter().map(|p|&p.id).collect::<BTreeSet<_>>().len(),3);assert_eq!(p[0].id,"page-1");assert_eq!(p[2].height_mm,45.);
        for (tag,data) in [(19130,vec![0;8]),(19130,vec![1;4]),(9999,vec![0;4])] {
            let mut page=empty();page.children.as_mut().unwrap().push(Node::list(b"lgob",vec![Node::leaf(b"loda",super::super::write::loda(0,&[(tag,data)]))]));
            assert!(decode(&[page],[210.,297.]).is_err());
        }
    }
    #[test]
    fn unrepresented_populated_master_layer_flags_fail_before_import() {
        for flag in [0x98000000u32,0x98010010,0x98010001] {
            let mut tree=read(WORKSPACE_PROFILE).unwrap();
            fn change(nodes:&mut [Node],flag:u32) {
                for n in nodes {
                    if n.kind()==Some(b"layr")&&super::super::read::flags(n).is_ok_and(|v|v==0x98010000)&&n.children.as_deref().unwrap().iter().any(|c|c.kind()==Some(b"obj ")) {n.find_mut(b"flgs").unwrap().data=flag.to_le_bytes().to_vec();return;}
                    if let Some(c)=&mut n.children {change(c,flag);}
                }
            }
            change(&mut tree,flag);assert!(decode(&tree,[210.,297.]).is_err());
        }
    }
}
