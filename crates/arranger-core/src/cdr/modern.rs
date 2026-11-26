//! Genuine version21 object styles, floating rectangle geometry and text-story
//! records. JSON here is Corel's native style grammar, never an Arranger model.
use super::{binary::*,invalid,unsupported,Result};
use crate::{Style,LineCap,LineJoin,TextAlign,TextLayoutOptions,TextMode};
use serde_json::{Value,json};

fn object(value:&Value)->Result<&serde_json::Map<String,Value>> {value.as_object().ok_or_else(||invalid("native21 style must be a JSON object"))}
fn s<'a>(value:&'a Value,key:&str,default:&'a str)->Result<&'a str> {match value.get(key){None=>Ok(default),Some(v)=>v.as_str().ok_or_else(||invalid(format!("native21 {key} is not a string")))}}
fn number(value:&Value,key:&str,default:f64)->Result<f64> {let Some(v)=value.get(key) else{return Ok(default)};let v=v.as_str().ok_or_else(||invalid("native21 style number is not a string"))?.parse::<f64>().map_err(|_|invalid("invalid native21 style number"))?;if !v.is_finite(){return Err(invalid("non-finite native21 style number"));}Ok(v)}
fn neutral(value:&Value,constants:&[(&str,&str)],dynamic:&[&str])->Result<()> {
    for (key,v) in object(value)? {if dynamic.contains(&key.as_str()){continue;}let Some((_,expected))=constants.iter().find(|(name,_)|*name==key) else{return Err(unsupported(format!("native21 style property {key} is not represented")))};
        if v.as_str()!=Some(expected){return Err(unsupported(format!("native21 nondefault {key} is not represented")));}
    }Ok(())
}
pub(super) fn merge(a:&Value,b:&Value)->Value {
    if let(Some(a),Some(b))=(a.as_object(),b.as_object()) {let mut out=a.clone();for(k,v)in b {out.insert(k.clone(),out.get(k).map_or_else(||v.clone(),|a|merge(a,v)));}Value::Object(out)}else{b.clone()}
}
pub(super) fn style_record(data:&[u8])->Result<Value> {
    if u32_at(data,0)?!=1 || u32_at(data,4)? as usize+8!=data.len(){return Err(invalid("native21 object-style envelope differs"));}
    let value:Value=serde_json::from_slice(&data[8..]).map_err(|e|invalid(format!("native21 object JSON: {e}")))?;object(&value)?;Ok(value)
}
pub(super) fn encoded_style(value:&Value)->Result<Vec<u8>> {let raw=serde_json::to_vec(value).map_err(|e|invalid(e.to_string()))?;let mut out=1u32.to_le_bytes().to_vec();out.extend((raw.len() as u32).to_le_bytes());out.extend(raw);Ok(out)}
fn color(text:&str)->Result<String> {
    let parts=text.split(',').collect::<Vec<_>>();let (n,model)=match parts.first().copied(){Some("RGB255")=>(3,0),Some("CMYK")=>(4,1),Some("CMYK255")=>(4,2),_=>return Err(unsupported("native21 color space requires a preservation codec"))};
    if parts.len()!=n+4 || parts[1]!="USER" || parts[n+2]!="100" || parts[n+3]!="00000000-0000-0000-0000-000000000000" {return Err(unsupported("native21 alpha, named or managed colors remain unsupported"));}
    let values=parts[2..2+n].iter().map(|s|s.parse::<f64>().map_err(|_|invalid("native21 invalid color channel"))).collect::<Result<Vec<_>>>()?;
    let max=if model==1 {100.}else{255.};if values.iter().any(|x|!x.is_finite()||*x<0.||*x>max){return Err(invalid("native21 color channel outside range"));}
    if model==0 {if values.iter().any(|x|x.fract()!=0.){return Err(unsupported("fractional native21 RGB is not represented"));}return Ok(format!("#{:02x}{:02x}{:02x}",values[0] as u8,values[1] as u8,values[2] as u8));}
    if model==2 {if values.iter().any(|x|x.fract()!=0.){return Err(unsupported("fractional native21 CMYK255 is outside the channel grid"));}return Ok(format!("cmyk255({},{},{},{})",values[0] as u8,values[1] as u8,values[2] as u8,values[3] as u8));}
    let integers=values.iter().all(|x|x.fract()==0.);let values=if integers {values}else{let v=values.iter().map(|x|x*2.55).collect::<Vec<_>>();if v.iter().any(|x|(x-x.round()).abs()>1e-6){return Err(unsupported("fractional native21 CMYK is outside this model's channel grids"));}v};
    Ok(format!("{}({},{},{},{})",if integers {"cmyk"}else{"cmyk255"},values[0].round() as u8,values[1].round() as u8,values[2].round() as u8,values[3].round() as u8))
}
fn retain_channels(prior:&str,candidate:String)->Result<String> {
    use crate::paint::Paint;
    let same=match (crate::paint::parse(prior)?,crate::paint::parse(&candidate)?) {
        (Paint::Cmyk{channels:a,scale:sa},Paint::Cmyk{channels:b,scale:sb})=>a.into_iter().zip(b).all(|(a,b)|u32::from(a)*u32::from(sb)==u32::from(b)*u32::from(sa)),
        (a,b)=>a==b,
    };Ok(if same {prior.into()}else{candidate})
}
fn fill(value:&Value,prior:&str)->Result<String> {
    neutral(value,&[("overprint","0"),("screenSpec","0,0,45000000,60,0")],&["type","primaryColor","secondaryColor","winding"])?;
    match s(value,"type",if prior=="none" {"0"}else{"1"})? {"0"=>Ok("none".into()),"1"=>value.get("primaryColor").map_or_else(||Ok(prior.into()),|v|retain_channels(prior,color(v.as_str().ok_or_else(||invalid("native21 primary color is not a string"))?)?)),_=>Err(unsupported("native21 nonuniform fills are not represented"))}
}
fn outline(value:&Value,style:&mut Style)->Result<()> {
    // Real Corel2019 SaveAs21 adds type=0 to ordinary solid outlines,
    // including live fitted text. Other outline types remain unsupported.
    neutral(value,&[("type","0"),("angle","0"),("screenSpec","0,0,45000000,60,0"),("overprint","0"),("justification","0"),("leftArrow","|0"),("matrix","1,0,0,0,1,0"),("rightArrow","|0"),("shareArrow","0"),("dashAdjust","0"),("aspect","100"),("behindFill","0"),("dashDotSpec","0"),("rightArrowAttributes","0|0|0|0|0|0|0"),("dotLength","0"),("leftArrowAttributes","0|0|0|0|0|0|0"),("overlapArrow","0")],&["color","width","miterLimit","endCaps","joinType","scaleWithObject"])?;
    if !["0","1"].contains(&s(value,"scaleWithObject","0")?){return Err(unsupported("unknown native21 outline scaling mode"));}
    let width=number(value,"width",style.stroke_width_mm*10000.)?/10000.;if width<0. {return Err(invalid("negative native21 outline"));}
    if width==0. {style.stroke="none".into();}else if let Some(v)=value.get("color"){style.stroke=retain_channels(&style.stroke,color(v.as_str().ok_or_else(||invalid("native21 outline color is not a string"))?)?)?;}
    style.stroke_width_mm=width;style.stroke_miterlimit=number(value,"miterLimit",style.stroke_miterlimit)?;
    style.stroke_linecap=match s(value,"endCaps",match style.stroke_linecap {LineCap::Butt=>"0",LineCap::Round=>"1",LineCap::Square=>"2"})? {"0"=>LineCap::Butt,"1"=>LineCap::Round,"2"=>LineCap::Square,_=>return Err(unsupported("native21 outline caps unsupported"))};
    style.stroke_linejoin=match s(value,"joinType",match style.stroke_linejoin {LineJoin::Miter=>"0",LineJoin::Round=>"1",LineJoin::Bevel=>"2"})? {"0"=>LineJoin::Miter,"1"=>LineJoin::Round,"2"=>LineJoin::Bevel,_=>return Err(unsupported("native21 outline join unsupported"))};Ok(())
}
pub(super) fn geometry_style(value:&Value,mut prior:Style)->Result<Style> {
    for key in object(value)?.keys(){if !["fill","outline","character","frame","paragraph","transparency"].contains(&key.as_str()){return Err(unsupported(format!("native21 object property {key} unsupported")));}}
    super::opacity::modern(value,&mut prior)?;
    if let Some(v)=value.get("fill"){prior.fill=fill(v,&prior.fill)?;}if let Some(v)=value.get("outline"){outline(v,&mut prior)?;}Ok(prior)
}
pub(super) fn rectangle(data:&[u8])->Result<(f64,f64,f64)> {
    if data.len()!=128{return Err(unsupported("native21 rectangle record layout unsupported"));}
    let sx=f64_at(data,16)?;let sy=f64_at(data,24)?;
    if !sx.is_finite()||!sy.is_finite()||sx==0.||sy==0.{return Err(unsupported("native21 invalid corner scaling unsupported"));}
    let mut radius=None;let relative=u32_at(data,32)?==0;
    for p in [32,56,80,104] {let kind=u32_at(data,p)?;let flags=u32_at(data,p+4)?;let r=f64_at(data,p+8)?;
        // CDR1500+: byte16 is the corner type; the following seven bytes
        // are padding, not a second floating radius. Corel2019's converted
        // 14-source diagnostic rectangles retain nonzero padding here.
        // See libcdr CDRParser::readRectangle (corner_type/readU8 + seek).
        if !(if relative{kind==0&&flags==0&&(0.0..=1.0).contains(&r)}else{kind==1&&[477,u32::MAX].contains(&flags)&&r>=0.})||data[p+16]!=0 {return Err(unsupported("native21 scallop/chamfer/independent corner geometry unsupported"));}
        if radius.is_some_and(|v|v!=r){return Err(unsupported("independent native21 corner radii unsupported"));}radius=Some(r);
    }
    let radius=radius.unwrap();if radius!=0.&&(sx<=0.||sy<=0.||sx!=sy){return Err(unsupported("native21 unequal nonzero corner scales require elliptical-corner preservation"));}
    let(w,h)=(f64_at(data,0)?,f64_at(data,8)?);
    // Report106's SDK-created .75mm rectangle stores .15 in mode0:
    // the fraction of the smaller scaled half-dimension. Mode1 retains the
    // previous absolute-coordinate profile; never infer one from magnitude.
    let radius=if radius==0.{0.}else if relative{
        // Nonzero unequal sx/sy was rejected above. Cancel the common scale
        // algebraically, rather than introducing multiply/divide drift.
        let half=w.abs().min(h.abs())/2.;let value=radius*half;let grid=value.round_ties_even();
        // Exact inverse of our integer-coordinate projection, not a tolerance:
        // a neighbouring fractional edit must remain a different radius.
        if half>0.&&radius==grid/half{grid}else{value}
    }else{radius/sx};
    Ok((w/10000.,h/10000.,radius/10000.))
}
pub(super) fn encoded_rectangle(width:f64,height:f64,radius:f64)->Result<Vec<u8>> {
    let mut out=vec![];for n in [coord(width)? as f64,coord(height)? as f64,1.,1.]{out.extend(n.to_le_bytes());}
    let half=(coord(width)? as f64).abs().min((coord(height)? as f64).abs())/2.;
    if radius>0.&&half<=0.{return Err(unsupported("native21 rounded rectangle has no positive half-dimension"));}
    let fraction=if radius==0.{0.}else{coord(radius)? as f64/half};
    if !(0.0..=1.0).contains(&fraction){return Err(unsupported("native21 rounded rectangle fraction outside0..1"));}
    for _ in 0..4 {out.extend([0;8]);out.extend(fraction.to_le_bytes());out.extend(0f64.to_le_bytes());}Ok(out)
}
pub(super) fn encoded_rectangle_in_frame(width:f64,height:f64,radius:f64,m:[f64;6])->Result<Vec<u8>> {
    let mut data=encoded_rectangle(width,height,radius)?;let[a,c,_,b,d,_]=m;let determinant=a*d-b*c;let sx=a.hypot(b);
    if determinant<=0.||!sx.is_finite()||sx<=0.{
        // Native122 reflected-shear rectangles retain signed zero-corner
        // scales. These are inert for zero corners, but resetting both to1
        // discards the actual native frame chart. Rounded reflection remains
        // in its existing normalized profile.
        if radius==0.&&determinant<0.&&sx.is_finite()&&sx>0.&&sx!=c.hypot(d){
            let signed_sx=sx*if a<0.||a==0.&&b<0.{-1.}else{1.};let sy=determinant/signed_sx;
            if !sy.is_finite()||sy==0.{return Err(invalid("native21 reflected rectangle frame scales are not finite nonzero values"));}
            data[16..24].copy_from_slice(&signed_sx.to_le_bytes());data[24..32].copy_from_slice(&sy.to_le_bytes());
        }
        return Ok(data);
    }
    let sy=determinant/sx;if !sy.is_finite()||sy<=0.{return Err(invalid("native21 rectangle frame scales are not finite positive values"));}
    // Zero corners permit independent axis scales, as in actual Corel21
    // Stretch(2,1). Uniform circular corners also have a measured2.5x profile.
    // Other rounded affine frames keep their existing experimental profile;
    // no unmeasured elliptical-corner interpretation is silently introduced.
    let(sx,sy)=if radius==0.{(sx,sy)}else if a==d&&b== -c{(sx,sx)}else{return Ok(data);};
    data[16..24].copy_from_slice(&sx.to_le_bytes());data[24..32].copy_from_slice(&sy.to_le_bytes());
    // Relative fractions already scale through the dimensions and sx/sy.
    Ok(data)
}
#[derive(Clone,Debug,PartialEq)]
struct Format {font:String,size:f64,style:Style,align:TextAlign,leading:Option<f64>,tracking:i32}
pub(super) struct Text {pub content:String,pub font:String,pub size:f64,pub style:Style,pub layout:TextLayoutOptions,pub tracking:i32,pub matrix:[f64;6],pub frame:Option<[i32;4]>,pub column_width:Option<i32>,pub path:Option<super::path_text::Options>}
fn format(value:&Value,language:&str,area:bool,base_style:&Style)->Result<Format> {
    let c=value.get("character").ok_or_else(||unsupported("native21 character defaults missing"))?;
    // Retained14-origin Corel21 stories explicitly serialize the neutral
    // liga=0 field. No nonzero or additional OpenType override is admitted.
    neutral(c,&[("angle","0"),("v_shift","0"),("h_shift","0"),("liga","0")],&["fill","outline","latin","farEast","RTL"])?;
    let slot=match language {"ENU"=>"latin","CHC"=>"farEast",_=>return Err(unsupported("native21 character script requires a richer importer"))};let face=c.get(slot).ok_or_else(||unsupported("native21 script font missing"))?;
    let family_name=s(face,"font","")?;let charset=s(face,"charset","")?;
    let scalar_legacy=matches!(family_name,"SimSun-ExtB"|"SimSun-ExtG");
    let legacy=s(face,"technology","4")?=="2"&&(charset=="134"||charset=="0"&&scalar_legacy)&&super::fonts::verified_legacy(family_name)?;
    neutral(face,&[("technology",if legacy {"2"}else{"4"}),("weight","64"),("italic","0"),("effect","0"),("overline","0"),("script","0"),("strikeout","0"),("underline","0")],&["font","size","charset","version"])?;
    let family=s(face,"font","")?.to_owned();let size=number(face,"size",0.)?/10000.;if family.is_empty()||size<=0. {return Err(invalid("native21 font name/size missing"));}
    let mut style=geometry_style(value,base_style.clone())?;if let Some(v)=c.get("fill"){style.fill=fill(v,&style.fill)?;}if let Some(v)=c.get("outline"){outline(v,&mut style)?;}
    if let Some(frame)=value.get("frame") {
        neutral(frame,&[("rtlColumns","0"),("equalWidthColumns","1"),("verticalText","0"),("v_justify","0"),("margin","0"),("hotelText","0")],&["columns"])?;
        if let Some(columns)=frame.get("columns") {let parts=columns.as_str().ok_or_else(||invalid("native21 columns not a string"))?.split(',').collect::<Vec<_>>();if parts.len()!=2||parts[1]!="0" {return Err(unsupported("native21 multiple columns or column gaps unsupported"));}}
    }
    let p=value.get("paragraph").ok_or_else(||unsupported("native21 paragraph defaults missing"))?;
    // These properties are inactive only while bullets, decorations, tabs and
    // hyphenation are disabled by the character/content/paragraph checks. An
    // unknown paragraph effect must not disappear during import.
    const PARAGRAPH_KEYS:&[&str]=&["RTL","interLineSpacing","beforeParaSpacing","afterParaSpacing","maxInterCharSpacing","minInterWordSpacing","maxInterWordSpacing","spacingMethod","interCharSpacing","interLangSpacing","interWordSpacing","justify","firstLineIndent","restOfLinesIndent","leftMargin","hangingIndent","spacingFromText","bulletOn","dropCapOn","autoHyphenate","bulletOutline","bulletHShift","bulletVShift","bulletHanging","bulletAngle","bulletFont","bulletFirstCharIndent","bulletFill","bulletChar","hyphenateMinCharsBefore","hyphenateMinCharsAfter","hyphenateHotZone","tabs","tabLeaderChar","tabLeaderSpacing","breakCapitalizedWords","breakAllCapWords","hyphenateMinWordLength","linesToDrop","dblStrikeout1Absolute","thinStrikeoutThickness","dblOverline2Distance","thickStrikeoutAbsolute","dblOverline1Thickness","thinOverlineThickness","thickDistance","dblStrikeout1Distance","dblUnderline2Thickness","thickAbsolute","dblUnderline2Distance","dblOverline1Distance","thinStrikeoutDistance","dblStrikeout2Distance","dblUnderline1Distance","thinOverlineDistance","dblStrikeout2Thickness","dblUnderline1Absolute","thickStrikeoutThickness","thickOverlineDistance","thickOverlineThickness","thickOverlineAbsolute","thinStrikeoutAbsolute","thinAbsolute","thinThickness","thickStrikeoutDistance","thinOverlineAbsolute","dblOverline1Absolute","thickThickness","dblUnderline2Absolute","dblUnderline1Thickness","thinDistance","dblOverline2Absolute","dblStrikeout1Thickness","dblOverline2Thickness","dblStrikeout2Absolute"];
    for key in object(p)?.keys(){if !PARAGRAPH_KEYS.contains(&key.as_str()){return Err(unsupported(format!("native21 paragraph property {key} is not represented")));}}
    for (key,wanted) in [("RTL","0"),("bulletOn","0"),("dropCapOn","0"),("autoHyphenate","0"),("firstLineIndent","0"),("restOfLinesIndent","0"),("leftMargin","0"),("hangingIndent","0"),("spacingFromText","0"),("afterParaSpacing","0")] {if s(p,key,wanted)?!=wanted {return Err(unsupported(format!("native21 paragraph {key} not represented")));}}
    let align=match s(p,"justify",if area {"1"}else{"0"})? {"0"|"1"=>TextAlign::Left,"2"=>TextAlign::Center,"3"=>TextAlign::Right,_=>return Err(unsupported("native21 justified/point alignment unsupported"))};
    let leading=match s(p,"spacingMethod","0")? {"1" if number(p,"beforeParaSpacing",0.)?==0.=>Some(number(p,"interLineSpacing",0.)?/10000.),"0" if !area&&number(p,"beforeParaSpacing",1_000_000.)?==1_000_000.&&number(p,"interLineSpacing",1_000_000.)?==1_000_000.=>None,_=>return Err(unsupported("native21 relative leading or paragraph spacing unsupported"))};
    if leading.is_some_and(|v|v<=0.) {return Err(invalid("native21 nonpositive baseline spacing"));}let tracking=number(p,"interCharSpacing",0.)?;if tracking.fract()!=0.||!(-1_000_000.0..=20_000_000.).contains(&tracking){return Err(unsupported("native21 tracking outside range"));}
    if !super::typography::represented_word_tracking(tracking as i32,number(p,"interWordSpacing",1_000_000.)?){return Err(unsupported("native21 independent word spacing is not represented"));}
    if !super::typography::represented_language_tracking(tracking as i32,number(p,"interLangSpacing",0.)?){return Err(unsupported("native21 independent inter-language spacing is not represented"));}
    Ok(Format{font:family,size,style,align,leading,tracking:tracking as i32})
}
struct Cursor<'a>{data:&'a [u8],at:usize}
impl<'a> Cursor<'a> {
    fn take(&mut self,n:usize)->Result<&'a [u8]>{let v=bytes(self.data,self.at,n)?;self.at+=n;Ok(v)}
    fn u32(&mut self)->Result<u32>{Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))}
    fn u16(&mut self)->Result<u16>{Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))}
    fn json(&mut self)->Result<Value>{let n=self.u32()? as usize;if n>16*1024*1024{return Err(invalid("native21 JSON exceeds range"));}let v=serde_json::from_slice(self.take(n)?).map_err(|e|invalid(format!("native21 text JSON: {e}")))?;object(&v)?;Ok(v)}
}
pub(super) fn text(data:&[u8],area:bool,base:&Value,base_style:&Style,postscript:&std::collections::BTreeSet<String>,declared:&std::collections::BTreeSet<String>)->Result<Text> {
    let mut c=Cursor{data,at:0};let header=c.take(36)?;let story_version=c.u32()?;
    if (u32_at(header,0)?==0)!=area||u32_at(header,20)?!=13||!matches!(story_version,1500|1700)||c.take(1)?!=[0]||c.u32()?!=1 {return Err(unsupported("native21 story/frame version unsupported"));}
    c.take(4)?;let mut matrix=[0.;6];for n in &mut matrix {*n=f64::from_le_bytes(c.take(8)?.try_into().unwrap());if !n.is_finite(){return Err(invalid("non-finite native21 text matrix"));}}
    let on_path=c.u32()?;let path=match on_path {0=>None,1 if !area=>Some(super::path_text::Options::parse(c.take(40)?)?),_=>return Err(unsupported("native21 path/linked story flags unsupported"))};if c.u32()?!=0||c.u32()?!=0 {return Err(unsupported("native21 linked story flags unsupported"));}
    // Windows148's eight actual14-origin diagnostic documents are genuine
    // CDR21 containers with1500 in the story header, but the complete modern
    // UTF8/64-bit story payload. Do not switch JSON encodings from that scalar
    // or admit unmeasured area/plain-text1500 profiles. All remaining bounds,
    // effects, style, relation, cache and complete-trailer checks still apply.
    if story_version==1500&&(area||path.is_none()){return Err(unsupported("native21 retained1500 story is measured only for artistic fitted text"));}
    let mut frame_style=json!({});
    let frame=if area {let raw=c.take(16)?;let frame=[i32_at(raw,0)?,i32_at(raw,4)?,i32_at(raw,8)?,i32_at(raw,12)?];let v=c.json()?;
        // Actual Corel2019 text edits serialize the same single column and
        // explicit zero margins/vertical justification in this frame layer.
        // Merge it into resolved paragraph styles so the existing column,
        // uniformity and geometry checks remain authoritative.
        neutral(&v,&[],&["frame"])?;let f=v.get("frame").ok_or_else(||unsupported("native21 frame defaults missing"))?;
        neutral(f,&[("rtlColumns","0"),("equalWidthColumns","1"),("v_justify","0"),("margin","0")],&["columns"])?;
        frame_style=v;Some(frame)}else{None};
    let count=c.u32()? as usize;if count==0||count>65535{return Err(invalid("native21 paragraph count outside range"));}if path.is_some()&&count!=1{return Err(unsupported("native21 multiline path text unsupported"));}let mut parts=vec![];let mut uniform=None;let mut latin_override=false;let mut column_width=None;
    for _ in 0..count {
        c.take(4)?;if c.take(if area {1}else{2})?.iter().any(|x|*x!=0){return Err(unsupported("native21 additional paragraph flags"));}let parent=merge(&merge(base,&frame_style),&c.json()?);let n=c.u32()? as usize;if n==0||n>65535{return Err(invalid("native21 style count outside range"));}let mut formats=vec![];
        for _ in 0..n {c.take(4)?;let flag=c.u16()?;let language=if flag&4!=0 {let n=c.u32()? as usize;if n>16{return Err(unsupported("native21 language code exceeds range"));}read_wide(c.take(n*2)?)?}else{"ENU".into()};let own=c.json()?;let value=merge(&parent,&own);
            if matches!(base.pointer("/transparency/appliesTo").and_then(Value::as_str),Some("0"|"1"))&&value.pointer("/transparency/appliesTo")!=base.pointer("/transparency/appliesTo"){return Err(unsupported("native21 story and object paint transparency scopes differ"));}
            if let Some(columns)=value.pointer("/frame/columns") {
                let raw=columns.as_str().ok_or_else(||invalid("native21 columns not a string"))?;
                let width=raw.strip_suffix(",0").and_then(|s|s.parse::<i32>().ok()).filter(|v|*v>=0&&(!area||*v>0)).ok_or_else(||unsupported("native21 single-column width is not positive native coordinates"))?;
                if column_width.is_some_and(|previous|previous!=width){return Err(unsupported("native21 paragraph column widths differ"));}column_width=Some(width);
            }
            let resolved=format(&value,&language,area,base_style)?;
            // SDK-verified regular CFF artistic SaveAs21 uses exactlye4 and
            // an empty character override. No other flag combination or
            // undeclared font can use this bounded profile.
            let native_latin_edit=flag==0xf954&&!area&&language=="ENU"&&declared.contains(&resolved.font)&&own.as_object().is_some_and(|o|o.len()==1&&o.get("character").is_some_and(Value::is_object));
            // Actual regular mixed-CJK paragraph SaveAs21 also produced4904
            // in both ENU and CHC runs. This exact area profile still passes
            // every resolved character/paragraph/effect and uniformity check.
            let native_area_regular=flag==0x4904&&area&&matches!(language.as_str(),"ENU"|"CHC")&&declared.contains(&resolved.font)&&own.as_object().is_some_and(|o|o.len()==1&&o.get("character").is_some_and(Value::is_object));
            let scalar_regular=matches!(flag,0xf954|0xf950)&&matches!(resolved.font.as_str(),"SimSun-ExtB"|"SimSun-ExtG")&&matches!(language.as_str(),"ENU"|"CHC")&&declared.contains(&resolved.font)&&own.as_object().is_some_and(|o|o.len()==1&&o.get("character").is_some_and(Value::is_object));
            // Windows93 supplied complete native stories with these two
            // additional regular point-text profiles. Their flag values are
            // measured records, not a mask permitting unknown combinations.
            // Resolved effects and cross-run formatting are still checked;
            // accepting the text does not establish supplementary glyphs.
            let native_unicode_regular=!area&&((flag==0xfd44&&language=="CHC"&&resolved.font=="SimSun-ExtG")||(flag==0x04f4&&language=="ENU"&&resolved.font=="Yu Gothic"))&&declared.contains(&resolved.font)&&own.as_object().is_some_and(|o|o.len()==1&&o.get("character").is_some_and(Value::is_object));
            // Actual Corel-created and model-created Arial -> Calibri edits
            // use f954, with both empty and explicit regular character JSON.
            // The resolved format above still rejects every active effect.
            if flag&!4!=0 && !(flag==0xe4&&!area&&own==json!({"character":{}})&&postscript.contains(&resolved.font)) && !native_latin_edit&&!native_area_regular&&!scalar_regular&&!native_unicode_regular {return Err(unsupported("native21 character style flags unsupported"));}
            latin_override|=native_latin_edit&&!scalar_regular;
            formats.push(resolved);}
        let n=c.u32()? as usize;if n>data.len()/8{return Err(invalid("native21 character count exceeds record"));}let descriptions=c.take(n*8)?;let len=c.u32()? as usize;let encoded=c.take(len)?;let marker=c.take(1)?[0];if marker>1||marker==1&&path.is_none(){return Err(invalid("native21 text terminator/path-cache marker differs"));}let mut pos=0;let mut units=vec![];
        let mut check=|f:&Format|->Result<()> {if uniform.as_ref().is_some_and(|a|a!=f){return Err(unsupported("mixed native21 character/paragraph formatting is not represented"));}uniform=Some(f.clone());Ok(())};
        if n==0 {check(&formats[0])?;}
        for raw in descriptions.chunks_exact(8) {let d=u64::from_le_bytes(raw.try_into().unwrap());let index=((d as u32>>16)/2) as usize;check(formats.get(index).ok_or_else(||invalid("native21 style index missing"))?)?;super::text_units::decode(d,encoded,&mut pos,&mut units)?;}
        if marker==1 {super::path_text::cache(c.take(n*24)?,n)?;}
        if pos!=encoded.len(){return Err(invalid("native21 encoded text length differs"));}parts.push(String::from_utf16(&units).map_err(|_|invalid("native21 invalid UTF16 text"))?);
    }
    if c.at!=data.len(){return Err(unsupported("native21 text trailer needs preservation"));}let f=uniform.ok_or_else(||unsupported("native21 empty character defaults"))?;let content=parts.join("\n").replace("\r\n","\n").replace('\r',"\n");
    if content.chars().any(|c|c.is_control()&&c!='\n'){return Err(unsupported("native21 tabs/control characters not represented"));}
    if latin_override&&!content.is_ascii(){return Err(unsupported("native21 regular override profile is measured only for ASCII point text"));}
    if column_width==Some(0)&&!content.is_empty(){return Err(unsupported("native21 zero-width column is only represented for empty artistic text"));}
    Ok(Text{content,font:f.font,size:f.size,style:f.style,layout:TextLayoutOptions{mode:if area {TextMode::Area}else{TextMode::Point},align:f.align,letter_spacing_mm:0.,line_height_mm:f.leading},tracking:f.tracking,matrix,frame,column_width,path})
}

