//! Standard document-local string fields, measured in real14/21 files.
//! Only bounded resource and paint parameters; no hidden document model.
use super::{binary::*,invalid,unsupported,Result};
use std::collections::{BTreeMap,BTreeSet};
const PROFILE14:&[u8]=include_bytes!("../../../../reports/windows/WIN-SAVE-CLOSURE-20261024-01/object-data/v14-and-small/original-png-v14.cdr");
const PROFILE21:&[u8]=include_bytes!("../../../../reports/windows/WIN-SAVE-CLOSURE-20261024-01/object-data/v14-and-small/original-png-v21.cdr");

pub(super) fn field_name(data:&[u8],modern:bool)->Result<String> {
    let at=if modern {30}else{26};let n=u16_at(data,16)? as usize;
    if bytes(data,at+n*2,2)?!=[0,0]{return Err(invalid("native resource field name terminator differs"));}
    read_wide(bytes(data,at,n*2)?)
}
pub(super) fn template(modern:bool)->Result<Vec<u8>> {
    static OLD:std::sync::OnceLock<Result<Vec<u8>>>=std::sync::OnceLock::new();
    static NEW:std::sync::OnceLock<Result<Vec<u8>>>=std::sync::OnceLock::new();
    (if modern {&NEW}else{&OLD}).get_or_init(|| {
        let native=read_native(if modern {PROFILE21}else{PROFILE14})?;
        all(&native.nodes).into_iter().find(|n|n.tag==*b"udfe"&&field_name(&n.data,modern).ok().as_deref()==Some("Arranger_SourceBytes")).map(|n|n.data.clone()).ok_or_else(||invalid("native local string field profile missing"))
    }).clone()
}
pub(super) fn definition(profile:&[u8],name:&str,key:u32,order:u16,modern:bool)->Result<Vec<u8>> {
    let at=if modern {30}else{26};let old=u16_at(profile,16)? as usize;
    let size=u16::try_from(name.encode_utf16().count()).map_err(|_|unsupported("native resource field name exceeds range"))?;
    let mut out=profile[..at].to_vec();out.extend(wide(name));out.extend(bytes(profile,at+(old+1)*2,profile.len()-at-(old+1)*2)?);
    set32(&mut out,0,key)?;out[6..8].copy_from_slice(&order.to_le_bytes());out[16..18].copy_from_slice(&size.to_le_bytes());Ok(out)
}
#[cfg(test)]
pub(super) fn item(key:u32,value:&str)->Node {
    let mut raw=key.to_le_bytes().to_vec();raw.extend([2,0]);raw.extend(wide(value));raw.push(0);Node::leaf(b"udta",raw)
}
pub(super) fn string(raw:&[u8])->Result<String> {
    if raw.len()<9||raw.len()%2!=1||bytes(raw,4,2)?!=[2,0]||raw[raw.len()-3..]!=[0,0,0]{return Err(invalid("native original resource item is not a terminated string"));}
    let units=bytes(raw,6,raw.len()-9)?.chunks_exact(2).map(|r|u16::from_le_bytes([r[0],r[1]])).collect::<Vec<_>>();
    if units.contains(&0){return Err(invalid("embedded zero in original resource string"));}
    String::from_utf16(&units).map_err(|_|invalid("invalid original resource UTF16"))
}

