//! Exact alpha/opacity decomposition of one native paint. Only bounded
//! scalar parameters in standard ObjectData; native paints/effects stay live.
//! float_alpha preserves continuous alpha without changing legacy byte fields.
use super::{binary::*,invalid,unsupported,Result};
use crate::{Document,Geometry,Object,LineCap,LineJoin,paint::Paint};
use serde::{Deserialize,Serialize};
use std::collections::BTreeMap;
const PREFIX:&str="Arranger_PaintV1_";
const NOTES:&str="ArrangerPaintParametersV1|sha256:";
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PaintNotes{version:u32,objects:BTreeMap<String,LocalPaint>}
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalPaint{part:String,alpha:u8,#[serde(default,skip_serializing_if="Option::is_none")]float_alpha:Option<f64>,opacity:f64,#[serde(default,skip_serializing_if="Option::is_none")]inactive_outline:Option<InactiveOutline>,#[serde(default,skip_serializing_if="Option::is_none")]active_outline_width_mm:Option<f64>}
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct InactiveOutline{width_mm:f64,cap:LineCap,join:LineJoin,miter:f64,#[serde(default="crate::model::default_true",skip_serializing_if="crate::model::is_true")]scale_with_object:bool,#[serde(default,skip_serializing_if="Option::is_none")]fixed_nib:Option<crate::FixedStrokeNib>}
pub(super) fn notes(doc:&Document)->Result<Option<String>> {
    let mut objects=BTreeMap::new();for o in &doc.objects {
        let p=parameters(o)?;let s=&o.style;
        let inactive_outline=(s.stroke=="none"&&!matches!(o.geometry,Geometry::Group)).then(||InactiveOutline{width_mm:s.stroke_width_mm,cap:s.stroke_linecap,join:s.stroke_linejoin,miter:s.stroke_miterlimit,scale_with_object:s.stroke_scale_with_object,fixed_nib:s.stroke_fixed_nib});
        // Active width restoration belongs to the final native-frame guard,
        // where pen units and geometry share one world-displacement budget.
        // Other geometry remains native until its own outline guard exists.
        let active_outline_width_mm=(s.stroke!="none"&&matches!(o.geometry,Geometry::Rect{..}|Geometry::Ellipse|Geometry::Path{..}|Geometry::Text{text_path:None,..})&&s.stroke_width_mm!=coord(s.stroke_width_mm)? as f64/10000.).then_some(s.stroke_width_mm);
        if p.part!="opacity"||native_value(p).to_bits()!=p.opacity.to_bits()||inactive_outline.is_some()||active_outline_width_mm.is_some(){objects.insert(o.id.clone(),LocalPaint{part:p.part.into(),alpha:p.alpha,float_alpha:p.float_alpha,opacity:p.opacity,inactive_outline,active_outline_width_mm});}
    }
    if objects.is_empty(){return Ok(None);}super::metadata::encode(NOTES,&PaintNotes{version:1,objects}).map(Some)
}
pub(super) fn read(members:&BTreeMap<String,Vec<u8>>)->Result<Option<PaintNotes>> {super::metadata::find(members,NOTES)?.map(|s|super::metadata::decode(NOTES,&s)).transpose()}
pub(super) fn active_widths(notes:&PaintNotes)->Result<BTreeMap<String,f64>> {
    if notes.version!=1{return Err(unsupported("unknown paint scalar version"));}
    let mut widths=BTreeMap::new();
    for(id,p)in &notes.objects {
        if !crate::model::valid_id(id){return Err(invalid("invalid paint scalar name"));}
        if let Some(alpha)=p.float_alpha {if p.alpha!=255||p.part=="opacity"||!alpha.is_finite()||!(0.0..=1.0).contains(&alpha)||alpha.is_sign_negative(){return Err(invalid("invalid floating paint alpha scalar"));}}
        if let Some(width)=p.active_outline_width_mm {
            if !width.is_finite()||width<0.||p.inactive_outline.is_some(){return Err(invalid("invalid active outline width scalar"));}
            coord(width)?;widths.insert(id.clone(),width);
        }
    }Ok(widths)
}
pub(super) fn restore_notes(doc:&mut Document,notes:PaintNotes,modern:bool)->Result<(usize,usize)> {
    // Validate active scalars, but never overwrite native widths here: the
    // local-frame pass must retain the original pen as its comparison input.
    active_widths(&notes)?;let mut exact=0;let mut native=0;
    for(id,p)in notes.objects {
        if !crate::model::valid_id(&id)||!p.opacity.is_finite()||!(0.0..=1.0).contains(&p.opacity){return Err(invalid("invalid paint scalar"));}
        let part=match p.part.as_str(){"opacity"=>"opacity","fill"=>"fill","outline"=>"outline",_=>return Err(unsupported("unknown paint scalar target"))};
        if part=="opacity"&&p.alpha!=255{return Err(invalid("opacity scalar has paint alpha"));}
        let Some(o)=doc.objects.iter_mut().find(|o|o.id==id)else{continue};
        if let Some(a)=p.inactive_outline {
            if !a.width_mm.is_finite()||a.width_mm<0.||!a.miter.is_finite()||a.miter<1.{return Err(invalid("invalid inactive outline scalar"));}
            if o.style.stroke=="none"&&!matches!(o.geometry,Geometry::Group){o.style.stroke_width_mm=a.width_mm;o.style.stroke_linecap=a.cap;o.style.stroke_linejoin=a.join;o.style.stroke_miterlimit=a.miter;o.style.stroke_scale_with_object=a.scale_with_object;o.style.stroke_fixed_nib=a.fixed_nib;}
        }
        let expected=Params{part,alpha:p.alpha,float_alpha:p.float_alpha,opacity:p.opacity};
        if part!="opacity"&&o.style.fill!="none"&&o.style.stroke!="none"{
            let value=if part=="fill"{&mut o.style.fill}else{&mut o.style.stroke};let paint=crate::paint::parse(value)?;
            // Compare the current native effect's projection, not its source
            // spelling. A changed scope/percentage must retain the new paint.
            if p.opacity==1.&&o.style.opacity==1.&&matches!(paint,Paint::Rgba(_)|Paint::RgbaFloat{..})&&native_value(expected)==projected(paint.alpha()) {
                *value=with_alpha(paint,expected)?;exact+=1;
            }else{native+=1;}continue;
        }
        if matches!(o.geometry,Geometry::Group)&&(!modern||part!="opacity")||o.style.opacity!=native_value(expected){native+=1;continue;}
        if part!="opacity" {
            if matches!(o.geometry,Geometry::Image{..})||o.style.fill!="none"&&o.style.stroke!="none"{native+=1;continue;}
            let value=if part=="fill"{&mut o.style.fill}else{&mut o.style.stroke};
            let paint=crate::paint::parse(value)?;if !matches!(paint,Paint::Rgba([_,_,_,255])){native+=1;continue;}*value=with_alpha(paint,expected)?;
        }
        o.style.opacity=p.opacity;exact+=1;
    }Ok((exact,native))
}
#[derive(Clone,Copy)]
struct Params {part:&'static str,alpha:u8,float_alpha:Option<f64>,opacity:f64}
fn projected(alpha:f64)->f64 {(100.-super::opacity::percent(alpha))/100.}
fn with_alpha(paint:Paint,p:Params)->Result<String> {
    let [r,g,b,_]=paint.rgba();if !matches!(paint,Paint::Rgba(_)|Paint::RgbaFloat{..}){return Err(unsupported("paint scalar requires a native RGB paint"));}
    Ok(if let Some(alpha)=p.float_alpha{format!("rgba({r},{g},{b},{alpha})")}else{format!("#{r:02x}{g:02x}{b:02x}{:02x}",p.alpha)})
}
fn parameters(o:&Object)->Result<Params> {
    let mut params=Params{part:"opacity",alpha:255,float_alpha:None,opacity:o.style.opacity};
    if matches!(o.geometry,Geometry::Group){return Ok(params);}
    for (part,value) in [("fill",&o.style.fill),("outline",&o.style.stroke)] {
        let paint=crate::paint::parse(value)?;
        if matches!(paint,Paint::RgbaFloat{..})||matches!(paint,Paint::Rgba(_))&&value.len()==9 {
            if paint.alpha()==1.&&(matches!(o.geometry,Geometry::Group|Geometry::Image{..})||o.style.fill!="none"&&o.style.stroke!="none"){continue;}
            if matches!(o.geometry,Geometry::Group|Geometry::Image{..}){return Err(unsupported("paint alpha on groups/images requires native preservation support"));}
            if o.style.fill!="none"&&o.style.stroke!="none" {
                if !matches!(o.geometry,Geometry::Rect{..}|Geometry::Ellipse|Geometry::Path{..}|Geometry::Text{..})||o.style.opacity!=1.||o.clip.is_some()||params.part!="opacity"{return Err(unsupported("combined paints require at most one alpha on an unclipped vector or text story with whole opacity1"));}
            }
            params.part=part;match paint{Paint::Rgba([_,_,_,alpha])=>params.alpha=alpha,Paint::RgbaFloat{alpha,..}=>params.float_alpha=Some(alpha),_=>unreachable!()}
        }
    }Ok(params)
}
pub(super) fn native_scope(o:&Object)->Result<u32>{let p=parameters(o)?;Ok(if o.style.fill!="none"&&o.style.stroke!="none"{match p.part{"fill"=>0,"outline"=>1,_=>2}}else{2})}
fn combined(p:Params)->f64 {p.opacity*p.float_alpha.unwrap_or(f64::from(p.alpha)/255.)}
fn native_value(p:Params)->f64 {(100.-super::opacity::percent(combined(p)))/100.}
pub(super) fn lower(o:&mut Object)->Result<()> {
    let p=parameters(o)?;
    if p.part!="opacity" {let value=if p.part=="fill"{&mut o.style.fill}else{&mut o.style.stroke};let[r,g,b,_]=crate::paint::parse(value)?.rgba();*value=format!("#{r:02x}{g:02x}{b:02x}");}
    o.style.opacity=combined(p);Ok(())
}
#[cfg(test)]
pub(super) fn attach(nodes:&mut [Node],source:&Document)->Result<usize> {
    attach_selected(nodes,source,false)
}
#[cfg(test)]
fn attach_selected(nodes:&mut [Node],source:&Document,groups_only:bool)->Result<usize> {
    let mut entries=vec![];
    for n in all(nodes).into_iter().filter(|n|matches!(n.kind(),Some(b"obj "|b"grp "))) {
        let (_,args)=super::write::arguments(&super::read::own_record(n,b"loda")?.data)?;
        let Some((_,name))=args.iter().find(|(k,_)|*k==1000) else{continue};let name=read_wide(name)?;
        let Ok(object)=source.object(&name) else{continue};if groups_only&&!matches!(object.geometry,Geometry::Group){continue;}let p=parameters(object)?;
        if p.part=="opacity"&&native_value(p).to_bits()==p.opacity.to_bits() {continue;}
        let owner=u32_at(&super::read::own_record(n,b"usdn")?.data,0)?;
        let fields=BTreeMap::from([("Version".into(),"u32:1".into()),("Part".into(),p.part.into()),("Alpha".into(),format!("u8:{}",p.alpha)),("Opacity".into(),format!("f64:{}",p.opacity))]);
        entries.push((owner,fields));
    }
    let count=entries.len();super::local_data::append(nodes,PREFIX,entries)?;Ok(count)
}
pub(super) fn restore(doc:&mut Document,nodes:&[Node],modern:bool)->Result<usize> {
    let definitions=super::local_data::definitions(nodes,modern,PREFIX)?;if definitions.is_empty(){return Ok(0);}
    let mut owners=BTreeMap::new();
    for n in all(nodes).into_iter().filter(|n|matches!(n.kind(),Some(b"obj "|b"grp "))) {
        let Some(record)=super::read::optional_own_record(n,b"usdn")? else{continue};let owner=u32_at(&record.data,0)?;
        let (_,args)=super::write::arguments(&super::read::own_record(n,b"loda")?.data)?;
        let name=args.iter().find(|(k,_)|*k==1000).map(|(_,v)|read_wide(v)).transpose()?.unwrap_or_default();
        owners.entry(owner).or_insert_with(Vec::new).push(name);
    }
    let values=super::local_data::values(nodes,&definitions)?;if values.is_empty(){return Err(invalid("paint parameter schema has no complete owners"));}let count=values.len();
    for (owner,fields) in values {
        if fields.len()!=4||fields.get("Version").map(String::as_str)!=Some("u32:1") {return Err(unsupported("unknown paint parameter layout"));}
        let part=match fields.get("Part").map(String::as_str){Some("opacity")=>"opacity",Some("fill")=>"fill",Some("outline")=>"outline",_=>return Err(unsupported("unknown paint parameter target"))};
        let alpha=fields.get("Alpha").and_then(|s|s.strip_prefix("u8:")).and_then(|s|s.parse::<u8>().ok()).ok_or_else(||invalid("invalid paint alpha parameter"))?;
        let opacity=fields.get("Opacity").and_then(|s|s.strip_prefix("f64:")).and_then(|s|s.parse::<f64>().ok()).filter(|v|v.is_finite()&&(0.0..=1.0).contains(v)).ok_or_else(||invalid("invalid paint opacity parameter"))?;
        if part=="opacity"&&alpha!=255 {return Err(invalid("opacity-only parameter has paint alpha"));}
        let p=Params{part,alpha,float_alpha:None,opacity};let ids=owners.get(&owner).ok_or_else(||invalid("paint parameter owner missing"))?;
        if ids.len()!=1 {return Err(invalid("paint parameter owner ambiguous"));}
        let o=doc.objects.iter_mut().find(|o|o.id==ids[0]).ok_or_else(||invalid("paint parameter object missing"))?;
        if matches!(o.geometry,Geometry::Group)&&(!modern||part!="opacity")||o.style.opacity!=native_value(p) {return Err(unsupported("paint parameter attachment is stale: native effect or owner differs"));}
        if part!="opacity" {
            if matches!(o.geometry,Geometry::Image{..})||o.style.fill!="none"&&o.style.stroke!="none" {return Err(unsupported("paint parameter native single-paint mode changed"));}
            let value=if part=="fill"{&mut o.style.fill}else{&mut o.style.stroke};
            let Paint::Rgba([r,g,b,255])=crate::paint::parse(value)? else{return Err(unsupported("paint parameter native color kind changed"))};
            *value=format!("#{r:02x}{g:02x}{b:02x}{alpha:02x}");
        }
        o.style.opacity=opacity;
    }Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn outlined()->Document {
        let mut doc=source();let o=&mut doc.objects[0];o.style.fill="#123456".into();o.style.stroke="#000000".into();o.style.opacity=1.;o.style.stroke_width_mm=0.1234567;doc
    }
    fn note(doc:&Document)->PaintNotes{super::super::metadata::decode(NOTES,&notes(doc).unwrap().unwrap()).unwrap()}
    #[test]
    fn active_outline_notes_hold_one_width_scalar_without_restoring_it_before_frame_validation() {
        let source=outlined();let notes=note(&source);let values=serde_json::to_value(&notes).unwrap();let fields=values["objects"]["alpha"].as_object().unwrap();
        assert_eq!(fields.len(),4);assert_eq!(fields["active_outline_width_mm"],serde_json::json!(0.1234567));
        assert_eq!(active_widths(&notes).unwrap(),BTreeMap::from([("alpha".into(),0.1234567)]));
        let mut native=source.clone();native.objects[0].style.stroke_width_mm=0.1235;
        restore_notes(&mut native,notes,true).unwrap();assert_eq!(native.objects[0].style.stroke_width_mm,0.1235,"the final geometry guard needs the untouched native pen");
    }
    #[test]
    fn grid_widths_and_inert_group_paints_do_not_gain_redundant_scalars() {
        let mut doc=outlined();doc.objects[0].style.stroke_width_mm=0.1235;assert!(notes(&doc).unwrap().is_none());
        doc.objects[0].style.stroke_width_mm=0.1234567;doc.objects[0].geometry=Geometry::Path{d:"M0 0L12 0L12 8Z".into(),fill_rule:Default::default()};assert_eq!(active_widths(&note(&doc)).unwrap().len(),1);
        doc.objects[0].geometry=Geometry::Ellipse;assert_eq!(active_widths(&note(&doc)).unwrap().len(),1);
        doc.objects[0].geometry=Geometry::Group;assert!(notes(&doc).unwrap().is_none());
    }
    #[test]
    fn malformed_active_outline_scalars_are_rejected_without_changing_native_values() {
        let source=outlined();
        for width in [f64::NAN,f64::INFINITY,-0.001,300_000.] {
            let mut notes=note(&source);notes.objects.get_mut("alpha").unwrap().active_outline_width_mm=Some(width);
            assert!(active_widths(&notes).is_err());let mut native=source.clone();let before=native.clone();assert!(restore_notes(&mut native,notes,true).is_err());assert_eq!(native,before);
        }
        let mut notes=note(&source);notes.objects.get_mut("alpha").unwrap().inactive_outline=Some(InactiveOutline{width_mm:1.,cap:Default::default(),join:Default::default(),miter:4.,scale_with_object:true,fixed_nib:None});assert!(active_widths(&notes).is_err());
    }
    #[test]
    fn invalid_float_scalars_do_not_change_any_native_paint() {
        let mut source=outlined();source.objects[0].style.fill="rgba(30,90,180,0.5)".into();
        for alpha in [f64::NAN,f64::INFINITY,-0.1,1.1,-0.] {let mut notes=note(&source);notes.objects.get_mut("alpha").unwrap().float_alpha=Some(alpha);let mut native=source.clone();assert!(restore_notes(&mut native,notes,true).is_err());assert_eq!(native,source);}
        let mut notes=note(&source);notes.objects.get_mut("alpha").unwrap().alpha=128;assert!(active_widths(&notes).is_err());
    }
    fn edited_outline(doc:&Document,version:u32,width:Option<f64>,remove:bool,recolor:bool)->Vec<u8> {
        // The test requests millimetres in the source chart, not raw units
        // of an adaptively scaled native pen.
        let units=super::super::path_precision::plan(doc,&doc.objects[0]).unwrap().unwrap_or(1);
        let width=width.map(|v|v*f64::from(units));
        let bytes=if version==14{crate::cdr::encode_document(doc)}else{crate::cdr::encode_document21(doc)}.unwrap().0;
        let mut native=read_native(&bytes).unwrap();let mut changed=0;
        fn edit(nodes:&mut[Node],width:Option<f64>,remove:bool,recolor:bool,changed:&mut usize){
            for n in nodes {
                if n.tag==*b"outl" {let at=super::super::read::outline_body(&n.data).unwrap();if super::super::read::outline(&n.data).unwrap().0!="none" {
                    if let Some(width)=width{set_coord(&mut n.data,at+6,width).unwrap();}
                    if remove{n.data[at..at+2].copy_from_slice(&1u16.to_le_bytes());}
                    if recolor{set32(&mut n.data,at+72,0xabcdef).unwrap();}*changed+=1;
                }}
                if n.kind()==Some(b"obj ") {
                    let(kind,mut args)=super::super::write::arguments(&super::super::read::own_record(n,b"loda").unwrap().data).unwrap();
                    if let Some((_,style))=args.iter_mut().find(|(key,_)|*key==201) {
                        let mut value=super::super::modern::style_record(style).unwrap();
                        if let Some(width)=width{value["outline"]["width"]=serde_json::json!(coord(width).unwrap().to_string());}
                        if remove{value["outline"]["width"]=serde_json::json!("0");}
                        if recolor{value["outline"]["color"]=serde_json::json!("RGB255,USER,171,205,239,100,00000000-0000-0000-0000-000000000000");}
                        *style=super::super::modern::encoded_style(&value).unwrap();n.find_mut(b"loda").unwrap().data=super::super::write::loda(kind,&args);
                    }
                }
                if let Some(children)=&mut n.children{edit(children,width,remove,recolor,changed);}
            }
        }
        edit(&mut native.nodes,width,remove,recolor,&mut changed);assert!(changed>0);
        if version==21{repack21(&native).unwrap()}else{native.members.insert("content/riffData.cdr".into(),write(&native.nodes).unwrap());zip(&native.members.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>())}
    }
    #[test]
    fn native14_and21_outline_edits_keep_priority_over_the_old_width_note() {
        for path in [false,true] {let mut source=outlined();if path{source.objects[0].geometry=Geometry::Path{d:"M0 0L12 0L12 8Z".into(),fill_rule:Default::default()};}
        for version in [14,21] {
            for(width,remove)in [(Some(0.1236),false),(Some(0.),false),(None,true)] {
                let bytes=edited_outline(&source,version,width,remove,false);let actual=crate::cdr::decode(&bytes,"edited").unwrap().0;let o=actual.object("alpha").unwrap();
                assert_eq!(o.style.stroke_width_mm,if remove{0.}else{width.unwrap()},"native{version}: stale Notes must not undo an explicit width edit");
                if remove||width==Some(0.)&&version==21{assert_eq!(o.style.stroke,"none");}
            }
        }}
    }
    #[test]
    fn native14_and21_recoloring_does_not_resurrect_the_old_outline_color() {
        for path in [false,true] {let mut source=outlined();if path{source.objects[0].geometry=Geometry::Path{d:"M0 0L12 0L12 8Z".into(),fill_rule:Default::default()};}
        for version in [14,21] {let bytes=edited_outline(&source,version,None,false,true);let actual=crate::cdr::decode(&bytes,"edited").unwrap().0;assert_eq!(actual.object("alpha").unwrap().style.stroke,"#abcdef");assert_eq!(actual.object("alpha").unwrap().style.stroke_width_mm,0.1234567);}}
    }
    fn guarded_native(source:&Document,native:&mut Document,version:u32) {
        let raw=if version==14{crate::cdr::encode_document(source)}else{crate::cdr::encode_document21(source)}.unwrap().0;
        let members=read_native(&raw).unwrap().members;let frames=super::super::local_frames::read(&members).unwrap().unwrap();let widths=active_widths(&read(&members).unwrap().unwrap()).unwrap();
        super::super::local_frames::restore_with_widths(native,frames,&widths).unwrap();
    }
    #[test]
    fn native_normalized_coordinate_units_keep_their_active_width_and_physical_outline() {
        let mut source=outlined();source.objects[0].transform=[2.,0.,0.,2.,0.,0.];
        for version in [14,21] {
            let mut native=source.clone();let o=&mut native.objects[0];o.width_mm*=2.;o.height_mm*=2.;o.x_mm=0.;o.y_mm=0.;o.transform=[1.,0.,0.,1.,40.,60.];o.style.stroke_width_mm=0.1235;
            let before=native.world_transform(&native.objects[0]).unwrap();guarded_native(&source,&mut native,version);let o=&native.objects[0];
            assert_eq!(o.style.stroke_width_mm,0.1235);assert_eq!([o.width_mm,o.height_mm],[80.,40.]);assert_eq!(native.world_transform(o).unwrap(),before);
        }
    }
    #[test]
    fn native_affine_and_active_width_restoration_share_one_outline_budget() {
        for path in [false,true] {
        let mut source=outlined();let o=&mut source.objects[0];o.x_mm=0.;o.y_mm=0.;o.width_mm=12.;o.height_mm=8.;o.transform=[4.,0.,0.,4.,0.,0.];o.style.stroke_width_mm=0.123493;
        if path{o.geometry=Geometry::Path{d:"M0 0L12 0L12 8Z".into(),fill_rule:Default::default()};}
        for version in [14,21] {
            let mut native=source.clone();native.objects[0].style.stroke_width_mm=0.1235;native.objects[0].transform[0]+=0.00020/(12.+0.1235*4.);
            let before=native.clone();guarded_native(&source,&mut native,version);assert_eq!(native,before,"native{version}: separate affine/width budgets must not accumulate");
        }}
    }
    fn source()->Document {
        let mut doc=Document::new("paint".into(),210.,297.).unwrap();
        doc.objects.push(serde_json::from_value(serde_json::json!({"id":"alpha","page_id":"page-1","x_mm":20.,"y_mm":30.,"width_mm":40.,"height_mm":20.,"style":{"fill":"#287ac7aa","stroke":"none","stroke_width_mm":0.,"opacity":0.123456789012345},"geometry":{"kind":"rect","radius_mm":0.}})).unwrap());doc
    }
    fn legacy_native(doc:&Document,version:u32)->NativeRecords {
        let mut old=read_native(&crate::cdr::encode_document(doc).unwrap().0).unwrap();attach(&mut old.nodes,doc).unwrap();super::super::resources::attach(&mut old.nodes,doc).unwrap();
        if version==14{return old;}
        let mut native=read_native(&crate::cdr::encode_document21(doc).unwrap().0).unwrap();
        let source=all(&old.nodes).into_iter().find(|n|n.kind()==Some(b"doc ")).unwrap();let target=native.nodes.iter_mut().find(|n|n.kind()==Some(b"doc ")).unwrap();super::super::local_data::transfer21(source,target).unwrap();native
    }
    #[test]
    fn partial_deleted_or_stale_paint_parameters_never_override_native_edits() {
        for version in [14,21] {for mode in 0..9 {
            let doc=source();let mut native=legacy_native(&doc,version);let mut changed=false;
            fn edit(nodes:&mut [Node],mode:usize,changed:&mut bool) {
                for n in nodes {
                    if !*changed&&n.kind()==Some(b"udte")&&(mode<7||mode==8) {
                        let rows=n.children.as_mut().unwrap();
                        let values=rows.iter().filter(|r|r.tag==*b"udta").count();if values!=4{continue;}
                        if mode==0{rows.remove(0);}else if mode==1{let row=rows[0].clone();rows.push(row);}else if mode==2{let owner=rows.iter_mut().find(|r|r.tag==*b"udid").unwrap();set32(&mut owner.data,0,99999).unwrap();}
                        else {let target=rows.iter_mut().find(|r|r.tag==*b"udta"&&super::super::local_data::string(&r.data).is_ok_and(|s|if mode==3{s.starts_with("u8:")}else if mode==4{s.starts_with("f64:")}else if mode==5||mode==8{s=="fill"}else{s=="u32:1"})).unwrap();
                            let text=match mode{3=>"u8:171",4=>"f64:NaN",5=>"outline",8=>"both",_=>"u32:2"};target.data=super::super::local_data::item(u32_at(&target.data,0).unwrap(),text).data;}
                        *changed=true;
                    }
                    if !*changed&&n.kind()==Some(b"obj ")&&mode==7 {
                        let load=n.find_mut(b"loda").unwrap();let (kind,mut args)=super::super::write::arguments(&load.data).unwrap();
                        if mode==7{args.iter_mut().find(|(k,_)|*k==8000).unwrap().1[69..77].copy_from_slice(&50f64.to_le_bytes());}
                        else{let key=args.iter().find(|(k,_)|*k==20).unwrap().1.clone();args.iter_mut().find(|(k,_)|*k==10).unwrap().1=key;}
                        load.data=super::super::write::loda(kind,&args);*changed=true;
                    }
                    if let Some(c)=&mut n.children{edit(c,mode,changed);}
                }
            }
            edit(&mut native.nodes,mode,&mut changed);assert!(changed);
            let raw=if version==14{write(&native.nodes).unwrap()}else{repack21(&native).unwrap()};
            let result=std::panic::catch_unwind(||crate::cdr::decode(&raw,"bad"));assert!(result.is_ok());assert!(result.unwrap().is_err(),"version{version} mode{mode}");
        }}
    }
    #[test]
    fn copied_base_values_are_removed_and_image_and_paint_fields_share_one_owner() {
        let mut doc=source();let bytes=std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/assets/images/quadrants.png")).unwrap();doc.resources.insert("source".into(),crate::Resource::png(&bytes).unwrap());
        let o=&mut doc.objects[0];o.style.fill="none".into();o.geometry=Geometry::Image{resource_id:"source".into(),fit:crate::ImageFit::Stretch,clip_mm:None};
        for version in [14,21] {
            let native=legacy_native(&doc,version);let entries=all(&native.nodes).into_iter().filter(|n|n.kind()==Some(b"udte")).collect::<Vec<_>>();assert_eq!(entries.len(),1);
            assert_eq!(entries[0].children.as_ref().unwrap().last().unwrap().tag,*b"udid");
            let raw=if version==14{write(&native.nodes).unwrap()}else{repack21(&native).unwrap()};
            let restored=crate::cdr::decode(&raw,&doc.id).unwrap().0;assert_eq!(restored.resources,doc.resources);assert_eq!(restored.objects[0].style.opacity,doc.objects[0].style.opacity);
        }
        let empty=Document::new("empty".into(),210.,297.).unwrap();let raw=crate::cdr::encode_document(&empty).unwrap().0;assert!(all(&super::super::binary::read(&raw).unwrap()).into_iter().all(|n|n.kind()!=Some(b"udte")));
    }
    #[test]
    fn paint_notes_survive_missing_object_data_and_keep_native_edits() {
        let doc=source();let mut native=doc.clone();lower(&mut native.objects[0]).unwrap();native.objects[0].style.opacity=native_value(parameters(&doc.objects[0]).unwrap());native.objects[0].style.stroke_miterlimit=5.;
        let params:PaintNotes=super::super::metadata::decode(NOTES,&notes(&doc).unwrap().unwrap()).unwrap();assert_eq!(restore_notes(&mut native,params,true).unwrap(),(1,0));assert_eq!(native.objects[0].style,doc.objects[0].style);
        native.objects[0].style.fill="#334455".into();native.objects[0].style.opacity=0.5;let expected=native.clone();let params=super::super::metadata::decode(NOTES,&notes(&doc).unwrap().unwrap()).unwrap();assert_eq!(restore_notes(&mut native,params,true).unwrap(),(0,1));assert_eq!(native,expected);
    }
}
#[cfg(test)]
mod text_width_tests{
 use super::*;
 #[test]
 fn fine_text_outline_and_frame_share_one_budget_and_native_width_edits_win(){
  let source:Document=serde_json::from_value(serde_json::json!({"id":"text-width","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"text","page_id":"page-1","x_mm":0.,"y_mm":0.,"width_mm":20.,"height_mm":10.,"transform":[4.,0.,0.,4.,0.,0.],"geometry":{"kind":"text","content":"AV","font_family":"Arial","font_size_mm":5.},"style":{"fill":"none","stroke":"#123456","stroke_width_mm":0.123493,"opacity":1.}}]})).unwrap();
  for empty in [false,true]{let mut source=source.clone();if empty{if let Geometry::Text{content,..}=&mut source.objects[0].geometry{content.clear();}}
  for version in [14,21]{let raw=if version==14{crate::cdr::encode_document(&source)}else{crate::cdr::encode_document21(&source)}.unwrap().0;let members=read_native(&raw).unwrap().members;
   for (native_width,shift,restored) in [(0.1235,0.,true),(0.1235,0.0002,false),(0.1235,0.01,false),(0.1236,0.,false)]{
    let mut doc=source.clone();doc.objects[0].style.stroke_width_mm=native_width;doc.objects[0].transform[4]=shift;
    let frames=super::super::local_frames::read(&members).unwrap().unwrap();let widths=active_widths(&read(&members).unwrap().unwrap()).unwrap();super::super::local_frames::restore_with_widths(&mut doc,frames,&widths).unwrap();
    assert_eq!(doc.objects[0].style.stroke_width_mm,if restored{0.123493}else{native_width},"version{version}/{shift}");assert_eq!(doc.objects[0].transform[4],shift);
   }
  }}
 }
}