pub(super) fn color_string(value:&str)->Result<Option<String>> {
    use crate::paint::Paint;
    let suffix="100,00000000-0000-0000-0000-000000000000";
    Ok(match crate::paint::parse(value)? {
        Paint::None=>None,
        Paint::Rgba([r,g,b,255])=>Some(format!("RGB255,USER,{r},{g},{b},{suffix}")),
        Paint::RgbaFloat{rgb:[r,g,b],alpha:1.}=>Some(format!("RGB255,USER,{r},{g},{b},{suffix}")),
        Paint::RgbaFloat{..}=>return Err(unsupported("native21 paint alpha needs controlled native records")),
        Paint::Rgba(_)=>return Err(unsupported("native21 paint alpha needs controlled native records")),
        Paint::Cmyk{channels,scale}=>Some(format!("CMYK,USER,{},{},{},{},{suffix}",channels[0] as f64*100./scale as f64,channels[1] as f64*100./scale as f64,channels[2] as f64*100./scale as f64,channels[3] as f64*100./scale as f64)),
    })
}
pub(super) fn style_json(style:&Style)->Result<Value> {
    let fill=if let Some(c)=color_string(&style.fill)? {json!({"type":"1","primaryColor":c,"winding":"1"})}else{json!({"type":"0","winding":"1"})};
    let mut outline=json!({"width":if style.stroke=="none" {"0".into()}else{coord(style.stroke_width_mm)?.to_string()},"miterLimit":style.stroke_miterlimit.to_string(),"endCaps":match style.stroke_linecap {LineCap::Butt=>"0",LineCap::Round=>"1",LineCap::Square=>"2"},"joinType":match style.stroke_linejoin {LineJoin::Miter=>"0",LineJoin::Round=>"1",LineJoin::Bevel=>"2"}});
    if let Some(c)=color_string(&style.stroke)? {outline["color"]=json!(c);outline["scaleWithObject"]=json!(if style.stroke_scale_with_object{"1"}else{"0"});}
    Ok(json!({"fill":fill,"outline":outline,"transparency":super::opacity::json(style.opacity)?}))
}
pub(super) fn text_json(font:Value,style:&Style,layout:&TextLayoutOptions,size:f64,width:f64,tracking:i32)->Result<Value> {
    let mut value=style_json(style)?;
    let mut latin=font.clone();if font.get("technology").and_then(Value::as_str)!=Some("2"){latin["charset"]=json!("0");}
    value["character"]=json!({"angle":"0","v_shift":"0","h_shift":"0","latin":latin,"farEast":font,"RTL":font,"fill":value["fill"],"outline":value["outline"]});
    value["frame"]=if layout.mode==TextMode::Area {json!({"rtlColumns":"0","columns":format!("{},0",coord(width)?),"equalWidthColumns":"1"})}else{json!({})};
    value["paragraph"]=json!({"spacingMethod":"1","interLineSpacing":coord(layout.line_height_mm.unwrap_or(size*1.2))?.to_string(),"beforeParaSpacing":"0","afterParaSpacing":"0","interCharSpacing":tracking.to_string(),"interWordSpacing":super::typography::word_tracking(tracking).to_string(),"interLangSpacing":tracking.to_string(),"justify":match layout.align {TextAlign::Left=>"1",TextAlign::Center=>"2",TextAlign::Right=>"3"},"firstLineIndent":"0","restOfLinesIndent":"0","leftMargin":"0","bulletOn":"0","dropCapOn":"0","autoHyphenate":"0"});Ok(value)
}
pub(super) fn encoded_text(template:&[u8],area:bool,content:&str,style:&Value,language:[u8;6],width:f64,height:f64,style_ids:&[u32])->Result<Vec<u8>> {
    fn json(out:&mut Vec<u8>,value:&Value)->Result<()> {let raw=serde_json::to_vec(value).map_err(|e|invalid(e.to_string()))?;out.extend((raw.len() as u32).to_le_bytes());out.extend(raw);Ok(())}
    let on_path=u32_at(template,92)?;if on_path>1||on_path==1&&area{return Err(unsupported("native21 encoded text path/frame combination unsupported"));}
    let mut out=bytes(template,0,36)?.to_vec();set32(&mut out,20,13)?;out.extend(1700u32.to_le_bytes());out.push(0);out.extend(1u32.to_le_bytes());out.extend(bytes(template,40,52)?);out.extend(on_path.to_le_bytes());if on_path==1 {out.extend(bytes(template,96,40)?);}out.extend([0;8]);
    if area {for n in [0,-coord(height)?,0,coord(width)?] {out.extend(n.to_le_bytes());}json(&mut out,&json!({"frame":{}}))?;}
    let normalized=content.replace("\r\n","\n").replace('\r',"\n");let parts=normalized.split('\n').collect::<Vec<_>>();out.extend((parts.len() as u32).to_le_bytes());
    if style_ids.len()!=parts.len()||style_ids.contains(&0){return Err(invalid("native21 paragraph style references incomplete"));}
    for (paragraph,part) in parts.into_iter().enumerate() {
        let charset=if language==[b'C',0,b'H',0,b'C',0]{134}else{0};let legacy=style.pointer("/character/farEast/technology").and_then(Value::as_str)==Some("2");
        // Real per-character FZ controls retain GB2312 for Latin as well.
        // Forcing charset0 makes Corel substitute the Latin glyphs on save.
        let runs=if legacy {vec![(part.encode_utf16().collect(),charset)]}else{super::fonts::script_runs(part,charset)};
        let counts=runs.iter().map(|r|super::text_units::count(&r.0)).collect::<Result<Vec<_>>>()?;let count=counts.iter().sum::<usize>();
        // Paragraph words reference the document-wide stlt, including across
        // separate stories. The caller allocates matching native style rows.
        // The first character style is Western even for a CJK-first story.
        // Actual native controls mark the CJK override with3fff in the upper
        // half of its count word and carry an explicit character style.
        let cjk=runs.iter().zip(&counts).filter(|(r,_)|r.1==134).map(|(_,n)|*n).sum::<usize>();
        out.extend(style_ids[paragraph].to_le_bytes());out.extend(if area {vec![0]}else{vec![0,0]});json(&mut out,style)?;out.extend((if cjk>0 {2u32}else{1}).to_le_bytes());
        out.extend((count as u32-cjk as u32+1).to_le_bytes());out.extend(4u16.to_le_bytes());out.extend(3u32.to_le_bytes());out.extend([b'E',0,b'N',0,b'U',0]);json(&mut out,&json!({"character":{}}))?;
        if cjk>0 {out.extend((0x3fff0000|cjk as u32).to_le_bytes());out.extend(4u16.to_le_bytes());out.extend(3u32.to_le_bytes());out.extend([b'C',0,b'H',0,b'C',0]);json(&mut out,&json!({"character":style["character"]}))?;}
        out.extend((count as u32).to_le_bytes());let mut payload=vec![];for(units,charset)in &runs{let(records,bytes)=super::text_units::encoded(units,usize::from(*charset==134),*charset==0&&!legacy)?;out.extend(records);payload.extend(bytes);}out.extend((payload.len() as u32).to_le_bytes());out.extend(payload);out.push(0);
    }Ok(out)
}

