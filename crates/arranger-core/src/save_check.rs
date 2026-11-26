//! Read-only, document-specific preservation inventory. This reports codec
//! implementation evidence, never whether the CDR specification can hold a field.
use crate::{Document, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveFormat { Arranger, Cdr14, Cdr21 }

fn feature(path: &str, object: Option<&Value>) -> &'static str {
    let field = path.rsplit('/').next().unwrap_or("");
    if path.starts_with("/resources/") { return "original_image_resource"; }
    if path.starts_with("/pages/") { return "pages_and_ownership"; }
    if !path.starts_with("/objects/") { return "document_identity_and_units"; }
    if path.contains("/transform") { return "affine_transform"; }
    if path.contains("/clip") { return "non_destructive_clip"; }
    if path.contains("/style/") {
        return match field {
            "fill" | "stroke" | "stroke_width_mm" => "paint_and_outline",
            "opacity" => "object_or_group_opacity",
            _ => "outline_details",
        };
    }
    if path.contains("/geometry/") {
        if path.contains("/text_path") { return "live_text_path"; }
        if path.contains("/layout") { return "paragraph_layout"; }
        return match object.map(|o| &o["geometry"]["kind"]).and_then(Value::as_str) {
            Some("text") => "editable_text",
            Some("image") => "shared_image_and_fit",
            Some("group") => "group_hierarchy",
            _ => "editable_vector_geometry",
        };
    }
    match field {
        "id" => "stable_object_identity",
        "page_id" | "parent_id" => "pages_and_ownership",
        "visible" | "locked" => "visibility_and_lock",
        _ => "local_geometry_and_order",
    }
}

fn experiment_evidence(feature: &str, path: &str, object: Option<&Value>, value: &Value) -> bool {
    let Some(object) = object else { return false; };
    match feature {
        "paint_and_outline" => value.as_str().is_none_or(|s| crate::paint::parse(s).is_ok_and(|p|p==crate::paint::Paint::None||p.alpha()==1.)),
        "editable_vector_geometry" => match object["geometry"]["kind"].as_str() {
            Some("rect") => object["geometry"]["radius_mm"].as_f64().unwrap_or(0.) == 0.,
            Some("ellipse") => true,
            _ => false, // Native evidence only covers one open cubic, not arbitrary SVG paths.
        },
        "editable_text" => {
            let g = &object["geometry"];
            matches!(g["font_family"].as_str(), Some("Microsoft YaHei" | "微软雅黑"))
                && g["text_path"].is_null() && g.get("layout").is_none()
                && g["content"].as_str().is_some_and(|s| !s.is_empty() && s.chars().all(|c| {
                    (c == '\r' || c == '\n' || (!c.is_control() && c as u32 <= 0xffff)) && c != '\0'
                }))
        },
        "local_geometry_and_order" => !path.ends_with("/parent_id"),
        _ => false,
    }
}

fn leaves(value: &Value, path: &str, output: &mut Vec<(String, Value)>) {
    match value {
        Value::Object(map) if !map.is_empty() => for (key, value) in map {
            let key = key.replace('~', "~0").replace('/', "~1");
            leaves(value, &format!("{path}/{key}"), output);
        },
        Value::Array(items) if !items.is_empty() => for (index, value) in items.iter().enumerate() {
            leaves(value, &format!("{path}/{index}"), output);
        },
        _ => output.push((path.into(), value.clone())),
    }
}

fn field_summary(value: &Value) -> Value {
    match value {
        Value::String(text) if text.len()>512 => json!({"type":"string","utf8_bytes":text.len(),"sha256":crate::model::sha256(text.as_bytes())}),
        Value::Array(values)=>Value::Array(values.iter().map(field_summary).collect()),
        Value::Object(values)=>Value::Object(values.iter().map(|(key,value)|(key.clone(),field_summary(value))).collect()),
        _=>value.clone(),
    }
}

