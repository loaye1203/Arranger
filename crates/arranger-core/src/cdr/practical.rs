//! Owner-approved editable/appearance delivery copies (ADR-002).
//! The complete editable source stays in .arranger. Projection never mutates
//! the source model or stores that model/derived glyph positions in the CDR.
use super::{Result,unsupported};
use crate::{Document,Error,FillRule,Geometry,command::Notice};
use serde::{Deserialize,Serialize};
use serde_json::{Value,json};
use std::path::Path;

#[derive(Clone,Copy,Debug,Default,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="snake_case")]
pub enum ExportMode { #[default] Editable, Appearance }

fn modern(format:&str)->Result<bool>{match format{"cdr14"=>Ok(false),"cdr21"=>Ok(true),_=>Err(Error::new("INVALID_PARAMETER","CDR format must be cdr14 or cdr21"))}}

struct Prepared { bytes:Vec<u8>, warnings:Vec<Notice>, report:Value }

fn cubic(path:&kurbo::BezPath)->kurbo::BezPath{
    use kurbo::{PathEl,Point};let mut result=kurbo::BezPath::new();let(mut current,mut start)=(Point::ZERO,Point::ZERO);
    for e in path.elements(){match *e{
        PathEl::MoveTo(p)=>{result.move_to(p);current=p;start=p;},PathEl::LineTo(p)=>{result.line_to(p);current=p;},
        PathEl::CurveTo(a,b,p)=>{result.curve_to(a,b,p);current=p;},
        PathEl::QuadTo(c,p)=>{let q=kurbo::QuadBez::new(current,c,p).raise();result.curve_to(q.p1,q.p2,p);current=p;},
        PathEl::ClosePath=>{if current!=start{result.line_to(start);}result.close_path();current=start;},
    }}result
}

fn verify_projection(expected:&Document,actual:&Document,converted:&[String])->Result<f64>{
    let mut canonical=expected.clone();let mut maximum=0f64;
    for id in converted{if !matches!(expected.object(id)?.geometry,Geometry::Path{..}){return Err(unsupported("outlined delivery object is no longer a native path"));}}
    // Explicit document conversion already produces paths before this export.
    // Font outlines can have f32 coordinate spellings that the native integer
    // point table cannot reproduce byte for byte. Apply the same physical
    // budget to those paths as to this export's derived appearance outlines.
    for a in &expected.objects{
        let Geometry::Path{..}=&a.geometry else{continue};let id=&a.id;let b=actual.object(id)?;
        let (Geometry::Path{d:ad,fill_rule:ar},Geometry::Path{d:bd,fill_rule:br})=(&a.geometry,&b.geometry)else{return Err(unsupported("outlined delivery object is no longer a native path"));};
        if ar!=br{return Err(unsupported("outlined delivery winding changed"));}
        let (ap,bp)=(cubic(&crate::path::parse(ad)?),cubic(&crate::path::parse(bd)?));
        if ap.elements().len()!=bp.elements().len()||ap.elements().iter().zip(bp.elements()).any(|(a,b)|std::mem::discriminant(a)!=std::mem::discriminant(b)){return Err(unsupported(format!("{id}: delivery path topology changed")));}
        if !super::path_precision::same_world(&ap,expected.world_transform(a)?,&bp,actual.world_transform(b)?){return Err(unsupported(format!("{id}: derived outline exceeds the existing0.00021mm control-point budget")));}
        let points=|p:&kurbo::BezPath|p.elements().iter().flat_map(|e|match *e{kurbo::PathEl::MoveTo(p)|kurbo::PathEl::LineTo(p)=>vec![p],kurbo::PathEl::CurveTo(a,b,p)=>vec![a,b,p],kurbo::PathEl::QuadTo(c,p)=>vec![c,p],kurbo::PathEl::ClosePath=>vec![]}).collect::<Vec<_>>();
        for(p,q)in points(&ap).into_iter().zip(points(&bp)){let(p,q)=(expected.world_transform(a)?*p,actual.world_transform(b)?*q);maximum=maximum.max((p.x-q.x).abs()).max((p.y-q.y).abs());}
        canonical.objects.iter_mut().find(|o|o.id==*id).expect("source path").geometry=b.geometry.clone();
    }
    if canonical!=*actual{
        let differences=expected.objects.iter().zip(&actual.objects).filter(|(a,b)|a!=b).map(|(a,_)|a.id.clone()).take(8).collect::<Vec<_>>();
        return Err(unsupported(format!("delivery fields outside the approved outline projection changed ({differences:?}); no file was written")));
    }Ok(maximum)
}

fn project(source:&Document,mode:ExportMode)->Result<(Document,Vec<String>,Vec<String>,Vec<Notice>)>{
    source.validate()?;let mut copy=source.clone();let mut converted=vec![];let mut live=vec![];let mut warnings=vec![];
    for original in &source.objects {
        if !matches!(original.geometry,Geometry::Text{..}){continue;}
        if mode==ExportMode::Editable {live.push(original.id.clone());continue;}
        let layout=crate::text::layout(source,original)?;crate::text::require_complete(&layout)?;
        if !layout.outline_supported{return Err(Error::new("UNSUPPORTED_GLYPH_OUTLINE",format!("{}: appearance export requires monochrome vector glyphs",original.id)));}
        warnings.extend(layout.warnings.clone());
        // An empty/whitespace story has no ink to outline. Retain that native
        // story rather than producing an invalid empty drawing path.
        if layout.outline.elements().is_empty(){live.push(original.id.clone());continue;}
        let target=copy.objects.iter_mut().find(|o|o.id==original.id).expect("source clone");
        target.geometry=Geometry::Path{d:layout.outline.to_svg(),fill_rule:FillRule::NonZero};
        // Keep the original local frame, clipping, hierarchy, dimensions and
        // lock state. Coordinates in the outline already use that same frame.
        converted.push(original.id.clone());
    }
    copy.validate()?;Ok((copy,converted,live,warnings))
}

fn prepare(source:&Document,format:&str,mode:ExportMode,page_id:Option<&str>)->Result<Prepared>{
    let modern=modern(format)?;
    let selected=if let Some(page)=page_id{super::write::selected(source,page)?}else{source.clone()};
    let (projected,converted,live,mut warnings)=project(&selected,mode)?;
    if !modern&&projected.objects.iter().any(|o|matches!(o.geometry,Geometry::Group)&&o.style.opacity!=1.){
        return Err(Error::new("CDR_REQUIRES_VERSION21","CDR14 does not preserve the measured isolated group transparency after Corel resave. Select CDR21; no transparency was dropped or file written."));
    }
    let encoded=if modern{super::encode_document21(&projected)}else{super::encode_document(&projected)};
    let (bytes,codec_warnings)=match encoded{
        Ok(value)=>value,
        Err(error)=>{
            // Layered paints can introduce an isolated group too. Recommend
            // another version only if this exact projected model encodes there.
            if !modern&&error.message.contains("isolated group opacity")&&super::encode_document21(&projected).is_ok(){return Err(Error::new("CDR_REQUIRES_VERSION21","This paint combination needs isolated group transparency. Select CDR21; CDR14 was not written."));}
            return Err(error);
        }
    };
    let (readback,_)=super::decode(&bytes,&projected.id)?;
    let outline_error=verify_projection(&projected,&readback,&converted)?;
    warnings.extend(codec_warnings.into_iter().filter(|n|n.code!="CDR_EXPERIMENTAL"));
    warnings.push(Notice::new("CDR_SOURCE_REQUIRED","This CDR is a delivery copy. Keep the complete editable .arranger original; export leaves its model, history and save binding unchanged."));
    if mode==ExportMode::Editable&&!live.is_empty(){warnings.push(Notice::new("CDR_EDITABLE_LAYOUT","Live text uses Corel's native layout. Fonts, spacing and fitted/curved text can differ from the Arranger display. Choose appearance mode for outlined text instead."));}
    if !converted.is_empty(){warnings.push(Notice::new("CDR_TEXT_OUTLINED",&format!("Appearance copy: {} text object(s) became editable vector outlines; their characters cannot be edited in Corel. The .arranger original retains all text.",converted.len())));}
    let report=json!({"can_export":true,"format":format,"mode":mode,"document_hash":source.hash(),"default_save_format":"arranger","full_fidelity_native_save":false,
        "scope":if page_id.is_some(){"selected_page"}else{"all_pages_and_workspace"},"page_id":page_id,"page_count":projected.pages.len(),"object_count":projected.objects.len(),
        "converted_text_ids":converted,"live_text_ids":live,"roundtrip_projected_fields_exact":readback==projected,"roundtrip_projected_semantics_verified":true,"maximum_outline_control_difference_mm":outline_error,"maximum_path_control_difference_mm":outline_error,"outline_control_budget_mm":0.00021,"source_document_unchanged":true,"warnings":warnings});
    Ok(Prepared{bytes,warnings,report})
}

pub fn check(source:&Document,format:&str,mode:ExportMode,page_id:Option<&str>)->Result<Value>{
    modern(format)?;source.validate()?;
    Ok(match prepare(source,format,mode,page_id){
        Ok(p)=>p.report,
        Err(error)=>json!({"can_export":false,"format":format,"mode":mode,"document_hash":source.hash(),"default_save_format":"arranger","full_fidelity_native_save":false,"page_id":page_id,
            "suggested_format":if error.code=="CDR_REQUIRES_VERSION21"{Some("cdr21")}else{None},"error":error}),
    })
}

pub fn export(source:&Document,format:&str,mode:ExportMode,page_id:Option<&str>,path:&Path,overwrite:bool)->Result<Value>{
    let p=prepare(source,format,mode,page_id)?;let(path,_)=super::output(path,&p.bytes,p.warnings,overwrite)?;
    let mut report=p.report;report["path"]=json!(path);Ok(report)
}

#[cfg(test)]mod tests{
    use super::*;
    fn outlined_source()->Document{
        let mut d=Document::new("curves".into(),210.,297.).unwrap();
        d.objects.push(serde_json::from_value(json!({"id":"outline","page_id":"page-1","x_mm":20.,"y_mm":30.,"width_mm":40.,"height_mm":20.,"geometry":{"kind":"path","d":"M3.2280001640319824 4.368000030517578 C3.4840002059936523 4.368000030517578 3.616000175476074 4.236000061035156 3.616000175476074 3.9719998836517334 L3.2280001640319824 4.368000030517578 Z"},"style":{"fill":"#123456","stroke":"none","stroke_width_mm":0.2,"opacity":1.}})).unwrap());d
    }
    #[test]fn preexisting_outline_paths_export_with_bounded_native_precision_in_both_modes(){
        let d=outlined_source();let original=d.clone();
        for format in["cdr14","cdr21"]{for mode in[ExportMode::Editable,ExportMode::Appearance]{
            let p=prepare(&d,format,mode,None).unwrap();let (read,_)=super::super::decode(&p.bytes,&d.id).unwrap();
            assert!(matches!(read.objects[0].geometry,Geometry::Path{..}));assert_ne!(read,d);
            assert_eq!(p.report["converted_text_ids"],json!([]));assert_eq!(p.report["roundtrip_projected_fields_exact"],false);
            let error=p.report["maximum_path_control_difference_mm"].as_f64().unwrap();assert!(error>0.&&error<=0.00021);
        }}assert_eq!(d,original);
    }
    #[test]fn native_path_normalization_still_rejects_geometry_topology_and_other_field_changes(){
        let d=outlined_source();let mut changed=d.clone();
        changed.objects[0].geometry=Geometry::Path{d:"M3.23 4.368 C3.484 4.368 3.616 4.236 3.616 3.972 L3.23 4.368 Z".into(),fill_rule:FillRule::NonZero};
        assert!(verify_projection(&d,&changed,&[]).is_err());
        // Same ordered coordinate list, different subpath boundaries.
        changed.objects[0].geometry=Geometry::Path{d:"M3.2280001640319824 4.368000030517578 C3.4840002059936523 4.368000030517578 3.616000175476074 4.236000061035156 3.616000175476074 3.9719998836517334 M3.2280001640319824 4.368000030517578 Z".into(),fill_rule:FillRule::NonZero};
        assert!(verify_projection(&d,&changed,&[]).unwrap_err().message.contains("topology"));
        changed=d.clone();if let Geometry::Path{fill_rule,..}=&mut changed.objects[0].geometry{*fill_rule=FillRule::EvenOdd;}assert!(verify_projection(&d,&changed,&[]).is_err());
        changed=d.clone();changed.objects[0].style.fill="#654321".into();assert!(verify_projection(&d,&changed,&[]).is_err());
        changed=d.clone();changed.objects[0].locked=true;assert!(verify_projection(&d,&changed,&[]).is_err());
        changed=d.clone();changed.pages[0].width_mm=211.;assert!(verify_projection(&d,&changed,&[]).is_err());
        changed=d.clone();changed.objects[0].transform=[10000.,0.,0.,10000.,0.,0.];let mut tiny=changed.clone();if let Geometry::Path{d,..}=&mut tiny.objects[0].geometry{*d=d.replace("3.2280001640319824","3.2280001940319824");}assert!(verify_projection(&changed,&tiny,&[]).is_err());
    }
    fn source()->Document{
        let mut d=Document::new("source".into(),210.,297.).unwrap();
        d.objects.push(serde_json::from_value(json!({"id":"guide","page_id":"page-1","x_mm":0.,"y_mm":0.,"width_mm":100.,"height_mm":40.,"geometry":{"kind":"path","d":"M20 60 C40 20 80 100 120 60"},"style":{"fill":"none","stroke":"none","stroke_width_mm":0.,"opacity":1.}})).unwrap());
        d.objects.push(serde_json::from_value(json!({"id":"text","page_id":"page-1","x_mm":0.,"y_mm":0.,"width_mm":80.,"height_mm":10.,"geometry":{"kind":"text","content":"HAM42","font_family":"Arial","font_size_mm":4.,"text_path":{"path_id":"guide","start_offset_mm":20.,"baseline_offset_mm":0.125,"anchor":"start"}}})).unwrap());d
    }
    #[test]fn appearance_outlines_on_a_copy_and_keeps_the_real_guide_and_source(){
        let mut d=source();d.objects[1].locked=true;let before=d.clone();let(out,ids,_,_)=project(&d,ExportMode::Appearance).unwrap();assert_eq!(d,before);assert_eq!(ids,vec!["text"]);assert_eq!(out.objects[0],d.objects[0]);assert!(matches!(out.objects[1].geometry,Geometry::Path{..}));assert!(out.objects[1].locked);assert_eq!(out.resources,d.resources);
        for format in["cdr14","cdr21"]{let prepared=prepare(&d,format,ExportMode::Appearance,None).unwrap();assert_eq!(prepared.report["converted_text_ids"],json!(["text"]));assert!(prepared.report["roundtrip_projected_semantics_verified"].as_bool().unwrap());assert!(!prepared.warnings.iter().any(|n|n.code=="CDR_EXPERIMENTAL"));}
    }
    #[test]fn editable_keeps_a_live_story_and_reports_the_layout_contract(){
        let d=source();let p=prepare(&d,"cdr21",ExportMode::Editable,None).unwrap();assert!(p.warnings.iter().any(|n|n.code=="CDR_EDITABLE_LAYOUT"));let(q,_)=super::super::decode(&p.bytes,&d.id).unwrap();assert_eq!(q,d);assert!(matches!(q.objects[1].geometry,Geometry::Text{text_path:Some(_),..}));
    }
    #[test]fn appearance_handles_general_own_transform_without_simplifying_the_guide(){
        let mut d=source();d.objects[1].transform=[1.2,0.2,0.3,0.8,2.,3.];assert!(!check(&d,"cdr21",ExportMode::Editable,None).unwrap()["can_export"].as_bool().unwrap());assert!(check(&d,"cdr21",ExportMode::Appearance,None).unwrap()["can_export"].as_bool().unwrap());
    }
    #[test]fn group_transparency_never_silently_drops_or_changes_file_version(){
        let mut d=Document::new("alpha".into(),210.,297.).unwrap();d.objects.push(serde_json::from_value(json!({"id":"group","page_id":"page-1","x_mm":0.,"y_mm":0.,"width_mm":0.,"height_mm":0.,"geometry":{"kind":"group"},"style":{"fill":"none","stroke":"none","stroke_width_mm":0.,"opacity":0.7}})).unwrap());
        d.objects.push(serde_json::from_value(json!({"id":"rect","parent_id":"group","page_id":"page-1","x_mm":20.,"y_mm":30.,"width_mm":40.,"height_mm":30.,"geometry":{"kind":"rect","radius_mm":0.}})).unwrap());
        for mode in[ExportMode::Editable,ExportMode::Appearance]{let r=check(&d,"cdr14",mode,None).unwrap();assert_eq!(r["can_export"],false);assert_eq!(r["error"]["code"],"CDR_REQUIRES_VERSION21");assert_eq!(r["suggested_format"],"cdr21");assert_eq!(check(&d,"cdr21",mode,None).unwrap()["can_export"],true);}
    }
}
