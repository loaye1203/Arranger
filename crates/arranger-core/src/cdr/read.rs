use super::{binary::*, invalid, unsupported, Result};
use crate::{Document, Geometry, Object, Resource, Style, ImageFit, TextAlign, TextMode, TextLayoutOptions, command::Notice, model::{sha256,valid_id}};
use kurbo::{BezPath, Point, Shape};
use std::{collections::{BTreeMap,BTreeSet},io::Cursor as IoCursor};

/// Controlled Corel2019 original/resaved paths (including unpainted clip
/// frames) bind14001=1 to nonzero and its absence to evenodd in14 and21.
/// Native21 JSON alone did not set the SDK FillMode in our previous output.
pub(super) fn fill_rule(args:&BTreeMap<u32,Vec<u8>>,style:Option<&serde_json::Value>)->Result<crate::FillRule> {
    let rule=match args.get(&14001) {
        None=>crate::FillRule::EvenOdd,
        Some(v) if v.as_slice()==[1,0,0,0]=>crate::FillRule::NonZero,
        _=>return Err(unsupported("unknown native path fill-mode argument")),
    };
    if let Some(v)=style.and_then(|s|s.get("fill")).and_then(|f|f.get("winding")) {
        let expected=if rule.is_nonzero(){"1"}else{"0"};
        if v.as_str()!=Some(expected){return Err(unsupported("native21 winding conflicts with native path fill-mode argument"));}
    }
    Ok(rule)
}