#[cfg(test)]
mod fidelity_tests {
    use super::*;
    #[test]
    fn removed_objects_and_long_strings_are_reported_without_copying_their_payload() {
        let source=Document::new("summary".into(),210.,297.).unwrap();
        let long="M0.1234567 0 L1 1 ".repeat(200);
        let expected=json!({"objects":[{"geometry":{"d":long}}]});let actual=json!({"objects":[]});
        let mut differences=vec![];field_differences(Some(&expected),Some(&actual),"",&source,&mut differences);
        assert_eq!(differences[0]["path"],"/objects/0");
        assert_eq!(differences[0]["expected"]["geometry"]["d"]["sha256"],crate::model::sha256(long.as_bytes()));
        assert!(!serde_json::to_string(&differences).unwrap().contains(&long));
    }
    #[test]
    fn resource_payload_differences_keep_the_serialized_field_path_and_only_summaries() {
        let source=Document::new("summary".into(),210.,297.).unwrap();
        let (a,b)=(json!({"encoded_characters":100,"encoded_sha256":"a"}),json!({"encoded_characters":104,"encoded_sha256":"b"}));
        let mut differences=vec![];field_differences(Some(&a),Some(&b),"/resources/image/data_base64",&source,&mut differences);
        assert_eq!(differences.len(),1);assert_eq!(differences[0]["path"],"/resources/image/data_base64");assert_eq!(differences[0]["expected"],a);assert_eq!(differences[0]["actual"],b);
    }
}
fn pointer_key(key:&str)->String{key.replace('~',"~0").replace('/',"~1")}
fn fidelity_fields(document:&Document)->Value {
    // Compare every serialized resource field without cloning its base64
    // payload into the report or another document-sized JSON allocation.
    let resources:serde_json::Map<String,Value>=document.resources.iter().map(|(id,r)|{
        let padding=r.data_base64.as_bytes().iter().rev().take_while(|v|**v==b'=').count();
        (id.clone(),json!({"mime":r.mime,"sha256":r.sha256,"data_base64":{"decoded_bytes":r.data_base64.len()/4*3-padding,"encoded_characters":r.data_base64.len(),"encoded_sha256":crate::model::sha256(r.data_base64.as_bytes())}}))
    }).collect();
    json!({"id":document.id,"unit":document.unit,"pages":document.pages,"objects":document.objects,"resources":resources})
}
fn field_differences(expected:Option<&Value>,actual:Option<&Value>,path:&str,source:&Document,output:&mut Vec<Value>) {
    if expected==actual{return;}
    match (expected,actual) {
        (Some(Value::Object(a)),Some(Value::Object(b))) if !(path.starts_with("/resources/")&&path.ends_with("/data_base64"))=>{
            let keys=a.keys().chain(b.keys()).collect::<BTreeSet<_>>();
            for key in keys{field_differences(a.get(key),b.get(key),&format!("{path}/{}",pointer_key(key)),source,output);}
        },
        (Some(Value::Array(a)),Some(Value::Array(b)))=>{
            for index in 0..a.len().max(b.len()){field_differences(a.get(index),b.get(index),&format!("{path}/{index}"),source,output);}
        },
        _=>{
            let object_id=path.strip_prefix("/objects/").and_then(|p|p.split('/').next()).and_then(|i|i.parse::<usize>().ok()).and_then(|i|source.objects.get(i)).map(|o|&o.id);
            output.push(json!({"path":path,"object_id":object_id,"change":if expected.is_none(){"added"}else if actual.is_none(){"removed"}else{"changed"},"expected":expected.map(field_summary),"actual":actual.map(field_summary)}));
        },
    }
}

/// A measured source-field roundtrip, independent of the formal Save gate and
/// native compatibility. Only explicit save_check requests pay this codec cost.
fn interchange_fidelity(document:&Document,result:&Result<(Vec<u8>,Vec<crate::command::Notice>)>)->Value {
    let mut report=interchange_roundtrip(document,result);
    report["known_native_layout_issues"]=json!(crate::cdr::known_native_layout_issues(document));report
}
fn interchange_roundtrip(document:&Document,result:&Result<(Vec<u8>,Vec<crate::command::Notice>)>)->Value {
    let (bytes,_)=match result {
        Ok(value)=>value,
        Err(error)=>return json!({"encoding_status":"rejected","roundtrip_status":"not_attempted","roundtrip_source_fields":{"exact":null,"difference_count":null,"differences":[]},"error":error,"native_compatibility":"not_tested_by_this_query"}),
    };
    let actual=match crate::cdr::decode(bytes,&document.id) {
        Ok((actual,_))=>actual,
        Err(error)=>return json!({"encoding_status":"encoded","roundtrip_status":"rejected","roundtrip_source_fields":{"exact":null,"difference_count":null,"differences":[]},"error":error,"native_compatibility":"not_tested_by_this_query"}),
    };
    let mut differences=vec![];
    let expected=fidelity_fields(document);let actual=fidelity_fields(&actual);
    field_differences(Some(&expected),Some(&actual),"",document,&mut differences);
    json!({"encoding_status":"encoded","roundtrip_status":"decoded","roundtrip_source_fields":{"exact":differences.is_empty(),"difference_count":differences.len(),"differences":differences},"native_compatibility":"not_tested_by_this_query"})
}

