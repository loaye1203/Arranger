use super::{binary::*, invalid, unsupported, Result};
use crate::{Document, Geometry, ImageFit, LineCap, LineJoin, TextMode, command::Notice};
use base64::{Engine as _,engine::general_purpose::STANDARD};
use kurbo::{Affine, BezPath, PathEl, Point, Rect, Shape};
use std::collections::BTreeMap;

const GEOMETRY:&[u8]=include_bytes!("../../../../examples/assets/cdr/corel2019/geometry-profile-v14.cdr");
const GROUP:&[u8]=include_bytes!("../../../../examples/assets/cdr/corel2019/group-profile-v14.cdr");
pub(super) const CONTENT:&[u8]=include_bytes!("../../../../examples/assets/cdr/corel2019/content-profile-v14.cdr");

pub(super) fn selected(doc:&Document,page_id:&str)->Result<Document> {
    doc.validate()?;let page=doc.page(page_id)?.clone();
    let mut selected=doc.clone();selected.pages=vec![page];
    selected.objects.retain(|o|o.page_id==page_id);
    // Resources remain shared in memory; only referenced pixels are emitted.
    Ok(selected)
}
pub(super) fn normalized(doc:&Document)->Result<Document> {
    let mut result=normalized_without_guide_charts(doc)?;super::path_guide::lower(doc,&mut result)?;Ok(result)
}
pub(super) fn normalized_without_guide_charts(doc:&Document)->Result<Document> {
    doc.validate()?;let mut result=doc.clone();
    for (old,new) in doc.objects.iter().zip(&mut result.objects) {
        let mut current=old;let mut depth=0;
        while let Some(parent)=&current.parent_id {depth+=1;if depth>16 {return Err(unsupported("CDR14 group nesting currently limited to 16 levels"));}current=doc.object(parent)?;}
        if matches!(old.geometry,Geometry::Group) {
            if (old.transform[0]*old.transform[3]-old.transform[1]*old.transform[2]).abs()<1e-12 {return Err(unsupported("singular group transform is unsupported"));}
            new.x_mm=0.;new.y_mm=0.;new.transform=[1.,0.,0.,1.,0.,0.];
            new.style=crate::Style{opacity:old.style.opacity,..Default::default()};
            if let Some(path)=super::clipping::world_clip(doc,old)? {new.clip=Some(crate::Clip::Path{d:path.to_svg(),fill_rule:old.clip.as_ref().unwrap().rule()});}
        } else if old.parent_id.is_some() {
            // CDR14 group containers do not transform members (libcdr's
            // considerGroupTransform is false for version >=400). Members use
            // page coordinates. Keep hierarchy and canonicalize their placement.
            new.transform=doc.world_transform(old)?.as_coeffs();new.x_mm=0.;new.y_mm=0.;
        }
        if (matches!(new.geometry,Geometry::Rect{radius_mm} if radius_mm>0.)||!new.style.stroke_scale_with_object&&matches!(new.geometry,Geometry::Rect{..}|Geometry::Ellipse)) && new.clip.is_none() {
            let m=Affine::new(new.transform)*Affine::translate((new.x_mm,new.y_mm));
            if m.determinant()<0. {
                // A uniformly rounded rectangle is symmetric under x->w-x.
                // Use the equivalent positive-orientation local frame; Corel's
                // reflected native rectangle reports a lost first corner.
                new.transform=(m*Affine::new([-1.,0.,0.,1.,new.width_mm,0.])).as_coeffs();new.x_mm=0.;new.y_mm=0.;
            }
        }
    }
    for (old,new) in doc.objects.iter().zip(&mut result.objects){if matches!(old.geometry,Geometry::Text{text_path:Some(_),..}){*new=super::path_text::native_object(doc,old)?;}}
    super::images::normalize(doc,&mut result)?;
    for object in &mut result.objects {super::paint_params::lower(object)?;}
    Ok(result)
}
fn check_object(doc:&Document,o:&crate::Object,placement_normalized:bool)->Result<()> {
    let fail=|message:&str|unsupported(format!("{}: {message}",o.id));
    super::opacity::check(o)?;
    if o.clip.is_some(){super::clip_precision::plan(doc,doc,o)?;}
    if (o.style.stroke_miterlimit*1_000_000.).round_ties_even()>u32::MAX as f64 {return Err(fail("outline miter limit exceeds native millionth-unit range"));}
    for color in [&o.style.fill,&o.style.stroke] {rgb(color)?;}
    for v in [o.x_mm,o.y_mm,o.width_mm,o.height_mm,o.style.stroke_width_mm] {coord(v)?;}
    let [a,b,c,d,_,_]=o.transform;
    if (a*d-b*c).abs()<1e-12 {return Err(fail("singular transform is unsupported"));}
    match &o.geometry {
        Geometry::Rect{radius_mm}=>{if coord(o.width_mm)?<=0||coord(o.height_mm)?<=0||2*i64::from(coord(*radius_mm)?)>i64::from(coord(o.width_mm)?.min(coord(o.height_mm)?)){return Err(fail("native rounded rectangle dimensions/radius are not a valid quantized primitive"));}},
        Geometry::Ellipse=>{if coord(o.width_mm)?<=0||coord(o.height_mm)?<=0{return Err(fail("ellipse dimensions are below native precision"));}},
        Geometry::Path{d,..}=>{
            path_geometry(&crate::path::parse(d)?)?;
        },
        Geometry::Text{content,font_family,font_size_mm,text_path,layout}=>{
            if text_path.is_some(){super::path_text::encoded_options(doc,o)?;}
            super::fonts::check(font_family)?;
            if (layout.mode==TextMode::Area&&(coord(o.width_mm)?<=0||coord(o.height_mm)?<=0))||coord(layout.line_height_mm.unwrap_or(*font_size_mm*1.2))?<=0 {return Err(fail("text dimensions or line height are below native precision"));}super::typography::tracking(doc,o)?;
            // Keep the existing bounded UTF16 payload limit. Native character
            // descriptions/style ranges count scalars; supplementary scalars
            // have one four-byte description, not two standalone surrogates.
            if content.encode_utf16().count()>65534 || content.chars().any(|c|c.is_control()&&c!='\r'&&c!='\n') {return Err(fail("text requires 0..65534 UTF16 code units with printable Unicode or explicit newlines"));}
        },
        Geometry::Image{resource_id,clip_mm,fit}=>{
            if (placement_normalized && (clip_mm.is_some() || *fit!=ImageFit::Stretch)) || o.style.stroke!="none" {return Err(fail("images currently require stretch fit without clipping or outline"));}
            let resource=&doc.resources[resource_id];let data=STANDARD.decode(&resource.data_base64).map_err(|e|invalid(e.to_string()))?;
            // Resource Notes retain the original container including ICC/EXIF.
            // Screen preview remains unmanaged RGBA8. Active CMYK8 and opaque
            // grayscale16 samples use their separately measured native model.
            super::native_pixels::source(&data,&resource.mime)?;
        },
        Geometry::Group=>{},
    }
    Ok(())
}
fn check_normalized(doc:&Document)->Result<()> {
    doc.validate()?;
    for p in &doc.pages {
        if coord(p.width_mm)?<=0 || coord(p.height_mm)?<=0 {return Err(unsupported("page dimensions are below native coordinate precision"));}
    }
    for o in &doc.objects {check_object(doc,o,true)?;}
    Ok(())
}
/// Enumerate the first concrete encoding failure of every object, rather than
/// making callers repair one shape and query again to discover the next one.
/// This is codec readiness, never a verdict on the CDR specification's capacity.
pub(super) fn blockers(doc:&Document)->Vec<serde_json::Value> {
    use serde_json::json;
    if let Err(error)=doc.validate(){return vec![json!({"scope":"document","error":error})];}
    let mut result=vec![];
    let normalized=match normalized(doc) {
        Ok(value)=>Some(value),
        Err(error)=>{result.push(json!({"scope":"normalization","error":error}));None},
    };
    let inspected=normalized.as_ref().unwrap_or(doc);
    for p in &inspected.pages {
        let checked=(||{if coord(p.width_mm)?<=0 || coord(p.height_mm)?<=0 {return Err(unsupported("page dimensions are below native coordinate precision"));}Ok(())})();
        if let Err(error)=checked {result.push(json!({"scope":"page","page_id":p.id,"error":error}));}
    }
    for o in &inspected.objects {
        if let Err(error)=check_object(inspected,o,normalized.is_some()).and_then(|_|check14_text(o)) {result.push(json!({"scope":"object","object_id":o.id,"page_id":o.page_id,"error":error}));}
    }
    result
}
fn check14_text(o:&crate::Object)->Result<()> {
    if matches!(&o.geometry,Geometry::Text{content,..} if content.chars().any(|c|c as u32>0xffff)) {
        return Err(unsupported(format!("{}: supplementary live text has no successful Corel2019 CDR14 reopen; use CDR21 or retain the complete .arranger source",o.id)));
    }Ok(())
}
pub(super) fn arguments(data:&[u8])->Result<(u32,Vec<(u32,Vec<u8>)>)> {
    let size=u32_at(data,0)? as usize;let count=u32_at(data,4)? as usize;let oa=u32_at(data,8)? as usize;let ta=u32_at(data,12)? as usize;let kind=u32_at(data,16)?;
    if size!=data.len() || count==0 || count>256 || oa.min(ta)<20 {return Err(invalid("invalid loda tables"));}
    bytes(data,oa,count*4)?;bytes(data,ta,count*4)?;
    let offsets=(0..count).map(|i|u32_at(data,oa+i*4).map(|n|n as usize)).collect::<Result<Vec<_>>>()?;
    let mut result=vec![];
    for (i,&start) in offsets.iter().enumerate() {
        let tag=u32_at(data,ta+(count-1-i)*4)?;
        if start<oa.max(ta)+count*4 || start>=size || offsets.iter().filter(|&&v|v==start).count()!=1 || result.iter().any(|(t,_)|*t==tag) {return Err(invalid("invalid loda offsets or tags"));}
        let end=offsets.iter().copied().filter(|&v|v>start).min().unwrap_or(size);result.push((tag,bytes(data,start,end-start)?.to_vec()));
    }Ok((kind,result))
}
pub(super) fn loda(kind:u32,args:&[(u32,Vec<u8>)])->Vec<u8> {
    let start=24+args.len()*8;let total=start+args.iter().map(|(_,v)|v.len()).sum::<usize>();
    let mut out=[total as u32,args.len() as u32,20,24+args.len() as u32*4,kind].into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>();
    let mut at=start;for (_,v) in args {out.extend((at as u32).to_le_bytes());at+=v.len();}out.extend((total as u32).to_le_bytes());
    for (tag,_) in args.iter().rev() {out.extend(tag.to_le_bytes());}for (_,v) in args {out.extend(v);}out
}
fn change(args:&mut [(u32,Vec<u8>)],tag:u32,data:Vec<u8>)->Result<()> {
    args.iter_mut().find(|(t,_)|*t==tag).ok_or_else(||invalid("missing template argument"))?.1=data;Ok(())
}
fn arg(args:&[(u32,Vec<u8>)],tag:u32)->Result<&[u8]> {args.iter().find(|(t,_)|*t==tag).map(|(_,v)|v.as_slice()).ok_or_else(||invalid("missing object argument"))}
fn rgb(value:&str)->Result<Option<(u16,u32)>> {match crate::paint::parse(value)? {
    crate::paint::Paint::None=>Ok(None),
    crate::paint::Paint::Rgba([r,g,b,255])=>Ok(Some((5,(u32::from(r)<<16)|(u32::from(g)<<8)|u32::from(b)))),
    crate::paint::Paint::RgbaFloat{rgb:[r,g,b],alpha:1.}=>Ok(Some((5,(u32::from(r)<<16)|(u32::from(g)<<8)|u32::from(b)))),
    crate::paint::Paint::RgbaFloat{..}=>Err(unsupported("native paint alpha is not encoded")),
    crate::paint::Paint::Rgba(_)=>Err(unsupported("native paint alpha is not encoded")),
    crate::paint::Paint::Cmyk{channels,scale}=>Ok(Some((if scale==100 {2}else{3},u32::from_le_bytes(channels)))),
}}
fn color_record(data:&mut [u8],at:usize,(model,value):(u16,u32))->Result<()> {
    bytes(data,at,12)?;data[at..at+2].copy_from_slice(&model.to_le_bytes());data[at+2..at+4].copy_from_slice(&5u16.to_le_bytes());set32(data,at+4,0)?;set32(data,at+8,value)
}
fn paint(template:&[u8],key:u32,color:&str,line:bool,width:f64)->Result<Vec<u8>> {
    let mut data=template.to_vec();set32(&mut data,0,key)?;
    if let Some(rgb)=rgb(color)? {
        if line {
            let mut at=4;
            loop {let tag=u32_at(&data,at)?;let len=u32_at(&data,at+4)? as usize;at+=8;if tag==1 {break;}
                if tag==2 {if len!=43 {return Err(invalid("unexpected native color cache"));}color_record(&mut data,at+5,rgb)?;}
                bytes(&data,at,len)?;at+=len;
            }set_coord(&mut data,at+6,width)?;color_record(&mut data,at+64,rgb)?;
        }else{color_record(&mut data,27,rgb)?;}
    }Ok(data)
}
pub(super) fn path_geometry(path:&BezPath)->Result<Vec<u8>> {
    let mut pts=Vec::<Point>::new();let mut flags=Vec::<u8>::new();let mut start=0;let mut current=Point::ORIGIN;let mut drawable=false;
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p)=>{pts.push(p);flags.push(4);start=pts.len()-1;current=p;},
            PathEl::LineTo(p)=>{pts.push(p);flags.push(0x40);current=p;drawable=true;},
            PathEl::QuadTo(q,p)=>{let c1=current+(q-current)*(2./3.);let c2=p+(q-p)*(2./3.);pts.extend([c1,c2,p]);flags.extend([0xc0,0xc0,0x80]);current=p;drawable=true;},
            PathEl::CurveTo(c1,c2,p)=>{pts.extend([c1,c2,p]);flags.extend([0xc0,0xc0,0x80]);current=p;drawable=true;},
            PathEl::ClosePath=>{if pts.len()<=start+1 {return Err(unsupported("empty closed subpath"));}if current!=pts[start] {pts.push(pts[start]);flags.push(0x40);}flags[start]|=8;*flags.last_mut().unwrap()|=8;current=pts[start];},
        }
    }
    if !drawable || pts.len()>65535 {return Err(unsupported("CDR14 path needs drawable geometry and at most 65535 native points"));}
    let mut out=(pts.len() as u16).to_le_bytes().to_vec();out.extend([0,0]);for p in pts {out.extend(coord(p.x)?.to_le_bytes());out.extend(coord(-p.y)?.to_le_bytes());}out.extend(flags);out.extend([0;8]);Ok(out)
}
fn transform(node:&mut Node,m:[f64;6])->Result<()> {
    let data=&mut node.find_mut(b"trfd")?.data;let at=u32_at(data,u32_at(data,8)? as usize)? as usize+16;
    for (i,v) in m.iter().enumerate() {data.get_mut(at+i*8..at+(i+1)*8).ok_or_else(||invalid("short transform template"))?.copy_from_slice(&v.to_le_bytes());}Ok(())
}
fn bounds(node:&mut Node,b:Rect,size:[f64;2])->Result<()> {
    let [w,h]=size;let values=[b.x0-w/2.,h/2.-b.y0,b.x1-w/2.,h/2.-b.y1];let mut raw=vec![];for v in values {raw.extend(coord(v)?.to_le_bytes());}
    node.find_mut(b"bbox")?.data=raw.clone();let mut twice=raw.clone();twice.extend(raw);node.find_mut(b"obbx")?.data=twice;Ok(())
}
pub(super) fn painted_bounds(node:&mut Node,b:Rect,ink:Rect,size:[f64;2])->Result<()> {
    bounds(node,b,size)?;let mut raw=node.find(b"bbox")?.data.clone();raw.extend(super::ink_cache::encoded(b.union(ink),size)?);node.find_mut(b"obbx")?.data=raw;Ok(())
}
fn text_style(template:&[u8],count:u16,size:f64,style_id:u32,font:super::fonts::Choice)->Result<Vec<u8>> {
    let marker=[0xff,8,0x39,0,0x86,0];let pos=template.windows(marker.len()).position(|v|v==marker).ok_or_else(||invalid("native text style not found"))?.checked_sub(2).ok_or_else(||invalid("short native text style"))?;
    let mut style=bytes(template,pos,94)?.to_vec();
    style[..2].copy_from_slice(&(count).to_le_bytes());style[4..6].copy_from_slice(&font.id.to_le_bytes());set_coord(&mut style,12,size)?;set32(&mut style,28,style_id)?;set32(&mut style,80,style_id)?;
    // Character-set and language refer to the same actual font declaration.
    // Cloning CHC for a Western font can resolve it as SimSun in Corel.
    style[6..8].copy_from_slice(&font.charset.to_le_bytes());style[88..94].copy_from_slice(&font.language());
    Ok(style)
}
fn artistic(template:&[u8],text:&str,size:f64,key:u32,paragraph:u32,paint:u32,font:super::fonts::Choice,matrix:[f64;6])->Result<Vec<u8>> {
    let normalized=text.replace("\r\n","\n").replace('\r',"\n");let parts=normalized.split('\n').collect::<Vec<_>>();
    let mut out=bytes(template,0,36)?.to_vec();out.extend(1u32.to_le_bytes());out.extend(key.to_le_bytes());for v in matrix {out.extend(v.to_le_bytes());}out.extend(0u32.to_le_bytes());out.extend((parts.len() as u32).to_le_bytes());
    for part in parts {
        out.extend(paragraph.to_le_bytes());out.extend([0,0]);text_runs(&mut out,template,part,size,paint,font,false)?;
    }Ok(out)
}

