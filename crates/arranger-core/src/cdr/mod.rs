//! Experimental native CDR14 interchange. Full-document Save remains separate:
//! this codec never hides an Arranger document inside a CDR or rasterizes vectors.
//! Explicit delivery modes (ADR-002) may outline text in a derived copy.
mod binary;
mod clipping;
mod images;
mod native_pixels;
mod fonts;
mod modern;
mod write21;
mod pages;
mod origin;
mod typography;
mod path_text;
mod path_outline;
#[cfg(test)] mod path_appearance;
mod path_guide;
mod path_rotation;
mod cyclic_source;
mod straight_shear;
mod path_shear;
mod resources;
mod resource_vault;
mod metadata;
mod local_frames;
mod text_params;
mod text_units;
mod image_params;
mod path_notation;
mod path_precision;
mod clip_precision;
mod geometry_params;
mod native_order;
mod local_data;
mod paint_params;
mod paint_layers;
mod paint_notation;
mod ink_cache;
mod group_params;
mod opacity;
mod pen;
mod primitive_frames;
mod read;
mod write;
mod practical;
pub use practical::{ExportMode,check as check_delivery,export as export_delivery};
use crate::{Document, Error, Result, command::Notice};
use serde_json::{Value, json};
use std::{io::Read, path::Path};

fn invalid(message: impl Into<String>) -> Error { Error::new("INVALID_CDR",message) }
fn unsupported(message: impl Into<String>) -> Error { Error::new("CDR_UNSUPPORTED",message) }
fn output(path:&Path,data:&[u8],warnings:Vec<Notice>,overwrite:bool)->Result<(std::path::PathBuf,Vec<Notice>)> {
    if path.extension().and_then(|s|s.to_str()).is_none_or(|s|!s.eq_ignore_ascii_case("cdr")) {return Err(Error::new("INVALID_PATH","CDR exports use .cdr"));}
    Ok((crate::native::write_export(path,data,overwrite)?,warnings))
}
pub fn export(document:&Document,page_id:&str,path:&Path,overwrite:bool)->Result<(std::path::PathBuf,Vec<Notice>)> {
    let (data,warnings)=encode(document,page_id)?;output(path,&data,warnings,overwrite)
}
pub fn export_document21(document:&Document,path:&Path,overwrite:bool)->Result<(std::path::PathBuf,Vec<Notice>)> {
    let (data,warnings)=encode_document21(document)?;output(path,&data,warnings,overwrite)
}
pub fn export21(document:&Document,page_id:&str,path:&Path,overwrite:bool)->Result<(std::path::PathBuf,Vec<Notice>)> {
    let (data,warnings)=encode21(document,page_id)?;output(path,&data,warnings,overwrite)
}
pub fn check_document21(document:&Document)->Value {
    checked_encoding(document,true,false,&encode_document21(document))
}
pub fn check21(document:&Document,page_id:&str)->Value {
    match write::selected(document,page_id) {Ok(d)=>check_document21(&d),Err(e)=>json!({"can_export":false,"status":"unsupported","format":"cdr21","error":e})}
}
pub fn export_document(document:&Document,path:&Path,overwrite:bool)->Result<(std::path::PathBuf,Vec<Notice>)> {
    let (data,warnings)=encode_document(document)?;output(path,&data,warnings,overwrite)
}
pub fn encode_document21(document:&Document)->Result<(Vec<u8>,Vec<Notice>)> {encode_with_outline_bound(document,true)}
pub fn encode_document(document:&Document)->Result<(Vec<u8>,Vec<Notice>)> {encode_with_outline_bound(document,false)}
/// Research entry only: same native cyclic-start carrier, with guarded recovery
/// of the requested Own matrix. Windows148 disproved actual Corel rotation
/// and resave recovery. Retained as a reproducer; never used by Save/export.
pub fn encode_cyclic_start_research(requested:&Document,carrier:&Document,modern:bool)->Result<(Vec<u8>,Vec<Notice>)>{cyclic_source::encode(requested,carrier,modern)}
/// Controlled native-frame experiment only. Neither Save nor ordinary export
/// uses this entry; a nonneutral output is deliberately rejected by our reader.
pub fn encode_straight_shear_research(requested:&Document,text_id:&str,modern:bool)->Result<(Vec<u8>,Vec<Notice>)>{straight_shear::encode(requested,text_id,modern)}
fn encode_with_outline_bound(document:&Document,modern:bool)->Result<(Vec<u8>,Vec<Notice>)>{
    path_shear::check(document)?;let encoded=paint_layers::encode(document,modern)?;path_outline::check_encoding(document,&encoded.0)?;Ok(encoded)
}
pub fn encode(document:&Document,page_id:&str)->Result<(Vec<u8>,Vec<Notice>)> {
    encode_selected(document,page_id,encode_document)
}
pub fn encode21(document:&Document,page_id:&str)->Result<(Vec<u8>,Vec<Notice>)> {
    encode_selected(document,page_id,encode_document21)
}
fn encode_selected(document:&Document,page_id:&str,encoder:fn(&Document)->Result<(Vec<u8>,Vec<Notice>)>)->Result<(Vec<u8>,Vec<Notice>)> {
    let selected=write::selected(document,page_id)?;
    let (data,mut warnings)=encoder(&selected)?;
    if selected.pages.len()!=document.pages.len() || selected.objects.len()!=document.objects.len() {
        warnings.push(Notice::new("CDR_PAGE_ONLY","Explicit selected-page export: other pages and shared-workspace objects are outside this output. Use cdr_export for all drawing pages."));
    }
    Ok((data,warnings))
}
pub fn decode(data:&[u8],document_id:&str)->Result<(Document,Vec<Notice>)> { read::decode(data,document_id) }
pub fn import(path:&Path,document_id:&str)->Result<(Document,Vec<Notice>)> {
    let path=crate::native::normalize(path)?;let file=std::fs::File::open(path).map_err(|e|Error::io("Read CDR",e))?;
    let mut data=vec![];file.take(binary::LIMIT as u64+1).read_to_end(&mut data).map_err(|e|Error::io("Read CDR",e))?;
    decode(&data,document_id)
}
pub fn check(document:&Document,page_id:&str)->Value {
    let selected=match write::selected(document,page_id){Ok(value)=>value,Err(error)=>return json!({"can_export":false,"status":"unsupported","format":"cdr14","error":error})};
    checked_encoding(&selected,false,true,&encode_document(&selected))
}

