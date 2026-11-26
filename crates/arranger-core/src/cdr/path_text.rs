//! Measured native text/guide relations. Cached glyph placement is derived;
//! unknown orientation/side/cache effects must not become default text.
use super::{binary::*,invalid,unsupported,Result};
use crate::{Document,Geometry,TextAnchor,TextOnPath,Object,Clip};
use kurbo::{Affine,ParamCurveArclen};
use std::collections::{BTreeMap,BTreeSet};
#[derive(Clone,Debug)]
pub(super) struct Options {pub offset:f64,pub distance:f64,pub anchor:TextAnchor,pub orientation:u32}
pub(super) fn check_frame(story:[f64;6],frame:[f64;6])->Result<()> {
    // These are dimensionless linear coefficients and raw native translations,
    // not millimetre errors. A coefficient epsilon can erase a live deformation
    // and amplify it over a large glyph/guide. The measured neutral profile has
    // identical matrices (including numerically equal signed zero).
    if story.into_iter().zip(frame).any(|(a,b)|!a.is_finite()||a!=b){return Err(unsupported("native fitted text independent story/frame matrix requires preservation support"));}
    if frame[0]!=1.||frame[1]!=0.||frame[3]!=0.||frame[4]!=1.{return Err(unsupported("native fitted text linear frame deformation requires preservation support"));}Ok(())
}
pub(super) fn similarity_scale(matrix:Affine)->Result<f64>{
    let[a,b,c,d,_,_]=matrix.as_coeffs();let scale=a.hypot(b);
    // Exact complex-similarity form; no coefficient epsilon that could hide
    // a shear/nonuniform scale over a long guide or large glyph.
    if a!=d||b!= -c||!scale.is_finite()||scale<=0.{return Err(unsupported("native path text parent must be a positive similarity, not shear, nonuniform scale or reflection"));}Ok(scale)
}
pub(super) fn parent_scale(doc:&Document,text:&Object)->Result<f64>{similarity_scale(text.parent_id.as_ref().map(|id|doc.world_transform(doc.object(id)?)).transpose()?.unwrap_or(Affine::IDENTITY))}
pub(super) fn scale_text(text:&mut Object,scale:f64){
    if let Geometry::Text{font_size_mm,layout,text_path,..}=&mut text.geometry{*font_size_mm*=scale;layout.letter_spacing_mm*=scale;if let Some(height)=&mut layout.line_height_mm{*height*=scale;}if let Some(binding)=text_path{binding.start_offset_mm*=scale;binding.baseline_offset_mm*=scale;}}
    if text.style.stroke!="none"{text.style.stroke_width_mm*=scale;}
}
pub(super) fn own_scale(text:&Object)->Result<f64>{
    if own_turn(text).is_some()||super::path_shear::factor(text).is_some(){return Ok(1.);}
    let[a,b,c,d,_,_]=text.transform;
    if a!=d||b!=0.||c!=0.||!a.is_finite()||a<=0.{return Err(unsupported("native fitted text own rotation/nonuniform scale/shear/reflection requires preservation support"));}
    Ok(a)
}
pub(super) fn own_turn(text:&Object)->Option<u8>{let m=&text.transform[..4];if m==[0.,1.,-1.,0.]{Some(1)}else if m==[-1.,0.,0.,-1.]{Some(2)}else if m==[0.,-1.,1.,0.]{Some(3)}else{None}}
/// World placement is s*p+delta after the existing guide layout. Conjugate
/// the source-local uniform scale by its parent, including the parent's origin.
/// Native font size and fit options carry this affine map; the guide stays live.
pub(super) fn translation_world(doc:&Document,text:&Object)->Result<Option<[f64;2]>>{
    if super::path_shear::plan(doc,text)?.is_some(){return Ok(None);}
    if let Some(turns)=own_turn(text){
        let[_,_,_,_,e,f]=text.transform;let local=super::path_rotation::rotate(kurbo::Point::new(text.x_mm,text.y_mm),turns)+kurbo::Vec2::new(e,f);
        let parent=text.parent_id.as_ref().map(|id|doc.world_transform(doc.object(id)?)).transpose()?.unwrap_or(Affine::IDENTITY);similarity_scale(parent)?;
        let[a,b,c,d,x,y]=parent.as_coeffs();let rotated=super::path_rotation::rotate(kurbo::Point::new(x,y),turns);let delta=[a*local.x+c*local.y+x-rotated.x,b*local.x+d*local.y+y-rotated.y];
        if delta.into_iter().any(|v|!v.is_finite()){return Err(invalid("native fitted rotation is not finite"));}return Ok(Some(delta));
    }
    let scale=own_scale(text)?;let[_,_,_,_,e,f]=text.transform;
    let delta=kurbo::Vec2::new(scale*text.x_mm+e,scale*text.y_mm+f);
    if scale==1.&&delta==kurbo::Vec2::ZERO{return Ok(None);}
    let parent=text.parent_id.as_ref().map(|id|doc.world_transform(doc.object(id)?)).transpose()?.unwrap_or(Affine::IDENTITY);similarity_scale(parent)?;
    let[a,b,c,d,x,y]=parent.as_coeffs();let world=[a*delta.x+c*delta.y+(1.-scale)*x,b*delta.x+d*delta.y+(1.-scale)*y];
    if world.into_iter().any(|v|!v.is_finite()){return Err(invalid("native fitted text translation is not finite"));}Ok(Some(world))
}
fn placement_offsets(doc:&Document,text:&Object)->Result<Option<[f64;2]>>{
    let Some([dx,dy])=translation_world(doc,text)?else{return Ok(None)};
    let Geometry::Text{text_path:Some(binding),..}=&text.geometry else{return Err(invalid("fitted translation requires a path story"))};
    let guide=doc.object(&binding.path_id)?;let Geometry::Path{d,..}=&guide.geometry else{return Err(invalid("fitted translation requires a curve guide"))};
    let units=super::path_precision::plan(doc,guide)?.unwrap_or(1);
    let raw=super::path_precision::projected(d,units)?;
    if let Some(turns)=own_turn(text){return super::path_rotation::offsets(doc,text,&raw,turns,[dx,dy]).map(Some);}
    // Classify in the actual projected local chart, before affine roundoff can
    // make a straight span appear bent. The guide itself is never simplified.
    let span=if let[kurbo::PathEl::MoveTo(p),kurbo::PathEl::LineTo(q)]=raw.elements(){Some([*p,*q])}else{
        super::path_guide::linear_span(&crate::path::parse(d)?,units).and_then(|_|super::path_guide::linear_span(&raw,units))
    };
    let [p,q]=span.ok_or_else(||unsupported("native fitted text placement requires one open monotone straight guide contour"))?;
    let world=doc.world_transform(guide)?;let(p,q)=(world*p,world*q);
    let v=q-p;let length=v.hypot();if length<=0.||!length.is_finite(){return Err(invalid("translated fitted text guide has no finite direction"));}
    let (u,v)=(v.x/length,v.y/length);let own=own_scale(text)?;
    let delta=kurbo::Vec2::new((own-1.)*p.x+dx,(own-1.)*p.y+dy);
    let offsets=[delta.x*u+delta.y*v,-delta.x*v+delta.y*u];let scale=parent_scale(doc,text)?;
    let run=crate::text::layout(doc,text)?;let advance=run.advance_mm*scale;
    let start=binding.start_offset_mm*scale-match binding.anchor{TextAnchor::Start=>0.,TextAnchor::Middle=>advance/2.,TextAnchor::End=>advance};
    // Moving the fit offset must not introduce a different endpoint clipping
    // or overflow regime from the original post-layout translation.
    if run.overflow||start<0.||start+advance>length||start*own+offsets[0]<0.||(start+advance)*own+offsets[0]>length{return Err(unsupported("transformed fitted text would cross the live guide endpoint/overflow boundary"));}
    Ok(Some(offsets))
}
pub(super) fn native_object(doc:&Document,text:&Object)->Result<Object>{
    let shear=super::path_shear::plan(doc,text)?;let offsets=placement_offsets(doc,text)?;
    if text.style.stroke!="none"&&super::path_outline::needs_check(doc,text)?&&coord(text.style.stroke_width_mm*parent_scale(doc,text)?*own_scale(text)?)?<=0{return Err(unsupported("fitted text outline would enter an unverified native hairline profile"));}
    let mut native=text.clone();scale_text(&mut native,parent_scale(doc,text)?*own_scale(text)?);native.x_mm=0.;native.y_mm=0.;native.transform=shear.map_or(Affine::IDENTITY.as_coeffs(),|(k,y)|super::path_shear::effective(k,y));
    if let (Some([along,normal]),Geometry::Text{text_path:Some(binding),..})=(offsets,&mut native.geometry){binding.start_offset_mm+=along;binding.baseline_offset_mm+=normal;}
    native.clip=text.clip.as_ref().map(|clip|Ok(Clip::Path{d:(doc.world_transform(text)?*clip.path()?).to_svg(),fill_rule:clip.rule()})).transpose()?;
    Ok(native)
}
pub(super) fn restore_parent_charts(doc:&mut Document,frames:Option<&super::local_frames::Frames>)->Result<BTreeMap<String,f64>>{
    let mut scales=BTreeMap::new();
    for index in 0..doc.objects.len(){let text=&doc.objects[index];if !matches!(text.geometry,Geometry::Text{text_path:Some(_),..}){continue;}
        let target=similarity_scale(super::local_frames::parent_chart(doc,frames,text)?)?;let current=parent_scale(doc,text)?;let id=text.id.clone();scale_text(&mut doc.objects[index],current/target);scales.insert(id,target);
    }Ok(scales)
}
impl Options {
    pub fn encode(&self)->Result<Vec<u8>> {
        let mut raw=vec![0;40];set32(&mut raw,4,self.orientation)?;
        set32(&mut raw,16,coord(self.offset)? as u32)?;set32(&mut raw,24,coord(self.distance)? as u32)?;
        set32(&mut raw,28,match self.anchor {TextAnchor::Start=>1,TextAnchor::End=>2,TextAnchor::Middle=>3})?;Ok(raw)
    }
    pub fn parse(raw:&[u8])->Result<Self> {
        if raw.len()!=40{return Err(invalid("native path option length differs"));}
        if [0,8,20,32,36].into_iter().any(|at|u32_at(raw,at).ok()!=Some(0))||u32_at(raw,4)?>4{return Err(unsupported("native text-path side/orientation options are not represented"));}
        let anchor=match u32_at(raw,28)? {1=>TextAnchor::Start,2=>TextAnchor::End,3=>TextAnchor::Middle,_=>return Err(unsupported("native text-path placement is not represented"))};
        // Word3 is retained in the source native file, not mistaken for the
        // explicit millimetre Offset. The controlled75000 word is7.5mm.
        Ok(Self{offset:i32_at(raw,16)? as f64/10000.,distance:i32_at(raw,24)? as f64/10000.,anchor,orientation:u32_at(raw,4)?})
    }
}
/// Only the canonical clockwise square measured by Windows158. Native selector
/// 1 is the TOP quadrant here, not an open-path start anchor. On its straight
/// top span, centering can be represented without guessing the native advance.
fn square_top_center(path:&kurbo::BezPath)->Option<(f64,f64)>{
    use kurbo::PathEl::*;
    let (p,q,r,s)=match path.elements(){
        [MoveTo(p),LineTo(q),LineTo(r),LineTo(s),ClosePath] =>(*p,*q,*r,*s),
        [MoveTo(p),LineTo(q),LineTo(r),LineTo(s),LineTo(t),ClosePath] if t==p =>(*p,*q,*r,*s),
        _=>return None,
    };
    let side=q.x-p.x;
    (side>0.&&side.is_finite()&&p.y==q.y&&q.x==r.x&&r.y==s.y&&s.x==p.x&&r.y-q.y==side).then_some((side/2.,side))
}
/// A clockwise four-cubic contour whose distinguished point is its top.
/// Exact quarter-turn symmetry proves the quadrant reference without searching
/// for or fitting a position to glyphs. Other contours remain unclassified.
fn closed_top_reference(path:&kurbo::BezPath)->Option<(f64,f64)>{
    if let Some(value)=square_top_center(path){return Some(value)}
    let pieces=super::path_rotation::pieces(path)?;
    if pieces.len()!=4||pieces.iter().any(|s|s.len()!=4){return None}
    let p=pieces[0][0];let q=pieces[0][3];let center=kurbo::Point::new(p.x,q.y);let radius=q.x-p.x;
    if radius<=0.||q.y-p.y!=radius||pieces[0][1].y!=p.y||pieces[0][1].x<=p.x||pieces[0][2].x!=q.x||pieces[0][2].y>=q.y{return None}
    for(i,piece)in pieces.iter().enumerate(){for(a,b)in pieces[0].iter().zip(piece){
        let v=super::path_rotation::rotate(kurbo::Point::new(a.x-center.x,a.y-center.y),i as u8)+center.to_vec2();
        if v!=*b{return None}
    }}
    let length:f64=path.segments().map(|s|s.arclen(1e-6)).sum();
    (length.is_finite()&&length>0.).then_some((0.,length))
}
fn centered_top_span(doc:&Document,text:&Object,path:&kurbo::BezPath,start:f64)->Result<Option<f64>>{
    let Some((center,side))=closed_top_reference(path)else{return Ok(None)};
    let Geometry::Text{layout,..}=&text.geometry else{return Ok(None)};
    if layout.mode!=crate::TextMode::Point||layout.align!=crate::TextAlign::Left{return Ok(None)}
    let advance=crate::text::layout(doc,text)?.advance_mm*parent_scale(doc,text)?*own_scale(text)?;
    Ok((advance>0.&&start-advance/2.>=0.&&start+advance/2.<=side).then_some(center))
}
/// Closed TOP places the *center* of the story at the quadrant reference,
/// independently of paragraph alignment (Windows closed-alignment experiment).
/// Start/end therefore need a measured advance in the encoded font/spacing
/// units. This is a font measurement, never a saved glyph-position cache.
fn closed_top_offset(doc:&Document,text:&Object,path:&kurbo::BezPath,scale:f64,along:f64)->Result<Option<f64>>{
    let Geometry::Text{layout,text_path:Some(binding),..}=&text.geometry else{return Ok(None)};
    let Some((center,side))=closed_top_reference(path)else{return Ok(None)};
    if layout.mode!=crate::TextMode::Point||layout.align!=crate::TextAlign::Left{return Ok(None)}
    let mut native=text.clone();scale_text(&mut native,scale);
    let tracking=super::typography::tracking(doc,&native)?;
    if let Geometry::Text{font_size_mm,layout,text_path,..}=&mut native.geometry{
        *font_size_mm=coord(*font_size_mm)? as f64/10000.;layout.letter_spacing_mm=0.;*text_path=None;
    }
    let spacing=super::typography::space_width(doc,&native)?*tracking as f64/1_000_000.;
    if let Geometry::Text{layout,..}=&mut native.geometry{layout.letter_spacing_mm=spacing;}
    let advance=crate::text::layout(doc,&native)?.advance_mm;
    let middle=binding.start_offset_mm*scale+along+match binding.anchor{TextAnchor::Start=>advance/2.,TextAnchor::Middle=>0.,TextAnchor::End=>-advance/2.};
    if advance<=0.||middle-advance/2.<0.||middle+advance/2.>side{return Ok(None)}
    Ok(Some(middle-center))
}
pub(super) fn encoded_options(doc:&Document,text:&crate::Object)->Result<Option<Vec<u8>>> {
    let Geometry::Text{text_path:Some(binding),..}=&text.geometry else{return Ok(None)};
    let guide=doc.object(&binding.path_id)?;let Geometry::Path{d,..}=&guide.geometry else{return Err(invalid("native path guide is not a curve"))};
    let translation=placement_offsets(doc,text)?.unwrap_or([0.,0.]);
    let scale=parent_scale(doc,text)?*own_scale(text)?;
    let native=super::path_precision::projected(d,super::path_precision::plan(doc,guide)?.unwrap_or(1))?;
    let path=doc.world_transform(guide)?*native;let length:f64=path.segments().map(|s|s.arclen(1e-6)).sum();
    if !length.is_finite()||length<=0. {return Err(invalid("native path guide has no finite length"));}
    if own_scale(text)?==1. {
        if let Some(offset)=closed_top_offset(doc,text,&path,scale,translation[0])?{
            return Ok(Some(Options{offset,distance:-(binding.baseline_offset_mm*scale+translation[1]),anchor:TextAnchor::Start,orientation:4}.encode()?));
        }
    }
    let offset=binding.start_offset_mm*scale+translation[0]-match binding.anchor {TextAnchor::Start=>0.,TextAnchor::Middle=>length/2.,TextAnchor::End=>length};
    Ok(Some(Options{offset,distance:-(binding.baseline_offset_mm*scale+translation[1]),anchor:binding.anchor,orientation:if super::path_shear::plan(doc,text)?.is_some(){0}else{4}}.encode()?))
}
pub(super) fn relation_table(rows:&[(u32,u32)])->Result<Vec<u8>> {
    if rows.len()>65535{return Err(unsupported("too many native path relations"));}
    let start=12+4*(rows.len()+1);let size=start+32*rows.len();let mut raw=vec![];
    for v in [size as u32,rows.len() as u32,12] {raw.extend(v.to_le_bytes());}
    for i in 0..=rows.len(){raw.extend(((start+32*i) as u32).to_le_bytes());}
    for (text,guide) in rows {for v in [64,*text,*guide,0,0,0,0,0] {raw.extend(v.to_le_bytes());}}Ok(raw)
}
pub(super) fn cache(raw:&[u8],count:usize)->Result<()> {
    if raw.len()!=count.checked_mul(24).ok_or_else(||invalid("native path cache overflow"))? {return Err(invalid("native path cache length differs"));}
    for row in raw.chunks_exact(24){if row[12..].iter().any(|b|*b!=0){return Err(unsupported("native path glyph-cache effects require preservation support"));}}
    Ok(())
}
pub(super) struct Links {pub by_text:BTreeMap<u32,u32>,guides:BTreeSet<u32>}
impl Links {
    pub fn new(nodes:&[Node])->Result<Self> {
        let tables=all(nodes).into_iter().filter(|n|n.tag==*b"lnkt").collect::<Vec<_>>();
        if tables.len()>1{return Err(invalid("ambiguous native text-path relation table"));}
        let mut result=Self{by_text:BTreeMap::new(),guides:BTreeSet::new()};
        let Some(table)=tables.first() else{return Ok(result)};let raw=&table.data;
        if raw.len()<16||u32_at(raw,0)? as usize!=raw.len()||u32_at(raw,8)?!=12{return Err(invalid("native relation table header differs"));}
        let n=u32_at(raw,4)? as usize;if n>65535||12+4*(n+1)>raw.len(){return Err(invalid("native relation count exceeds record"));}
        let offsets=(0..=n).map(|i|u32_at(raw,12+4*i).map(|v|v as usize)).collect::<Result<Vec<_>>>()?;
        if offsets[0]!=12+4*(n+1)||offsets[n]!=raw.len(){return Err(invalid("native relation bounds differ"));}
        for range in offsets.windows(2) {
            let [start,end]=[range[0],range[1]];
            if end<start||end-start!=32{return Err(unsupported("native relation record layout is not represented"));}
            let row=bytes(raw,start,32)?;
            if u32_at(row,0)?!=64||row[12..].iter().any(|b|*b!=0){return Err(unsupported("native relation type/options are not represented"));}
            let text=u32_at(row,4)?;let guide=u32_at(row,8)?;
            if text==0||guide==0||text==guide||result.by_text.insert(text,guide).is_some(){return Err(invalid("duplicate or cyclic native text-path relation"));}result.guides.insert(guide);
        }Ok(result)
    }
    pub fn flags(&self,key:u32,kind:u32)->Result<u32> {
        if self.by_text.contains_key(&key) {if kind!=4{return Err(invalid("native text-path endpoint is not artistic text"));}Ok(1)}
        else if self.guides.contains(&key) {if kind!=3{return Err(invalid("native text-path guide is not a curve"));}Ok(4)}else{Ok(0)}
    }
    pub fn bind(&self,doc:&mut Document,ids:&BTreeMap<u32,String>,options:&BTreeMap<u32,Options>)->Result<()> {
        if options.len()!=self.by_text.len()||options.keys().any(|k|!self.by_text.contains_key(k)){return Err(invalid("native path story and relation table disagree"));}
        for (key,target) in &self.by_text {
            let text_id=ids.get(key).ok_or_else(||invalid("native linked text missing"))?;let guide_id=ids.get(target).ok_or_else(||invalid("native path guide missing"))?;let opts=options.get(key).ok_or_else(||invalid("native path options missing"))?;
            let guide=doc.object(guide_id)?;
            if !matches!(guide.geometry,Geometry::Path{..}) {return Err(invalid("native path guide is not editable curve geometry"));}
            let path=doc.world_transform(guide)?*guide.local_path()?;
            let length:f64=path.segments().map(|s|s.arclen(1e-6)).sum();
            if !length.is_finite()||length<=0. {return Err(invalid("native path guide has no finite length"));}
            let mut start_offset_mm=opts.offset+match opts.anchor {TextAnchor::Start=>0.,TextAnchor::Middle=>length/2.,TextAnchor::End=>length};
            let mut anchor=opts.anchor;
            if opts.anchor==TextAnchor::Start&&opts.orientation==4 {
                if let Some((center,_))=closed_top_reference(&path){
                    let candidate=opts.offset+center;
                    if centered_top_span(doc,doc.object(text_id)?,&path,candidate)?.is_some(){start_offset_mm=candidate;anchor=TextAnchor::Middle;}
                }
            }
            let text=doc.objects.iter_mut().find(|o|&o.id==text_id).ok_or_else(||invalid("native text missing"))?;
            // Native path story matrices locate a derived cache. Live placement
            // follows the linked page-space guide and its explicit fit options.
            // Nonidentity linear text-frame matrices are rejected by the caller.
            text.x_mm=0.;text.y_mm=0.;text.transform=[1.,0.,0.,1.,0.,0.];
            let Geometry::Text{text_path,..}=&mut text.geometry else{return Err(invalid("native relation text has wrong geometry"))};
            *text_path=Some(TextOnPath{path_id:guide_id.clone(),start_offset_mm,baseline_offset_mm:-opts.distance,anchor});
        }Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn centered_square()->Document{
        let p=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/assets/cdr/cyclic-rotation-20261105/manifest.json");
        let v:serde_json::Value=serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap();let mut doc:Document=serde_json::from_value(v["cases"][0]["document"].clone()).unwrap();
        let t=doc.objects.iter_mut().find(|o|o.id=="content").unwrap();t.x_mm=0.;t.y_mm=0.;t.transform=Affine::IDENTITY.as_coeffs();
        let Geometry::Text{content,layout,text_path:Some(p),..}=&mut t.geometry else{panic!()};*content="HAM42".into();layout.letter_spacing_mm=0.;p.anchor=TextAnchor::Middle;p.start_offset_mm=30.;doc
    }
    fn pack_centered(mut n:NativeRecords)->Vec<u8>{if n.version==2100{repack21(&n).unwrap()}else{n.members.insert("content/riffData.cdr".into(),write(&n.nodes).unwrap());zip(&n.members.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>())}}
    #[test]fn centered_square_top_uses_a_closed_quadrant_and_recovers_without_source_notes(){
        use super::super::binary::*;
        for modern in[false,true]{for baseline in[0.,0.125]{
            let mut doc=centered_square();let t=doc.objects.iter_mut().find(|o|o.id=="content").unwrap();let Geometry::Text{text_path:Some(p),..}=&mut t.geometry else{panic!()};p.baseline_offset_mm=baseline;
            let options=encoded_options(&doc,doc.object("content").unwrap()).unwrap().unwrap();assert_eq!(u32_at(&options,28).unwrap(),1);assert_eq!(i32_at(&options,16).unwrap(),-100000);
            let bytes=if modern{crate::cdr::encode_document21(&doc)}else{crate::cdr::encode_document(&doc)}.unwrap().0;assert_eq!(crate::cdr::decode(&bytes,&doc.id).unwrap().0,doc);
            let mut native=read_native(&bytes).unwrap();native.members.remove("metadata/metadata.xml");native.members.remove("META-INF/metadata.xml");let decoded=crate::cdr::decode(&pack_centered(native),&doc.id).unwrap().0;
            let Geometry::Text{text_path:Some(binding),..}=&decoded.object("content").unwrap().geometry else{panic!()};assert_eq!(binding.anchor,TextAnchor::Middle);assert_eq!(binding.start_offset_mm,30.);assert_eq!(binding.baseline_offset_mm,baseline);
            let (a,b)=(doc.object("content").unwrap(),decoded.object("content").unwrap());
            assert!(super::super::path_precision::same_world(&doc.object_path(a).unwrap(),doc.world_transform(a).unwrap(),&decoded.object_path(b).unwrap(),decoded.world_transform(b).unwrap()),"native options, without source Notes, must preserve this bounded model's glyph geometry");
        }}
    }
    #[test]fn a_real_top_offset_edit_overrides_the_old_centered_source_scalar(){
        use super::super::binary::*;
        for modern in[false,true]{let doc=centered_square();let bytes=if modern{crate::cdr::encode_document21(&doc)}else{crate::cdr::encode_document(&doc)}.unwrap().0;let mut raw=read_native(&bytes).unwrap();
            fn edit(nodes:&mut[Node],modern:bool)->bool{for n in nodes{if n.tag==*b"txsm"{let at=if modern{101}else{96};set32(&mut n.data,at+16,(-90000i32)as u32).unwrap();return true;}if let Some(c)=&mut n.children{if edit(c,modern){return true;}}}false}
            assert!(edit(&mut raw.nodes,modern));let decoded=crate::cdr::decode(&pack_centered(raw),&doc.id).unwrap().0;let Geometry::Text{text_path:Some(binding),..}=&decoded.object("content").unwrap().geometry else{panic!()};assert_eq!(binding.start_offset_mm,31.);assert_eq!(binding.anchor,TextAnchor::Middle);
        }
    }
    #[test]fn closed_top_reference_rejects_unmeasured_contours(){
        let square=crate::path::parse("M0 0L80 0L80 80L0 80Z").unwrap();assert_eq!(square_top_center(&square),Some((40.,80.)));
        for d in["M0 0L80 0L80 60L0 60Z","M80 0L80 80L0 80L0 0Z","M0 0L0 80L80 80L80 0Z","M0 0C40 0 80 40 80 80Z"]{assert_eq!(square_top_center(&crate::path::parse(d).unwrap()),None);}
        for anchor in[TextAnchor::Start,TextAnchor::End]{let mut doc=centered_square();let t=doc.objects.iter_mut().find(|o|o.id=="content").unwrap();let Geometry::Text{text_path:Some(p),..}=&mut t.geometry else{panic!()};p.anchor=anchor;let raw=encoded_options(&doc,doc.object("content").unwrap()).unwrap().unwrap();assert_eq!(Options::parse(&raw).unwrap().anchor,TextAnchor::Start);for encode in[crate::cdr::encode_document,crate::cdr::encode_document21]{let bytes=encode(&doc).unwrap().0;let actual=crate::cdr::decode(&bytes,"source-anchor").unwrap().0;assert_eq!(actual.objects,doc.objects);}}
    }
    #[test]fn symmetric_closed_cubic_top_reference_and_native_offset_edits_are_semantic(){
        let curve="M60 20C82 20 100 38 100 60C100 82 82 100 60 100C38 100 20 82 20 60C20 38 38 20 60 20Z";
        assert_eq!(closed_top_reference(&crate::path::parse(curve).unwrap()).unwrap().0,0.);
        let changed=curve.replacen("82 20","82.0001 20",1);assert!(closed_top_reference(&crate::path::parse(&changed).unwrap()).is_none());
        for anchor in[TextAnchor::Start,TextAnchor::Middle,TextAnchor::End]{for encode in[crate::cdr::encode_document,crate::cdr::encode_document21]{
            let mut doc=centered_square();doc.objects.iter_mut().find(|o|o.id=="guide").unwrap().geometry=Geometry::Path{d:curve.into(),fill_rule:Default::default()};
            let Geometry::Text{text_path:Some(p),..}=&mut doc.objects.iter_mut().find(|o|o.id=="content").unwrap().geometry else{panic!()};p.anchor=anchor;
            let bytes=encode(&doc).unwrap().0;assert_eq!(crate::cdr::decode(&bytes,"curve-source").unwrap().0.objects,doc.objects);
            let mut records=read_native(&bytes).unwrap();records.members.remove("metadata/metadata.xml");records.members.remove("META-INF/metadata.xml");
            let actual=crate::cdr::decode(&pack_centered(records),"bare-curve").unwrap().0;
            let Geometry::Text{text_path:Some(binding),..}=&actual.object("content").unwrap().geometry else{panic!()};assert_eq!(binding.anchor,TextAnchor::Middle);
            let original=Options::parse(&encoded_options(&doc,doc.object("content").unwrap()).unwrap().unwrap()).unwrap();assert_eq!(binding.start_offset_mm,original.offset);assert_eq!(binding.baseline_offset_mm,0.125);
        }}
    }
    #[test]
    fn tiny_independent_path_story_deformations_are_never_erased_by_a_coefficient_epsilon(){
        let identity=[1.,0.,-850000.,0.,1.,1115000.];
        check_frame(identity,identity).unwrap();let mut signed=identity;signed[1]= -0.;check_frame(signed,identity).unwrap();
        for i in 0..6{let mut changed=identity;changed[i]+=2f64.powi(-22);assert!(check_frame(changed,identity).is_err(),"coefficient {i}");}
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports/windows/WIN-SAVE-CLOSURE-20261024-01/controls");
        for version in [14,21]{for coefficient in [0usize,1,3,4]{
            let source=std::fs::read(root.join(format!("path-control-v{version}.cdr"))).unwrap();let mut native=read_native(&source).unwrap();
            fn edit(nodes:&mut[Node],version:u32,coefficient:usize)->bool{
                for n in nodes{if n.tag==*b"txsm"{let start=if version==14{44}else{49};let at=start+coefficient*8;let value=f64_at(&n.data,at).unwrap()+2f64.powi(-22);n.data[at..at+8].copy_from_slice(&value.to_le_bytes());return true;}if let Some(c)=&mut n.children{if edit(c,version,coefficient){return true;}}}false
            }
            assert!(edit(&mut native.nodes,version,coefficient));let bytes=if version==14{write(&native.nodes).unwrap()}else{write21(&native.nodes,native.members).unwrap()};assert!(crate::cdr::decode(&bytes,"independent-story").is_err(),"{version}/{coefficient}");
        }}
    }
    fn similarity_source()->Document{
        serde_json::from_value(serde_json::json!({"id":"parent-native","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"parent","page_id":"page-1","x_mm":3.,"y_mm":4.,"width_mm":0.,"height_mm":0.,"transform":[0.,2.,-2.,0.,0.,0.],"geometry":{"kind":"group"}},{"id":"guide","page_id":"page-1","x_mm":10.,"y_mm":20.,"width_mm":110.,"height_mm":20.,"geometry":{"kind":"path","d":"M0 0 C30 10 70 -10 100 0"}},{"id":"text","page_id":"page-1","parent_id":"parent","x_mm":0.,"y_mm":0.,"width_mm":80.,"height_mm":25.,"geometry":{"kind":"text","content":"Live 42","font_family":if cfg!(target_os="macos"){"PingFang SC"}else{"Microsoft YaHei"},"font_size_mm":4.,"text_path":{"path_id":"guide","start_offset_mm":3.,"baseline_offset_mm":0.125,"anchor":"start"}}}]})).unwrap()
    }
    fn translated_source(parent:bool,direction:u8)->Document{
        let mut d=similarity_source();if !parent{d.objects[2].parent_id=None;}
        let guide=&mut d.objects[1];guide.geometry=Geometry::Path{d:match direction{0=>"M0 0L100 0",1=>"M0 0L0 100",2=>"M0 0L60 80",_=>"M100 0L0 0"}.into(),fill_rule:Default::default()};
        let text=&mut d.objects[2];text.x_mm=0.25;text.y_mm= -0.5;text.transform=[1.,0.,0.,1.,0.75,1.];
        let Geometry::Text{text_path:Some(p),..}=&mut text.geometry else{unreachable!()};p.start_offset_mm=20.;p.baseline_offset_mm=0.125;d
    }
    #[test]
    fn straight_guide_translation_preserves_source_and_bare_native_live_glyphs(){
        for parent in [false,true]{for direction in 0..4{for version in[14,21]{
            let source=translated_source(parent,direction);let encode=if version==14{crate::cdr::encode_document}else{crate::cdr::encode_document21};let raw=encode(&source).unwrap().0;let decoded=crate::cdr::decode(&raw,&source.id).unwrap().0;assert_eq!(decoded,source,"parent{parent}, direction{direction}, version{version}");
            let second=encode(&decoded).unwrap().0;assert_eq!(crate::cdr::decode(&second,&source.id).unwrap().0,source);
            let mut native=read_native(&raw).unwrap();native.members.remove("metadata/metadata.xml");native.members.remove("META-INF/metadata.xml");let raw=if version==21{repack21(&native).unwrap()}else{native.members.insert("content/riffData.cdr".into(),write(&native.nodes).unwrap());zip(&native.members.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>())};let bare=crate::cdr::decode(&raw,"bare").unwrap().0;
            assert_eq!(bare.objects.len(),3);let(a,b)=(bare.object("guide").unwrap(),source.object("guide").unwrap());assert!(super::super::path_precision::same_world(&a.local_path().unwrap(),bare.world_transform(a).unwrap(),&b.local_path().unwrap(),source.world_transform(b).unwrap()));same_glyph_world(&source,&bare);
        }}}
    }
    #[test]
    fn translated_fit_rejects_curves_endpoint_crossing_and_any_own_linear_deformation(){
        let mut d=translated_source(false,0);d.objects[1].geometry=Geometry::Path{d:"M0 0C30 10 70 -10 100 0".into(),fill_rule:Default::default()};assert!(native_object(&d,&d.objects[2]).is_err());
        d=translated_source(false,0);d.objects[2].transform[4]= -30.;assert!(native_object(&d,&d.objects[2]).is_err());
        for i in 0..4{d=translated_source(false,0);d.objects[2].transform[i]+=2f64.powi(-30);assert!(native_object(&d,&d.objects[2]).is_err());}
    }
    #[test]
    fn translated_parent_keeps_scaled_text_outline_width(){
        {let mut d=translated_source(true,0);d.objects[2].style.stroke="#123456".into();d.objects[2].style.stroke_width_mm=0.125;
            let n=native_object(&d,&d.objects[2]).unwrap();assert_eq!(n.style.stroke_width_mm,0.25);
            for version in[14,21]{let encode=if version==14{crate::cdr::encode_document}else{crate::cdr::encode_document21};let raw=encode(&d).unwrap().0;let actual=crate::cdr::decode(&raw,&d.id).unwrap().0;assert_eq!(actual,d);}
        }
    }
    #[test]
    fn fine_translation_and_original_clip_survive_both_native_versions(){
        for parent in[false,true]{for version in[14,21]{let mut source=translated_source(parent,2);let text=&mut source.objects[2];text.x_mm=0.1234567;text.y_mm=-0.7654321;text.clip=Some(Clip::Rect{rect_mm:[-100.,-100.,250.,250.]});
            let encode=if version==14{crate::cdr::encode_document}else{crate::cdr::encode_document21};let raw=encode(&source).unwrap().0;let first=crate::cdr::decode(&raw,&source.id).unwrap().0;assert_eq!(first,source);
            let second=crate::cdr::decode(&encode(&first).unwrap().0,&source.id).unwrap().0;assert_eq!(second,source);
        }}
    }
    #[test]
    fn a_single_native_fit_word_edit_disables_old_placement_without_losing_the_edit(){
        for parent in[false,true]{for version in[14,21]{
            let source=translated_source(parent,0);let encode=if version==14{crate::cdr::encode_document}else{crate::cdr::encode_document21};let mut native=read_native(&encode(&source).unwrap().0).unwrap();
            fn edit(nodes:&mut[Node],at:usize){for n in nodes{if n.tag==*b"txsm"{let word=i32_at(&n.data,at).unwrap();set32(&mut n.data,at,(word+1)as u32).unwrap();}if let Some(c)=&mut n.children{edit(c,at);}}}edit(&mut native.nodes,if version==21{117}else{112});
            let raw=if version==21{repack21(&native).unwrap()}else{native.members.insert("content/riffData.cdr".into(),write(&native.nodes).unwrap());zip(&native.members.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>())};
            let actual=crate::cdr::decode(&raw,"native").unwrap().0;let text=actual.object("text").unwrap();assert_eq!(text.x_mm,0.);assert_eq!(text.y_mm,0.);assert_eq!(text.transform,Affine::IDENTITY.as_coeffs());
            let mut expected=Options::parse(&encoded_options(&source,source.object("text").unwrap()).unwrap().unwrap()).unwrap();expected.offset+=0.0001;assert_eq!(encoded_options(&actual,text).unwrap().unwrap(),expected.encode().unwrap(),"one native coordinate unit must survive source restoration");
            native.members.remove("metadata/metadata.xml");native.members.remove("META-INF/metadata.xml");let bare=if version==21{repack21(&native).unwrap()}else{native.members.insert("content/riffData.cdr".into(),write(&native.nodes).unwrap());zip(&native.members.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>())};same_glyph_world(&actual,&crate::cdr::decode(&bare,"bare").unwrap().0);
        }}
    }
    #[test]
    fn an_actual_guide_direction_edit_keeps_its_native_fit_instead_of_reusing_old_translation(){
        for version in[14,21]{let source=translated_source(true,0);let encode=if version==14{crate::cdr::encode_document}else{crate::cdr::encode_document21};let mut native=read_native(&encode(&source).unwrap().0).unwrap();
            fn edit(nodes:&mut[Node]){for n in nodes{if n.kind()==Some(b"obj "){
                let(kind,mut args)=super::super::write::arguments(&super::super::read::own_record(n,b"loda").unwrap().data).unwrap();
                if args.iter().any(|(k,v)|*k==1000&&read_wide(v).unwrap()=="guide"){args.iter_mut().find(|(k,_)|*k==30).unwrap().1=super::super::write::path_geometry(&crate::path::parse("M0 0L80 60").unwrap()).unwrap();n.find_mut(b"loda").unwrap().data=super::super::write::loda(kind,&args);}
            }if let Some(c)=&mut n.children{edit(c);}}}edit(&mut native.nodes);
            let raw=if version==21{repack21(&native).unwrap()}else{native.members.insert("content/riffData.cdr".into(),write(&native.nodes).unwrap());zip(&native.members.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>())};let actual=crate::cdr::decode(&raw,"changed").unwrap().0;
            assert_eq!(actual.object("text").unwrap().transform,Affine::IDENTITY.as_coeffs());assert_eq!(actual.object("text").unwrap().x_mm,0.);
            native.members.remove("metadata/metadata.xml");native.members.remove("META-INF/metadata.xml");let raw=if version==21{repack21(&native).unwrap()}else{native.members.insert("content/riffData.cdr".into(),write(&native.nodes).unwrap());zip(&native.members.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>())};same_glyph_world(&actual,&crate::cdr::decode(&raw,"bare").unwrap().0);
        }
    }
    fn uniform_source(parent:bool,direction:u8,scale:f64,anchor:TextAnchor)->Document{
        let mut d=translated_source(parent,direction);d.objects[1].geometry=Geometry::Path{d:match direction{0=>"M0 0L120 0",2=>"M0 0L72 96",_=>"M120 0L0 0"}.into(),fill_rule:Default::default()};let text=d.object("text").unwrap().clone();let p=if parent{d.world_transform(d.object("parent").unwrap()).unwrap()}else{Affine::IDENTITY};let guide=d.object("guide").unwrap();let first=guide.local_path().unwrap().elements()[0].end_point().unwrap();let pivot=d.world_transform(guide).unwrap()*first;
        let[x,y,u,v,px,py]=p.as_coeffs();let delta=kurbo::Vec2::new((1.-scale)*(pivot.x-px)+1.,(1.-scale)*(pivot.y-py)+0.5);let inverse=Affine::new([x,y,u,v,0.,0.]).inverse()*kurbo::Point::new(delta.x,delta.y);
        let t=d.objects.iter_mut().find(|o|o.id=="text").unwrap();t.transform=[scale,0.,0.,scale,inverse.x-scale*text.x_mm,inverse.y-scale*text.y_mm];
        let Geometry::Text{text_path:Some(binding),layout,..}=&mut t.geometry else{panic!()};binding.anchor=anchor;layout.letter_spacing_mm=0.125;layout.line_height_mm=Some(5.25);d
    }
    fn pack_native(n:&mut NativeRecords,version:u16)->Vec<u8>{if version==21{repack21(n).unwrap()}else{n.members.insert("content/riffData.cdr".into(),write(&n.nodes).unwrap());zip(&n.members.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>())}}
    #[test]
    fn uniform_own_scale_keeps_source_and_bare_live_glyphs_for_three_anchors_and_parent_origins(){
        for parent in[false,true]{for direction in[0,2,3]{for anchor in[TextAnchor::Start,TextAnchor::Middle,TextAnchor::End]{for scale in[0.5,1.5]{for version in[14,21]{
            let source=uniform_source(parent,direction,scale,anchor);let encode=if version==14{crate::cdr::encode_document}else{crate::cdr::encode_document21};let bytes=encode(&source).unwrap().0;let actual=crate::cdr::decode(&bytes,&source.id).unwrap().0;assert_eq!(actual,source,"parent{parent} direction{direction} scale{scale} anchor{anchor:?} v{version}");assert_eq!(crate::cdr::decode(&encode(&actual).unwrap().0,&source.id).unwrap().0,source);
            let mut n=read_native(&bytes).unwrap();n.members.remove("metadata/metadata.xml");n.members.remove("META-INF/metadata.xml");let bare=crate::cdr::decode(&pack_native(&mut n,version),"bare").unwrap().0;same_glyph_world(&source,&bare);assert_eq!(bare.objects.len(),3);let(a,b)=(source.object("guide").unwrap(),bare.object("guide").unwrap());assert!(super::super::path_precision::same_world(&a.local_path().unwrap(),source.world_transform(a).unwrap(),&b.local_path().unwrap(),bare.world_transform(b).unwrap()));
        }}}}}
    }
    #[test]
    fn monotone_straight_bezier_and_multisegment_fit_keeps_all_native_nodes_and_live_text(){
        for d in["M0 0L30 0L120 0","M0 0C20 0 80 0 120 0","M0 0Q60 0 120 0","M120 0C90 0 60 0 30 0L0 0","M0 0L18 24C54 72 36 48 72 96","M0 0C80 0 40 0 120 0"]{for parent in[false,true]{for scale in[1.,1.5]{for version in[14,21]{
            let direction=if d.starts_with("M120"){3}else if d.contains("24"){2}else{0};let mut source=uniform_source(parent,direction,scale,TextAnchor::Middle);source.objects[1].geometry=Geometry::Path{d:d.into(),fill_rule:Default::default()};source.objects[2].style.stroke="#123456".into();source.objects[2].style.stroke_width_mm=0.125;
            let encode=if version==14{crate::cdr::encode_document}else{crate::cdr::encode_document21};let raw=encode(&source).unwrap().0;let actual=crate::cdr::decode(&raw,&source.id).unwrap().0;assert_eq!(actual,source,"{d} parent{parent} scale{scale} v{version}");assert_eq!(crate::cdr::decode(&encode(&actual).unwrap().0,&source.id).unwrap().0,source);
            let bare=super::super::read::decode_geometry(&raw,"native").unwrap().0;same_glyph_world(&source,&bare);let guide=bare.object("guide").unwrap();assert_eq!(guide.local_path().unwrap().elements().len(),source.objects[1].local_path().unwrap().elements().len(),"guide node structure must remain editable");assert_eq!(bare.objects.len(),3);let projected=super::super::path_precision::projected(d,1).unwrap();assert!(super::super::path_precision::same_world(&guide.local_path().unwrap(),bare.world_transform(guide).unwrap(),&projected,source.world_transform(&source.objects[1]).unwrap()),"{d}");
        }}}}
    }
    #[test]
    fn transformed_fit_never_flattens_a_bent_reversing_or_closed_multisegment_guide(){
        for d in["M0 0L60 0.000001L120 0","M0 0C20 1 80 -1 120 0","M0 0L80 0L40 0L120 0","M0 0C160 0 -40 0 120 0","M0 0L60 0L120 0Z","M0 0L60 0M60 0L120 0"]{
            let mut source=uniform_source(false,0,1.5,TextAnchor::Start);source.objects[1].geometry=Geometry::Path{d:d.into(),fill_rule:Default::default()};
            for encode in[crate::cdr::encode_document,crate::cdr::encode_document21]{assert!(encode(&source).is_err(),"{d}");}
        }
    }
    #[test]
    fn actual_multinode_guide_bend_keeps_native_fit_instead_of_restoring_old_own_scale(){
        for parent in[false,true]{for version in[14,21]{
            let mut source=uniform_source(parent,0,1.5,TextAnchor::Middle);source.objects[1].geometry=Geometry::Path{d:"M0 0L30 0L120 0".into(),fill_rule:Default::default()};source.objects[2].style.stroke="#123456".into();source.objects[2].style.stroke_width_mm=0.125;
            let encode=if version==14{crate::cdr::encode_document}else{crate::cdr::encode_document21};let original=read_native(&encode(&source).unwrap().0).unwrap();
            let mut edited=super::super::write::normalized(&source).unwrap();edited.objects[1].geometry=Geometry::Path{d:"M0 0L30 0.0001L120 0".into(),fill_rule:Default::default()};let mut native=read_native(&encode(&edited).unwrap().0).unwrap();
            for name in["metadata/metadata.xml","META-INF/metadata.xml"]{if let Some(raw)=original.members.get(name){native.members.insert(name.into(),raw.clone());}else{native.members.remove(name);}}
            let raw=pack_native(&mut native,version);let actual=crate::cdr::decode(&raw,"actual").unwrap().0;let bare=super::super::read::decode_geometry(&raw,"bare").unwrap().0;same_glyph_world(&actual,&bare);assert_eq!(actual.object("text").unwrap().transform,Affine::IDENTITY.as_coeffs());assert_ne!(actual.object("guide").unwrap().geometry,source.objects[1].geometry);
            let(a,b)=(actual.object("guide").unwrap(),bare.object("guide").unwrap());assert!(super::super::path_precision::same_world(&a.local_path().unwrap(),actual.world_transform(a).unwrap(),&b.local_path().unwrap(),bare.world_transform(b).unwrap()));
        }}
    }
    #[test]
    fn uniform_source_translation_conjugates_the_parent_origin_and_preserves_clips(){
        for parent in[false,true]{let mut source=uniform_source(parent,0,1.5,TextAnchor::Start);source.objects[2].clip=Some(Clip::Rect{rect_mm:[-100.,-100.,250.,250.]});let text=source.object("text").unwrap();let delta=translation_world(&source,text).unwrap().unwrap();let world=source.world_transform(text).unwrap();let parent=text.parent_id.as_ref().map(|id|source.world_transform(source.object(id).unwrap()).unwrap()).unwrap_or(Affine::IDENTITY);
            for point in[(0.,0.),(10.,20.),(-3.,7.)]{let p=kurbo::Point::from(point);let a=world*parent.inverse()*p;let b=kurbo::Point::new(1.5*p.x+delta[0],1.5*p.y+delta[1]);assert!((a.x-b.x).abs()<1e-12&&(a.y-b.y).abs()<1e-12);}
            for encode in[crate::cdr::encode_document,crate::cdr::encode_document21]{let raw=encode(&source).unwrap().0;assert_eq!(crate::cdr::decode(&raw,&source.id).unwrap().0,source);}
        }
    }
    #[test]
    fn actual_native_fit_and_style_edits_disable_old_own_scale_as_one_chart(){
        for parent in[false,true]{for mode in 0..6{for version in[14,21]{let source=uniform_source(parent,0,1.5,TextAnchor::Start);let encode=if version==14{crate::cdr::encode_document}else{crate::cdr::encode_document21};let original=read_native(&encode(&source).unwrap().0).unwrap();let mut edited=super::super::write::normalized(&source).unwrap();let t=edited.objects.iter_mut().find(|o|o.id=="text").unwrap();let Geometry::Text{font_family,font_size_mm,layout,text_path:Some(binding),..}=&mut t.geometry else{panic!()};match mode{0=>*font_size_mm+=0.0001,1=>layout.letter_spacing_mm+=0.01,2=>layout.line_height_mm=Some(layout.line_height_mm.unwrap()+0.01),3=>binding.start_offset_mm+=0.0001,4=>*font_family="Arial".into(),_=>{t.style.stroke="#123456".into();t.style.stroke_width_mm=0.2;}}
            let mut native=read_native(&encode(&edited).unwrap().0).unwrap();for name in["metadata/metadata.xml","META-INF/metadata.xml"]{if let Some(raw)=original.members.get(name){native.members.insert(name.into(),raw.clone());}else{native.members.remove(name);}}
            let actual=crate::cdr::decode(&pack_native(&mut native,version),"edited").unwrap().0;assert_eq!(actual.object("text").unwrap().transform,Affine::IDENTITY.as_coeffs(),"parent{parent} mode{mode} v{version}");assert_eq!(actual.object("text").unwrap().x_mm,0.);native.members.remove("metadata/metadata.xml");native.members.remove("META-INF/metadata.xml");let bare=crate::cdr::decode(&pack_native(&mut native,version),"bare").unwrap().0;same_glyph_world(&actual,&bare);
        }}}
    }
    #[test]
    fn fine_uniform_scales_preserve_source_without_using_notes_as_glyph_geometry(){
        for own in[1.1234567,1.0000001]{for parent in[false,true]{for version in[14,21]{let source=uniform_source(parent,0,own,TextAnchor::Middle);let encode=if version==14{crate::cdr::encode_document}else{crate::cdr::encode_document21};let raw=encode(&source).unwrap().0;assert_eq!(crate::cdr::decode(&raw,&source.id).unwrap().0,source);let mut native=read_native(&raw).unwrap();native.members.remove("metadata/metadata.xml");native.members.remove("META-INF/metadata.xml");let bare=crate::cdr::decode(&pack_native(&mut native,version),"bare").unwrap().0;same_glyph_world(&source,&bare);}}}
    }
    #[test]
    fn uniform_scale_rejects_curves_hairline_reflection_nonuniformity_and_endpoint_crossings(){
        for mode in 0..6{let mut source=uniform_source(false,0,1.5,TextAnchor::Start);match mode{0=>source.objects[1].geometry=Geometry::Path{d:"M0 0C30 10 70 -10 100 0".into(),fill_rule:Default::default()},1=>{source.objects[2].style.stroke="#000000".into();source.objects[2].style.stroke_width_mm=0.;},2=>source.objects[2].transform[0]= -1.5,3=>source.objects[2].transform[3]+=1e-12,4=>source.objects[2].transform[1]=1e-20,_=>source.objects[2].transform[4]+=300.}assert!(crate::cdr::encode_document(&source).is_err(),"mode{mode}");assert!(crate::cdr::encode_document21(&source).is_err(),"mode{mode}");}
    }
    #[test]
    fn outlined_uniform_scale_keeps_fine_source_width_and_bare_glyph_pen_hulls(){
        for parent in[false,true]{for own in[0.5,1.5]{for width in[0.125,0.1234567]{for anchor in[TextAnchor::Start,TextAnchor::Middle,TextAnchor::End]{for version in[14,21]{
            let mut source=uniform_source(parent,0,own,anchor);source.objects[2].style.stroke="#123456".into();source.objects[2].style.stroke_width_mm=width;
            if parent&&anchor==TextAnchor::End{source.objects[2].clip=Some(Clip::Rect{rect_mm:[-100.,-100.,250.,250.]});}
            let encode=if version==14{crate::cdr::encode_document}else{crate::cdr::encode_document21};let raw=encode(&source).unwrap().0;let restored=crate::cdr::decode(&raw,&source.id).unwrap().0;assert_eq!(restored,source,"parent{parent} own{own} width{width} anchor{anchor:?} v{version}");assert_eq!(crate::cdr::decode(&encode(&restored).unwrap().0,&source.id).unwrap().0,source);
            let bare=super::super::read::decode_geometry(&raw,"native").unwrap().0;assert!(super::super::path_outline::preserves(&source,source.object("text").unwrap(),&bare,bare.object("text").unwrap()).unwrap());
        }}}}}
    }
    #[test]
    fn actual_native_outline_width_color_removal_and_font_edits_keep_their_physical_pen(){
        for parent in[false,true]{for mode in 0..4{for version in[14,21]{let mut source=uniform_source(parent,0,1.5,TextAnchor::Start);source.objects[2].style.stroke="#123456".into();source.objects[2].style.stroke_width_mm=0.125;let encode=if version==14{crate::cdr::encode_document}else{crate::cdr::encode_document21};let original=read_native(&encode(&source).unwrap().0).unwrap();let mut edited=super::super::write::normalized(&source).unwrap();let t=edited.objects.iter_mut().find(|o|o.id=="text").unwrap();match mode{0=>t.style.stroke_width_mm+=0.0001,1=>t.style.stroke="#abcdef".into(),2=>{t.style.stroke="none".into();t.style.stroke_width_mm=0.;},_=>{let Geometry::Text{font_size_mm,..}=&mut t.geometry else{panic!()};*font_size_mm+=0.0001;}}
            let mut native=read_native(&encode(&edited).unwrap().0).unwrap();for name in["metadata/metadata.xml","META-INF/metadata.xml"]{if let Some(raw)=original.members.get(name){native.members.insert(name.into(),raw.clone());}else{native.members.remove(name);}}
            let bytes=pack_native(&mut native,version);let actual=crate::cdr::decode(&bytes,"actual").unwrap().0;let bare=super::super::read::decode_geometry(&bytes,"bare").unwrap().0;let(a,b)=(actual.object("text").unwrap(),bare.object("text").unwrap());assert_eq!(a.style.stroke,b.style.stroke);assert!(super::super::path_outline::preserves(&actual,a,&bare,b).unwrap(),"mode{mode} parent{parent} v{version}");let aw=a.style.stroke_width_mm*similarity_scale(actual.world_transform(a).unwrap()).unwrap();let bw=b.style.stroke_width_mm*similarity_scale(bare.world_transform(b).unwrap()).unwrap();assert!((aw-bw).abs()<1e-12,"a native pen edit must not be replaced by an old width");
            if mode==0{assert_ne!(a.style.stroke_width_mm,source.objects[2].style.stroke_width_mm);}if mode==2{assert_eq!(a.style.stroke,"none");assert_eq!(a.style.stroke_width_mm,0.);}if mode==3{assert_eq!(a.transform,Affine::IDENTITY.as_coeffs());}
        }}}
    }
    #[test]
    fn glyph_displacement_and_pen_error_share_one_bound_and_bad_export_is_rejected(){
        let mut a=uniform_source(false,0,1.5,TextAnchor::Start);a.objects[2].style.stroke="#123456".into();a.objects[2].style.stroke_width_mm=0.125;let a=super::super::write::normalized(&a).unwrap();let index=a.objects.iter().position(|o|o.id=="text").unwrap();
        let mut moved=a.clone();moved.objects[index].transform[4]+=0.00015;assert!(super::super::path_outline::preserves(&a,&a.objects[index],&moved,&moved.objects[index]).unwrap());
        let mut wider=a.clone();wider.objects[index].style.stroke_width_mm+=0.00002;assert!(super::super::path_outline::preserves(&a,&a.objects[index],&wider,&wider.objects[index]).unwrap());
        moved.objects[index].style.stroke_width_mm+=0.00002;assert!(!super::super::path_outline::preserves(&a,&a.objects[index],&moved,&moved.objects[index]).unwrap());
        let mut source=uniform_source(false,0,1.5,TextAnchor::Start);source.objects[2].transform[4]+=0.0000499;source.objects[2].style.stroke="#123456".into();source.objects[2].style.stroke_width_mm=0.1875499/1.5;
        for modern in[false,true]{let unchecked=super::super::paint_layers::encode(&source,modern).unwrap().0;let actual=crate::cdr::decode(&unchecked,"actual").unwrap().0;let bare=super::super::read::decode_geometry(&unchecked,"bare").unwrap().0;assert_eq!(actual.object("text").unwrap().transform,Affine::IDENTITY.as_coeffs());assert!(super::super::path_outline::preserves(&actual,actual.object("text").unwrap(),&bare,bare.object("text").unwrap()).unwrap());let result=if modern{crate::cdr::encode_document21(&source)}else{crate::cdr::encode_document(&source)};let error=result.unwrap_err();assert!(error.message.contains("joint 0.00021"),"{error:?}");}
    }
    #[test]
    fn outlined_glyph_bound_keeps_lossless_cmyk_catalogue_in_a_mixed_document(){
        let mut source=uniform_source(true,0,1.5,TextAnchor::Start);source.objects[2].style.stroke="#123456".into();source.objects[2].style.stroke_width_mm=0.1234567;
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/assets/cdr/native-cmyk");let raw=std::fs::read(root.join("cmyk.jpg")).unwrap();source.resources.insert("cmyk".into(),crate::Resource::image(&raw).unwrap());let mut image=source.objects[1].clone();image.id="image".into();image.parent_id=None;image.x_mm=130.;image.y_mm=50.;image.width_mm=30.;image.height_mm=20.;image.geometry=Geometry::Image{resource_id:"cmyk".into(),fit:crate::ImageFit::Stretch,clip_mm:None};image.style=Default::default();source.objects.push(image);
        for encode in[crate::cdr::encode_document,crate::cdr::encode_document21]{let bytes=encode(&source).unwrap().0;assert_eq!(crate::cdr::decode(&bytes,&source.id).unwrap().0,source);let actual=super::super::read::decode_geometry(&bytes,"native").unwrap().0;assert_eq!(actual.resources["cmyk"],source.resources["cmyk"]);let scene=super::super::path_outline::scene(&actual,"text").unwrap();assert!(scene.resources.is_empty());assert!(scene.objects.iter().all(|o|!matches!(o.geometry,Geometry::Image{..})));assert_eq!(scene.world_transform(scene.object("text").unwrap()).unwrap(),actual.world_transform(actual.object("text").unwrap()).unwrap());assert!(super::super::path_outline::preserves(&actual,actual.object("text").unwrap(),&source,source.object("text").unwrap()).unwrap());}
    }
    #[test]
    fn native_zero_width_outline_keeps_actual_native_semantics_instead_of_old_width(){
        for version in[14,21]{let mut source=uniform_source(false,0,1.5,TextAnchor::Start);source.objects[2].style.stroke="#123456".into();source.objects[2].style.stroke_width_mm=0.125;let encode=if version==14{crate::cdr::encode_document}else{crate::cdr::encode_document21};let old=read_native(&encode(&source).unwrap().0).unwrap();let mut model=super::super::write::normalized(&source).unwrap();model.objects[2].style.stroke_width_mm=0.;let mut native=read_native(&encode(&model).unwrap().0).unwrap();for key in["metadata/metadata.xml","META-INF/metadata.xml"]{if let Some(raw)=old.members.get(key){native.members.insert(key.into(),raw.clone());}else{native.members.remove(key);}}let bytes=pack_native(&mut native,version);let actual=crate::cdr::decode(&bytes,"actual").unwrap().0;let reference=super::super::read::decode_geometry(&bytes,"reference").unwrap().0;assert_eq!(actual.object("text").unwrap().style.stroke_width_mm,0.);assert_eq!(actual.object("text").unwrap().style.stroke,reference.object("text").unwrap().style.stroke);if reference.object("text").unwrap().style.stroke!="none"{assert_eq!(actual.object("text").unwrap().transform,Affine::IDENTITY.as_coeffs());}same_glyph_world(&actual,&reference);}
    }
    #[test]
    fn outlined_uniform_source_with_nonbinary_parent_scale_stays_source_exact(){
        let mut source=uniform_source(true,0,1.1,TextAnchor::Middle);source.objects[0].transform=[0.,1.7,-1.7,0.,0.,0.];source.objects[2].style.stroke="#123456".into();source.objects[2].style.stroke_width_mm=0.0625;
        for encode in[crate::cdr::encode_document,crate::cdr::encode_document21]{let bytes=encode(&source).unwrap().0;let restored=crate::cdr::decode(&bytes,&source.id).unwrap().0;assert_eq!(restored,source);}
    }
    fn same_glyph_world(a:&Document,b:&Document){
        let(x,y)=(a.object("text").unwrap(),b.object("text").unwrap());
        let(ap,bp)=(a.object_path(x).unwrap(),b.object_path(y).unwrap());let(am,bm)=(a.world_transform(x).unwrap(),b.world_transform(y).unwrap());
        let same=super::super::path_precision::same_world(&ap,am,&bp,bm);
        if !same {if let Some(directory)=std::env::var_os("ARRANGER_PATH_TEXT_DIAGNOSTICS") {
            let directory=std::path::PathBuf::from(directory);std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join("restored-document.json"),serde_json::to_vec_pretty(a).unwrap()).unwrap();
            std::fs::write(directory.join("native-reference-document.json"),serde_json::to_vec_pretty(b).unwrap()).unwrap();
            std::fs::write(directory.join("restored-world.svgd"),(am*ap.clone()).to_svg()).unwrap();
            std::fs::write(directory.join("native-reference-world.svgd"),(bm*bp.clone()).to_svg()).unwrap();
        }}
        assert!(same,"actual glyph controls must stay in the same world positions; elements={}/{}; matrices={am:?}/{bm:?}; text={x:?}/{y:?}",ap.elements().len(),bp.elements().len());
    }
    #[test]
    fn similarity_parent_is_real_native_font_and_guide_without_notes(){
        let source=similarity_source();for version in [14,21]{let bytes=if version==14{crate::cdr::encode_document(&source)}else{crate::cdr::encode_document21(&source)}.unwrap().0;let mut native=read_native(&bytes).unwrap();native.members.remove("metadata/metadata.xml");native.members.remove("META-INF/metadata.xml");let bytes=if version==21{repack21(&native).unwrap()}else{native.members.insert("content/riffData.cdr".into(),write(&native.nodes).unwrap());zip(&native.members.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>())};let actual=crate::cdr::decode(&bytes,"bare").unwrap().0;
            assert_eq!(actual.objects.len(),3,"no copied or hidden guide");let Geometry::Text{font_size_mm,text_path:Some(path),..}=&actual.object("text").unwrap().geometry else{panic!()};assert_eq!(*font_size_mm,8.);assert_eq!(path.path_id,"guide");assert_eq!(path.start_offset_mm,6.);same_glyph_world(&source,&actual);
        }
    }
    #[test]
    fn native_font_offset_content_and_reparenting_edits_survive_chart_restoration(){
        for mode in 0..3{
            let mut source=similarity_source();if mode==2{let mut other=source.objects[0].clone();other.id="other".into();other.transform=[3.,0.,0.,3.,0.,0.];source.objects.push(other);}
            let frames:super::super::local_frames::Frames=super::super::metadata::decode("ArrangerLocalFramesV1|sha256:",&super::super::local_frames::notes(&source).unwrap().unwrap()).unwrap();let params:super::super::text_params::Parameters=super::super::metadata::decode("ArrangerTextParametersV1|sha256:",&super::super::text_params::notes(&source).unwrap().unwrap()).unwrap();let mut native=super::super::write::normalized(&source).unwrap();let text=native.objects.iter_mut().find(|o|o.id=="text").unwrap();
            if mode==0{let Geometry::Text{content,font_size_mm,text_path:Some(path),..}=&mut text.geometry else{panic!()};*content="Edited".into();*font_size_mm=12.;path.start_offset_mm=8.;}else{text.parent_id=if mode==1{None}else{Some("other".into())};}
            let reference=native.clone();let charts=restore_parent_charts(&mut native,Some(&frames)).unwrap();super::super::text_params::restore_with_charts(&mut native,params,&charts).unwrap();super::super::local_frames::restore_with_units(&mut native,frames,&BTreeMap::new(),&BTreeMap::new()).unwrap();same_glyph_world(&native,&reference);
            let Geometry::Text{font_size_mm,content,text_path:Some(path),..}=&native.object("text").unwrap().geometry else{panic!()};if mode==0{assert_eq!(content,"Edited");assert_eq!(*font_size_mm,6.);assert_eq!(path.start_offset_mm,4.);}else{assert_eq!(*font_size_mm,8./if mode==1{1.}else{3.});assert_eq!(path.start_offset_mm,6./if mode==1{1.}else{3.});}
        }
    }
    #[test]
    fn live_path_geometry_and_fixed_marker_follow_two_native_stories() {
        let raw=include_bytes!("../../../../examples/assets/cdr/corel2019/path-two-stories-v14.cdr");
        let actual=read_native(raw).unwrap();let mut actual_stories=0;
        for n in all(&actual.nodes).into_iter().filter(|n|n.kind()==Some(b"obj ")) {
            let (kind,args)=super::super::write::arguments(&super::super::read::own_record(n,b"loda").unwrap().data).unwrap();
            if kind==4 {assert_eq!(u32_at(&args.iter().find(|(k,_)|*k==16015).unwrap().1,0).unwrap(),1);actual_stories+=1;}
        }assert_eq!(actual_stories,2);
        let model=crate::cdr::decode(include_bytes!("../../../../reports/windows/WIN-SAVE-CLOSURE-20261024-01/controls/path-offset-v14.cdr"),"paths").unwrap().0;
        for version in [14,21] {
            let encoded=if version==14{crate::cdr::encode_document(&model)}else{crate::cdr::encode_document21(&model)}.unwrap().0;
            let native=read_native(&encoded).unwrap();let mut seen=0;
            for n in all(&native.nodes).into_iter().filter(|n|n.kind()==Some(b"obj ")) {
                let (kind,args)=super::super::write::arguments(&super::super::read::own_record(n,b"loda").unwrap().data).unwrap();if kind!=4{continue;}
                let get=|tag|&args.iter().find(|(k,_)|*k==tag).unwrap().1;
                assert_eq!(*get(30),[1u32,8].into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>());
                assert_eq!(u32_at(get(16015),0).unwrap(),1,"fixed marker is not the native object key");seen+=1;
            }assert_eq!(seen,1);
        }
    }
    #[test]
    #[ignore = "controlled model path field probes; requires ARRANGER_CDR_PATH_PARAMETER_PROBES"]
    fn controlled_model_path_field_probes_keep_every_unmodified_native_record() {
        let directory=std::path::PathBuf::from(std::env::var_os("ARRANGER_CDR_PATH_PARAMETER_PROBES").expect("set a fresh probe directory"));std::fs::create_dir(&directory).unwrap();
        let input=include_bytes!("../../../../reports/windows/WIN-SAVE-CLOSURE-20261024-01/controls/path-offset-v14.cdr");
        let model=crate::cdr::decode(input,"probe").unwrap().0;let baseline=crate::cdr::encode_document(&model).unwrap().0;
        let mut cases=vec![];
        for marker in [false,true] {for frame in [false,true] {for flags in [false,true] {for geometry in [false,true] {
            let mut native=read_native(&baseline).unwrap();
            fn edit(nodes:&mut [Node],marker:bool,frame:bool,flags:bool,geometry:bool) {
                for n in nodes {
                    if n.kind()==Some(b"obj ") {
                        let load=n.find_mut(b"loda").unwrap();let (kind,mut args)=super::super::write::arguments(&load.data).unwrap();let text=kind==4;
                        if text&&marker {args.iter_mut().find(|(tag,_)|*tag==16015).unwrap().1=1u32.to_le_bytes().to_vec();}
                        if text&&geometry {args.iter_mut().find(|(tag,_)|*tag==30).unwrap().1=[1u32,8].into_iter().flat_map(u32::to_le_bytes).collect();}
                        load.data=super::super::write::loda(kind,&args);
                        if text&&frame {set32(&mut n.find_mut(b"txsm").unwrap().data,40,1).unwrap();}
                        if flags {let flag=n.find_mut(b"flgs").unwrap();let v=u32_at(&flag.data,0).unwrap();flag.data=(if text{v|0x800}else{v&!0x800}).to_le_bytes().to_vec();}
                    }
                    if let Some(c)=&mut n.children{edit(c,marker,frame,flags,geometry);}
                }
            }
            edit(&mut native.nodes,marker,frame,flags,geometry);let riff=write(&native.nodes).unwrap();native.members.insert("content/riffData.cdr".into(),riff);
            let bytes=zip(&native.members.iter().map(|(n,d)|(n.as_str(),d.as_slice())).collect::<Vec<_>>());assert_eq!(read_native(&bytes).unwrap().nodes,native.nodes);
            let file=format!("path-marker{}-frame{}-flags{}-geometry{}.cdr",u8::from(marker),u8::from(frame),u8::from(flags),u8::from(geometry));std::fs::write(directory.join(&file),&bytes).unwrap();cases.push(serde_json::json!({"file":file,"marker":marker,"frame":frame,"flags":flags,"geometry":geometry,"sha256":crate::model::sha256(&bytes)}));
        }}}}
        std::fs::write(directory.join("manifest.json"),serde_json::to_vec_pretty(&serde_json::json!({"cases":cases,"model_encoder":false,"scope":"One live model scene with three independently controlled native bookkeeping fields."})).unwrap()).unwrap();
    }
    #[test]
    fn malformed_relations_and_unrepresented_path_effects_reject_before_import() {
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports/windows/WIN-SAVE-CLOSURE-20261024-01/controls");
        for version in [14,21] {for mode in 0..6 {
            let source=std::fs::read(root.join(format!("path-control-v{version}.cdr"))).unwrap();let native=read_native(&source).unwrap();let mut nodes=native.nodes;let mut changed=false;
            fn edit(nodes:&mut [Node],version:u32,mode:u32,changed:&mut bool) {
                for n in nodes {
                    if !*changed&&n.tag==*b"lnkt"&&mode<3 {
                        set32(&mut n.data,if mode==0 {20}else if mode==1 {28}else{12},if mode==0 {65}else if mode==1 {9999}else{16}).unwrap();*changed=true;
                    }
                    if !*changed&&n.tag==*b"txsm"&&mode>=3 {
                        if mode==3 {set32(&mut n.data,if version==14 {100}else{105},3).unwrap();}
                        else if mode==4 {let last=n.data.len()-1;n.data[last]=1;}
                        else {let start=if version==14 {44}else{49};n.data[start..start+8].copy_from_slice(&2f64.to_le_bytes());}
                        *changed=true;
                    }
                    if let Some(children)=&mut n.children {edit(children,version,mode,changed);}
                }
            }
            edit(&mut nodes,version,mode,&mut changed);assert!(changed);
            let damaged=if version==14 {write(&nodes).unwrap()}else{write21(&nodes,native.members).unwrap()};
            let response=std::panic::catch_unwind(||crate::cdr::decode(&damaged,"bad"));assert!(response.is_ok());assert!(response.unwrap().is_err(),"{version}/{mode}");
        }}
    }
    #[test]
    fn controlled_path_cache_probes_preserve_every_other_native_payload() {
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
        let output=std::env::var_os("ARRANGER_CDR_PATH_CACHE_PROBES").map(std::path::PathBuf::from);
        if let Some(out)=&output {assert!(out.parent().unwrap().canonicalize().unwrap().starts_with(root.join("artifacts")));std::fs::create_dir(out).unwrap();}
        let mut cases=vec![];
        for version in [14,21] {for kind in ["control","offset","distance","center","right"] {
            let relative=format!("reports/windows/WIN-SAVE-CLOSURE-20261024-01/controls/path-{kind}-v{version}.cdr");let source=root.join(&relative);let input=std::fs::read(&source).unwrap();
            let doc=crate::cdr::decode(&input,"probe").unwrap().0;let text=doc.object("control-text").unwrap();
            let Geometry::Text{content,..}=&text.geometry else{unreachable!()};let units=content.encode_utf16().count();
            for (mode,label) in [(0,"repacked-control"),(1,"no-cache"),(2,"no-cache-zero-start"),(3,"no-cache-zero-start-identity-frame")] {
                let mut native=read_native(&input).unwrap();let before=native.nodes.clone();let mut changed=0;
                fn edit(nodes:&mut [Node],version:u32,mode:u32,units:usize,changed:&mut usize) {
                    for n in nodes {
                        if n.kind()==Some(b"obj ")&&super::super::write::arguments(&super::super::read::own_record(n,b"loda").unwrap().data).is_ok_and(|(k,_)|k==4) {
                            let story=&mut n.find_mut(b"txsm").unwrap().data;let at=story.len().checked_sub(units*24+1).unwrap();assert_eq!(story[at],1);cache(&story[at+1..],units).unwrap();
                            if mode>0 {story.truncate(at+1);story[at]=0;}
                            if mode>1 {set32(story,if version==14 {108}else{113},0).unwrap();}
                            if mode>2 {
                                let identity=[1f64,0.,0.,0.,1.,0.];let start=if version==14 {44}else{49};for(i,v)in identity.iter().enumerate(){story[start+i*8..start+i*8+8].copy_from_slice(&v.to_le_bytes());}
                                let matrix=&mut n.find_mut(b"trfd").unwrap().data;let start=u32_at(matrix,u32_at(matrix,8).unwrap() as usize).unwrap() as usize+16;
                                for(i,v)in identity.iter().enumerate(){matrix[start+i*8..start+i*8+8].copy_from_slice(&v.to_le_bytes());}
                            }*changed+=1;
                        }
                        if let Some(children)=&mut n.children {edit(children,version,mode,units,changed);}
                    }
                }
                edit(&mut native.nodes,version,mode,units,&mut changed);assert_eq!(changed,1);
                let expected=native.nodes.clone();let bytes=if version==21 {super::super::binary::repack21(&native).unwrap()}else{
                    let riff=write(&native.nodes).unwrap();native.members.insert("content/riffData.cdr".into(),riff);zip(&native.members.iter().map(|(n,d)|(n.as_str(),d.as_slice())).collect::<Vec<_>>())
                };
                let after=read_native(&bytes).unwrap();assert_eq!(after.nodes,expected);
                if mode==0 {assert_eq!(after.nodes,before);}
                for (name,value) in &native.members {if !["content/riffData.cdr","content/root.dat","content/dataFileList.dat"].contains(&name.as_str()){assert_eq!(after.members.get(name),Some(value));}}
                if let Some(out)=&output {
                    let name=format!("path-{kind}-v{version}-{label}.cdr");std::fs::write(out.join(&name),&bytes).unwrap();
                    cases.push(serde_json::json!({"source":relative,"source_sha256":crate::model::sha256(&input),"file":name,"sha256":crate::model::sha256(&bytes),"native_version":version,"kind":kind,"variant":label,"model_encoder":false,"source_text":content,"source_utf16_units":units}));
                }
            }
            assert_eq!(std::fs::read(source).unwrap(),input);
        }}
        if let Some(out)=&output {let manifest=serde_json::json!({"cases":cases,"model_encoder":false,"corel_new_outputs":"pending","scope":"40 controlled native container/cache variations, not an Arranger model writer or compatibility acceptance. Source originals and every unmodified native payload/auxiliary member preserved."});std::fs::write(out.join("manifest.json"),serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();}
    }

}
