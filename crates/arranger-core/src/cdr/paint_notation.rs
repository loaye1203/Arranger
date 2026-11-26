//! Hex spelling only: length and letter-case bits, never RGB/alpha channels.
//! All active colors continue to come from native paints and effect records.
use super::{invalid,unsupported,Result};
use crate::{Document,Geometry,paint::Paint};
use serde::{Deserialize,Serialize};
use std::collections::BTreeMap;
const PREFIX:&str="ArrangerPaintNotationV1|sha256:";
#[derive(Deserialize,Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Notations{version:u32,objects:BTreeMap<String,PaintNotation>}
#[derive(Deserialize,Serialize)]
#[serde(deny_unknown_fields)]
struct PaintNotation{#[serde(skip_serializing_if="Option::is_none")]fill:Option<Hex>,#[serde(skip_serializing_if="Option::is_none")]stroke:Option<Hex>}
#[derive(Deserialize,Serialize)]
#[serde(deny_unknown_fields)]
struct Hex{digits:u8,uppercase:u8,#[serde(default,skip_serializing_if="std::ops::Not::not")]decimal_alpha:bool}
fn notation(color:&str)->Option<Hex>{
    if color.starts_with("rgba("){return Some(Hex{digits:0,uppercase:0,decimal_alpha:true});}
    let raw=color.strip_prefix('#')?;
    let uppercase=raw.bytes().enumerate().fold(0,|mask,(i,b)|mask|if b.is_ascii_uppercase(){1<<i}else{0});
    (raw.len()==8||uppercase!=0).then_some(Hex{digits:raw.len() as u8,uppercase,decimal_alpha:false})
}
pub(super) fn notes(doc:&Document)->Result<Option<String>>{
    let mut objects=BTreeMap::new();
    for o in &doc.objects {
        // Inert group styles already have their own bounded scalar section.
        if matches!(o.geometry,Geometry::Group){continue;}
        let(fill,stroke)=(notation(&o.style.fill),notation(&o.style.stroke));
        if fill.is_some()||stroke.is_some(){objects.insert(o.id.clone(),PaintNotation{fill,stroke});}
    }
    if objects.is_empty(){return Ok(None);}super::metadata::encode(PREFIX,&Notations{version:1,objects}).map(Some)
}
pub(super) fn read(members:&BTreeMap<String,Vec<u8>>)->Result<Option<Notations>>{
    super::metadata::find(members,PREFIX)?.map(|raw|super::metadata::decode(PREFIX,&raw)).transpose()
}
fn validate(hex:&Hex)->Result<()> {
    if hex.decimal_alpha {return if hex.digits==0&&hex.uppercase==0{Ok(())}else{Err(invalid("invalid decimal paint notation"))};}
    if ![6,8].contains(&hex.digits)||u16::from(hex.uppercase)>=(1u16<<hex.digits){return Err(invalid("invalid native hex paint notation"));}Ok(())
}
fn apply(value:&mut String,hex:&Hex)->Result<()> {
    if hex.decimal_alpha {let paint=crate::paint::parse(value)?;if matches!(paint,Paint::Rgba(_)|Paint::RgbaFloat{..}){let[r,g,b,_]=paint.rgba();*value=format!("rgba({r},{g},{b},{})",paint.alpha());}return Ok(());}
    let Paint::Rgba([r,g,b,a])=crate::paint::parse(value)?else{return Ok(())};
    let mut raw=format!("{r:02x}{g:02x}{b:02x}");if hex.digits==8||a!=255{raw.push_str(&format!("{a:02x}"));}
    *value=format!("#{}",raw.chars().enumerate().map(|(i,c)|if hex.uppercase&(1<<i)!=0{c.to_ascii_uppercase()}else{c}).collect::<String>());Ok(())
}
pub(super) fn restore(doc:&mut Document,notes:Notations)->Result<usize>{
    if notes.version!=1{return Err(unsupported("unknown paint notation version"));}
    let mut restored=0;
    for(id,p)in notes.objects {
        if !crate::model::valid_id(&id){return Err(invalid("invalid native paint notation target"));}
        for hex in [&p.fill,&p.stroke].into_iter().flatten(){validate(hex)?;}
        let Some(object)=doc.objects.iter_mut().find(|o|o.id==id)else{continue};
        if matches!(object.geometry,Geometry::Group){continue;}
        if let Some(hex)=p.fill{apply(&mut object.style.fill,&hex)?;}
        if let Some(hex)=p.stroke{apply(&mut object.style.stroke,&hex)?;}
        restored+=1;
    }Ok(restored)
}

#[cfg(test)]
mod tests{
    use super::*;
    use serde_json::json;
    fn document()->Document{serde_json::from_value(json!({"id":"paint-notation","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"shape","page_id":"page-1","x_mm":20.,"y_mm":30.,"width_mm":40.,"height_mm":20.,"geometry":{"kind":"rect","radius_mm":0.},"style":{"fill":"#Aa12fF","stroke":"#AbCdEfFF","stroke_width_mm":0.25,"opacity":1.}}]})).unwrap()}
    fn parameters(doc:&Document)->Notations{super::super::metadata::decode(PREFIX,&notes(doc).unwrap().unwrap()).unwrap()}
    #[test]
    fn parameters_hold_only_spelling_bits_and_never_source_color_channels(){
        let source=document();let value=serde_json::to_value(parameters(&source)).unwrap();assert_eq!(value,json!({"version":1,"objects":{"shape":{"fill":{"digits":6,"uppercase":33},"stroke":{"digits":8,"uppercase":213}}}}));
        assert!(!value.to_string().contains("Aa12fF"));assert!(!value.to_string().contains("AbCdEfFF"));
    }
    #[test]
    fn actual_native_channel_kind_and_alpha_edits_remain_authoritative(){
        let source=document();let mut actual=source.clone();actual.objects[0].style.fill="#efc123".into();actual.objects[0].style.stroke="cmyk(1,2,3,4)".into();restore(&mut actual,parameters(&source)).unwrap();assert_eq!(actual.objects[0].style.fill,"#Efc123");assert_eq!(actual.objects[0].style.stroke,"cmyk(1,2,3,4)");
        let mut actual=source.clone();actual.objects[0].style.fill="none".into();actual.objects[0].style.stroke="#12345680".into();restore(&mut actual,parameters(&source)).unwrap();assert_eq!(actual.objects[0].style.fill,"none");assert_eq!(actual.objects[0].style.stroke,"#12345680");
        let mut actual=source.clone();actual.objects[0].style.fill="#efc12380".into();restore(&mut actual,parameters(&source)).unwrap();assert_eq!(actual.objects[0].style.fill,"#Efc12380","six-digit source cannot erase an actual native alpha");
        let mut absent=source.clone();absent.objects.clear();assert_eq!(restore(&mut absent,parameters(&source)).unwrap(),0);assert!(absent.objects.is_empty());
    }
    #[test]
    fn unknown_or_corrupted_notation_never_changes_native_color_channels(){
        for (digits,uppercase) in [(5,0),(7,0),(6,64),(9,0)]{
            let source=document();let mut actual=source.clone();let mut p=parameters(&source);p.objects.get_mut("shape").unwrap().fill=Some(Hex{digits,uppercase,decimal_alpha:false});assert_eq!(restore(&mut actual,p).unwrap_err().code,"INVALID_CDR");assert_eq!(actual,source);
        }
        let source=document();let mut actual=source.clone();let mut p=parameters(&source);p.version=2;assert_eq!(restore(&mut actual,p).unwrap_err().code,"CDR_UNSUPPORTED");assert_eq!(actual,source);
    }
}