pub fn check_document(document:&Document)->Value {
    checked_encoding(document,false,false,&encode_document(document))
}
/// Describe this exact encode attempt without encoding it again. Save checks
/// share bytes only within one query; native decode is a separate outcome.
pub(crate) fn checked_encoding(document:&Document,modern:bool,selected:bool,result:&Result<(Vec<u8>,Vec<Notice>)>)->Value {
    let format=if modern{"cdr21"}else{"cdr14"};
    match result {
        Ok(_)=>{
            let mut value=json!({"can_export":true,"status":"experimental","format":format,"scope":if selected&&!modern{"selected page; not full-document save"}else{"all drawing pages and shared desktop; not full-fidelity save"},"precision_mm":0.0001,"corel_this_product":"unverified"});
            if modern||!selected{value["page_count"]=json!(document.pages.len());}
            value["known_native_layout_issues"]=json!(known_native_layout_issues(document));value
        },
        Err(e)=>{
            let mut value=json!({"can_export":false,"status":"unsupported","format":format,"error":e});
            if !modern{value["blockers"]=json!(encoding_blockers(document,e));}value
        },
    }
}
/// Source field recovery is independent of actual Corel layout. These are
/// unresolved profiles with measured failures, not a verdict on every shape
/// sharing the profile and not a new native acceptance check in this query.
pub(crate) fn known_native_layout_issues(document:&Document)->Vec<Value>{
    document.objects.iter().filter_map(|text|{
        let crate::Geometry::Text{text_path:Some(binding),..}=&text.geometry else{return None};
        let guide=document.object(&binding.path_id).ok()?;
        if !guide.local_path().ok()?.elements().iter().any(|e|matches!(e,kurbo::PathEl::ClosePath)){return None;}
        Some(json!({"code":"CDR_CLOSED_FIT_PLACEMENT_UNRESOLVED","object_id":text.id,"guide_id":guide.id,"status":"known_failures_in_profile","source_field_recovery_is_native_layout_proof":false,"message":"Closed-guide text placement remains unresolved. Corel uses closed-fit quadrant settings; measured controls diverged from the requested position. Exact recovered source fields do not certify this layout.","evidence":"reports/154-windows-path-affine-validation.md"}))
    }).collect()
}
fn encoding_blockers(document:&Document,error:&Error)->Vec<Value> {
    let mut blockers=write::blockers(document);
    if blockers.is_empty(){blockers.push(json!({"scope":"encoding","error":error}));}
    blockers
}

