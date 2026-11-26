//! Single rectangular, unlinked CDR14 paragraph frames. Native W-20 records
//! supply the defaults; character/paragraph formatting is checked explicitly.
use super::{binary::*, invalid, unsupported, Result};
use crate::{Document, Geometry, Object, TextAlign, TextLayoutOptions, TextMode};
use std::collections::BTreeMap;

pub(super) const PROFILE:&[u8]=include_bytes!("../../../../examples/assets/cdr/corel2019/area-profile-v14.cdr");
const RESAVED_PROFILE:&[u8]=include_bytes!("../../../../examples/assets/cdr/corel2019/area-resaved-inactive-bullet-v14.cdr");
fn resaved_inactive_bullet()->Result<&'static [u8]> {
    static ROW:std::sync::OnceLock<Result<Vec<u8>>>=std::sync::OnceLock::new();
    match ROW.get_or_init(|| {
        let tree=read(RESAVED_PROFILE)?;
        let node=all(&tree).into_iter().find(|n|n.kind()==Some(b"stlt")).ok_or_else(||invalid("resaved paragraph profile missing"))?;
        Tables::parse(&node.data)?.rows.get("bullets").and_then(|rows|rows.first()).cloned().ok_or_else(||invalid("resaved inactive bullet profile missing"))
    }) {Ok(row)=>Ok(row),Err(error)=>Err(error.clone())}
}
const SECTIONS:[(&str,usize);12]=[("fills",60),("outlines",12),("fonts",60),("aligns",12),("intervals",52),("set5",152),("tabs",784),("bullets",0),("indents",28),("hyphens",36),("dropcaps",28),("set11",12)];
// Corel tracking excludes both boundaries around a space. The measured SDK
// WordSpacing setter uses the same millionths-of-space units as tracking.
// Keep the full uniform model spacing by accounting for those two boundaries.
pub(super) fn word_tracking(tracking:i32)->i32{1_000_000+2*tracking}
pub(super) fn represented_word_tracking(tracking:i32,word:f64)->bool{word==1_000_000.||word==f64::from(word_tracking(tracking))}
pub(super) fn represented_language_tracking(tracking:i32,language:f64)->bool{language==0.||language==f64::from(tracking)}
pub(super) struct Tables {pub rows:BTreeMap<&'static str,Vec<Vec<u8>>>,pub styles:Vec<Vec<u8>>,next:u32,aux:Option<Vec<u32>>}
impl Tables {
    pub fn parse(data:&[u8])->Result<Self> {
        let mut at=4;let count=u32_at(data,at)? as usize;at+=4;
        if count>4096 {return Err(invalid("too many native paragraph styles"));}
        let mut result=Self{rows:BTreeMap::new(),styles:vec![],next:1,aux:None};
        if count==0 {return Ok(result);}
        for (name,size) in SECTIONS {
            let n=u32_at(data,at)? as usize;at+=4;if n>4096 {return Err(invalid("too many native paragraph table entries"));}
            let mut rows=vec![];
            for _ in 0..n {
                let len=if size==0 {if u32_at(data,at+44)?==0 {60}else{116}}else{size};
                rows.push(bytes(data,at,len)?.to_vec());at+=len;
            }
            result.rows.insert(name,rows);
        }
        for _ in 0..count {
            let start=at;let level=u32_at(data,at)?;if !(1..=3).contains(&level) {return Err(unsupported("native paragraph style hierarchy is unsupported"));}
            at+=24;let name=u32_at(data,start+20)? as usize;at=at.checked_add(name.checked_mul(2).ok_or_else(||invalid("paragraph style name overflow"))?).ok_or_else(||invalid("paragraph style overflow"))?;
            at+=8+if level>1 {20}else{0}+if level>2 {20}else{0};result.styles.push(bytes(data,start,at-start)?.to_vec());
        }
        if at!=data.len() {return Err(unsupported("unknown native paragraph table trailer"));}
        for (row,offset) in result.rows.values().flatten().map(|r|(r,0)).chain(result.styles.iter().map(|r|(r,4))) {
            let key=u32_at(row,offset)?;result.next=result.next.max(key.checked_add(1).ok_or_else(||invalid("paragraph ID overflow"))?);
        }
        Ok(result)
    }
    pub fn row(&self,section:&str,key:u32)->Result<&[u8]> {
        let matches=self.rows.get(section).into_iter().flatten().filter(|r|u32_at(r,0).ok()==Some(key)).collect::<Vec<_>>();
        if matches.len()!=1 {return Err(invalid("missing or duplicate native paragraph style reference"));}Ok(matches[0])
    }
    fn allocate(&mut self)->Result<u32> {let n=self.next;self.next=self.next.checked_add(1).ok_or_else(||invalid("paragraph ID overflow"))?;Ok(n)}
    fn append(&mut self,name:&'static str,mut row:Vec<u8>)->Result<u32> {
        let key=self.allocate()?;set32(&mut row,0,key)?;self.rows.entry(name).or_default().push(row);Ok(key)
    }
    pub fn encode(&self)->Vec<u8> {
        let mut out=b"stlt".to_vec();out.extend((self.styles.len() as u32).to_le_bytes());
        for (name,_) in SECTIONS {let rows=self.rows.get(name).map(Vec::as_slice).unwrap_or_default();out.extend((rows.len() as u32).to_le_bytes());for row in rows {out.extend(row);}}
        for row in &self.styles {out.extend(row);}out
    }
    pub fn add_paragraph(&mut self,profile:&Self,font:super::fonts::Choice,size:f64,paint:u32,settings:&TextLayoutOptions,tracking:i32)->Result<u32> {
        let sample=|name:&str|profile.rows.get(name).and_then(|r|r.first()).cloned().ok_or_else(||invalid("native paragraph profile entry missing"));
        let mut fill=sample("fills")?;set32(&mut fill,8,paint)?;let fill=self.append("fills",fill)?;
        let mut line=sample("outlines")?;set32(&mut line,8,paint)?;let line=self.append("outlines",line)?;
        let mut face=sample("fonts")?;
        for at in [24,28,32] {face[at..at+2].copy_from_slice(&font.id.to_le_bytes());face[at+2..at+4].copy_from_slice(&font.charset.to_le_bytes());}
        // The SDK-bound native profile uses charset 0 in the Western slot,
        // independently of the same CJK family's GB2312 declaration.
        if !font.legacy {face[26..28].copy_from_slice(&0u16.to_le_bytes());}
        for at in [36,40,44] {set_coord(&mut face,at,size)?;}
        let face=self.append("fonts",face)?;let mut align=sample("aligns")?;
        // Stored 2=center, 3=right. Corel's SDK enumeration uses the reverse.
        set32(&mut align,8,match settings.align {TextAlign::Left=>1,TextAlign::Center=>2,TextAlign::Right=>3})?;
        let align=self.append("aligns",align)?;let mut interval=sample("intervals")?;set32(&mut interval,12,tracking as u32)?;set32(&mut interval,16,tracking as u32)?;set32(&mut interval,20,word_tracking(tracking) as u32)?;
        set_coord(&mut interval,24,settings.line_height_mm.unwrap_or(size*1.2))?;set32(&mut interval,48,1)?;let interval=self.append("intervals",interval)?;
        let aux=if let Some(aux)=&self.aux {aux.clone()}else{
            let mut aux=vec![];for name in ["set5","set11","tabs","bullets","indents","hyphens","dropcaps"] {aux.push(self.append(name,sample(name)?)?);}self.aux=Some(aux.clone());aux
        };
        let key=self.allocate()?;let area=settings.mode==TextMode::Area;
        let mut values=vec![if area {3}else{2},key,0,0,0,0,fill,line,face,align,interval,aux[0],aux[1]];
        if area {values.extend_from_slice(&aux[2..]);}
        self.styles.push(values.into_iter().flat_map(u32::to_le_bytes).collect());Ok(key)
    }
    pub fn point_options(&self,style:&[u8],profile:&Self)->Result<(Option<f64>,i32)> {
        if u32_at(style,0)?!=2 {return Err(unsupported("point text requires regular artistic paragraph defaults"));}
        let start=24+u32_at(style,20)? as usize*2;let refs=(0..7).map(|i|u32_at(style,start+i*4)).collect::<Result<Vec<_>>>()?;
        for (i,name) in [(5,"set5"),(6,"set11")] {if self.row(name,refs[i])?.get(4..)!=profile.rows[name][0].get(4..) {return Err(unsupported("additional point-text paragraph effects remain unsupported"));}}
        if ![0,1,2,3].contains(&u32_at(self.row("aligns",refs[3])?,8)?) {return Err(unsupported("point center/right alignment needs native anchor preservation"));}
        let interval=self.row("intervals",refs[4])?;let sample=&profile.rows["intervals"][0];
        for i in 4..52 {if ![12..24,24..32,48..52].iter().any(|r|r.contains(&i))&&interval[i]!=sample[i] {return Err(unsupported("additional point-text spacing effects remain unsupported"));}}
        let tracking=i32_at(interval,12)?;if !(-1_000_000..=20_000_000).contains(&tracking) {return Err(unsupported("point tracking outside supported native range"));}
        if !represented_word_tracking(tracking,i32_at(interval,20)? as f64){return Err(unsupported("independent native word spacing is not represented"));}
        if !represented_language_tracking(tracking,i32_at(interval,16)? as f64){return Err(unsupported("independent native inter-language spacing is not represented"));}
        let leading=match u32_at(interval,48)? {
            1 if i32_at(interval,28)?==0&&i32_at(interval,24)?>0=>Some(i32_at(interval,24)? as f64/10000.),
            0 if i32_at(interval,24)?==1_000_000&&i32_at(interval,28)?==1_000_000=>None,
            _=>return Err(unsupported("nondefault relative point leading or paragraph before-spacing remains unsupported")),
        };Ok((leading,tracking))
    }