pub(super) fn color(data:&[u8],at:usize)->Result<String> {
    let model=u16_at(data,at)?;let value=u32_at(data,at+8)?;
    if u16_at(data,at+2)?!=5 || u32_at(data,at+4)?!=0 {return Err(unsupported("native palette/profile color references are not imported"));}
    match model {
        5=>{if value>0xffffff {return Err(invalid("invalid RGB color value"));}Ok(format!("#{value:06x}"))},
        2|3=>{let [c,m,y,k]=value.to_le_bytes();if model==2 && [c,m,y,k].into_iter().any(|v|v>100){return Err(invalid("invalid native CMYK100 channel"));}Ok(format!("{}({c},{m},{y},{k})",if model==2 {"cmyk"}else{"cmyk255"}))},
        _=>Err(unsupported("native color model is outside solid RGB/CMYK100/CMYK255")),
    }
}
pub(super) fn flags(node:&Node)->Result<u32> {
    let items=node.children.as_ref().ok_or_else(||invalid("missing native flag container"))?;
    let found=items.iter().filter(|n|n.tag==*b"flgs").collect::<Vec<_>>();
    if found.len()!=1 {return Err(invalid("missing or ambiguous native flags"));}u32_at(&found[0].data,0)
}
pub(super) fn outline_body(data:&[u8])->Result<usize> {
    let mut at=4;
    loop {let tag=u32_at(data,at)?;let length=u32_at(data,at+4)? as usize;at+=8;if tag==1 {return Ok(at);}bytes(data,at,length)?;at+=length;}
}
pub(super) fn miter_limit(data:&[u8])->Result<f64> {
    let mut at=4;let mut found=None;
    loop {
        let tag=u32_at(data,at)?;let length=u32_at(data,at+4)? as usize;at+=8;
        if tag==1 {break;}
        bytes(data,at,length)?;
        if tag==3 {if length!=4 || found.is_some(){return Err(invalid("ambiguous native miter limit"));}found=Some(u32_at(data,at)? as f64/1_000_000.);}
        at+=length;
    }
    let value=found.ok_or_else(||unsupported("native outline has no explicit miter limit"))?;
    if value<1. {return Err(invalid("native miter limit is below one"));}Ok(value)
}
pub(super) fn outline(data:&[u8])->Result<(String,f64)> {
    let at=outline_body(data)?;
    let kind=u16_at(data,at)?;
    if kind&1!=0 {return Ok(("none".into(),0.));}
    if ![2,34].contains(&kind) || u16_at(data,at+2)?>2 || u16_at(data,at+4)?>2 {return Err(unsupported("non-solid native outline is not imported"));}
    let width=i32_at(data,at+6)? as f64/10000.;if width<0. {return Err(invalid("negative outline width"));}
    Ok((color(data,at+64)?,width))
}
pub(super) fn curve(data:&[u8])->Result<BezPath> {
    let n=u16_at(data,0)? as usize;let end=4+9*n;
    if n==0 || u16_at(data,2)?!=0 || !matches!(data.get(end..),Some([])|Some([0,0,0,0,0,0,0,0])) {return Err(unsupported("unrecognized native curve layout"));}
    let flags=bytes(data,4+8*n,n)?;let mut path=BezPath::new();let mut controls=vec![];let mut active=false;let mut drawn=false;let mut closed_hint=false;
    for (i,&flag) in flags.iter().enumerate() {
        let p=Point::new(i32_at(data,4+8*i)? as f64/10000.,-i32_at(data,8+8*i)? as f64/10000.);
        match flag&0xc0 {
            0=>{if !controls.is_empty() || active&&(!drawn||closed_hint) {return Err(invalid("incomplete native subpath"));}path.move_to(p);active=true;drawn=false;closed_hint=flag&8!=0;},
            0xc0=>{if !active || controls.len()>=2 || flag&8!=0 {return Err(invalid("invalid cubic control"));}controls.push(p);continue;},
            0x40=>{if !active || !controls.is_empty() {return Err(invalid("line without a native subpath"));}path.line_to(p);drawn=true;},
            _=>{if !active || controls.len()!=2 {return Err(invalid("cubic requires two native controls"));}path.curve_to(controls[0],controls[1],p);controls.clear();drawn=true;},
        }
        if flag&8!=0 && flag&0xc0!=0 {if !drawn {return Err(invalid("empty closed native curve"));}path.close_path();active=false;closed_hint=false;}
    }
    if !controls.is_empty() || active&&(!drawn||closed_hint) {return Err(invalid("incomplete native curve"));}Ok(path)
}
pub(super) fn own_record<'a>(node:&'a Node,tag:&[u8;4])->Result<&'a Node> {
    optional_own_record(node,tag)?.ok_or_else(||invalid("missing or ambiguous own object metadata"))
}
pub(super) fn optional_own_record<'a>(node:&'a Node,tag:&[u8;4])->Result<Option<&'a Node>> {
    fn walk<'a>(node:&'a Node,tag:&[u8;4],found:&mut Vec<&'a Node>) {
        if node.tag==*tag {found.push(node);}
        if let Some(children)=&node.children {for n in children {if !matches!(n.kind(),Some(b"obj "|b"grp "|b"lnkg"|b"clpt")) {walk(n,tag,found);}}}
    }
    let mut found=vec![];walk(node,tag,&mut found);
    if found.len()>1 {return Err(invalid("missing or ambiguous own object metadata"));}Ok(found.first().copied())
}
pub(super) fn matrix(node:&Node)->Result<[f64;6]> {
    let data=&own_record(node,b"trfd")?.data;
    if u32_at(data,0)? as usize!=data.len() || u32_at(data,4)?!=1 {return Err(unsupported("only a single native affine transform is imported"));}
    let at=u32_at(data,u32_at(data,8)? as usize)? as usize;
    if u16_at(data,at+8)?!=8 {return Err(unsupported("unknown native transform type"));}
    let mut result=[0.;6];for (i,v) in result.iter_mut().enumerate(){*v=f64_at(data,at+16+8*i)?;}
    if (result[0]*result[4]-result[1]*result[3]).abs()<1e-12 {return Err(unsupported("singular native transform"));}Ok(result)
}
struct Cursor<'a>{data:&'a[u8],at:usize}
impl<'a> Cursor<'a>{
    fn new(data:&'a[u8])->Self{Self{data,at:0}}
    fn take(&mut self,n:usize)->Result<&'a[u8]>{let out=bytes(self.data,self.at,n)?;self.at+=n;Ok(out)}
    fn u32(&mut self)->Result<u32>{Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))}
    fn u16(&mut self)->Result<u16>{Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))}
    fn byte(&mut self)->Result<u8>{Ok(self.take(1)?[0])}
    fn array(&mut self,size:usize)->Result<()> {let n=self.u32()? as usize;self.take(n.checked_mul(size).ok_or_else(||invalid("CDR table overflow"))?)?;Ok(())}
}
#[derive(Clone,Default)]
struct TextStyle {font:u16,size:f64,fill:u32,line:u32,slots:BTreeMap<String,(u16,f64,u32)>,face:u32,spacing:i32,dx:i32,dy:i32,align:u32,area:Option<(TextAlign,f64,i32)>,point:Option<(Option<f64>,i32)>}
fn defaults(data:&[u8])->Result<BTreeMap<u32,TextStyle>> {
    let mut c=Cursor::new(bytes(data,4,data.len().saturating_sub(4))?);let count=c.u32()?;
    if count==0 {return Ok(BTreeMap::new());}if count>4096 {return Err(invalid("too many native text styles"));}
    let table=super::typography::Tables::parse(data)?;let (_,area_profile)=super::typography::profile()?;
    let mut fills=BTreeMap::new();let mut lines=BTreeMap::new();let mut fonts=BTreeMap::new();let mut aligns=BTreeMap::new();
    for _ in 0..c.u32()? {let key=c.u32()?;c.take(4)?;fills.insert(key,c.u32()?);c.take(48)?;}
    for _ in 0..c.u32()? {let key=c.u32()?;c.take(4)?;lines.insert(key,c.u32()?);}
    for _ in 0..c.u32()? {let row=c.take(60)?;let mut style=TextStyle::default();for (lang,offset,size,face) in [("ENU",24,36,48),("CHC",28,40,52),("complex",32,44,56)] {style.slots.insert(lang.into(),(u16_at(row,offset)?,i32_at(row,size)? as f64/10000.,u32_at(row,face)?));}let (font,size,face)=style.slots["ENU"];style.font=font;style.size=size;style.face=face;fonts.insert(u32_at(row,0)?,style);}
    for _ in 0..c.u32()? {let row=c.take(12)?;aligns.insert(u32_at(row,0)?,u32_at(row,8)?);}
    for size in [52,152,784] {c.array(size)?;}
    for _ in 0..c.u32()? {c.take(44)?;let flag=c.u32()?;c.take(if flag!=0 {68}else{12})?;}
    for size in [28,36,28,12] {c.array(size)?;}
    let mut styles=BTreeMap::new();
    for _ in 0..count {let level=c.u32()?;let key=c.u32()?;let _parent=c.u32()?;c.take(8)?;let n=c.u32()? as usize;c.take(n.checked_mul(2).ok_or_else(||invalid("text style name overflow"))?)?;
        let fill=c.u32()?;let line=c.u32()?;let mut style=TextStyle::default();
        if level>1 {style=fonts.get(&c.u32()?).cloned().unwrap_or_default();style.align=*aligns.get(&c.u32()?).unwrap_or(&0);c.take(12)?;}
        if level>2 {c.take(20)?;}
        style.point=table.styles.iter().find(|r|u32_at(r,4).ok()==Some(key)).and_then(|row|table.point_options(row,&area_profile).ok());
        style.area=table.styles.iter().find(|r|u32_at(r,4).ok()==Some(key)).and_then(|row|table.area_options(row,&area_profile).ok());
        style.fill=*fills.get(&fill).unwrap_or(&0);style.line=*lines.get(&line).unwrap_or(&0);styles.insert(key,style);
    }Ok(styles)
}
fn artistic(data:&[u8],area:bool,defaults:&BTreeMap<u32,TextStyle>,fills:&BTreeMap<u32,&[u8]>,lines:&BTreeMap<u32,&[u8]>,postscript:&BTreeSet<u16>)->Result<(String,TextStyle,Option<super::path_text::Options>)> {
    let mut c=Cursor::new(data);let flag=c.u32()?;c.take(32)?;
    if (flag==0)!=area || c.u32()?!=1 {return Err(unsupported("only single-frame artistic text is imported; area/linked text remains unsupported"));}
    c.take(4+48)?;let on_path=c.u32()?;let path=match on_path {0=>None,1 if !area=>Some(super::path_text::Options::parse(c.take(40)?)?),_=>return Err(unsupported("native path/frame flags are not represented"))};if area {c.take(36)?;}
    let count=c.u32()?;if count>65535 {return Err(invalid("too many native text paragraphs"));}if path.is_some()&&count!=1{return Err(unsupported("native multiline path text is not represented"));}
    let mut parts=vec![];let mut used:Option<TextStyle>=None;
    for _ in 0..count {
        let inherited=defaults.get(&c.u32()?).cloned().unwrap_or_default();if !area&&inherited.point.is_none() {return Err(unsupported("unrepresented point paragraph defaults"));}let flags=c.take(if area {1}else{2})?;if area && flags.iter().any(|v|*v!=0) {return Err(unsupported("additional paragraph flags are unsupported"));}let n=c.u32()?;
        if n>65535 {return Err(invalid("too many character styles"));}let mut styles=vec![];
        for _ in 0..n {
            let _count=c.u16()?;let f2=c.byte()?;let f3=c.byte()?;let mut s=inherited.clone();
            if f3&!0x28!=0 {return Err(unsupported("native character style flags are unsupported"));}
            if f2&1!=0 {s.font=c.u16()?;c.take(2)?;}
            for (flag,label) in [(2,0),(4,1),(8,2),(16,3),(32,4)] {if f2&flag!=0 {let v=c.u32()? as i32;match label {0=>s.face=v as u32,1=>s.size=v as f64/10000.,2=>s.spacing=v,3=>s.dx=v,_=>s.dy=v}}}
            if f2&64!=0 {s.fill=c.u32()?;c.take(48)?;}if f2&128!=0 {s.line=c.u32()?;}
            let language=if f3&8!=0 {let size=c.u32()? as usize;read_wide(c.take(size.checked_mul(2).ok_or_else(||invalid("language size overflow"))?)?)?}else{String::new()};
            if f3&32!=0 && *bytes(c.data,c.at,1)?.first().unwrap()!=0 {return Err(unsupported("text fill transforms remain unsupported"));}
            if let Some(&(font,size,face))=inherited.slots.get(&language){if f2&1==0 {s.font=font;}if f2&4==0 {s.size=size;}if f2&2==0 {s.face=face;}}
            styles.push(s);
        }
        let count=c.u32()? as usize;if count>data.len()/8 {return Err(invalid("native character count exceeds record"));}
        let descriptions=c.take(count*8)?;let size=c.u32()? as usize;let encoded=c.take(size)?;let marker=c.byte()?;if marker>1||marker==1&&path.is_none() {return Err(invalid("native text terminator/path-cache marker differs"));}
        let mut at=0;let mut units=vec![];
        if count==0 {if area&&inherited.area.is_none() {return Err(unsupported("unrepresented empty paragraph defaults"));}if let Some(prior)=&used {if prior.area!=inherited.area || prior.point!=inherited.point || prior.font!=inherited.font || prior.size!=inherited.size || fill_value(prior.fill,fills)?!=fill_value(inherited.fill,fills)? || line_value(prior.line,lines)?!=line_value(inherited.line,lines)? {return Err(unsupported("mixed empty paragraph formatting is unsupported"));}}else{used=Some(inherited.clone());}}
        for description in descriptions.chunks_exact(8) {
            let d=u64::from_le_bytes(description.try_into().unwrap());let style_index=((d as u32>>16)/2) as usize;let s=styles.get(style_index).ok_or_else(||invalid("native character style index missing"))?.clone();
            // Corel's real CFF SaveAs14 repeats the regular PostScript4096
            // face in both the font declaration and resolved paragraph slot.
            // Other weight/effect bits still cannot become ordinary text.
            if !([0,64].contains(&s.face)||s.face==4096&&postscript.contains(&s.font)) || s.spacing!=0 || s.dx!=0 || s.dy!=0 || (!area && (s.point.is_none()||![0,1,2,3].contains(&s.align))) || (area && s.area.is_none()) {return Err(unsupported("formatted typography requires a richer text importer"));}
            if let Some(prior)=&used {if prior.font!=s.font || prior.size!=s.size || (area && prior.area!=s.area) || (!area&&prior.point!=s.point) || fill_value(prior.fill,fills)?!=fill_value(s.fill,fills)? || line_value(prior.line,lines)?!=line_value(s.line,lines)? {return Err(unsupported("mixed character typography is not represented by Arranger's uniform text model"));}}else{used=Some(s);}
            super::text_units::decode(d,encoded,&mut at,&mut units)?;
        }
        if marker==1 {super::path_text::cache(c.take(count*24)?,count)?;}
        if at!=encoded.len() {return Err(invalid("native text byte count differs"));}parts.push(String::from_utf16(&units).map_err(|_|invalid("invalid native text UTF16"))?);
    }
    if c.at!=data.len() {return Err(unsupported("unknown native text trailer"));}
    Ok((parts.join("\n").replace("\r\n","\n").replace('\r',"\n"),used.ok_or_else(||unsupported("empty native text style is unresolved"))?,path))
}
fn raster(data:&[u8],at:usize)->Result<(image::RgbaImage,u32,usize,Option<super::native_pixels::Cmyk>,Option<super::native_pixels::Gray16>,Option<super::native_pixels::Rgb16>)> {
    if bytes(data,at,2)?!=b"RI" {return Err(invalid("native bitmap RI header missing"));}
    let length=u32_at(data,at+2)? as usize;let model=u32_at(data,at+14)?;let w=u32_at(data,at+22)?;let h=u32_at(data,at+26)?;let planes=u32_at(data,at+30)?;let bpp=u32_at(data,at+34)?;let stride=u32_at(data,at+38)? as usize;let size=u32_at(data,at+42)? as usize;
    let channels=match model {1=>3,3=>4,12=>6,14=>2,5|99=>1,_=>return Err(unsupported("native image color model is unsupported"))};
    if matches!(model,3|12|14)&&u32_at(data,at+18)?!=1{return Err(unsupported("unmeasured native CMYK/grayscale16/RGB16 bitmap flags"));}
    if w==0 || h==0 || w as u64*h as u64>crate::resource::MAX_PIXELS || planes!=1 || bpp!=channels*8 || stride<(w as usize*channels as usize) || stride.checked_mul(h as usize)!=Some(size) || size.checked_add(78)!=Some(length) {return Err(invalid("inconsistent native image layout"));}
    let raw=bytes(data,at+78,size)?;let mut img=image::RgbaImage::new(w,h);
    let mut cmyk=if model==3{Some(super::native_pixels::Cmyk{width:w,height:h,pixels:vec![]})}else{None};
    let mut gray16=if model==14{Some(super::native_pixels::Gray16{width:w,height:h,pixels:Vec::with_capacity(w as usize*h as usize)})}else{None};
    let mut rgb16=if model==12{Some(super::native_pixels::Rgb16{width:w,height:h,pixels:Vec::with_capacity(w as usize*h as usize*3)})}else{None};
    for y in 0..h {for x in 0..w {let p=(h-1-y) as usize*stride+x as usize*channels as usize;let rgba=if model==1 {[raw[p+2],raw[p+1],raw[p],255]}else if model==3 {
        cmyk.as_mut().unwrap().pixels.extend_from_slice(&raw[p..p+4]);
        let channel=|v:u8|((u32::from(255-v)*u32::from(255-raw[p+3])+127)/255) as u8;
        [channel(raw[p]),channel(raw[p+1]),channel(raw[p+2]),255]
    }else if model==14 {let sample=u16::from_le_bytes([raw[p],raw[p+1]]);gray16.as_mut().unwrap().pixels.push(sample);[0,0,0,255]}
    else if model==12 {for c in[2,1,0]{rgb16.as_mut().unwrap().pixels.push(u16::from_le_bytes([raw[p+c*2],raw[p+c*2+1]]));}[0,0,0,255]}
    else if model==99 {[0,0,0,raw[p]]}else{[raw[p],raw[p],raw[p],255]};img.put_pixel(x,y,image::Rgba(rgba));}}
    if let Some(gray)=&gray16{img=image::DynamicImage::ImageLuma16(gray.image()).to_rgba8();}
    if let Some(rgb)=&rgb16{img=image::DynamicImage::ImageRgb16(rgb.image()).to_rgba8();}
    Ok((img,model,length,cmyk,gray16,rgb16))
}
fn bitmap(data:&[u8])->Result<(u32,image::RgbaImage,Option<super::native_pixels::Cmyk>,Option<super::native_pixels::Gray16>,Option<super::native_pixels::Rgb16>)> {
    if u32_at(data,12)? as usize+8!=data.len() || u32_at(data,20)?!=32 {return Err(unsupported("unrecognized native bitmap container"));}
    let key=u32_at(data,0)?;let (mut image,model,len,cmyk,gray16,rgb16)=raster(data,40)?;
    if model==99 {return Err(invalid("primary bitmap cannot be alpha"));}let mut next=40+len;let mask=u32_at(data,24)? as usize;
    if mask!=0 {if cmyk.is_some()||gray16.is_some()||rgb16.is_some(){return Err(unsupported("CMYK/grayscale16/RGB16 with a native alpha plane has no measured lossless profile"));}if mask+8!=next {return Err(invalid("native alpha pointer differs"));}let (alpha,model,len,_,_,_)=raster(data,next)?;if model!=99 || alpha.dimensions()!=image.dimensions() {return Err(invalid("native alpha layout differs"));}for (dst,src) in image.pixels_mut().zip(alpha.pixels()){dst[3]=src[3];}next+=len;}
    if next!=data.len() {return Err(unsupported("unknown native bitmap trailer"));}Ok((key,image,cmyk,gray16,rgb16))
}
fn fill_value(id:u32,table:&BTreeMap<u32,&[u8]>)->Result<String> {
    if id==0 {return Ok("none".into());}let data=table.get(&id).ok_or_else(||invalid("referenced fill missing"))?;
    match u16_at(data,12)? {0=>Ok("none".into()),1=>color(data,27),_=>Err(unsupported("native fill type is not imported"))}
}
fn line_value(id:u32,table:&BTreeMap<u32,&[u8]>)->Result<(String,f64)> {
    if id==0 {Ok(("none".into(),0.))}else{outline(table.get(&id).ok_or_else(||invalid("referenced outline missing"))?)}
}
fn style(args:&BTreeMap<u32,Vec<u8>>,fills:&BTreeMap<u32,&[u8]>,lines:&BTreeMap<u32,&[u8]>)->Result<Style> {
    let fill=u32_at(args.get(&20).ok_or_else(||invalid("fill reference missing"))?,0)?;let line=u32_at(args.get(&10).ok_or_else(||invalid("outline reference missing"))?,0)?;
    let fill=if fill==0 {"none".into()}else{let d=fills.get(&fill).ok_or_else(||invalid("referenced fill missing"))?;match u16_at(d,12)? {0=>"none".into(),1=>color(d,27)?,_=>return Err(unsupported("native fill type is not imported"))}};
    let (stroke,width)=if line==0 {("none".into(),0.)}else{outline(lines.get(&line).ok_or_else(||invalid("referenced outline missing"))?)?};
    let mut value=Style{fill,stroke,stroke_width_mm:width,..Default::default()};
    if line!=0 {let data=lines[&line];let at=outline_body(data)?;
        value.stroke_miterlimit=miter_limit(data)?;
        value.stroke_linecap=match u16_at(data,at+2)? {0=>crate::LineCap::Butt,1=>crate::LineCap::Round,2=>crate::LineCap::Square,_=>return Err(unsupported("unknown outline caps"))};
        value.stroke_linejoin=match u16_at(data,at+4)? {0=>crate::LineJoin::Miter,1=>crate::LineJoin::Round,2=>crate::LineJoin::Bevel,_=>return Err(unsupported("unknown outline join"))};
    }Ok(value)
}
pub(super) fn decode(data:&[u8],document_id:&str)->Result<(Document,Vec<Notice>)> {decode_charts(data,document_id,true)}
pub(super) fn decode_geometry(data:&[u8],document_id:&str)->Result<(Document,Vec<Notice>)> {decode_charts(data,document_id,false)}
fn decode_charts(data:&[u8],document_id:&str,source_charts:bool)->Result<(Document,Vec<Notice>)> {
    let native=read_native(data)?;
    let embedded_fonts = native.version == 2100 && super::metadata::declares_embedded_fonts(&native.members);
    let vault=super::resource_vault::read(&native.members)?;let mut frames=if source_charts{super::local_frames::read(&native.members)?}else{None};let cyclic_source=if source_charts{super::cyclic_source::read(&native.members,&mut frames)?}else{None};let text_params=if source_charts{super::text_params::read(&native.members)?}else{None};let image_params=if source_charts{super::image_params::read(&native.members)?}else{None};let mut geometry_params=if source_charts{super::geometry_params::read(&native.members)?}else{None};let paint_notes=super::paint_params::read(&native.members)?;let paint_layers=if source_charts{super::paint_layers::read(&native.members)?}else{None};let paint_notation=if source_charts{super::paint_notation::read(&native.members)?}else{None};let has_vault=vault.is_some();let modern=native.version==2100;let tree=native.nodes;drop(native.members);let mut layer_proof=super::paint_layers::native_proof(&tree,paint_layers.as_ref())?;let path_links=super::path_text::Links::new(&tree)?;let mut path_ids=BTreeMap::new();let mut path_options=BTreeMap::new();let mut path_shears=BTreeMap::new();let mut links=super::clipping::Links::new(&tree)?;let all=all(&tree);let configs=all.iter().filter(|n|n.tag==*b"mcfg").collect::<Vec<_>>();if configs.len()!=1 {return Err(invalid("one native page configuration required"));}
    let mut source_names=frames.as_ref().map(super::local_frames::source_names).unwrap_or_default();if let Some(params)=&image_params{source_names.extend(super::image_params::source_names(params));}let mut source_indices=frames.as_ref().map(super::local_frames::source_indices).unwrap_or_default();if let Some(params)=&image_params{source_indices.extend(super::image_params::source_indices(params));}let mut native_static_ids=BTreeMap::new();
    let w=i32_at(&configs[0].data,12)? as f64/10000.;let h=i32_at(&configs[0].data,16)? as f64/10000.;let mut image_object_keys=BTreeMap::new();
    let (pages,scenes,pages_assigned)=super::pages::decode(&tree,[w,h])?;
    let mut fills=BTreeMap::new();let mut lines=BTreeMap::new();let mut fonts=BTreeMap::new();let mut postscript=BTreeSet::new();let mut styles=BTreeMap::new();let mut images=BTreeMap::new();let mut resources=BTreeMap::new();
    let mut native_cmyk=BTreeMap::new();let mut native_gray16=BTreeMap::new();let mut native_rgb16=BTreeMap::new();let mut scaled_outlines=BTreeSet::new();let mut nib_recharts=0;let mut nib_objects=BTreeMap::new();let mut scaled_widths=0;
    for n in &all {
        match &n.tag {
            b"fild"=>{if fills.insert(u32_at(&n.data,0)?,n.data.as_slice()).is_some(){return Err(invalid("duplicate native fill"));}},
            b"outl"=>{if lines.insert(u32_at(&n.data,0)?,n.data.as_slice()).is_some(){return Err(invalid("duplicate native outline"));}},
            b"font"=>{let key=u16_at(&n.data,0)?;let font=read_wide(bytes(&n.data,18,64)?)?;if u32_at(&n.data,4)?==4096 || modern&&u32_at(&n.data,4)?==64&&bytes(&n.data,8,10)?==[0;10]&&super::fonts::verified_cff(&font)? {postscript.insert(key);}if let Some(prior)=fonts.insert(key,font.clone()){if prior!=font {return Err(unsupported("script-specific font substitutions are unsupported"));}}},
            b"bmp "=>{let (key,image,cmyk,gray16,rgb16)=bitmap(&n.data)?;if let Some(cmyk)=cmyk{native_cmyk.insert(key,cmyk);}let output=if let Some(gray)=gray16{let output=image::DynamicImage::ImageLuma16(gray.image());native_gray16.insert(key,gray);output}else if let Some(rgb)=rgb16{let output=image::DynamicImage::ImageRgb16(rgb.image());native_rgb16.insert(key,rgb);output}else{image::DynamicImage::ImageRgba8(image.clone())};let mut encoded=IoCursor::new(Vec::new());output.write_to(&mut encoded,image::ImageFormat::Png).map_err(|e|invalid(e.to_string()))?;let raw=encoded.into_inner();let id=format!("cdr-image-{}",&sha256(&raw)[..16]);resources.entry(id.clone()).or_insert(Resource::png(&raw)?);if images.insert(key,(id,image.dimensions())).is_some(){return Err(invalid("duplicate native bitmap key"));}},
            _=>{},
        }
        if n.kind()==Some(b"stlt") {styles.extend(defaults(&n.data)?);}
    }
    for style in styles.values() {
        if style.fill!=0 && !fills.contains_key(&style.fill) {return Err(invalid("native inherited text style references a missing fill"));}
        if style.line!=0 && !lines.contains_key(&style.line) {return Err(invalid("native inherited text style references a missing outline"));}
    }
    let owned_definitions=super::local_data::definitions(&tree,modern,"Arranger_")?;
    let bound_static_ids=if owned_definitions.is_empty(){BTreeSet::new()}else{super::local_data::values(&tree,&owned_definitions)?.into_iter().map(|(id,_)|id).collect()};
    let mut doc=Document::new(document_id.into(),w,h)?;let mut ids=BTreeSet::new();let mut native_static_seen=BTreeSet::new();let mut unbound_static_alias=false;let mut renamed=false;let mut clip_frames=vec![];
    doc.pages=pages;
    for scene in &scenes {
    let page=scene.node;let [ox,oy]=super::origin::paper(&configs[0].data,scene.size,modern)?;let owner=&scene.page_id;
    let mut native_objects=vec![];links.scene(page.children.as_deref().unwrap_or_default(),None,&mut native_objects,0)?;
    let page_start=doc.objects.len();let mut group_matrices=BTreeMap::new();
    for (node,parent_index) in native_objects {
        let index=doc.objects.len();let group=node.kind()==Some(b"grp ");
        let (kind,args)=super::write::arguments(&own_record(node,b"loda")?.data)?;let args=args.into_iter().collect::<BTreeMap<_,_>>();
        let modern_style=if modern {Some(super::modern::style_record(args.get(&201).ok_or_else(||unsupported("native21 object style missing"))?)?)}else{None};
        let is_clip=!group&&super::clipping::reference(node)?.is_some();let native_key=super::clipping::key(node)?;let path_flags=path_links.flags(native_key,kind)?;
        let flags=flags(node)?;let alpha=args.get(&8000).map(|v|super::opacity::read_effect(v,&fills,modern)).transpose()?;
        if let Some(effect)=alpha{if group&&effect.scope!=2{return Err(unsupported("per-paint transparency cannot be assigned to a group"));}if let Some(v)=&modern_style{super::opacity::check_typed_scope(v,effect)?;}}
        if if group {flags&0x80000!=0||alpha.is_some()&&!modern}else{(flags&0x80000!=0)!=alpha.is_some()||alpha.is_some()&&is_clip} {return Err(unsupported("native transparency flag/owner requires composition support"));}
        if flags&5!=path_flags{return Err(unsupported("native path-role flags and relation table disagree"));}
        // SaveAs14 writes 16016=1 for the already-hidden non-group objects.
        // Do not accept another value, or the marker on a visible object.
        let hidden_marker=|k:u32,v:&[u8]|k==16016&&!group&&flags&0x100!=0&&v==[1,0,0,0];
        if args.iter().any(|(k,v)|!(if group {[100,1000,12000,40000,40001].contains(k)||modern&&*k==8000}else{[10,20,30,100,1000,8000,16015,40000,40001].contains(k)||kind==3&&*k==14001||is_clip&&[8005,40200].contains(k)}) && !(modern&&*k==201) && !hidden_marker(*k,v) && !((*k==12000&&v.len()==4 || *k==40040&&v.len()==16)&&v.iter().all(|b|*b==0))) {return Err(unsupported("additional native object/group effects/arguments are not imported"));}
        if group && (kind!=0 || args.get(&12000).is_some_and(|v|v.len()!=4||v.iter().any(|b|*b!=0))) {return Err(unsupported("native group effects are not represented"));}
        // Accept only the regular-object flag combinations in the native
        // reference set; unknown visibility/lock/effect flags fail explicitly.
        // Actual identity-v14-edited SaveAs14 adds 0x40 without changing own state.
        // Verified Corel uniform artistic text gains bit 0x01000000 on save.
        // Its frame, typography and contents are still validated by artistic().
        let text_flag=if !group&&matches!(kind,4|6) {0x01000000}else{0};
        // Actual122 Corel-created affine reference groups carry10000880.
        // Admit this complete measured group profile, not arbitrary flag bits.
        if is_clip!=(flags&0x100000!=0) || !(if group {[0x10000080,0x10000000,0x10000880].contains(&(flags&!0x150))}else{[0x08000280,0x08000a80,0x08000200,0x08000a00].contains(&(flags&!(0x100000|0x80000|0x150|text_flag|path_flags)))}) {return Err(unsupported(format!("native object flags 0x{flags:x} require preservation support")));}
        let name=args.get(&1000).map(|v|read_wide(v)).transpose()?.unwrap_or_default();let mut id=name.clone();
        if !valid_id(&id) || ids.contains(&id) {id=format!("cdr-object-{}",index+1);while ids.contains(&id){id.push('_');}renamed=true;}ids.insert(id.clone());path_ids.insert(native_key,id.clone());
        if let Some(record)=optional_own_record(node,b"usdn")?{let sid=u32_at(&record.data,0)?;if !native_static_seen.insert(sid){if bound_static_ids.contains(&sid){return Err(invalid("duplicate native StaticID makes object-data ownership ambiguous"));}unbound_static_alias=true;}native_static_ids.insert(id.clone(),sid);}
        if is_clip {clip_frames.push(id.clone());}
        let parent_id=parent_index.map(|i|doc.objects[page_start+i].id.clone());
        // Corel omits the bookkeeping transform of an ordinary, untouched
        // group. Member transforms remain required and already use page space.
        // Present group transforms must still pass the existing validation.
        let native_matrix=if group && optional_own_record(node,b"trfd")?.is_none() {[1.,0.,0.,0.,1.,0.]}else{matrix(node)?};
        if group {
            group_matrices.insert(index,native_matrix);
            let group_style=Style{opacity:alpha.map_or(1.,|a|a.opacity),..Style::default()};
            doc.objects.push(Object{id,page_id:owner.clone(),parent_id,x_mm:0.,y_mm:0.,width_mm:0.,height_mm:0.,transform:[1.,0.,0.,1.,0.,0.],style:if let Some(v)=&modern_style {super::modern::geometry_style(v,group_style)?}else{group_style},visible:flags&0x100==0,locked:flags&0x10!=0,clip:None,geometry:Geometry::Group});continue;
        }
        let nib=args.get(&10).map(|v|u32_at(v,0)).transpose()?.filter(|key|*key!=0).map(|key|super::pen::read(lines.get(&key).ok_or_else(||invalid("native nib outline missing"))?)).transpose()?.flatten();
        let [a,b,tx,c,d,ty]=native_matrix;let mut transform=[a,-c,-b,d,(tx+if modern {coord(ox)? as f64}else{ox*10000.})/10000.,(if modern {coord(oy)? as f64}else{oy*10000.}-ty)/10000.];let mut style=style(&args,&fills,&lines)?;style.opacity=alpha.map_or(1.,|a|a.opacity);if let Some(v)=&modern_style {
            let adjusted=if matches!(kind,1|2|3){args.get(&10).map(|v|u32_at(v,0)).transpose()?.filter(|key|*key!=0).and_then(|key|lines.get(&key)).map(|record|if nib.is_some(){super::pen::typed_style(v,record,native_matrix)}else{super::primitive_frames::local_style(v,record,native_matrix,parent_index.and_then(|i|group_matrices.get(&(page_start+i)).copied()))}).transpose()?.flatten()}else{None};
            if kind==1&&nib.is_some()&&adjusted.is_none(){return Err(unsupported("native independent rectangle pen requires its measured typed ellipse profile"));}
            if adjusted.is_some(){scaled_widths+=1;}style=super::modern::geometry_style(adjusted.as_ref().unwrap_or(v),style)?;
        }
        if let Some(record)=args.get(&10).and_then(|v|u32_at(v,0).ok()).and_then(|key|lines.get(&key)){
            if u16_at(record,outline_body(record)?)?==34&&(!modern||modern_style.as_ref().and_then(|v|v.pointer("/outline/scaleWithObject")).and_then(serde_json::Value::as_str)==Some("1")){scaled_outlines.insert(id.clone());}
        }
        let g=args.get(&30).ok_or_else(||invalid("native geometry argument missing"))?;let width;let height;let geometry;let mut area_tracking=None;
        match kind {
            1|2=>{
                let modern_rect=if modern&&kind==1 {Some(super::modern::rectangle(g)?)}else{None};
                let (native_width,native_height)=if let Some((w,h,_))=modern_rect {(w,h)}else{(i32_at(g,0)? as f64/10000.,i32_at(g,4)? as f64/10000.)};
                if native_width==0. || native_height==0. {return Err(unsupported("zero native primitive dimensions are not imported"));}
                // Corel SaveAs14 can express reflected primitives with signed
                // local dimensions. Keep their native origin and move each
                // sign into its affine axis instead of dropping the reflection.
                width=native_width.abs();height=native_height.abs();
                let radius=if let Some((_,_,r))=modern_rect {r}else if kind==1 {
                    let values=[8,12,16,20].map(|at|i32_at(g,at)).into_iter().collect::<Result<Vec<_>>>()?;
                    if values[0]<0 || values.iter().any(|v|*v!=values[0]) {return Err(unsupported("independent or negative rectangle corner radii are not represented"));}
                    values[0] as f64/10000.
                }else{0.};
                if kind==2 && (u32_at(g,8)?!=u32_at(g,12)? || u32_at(g,16)?!=0) {return Err(unsupported("native arc/pie remains unsupported"));}
                transform[0]*=native_width.signum();transform[1]*=native_width.signum();
                transform[2]*=native_height.signum();transform[3]*=native_height.signum();
                transform[4]+=b*native_height;transform[5]-=d*native_height;geometry=if kind==1 {Geometry::Rect{radius_mm:radius}}else{Geometry::Ellipse};
            },
            3=>{let path=curve(g)?;let bounds=path.bounding_box();width=bounds.width().max(0.0001);height=bounds.height().max(0.0001);
                let equivalent_nib_fill=nib.is_some()&&!args.contains_key(&14001)&&modern_style.as_ref().and_then(|v|v.pointer("/fill/winding")).and_then(serde_json::Value::as_str)==Some("1")&&super::pen::equivalent_fill(&path);
                geometry=Geometry::Path{d:path.to_svg(),fill_rule:fill_rule(&args,if equivalent_nib_fill{None}else{modern_style.as_ref()})?};},
            4|6 if modern=>{
                let postscript_names=postscript.iter().filter_map(|id|fonts.get(id).cloned()).collect();
                let declared_names=fonts.values().cloned().collect();let t=super::modern::text(&node.find(b"txsm")?.data,kind==6,modern_style.as_ref().unwrap(),&style,&postscript_names,&declared_names)?;
                if kind==6 {super::typography::check_frame_matrix(t.matrix,native_matrix)?;}else if t.path.is_some(){if let Some(k)=super::path_shear::frame(t.matrix,native_matrix,t.path.as_ref().unwrap().orientation)?{path_shears.insert(native_key,k);}}else if t.matrix.iter().zip(native_matrix).any(|(a,b)|(a-b).abs()>0.000001){return Err(unsupported("native21 independent text-frame matrix unsupported"));}
                if let Some(p)=t.path.clone(){path_options.insert(native_key,p);}
                if kind==6 {
                    (width,height)=super::typography::dimensions21(g,t.frame.ok_or_else(||invalid("native21 area frame absent"))?,t.matrix,native_matrix,args.get(&100).map(|v|u32_at(v,0)).transpose()?)?;
                    if t.column_width.is_some_and(|v|coord(width).ok()!=Some(v)) {return Err(unsupported("native21 local column width differs from area geometry"));}
                }else{
                    transform[4]-=transform[2]*t.size;transform[5]-=transform[3]*t.size;let bbox=&node.find(b"bbox")?.data;width=((i32_at(bbox,8)? as f64-i32_at(bbox,0)? as f64)/10000.).abs().max(0.0001);height=((i32_at(bbox,4)? as f64-i32_at(bbox,12)? as f64)/10000.).abs().max(0.0001);
                }
                style=t.style;area_tracking=Some(t.tracking);geometry=Geometry::Text{content:t.content,font_family:t.font,font_size_mm:t.size,text_path:None,layout:t.layout};
            },
            4|6=>{
                let (content,s,path)=artistic(&node.find(b"txsm")?.data,kind==6,&styles,&fills,&lines,&postscript)?;if let Some(p)=path {let story=&node.find(b"txsm")?.data;let mut matrix=[0.;6];for(i,v)in matrix.iter_mut().enumerate(){*v=f64_at(story,44+i*8)?;}if let Some(k)=super::path_shear::frame(matrix,native_matrix,p.orientation)?{path_shears.insert(native_key,k);}path_options.insert(native_key,p);}
                let font=fonts.get(&s.font).ok_or_else(||unsupported("native text font is unresolved"))?.clone();if s.size<=0. {return Err(invalid("native text size is invalid"));}
                let mut args2=args.clone();args2.insert(20,s.fill.to_le_bytes().to_vec());args2.insert(10,s.line.to_le_bytes().to_vec());style=style_for_text(&args2,&fills,&lines)?;style.opacity=alpha.map_or(1.,|a|a.opacity);
                let layout=if kind==6 {let (w,h)=super::typography::dimensions(g,&node.find(b"txsm")?.data,native_matrix,args.get(&100).map(|v|u32_at(v,0)).transpose()?)?;width=w;height=h;let (align,leading,tracking)=s.area.ok_or_else(||unsupported("unresolved paragraph layout"))?;area_tracking=Some(tracking);TextLayoutOptions{mode:TextMode::Area,align,line_height_mm:Some(leading),letter_spacing_mm:0.}}else{
                    transform[4]-=transform[2]*s.size;transform[5]-=transform[3]*s.size;let (leading,tracking)=s.point.ok_or_else(||unsupported("unresolved point paragraph layout"))?;area_tracking=Some(tracking);let bbox=&node.find(b"bbox")?.data;width=((i32_at(bbox,8)? as f64-i32_at(bbox,0)? as f64)/10000.).abs().max(0.0001);height=((i32_at(bbox,4)? as f64-i32_at(bbox,12)? as f64)/10000.).abs().max(0.0001);TextLayoutOptions{mode:TextMode::Point,align:match s.align{2=>TextAlign::Center,3=>TextAlign::Right,_=>TextAlign::Left},line_height_mm:leading,letter_spacing_mm:0.}
                };
                geometry=Geometry::Text{content,font_family:font,font_size_mm:s.size,text_path:None,layout};
            },
            5=>{
                let rid=u32_at(g,48)?;let (resource_id,_)=images.get(&rid).ok_or_else(||invalid("native bitmap reference missing"))?;let x1=i32_at(g,0)? as f64/10000.;let y1=i32_at(g,4)? as f64/10000.;let x2=i32_at(g,8)? as f64/10000.;let y2=i32_at(g,12)? as f64/10000.;
                image_object_keys.insert(id.clone(),rid);
                if x1!=0. || y2!=0. || x2<=0. || y1<=0. {return Err(unsupported("native image crop/placement is unsupported"));}width=x2;height=y1;transform[4]+=b*height;transform[5]-=d*height;
                // The five native clip points must describe the full bitmap,
                // otherwise accepting this object would silently drop a crop.
                let expected=[0.,height,width,height,width,0.,0.,0.,0.,height];for (i,v) in expected.iter().enumerate(){if i32_at(g,76+4*i)?!=coord(*v)? {return Err(unsupported("native image clipping is unsupported"));}}
                geometry=Geometry::Image{resource_id:resource_id.clone(),clip_mm:None,fit:ImageFit::Stretch};
            },
            _=>return Err(unsupported(format!("native object kind {kind} is not imported"))),
        }
        // Resolve actual character paints first: both text readers replace
        // the object style. Applying a scoped effect earlier loses its alpha.
        if let Some(effect)=alpha{if effect.scope!=2&&!matches!(kind,1|2|3|4|6){return Err(unsupported("native scoped paint alpha requires a vector or text story"));}super::opacity::apply_part(&mut style,effect)?;}
        let mut object=Object{id,page_id:owner.clone(),parent_id,x_mm:0.,y_mm:0.,width_mm:width,height_mm:height,transform,style,visible:flags&0x100==0,locked:flags&0x10!=0,clip:None,geometry};
        if matches!(kind,1|2|3)&&object.style.stroke!="none"{if let Some(outline)=args.get(&10).and_then(|v|u32_at(v,0).ok()).and_then(|key|lines.get(&key)){
            object.style.stroke_scale_with_object=u16_at(outline,outline_body(outline)?)?&32!=0;
            if let Some(mode)=modern_style.as_ref().and_then(|v|v.pointer("/outline/scaleWithObject")).and_then(serde_json::Value::as_str){if (mode=="1")!=object.style.stroke_scale_with_object{return Err(unsupported("native typed and effective outline scaling modes disagree"));}}
        }}
        if let Some(nib)=nib{if !nib.scale_with_object {
            if !matches!(kind,1|2|3){return Err(unsupported("fixed elliptical nibs require vector geometry"));}
            let outline=args.get(&10).map(|v|u32_at(v,0)).transpose()?.and_then(|key|lines.get(&key)).ok_or_else(||invalid("fixed nib outline missing"))?;let at=outline_body(outline)?;
            object.style.stroke_scale_with_object=false;object.style.stroke_fixed_nib=Some(crate::FixedStrokeNib{stretch_percent:u16_at(outline,at+10)?,angle_millionth_degrees:i32_at(outline,at+12)?});
        }else{let[u,v,w,z,_,_]=nib.basis.as_coeffs();let(sx,sy)=(u.hypot(w),v.hypot(z));let minor=sx.min(sy);nib.apply(&mut object)?;let m=object.transform;let nominal=object.style.stroke_width_mm*m[0].hypot(m[1]).max(m[2].hypot(m[3]));nib_objects.insert(object.id.clone(),super::local_frames::NativeNib{nominal,raw:nib.width,grid_scale:[sx/minor,sy/minor]});nib_recharts+=1;}}

        if let Some(tracking)=area_tracking {if tracking!=0 {let spacing=super::typography::space_width(&doc,&object)?*tracking as f64/1_000_000.;if let Geometry::Text{layout,..}=&mut object.geometry {layout.letter_spacing_mm=spacing;}}}
        let shift=super::typography::point_anchor_shift(&doc,&object)?;
        object.transform[4]-=object.transform[0]*shift;object.transform[5]-=object.transform[1]*shift;
        doc.objects.push(object);
    }
    }
    links.finish()?;path_links.bind(&mut doc,&path_ids,&path_options)?;let path_shears=path_shears.into_iter().map(|(key,k)|path_ids.get(&key).cloned().map(|id|(id,k)).ok_or_else(||invalid("native shear identity missing"))).collect::<Result<BTreeMap<_,_>>>()?;super::path_shear::validate_imports(&doc,&path_shears)?;super::geometry_params::capture_native_clips(&doc,&clip_frames,geometry_params.as_mut())?;super::clipping::restore(&mut doc,&clip_frames)?;doc.resources=resources;let originals=if let Some(vault)=vault {super::resource_vault::restore(&mut doc,vault,&images,&image_object_keys,&native_cmyk,&native_gray16,&native_rgb16)?}else{if !native_cmyk.is_empty(){return Err(unsupported("standalone native CMYK requires lossless resource support; original four-channel catalogue missing"));}super::resources::restore(&mut doc,&tree,modern,&images,&image_object_keys)?};let legacy_paint=super::paint_params::restore(&mut doc,&tree,modern)?;let active_widths=paint_notes.as_ref().map(super::paint_params::active_widths).transpose()?.unwrap_or_default();let paint_notes=if let Some(notes)=paint_notes{super::paint_params::restore_notes(&mut doc,notes,modern)?}else{(0,0)};let paint_params=legacy_paint+paint_notes.0;super::group_params::restore(&mut doc,&tree,modern)?;let path_charts=super::path_text::restore_parent_charts(&mut doc,frames.as_ref())?;if let Some(frames)=&mut frames{let declared=text_params.as_ref().map(super::text_params::quarter_names).unwrap_or_default();super::local_frames::suppress_unbound_quarter_turns(&doc,frames,&declared);super::local_frames::suppress_source_shears(&doc,frames);}let mut discarded_path_placements=BTreeSet::new();let text_parameters=if let Some(params)=text_params{super::text_params::restore_with_placements(&mut doc,params,&path_charts,&mut discarded_path_placements)?}else{(0,0)};if let Some(frames)=&mut frames{super::local_frames::suppress_path_placements(frames,&discarded_path_placements);}let path_units=super::geometry_params::restore_path_units(&mut doc,geometry_params.as_mut(),&scaled_outlines,&active_widths)?;if let Some(frames)=&mut frames{super::paint_layers::reconcile_charts(&doc,frames,&mut layer_proof,paint_layers.as_ref(),&active_widths,&super::geometry_params::restored_units(geometry_params.as_ref()))?;}let frame_normalized=if let Some(frames)=frames{super::local_frames::restore_with_nibs(&mut doc,frames,&active_widths,&super::geometry_params::restored_units(geometry_params.as_ref()),&nib_objects)?.1}else{0};doc.validate()?;
    let image_placement=if let Some(params)=image_params{super::image_params::restore(&mut doc,params)?}else{(0,0)};
    let geometry_parameters=if let Some(params)=geometry_params{super::geometry_params::restore(&mut doc,params)?}else{(0,0)};
    let source_interleaving=super::native_order::restore(&mut doc,&native_static_ids,&source_names,&source_indices);
    let notation_count=if let Some(notation)=paint_notation{super::paint_notation::restore(&mut doc,notation)?}else{0};
    let layer_counts=if let Some(layers)=paint_layers{super::paint_layers::restore_with_proof(&mut doc,layers,layer_proof)?}else{(0,0)};
    super::path_shear::restore(&mut doc,&path_shears)?;doc.validate()?;let cyclic_restored=if let Some(contract)=cyclic_source{Some(super::cyclic_source::restore(&mut doc,&decode_geometry(data,document_id)?.0,contract,modern)?)}else{None};
    let mut warnings=vec![Notice::new("CDR_EXPERIMENTAL",&format!("Experimental CDR{} interchange import: supported native objects reconstructed as editable Arranger objects. Save to .arranger; full CDR round-trip is not available. Text layout and font substitution may differ; native metadata and ruler origins are not restored.",if modern {21}else{14}))];
    if embedded_fonts {
        warnings.push(Notice::new("CDR_EMBEDDED_FONTS", "Corel declares embedded fonts in this document. Arranger uses installed fonts for editing. Saving an Arranger document or exporting a new CDR does not preserve the embedded font programs. Keep the original CDR."));
    }
    if let Some((restored,retained))=cyclic_restored{if restored>0{warnings.push(Notice::new("CDR_CYCLIC_SOURCE",&format!("Recovered {restored} experimental cyclic Own/start charts after native endpoint, ancestor and complete control validation; no geometry backup.")));}if retained>0{warnings.push(Notice::new("CDR_CYCLIC_SOURCE_NATIVE",&format!("Retained {retained} edited native story/guide charts without reapplying old Own/start values.")));}}
    if layer_counts.0>0{warnings.push(Notice::new("CDR_PAINT_LAYERS","Matched native fill/outline layer pairs restored as single editable objects after checking geometry, paints, hierarchy and stacking. Binding Notes contain identities and native geometry digests only."));}
    if layer_counts.1>0{warnings.push(Notice::new("CDR_PAINT_LAYERS_NATIVE","Changed, missing or reordered native paint layers retained as actual editable objects; no old geometry or paint was resurrected."));}
    if unbound_static_alias{warnings.push(Notice::new("CDR_STATICID_UNBOUND","Repeated native StaticIDs have no Arranger restoration-field owner binding in this file. Editable names and Arranger identities remain distinct; native StaticID identity is not reconstructed. Bound duplicate owners are rejected."));}
    if doc.objects.iter().any(|o|matches!(&o.geometry,Geometry::Text{..})) {warnings.push(Notice::new("CDR_TEXT_LAYOUT","Editable point/rectangular area paragraphs and blank lines imported with native tracking and explicit baseline spacing. Default relative artistic leading is normalized to this host model default and may reflow differently. Native tracking percentages are converted using this host resolved font space advance; text reflows with installed fonts. Linked/multicolumn frames and advanced paragraph formatting remain unsupported."));}
    if scaled_widths>0{warnings.push(Notice::new("CDR_SCALED_OUTLINE_WIDTH","Measured Corel21 orthogonal scaled-vector profiles compare the typed world width, angle and aspect with the actual local binary pen in one physical budget. The effective local width is not scaled twice; conflicting pen profiles still reject."));}
    if doc.objects.iter().any(|o|!o.style.stroke_scale_with_object&&o.style.stroke!="none"&&matches!(o.geometry,Geometry::Rect{..}|Geometry::Ellipse|Geometry::Path{..})){warnings.push(Notice::new("CDR_FIXED_WORLD_STROKE","Native fixed vector outlines retain their page-millimetre width under object and parent transforms, including nonorthogonal frames. Body geometry and primitive type are retained."));}
    if nib_recharts>0{warnings.push(Notice::new("CDR_NIB_RECHARTED","Effective binary calligraphic nib width, stretch and angle were retained in a real curve or measured rectangle coordinate chart. Native rectangles keep their primitive type. CDR21 may retain a stale nominal typed width after native normalization; it does not replace the effective binary nib. Primitive identity already lost by Corel is not recreated. Native visual validation and new export validation remain pending."));}
    if !path_options.is_empty(){warnings.push(Notice::new("CDR_PATH_LAYOUT","Native single-line text/guide relations, offsets and anchors imported as editable bindings. Derived Corel glyph caches are re-laid out by this host; installed fonts and layout algorithms may differ. Export uses the bounded no-cache native relation profile; new model outputs require Corel validation."));}
    if notation_count>0{warnings.push(Notice::new("CDR_PAINT_NOTATION","Hex digit counts and letter-case masks restored without source RGB/alpha channel backups. Actual native colors, paint removal and color-model edits remain authoritative."));}
    warnings.extend(crate::paint::warnings(&doc));
    if pages_assigned {warnings.push(Notice::new("CDR_PAGES_ASSIGNED","Drawing-page dimensions, order and ownership imported. Missing, duplicate or invalid native page names received stable Arranger page IDs in file order."));}
    if doc.objects.iter().any(|o|o.page_id==crate::WORKSPACE_PAGE) {warnings.push(Notice::new("CDR_WORKSPACE","Native document-wide desktop objects imported into the shared, nonprinting workspace. Specialized master layers remain unsupported."));}
    if doc.objects.iter().any(|o|matches!(o.geometry,Geometry::Group)) {warnings.push(Notice::new("CDR_GROUP_NORMALIZED","Native group hierarchy imported with members in page coordinates; original local placement/frame and group metadata are not reconstructed. Native group bookkeeping matrices are not applied twice."));}
    if renamed {warnings.push(Notice::new("CDR_IDS_ASSIGNED","Missing, duplicate or non-Arranger object names received new stable IDs for this imported document."));}
    if !clip_frames.is_empty(){warnings.push(Notice::new("CDR_CLIP_NORMALIZED","PowerClip contents and closed frame paths imported as editable objects and clips. Native painted frames become separate fill/outline objects around a clipped content group; original frame edit linkage is not reconstructed. Empty single-object Arranger frames are collapsed into object-local clips."));}
    if paint_params>0 {warnings.push(Notice::new("CDR_PAINT_PARAMETERS","Original paint alpha, exact opacity and inactive outline scalars restored from bounded Notes after validating native single-paint effects. Legacy ObjectData remains readable."));}
    if paint_notes.1>0 {warnings.push(Notice::new("CDR_PAINT_PARAMETERS_NATIVE","Native paint mode or transparency changed; retained native editable values instead of old alpha/opacity decomposition."));}
    if !active_widths.is_empty(){warnings.push(Notice::new("CDR_OUTLINE_PRECISION","Bounded rectangle/ellipse/path outline-width scalars were checked together with native geometry and frames under both affine and fixed-world pen bounds. Changed native widths, zero-width projections and candidates outside the shared world-displacement budget retain actual native values."));}
    if !native_cmyk.is_empty(){warnings.push(Notice::new("CDR_CMYK_PIXELS","Native CMYK8 channels were verified exactly against their original JPEG resource. Original container and channels are preserved; screen preview is unmanaged RGB, ICC/print behavior remains unverified."));}
    if !native_gray16.is_empty(){warnings.push(Notice::new("CDR_GRAY16_PIXELS","Native opaque grayscale16 samples were retained exactly in16-bit PNG resources and checked against bound originals. Screen/PNG previews remain8-bit; no managed color/print guarantee."));}
    if !native_rgb16.is_empty(){warnings.push(Notice::new("CDR_RGB16_PIXELS","Native opaque RGB48 samples were retained exactly in16-bit RGB PNG resources and checked against bound originals. Screen/PNG previews remain8-bit; no managed color/print guarantee."));}
    if originals>0||has_vault {warnings.push(Notice::new("CDR_RESOURCE_ORIGINAL","Original image containers, IDs and sharing restored after checksum/MIME/native RGBA validation. Document resource Notes also retain unreferenced originals; legacy object attachments remain readable. Native pixel edits with stale sources are rejected; no full-format compatibility claim."));}
    if doc.objects.iter().filter(|o|matches!(o.geometry,Geometry::Image{..})).count()>originals {warnings.push(Notice::new("CDR_IMAGE_PIXELS","Native bitmaps without original attachments imported as shared PNG resources; their source PNG/JPEG containers are unavailable. Unreferenced originals in Notes are retained."));}
    if frame_normalized>0 {warnings.push(Notice::new("CDR_LOCAL_FRAME_NORMALIZED",&format!("Retained {frame_normalized} native frames because reusing old dimensions could change corner radii, stroke units or area-text reflow. Original local frame fidelity is not claimed for these objects.")));}
    if text_parameters.0>0 {warnings.push(Notice::new("CDR_TEXT_PARAMETERS","Source font aliases, exact size/tracking and optional leading restored only after verifying their native scalar projection. Native story content, mode, alignment and path bindings remain authoritative."));}
    if text_parameters.1>0 {warnings.push(Notice::new("CDR_TEXT_PARAMETERS_NATIVE","Native text edits differ from the old scalar projection; retained the changed native font/layout values. Original text parameter fidelity is not claimed for these objects."));}
    if image_placement.0>0{warnings.push(Notice::new("CDR_IMAGE_PLACEMENT","Source image fit, viewport and legacy crop restored after validating native bitmap placement and crop masks. Generated single-image wrappers folded; actual image moves and native custom masks retained."));}
    if image_placement.1>0{warnings.push(Notice::new("CDR_IMAGE_PLACEMENT_NATIVE","Native image crop, outline or generated-wrapper structure changed; retained its editable native placement instead of restoring old fit scalars."));}
    if path_units.1>0{warnings.push(Notice::new("CDR_PATH_UNITS_NATIVE","Native path identity, type, active outline or text binding prevents a safe unit rebase; retained actual native coordinate units and skipped old unit-dependent notation."));}
    if geometry_parameters.0>0{warnings.push(Notice::new("CDR_GEOMETRY_PARAMETERS","Original path notation and bounded rectangular clip scalars restored from validated native coordinates. Notes contain formatting and projection hashes, never original curve coordinates."));}
    if geometry_parameters.1>0{warnings.push(Notice::new("CDR_GEOMETRY_PARAMETERS_NATIVE","Native curve or clip edits differ from old scalar/notation projections; retained native geometry."));}
    if source_interleaving{warnings.push(Notice::new("CDR_NATIVE_INTERLEAVING","Cross-owner source serialization interleaving recovered from scalar source indices (legacy files use native StaticIDs) after proving unchanged sibling order for every current native owner/parent. New unbound objects or stacking edits prevent restoration; owners and hierarchy stay native."));}
    Ok((doc,warnings))
}
fn style_for_text(args:&BTreeMap<u32,Vec<u8>>,fills:&BTreeMap<u32,&[u8]>,lines:&BTreeMap<u32,&[u8]>)->Result<Style>{
    if let Some(record)=args.get(&10).map(|v|u32_at(v,0)).transpose()?.filter(|key|*key!=0).and_then(|key|lines.get(&key)){if super::pen::read(record)?.is_some(){return Err(unsupported("calligraphic live text nib preservation is not implemented"));}}
    style(args,fills,lines)
}

