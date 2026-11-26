//! Compatibility reader for the earlier document-local group style fields.
//! New writes use document Notes because Corel drops desktop ObjectData.
use super::{binary::*,invalid,unsupported,Result};
use crate::{Document,Geometry,LineCap,LineJoin,Style};
use std::collections::BTreeMap;
const PREFIX:&str="Arranger_GroupStyleV1_";

pub(super) fn restore(doc:&mut Document,nodes:&[Node],modern:bool)->Result<usize> {
    let definitions=super::local_data::definitions(nodes,modern,PREFIX)?;if definitions.is_empty(){return Ok(0);}
    let mut owners:BTreeMap<u32,Vec<String>>=BTreeMap::new();
    for node in all(nodes).into_iter().filter(|n|n.kind()==Some(b"grp ")) {
        let Some(id)=super::read::optional_own_record(node,b"usdn")? else{continue};
        let (_,args)=super::write::arguments(&super::read::own_record(node,b"loda")?.data)?;
        let Some((_,name))=args.iter().find(|(k,_)|*k==1000) else{continue};owners.entry(u32_at(&id.data,0)?).or_default().push(read_wide(name)?);
    }
    let values=super::local_data::values(nodes,&definitions)?;if values.is_empty(){return Err(invalid("group style fields have no group owner"));}let count=values.len();
    for (owner,fields) in values {
        if fields.len()!=7||fields.get("Version").map(String::as_str)!=Some("u32:1"){return Err(unsupported("unknown group style scalar layout"));}
        let get=|key|fields.get(key).map(String::as_str).ok_or_else(||invalid("group style scalar missing"));
        let scalar=|key|->Result<f64>{get(key)?.strip_prefix("f64:").and_then(|v|v.parse::<f64>().ok()).filter(|v|v.is_finite()).ok_or_else(||invalid("invalid group style scalar"))};
        let ids=owners.get(&owner).filter(|ids|ids.len()==1).ok_or_else(||invalid("group style owner missing or ambiguous"))?;
        let object=doc.objects.iter_mut().find(|o|o.id==ids[0]&&matches!(o.geometry,Geometry::Group)).ok_or_else(||invalid("group style target is not a group"))?;
        object.style=Style{fill:get("Fill")?.into(),stroke:get("Stroke")?.into(),stroke_width_mm:scalar("Width")?,stroke_scale_with_object:true,stroke_fixed_nib:None,opacity:object.style.opacity,
            stroke_linecap:match get("Cap")?{"butt"=>LineCap::Butt,"round"=>LineCap::Round,"square"=>LineCap::Square,_=>return Err(invalid("group cap scalar differs"))},
            stroke_linejoin:match get("Join")?{"miter"=>LineJoin::Miter,"round"=>LineJoin::Round,"bevel"=>LineJoin::Bevel,_=>return Err(invalid("group join scalar differs"))},stroke_miterlimit:scalar("Miter")?};
    }Ok(count)
}
