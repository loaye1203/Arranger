//! Original image containers in standard document-local Corel ObjectData.
//! Live bitmap pixels remain native. This bounded attachment contains only an
//! image resource, never an Arranger document, scene or rendering substitute.
use super::{binary::*, invalid, unsupported, Result};
use crate::{Document, Geometry, Resource, model::{sha256,valid_id}};
use base64::{Engine as _,engine::general_purpose::STANDARD};
use super::local_data::{field_name,template,definition,string};
use std::collections::BTreeMap;

const PREFIX:&str="Arranger_SourceV1_";
const CHUNK:usize=768;
#[cfg(test)]
const PAYLOAD:usize=CHUNK-4;
#[cfg(test)]
fn values(id:&str,r:&Resource)->Result<BTreeMap<String,String>> {
    let raw=STANDARD.decode(&r.data_base64).map_err(|e|invalid(e.to_string()))?;
    let encoded=STANDARD.encode(&raw);let count=encoded.len().div_ceil(PAYLOAD);
    if count==0||count>u16::MAX as usize-16{return Err(unsupported("original image exceeds this native field-count codec range"));}
    let pixels=image::load_from_memory(&raw).map_err(|e|invalid(e.to_string()))?.to_rgba8();
    let mut fields=BTreeMap::from([("Version".into(),"u32:1".into()),("ID".into(),format!("id:{id}")),("MIME".into(),r.mime.clone()),("SHA256".into(),format!("sha256:{}",r.sha256)),("RGBA_SHA256".into(),format!("sha256:{}",sha256(pixels.as_raw()))),("Bytes".into(),format!("u32:{}",raw.len())),("Count".into(),format!("u32:{count}"))]);
    for (i,part) in encoded.as_bytes().chunks(PAYLOAD).enumerate(){fields.insert(format!("P{i:05}"),format!("b64:{}",String::from_utf8(part.to_vec()).unwrap()));}Ok(fields)
}
/// Each used image instance carries its source so deleting another shared
/// instance in Corel cannot orphan the surviving object's original container.
#[cfg(test)]
pub(super) fn attach(nodes:&mut [Node],doc:&Document)->Result<()> {
    let mut entries=vec![];
    for n in all(nodes) {
        if n.kind()!=Some(b"obj "){continue;}
        let (kind,args)=super::write::arguments(&super::read::own_record(n,b"loda")?.data)?;if kind!=5{continue;}
        let name=read_wide(&args.iter().find(|(k,_)|*k==1000).ok_or_else(||invalid("image name missing"))?.1)?;
        let o=doc.object(&name)?;let Geometry::Image{resource_id,..}=&o.geometry else{return Err(invalid("native resource owner is not an image"))};
        let fields=values(resource_id,&doc.resources[resource_id])?;
        entries.push((u32_at(&super::read::own_record(n,b"usdn")?.data,0)?,fields));
    }
    super::local_data::append(nodes,PREFIX,entries)
}