#[cfg(test)]
mod native_fill_modes {
    use super::*;
    #[test]
    fn all16_sdk_bound_ordinary_and_powerclip_rules_match_real_native_arguments() {
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports/windows/WIN-SAVE-CLOSURE-20261024-01/fill-rules");
        for version in [14,21] {for clip in ["","-clip"] {for (name,expected) in [("nonzero",crate::FillRule::NonZero),("evenodd",crate::FillRule::EvenOdd)] {for edit in ["","-edited"] {
            let file=root.join(format!("fill{clip}-{name}-v{version}{edit}.cdr"));let native=read_native(&std::fs::read(&file).unwrap()).unwrap();
            let node=all(&native.nodes).into_iter().find(|n|n.kind()==Some(b"obj ")&&super::super::write::arguments(&own_record(n,b"loda").unwrap().data).unwrap().0==3).unwrap();
            let (_,args)=super::super::write::arguments(&own_record(node,b"loda").unwrap().data).unwrap();let args=args.into_iter().collect::<BTreeMap<_,_>>();
            let style=args.get(&201).map(|v|super::super::modern::style_record(v).unwrap());
            assert_eq!(fill_rule(&args,style.as_ref()).unwrap(),expected,"{}",file.display());
            let mut bad=args.clone();for marker in [vec![],vec![0,0,0,0],vec![2,0,0,0],vec![1,0,0,0,0]] {bad.insert(14001,marker);assert!(fill_rule(&bad,style.as_ref()).is_err());}
        }}}}
    }
    #[test]
    fn json_only_nonzero_without_the_actual_native_mode_is_rejected() {
        assert!(fill_rule(&BTreeMap::new(),Some(&serde_json::json!({"fill":{"winding":"1"}}))).is_err());
        assert!(fill_rule(&BTreeMap::from([(14001,vec![1,0,0,0])]),Some(&serde_json::json!({"fill":{"winding":"0"}}))).is_err());
    }
}

