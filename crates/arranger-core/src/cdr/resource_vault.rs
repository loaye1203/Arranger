//! Source resources only, in Corel's standard document Notes XMP field.
//! Native pixels and geometry remain independently editable; this catalogue
//! contains no document model, text, paths, transforms or rendering fallback.
use super::{invalid,unsupported,Result};
use crate::{Document,Geometry,Resource,model::{sha256,valid_id}};
use base64::{Engine as _,engine::general_purpose::STANDARD};
use serde::{Serialize,Deserialize};
use std::collections::{BTreeMap,BTreeSet};

const PREFIX:&str="ArrangerResourceVaultV2|sha256:";
const PREFIX3:&str="ArrangerResourceVaultV3|sha256:";
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Vault {
    version:u32,
    resources:BTreeMap<String,Resource>,
    bindings:BTreeMap<String,String>,
    pixels:BTreeMap<String,String>,
    #[serde(default,skip_serializing_if="BTreeMap::is_empty")]
    cmyk_pixels:BTreeMap<String,String>,
}

pub(super) fn notes(doc:&Document)->Result<Option<String>> {
    if doc.resources.is_empty(){return Ok(None);}
    let bindings:BTreeMap<String,String>=doc.objects.iter().filter_map(|o|if let Geometry::Image{resource_id,..}=&o.geometry{Some((o.id.clone(),resource_id.clone()))}else{None}).collect();
    let mut pixels=BTreeMap::new();for id in bindings.values(){if pixels.contains_key(id){continue;}let raw=STANDARD.decode(&doc.resources[id].data_base64).map_err(|e|invalid(e.to_string()))?;let image=image::load_from_memory(&raw).map_err(|e|invalid(e.to_string()))?.to_rgba8();pixels.insert(id.clone(),sha256(image.as_raw()));}
    let mut cmyk_pixels=BTreeMap::new();for id in bindings.values(){if cmyk_pixels.contains_key(id){continue;}let r=&doc.resources[id];let raw=STANDARD.decode(&r.data_base64).map_err(|e|invalid(e.to_string()))?;if let Some(image)=super::native_pixels::source(&raw,&r.mime)?{cmyk_pixels.insert(id.clone(),sha256(&image.pixels));}}
    let (prefix,version)=if cmyk_pixels.is_empty(){(PREFIX,2)}else{(PREFIX3,3)};
    super::metadata::encode(prefix,&Vault{version,resources:doc.resources.clone(),bindings,pixels,cmyk_pixels}).map(Some)
}

pub(super) fn read(members:&BTreeMap<String,Vec<u8>>)->Result<Option<Vault>> {
    let v2=super::metadata::find(members,PREFIX)?;let v3=super::metadata::find(members,PREFIX3)?;
    let (prefix,value,version)=match (v2,v3){(None,None)=>return Ok(None),(Some(v),None)=>(PREFIX,v,2),(None,Some(v))=>(PREFIX3,v,3),_=>return Err(invalid("ambiguous source resource catalogue versions"))};
                let vault:Vault=super::metadata::decode(prefix,&value)?;
                if vault.version!=version||(version==2&&!vault.cmyk_pixels.is_empty())||(version==3&&vault.cmyk_pixels.is_empty()){return Err(unsupported("unknown source resource vault version"));}
                for (id,resource) in &vault.resources {if !valid_id(id){return Err(invalid("source resource ID invalid"));}crate::resource::validate(resource)?;}
                for (id,resource) in &vault.bindings {if !valid_id(id)||!vault.resources.contains_key(resource)||!vault.pixels.contains_key(resource){return Err(invalid("source resource instance binding invalid"));}}
                for (id,hash) in &vault.pixels {if !vault.resources.contains_key(id)||hash.len()!=64||!hash.bytes().all(|b|b.is_ascii_hexdigit()){return Err(invalid("source resource pixel checksum invalid"));}}
                for (id,hash) in &vault.cmyk_pixels {if !vault.pixels.contains_key(id)||!vault.bindings.values().any(|v|v==id)||hash.len()!=64||!hash.bytes().all(|b|b.is_ascii_hexdigit()){return Err(invalid("source CMYK channel checksum invalid"));}}
    Ok(Some(vault))
}