fn interchange_reports(document:&Document,format:SaveFormat)->(Value,Value,Value) {
    if format==SaveFormat::Arranger{return (Value::Null,Value::Null,Value::Null);}
    let modern=format==SaveFormat::Cdr21;
    let encoded=if modern{crate::cdr::encode_document21(document)}else{crate::cdr::encode_document(document)};
    let full=crate::cdr::checked_encoding(document,modern,false,&encoded);
    // Selected-page export excludes shared objects even for a one-page file.
    // Reuse only when selecting the first page leaves this model unchanged.
    let same_page=document.pages.len()==1&&document.objects.iter().all(|o|o.page_id==document.pages[0].id);
    let selected=if same_page{crate::cdr::checked_encoding(document,modern,true,&encoded)}else if modern{crate::cdr::check21(document,&document.pages[0].id)}else{crate::cdr::check(document,&document.pages[0].id)};
    (interchange_fidelity(document,&encoded),full,selected)
}

pub fn analyze(document: &Document, format: SaveFormat) -> Result<Value> {
    document.validate()?;
    // Keep the same field paths while avoiding another full copy of original
    // base64 resources just to enumerate their three serialized fields.
    let resources_for_inventory: serde_json::Map<String, Value> = document.resources.iter().map(|(id,r)| {
        (id.clone(),json!({"mime":r.mime,"sha256":r.sha256,"data_base64":null}))
    }).collect();
    let serialized = json!({"id":document.id,"unit":document.unit,"pages":document.pages,
        "objects":document.objects,"resources":resources_for_inventory});
    let mut fields = vec![];
    leaves(&serialized, "", &mut fields);
    let mut requirements = vec![];
    for (path, value) in fields {
        let index = path.strip_prefix("/objects/").and_then(|p| p.split('/').next()).and_then(|s| s.parse::<usize>().ok());
        let object = index.and_then(|i| serialized["objects"].get(i));
        let category = feature(&path, object);
        let status = match format {
            SaveFormat::Arranger => "implemented",
            SaveFormat::Cdr14 if experiment_evidence(category, &path, object, &value) => "experiment_only",
            _ => "not_implemented",
        };
        requirements.push(json!({"path":path,"feature":category,"object_id":object.map(|o| &o["id"]),"status":status}));
    }
    // Blob bytes stay in the document/resource API; do not duplicate base64 in this query.
    let resources: Vec<_> = document.resources.iter().map(|(id, r)| {
        let refs: Vec<_> = document.objects.iter().filter_map(|o| match &o.geometry {
            crate::Geometry::Image { resource_id, .. } if resource_id == id => Some(&o.id), _ => None,
        }).collect();
        json!({"id":id,"mime":r.mime,"sha256":r.sha256,"object_ids":refs,"image":crate::resource::validate(r).ok()})
    }).collect();
    let available = format == SaveFormat::Arranger;
    let(fidelity,document_interchange,interchange)=interchange_reports(document,format);
    Ok(json!({
        "format":format,"document_hash":document.hash(),"can_save":available,
        "full_document_save_available":available,"default_save_format":"arranger",
        "format_capacity":"not_determined_by_implementation_inventory",
        "blocking_codes":if available { vec![] } else { vec!["FORMAL_CDR_CODEC_NOT_IMPLEMENTED"] },
        "scope":"Current product codec coverage, not Corel compatibility or a format-capacity verdict",
        "counts":{"pages":document.pages.len(),"objects":document.objects.len(),"resources":document.resources.len(),
            "workspace_objects":document.objects.iter().filter(|o|o.page_id==crate::WORKSPACE_PAGE).count(),"fields":requirements.len()},
        "resources":resources,"requirements":requirements,
        "interchange_fidelity":fidelity,
        "document_interchange":document_interchange,
        "interchange":interchange,
        "next_format_policy":["cdr14","cdr21","own_format_only_after_evidence"],
    }))
}