#[cfg(test)]
pub(super) fn append(nodes:&mut [Node],prefix:&str,entries:Vec<(u32,BTreeMap<String,String>)>)->Result<()> {
    if entries.is_empty(){return Ok(());}
    let names=entries.iter().flat_map(|(_,f)|f.keys().cloned()).collect::<BTreeSet<_>>();
    let native=nodes.iter_mut().find_map(|n|n.list_mut(b"doc ").ok()).ok_or_else(||invalid("native document missing for local fields"))?;
    let definitions=native.list_mut(b"udaf")?.children.as_mut().ok_or_else(||invalid("native local field definitions missing"))?;
    let mut key=definitions.iter().filter(|n|n.tag==*b"udfe").map(|n|u32_at(&n.data,0)).collect::<Result<Vec<_>>>()?.into_iter().max().unwrap_or(0);
    let mut order=definitions.iter().filter(|n|n.tag==*b"udfe").map(|n|u16_at(&n.data,6)).collect::<Result<Vec<_>>>()?.into_iter().max().unwrap_or(0);let mut ids=BTreeMap::new();
    for name in names {
        let full_name=format!("{prefix}{name}");let existing=definitions.iter().filter(|n|n.tag==*b"udfe"&&field_name(&n.data,false).is_ok_and(|s|s==full_name)).collect::<Vec<_>>();
        if existing.len()>1 {return Err(invalid("duplicate native local field definition"));}
        if let Some(field)=existing.first(){let id=u32_at(&field.data,0)?;if field.data!=definition(&template(false)?,&full_name,id,u16_at(&field.data,6)?,false)?{return Err(unsupported("existing native local field schema differs"));}ids.insert(name,id);continue;}
        key=key.checked_add(1).ok_or_else(||unsupported("native local field key overflow"))?;order=order.checked_add(1).ok_or_else(||unsupported("native local field order overflow"))?;
        definitions.push(Node::leaf(b"udfe",definition(&template(false)?,&format!("{prefix}{name}"),key,order,false)?));ids.insert(name,key);
    }
    let data=native.list_mut(b"udat")?.children.as_mut().ok_or_else(||invalid("native local value table missing"))?;
    for (owner,fields) in entries {
        let matches=data.iter().enumerate().filter(|(_,n)|n.kind()==Some(b"udte")&&super::read::own_record(n,b"udid").is_ok_and(|r|u32_at(&r.data,0).ok()==Some(owner))).map(|(i,_)|i).collect::<Vec<_>>();
        if matches.len()>1 {return Err(invalid("ambiguous native local field owner"));}
        let mut rows=fields.iter().map(|(name,value)|item(ids[name],value)).collect::<Vec<_>>();
        if let Some(&i)=matches.first(){let children=data[i].children.as_mut().ok_or_else(||invalid("native local owner values missing"))?;let at=children.iter().position(|n|n.tag==*b"udid").ok_or_else(||invalid("native local owner record missing"))?;children.splice(at..at,rows);}else{rows.push(Node::leaf(b"udid",owner.to_le_bytes().to_vec()));data.push(Node::list(b"udte",rows));}
    }Ok(())
}
pub(super) fn definitions(nodes:&[Node],modern:bool,prefix:&str)->Result<BTreeMap<u32,String>> {
    let mut definitions=BTreeMap::new();let mut names=BTreeSet::new();
    for n in all(nodes).into_iter().filter(|n|n.tag==*b"udfe") {
        let name=field_name(&n.data,modern)?;let Some(name)=name.strip_prefix(prefix) else{continue};let key=u32_at(&n.data,0)?;
        if n.data!=definition(&template(modern)?,&format!("{prefix}{name}"),key,u16_at(&n.data,6)?,modern)?||definitions.insert(key,name.to_owned()).is_some()||!names.insert(name.to_owned()){return Err(unsupported("native local field schema changed"));}
    }Ok(definitions)
}
pub(super) fn values(nodes:&[Node],definitions:&BTreeMap<u32,String>)->Result<Vec<(u32,BTreeMap<String,String>)>> {
    let mut values=vec![];let mut owners=BTreeSet::new();
    for n in all(nodes).into_iter().filter(|n|n.kind()==Some(b"udte")) {
        let mut fields=BTreeMap::new();
        for row in n.children.as_deref().unwrap_or_default().iter().filter(|n|n.tag==*b"udta") {if let Some(name)=definitions.get(&u32_at(&row.data,0)?){if fields.insert(name.clone(),string(&row.data)?).is_some(){return Err(invalid("duplicate native local field value"));}}}
        if fields.is_empty(){continue;}let owner=u32_at(&super::read::own_record(n,b"udid")?.data,0)?;
        if !owners.insert(owner){return Err(invalid("duplicate native local field owner"));}values.push((owner,fields));
    }Ok(values)
}
pub(super) fn transfer21(source:&Node,target:&mut Node)->Result<()> {
    let fields=all(std::slice::from_ref(source)).into_iter().filter(|n|n.tag==*b"udfe"&&field_name(&n.data,false).is_ok_and(|s|["Arranger_SourceV1_","Arranger_PaintV1_","Arranger_GroupStyleV1_"].iter().any(|p|s.starts_with(p)))).collect::<Vec<_>>();
    if fields.is_empty(){return Ok(());}
    let profile=template(true)?;let out=target.list_mut(b"udaf")?.children.as_mut().ok_or_else(||invalid("native21 local field table missing"))?;
    for field in fields {out.push(Node::leaf(b"udfe",definition(&profile,&field_name(&field.data,false)?,u32_at(&field.data,0)?,u16_at(&field.data,6)?,true)?));}
    let data=source.children.as_deref().unwrap_or_default().iter().find(|n|n.kind()==Some(b"udat")).ok_or_else(||invalid("lowered local item table missing"))?;
    *target.list_mut(b"udat")?=data.clone();Ok(())
}