pub(super) fn restore(doc:&mut Document,vault:Vault,images:&BTreeMap<u32,(String,(u32,u32))>,keys:&BTreeMap<String,u32>,cmyk:&BTreeMap<u32,super::native_pixels::Cmyk>,gray16:&BTreeMap<u32,super::native_pixels::Gray16>,rgb16:&BTreeMap<u32,super::native_pixels::Rgb16>)->Result<usize> {
    let mut originals=BTreeMap::new();let mut native=BTreeMap::new();let mut count=0;
    let native_resources=std::mem::take(&mut doc.resources);let mut known:BTreeMap<u32,BTreeSet<String>>=BTreeMap::new();let mut unbound=vec![];let mut checked_cmyk=BTreeSet::new();let mut checked_sources=BTreeSet::new();let mut source_cmyk=BTreeMap::new();let mut source_gray16=BTreeMap::new();let mut source_rgb16=BTreeMap::new();
    for object in &mut doc.objects {
        let Geometry::Image{resource_id,..}=&mut object.geometry else{continue};
        let key=*keys.get(&object.id).ok_or_else(||invalid("native bitmap instance key missing"))?;
        let Some(source)=vault.bindings.get(&object.id) else{unbound.push((object.id.clone(),key));continue};
        if !originals.contains_key(source) {
            let raw=STANDARD.decode(&vault.resources[source].data_base64).map_err(|e|invalid(e.to_string()))?;
            source_cmyk.insert(source.clone(),super::native_pixels::source(&raw,&vault.resources[source].mime)?);
            source_gray16.insert(source.clone(),super::native_pixels::gray16(&raw,&vault.resources[source].mime)?);
            source_rgb16.insert(source.clone(),super::native_pixels::rgb16(&raw,&vault.resources[source].mime)?);
            let image=image::load_from_memory(&raw).map_err(|e|invalid(e.to_string()))?.to_rgba8();
            if sha256(image.as_raw())!=vault.pixels[source]{return Err(invalid("source resource pixel checksum differs"));}
            originals.insert(source.clone(),image);
        }
        if !native.contains_key(&key) {
            let (id,_)=images.get(&key).ok_or_else(||invalid("native bitmap reference missing"))?;
            let raw=STANDARD.decode(&native_resources[id].data_base64).map_err(|e|invalid(e.to_string()))?;
            native.insert(key,image::load_from_memory(&raw).map_err(|e|invalid(e.to_string()))?.to_rgba8());
        }
        // An unchanged 8-bit preview cannot certify low-order sample bits.
        // Compare the active native samples against the source container,
        // rejecting downconversion and stale Notes after native pixel edits.
        if source_gray16[source].as_ref()!=gray16.get(&key){return Err(unsupported("source resource vault is stale: native grayscale16 samples or bit depth differ"));}
        if source_rgb16[source].as_ref()!=rgb16.get(&key){return Err(unsupported("source resource vault is stale: native RGB16 samples or bit depth differ"));}
        match (source_cmyk[source].as_ref(),cmyk.get(&key)) {
            (Some(expected),Some(actual))=>{if vault.cmyk_pixels.get(source)!=Some(&sha256(&expected.pixels)){return Err(invalid("source CMYK checksum differs"));}if expected!=actual{return Err(unsupported("source resource vault is stale: native CMYK channels differ"));}checked_cmyk.insert(key);checked_sources.insert(source.clone());},
            (None,None)=>if originals[source]!=native[&key]{return Err(unsupported("source resource vault is stale: native editable pixels differ"));},
            _=>return Err(unsupported("source resource native color model differs; RGB cannot replace CMYK channels")),
        }
        *resource_id=source.clone();known.entry(key).or_default().insert(source.clone());count+=1;
    }
    let mut resources=vault.resources;let mut added:BTreeMap<u32,String>=BTreeMap::new();
    for (object_id,key) in unbound {
        let id=if let Some(source)=known.get(&key).filter(|set|set.len()==1).and_then(|set|set.first()) {
            count+=1;source.clone()
        }else if let Some(id)=added.get(&key){id.clone()}else{
            if cmyk.contains_key(&key){return Err(unsupported("native CMYK requires a verified original CMYK resource; lossless standalone import is not implemented"));}
            let (native_id,_)=images.get(&key).ok_or_else(||invalid("native bitmap reference missing"))?;
            let value=native_resources.get(native_id).ok_or_else(||invalid("native bitmap pixels missing"))?;
            let mut id=native_id.clone();let mut index=0;
            while resources.get(&id).is_some_and(|r|r!=value){index+=1;id=format!("cdr-imported-{key}-{index}");}
            resources.insert(id.clone(),value.clone());added.insert(key,id.clone());id
        };
        let object=doc.objects.iter_mut().find(|o|o.id==object_id).ok_or_else(||invalid("new native image missing"))?;
        let Geometry::Image{resource_id,..}=&mut object.geometry else{return Err(invalid("native image kind changed"))};*resource_id=id;
    }
    if cmyk.keys().any(|key|!checked_cmyk.contains(key)){return Err(unsupported("native CMYK bitmap has no verified original four-channel resource"));}
    if vault.cmyk_pixels.keys().any(|id|!checked_sources.contains(id)){return Err(invalid("CMYK resource checksum has no verified active native bitmap"));}
    doc.resources=resources;Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unbound_native_pixel_resources_cannot_overwrite_an_unused_original_with_the_same_id() {
        use image::ImageEncoder;
        let raw=include_bytes!("../../../../examples/assets/images/quadrants.png");let original=Resource::png(raw).unwrap();let image=image::load_from_memory(raw).unwrap().to_rgba8();let mut output=Vec::new();
        image::codecs::png::PngEncoder::new_with_quality(&mut output,image::codecs::png::CompressionType::Fast,image::codecs::png::FilterType::NoFilter).write_image(image.as_raw(),image.width(),image.height(),image::ExtendedColorType::Rgba8).unwrap();
        let pixels=Resource::png(&output).unwrap();assert_ne!(original,pixels);assert_eq!(image,image::load_from_memory(&output).unwrap().to_rgba8());
        let mut doc=Document::new("new-image".into(),210.,297.).unwrap();doc.resources.insert("native".into(),pixels.clone());
        doc.objects.push(serde_json::from_value(serde_json::json!({"id":"new","page_id":"page-1","x_mm":0.,"y_mm":0.,"width_mm":40.,"height_mm":20.,"geometry":{"kind":"image","resource_id":"native","fit":"stretch"}})).unwrap());
        let vault=Vault{version:2,resources:BTreeMap::from([("native".into(),original.clone())]),bindings:BTreeMap::new(),pixels:BTreeMap::new(),cmyk_pixels:BTreeMap::new()};
        let images=BTreeMap::from([(7,("native".into(),(image.width(),image.height())))]);let keys=BTreeMap::from([("new".into(),7)]);
        assert_eq!(restore(&mut doc,vault,&images,&keys,&BTreeMap::new(),&BTreeMap::new(),&BTreeMap::new()).unwrap(),0);assert_eq!(doc.resources["native"],original);let Geometry::Image{resource_id,..}=&doc.objects[0].geometry else{panic!()};assert_ne!(resource_id,"native");assert_eq!(doc.resources[resource_id],pixels);doc.validate().unwrap();
    }
    #[test]
    fn renamed_native_bitmaps_keep_editable_pixels_and_unused_original_bytes() {
        use super::super::binary::*;
        let mut doc=Document::new("images".into(),210.,297.).unwrap();let raw=include_bytes!("../../../../examples/assets/images/quadrants.png");
        doc.resources.insert("original".into(),Resource::png(raw).unwrap());
        doc.objects.push(serde_json::from_value(serde_json::json!({"id":"image","page_id":"page-1","x_mm":10.,"y_mm":20.,"width_mm":40.,"height_mm":20.,"style":{"fill":"none","stroke":"none","stroke_width_mm":0.,"opacity":1.},"geometry":{"kind":"image","resource_id":"original","fit":"stretch"}})).unwrap());
        for version in [14,21] {
            let encoded=if version==14{crate::cdr::encode_document(&doc)}else{crate::cdr::encode_document21(&doc)}.unwrap().0;let mut native=read_native(&encoded).unwrap();
            fn rename(nodes:&mut [Node]) {for node in nodes {
                if node.kind()==Some(b"obj ") {let (kind,mut args)=super::super::write::arguments(&super::super::read::own_record(node,b"loda").unwrap().data).unwrap();if kind==5 {args.iter_mut().find(|(k,_)|*k==1000).unwrap().1=wide("renamed");node.find_mut(b"loda").unwrap().data=super::super::write::loda(kind,&args);}}
                if let Some(children)=&mut node.children{rename(children);}
            }}rename(&mut native.nodes);
            let encoded=if version==21{repack21(&native).unwrap()}else{native.members.insert("content/riffData.cdr".into(),write(&native.nodes).unwrap());zip(&native.members.iter().map(|(n,d)|(n.as_str(),d.as_slice())).collect::<Vec<_>>())};
            let (restored,notices)=crate::cdr::decode(&encoded,"renamed").unwrap();assert_eq!(restored.objects.len(),1);let object=restored.object("renamed").unwrap();let Geometry::Image{resource_id,..}=&object.geometry else{panic!()};
            let pixels=image::load_from_memory(&STANDARD.decode(&restored.resources[resource_id].data_base64).unwrap()).unwrap().to_rgba8();assert_eq!(pixels,image::load_from_memory(raw).unwrap().to_rgba8());
            assert_eq!(restored.resources["original"],doc.resources["original"]);assert!(notices.iter().any(|n|n.code=="CDR_IMAGE_PIXELS"));
        }
    }
    #[test]
    fn compressed_catalogue_rejects_trailing_bytes_instead_of_accepting_a_second_payload() {
        let mut doc=Document::new("resources".into(),210.,297.).unwrap();
        doc.resources.insert("unused".into(),Resource::png(include_bytes!("../../../../examples/assets/images/quadrants.png")).unwrap());
        let text=String::from_utf8(super::super::metadata::document(&doc,false).unwrap().unwrap()).unwrap();
        let mut members=BTreeMap::from([("metadata/metadata.xml".into(),text.as_bytes().to_vec())]);
        assert!(read(&members).unwrap().is_some());
        let encoded=text.split("|z64:").nth(1).unwrap().split('<').next().unwrap();
        let mut raw=STANDARD.decode(encoded).unwrap();raw.extend(b"extra");
        members.insert("metadata/metadata.xml".into(),text.replace(encoded,&STANDARD.encode(raw)).into_bytes());
        assert!(read(&members).is_err());
    }
}