#[cfg(test)]
mod tests {
    fn retained_story1500()->(Value,Vec<u8>){
        use base64::{Engine as _,engine::general_purpose::STANDARD};
        let fixture:Value=serde_json::from_str(include_str!("../../../../examples/assets/cdr/corel2019/retained-story1500.json")).unwrap();
        let bytes=STANDARD.decode(fixture["story_base64"].as_str().unwrap()).unwrap();
        assert_eq!(crate::model::sha256(&bytes),fixture["story_sha256"].as_str().unwrap());(fixture,bytes)
    }
    #[test]fn actual_retained1500_fitted_story_keeps_complete_utf8_payload(){
        let(fixture,bytes)=retained_story1500();let declared=["Arial".into()].into_iter().collect();let style=Style{fill:"#123456".into(),stroke:"#123456".into(),stroke_width_mm:0.25,..Default::default()};
        let story=text(&bytes,false,&fixture["base"],&style,&Default::default(),&declared).unwrap();
        assert_eq!(story.content,fixture["expected_text"].as_str().unwrap());assert_eq!(story.font,"Arial");assert_eq!(story.style.fill,"#123456");assert_eq!(story.style.stroke,"#123456");assert_eq!(story.style.stroke_width_mm,0.25);
        let matrix:Vec<f64>=serde_json::from_value(fixture["matrix"].clone()).unwrap();assert_eq!(story.matrix.as_slice(),matrix);assert!(story.path.is_some());assert!(story.frame.is_none());
        // Both measured headers have this same modern payload. This is not a
        // reason to accept an arbitrary old header or change emitted stories.
        let mut modern=bytes;modern[36..40].copy_from_slice(&1700u32.to_le_bytes());let other=text(&modern,false,&fixture["base"],&style,&Default::default(),&declared).unwrap();assert_eq!(other.content,story.content);assert_eq!(other.matrix,story.matrix);
    }
    #[test]fn retained1500_does_not_admit_unmeasured_profiles_effects_or_trailers(){
        let(fixture,original)=retained_story1500();let declared=["Arial".into()].into_iter().collect();
        for mode in 0..8{let mut bytes=original.clone();let base=fixture["base"].clone();match mode{
            0=>bytes[36..40].copy_from_slice(&1600u32.to_le_bytes()),
            1=>{bytes[97..101].copy_from_slice(&0u32.to_le_bytes());bytes.drain(101..141);},
            2=>{bytes.push(0);},3=>{bytes.pop();},
            4|5|6=>{let needle=match mode{4=>b"\"angle\": \"0\"".as_slice(),5=>b"\"liga\": \"0\"".as_slice(),_=>b"\"RTL\": \"0\"".as_slice()};let at=bytes.windows(needle.len()).position(|v|v==needle).unwrap();bytes[at+needle.len()-2]=b'1';},
            _=>{let n=bytes.len();bytes[n-1]=1;},
        }assert!(text(&bytes,false,&base,&Style::default(),&Default::default(),&declared).is_err(),"mode {mode}");}
    }
    use super::*;
    #[test]
    fn encoded_rectangle_scale_fields_match_real_uniform_and_independent_axis_controls(){
        for (m,expected) in [([2.,0.,0.,0.,1.,0.],[2.,1.]),([2.,0.,0.,0.,2.,0.],[2.,2.])]{let raw=encoded_rectangle_in_frame(20.,10.,0.,m).unwrap();assert_eq!([f64_at(&raw,16).unwrap(),f64_at(&raw,24).unwrap()],expected);assert_eq!(rectangle(&raw).unwrap(),(20.,10.,0.));}
        let raw=encoded_rectangle_in_frame(40.,20.,0.8,[2.5,0.,0.,0.,2.5,0.]).unwrap();assert_eq!(f64_at(&raw,40).unwrap(),0.08);assert_eq!(rectangle(&raw).unwrap(),(40.,20.,0.8));
        let mut unknown=raw;unknown[24..32].copy_from_slice(&1f64.to_le_bytes());assert!(rectangle(&unknown).is_err(),"nonzero elliptical corners are not silently circularized");
    }
    #[test]
    fn measured_relative_and_legacy_absolute_corners_remain_distinct(){
        let relative=encoded_rectangle(40.,10.,0.75).unwrap();assert_eq!(u32_at(&relative,32).unwrap(),0);assert_eq!(f64_at(&relative,40).unwrap(),0.15);assert_eq!(rectangle(&relative).unwrap(),(40.,10.,0.75));
        let mut absolute=relative.clone();for at in [32,56,80,104]{set32(&mut absolute,at,1).unwrap();set32(&mut absolute,at+4,u32::MAX).unwrap();absolute[at+8..at+16].copy_from_slice(&7500f64.to_le_bytes());}assert_eq!(rectangle(&absolute).unwrap(),(40.,10.,0.75));
        for at in [32,56,80,104]{for value in [-0.01,1.01,f64::NAN]{let mut bad=relative.clone();bad[at+8..at+16].copy_from_slice(&value.to_le_bytes());assert!(rectangle(&bad).is_err());}}
    }
    #[test]
    fn measured_solid_outline_type_does_not_admit_unknown_native_types(){
        let mut style=Style::default();
        outline(&json!({"type":"0","width":"2500","color":"RGB255,USER,0,0,0,100,00000000-0000-0000-0000-000000000000"}),&mut style).unwrap();
        assert_eq!(style.stroke,"#000000");assert_eq!(style.stroke_width_mm,0.25);
        for value in [json!("1"),json!("2"),json!(0)]{assert!(outline(&json!({"type":value}),&mut style).is_err());}
    }
    #[test]
    #[ignore="Explicit native-record isolation probes; not an automatic compatibility test"]
    fn generate_native_text_edit_isolation_probes() {
        use std::{path::PathBuf,fs};
        let source=PathBuf::from(std::env::var_os("ARRANGER_TEXT_PROBE_SOURCE").expect("source"));let control=PathBuf::from(std::env::var_os("ARRANGER_TEXT_PROBE_CONTROL").expect("control"));let out=PathBuf::from(std::env::var_os("ARRANGER_TEXT_PROBE_OUTPUT").expect("output"));assert!(source.is_absolute()&&control.is_absolute()&&out.is_absolute()&&!out.exists());fs::create_dir_all(&out).unwrap();
        let original=read_native(&fs::read(source).unwrap()).unwrap();let native=read_native(&fs::read(control).unwrap()).unwrap();
        let control_object=all(&native.nodes).into_iter().find(|n|n.kind()==Some(b"obj ")&&super::super::write::arguments(&super::super::read::own_record(n,b"loda").unwrap().data).is_ok_and(|(kind,_)|kind==6)).unwrap();
        let story=control_object.find(b"txsm").unwrap().data.clone();let (_,args)=super::super::write::arguments(&super::super::read::own_record(control_object,b"loda").unwrap().data).unwrap();let style=args.iter().find(|(k,_)|*k==201).unwrap().1.clone();
        let table=all(&native.nodes).into_iter().find(|n|n.kind()==Some(b"stlt")||n.tag==*b"stlt").unwrap().clone();
        for mask in 1..8 {
            let mut nodes=original.nodes.clone();
            fn edit(nodes:&mut[Node],mask:u32,story:&[u8],style:&[u8],table:&Node){for n in nodes {
                if mask&4!=0&&(n.kind()==Some(b"stlt")||n.tag==*b"stlt"){*n=table.clone();continue;}
                if n.kind()==Some(b"obj "){let (kind,mut args)=super::super::write::arguments(&super::super::read::own_record(n,b"loda").unwrap().data).unwrap();if kind==6{
                    if mask&1!=0{n.find_mut(b"txsm").unwrap().data=story.to_vec();}
                    if mask&2!=0{args.iter_mut().find(|(k,_)|*k==201).unwrap().1=style.to_vec();n.find_mut(b"loda").unwrap().data=super::super::write::loda(kind,&args);}
                }}if let Some(c)=&mut n.children{edit(c,mask,story,style,table);}
            }}edit(&mut nodes,mask,&story,&style,&table);
            fs::write(out.join(format!("text-isolation-{mask}.cdr")),write21(&nodes,original.members.clone()).unwrap()).unwrap();
        }
    }
    #[test]
    fn encoded_area_paragraph_references_and_default_script_follow_actual_native_controls() {
        let native=read_native(super::super::typography::PROFILE).unwrap();
        let template=&all(&native.nodes).into_iter().find(|n|n.tag==*b"txsm").unwrap().data;
        let style=regular();let ids=[7,11,19];let raw=encoded_text(template,true,"中文 mixed\n\nSecond line",&style,[b'C',0,b'H',0,b'C',0],70.,65.,&ids).unwrap();
        let mut c=Cursor{data:&raw,at:0};c.take(36).unwrap();c.u32().unwrap();c.take(1).unwrap();c.u32().unwrap();c.take(52).unwrap();assert_eq!(c.u32().unwrap(),0);c.take(8).unwrap();c.take(16).unwrap();c.json().unwrap();
        let count=c.u32().unwrap();assert_eq!(count,3);
        for index in 0..count {
            assert_eq!(c.u32().unwrap(),ids[index as usize],"native21 paragraph styles are allocated by the document writer");c.take(1).unwrap();c.json().unwrap();let styles=c.u32().unwrap();
            for i in 0..styles {let mask=c.u32().unwrap();assert_eq!(c.u16().unwrap(),4);let len=c.u32().unwrap() as usize;let language=read_wide(c.take(len*2).unwrap()).unwrap();let own=c.json().unwrap();
                if i==0{assert_eq!(language,"ENU");assert_eq!(mask>>16,0);assert_eq!(own,json!({"character":{}}));}
                else{assert_eq!(language,"CHC");assert_eq!(mask>>16,0x3fff);assert_eq!(own,json!({"character":style["character"]}));}
            }
            let chars=c.u32().unwrap() as usize;c.take(chars*8).unwrap();let bytes=c.u32().unwrap() as usize;c.take(bytes).unwrap();assert_eq!(c.take(1).unwrap(),[0]);
        }assert_eq!(c.at,raw.len());
    }
    #[test]
    fn actual_uniform_corner_scale_matches_corel_converted_curve_tangencies() {
        use kurbo::{ParamCurve,ParamCurveNearest};
        let raw=include_bytes!("../../../../examples/assets/cdr/corel2019/rounded-scaled-v21.cdr");let doc=crate::cdr::decode(raw,"scaled").unwrap().0;
        let rectangle=doc.object("edit-shape").unwrap();
        // Keep both the original local .8mm corner/2.5x body frame and
        // the independent fixed1mm world pen.
        let crate::Geometry::Rect{radius_mm}=&rectangle.geometry else{panic!()};assert_eq!(*radius_mm,0.8);assert_eq!(rectangle.style.stroke_width_mm,1.);assert!(!rectangle.style.stroke_scale_with_object);assert_eq!(*radius_mm*doc.world_transform(rectangle).unwrap().as_coeffs()[0].hypot(doc.world_transform(rectangle).unwrap().as_coeffs()[1]),2.);
        let reference=read_native(include_bytes!("../../../../examples/assets/cdr/corel2019/rounded-scaled-with-curve-v21.cdr")).unwrap();
        let nodes=all(&reference.nodes);let config=nodes.iter().find(|n|n.tag==*b"mcfg").unwrap();let origin=super::super::origin::paper(&config.data,[210.,297.],true).unwrap();
        let node=nodes.iter().find(|n|n.kind()==Some(b"obj ")&&super::super::write::arguments(&n.find(b"loda").unwrap().data).is_ok_and(|(k,_)|k==3)).unwrap();
        let (_,args)=super::super::write::arguments(&node.find(b"loda").unwrap().data).unwrap();let geometry=&args.iter().find(|(k,_)|*k==30).unwrap().1;
        let matrix=&node.find(b"trfd").unwrap().data;let at=u32_at(matrix,u32_at(matrix,8).unwrap() as usize).unwrap() as usize+16;let m:[f64;6]=std::array::from_fn(|i|f64_at(matrix,at+i*8).unwrap());
        let world=kurbo::Affine::new([m[0],-m[3],-m[1],m[4],m[2]/10000.+origin[0],origin[1]-m[5]/10000.]);
        let a=doc.world_transform(rectangle).unwrap()*rectangle.local_path().unwrap();let b=world*super::super::read::curve(geometry).unwrap();
        // Corel's ConvertToCurves uses a 0.55 cubic approximation. Its arc
        // interiors are not exact circles; verify both sets of tangencies at
        // unchanged native precision, without relabeling the earlier full-
        // contour comparison as a pass.
        for segment in b.segments(){for t in [0.,1.] {let point=segment.eval(t);let distance=a.segments().map(|s|s.nearest(point,1e-10).distance_sq.sqrt()).fold(f64::INFINITY,f64::min);assert!(distance<=0.00021);}}
        let r=*radius_mm;let w=rectangle.width_mm;let h=rectangle.height_mm;let matrix=doc.world_transform(rectangle).unwrap();
        for p in [(r,0.),(w-r,0.),(w,r),(w,h-r),(w-r,h),(r,h),(0.,h-r),(0.,r)] {
            let point=matrix*kurbo::Point::from(p);let distance=b.segments().map(|s|s.nearest(point,1e-10).distance_sq.sqrt()).fold(f64::INFINITY,f64::min);assert!(distance<=0.00021,"native corner tangency differs by {distance}mm");
        }
    }
    fn regular()->Value {text_json(json!({"font":"Arial","size":"63500","technology":"4","weight":"64"}),&Style::default(),&TextLayoutOptions::default(),6.35,80.,0).unwrap()}
    #[test]fn coupled_word_language_spacing_stays_distinct_from_independent_native_edits(){
        let mut value=regular();value["paragraph"]["interCharSpacing"]=json!("112478");value["paragraph"]["interWordSpacing"]=json!("1224956");value["paragraph"]["interLangSpacing"]=json!("112478");assert_eq!(format(&value,"ENU",false,&Style::default()).unwrap().tracking,112478);
        for(key,edit)in[("interWordSpacing","1224957"),("interLangSpacing","112479"),("interWordSpacing","1224956.5")]{let mut changed=value.clone();changed["paragraph"][key]=json!(edit);assert!(format(&changed,"ENU",false,&Style::default()).is_err());}
        value["paragraph"]["interWordSpacing"]=json!("1000000");value["paragraph"]["interLangSpacing"]=json!("0");assert!(format(&value,"ENU",false,&Style::default()).is_ok());
    }
    #[test]
    fn actual_cmyk255_and_cff_style_profiles_reject_other_channels_and_flags() {
        assert_eq!(color("CMYK255,USER,51,102,153,204,100,00000000-0000-0000-0000-000000000000").unwrap(),"cmyk255(51,102,153,204)");
        for v in ["256","-1","51.5","NaN"] {assert!(color(&format!("CMYK255,USER,{v},102,153,204,100,00000000-0000-0000-0000-000000000000")).is_err());}
        let raw=include_bytes!("../../../../reports/windows/WIN-53-20261025-01/fonts/native/font-cff-declaration-v21-corel-resaved.cdr");let native=read_native(raw).unwrap();
        let node=all(&native.nodes).into_iter().find(|n|n.kind()==Some(b"obj ")&&super::super::write::arguments(&n.find(b"loda").unwrap().data).is_ok_and(|(k,_)|k==4)).unwrap();
        let (_,args)=super::super::write::arguments(&node.find(b"loda").unwrap().data).unwrap();let base=style_record(&args.iter().find(|(k,_)|*k==201).unwrap().1).unwrap();let story=&node.find(b"txsm").unwrap().data;
        let fonts=std::collections::BTreeSet::from(["Source Han Sans JP".into()]);assert!(text(story,false,&base,&Style::default(),&fonts,&fonts).is_ok());assert!(text(story,false,&base,&Style::default(),&Default::default(),&fonts).is_err());
        let marker=[0xe4,0,3,0,0,0,b'E',0];let at=story.windows(marker.len()).position(|w|w==marker).unwrap();let mut bad=story.clone();bad[at]^=1;assert!(text(&bad,false,&base,&Style::default(),&fonts,&fonts).is_err());
    }
    #[test]
    fn unknown_paragraph_and_root_effects_are_never_silently_dropped() {
        for pointer in ["paragraph","frame",""] {let mut v=regular();if pointer.is_empty(){v["unrepresentedEffect"]=json!({});}else{v[pointer]["unrepresentedEffect"]=json!("1");}assert_eq!(format(&v,"ENU",false,&Style::default()).unwrap_err().code,"CDR_UNSUPPORTED");}
    }
    #[test]
    fn native_edit_frame_defaults_accept_only_neutral_single_column_effects(){
        let mut value=regular();
        for(key,zero)in[("verticalText","0"),("v_justify","0"),("margin","0"),("hotelText","0")]{
            value["frame"][key]=json!(zero);assert!(format(&value,"ENU",false,&Style::default()).is_ok());
            value["frame"][key]=json!("1");assert!(format(&value,"ENU",false,&Style::default()).is_err(),"{key}");value["frame"][key]=json!(zero);
        }
        value["frame"]["columns"]=json!("800000,10");assert!(format(&value,"ENU",false,&Style::default()).is_err());
    }
    #[test]
    fn active_text_effects_and_nonfinite_numeric_styles_reject() {
        for (section,key,value) in [("paragraph","bulletOn","1"),("paragraph","autoHyphenate","1"),("paragraph","leftMargin","100"),("character","angle","1"),("paragraph","interLineSpacing","NaN")] {let mut v=regular();v[section][key]=json!(value);assert!(format(&v,"ENU",false,&Style::default()).is_err(),"{section}/{key}");}
        let mut v=regular();v["character"]["latin"]["italic"]=json!("1");assert!(format(&v,"ENU",false,&Style::default()).is_err());
        let mut v=regular();v["character"]["latin"]["technology"]=json!("2");v["character"]["latin"]["charset"]=json!("134");assert!(format(&v,"ENU",false,&Style::default()).is_err(),"technology2 is restricted to the measured legacy family");
    }
    #[test]
    fn measured_regular_area_flags_require_declared_fonts_and_reject_unknown_bits() {
        let native=read_native(include_bytes!("../../../../examples/assets/cdr/corel2019/area-regular-style4904-v21.cdr")).unwrap();
        let node=all(&native.nodes).into_iter().find(|n|n.kind()==Some(b"obj ")&&super::super::write::arguments(&n.find(b"loda").unwrap().data).is_ok_and(|(k,_)|k==6)).unwrap();
        let (_,args)=super::super::write::arguments(&node.find(b"loda").unwrap().data).unwrap();let base=style_record(&args.iter().find(|(k,_)|*k==201).unwrap().1).unwrap();let story=&node.find(b"txsm").unwrap().data;let fonts=std::collections::BTreeSet::from(["微软雅黑".into()]);
        assert!(text(story,true,&base,&Style::default(),&Default::default(),&fonts).is_ok());assert!(text(story,true,&base,&Style::default(),&Default::default(),&Default::default()).is_err());
        let marker=[4,0x49,3,0,0,0,b'E',0];let at=story.windows(marker.len()).position(|w|w==marker).unwrap();let mut bad=story.clone();bad[at]^=1;assert!(text(&bad,true,&base,&Style::default(),&Default::default(),&fonts).is_err());
    }
    #[test]
    fn measured_scalar_legacy_flags_keep_complete_text_and_require_declared_neutral_fonts() {
        for (raw,family,expected) in [(include_bytes!("../../../../examples/assets/cdr/corel2019/extb-scalar-native-v21.cdr").as_slice(),"SimSun-ExtB","A𠮷𠀀𩸽B"),(include_bytes!("../../../../examples/assets/cdr/corel2019/extg-scalar-native-v21.cdr").as_slice(),"SimSun-ExtG","𰀀")] {
            let native=read_native(raw).unwrap();let node=all(&native.nodes).into_iter().find(|n|n.kind()==Some(b"obj ")).unwrap();
            let (_,args)=super::super::write::arguments(&node.find(b"loda").unwrap().data).unwrap();let base=style_record(&args.iter().find(|(k,_)|*k==201).unwrap().1).unwrap();let story=&node.find(b"txsm").unwrap().data;let fonts=std::collections::BTreeSet::from([family.into()]);
            assert_eq!(text(story,false,&base,&Style::default(),&Default::default(),&fonts).unwrap().content,expected);
            assert!(text(story,false,&base,&Style::default(),&Default::default(),&Default::default()).is_err());
            let marker=[0x54,0xf9,3,0,0,0,b'C',0];let at=story.windows(marker.len()).position(|w|w==marker).unwrap();let mut bad=story.clone();bad[at]^=1;assert!(text(&bad,false,&base,&Style::default(),&Default::default(),&fonts).is_err());
            let mut bad=story.clone();let marker=b"\"italic\"";let mut changes=0;
            for at in 0..=bad.len()-marker.len() {if &bad[at..at+marker.len()]==marker {
                let mut p=at+marker.len();while bad[p].is_ascii_whitespace(){p+=1;}assert_eq!(bad[p],b':');p+=1;while bad[p].is_ascii_whitespace(){p+=1;}assert_eq!(bad[p],b'\"');assert_eq!(bad[p+1],b'0');bad[p+1]=b'1';changes+=1;
            }}
            assert!(changes>0);assert!(text(&bad,false,&base,&Style::default(),&Default::default(),&fonts).is_err());
        }
    }
    #[test]
    fn relative_grid_inverse_does_not_snap_neighbouring_fraction_edits() {
        let raw=encoded_rectangle(60.,40.,0.1235).unwrap();
        assert_eq!(rectangle(&raw).unwrap().2,0.1235);
        let value=f64_at(&raw,40).unwrap();
        for bits in [value.to_bits()-1,value.to_bits()+1] {
            let mut edited=raw.clone();for at in [40,64,88,112]{edited[at..at+8].copy_from_slice(&f64::from_bits(bits).to_le_bytes());}
            assert_ne!(rectangle(&edited).unwrap().2,0.1235);
        }
    }
    #[test]
    fn malformed_style_envelopes_and_independent_rectangle_corners_reject() {
        let data=encoded_style(&json!({})).unwrap();for cut in 0..data.len(){assert!(style_record(&data[..cut]).is_err());}
        let mut r=encoded_rectangle(40.,20.,2.).unwrap();assert_eq!(rectangle(&r).unwrap(),(40.,20.,2.));r[64..72].copy_from_slice(&30000f64.to_le_bytes());assert!(rectangle(&r).is_err());
    }
    #[test]
    fn corner_type_byte_is_not_a_second_floating_radius() {
        for radius in [0.,0.75] {for absolute in [false,true] {
            let mut raw=encoded_rectangle(40.,20.,radius).unwrap();
            if absolute{for p in [32,56,80,104]{set32(&mut raw,p,1).unwrap();set32(&mut raw,p+4,u32::MAX).unwrap();raw[p+8..p+16].copy_from_slice(&(radius*10000.).to_le_bytes());}}
            for p in [32,56,80,104]{raw[p+16..p+24].copy_from_slice(&[0,0,0,0,0xc0,1,0,0]);}
            assert_eq!(rectangle(&raw).unwrap(),(40.,20.,radius));
            for p in [32,56,80,104]{for kind in [1,2,255]{let mut changed=raw.clone();changed[p+16]=kind;assert!(rectangle(&changed).is_err(),"active corner type {kind} at {p}");}}
        }}
    }
    #[test]
    fn actual_native21_truncated_stories_never_return_partial_text() {
        let native=read_native(include_bytes!("../../../../reports/windows/WIN-CDR-SAVE-20261022-01/focused/native-preservation/identity-v21.cdr")).unwrap();
        let node=all(&native.nodes).into_iter().find(|n|n.tag==*b"txsm").unwrap();
        for cut in (0..node.data.len()).step_by(47){let result=std::panic::catch_unwind(||text(&node.data[..cut],false,&json!({}),&Style::default(),&Default::default(),&Default::default()));assert!(result.is_ok());assert!(result.unwrap().is_err());}
    }
}