    pub fn area_options(&self,style:&[u8],profile:&Self)->Result<(TextAlign,f64,i32)> {
        if u32_at(style,0)?!=3 {return Err(unsupported("area text requires complete paragraph defaults"));}
        let start=24+u32_at(style,20)? as usize*2;let refs=(0..12).map(|i|u32_at(style,start+i*4)).collect::<Result<Vec<_>>>()?;
        for (i,name) in [(5,"set5"),(6,"set11"),(7,"tabs"),(8,"bullets"),(9,"indents"),(10,"hyphens"),(11,"dropcaps")] {
            let row=self.row(name,refs[i])?;let sample=profile.rows.get(name).and_then(|r|r.first()).ok_or_else(||invalid("missing area defaults"))?;
            // The SDK confirms empty BulletSymbol/BulletFont in every
            // paragraph of this actual resave. Only its exact inactive row
            // variant is accepted; active or unknown bullet data still fails.
            // ADR-002's actual two-paragraph native edit also serializes an
            // extended116-byte inactive row. COM independently observed
            // Effect=0, empty BulletFont/BulletSymbol and zero indents in both
            // paragraphs. Admit only that exact measured row, not arbitrary
            // extended bullet data or a guessed inactive-bit mask.
            let known_inactive=name=="bullets" && (row.get(4..)==resaved_inactive_bullet()?.get(4..)
                || row.get(4..)==include_bytes!("../../../../examples/assets/cdr/corel2019/delivery-inactive-bullet116.bin").get(4..));
            if row.get(4..)!=sample.get(4..) && !known_inactive {return Err(unsupported("tabs, bullets, indents, hyphenation or advanced paragraph effects are unsupported"));}
        }
        let align=match u32_at(self.row("aligns",refs[3])?,8)? {1=>TextAlign::Left,2=>TextAlign::Center,3=>TextAlign::Right,_=>return Err(unsupported("justified/mixed paragraph alignment is unsupported"))};
        let interval=self.row("intervals",refs[4])?;let sample=&profile.rows["intervals"][0];
        for i in 4..52 {if ![12..24,24..28].iter().any(|r|r.contains(&i)) && interval[i]!=sample[i] {return Err(unsupported("relative spacing or additional paragraph spacing is unsupported"));}}
        let height=i32_at(interval,24)? as f64/10000.;let tracking=i32_at(interval,12)?;
        if height<=0. || !(-1_000_000..=20_000_000).contains(&tracking) {return Err(unsupported("paragraph line height/tracking is outside supported native range"));}
        if !represented_word_tracking(tracking,i32_at(interval,20)? as f64){return Err(unsupported("independent native word spacing is not represented"));}
        if !represented_language_tracking(tracking,i32_at(interval,16)? as f64){return Err(unsupported("independent native inter-language spacing is not represented"));}
        Ok((align,height,tracking))
    }
}
pub(super) fn profile()->Result<(Vec<Node>,Tables)> {
    let nodes=read(PROFILE)?;let table=all(&nodes).into_iter().find(|n|n.kind()==Some(b"stlt")).ok_or_else(||invalid("native area style profile missing"))?;
    let tables=Tables::parse(&table.data)?;Ok((nodes,tables))
}
pub(super) fn space_width(doc:&Document,object:&Object)->Result<f64> {
    let mut probe=object.clone();probe.parent_id=None;probe.transform=[1.,0.,0.,1.,0.,0.];probe.clip=None;
    if let Geometry::Text{content,text_path,layout,..}=&mut probe.geometry {*content=" ".into();*text_path=None;*layout=Default::default();}
    let width=crate::text::layout(doc,&probe)?.advance_mm;
    if !width.is_finite() || width<=0. {return Err(unsupported("font space advance is unresolved for native tracking conversion"));}Ok(width)
}
pub(super) fn tracking(doc:&Document,object:&Object)->Result<i32> {
    let Geometry::Text{layout,..}=&object.geometry else{unreachable!()};if layout.letter_spacing_mm==0. {return Ok(0);}
    let units=(layout.letter_spacing_mm/space_width(doc,object)?*1_000_000.).round_ties_even();
    if !(-1_000_000.0..=20_000_000.0).contains(&units) {return Err(unsupported("native tracking requires -100..2000 percent of the resolved font's space advance"));}Ok(units as i32)
}
pub(super) fn frame(template:&[u8],w:f64,h:f64)->Result<Vec<u8>> {
    let mut data=bytes(template,96,36)?.to_vec();set_coord(&mut data,10,w)?;set_coord(&mut data,18,-h)?;Ok(data)
}
pub(super) fn geometry(template:&[u8],w:f64,h:f64)->Result<Vec<u8>> {
    if template.len()!=92 {return Err(invalid("native area frame profile is not rectangular"));}
    let mut data=template.to_vec();set_coord(&mut data,4,w)?;set_coord(&mut data,8,h)?;set_coord(&mut data,24,w)?;Ok(data)
}
// Across42 genuine Corel paragraph objects, the final geometry word is the
// preceding native bookkeeping ID, not an enum of paragraph formatting modes.
// Accept that measured relation, never a numeric whitelist of14/16/18/21.
pub(super) fn canonical_geometry(g:&[u8],additional_id:Option<u32>)->Result<Vec<u8>> {
    let mut out=g.to_vec();let serial=u32_at(g,88)?;
    if serial!=15&&!(serial>0&&serial.checked_add(1)==additional_id) {
        return Err(unsupported("unresolved native area geometry/bookkeeping ID relation"));
    }
    set32(&mut out,88,15)?;Ok(out)
}
fn frame_coordinates(story:[f64;6],matrix:[f64;6])->Result<[f64;3]> {
    let close=|a:[f64;6],b:[f64;6]|a.iter().zip(b).all(|(x,y)|(x-y).abs()<=0.000001);
    if close(story,matrix){return Ok([1.,0.,1.]);}
    // Actual Corel2019 SaveAs14 normalizes the paragraph story basis to
    // orthonormal axes while retaining the complete affine frame in trfd.
    // This is a derived orientation, not a second independent placement.
    let [a,b,tx,c,d,ty]=matrix;let length=a.hypot(c);let determinant=a*d-b*c;
    if !length.is_finite()||length<=1e-12||determinant.abs()<=1e-12{return Err(unsupported("singular area text frame transform"));}
    let orientation=determinant.signum();
    if close(story,[a/length,-orientation*c/length,tx,c/length,orientation*a/length,ty]){return Ok([length,(a*b+c*d)/length,determinant.abs()/length]);}
    Err(unsupported("independent area text frame transforms are unsupported"))
}
pub(super) fn check_frame_matrix(story:[f64;6],matrix:[f64;6])->Result<()> {frame_coordinates(story,matrix).map(|_|())}
pub(super) fn dimensions21(g:&[u8],frame:[i32;4],story:[f64;6],matrix:[f64;6],additional_id:Option<u32>)->Result<(f64,f64)> {
    let w=i32_at(g,4)? as f64/10000.;let h=i32_at(g,8)? as f64/10000.;
    if w<=0.||h<=0. {return Err(invalid("nonpositive native21 area dimensions"));}
    let (nodes,_)=profile()?;let node=all(&nodes).into_iter().find(|n|n.kind()==Some(b"obj ")).ok_or_else(||invalid("missing area profile"))?;
    let (_,args)=super::write::arguments(&node.find(b"loda")?.data)?;let template=args.iter().find(|(k,_)|*k==30).ok_or_else(||invalid("missing area geometry"))?;
    let [sx,shear,sy]=frame_coordinates(story,matrix)?;
    // Actual21 resaves retain local column width in the paragraph style while
    // moving scale/shear into these four rectangular story coordinates.
    let expected=[0,-coord(h*sy)?,coord(-h*shear)?,coord(w*sx)?];
    if frame!=expected||canonical_geometry(g,additional_id)?!=geometry(&template.1,w,h)? {return Err(unsupported("native21 area columns, insets or independent frame properties unsupported"));}
    Ok((w,h))
}
pub(super) fn dimensions(g:&[u8],text:&[u8],matrix:[f64;6],additional_id:Option<u32>)->Result<(f64,f64)> {
    let w=i32_at(g,4)? as f64/10000.;let h=i32_at(g,8)? as f64/10000.;
    if w<=0. || h<=0. {return Err(invalid("nonpositive native area dimensions"));}
    let (nodes,_)=profile()?;let node=all(&nodes).into_iter().find(|n|n.kind()==Some(b"obj ")).ok_or_else(||invalid("missing area profile"))?;
    let (_,args)=super::write::arguments(&node.find(b"loda")?.data)?;let template=args.iter().find(|(k,_)|*k==30).ok_or_else(||invalid("missing area geometry"))?;
    let actual_geometry=canonical_geometry(g,additional_id)?;
    let story=(0..6).map(|i|f64_at(text,44+i*8)).collect::<Result<Vec<_>>>()?;
    let [sx,shear,sy]=frame_coordinates(story.try_into().unwrap(),matrix)?;
    let mut expected_frame=frame(&node.find(b"txsm")?.data,w*sx,h*sy)?;
    // Corel stores the QR residual in the rectangular story frame: scaled
    // width, negative height, then the bottom edge's horizontal displacement.
    // These are rounded native coordinates, not an arbitrary frame allowance.
    set_coord(&mut expected_frame,22,-h*shear)?;
    if actual_geometry!=geometry(&template.1,w,h)? || bytes(text,96,36)?!=expected_frame {return Err(unsupported("area columns, insets, linked frames or additional frame properties are unsupported"));}
    Ok((w,h))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_extended_inactive_bullet_profile_rejects_unmeasured_byte_changes(){
        let native=read_native(include_bytes!("../../../../examples/assets/cdr/corel2019/delivery-14-area-edited.cdr")).unwrap();
        let node=all(&native.nodes).into_iter().find(|n|n.kind()==Some(b"stlt")).unwrap();let mut table=Tables::parse(&node.data).unwrap();let(_,profile)=profile().unwrap();
        let styles=table.styles.iter().filter(|s|u32_at(s,0).unwrap()==3).cloned().collect::<Vec<_>>();assert_eq!(styles.len(),2);
        for s in &styles{table.area_options(s,&profile).unwrap();}
        let at=table.rows["bullets"].iter().position(|r|r.len()==116).unwrap();let original=table.rows["bullets"][at].clone();
        for offset in[4,16,20,44,48,60,72,100,112]{table.rows.get_mut("bullets").unwrap()[at][offset]^=1;assert!(styles.iter().any(|s|table.area_options(s,&profile).is_err()),"{offset}");table.rows.get_mut("bullets").unwrap()[at]=original.clone();}
    }
    #[test]
    fn uniform_spacing_relations_do_not_admit_independent_word_or_language_edits(){
        let(_,sample)=profile().unwrap();let mut table=Tables::parse(&sample.encode()).unwrap();
        let settings=TextLayoutOptions{letter_spacing_mm:0.125,..Default::default()};
        table.add_paragraph(&sample,super::super::fonts::Choice{id:1,charset:0,legacy:false},4.,1,&settings,112478).unwrap();
        let style=table.styles.last().unwrap().clone();assert_eq!(table.point_options(&style,&sample).unwrap().1,112478);
        let original=table.rows["intervals"].last().unwrap().clone();
        for(at,value)in[(16,112479),(20,1224957)]{
            set32(table.rows.get_mut("intervals").unwrap().last_mut().unwrap(),at,value).unwrap();assert!(table.point_options(&style,&sample).is_err());
            *table.rows.get_mut("intervals").unwrap().last_mut().unwrap()=original.clone();
        }
        set32(table.rows.get_mut("intervals").unwrap().last_mut().unwrap(),16,0).unwrap();set32(table.rows.get_mut("intervals").unwrap().last_mut().unwrap(),20,1_000_000).unwrap();assert!(table.point_options(&style,&sample).is_ok());
    }
    #[test]
    fn actual_native21_qr_frame_rejects_changed_edges_and_independent_axes() {
        let native=read_native(include_bytes!("../../../../examples/assets/cdr/corel2019/unicode-area-resaved-v21.cdr")).unwrap();
        let node=all(&native.nodes).into_iter().find(|n|n.kind()==Some(b"obj ")&&super::super::write::arguments(&n.find(b"loda").unwrap().data).is_ok_and(|(k,_)|k==6)).unwrap();
        let (_,args)=super::super::write::arguments(&node.find(b"loda").unwrap().data).unwrap();let g=&args.iter().find(|(k,_)|*k==30).unwrap().1;let id=args.iter().find(|(k,_)|*k==100).map(|(_,v)|u32_at(v,0).unwrap());
        let transform=&node.find(b"trfd").unwrap().data;let at=u32_at(transform,u32_at(transform,8).unwrap() as usize).unwrap() as usize+16;let matrix=std::array::from_fn(|i|f64_at(transform,at+i*8).unwrap());let story=&node.find(b"txsm").unwrap().data;let basis=std::array::from_fn(|i|f64_at(story,49+i*8).unwrap());let frame:[i32;4]=std::array::from_fn(|i|i32_at(story,109+i*4).unwrap());
        assert_eq!(dimensions21(g,frame,basis,matrix,id).unwrap(),(150.,60.));
        for i in 0..4 {let mut bad=frame;bad[i]+=1;assert!(dimensions21(g,bad,basis,matrix,id).is_err());}
        let mut bad=basis;bad[0]+=0.01;assert!(dimensions21(g,frame,bad,matrix,id).is_err());
    }
    #[test]
    fn actual_affine_paragraph_resave_uses_derived_story_axes_without_losing_the_frame() {
        let raw=include_bytes!("../../../../examples/assets/cdr/corel2019/area-affine-resaved-v14.cdr");
        let doc=crate::cdr::decode(raw,"affine").unwrap().0;
        let text=doc.object("area-text").unwrap();
        assert_eq!((text.width_mm,text.height_mm),(65.,55.));
        for (value,expected) in text.transform.into_iter().zip([-0.79,0.2,0.39,1.135,127.45,62.225]) {assert!((value-expected).abs()<1e-10);}
        let Geometry::Text{content,layout,..}=&text.geometry else{panic!()};
        assert!(content.contains("区域文字"));assert_eq!(layout.mode,TextMode::Area);
        let native=read(raw).unwrap();let object=all(&native).into_iter().find(|n|n.kind()==Some(b"obj ")&&super::super::read::own_record(n,b"txsm").is_ok()).unwrap();
        let trfd=&object.find(b"trfd").unwrap().data;let at=u32_at(trfd,u32_at(trfd,8).unwrap() as usize).unwrap() as usize+16;
        let matrix=std::array::from_fn(|i|f64_at(trfd,at+i*8).unwrap());
        let mut story:[f64;6]=std::array::from_fn(|i|f64_at(&object.find(b"txsm").unwrap().data,44+i*8).unwrap());
        check_frame_matrix(story,matrix).unwrap();
        for i in 0..6 {story[i]+=0.01;assert!(check_frame_matrix(story,matrix).is_err());story[i]-=0.01;}
    }
    #[test]
    fn actual_resaved_frame_marker17_keeps_tracking_and_single_frame_geometry() {
        let raw=include_bytes!("../../../../examples/assets/cdr/corel2019/area-resaved-frame-marker17-v14.cdr");
        let doc=crate::cdr::decode(raw,"frame").unwrap().0;
        for (id,spacing) in [("area-text",0.35),("negative-tracking",-0.08)] {
            let o=doc.object(id).unwrap();let Geometry::Text{layout,..}=&o.geometry else{panic!()};
            assert_eq!((o.width_mm,o.height_mm),(65.,55.));assert_eq!(layout.mode,TextMode::Area);
            assert!((layout.letter_spacing_mm-spacing).abs()<0.00001);
        }
        assert_eq!(crate::cdr::decode(&crate::cdr::encode_document(&doc).unwrap().0,"frame").unwrap().0,doc);
    }
    #[test]
    fn actual_corel_resave_with_inactive_bullet_metadata_stays_editable() {
        let doc=crate::cdr::decode(RESAVED_PROFILE,"actual").unwrap().0;
        let text=doc.objects.iter().find(|o|matches!(o.geometry,Geometry::Text{..})).unwrap();
        let Geometry::Text{content,layout,..}=&text.geometry else{unreachable!()};
        assert_eq!(content,"区域文字 / English 自动换行。保留全部内容。\n\n保留空行和末行。\n edited");
        assert_eq!(layout.mode,TextMode::Area);assert_eq!(layout.line_height_mm,Some(8.));
        let again=crate::cdr::decode(&crate::cdr::encode_document(&doc).unwrap().0,"actual").unwrap().0;
        assert_eq!(again,doc);
    }
    #[test]
    fn exported_uniform_cjk_paragraph_slots_match_the_native_script_profile() {
        let doc=crate::cdr::decode(PROFILE,"slots").unwrap().0;
        let raw=crate::cdr::encode_document(&doc).unwrap().0;
        let tree=read(&raw).unwrap();
        let encoded=all(&tree).into_iter().find(|n|n.kind()==Some(b"stlt")).unwrap();
        let table=Tables::parse(&encoded.data).unwrap();
        let (_,native)=profile().unwrap();
        let original=native.rows["fonts"].iter().find(|row|u16_at(row,24).unwrap()==57).unwrap();
        for style in &table.styles {
            if u32_at(style,0).unwrap()!=3 {continue;}
            let start=24+u32_at(style,20).unwrap() as usize*2;
            let font=table.row("fonts",u32_at(style,start+8).unwrap()).unwrap();
            assert_eq!(&font[24..32],&original[24..32],"Latin and CJK slots must use their own declared charset");
        }
    }
    fn changed(mut alter:impl FnMut(&mut Node))->Vec<u8> {
        fn visit(nodes:&mut [Node],alter:&mut impl FnMut(&mut Node)) {
            for n in nodes {alter(n);if let Some(children)=&mut n.children {visit(children,alter);}}
        }
        let mut tree=read(PROFILE).unwrap();visit(&mut tree,&mut alter);let riff=write(&tree).unwrap();let members=unzip(PROFILE).unwrap();
        zip(&[("mimetype",b"application/x-vnd.corel.draw.document+zip"),("content/riffData.cdr",&riff),("font/fontTable.dat",&members["font/fontTable.dat"])])
    }
    #[test]
    fn additional_frame_properties_and_conflicting_dimensions_are_rejected() {
        for (offset,value) in [(4,123456),(12,2),(88,16)] {
            let bytes=changed(|n|if n.kind()==Some(b"obj ") {
                let load=n.find_mut(b"loda").unwrap();let (kind,mut args)=super::super::write::arguments(&load.data).unwrap();
                if kind==6 {let g=&mut args.iter_mut().find(|(k,_)|*k==30).unwrap().1;set32(g,offset,value).unwrap();load.data=super::super::write::loda(kind,&args);}
            });
            assert_eq!(crate::cdr::decode(&bytes,"bad").unwrap_err().code,"CDR_UNSUPPORTED");
        }
    }
    #[test]
    fn unsupported_paragraph_formatting_never_becomes_a_default_layout() {
        for section in ["aligns","intervals","bullets","fonts"] {
            let bytes=changed(|n|if n.kind()==Some(b"stlt") {
                let mut table=Tables::parse(&n.data).unwrap();let row=&mut table.rows.get_mut(section).unwrap()[0];
                match section {"aligns"=>set32(row,8,4),"intervals"=>set32(row,48,0),"fonts"=>set32(row,52,128),_=>set32(row,16,2)}.unwrap();n.data=table.encode();
            });
            assert_eq!(crate::cdr::decode(&bytes,"bad").unwrap_err().code,"CDR_UNSUPPORTED");
        }
    }
}

// Corel artistic text is anchored at the selected alignment. Arranger's point
// paragraphs start in a box whose width is the longest shaped line advance.
// Derive the shift from this host's actual layout, never the ink bounding box.
pub(super) fn point_anchor_shift(doc:&Document,o:&crate::Object)->Result<f64>{
    let Geometry::Text{layout,text_path,..}=&o.geometry else{return Ok(0.)};
    if layout.mode!=crate::TextMode::Point||text_path.is_some()||layout.align==TextAlign::Left{return Ok(0.);}
    let advance=crate::text::layout(doc,o)?.advance_mm;
    Ok(advance*if layout.align==TextAlign::Center{0.5}else{1.})
}