fn area_text(template:&[u8],artistic:&[u8],text:&str,size:f64,key:u32,style_id:u32,paint:u32,font:super::fonts::Choice,matrix:[f64;6],w:f64,h:f64)->Result<Vec<u8>> {
    let normalized=text.replace("\r\n","\n").replace('\r',"\n");let parts=normalized.split('\n').collect::<Vec<_>>();
    let mut out=bytes(template,0,36)?.to_vec();out.extend(1u32.to_le_bytes());out.extend(key.to_le_bytes());for v in matrix {out.extend(v.to_le_bytes());}out.extend(0u32.to_le_bytes());out.extend(super::typography::frame(template,w,h)?);out.extend((parts.len() as u32).to_le_bytes());
    for part in parts {
        out.extend(style_id.to_le_bytes());out.push(0);text_runs(&mut out,artistic,part,size,paint,font,true)?;
    }Ok(out)
}
fn text_runs(out:&mut Vec<u8>,template:&[u8],text:&str,size:f64,paint:u32,font:super::fonts::Choice,area:bool)->Result<()> {
    let runs=if font.legacy {vec![(text.encode_utf16().collect(),font.charset)]}else{super::fonts::script_runs(text,font.charset)};
    let counts=runs.iter().map(|r|super::text_units::count(&r.0)).collect::<Result<Vec<_>>>()?;let count=counts.iter().sum::<usize>();
    // Measured109 SaveAs14 area paragraphs inherit the complete regular
    // Western format from stlt, with one default style and no duplicate
    // character override. Mixed scripts and legacy declarations retain their
    // explicit styles until their own native profile has been established.
    let inherited=area&&font.charset==0&&!font.legacy&&text.is_ascii();
    out.extend((if inherited{1}else{runs.len() as u32+1}).to_le_bytes());out.extend((if inherited{count+1}else{1}as u16).to_le_bytes());out.extend([0,8,3,0,0,0,b'E',0,b'N',0,b'U',0]);
    if !inherited{for ((_,charset),count) in runs.iter().zip(&counts) {out.extend(text_style(template,*count as u16,size,paint,super::fonts::Choice{charset:*charset,..font})?);}}
    out.extend((count as u32).to_le_bytes());
    let mut payload=vec![];
    for (i,(units,charset)) in runs.iter().enumerate(){let(records,bytes)=super::text_units::encoded(units,if inherited{0}else{i+1},*charset==0&&!font.legacy)?;out.extend(records);payload.extend(bytes);}
    out.extend((payload.len() as u32).to_le_bytes());out.extend(payload);out.push(0);Ok(())
}
fn bitmap(template:&[u8],image:&image::RgbaImage,cmyk:Option<&super::native_pixels::Cmyk>,gray16:Option<&super::native_pixels::Gray16>,rgb16:Option<&super::native_pixels::Rgb16>,key:u32,w:f64,h:f64)->Result<Vec<u8>> {
    if usize::from(cmyk.is_some())+usize::from(gray16.is_some())+usize::from(rgb16.is_some())>1{return Err(invalid("ambiguous native image channel profile"));}
    if gray16.is_some_and(|g|(g.width,g.height)!=image.dimensions()||g.pixels.len()!=image.width() as usize*image.height() as usize)||gray16.is_some()&&image.pixels().any(|p|p[3]!=255){return Err(unsupported("grayscale16 native dimensions/alpha do not match the measured opaque profile"));}
    if rgb16.is_some_and(|g|(g.width,g.height)!=image.dimensions()||g.pixels.len()!=image.width() as usize*image.height() as usize*3)||rgb16.is_some()&&image.pixels().any(|p|p[3]!=255){return Err(unsupported("RGB16 native dimensions/alpha do not match the measured opaque profile"));}
    let mut parts=vec![];
    for alpha in [false,true] {
        if alpha && image.pixels().all(|p|p[3]==255) {continue;}
        let channels=if alpha {1}else if cmyk.is_some(){4}else if gray16.is_some(){2}else if rgb16.is_some(){6}else{3};let row=image.width() as usize*channels;let stride=(row+3)&!3;
        let mut raw=vec![0;stride*image.height() as usize];
        for y in 0..image.height() {for x in 0..image.width() {let p=image.get_pixel(x,y);let at=(image.height()-1-y) as usize*stride+x as usize*channels;if alpha {raw[at]=p[3];}else if let Some(cmyk)=cmyk {let src=(y as usize*image.width() as usize+x as usize)*4;raw[at..at+4].copy_from_slice(&cmyk.pixels[src..src+4]);}else if let Some(gray)=gray16{raw[at..at+2].copy_from_slice(&gray.pixels[y as usize*image.width() as usize+x as usize].to_le_bytes());}else if let Some(rgb)=rgb16{let src=(y as usize*image.width() as usize+x as usize)*3;for(c,k)in[2,1,0].into_iter().enumerate(){raw[at+2*c..at+2*c+2].copy_from_slice(&rgb.pixels[src+k].to_le_bytes());}}else{raw[at..at+3].copy_from_slice(&[p[2],p[1],p[0]]);}}}
        let offset=if alpha {u32_at(template,24)? as usize+8}else{40};let mut header=bytes(template,offset,78)?.to_vec();set32(&mut header,2,78+raw.len() as u32)?;
        for (i,v) in [if alpha {99}else if cmyk.is_some(){3}else if gray16.is_some(){14}else if rgb16.is_some(){12}else{1},1,image.width(),image.height(),1,channels as u32*8,stride as u32,raw.len() as u32].into_iter().enumerate(){set32(&mut header,14+i*4,v)?;}
        for (at,v) in [(50,image.width() as f64/w*1e6),(54,image.height() as f64/h*1e6)] {if !v.is_finite()||v>u32::MAX as f64 {return Err(unsupported("image resolution exceeds native range"));}set32(&mut header,at,v.round_ties_even() as u32)?;}
        header.extend(raw);parts.push(header);
    }
    let mut out=bytes(template,0,40)?.to_vec();set32(&mut out,0,key)?;set32(&mut out,12,32+parts.iter().map(Vec::len).sum::<usize>() as u32)?;set32(&mut out,24,if parts.len()==2 {32+parts[0].len() as u32}else{0})?;for p in parts {out.extend(p);}Ok(out)
}
fn scene(doc:&Document,nodes:&mut BTreeMap<String,Node>,frames:&mut BTreeMap<String,(Node,u32,u32)>,wrappers:&mut Vec<Node>,page:&str,parent:Option<&str>)->Result<Vec<Node>> {
    let mut result=vec![];
    for o in doc.objects.iter().rev().filter(|o|o.page_id==page&&o.parent_id.as_deref()==parent) {
        let mut n=nodes.remove(&o.id).ok_or_else(||invalid("missing encoded scene object"))?;
        if matches!(o.geometry,Geometry::Group) {n.children.as_mut().unwrap().extend(scene(doc,nodes,frames,wrappers,page,Some(&o.id))?);}
        if let Some((frame,key,frame_key))=frames.remove(&o.id) {
            let bbox=super::read::own_record(&n,b"bbox")?.data.clone();wrappers.push(super::clipping::wrapper(key,frame_key,bbox,n));n=frame;
        }
        result.push(n);
    }Ok(result)
}
pub(super) fn encode(doc:&Document)->Result<(Vec<u8>,Vec<Notice>)> {
    doc.validate()?;for o in &doc.objects {check14_text(o)?;}
    let (data,warnings,_)=lower(doc,false)?;Ok((data,warnings))
}
/// Internal geometry lowering only. Native21 must attach these frame rules to
/// its typed style records before exposing any21 bytes. Both versions carry
/// the SDK-bound native14001 mode as well.
pub(super) fn lower21(doc:&Document)->Result<(Vec<u8>,Vec<Notice>,BTreeMap<u32,crate::FillRule>)> {lower(doc,true)}
fn lower(doc:&Document,modern:bool)->Result<(Vec<u8>,Vec<Notice>,BTreeMap<u32,crate::FillRule>)> {
    let image_placement=super::images::needs_placement(doc);
    let source=doc;let mut normalized=normalized(doc)?;let path_units=super::path_precision::lower(source,&mut normalized)?;
    let mut independent_nibs=BTreeMap::new();
    for object in &mut normalized.objects {
        // Windows122's Corel-created21 shear references retain the original
        // circular LOCAL pen and the full body matrix. Do not manufacture the
        // fractional world preset / 50% binary chart that crashed in120.
        // Version14 has different measured failures and keeps its own gate.
        if modern&&super::primitive_frames::local_circle21(object)?{continue;}
        if let Ok(original)=source.object(&object.id){if super::path_precision::plan(source,original)?.is_none(){if let Some(angle)=super::primitive_frames::independent_nib_for_write(object)?{independent_nibs.insert(object.id.clone(),angle);}}}
    }
    let doc=&normalized;check_normalized(doc)?;
    let has_groups=doc.objects.iter().any(|o|matches!(o.geometry,Geometry::Group));
    let geo=read(GEOMETRY)?;let mut tree=read(CONTENT)?;let fonts=super::fonts::Fonts::new(doc)?;
    // Reject a description-style overflow before any glyph/ink measurement.
    // Otherwise the new painted cache would shape a known-invalid huge run.
    for o in &doc.objects{if let Geometry::Text{content,font_family,..}=&o.geometry{let font=fonts.choice(font_family)?;if !font.legacy{for paragraph in content.replace("\r\n","\n").replace('\r',"\n").split('\n'){if super::fonts::script_runs(paragraph,font.charset).len()>0x7fff{return Err(unsupported("native character style index exceeds this encoder's 15-bit field"));}}}}}
    fonts.apply(&mut tree)?;
    let group_profile=read(GROUP)?;let mut group_template=all(&group_profile).into_iter().find(|n|n.kind()==Some(b"grp ")).ok_or_else(||invalid("native group profile missing"))?.clone();
    group_template.children.as_mut().unwrap().retain(|n|!matches!(n.kind(),Some(b"obj "|b"grp ")));
    let ga=all(&geo);let ca=all(&tree);
    let area_profile=if doc.objects.iter().any(|o|matches!(&o.geometry,Geometry::Text{..})) {Some(super::typography::profile()?)}else{None};
    let mut paragraph_tables=area_profile.as_ref().map(|_|super::typography::Tables::parse(&ca.iter().find(|n|n.kind()==Some(b"stlt")).ok_or_else(||invalid("missing paragraph defaults"))?.data)).transpose()?;
    let mut templates=BTreeMap::new();
    for n in ga.iter().chain(ca.iter()).filter(|n|n.kind()==Some(b"obj ")) {let (kind,_)=arguments(&n.find(b"loda")?.data)?;templates.insert(kind,(*n).clone());}
    if let Some((nodes,_))=&area_profile {let template=all(nodes).into_iter().find(|n|n.kind()==Some(b"obj ")&&arguments(&n.find(b"loda").unwrap().data).is_ok_and(|(k,_)|k==6)).ok_or_else(||invalid("native area object profile missing"))?;templates.insert(6,template.clone());}
    let fills=ga.iter().filter(|n|n.tag==*b"fild").map(|n|n.data.clone()).collect::<Vec<_>>();
    let lines=ga.iter().filter(|n|n.tag==*b"outl").map(|n|n.data.clone()).collect::<Vec<_>>();
    let fill_none=fills.iter().find(|d|u16_at(d,12).ok()==Some(0)).ok_or_else(||invalid("no empty fill template"))?;
    let fill_solid=fills.iter().find(|d|u16_at(d,12).ok()==Some(1)).ok_or_else(||invalid("no RGB fill template"))?;
    let line_none=lines.iter().find(|d|super::read::outline(d).is_ok_and(|(s,_)|s=="none")).ok_or_else(||invalid("no empty outline template"))?;
    let line_solid=lines.iter().find(|d|super::read::outline(d).is_ok_and(|(s,_)|s!="none")).ok_or_else(||invalid("no RGB outline template"))?;
    let bitmap_template=ca.iter().find(|n|n.tag==*b"bmp "&&u32_at(&n.data,24).is_ok_and(|v|v!=0)).ok_or_else(||invalid("no alpha bitmap template"))?.data.clone();
    // The inherited text style table remains in the content profile even for
    // empty or geometry-only documents. Corel resolves its paint references
    // while opening the file, so retain those definitions and allocate object
    // paints above both tables instead of replacing/colliding with them.
    let mut fill_nodes=ca.iter().filter(|n|n.tag==*b"fild").map(|n|Node::list(b"filc",vec![(*n).clone()])).collect::<Vec<_>>();
    let mut line_nodes=ca.iter().filter(|n|n.tag==*b"outl").map(|n|(*n).clone()).collect::<Vec<_>>();
    let first_paint=ca.iter().filter(|n|matches!(&n.tag,b"fild"|b"outl")).map(|n|u32_at(&n.data,0)).collect::<Result<Vec<_>>>()?.into_iter().max().unwrap_or(0).checked_add(1).ok_or_else(||invalid("native paint key overflow"))?;
let opacity_mask_key=first_paint.checked_add((doc.objects.len() as u32).checked_mul(2).ok_or_else(||invalid("opacity mask key overflow"))?).and_then(|v|v.checked_add(1)).ok_or_else(||invalid("opacity mask key overflow"))?;
    if doc.objects.iter().any(|o|o.style.opacity!=1.) {fill_nodes.push(super::opacity::mask(opacity_mask_key,false)?);}
    let mut objects=BTreeMap::<String,Node>::new();let mut bitmaps=vec![];let mut resource_ids=BTreeMap::new();let mut unions=BTreeMap::<String,Rect>::new();let ink=super::ink_cache::with_nibs(doc,&independent_nibs)?;let mut ink_unions=BTreeMap::<String,Rect>::new();
    let ids=doc.objects.iter().enumerate().map(|(i,o)|(o.id.as_str(),i as u32+1)).collect::<BTreeMap<_,_>>();
    let mut path_roles=BTreeMap::<u32,u32>::new();let mut path_relations=vec![];
    for text in &doc.objects {if let Geometry::Text{text_path:Some(binding),..}=&text.geometry {
        let key=ids[text.id.as_str()];let guide=ids[binding.path_id.as_str()];path_relations.push((key,guide));path_roles.insert(key,1);path_roles.insert(guide,4);
    }}
    for (i,o) in doc.objects.iter().enumerate() {
        let size=super::pages::size(doc,&o.page_id)?;let [page_w,page_h]=size;
        let key=i as u32+1;let paint_key=first_paint.checked_add(i as u32).ok_or_else(||invalid("native paint key overflow"))?;
        if matches!(o.geometry,Geometry::Group) {
            let mut group=group_template.clone();
            let flags=u32_at(&group.find(b"flgs")?.data,0)?;group.find_mut(b"flgs")?.data=((flags&!0x110)|if o.locked {0x10}else{0}|if o.visible {0}else{0x100}).to_le_bytes().to_vec();
            group.find_mut(b"spnd")?.data=key.to_le_bytes().to_vec();
            group.find_mut(b"usdn")?.data=key.to_le_bytes().to_vec();
            group.find_mut(b"loda")?.data=loda(0,&[(1000,wide(&o.id)),(100,key.to_le_bytes().to_vec())]);
            transform(&mut group,[1.,0.,0.,0.,1.,0.])?;
            let [x,y,w,h]=doc.bounds(o)?;let b=Rect::new(x,y,x+w,y+h);if let Some(paint)=ink.get(&o.id){painted_bounds(&mut group,b,*paint,size)?;}else{bounds(&mut group,b,size)?;}objects.insert(o.id.clone(),group);continue;
        }
        let kind=match &o.geometry {Geometry::Rect{..}=>1,Geometry::Ellipse=>2,Geometry::Path{..}=>3,Geometry::Text{layout,..}=>if layout.mode==TextMode::Area {6}else{4},Geometry::Image{..}=>5,Geometry::Group=>unreachable!()};
        let mut obj=templates[&kind].clone();
        let flags=u32_at(&obj.find(b"flgs")?.data,0)?;obj.find_mut(b"flgs")?.data=((flags&!0x110)|if o.locked {0x10}else{0}|if o.visible {0}else{0x100}).to_le_bytes().to_vec();
        let (_,mut args)=arguments(&obj.find(b"loda")?.data)?;
        change(&mut args,1000,wide(&o.id))?;change(&mut args,100,key.to_le_bytes().to_vec())?;change(&mut args,20,paint_key.to_le_bytes().to_vec())?;change(&mut args,10,paint_key.to_le_bytes().to_vec())?;
        // Native names carry interchange identities; GUIDs copied from the
        // reference are removed rather than duplicated across real objects.
        args.retain(|(tag,_)|![0x9c40,0x9c41].contains(tag));obj.find_mut(b"spnd")?.data=key.to_le_bytes().to_vec();
        // Corel's SDK StaticID is stored in usdn, not the stream key or loda
        // bookkeeping argument. Cloning it makes shared/private shapes alias.
        obj.find_mut(b"usdn")?.data=key.to_le_bytes().to_vec();
        fill_nodes.push(Node::list(b"filc",vec![Node::leaf(b"fild",paint(if o.style.fill=="none" {fill_none}else{fill_solid},paint_key,&o.style.fill,false,0.)?)]));
        let mut outline=paint(if o.style.stroke=="none" {line_none}else{line_solid},paint_key,&o.style.stroke,true,o.style.stroke_width_mm)?;
        let mut cursor=4;let mut found=false;
        loop {let tag=u32_at(&outline,cursor)?;let length=u32_at(&outline,cursor+4)? as usize;cursor+=8;if tag==1 {break;}
            if tag==3 {if length!=4 || found {return Err(invalid("invalid native miter profile"));}set32(&mut outline,cursor,(o.style.stroke_miterlimit*1_000_000.).round_ties_even() as u32)?;found=true;}cursor+=length;
        }
        if !found {return Err(invalid("missing native miter profile"));}
        let at=super::read::outline_body(&outline)?;
        // Arranger strokes transform with their geometry even on the ordinary
        // native grid. A fixed-width native pen would only round-trip fields.
        if o.style.stroke!="none"{outline[at..at+2].copy_from_slice(&(if o.style.stroke_scale_with_object{34u16}else{2u16}).to_le_bytes());}
        if let Some((aspect,angle))=independent_nibs.get(&o.id){outline[at+10..at+12].copy_from_slice(&aspect.to_le_bytes());outline[at+12..at+16].copy_from_slice(&angle.to_le_bytes());}
        if let Some(nib)=o.style.stroke_fixed_nib{outline[at+10..at+12].copy_from_slice(&nib.stretch_percent.to_le_bytes());outline[at+12..at+16].copy_from_slice(&nib.angle_millionth_degrees.to_le_bytes());}
        outline[at+2..at+4].copy_from_slice(&(match o.style.stroke_linecap {LineCap::Butt=>0u16,LineCap::Round=>1,LineCap::Square=>2}).to_le_bytes());
        outline[at+4..at+6].copy_from_slice(&(match o.style.stroke_linejoin {LineJoin::Miter=>0u16,LineJoin::Round=>1,LineJoin::Bevel=>2}).to_le_bytes());
        line_nodes.push(Node::leaf(b"outl",outline));
        let [a,b,c,d,e,f]=o.transform;let (x,y)=(o.x_mm,o.y_mm);let native_x=x+super::typography::point_anchor_shift(doc,o)?;let mut native_y=y;
        let bound;
        match &o.geometry {
            Geometry::Rect{..}|Geometry::Ellipse=>{let mut data=arg(&args,30)?.to_vec();set_coord(&mut data,0,o.width_mm)?;set_coord(&mut data,4,o.height_mm)?;
                if let Geometry::Rect{radius_mm}=&o.geometry {for at in [8,12,16,20] {set_coord(&mut data,at,*radius_mm)?;}}
                let native_size=[i32_at(&data,0)? as f64/10000.,i32_at(&data,4)? as f64/10000.];let native_radius=if kind==1{i32_at(&data,8)? as f64/10000.}else{0.};change(&mut args,30,data)?;native_y+=o.height_mm;
                let m=Affine::new(o.transform)*Affine::translate((x,y));
                let native_frame=m*Affine::translate((0.,o.height_mm-native_size[1]));
                bound=if kind==2 {let p=native_frame*Point::new(native_size[0]/2.,native_size[1]/2.);let rx=(a*native_size[0]/2.).hypot(c*native_size[1]/2.);let ry=(b*native_size[0]/2.).hypot(d*native_size[1]/2.);Rect::new(p.x-rx,p.y-ry,p.x+rx,p.y+ry)}else{super::primitive_frames::rectangle_bounds(native_size[0],native_size[1],native_radius,native_frame)};
            },
            Geometry::Path{d,fill_rule}=>{let path=crate::path::parse(d)?;let geometry=path_geometry(&path)?;change(&mut args,30,geometry.clone())?;
                // All16 SDK-bound ordinary/PowerClip controls use14001=1
                // for nonzero and omit it for evenodd, in both14 and21.
                args.retain(|(k,_)|*k!=14001);
                if fill_rule.is_nonzero(){args.push((14001,1u32.to_le_bytes().to_vec()));}
                // Bounds use the quantized native curve, including converted
                // quadratic controls, rather than an unquantized source cache.
                let q=super::read::curve(&geometry)?;let matrix=Affine::new(o.transform)*Affine::translate((x,y));bound=(matrix*q).bounding_box();
            },
            Geometry::Text{content,font_size_mm,font_family,layout,..}=>{
                if kind==4 {native_y+=font_size_mm;}let tx=(a*native_x+c*native_y+e)*10000.-page_w*5000.;let ty=page_h*5000.-(b*native_x+d*native_y+f)*10000.;
                let font=fonts.choice(font_family)?;
                let style=paragraph_tables.as_mut().unwrap().add_paragraph(&area_profile.as_ref().unwrap().1,font,*font_size_mm,paint_key,layout,super::typography::tracking(doc,o)?)?;
                if kind==6 {
                    obj.find_mut(b"txsm")?.data=area_text(&obj.find(b"txsm")?.data,&templates[&4].find(b"txsm")?.data,content,*font_size_mm,key,style,paint_key,font,[a,-c,tx,-b,d,ty],o.width_mm,o.height_mm)?;
                    let geometry=super::typography::geometry(arg(&args,30)?,o.width_mm,o.height_mm)?;change(&mut args,30,geometry)?;
                }else{
                    let options=super::path_text::encoded_options(doc,o)?;
                    let matrix=if options.is_some(){super::path_shear::native_matrix(doc,o)?}else{[a,-c,tx,-b,d,ty]};
                    let mut story=artistic(&obj.find(b"txsm")?.data,content,*font_size_mm,key,style,paint_key,font,matrix)?;
                    if let Some(options)=options {
                        set32(&mut story,92,1)?;story.splice(96..96,options);
                        // Corel otherwise treats the story as ordinary point
                        // text and resets its live fit options on first open.
                        change(&mut args,30,[1u32,8].into_iter().flat_map(u32::to_le_bytes).collect())?;
                    }
                    obj.find_mut(b"txsm")?.data=story;change(&mut args,16015,1u32.to_le_bytes().to_vec())?;
                }
                // Shared text measurement supplies real Mac ink bounds. Corel
                // resolves the live text again against its installed font.
                if kind==6 {bound=(Affine::new(o.transform)*Affine::translate((x,y))).transform_rect_bbox(Rect::new(0.,0.,o.width_mm,o.height_mm));}else{
                    // Cache the bounds of the typography actually encoded, not
                    // the unquantized millimetres. Negative tracking can cross
                    // a bbox rounding boundary on the second exchange otherwise.
                    let mut measured=o.clone();let native_size=coord(*font_size_mm)? as f64/10000.;
                    if let Geometry::Text{font_size_mm,layout:settings,..}=&mut measured.geometry {*font_size_mm=native_size;settings.line_height_mm=Some(coord(layout.line_height_mm.unwrap_or(native_size*1.2))? as f64/10000.);settings.letter_spacing_mm=0.;}
                    let spacing=super::typography::space_width(doc,&measured)?*super::typography::tracking(doc,o)? as f64/1_000_000.;
                    if let Geometry::Text{layout,..}=&mut measured.geometry {layout.letter_spacing_mm=spacing;}
                    let [bx,by,bw,bh]=doc.bounds(&measured)?;bound=Rect::new(bx,by,bx+bw,by+bh);
                }
            },
            Geometry::Image{resource_id,..}=>{
                let rid=if let Some(id)=resource_ids.get(resource_id) {*id}else{
                    let rid=resource_ids.len() as u32+1;let r=&doc.resources[resource_id];let bytes=STANDARD.decode(&r.data_base64).map_err(|e|invalid(e.to_string()))?;let image=image::load_from_memory(&bytes).map_err(|e|invalid(e.to_string()))?.to_rgba8();
                    let cmyk=super::native_pixels::source(&bytes,&r.mime)?;let gray16=super::native_pixels::gray16(&bytes,&r.mime)?;let rgb16=super::native_pixels::rgb16(&bytes,&r.mime)?;
                    bitmaps.push(Node::leaf(b"bmp ",bitmap(&bitmap_template,&image,cmyk.as_ref(),gray16.as_ref(),rgb16.as_ref(),rid,o.width_mm,o.height_mm)?));resource_ids.insert(resource_id.clone(),rid);rid
                };
                let info=crate::resource::validate(&doc.resources[resource_id])?;let mut data=arg(&args,30)?.to_vec();
                for (j,v) in [0.,o.height_mm,o.width_mm,0.,0.,o.height_mm,o.width_mm,0.].into_iter().enumerate(){set_coord(&mut data,j*4,v)?;}
                let depth=match(info.metadata.png_bit_depth,info.metadata.png_color_type){(Some(16),Some(0))=>16u16,(Some(16),Some(2))=>48u16,_=>24u16};data[34..36].copy_from_slice(&depth.to_le_bytes());for (j,v) in [info.width_px,info.height_px,key,rid].into_iter().enumerate(){set32(&mut data,36+j*4,v)?;}
                for (j,v) in [0.,o.height_mm,o.width_mm,o.height_mm,o.width_mm,0.,0.,0.,0.,o.height_mm].into_iter().enumerate(){set_coord(&mut data,76+j*4,v)?;}
                change(&mut args,30,data)?;native_y+=o.height_mm;bound=(Affine::new(o.transform)*Affine::translate((x,y))).transform_rect_bbox(Rect::new(0.,0.,o.width_mm,o.height_mm));
            },
            Geometry::Group=>unreachable!(),
        }
        let tx=(a*native_x+c*native_y+e)*10000.-page_w*5000.;let ty=page_h*5000.-(b*native_x+d*native_y+f)*10000.;transform(&mut obj,[a,-c,tx,-b,d,ty])?;
        if path_roles.get(&key)==Some(&1) {transform(&mut obj,super::path_shear::native_matrix(doc,o)?)?;}
        if let Some(role)=path_roles.get(&key) {let flags=u32_at(&obj.find(b"flgs")?.data,0)?;obj.find_mut(b"flgs")?.data=(flags|role).to_le_bytes().to_vec();}
        let scope=source.object(&o.id).ok().map(super::paint_params::native_scope).transpose()?.unwrap_or(2);
        if let Some(effect)=super::opacity::encode_scoped(o.style.opacity,key,opacity_mask_key,scope)? {
            args.push((8000,effect));let value=u32_at(&obj.find(b"flgs")?.data,0)?;obj.find_mut(b"flgs")?.data=(value|0x80000).to_le_bytes().to_vec();
        }
        let painted=bound.union(ink.get(&o.id).copied().unwrap_or(bound));obj.find_mut(b"loda")?.data=loda(kind,&args);if ink.contains_key(&o.id){painted_bounds(&mut obj,bound,painted,size)?;}else{bounds(&mut obj,bound,size)?;}if kind==2{super::primitive_frames::minimum_ellipse_cache(&mut obj)?;}unions.entry(o.page_id.clone()).and_modify(|v|*v=v.union(bound)).or_insert(bound);ink_unions.entry(o.page_id.clone()).and_modify(|v|*v=v.union(painted)).or_insert(painted);objects.insert(o.id.clone(),obj);
    }
    let mut clip_frames=BTreeMap::new();let mut clip_wrappers=vec![];let mut clip_rules=BTreeMap::new();let mut clip_units=0;
    for (i,o) in doc.objects.iter().filter(|o|o.clip.is_some()).enumerate() {
        let frame_key=doc.objects.len() as u32+1+i as u32*2;let wrapper_key=frame_key+1;let paint_key=first_paint+doc.objects.len() as u32+i as u32;
        clip_rules.insert(frame_key,o.clip.as_ref().unwrap().rule());
        let size=super::pages::size(doc,&o.page_id)?;
        let clip_frame=super::clip_precision::plan(source,doc,o)?;
        clip_units+=usize::from(clip_frame.units>1||clip_frame.matrix!=[1.,0.,-size[0]*5000.,0.,1.,size[1]*5000.]);
        let mut frame=templates[&3].clone();let mut id=format!("{}{frame_key}",super::clipping::PREFIX);
        while doc.objects.iter().any(|o|o.id==id){id.push('_');}
        frame.find_mut(b"spnd")?.data=frame_key.to_le_bytes().to_vec();frame.find_mut(b"flgs")?.data=0x08100a80u32.to_le_bytes().to_vec();
        frame.find_mut(b"usdn")?.data=frame_key.to_le_bytes().to_vec();
        let mut frame_args=vec![(1000,wide(&id)),(100,frame_key.to_le_bytes().to_vec()),(30,clip_frame.geometry),(20,paint_key.to_le_bytes().to_vec()),(10,paint_key.to_le_bytes().to_vec()),(8005,[wrapper_key,1,0,1].into_iter().flat_map(u32::to_le_bytes).collect()),(40200,vec![0;4])];
        if o.clip.as_ref().unwrap().rule().is_nonzero(){frame_args.push((14001,1u32.to_le_bytes().to_vec()));}
        frame.find_mut(b"loda")?.data=loda(3,&frame_args);
        fill_nodes.push(Node::list(b"filc",vec![Node::leaf(b"fild",paint(fill_none,paint_key,"none",false,0.)?)]));
        line_nodes.push(Node::leaf(b"outl",paint(line_none,paint_key,"none",true,0.)?));
        transform(&mut frame,clip_frame.matrix)?;bounds(&mut frame,clip_frame.world_path.bounding_box(),size)?;
        clip_frames.insert(o.id.clone(),(frame,wrapper_key,frame_key));
    }
    for n in &mut tree {if let Ok(table)=n.list_mut(b"bmpt") {table.children=Some(bitmaps);break;}}
    // Replace the whole native drawing layer, leaving no reference objects.
    let main=tree.iter_mut().find(|n|n.children.as_ref().is_some_and(|c|c.iter().any(|x|x.kind()==Some(b"doc ")))).ok_or_else(||invalid("missing document profile"))?;
    let doc_node=main.list_mut(b"doc ")?;
    doc_node.children.as_mut().unwrap().retain(|n|n.tag!=*b"lnkt");
    if !path_relations.is_empty() {doc_node.children.as_mut().unwrap().push(Node::leaf(b"lnkt",super::path_text::relation_table(&path_relations)?));}
    if let Some(tables)=&paragraph_tables {doc_node.list_mut(b"stlt")?.data=tables.encode();}
    doc_node.list_mut(b"filt")?.children=Some(fill_nodes);doc_node.list_mut(b"otlt")?.children=Some(line_nodes);
    let pages=main.children.as_mut().ok_or_else(||invalid("missing document lists"))?;
    // Native drawing pages are siblings of doc/master; clone the known page
    // envelope, replace its entire object layer and all cached page bounds.
    // Object/style/bitmap keys above are document-wide, never restarted per page.
    let template=pages.iter().find(|n|n.kind()==Some(b"page") && super::read::flags(n).is_ok_and(|f|f&0x10000==0)).ok_or_else(||invalid("missing drawing page"))?.clone();
    pages.retain(|n|n.kind()!=Some(b"page") || super::read::flags(n).is_ok_and(|f|f&0x10000!=0));
    for p in &doc.pages {
        let mut page=template.clone();
        let group=page.list_mut(b"gobj")?;
        let layer=group.children.as_mut().and_then(|c|c.iter_mut().find(|n|n.kind()==Some(b"layr") && super::read::flags(n).is_ok_and(|f|f==0x98000000))).ok_or_else(||invalid("missing native object layer"))?;
        let children=layer.children.as_mut().ok_or_else(||invalid("missing drawing layer"))?;
        children.retain(|n|n.kind()!=Some(b"obj "));
        children.retain(|n|n.kind()!=Some(b"grp "));
        children.extend(scene(doc,&mut objects,&mut clip_frames,&mut clip_wrappers,&p.id,None)?);
        let b=unions.get(&p.id).copied();
        for n in page.children.as_mut().unwrap() {
            if n.tag==*b"bbox" || n.tag==*b"obbx" {
                let mut raw=vec![];
                for v in b.map_or([0.;4],|b|[b.x0-p.width_mm/2.,p.height_mm/2.-b.y0,b.x1-p.width_mm/2.,p.height_mm/2.-b.y1]) {raw.extend(coord(v)?.to_le_bytes());}
                if n.tag==*b"obbx" {let outer=ink_unions.get(&p.id).map(|b|super::ink_cache::encoded(*b,[p.width_mm,p.height_mm])).transpose()?.unwrap_or_else(||raw.clone());raw.extend(outer);}n.data=raw;
            }
        }
        super::pages::set_properties(&mut page,p)?;pages.push(page);
    }
    if doc.objects.iter().any(|o|o.page_id==crate::WORKSPACE_PAGE) {
        let master=main.children.as_mut().unwrap().iter_mut().find(|n|n.kind()==Some(b"page")&&super::read::flags(n).is_ok_and(|f|f&0x10000!=0)).ok_or_else(||invalid("missing native master page"))?;
        let mut layer=super::pages::workspace_layer()?;layer.children.as_mut().unwrap().extend(scene(doc,&mut objects,&mut clip_frames,&mut clip_wrappers,crate::WORKSPACE_PAGE,None)?);
        master.list_mut(b"gobj")?.children.as_mut().unwrap().push(layer);
        let flags=super::read::flags(master)?;master.find_mut(b"flgs")?.data=(flags|0x40).to_le_bytes().to_vec();
        let size=super::pages::size(doc,crate::WORKSPACE_PAGE)?;let b=unions[crate::WORKSPACE_PAGE];
        let raw=[b.x0-size[0]/2.,size[1]/2.-b.y0,b.x1-size[0]/2.,size[1]/2.-b.y1].into_iter().map(coord).collect::<Result<Vec<_>>>()?.into_iter().flat_map(i32::to_le_bytes).collect::<Vec<_>>();
        master.find_mut(b"bbox")?.data=raw.clone();let mut obbx=raw.clone();obbx.extend(super::ink_cache::encoded(ink_unions[crate::WORKSPACE_PAGE],size)?);
        let children=master.children.as_mut().unwrap();children.retain(|n|n.tag!=*b"obbx");children.push(Node::leaf(b"obbx",obbx));
    }
    if !objects.is_empty() || !clip_frames.is_empty() {return Err(invalid("encoded objects were not assigned to a drawing page or desktop layer"));}
    if !clip_wrappers.is_empty(){main.list_mut(b"doc ")?.children.as_mut().unwrap().push(Node::list(b"clpt",clip_wrappers));}
    for node in &mut tree {if let Ok(config)=node.find_mut(b"mcfg") {set_coord(&mut config.data,12,doc.pages[0].width_mm)?;set_coord(&mut config.data,16,doc.pages[0].height_mm)?;break;}}
    // A newly generated document cannot retain the template objects' values.
    for node in &mut tree {if let Ok(values)=node.list_mut(b"udat") {values.children=Some(vec![]);break;}}
    let riff=write(&tree)?;let font_table=fonts.encoded_cache();
    let metadata=super::metadata::document(source,false)?;
    let mut members:Vec<(&str,&[u8])>=vec![("mimetype",b"application/x-vnd.corel.draw.document+zip"),("content/riffData.cdr",&riff),("font/fontTable.dat",&font_table)];
    if let Some(data)=metadata.as_deref(){members.push(("metadata/metadata.xml",data));}
    let data=zip(&members);
    let mut warnings=vec![Notice::new("CDR_EXPERIMENTAL","CDR14 page/document interchange; native compatibility of this Rust output is pending. Keep the complete .arranger source. Coordinates use 0.0001 mm; text font resolution/layout may differ between systems.")];
    if doc.objects.iter().any(|o|matches!(o.geometry,Geometry::Text{..})) {warnings.push(Notice::new("CDR_FONT_DECLARATION","Native TrueType/default-variable declarations and caches use real family/OS2/name metadata; static CFF uses the measured declaration-only form. Verified missing-font declarations may be reused. Font programs are not embedded; missing fonts, local versions and Corel reflow require explicit native validation."));}
    if doc.objects.iter().any(|o|matches!(&o.geometry,Geometry::Text{content,..} if content.chars().any(|c|c as u32>0xffff))) {warnings.push(Notice::new("CDR_SUPPLEMENTARY_GLYPH","Supplementary Unicode scalar content survives the measured CDR21 Corel2019 roundtrips, but its Symbol/ExtB/ExtG glyphs render as missing-glyph boxes on this installation. Text data preservation does not establish visual compatibility or full-fidelity save. Other fonts and Corel versions remain unverified."));}
    if area_profile.is_some() {warnings.push(Notice::new("CDR_TEXT_LAYOUT","Editable point/rectangular area text preserves paragraphs/blank lines, explicit tracking and baseline spacing; point text is left aligned, area text also supports center/right. Area frame size is retained. Native tracking is a percentage of the resolved font space advance; it is converted to/from local millimetres. Corel reflows against its own installed fonts; overflow and glyph positions require native validation. Keep the complete .arranger source."));}
    if crate::paint::has_cmyk(doc) {warnings.push(Notice::new("CDR_CMYK_CHANNELS","Solid native CMYK100/CMYK255 fill and outline channels are preserved without conversion to RGB. ICC profiles, spot colors and print compatibility remain unverified; screen/SVG/PNG/PDF previews use an unmanaged RGB approximation."));}
    if doc.objects.iter().any(|o|o.page_id==crate::WORKSPACE_PAGE) {warnings.push(Notice::new("CDR_WORKSPACE","Shared objects are encoded once on the native nonprinting desktop layer; drawing-page exports exclude this layer. Corel validation of this output is pending."));}
    if has_groups {warnings.push(Notice::new("CDR_GROUP_NORMALIZED","Group hierarchy and per-container stacking preserved. CDR14 members use page coordinates: group affine placements are composed into members; original local transforms, group frames and inert group paint are not preserved. Native Corel validation remains pending."));}
    if has_groups {warnings.push(Notice::new("CDR_GROUP_STYLE_PARAMETERS","Nonpainting group fill/outline scalar properties are preserved with local frame parameters in document Notes. Active group opacity remains a native effect; no original text/geometry or scene hierarchy is embedded."));}
    if image_placement {warnings.push(Notice::new("CDR_IMAGE_PLACEMENT_NORMALIZED","Image contain/cover and legacy crop are encoded as fitted editable bitmaps with native PowerClip viewports. Source frame dimensions, fit parameter and clip representation are normalized. Original containers are retained separately; metadata requiring native color support remains guarded. Keep the complete .arranger source."));}
    if source.objects.iter().any(|o|o.style.stroke=="none"||o.style.opacity!=1.||o.style.fill.len()==9||o.style.stroke.len()==9) {warnings.push(Notice::new("CDR_PAINT_PARAMETERS","Native uniform transparency carries whole-object opacity, or composed alpha for a single paint. Bounded Notes retain source alpha, exact opacity and inactive outline scalars; native paint edits take priority. Legacy ObjectData remains readable."));}
    if source.resources.values().any(|r|STANDARD.decode(&r.data_base64).ok().and_then(|raw|crate::resource::metadata(&raw,&r.mime).ok()).is_some_and(|m|m.needs_managed_interpretation())) {warnings.push(Notice::new("CDR_RESOURCE_METADATA_UNMANAGED","Exact PNG/JPEG source containers including ICC/EXIF are retained in resource Notes. No ICC conversion or EXIF auto-orientation is applied. Managed appearance, high-depth editing and printing are not established."));}
    if source.objects.iter().any(|o|if let Geometry::Image{resource_id,..}=&o.geometry{let r=&source.resources[resource_id];STANDARD.decode(&r.data_base64).ok().and_then(|raw|crate::resource::metadata(&raw,&r.mime).ok()).is_some_and(|m|m.jpeg_components==Some(4))}else{false}) {warnings.push(Notice::new("CDR_CMYK_PIXELS","Adobe transform-0 CMYK8 JPEGs are decoded into four native editable CMYK channels and checked against source channel hashes. JPEG decoder rounding can differ from Corel's own import. Original containers remain exact; ICC-managed display/printing and new Corel output validation are pending."));}
    if !source.resources.is_empty() {warnings.push(Notice::new("CDR_RESOURCE_ORIGINAL","Used and unreferenced PNG/JPEG source containers, IDs and image bindings are carried in a checked resource-only catalogue in standard document Notes. Editable bitmap pixels remain native. New output still requires Corel validation; full-fidelity Save remains incomplete."));}
    if doc.objects.iter().any(|o|o.clip.is_some()){warnings.push(Notice::new("CDR_CLIP_NORMALIZED","Editable PowerClip frames enclose original vector/text/image/group contents. Clip geometry is encoded as native closed paths in page coordinates; original local parameterization is normalized. Corel validation of this output is pending."));}
    if clip_units>0{warnings.push(Notice::new("CDR_CLIP_LOCAL_UNITS","Unpainted PowerClip frames use bounded local point units with reciprocal native affine matrices. Native points and frame transforms carry the geometry; Notes add no clip coordinates or residuals. Actual native point/frame edits take priority. New Corel validation remains pending."));}
    if !path_relations.is_empty(){warnings.push(Notice::new("CDR_PATH_NORMALIZED","Editable native text/guide relations use explicit offsets with no glyph-placement cache. Open anchors use guide-length semantics; the bounded centered square top uses the measured TOP quadrant. Other closed placement remains unresolved. Corel recomputes against installed fonts; new output still requires actual Corel validation."));}
    if !super::known_native_layout_issues(source).is_empty(){warnings.push(Notice::new("CDR_CLOSED_FIT_PLACEMENT_UNRESOLVED","闭合导向上的文字定位尚未与 Corel 保持一致；现有实测出现位置偏差。源字段能恢复不代表实际字形位置正确，请同时保留完整源文件。"));}
    if source.objects.iter().any(|o|matches!(o.geometry,Geometry::Text{text_path:Some(_),..})&&super::path_text::own_turn(o).is_some()){warnings.push(Notice::new("CDR_PATH_CYCLIC_ROTATION","Closed-guide quarter turns currently encode source-derived fit offsets. Actual Corel controls have placement and rotation failures; this is not validated native rotation. Keep the complete source while closed-fit semantics remain unresolved."));}
    if path_units>0{warnings.push(Notice::new("CDR_PATH_LOCAL_UNITS","Eligible paths and outlined primitives use adaptive native point units with reciprocal affine matrices, including flattened parents and own clips. Active pens use actual native scaling properties; live guides compute offsets from the projected curve. Native point and pen edits take priority. Notes retain units, guarded scalar parameters and formats, never endpoint/control or residual arrays. New Corel output validation remains pending."));}
    if source.objects.iter().any(|o|matches!(o.geometry,Geometry::Rect{radius_mm} if radius_mm>0.)&&o.clip.is_none()&&source.world_transform(o).is_ok_and(|m|m.determinant()<0.)) {warnings.push(Notice::new("CDR_RECT_REFLECTION_NORMALIZED","A reflected uniformly rounded rectangle uses its equivalent positive-orientation symmetric frame. Shape/radius remain editable; the original local reflection chart is normalized. New Corel corner validation is pending."));}
    Ok((data,warnings,clip_rules))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ellipse_bbox_caches_use_the_actual_native_dimensions_and_bottom_origin() {
        let mut doc=Document::new("ellipse-cache".into(),210.,297.).unwrap();
        let object:crate::Object=serde_json::from_value(serde_json::json!({"id":"ellipse","page_id":"page-1","x_mm":20.,"y_mm":30.,"width_mm":20.000049,"height_mm":10.000049,"transform":[3.,0.25,0.125,1.5,5.,6.],"geometry":{"kind":"ellipse"}})).unwrap();
        assert_eq!(super::super::path_precision::plan(&doc,&object).unwrap(),None);doc.objects.push(object.clone());
        let native=read_native(&encode(&doc).unwrap().0).unwrap();let n=all(&native.nodes).into_iter().find(|n|n.kind()==Some(b"obj ")).unwrap();
        let (_,args)=arguments(&n.find(b"loda").unwrap().data).unwrap();let dimensions=arg(&args,30).unwrap();let w=i32_at(dimensions,0).unwrap() as f64/10000.;let h=i32_at(dimensions,4).unwrap() as f64/10000.;
        let raw=&n.find(b"trfd").unwrap().data;let at=u32_at(raw,u32_at(raw,8).unwrap() as usize).unwrap() as usize+16;let m:[f64;6]=std::array::from_fn(|i|f64_at(raw,at+i*8).unwrap());
        // Resolve the written native y-up matrix directly, independently of
        // source Notes and the writer's top-left cache calculation.
        let center_x=m[0]*w/2.+m[1]*h/2.+m[2]/10000.;let center_y=m[3]*w/2.+m[4]*h/2.+m[5]/10000.;
        let rx=(m[0]*w/2.).hypot(m[1]*h/2.);let ry=(m[3]*w/2.).hypot(m[4]*h/2.);
        let expected=[coord(center_x-rx).unwrap(),coord(center_y+ry).unwrap(),coord(center_x+rx).unwrap(),coord(center_y-ry).unwrap()];
        let observed:[i32;4]=std::array::from_fn(|i|i32_at(&n.find(b"bbox").unwrap().data,i*4).unwrap());assert_eq!(observed,expected);
        assert_eq!(n.find(b"obbx").unwrap().data,[n.find(b"bbox").unwrap().data.clone(),n.find(b"bbox").unwrap().data.clone()].concat());
        assert_eq!(crate::cdr::decode(&encode(&doc).unwrap().0,&doc.id).unwrap().0,doc);
    }

    #[test]
    fn repeated_shapes_have_unique_corel_static_ids_and_a_nonprinting_master() {
        let mut doc=Document::new("native-identities".into(),210.,297.).unwrap();
        for i in 0..4 {
            doc.objects.push(serde_json::from_value(serde_json::json!({
                "id":format!("ellipse-{i}"),"page_id":if i<2 {"page-1"}else{crate::WORKSPACE_PAGE},
                "x_mm":10.+i as f64*30.,"y_mm":20.,"width_mm":25.,"height_mm":25.,
                "geometry":{"kind":"ellipse"},"style":{"fill":"#123456","stroke":"none","stroke_width_mm":0.,"opacity":1.}
            })).unwrap());
        }
        let raw=encode(&doc).unwrap().0;let tree=read(&raw).unwrap();let records=all(&tree);
        let ids=records.iter().filter(|n|n.kind()==Some(b"obj ")).map(|n|{
            u32_at(&n.find(b"usdn").unwrap().data,0).unwrap()
        }).collect::<Vec<_>>();
        assert_eq!(ids.len(),4);
        assert_eq!(ids.iter().collect::<std::collections::BTreeSet<_>>().len(),4,"Corel StaticID must not be cloned from the shape template");
        let master_layers=records.iter().filter(|n|n.kind()==Some(b"layr")&&n.children.as_deref().unwrap().iter().any(|c|c.kind()==Some(b"obj "))&&super::super::read::flags(n).unwrap()&0x10000!=0).collect::<Vec<_>>();
        assert_eq!(master_layers.len(),1);
        assert_ne!(super::super::read::flags(master_layers[0]).unwrap()&8,0,"shared desktop objects must not print");
        assert_eq!(crate::cdr::decode(&raw,"reopened").unwrap().0.objects.len(),4);
    }

    #[test]
    fn inherited_paints_survive_empty_and_small_exports_without_collisions() {
        let profile=read(CONTENT).unwrap();
        let required=all(&profile).into_iter().filter(|n|matches!(&n.tag,b"fild"|b"outl")).map(|n|(n.tag,n.data.clone())).collect::<Vec<_>>();
        let mut doc=Document::new("small-native".into(),210.,297.).unwrap();
        for count in 0..=3 {
            let raw=encode(&doc).unwrap().0;let tree=read(&raw).unwrap();let records=all(&tree);
            for (tag,data) in &required {
                let key=u32_at(data,0).unwrap();
                let found=records.iter().filter(|n|n.tag==*tag && u32_at(&n.data,0).ok()==Some(key)).collect::<Vec<_>>();
                assert_eq!(found.len(),1,"inherited paint missing or duplicated for {count} objects");
                assert_eq!(&found[0].data,data,"object paint replaced the inherited native definition");
            }
            assert_eq!(crate::cdr::decode(&raw,"reopened").unwrap().0.objects.len(),count);
            doc.objects.push(serde_json::from_value(serde_json::json!({
                "id":format!("small-{count}"),"page_id":"page-1","x_mm":10.,"y_mm":20.,"width_mm":25.,"height_mm":25.,
                "geometry":{"kind":"ellipse"},"style":{"fill":"#123456","stroke":"none","stroke_width_mm":0.,"opacity":1.}
            })).unwrap());
        }
    }
}