#[cfg(test)]mod native_layout_status_tests{
    use super::*;
    fn source()->Document{
        let path=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/assets/cdr/cyclic-rotation-20261105/manifest.json");
        let data:Value=serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();serde_json::from_value(data["cases"][0]["document"].clone()).unwrap()
    }
    #[test]fn exact_source_recovery_keeps_known_native_layout_problem_and_formal_save_closed(){
        let doc=source();let before=doc.hash();
        for format in[crate::save_check::SaveFormat::Cdr14,crate::save_check::SaveFormat::Cdr21]{
            let query=crate::save_check::analyze(&doc,format).unwrap();assert_eq!(query["can_save"],false);assert_eq!(query["interchange_fidelity"]["roundtrip_source_fields"]["exact"],true);assert_eq!(query["interchange_fidelity"]["known_native_layout_issues"].as_array().unwrap().len(),1);assert_eq!(query["document_interchange"]["known_native_layout_issues"],query["interchange_fidelity"]["known_native_layout_issues"]);assert_eq!(query["document_interchange"]["can_export"],true);
            let(_,warnings)=if format==crate::save_check::SaveFormat::Cdr14{encode_document(&doc)}else{encode_document21(&doc)}.unwrap();assert!(warnings.iter().any(|w|w.code=="CDR_CLOSED_FIT_PLACEMENT_UNRESOLVED"));
        }
        assert_eq!(before,doc.hash());let native=crate::save_check::analyze(&doc,crate::save_check::SaveFormat::Arranger).unwrap();assert_eq!(native["can_save"],true);assert!(native["interchange_fidelity"].is_null());
    }
    #[test]fn an_open_guide_does_not_inherit_the_closed_fit_notice(){
        let mut doc=source();let text=doc.objects.iter().find(|o|matches!(o.geometry,crate::Geometry::Text{..})).unwrap();let crate::Geometry::Text{text_path:Some(b),..}=&text.geometry else{unreachable!()};let id=b.path_id.clone();let guide=doc.objects.iter_mut().find(|o|o.id==id).unwrap();let crate::Geometry::Path{d,..}=&mut guide.geometry else{unreachable!()};*d=d.trim_end_matches(['Z','z']).into();assert!(known_native_layout_issues(&doc).is_empty());
        for modern in[false,true]{let(_,warnings)=if modern{encode_document21(&doc)}else{encode_document(&doc)}.unwrap();assert!(!warnings.iter().any(|w|w.code=="CDR_CLOSED_FIT_PLACEMENT_UNRESOLVED"));}
    }
    #[test]fn ordinary_closed_graphics_without_a_live_text_binding_have_no_fit_issue(){
        let mut doc=source();for o in &mut doc.objects{if let crate::Geometry::Text{text_path,..}=&mut o.geometry{*text_path=None;}}assert!(known_native_layout_issues(&doc).is_empty());
    }
}
