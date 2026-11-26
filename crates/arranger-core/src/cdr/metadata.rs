//! Bounded resource and local scalar sections in Corel's standard Notes.
//! Native geometry, text and scene hierarchy remain authoritative.
use super::{binary::LIMIT,invalid,unsupported,Result};
use base64::{Engine as _,engine::general_purpose::STANDARD};
use flate2::{Compression,write::ZlibEncoder,read::ZlibDecoder};
use serde::{Serialize,de::DeserializeOwned};
use std::{collections::BTreeMap,io::{Read,Write}};
pub(super) fn encode<T:Serialize>(prefix:&str,value:&T)->Result<String> {
    let data=serde_json::to_vec(value).map_err(|e|invalid(e.to_string()))?;
    if data.len()>LIMIT{return Err(unsupported("native local extension exceeds bounded codec limit"));}
    let mut encoder=ZlibEncoder::new(Vec::new(),Compression::default());encoder.write_all(&data).map_err(|e|invalid(e.to_string()))?;
    Ok(format!("{prefix}{}|z64:{}",crate::model::sha256(&data),STANDARD.encode(encoder.finish().map_err(|e|invalid(e.to_string()))?)))
}
pub(super) fn decode<T:DeserializeOwned>(prefix:&str,value:&str)->Result<T> {
    let tail=value.strip_prefix(prefix).ok_or_else(||invalid("native extension marker differs"))?;
    let (checksum,encoded)=tail.split_once("|z64:").ok_or_else(||invalid("native extension header differs"))?;
    if checksum.len()!=64||!checksum.bytes().all(|b|b.is_ascii_hexdigit()){return Err(invalid("native extension checksum invalid"));}
    let raw=STANDARD.decode(encoded).map_err(|e|invalid(e.to_string()))?;let mut decoded=Vec::new();let mut decoder=ZlibDecoder::new(raw.as_slice());
    (&mut decoder).take(LIMIT as u64+1).read_to_end(&mut decoded).map_err(|e|invalid(e.to_string()))?;
    if decoder.total_in()!=raw.len() as u64||decoded.len()>LIMIT||crate::model::sha256(&decoded)!=checksum{return Err(invalid("native extension checksum/length differs"));}
    serde_json::from_slice(&decoded).map_err(|e|invalid(format!("native extension schema: {e}")))
}
/// The native flag is a declaration. It does not authorize installing the
/// attached fonts or establish that every face in the document was embedded.
pub(super) fn declares_embedded_fonts(members: &BTreeMap<String, Vec<u8>>) -> bool {
    const FILEINFO: &str = "http://namespace.corel.com/cdr/metadata/1.0/fileinfo/";
    let Some(raw) = members.get("META-INF/metadata.xml") else { return false };
    let Ok(text) = std::str::from_utf8(raw) else { return false };
    text.contains("FontsEmbedded") && roxmltree::Document::parse(text).is_ok_and(|xml| {
        xml.descendants().any(|node| node.has_tag_name((FILEINFO, "FontsEmbedded"))
            && node.text().is_some_and(|value| value.trim() == "true"))
    })
}

#[cfg(test)]
#[test]
fn native_embedding_notice_requires_a_positive_fileinfo_declaration() {
    let mut members = BTreeMap::new();
    members.insert("embed/embedding0".into(), vec![0; 2705]);
    assert!(!declares_embedded_fonts(&members));
    for (namespace, value, expected) in [
        ("http://namespace.corel.com/cdr/metadata/1.0/fileinfo/", "true", true),
        ("http://namespace.corel.com/cdr/metadata/1.0/fileinfo/", "false", false),
        ("urn:unrelated", "true", false),
    ] {
        let xml = format!(r#"<file:FontsEmbedded xmlns:file="{namespace}">{value}</file:FontsEmbedded>"#);
        members.insert("META-INF/metadata.xml".into(), xml.into_bytes());
        assert_eq!(declares_embedded_fonts(&members), expected);
    }
}

pub(super) fn find(members:&BTreeMap<String,Vec<u8>>,prefix:&str)->Result<Option<String>> {
    let mut selected=None;
    for key in ["metadata/metadata.xml","META-INF/metadata.xml"] {
        let Some(raw)=members.get(key) else{continue};if !raw.windows(prefix.len()).any(|s|s==prefix.as_bytes()){continue;}
        let text=std::str::from_utf8(raw).map_err(|_|invalid("native extension XMP is not UTF8"))?;
        let xml=roxmltree::Document::parse(text).map_err(|e|invalid(format!("native extension XMP: {e}")))?;
        for node in xml.descendants().filter(|n|n.is_element()&&n.tag_name().name()=="Notes"&&matches!(n.tag_name().namespace(),Some("http://namespace.corel.com/Cdr/metadata"|"http://namespace.corel.com/zcf/metadata/1.0/"))) {
            for text in node.descendants().filter(|n|n.is_text()).filter_map(|n|n.text()) {
                for value in text.split(';').filter(|s|s.starts_with(prefix)) {
                    if selected.is_some(){return Err(invalid("ambiguous native extension section"));}selected=Some(value.to_owned());
                }
            }
        }
    }Ok(selected)
}
pub(super) fn document(doc:&crate::Document,modern:bool)->Result<Option<Vec<u8>>> {
    document_with_extra(doc,modern,None)
}
pub(super) fn document_with_extra(doc:&crate::Document,modern:bool,extra:Option<String>)->Result<Option<Vec<u8>>> {
    let parts=[super::resource_vault::notes(doc)?,super::local_frames::notes(doc)?,super::text_params::notes(doc)?,super::image_params::notes(doc)?,super::geometry_params::notes_in_version(doc,modern)?,super::paint_params::notes(doc)?,super::paint_notation::notes(doc)?].into_iter().flatten().chain(extra).collect::<Vec<_>>();
    if parts.is_empty(){return Ok(None);}let notes=parts.join(";");
    let element=if modern{format!("<c:Notes xmlns:c=\"http://namespace.corel.com/zcf/metadata/1.0/\"><rdf:Alt><rdf:li xml:lang=\"x-default\">{notes}</rdf:li></rdf:Alt></c:Notes>")}else{format!("<c:Notes xmlns:c=\"http://namespace.corel.com/Cdr/metadata\">{notes}</c:Notes>")};
    Ok(Some(format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description rdf:about=\"\">{element}</rdf:Description></rdf:RDF></x:xmpmeta>").into_bytes()))
}