#[cfg(test)]
mod native_identity {
    use super::*;
    #[test]
    fn duplicate_bound_static_ids_across_objects_and_pages_reject_ambiguous_ownership(){
        for cross_page in [false,true]{let mut doc=crate::Document::new("ids".into(),210.,297.).unwrap();if cross_page{let mut page=doc.pages[0].clone();page.id="second".into();doc.pages.push(page);}
            for id in ["one","two"]{doc.objects.push(serde_json::from_value(serde_json::json!({"id":id,"page_id":if cross_page&&id=="two"{"second"}else{"page-1"},"x_mm":20.,"y_mm":30.,"width_mm":40.,"height_mm":20.,"geometry":{"kind":"rect","radius_mm":0.},"style":{"fill":"#12345680","stroke":"none","stroke_width_mm":0.,"opacity":0.7}})).unwrap());}
            for encode in [crate::cdr::encode_document,crate::cdr::encode_document21]{let bytes=encode(&doc).unwrap().0;let mut native=read_native(&bytes).unwrap();assert_eq!(super::super::paint_params::attach(&mut native.nodes,&doc).unwrap(),2);if native.version==2100{fn upgrade(ns:&mut [Node]){for n in ns{if n.tag==*b"udfe"{if let Ok(name)=super::super::local_data::field_name(&n.data,false){if name.starts_with("Arranger_"){n.data=super::super::local_data::definition(&super::super::local_data::template(true).unwrap(),&name,u32_at(&n.data,0).unwrap(),u16_at(&n.data,6).unwrap(),true).unwrap();}}}if let Some(c)=&mut n.children{upgrade(c);}}}upgrade(&mut native.nodes);}fn repeat(ns:&mut [Node],first:&mut Option<Vec<u8>>){for n in ns{if n.kind()==Some(b"obj "){let sid=n.find_mut(b"usdn").unwrap();if let Some(value)=first{sid.data=value.clone();}else{*first=Some(sid.data.clone());}}if let Some(c)=&mut n.children{repeat(c,first);}}}repeat(&mut native.nodes,&mut None);let bytes=if native.version==1400{super::super::binary::write(&native.nodes).unwrap()}else{super::super::binary::repack21(&native).unwrap()};let err=crate::cdr::decode(&bytes,"ids").unwrap_err();assert_eq!(err.code,"INVALID_CDR");assert!(err.message.contains("duplicate native StaticID"),"{err:?}");}
        }
    }
}