fn integer(fields:&BTreeMap<String,String>,key:&str)->Result<usize> {
    fields.get(key).and_then(|s|s.strip_prefix("u32:")).and_then(|s|s.parse::<usize>().ok()).ok_or_else(||invalid("native original resource integer missing or invalid"))
}
pub(super) fn restore(doc:&mut Document,nodes:&[Node],modern:bool,images:&BTreeMap<u32,(String,(u32,u32))>,object_keys:&BTreeMap<String,u32>)->Result<usize> {
    let mut definitions=BTreeMap::new();let mut names=std::collections::BTreeSet::new();
    for n in all(nodes).into_iter().filter(|n|n.tag==*b"udfe") {
        let name=field_name(&n.data,modern)?;let Some(name)=name.strip_prefix(PREFIX) else{continue};let key=u32_at(&n.data,0)?;
        if n.data!=definition(&template(modern)?,&format!("{PREFIX}{name}"),key,u16_at(&n.data,6)?,modern)?||definitions.insert(key,name.to_owned()).is_some()||!names.insert(name.to_owned()){return Err(unsupported("native original resource field schema changed"));}
    }
    if definitions.is_empty(){return Ok(0);}
    let mut owners=BTreeMap::new();
    for n in all(nodes).into_iter().filter(|n|matches!(n.kind(),Some(b"obj "|b"grp "))) {
        let (kind,args)=super::write::arguments(&super::read::own_record(n,b"loda")?.data)?;
        let Some(record)=super::read::optional_own_record(n,b"usdn")? else{continue};let sid=u32_at(&record.data,0)?;
        let bitmap=if kind==5&&n.kind()==Some(b"obj "){let g=&args.iter().find(|(k,_)|*k==30).ok_or_else(||invalid("native image geometry missing"))?.1;Some(u32_at(g,48)?)}else{None};
        owners.entry(sid).or_insert_with(Vec::new).push(bitmap);
    }
    let mut originals=BTreeMap::<String,Resource>::new();let mut bindings=BTreeMap::<u32,String>::new();let mut used_owners=std::collections::BTreeSet::new();
    for entry in all(nodes).into_iter().filter(|n|n.kind()==Some(b"udte")) {
        let mut fields=BTreeMap::new();
        for record in entry.children.as_deref().unwrap_or_default().iter().filter(|n|n.tag==*b"udta") {
            if let Some(name)=definitions.get(&u32_at(&record.data,0)?) {if fields.insert(name.clone(),string(&record.data)?).is_some(){return Err(invalid("duplicate original resource item"));}}
        }
        if fields.is_empty(){continue;}
        let owner=u32_at(&super::read::own_record(entry,b"udid")?.data,0)?;
        if !used_owners.insert(owner){return Err(invalid("duplicate original resource owner"));}
        let keys=owners.get(&owner).ok_or_else(||invalid("original resource owner missing or no longer a bitmap"))?;if keys.len()!=1{return Err(invalid("ambiguous original resource owner"));}let key=keys[0].ok_or_else(||invalid("original resource owner is no longer a bitmap"))?;
        let count=integer(&fields,"Count")?;let size=integer(&fields,"Bytes")?;
        if fields.get("Version").map(String::as_str)!=Some("u32:1")||count==0||count>u16::MAX as usize-16||size>LIMIT||fields.len()!=count+7{return Err(unsupported("unknown or incomplete original resource attachment"));}
        let mut encoded=String::new();
        for i in 0..count {let part=fields.get(&format!("P{i:05}")).ok_or_else(||invalid("original image chunk missing"))?;
            if part.len()<=4||part.len()>CHUNK||i+1<count&&part.len()!=CHUNK{return Err(invalid("original image chunk size differs"));}
            encoded.push_str(part.strip_prefix("b64:").ok_or_else(||invalid("original image chunk encoding differs"))?);
        }
        let raw=STANDARD.decode(encoded).map_err(|e|invalid(e.to_string()))?;if raw.len()!=size||fields.get("SHA256")!=Some(&format!("sha256:{}",sha256(&raw))){return Err(invalid("original resource length/hash differs"));}
        let id=fields.get("ID").and_then(|s|s.strip_prefix("id:")).filter(|s|valid_id(s)).ok_or_else(||invalid("original resource ID invalid"))?.to_owned();let resource=Resource::image(&raw)?;
        if fields.get("MIME")!=Some(&resource.mime){return Err(invalid("original resource MIME differs"));}
        // Never let stale metadata hide native pixel edits in another editor.
        let original=image::load_from_memory(&raw).map_err(|e|invalid(e.to_string()))?.to_rgba8();
        if fields.get("RGBA_SHA256")!=Some(&format!("sha256:{}",sha256(original.as_raw()))){return Err(unsupported("original resource decoded RGBA differs from declared source pixels"));}
        let (legacy,_)=images.get(&key).ok_or_else(||invalid("original resource bitmap reference missing"))?;
        let native=STANDARD.decode(&doc.resources[legacy].data_base64).map_err(|e|invalid(e.to_string()))?;
        if super::native_pixels::gray16(&raw,&resource.mime)?!=super::native_pixels::gray16(&native,"image/png")?{return Err(unsupported("original resource attachment is stale: native grayscale16 samples or bit depth differ"));}
        if super::native_pixels::rgb16(&raw,&resource.mime)?!=super::native_pixels::rgb16(&native,"image/png")?{return Err(unsupported("original resource attachment is stale: native RGB16 samples or bit depth differ"));}
        let native=image::load_from_memory(&native).map_err(|e|invalid(e.to_string()))?.to_rgba8();
        if original!=native{return Err(unsupported("original resource attachment is stale: editable native pixels differ"));}
        if originals.get(&id).is_some_and(|r|r!=&resource)||bindings.get(&key).is_some_and(|r|r!=&id){return Err(invalid("original resource identity or bitmap bindings conflict"));}
        originals.insert(id.clone(),resource);bindings.insert(key,id);
    }
    if bindings.is_empty(){return Err(invalid("original resource field definitions have no complete values"));}
    let restored=bindings.len();let mut resources=originals;
    for (key,(old,_)) in images {
        if bindings.contains_key(key){continue;}
        let r=doc.resources[old].clone();let mut id=old.clone();while resources.get(&id).is_some_and(|v|v!=&r){id.push('_');}
        resources.entry(id.clone()).or_insert(r);bindings.insert(*key,id);
    }
    for object in &mut doc.objects {if let Geometry::Image{resource_id,..}=&mut object.geometry {
        let key=object_keys.get(&object.id).ok_or_else(||invalid("native image key missing"))?;*resource_id=bindings.get(key).ok_or_else(||invalid("native original resource binding missing"))?.clone();
    }}
    doc.resources=resources;Ok(restored)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn legacy_bytes(doc:&Document,version:u32)->Vec<u8> {
        let mut old=read_native(&crate::cdr::encode_document(doc).unwrap().0).unwrap();attach(&mut old.nodes,doc).unwrap();
        if version==14{return write(&old.nodes).unwrap();}
        let mut native=read_native(&crate::cdr::encode_document21(doc).unwrap().0).unwrap();native.members.remove("META-INF/metadata.xml");
        let source=all(&old.nodes).into_iter().find(|n|n.kind()==Some(b"doc ")).unwrap();
        let target=native.nodes.iter_mut().find(|n|n.kind()==Some(b"doc ")).unwrap();super::super::local_data::transfer21(source,target).unwrap();
        super::super::binary::repack21(&native).unwrap()
    }
    #[test]
    fn real_corel_original_and_edited_small_resource_values_are_decoded_exactly() {
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports/windows/WIN-SAVE-CLOSURE-20261024-01/object-data/v14-and-small");
        let expected=std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/assets/images/quadrants.png")).unwrap();
        for version in [14,21] {for suffix in ["","-edited"] {
            let native=read_native(&std::fs::read(root.join(format!("original-png-v{version}{suffix}.cdr"))).unwrap()).unwrap();
            let ns=all(&native.nodes);let field=ns.iter().find(|n|n.tag==*b"udfe"&&field_name(&n.data,version==21).ok().as_deref()==Some("Arranger_SourceBytes")).unwrap();let key=u32_at(&field.data,0).unwrap();
            let item=ns.iter().find(|n|n.tag==*b"udta"&&u32_at(&n.data,0).ok()==Some(key)).unwrap();assert_eq!(STANDARD.decode(string(&item.data).unwrap()).unwrap(),expected);
        }}
    }
    #[test]
    fn corrupted_or_stale_native_attachments_are_rejected_before_partial_import() {
        let raw=std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/assets/images/quadrants.png")).unwrap();
        let mut doc=Document::new("source".into(),210.,297.).unwrap();doc.resources.insert("pixels".into(),Resource::png(&raw).unwrap());
        doc.objects.push(serde_json::from_value(serde_json::json!({"id":"image","page_id":"page-1","x_mm":20.,"y_mm":30.,"width_mm":40.,"height_mm":20.,"style":{"fill":"none","stroke":"none","stroke_width_mm":0.,"opacity":1.},"geometry":{"kind":"image","resource_id":"pixels","fit":"stretch"}})).unwrap());
        for version in [14,21] {for mode in 0..7 {
            let original=legacy_bytes(&doc,version);assert_eq!(crate::cdr::decode(&original,"legacy").unwrap().0.resources,doc.resources);
            let mut native=read_native(&original).unwrap();let mut changed=false;
            fn edit(nodes:&mut [Node],mode:u32,changed:&mut bool) {
                for node in nodes {
                    if !*changed&&node.tag==*b"udta"&&mode<5 {
                        // Every value has the same strict header/terminator;
                        // mutating one cannot silently turn into a plain bitmap.
                        match mode {0=>node.data[6]^=1,1=>node.data[4]=3,2=>node.data.truncate(node.data.len()-2),3=>{node.data[6]=0;node.data[7]=0;},_=>node.data[8..10].copy_from_slice(&0xd800u16.to_le_bytes())};*changed=true;
                    }else if !*changed&&node.tag==*b"udid"&&mode==5 {set32(&mut node.data,0,99999).unwrap();*changed=true;
                    }else if !*changed&&node.tag==*b"bmp "&&mode==6 {node.data[118]^=1;*changed=true;}
                    if let Some(children)=&mut node.children {edit(children,mode,changed);}
                }
            }
            edit(&mut native.nodes,mode,&mut changed);assert!(changed);
            let bytes=if version==14 {write(&native.nodes).unwrap()}else{super::super::binary::repack21(&native).unwrap()};
            let result=std::panic::catch_unwind(||crate::cdr::decode(&bytes,"bad"));assert!(result.is_ok());assert!(result.unwrap().is_err(),"version{version} mode{mode}");
            assert_eq!(doc.resources["pixels"].data_base64,STANDARD.encode(&raw));
        }}
    }
}
