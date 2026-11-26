//! Native font declarations and fontTable.dat, based on the W-24 original
//! files. Font programs are never embedded. Installed TrueType metadata comes
//! from its real OS/2 and name tables. Local CFF uses Corel's measured
//! declaration-only form; its cache metrics are not invented from TrueType.
use super::{binary::*,invalid,unsupported,Result};
use crate::{Document,Geometry};
use resvg::usvg::fontdb;
use std::{collections::BTreeMap,sync::{Mutex,OnceLock}};

#[derive(Clone,Debug,PartialEq,Eq)]
struct Entry {header:[u8;18],family:String,script:String,used:u32,technology:u32,postscript:String,version:String,subfamily:String,cached:bool}
impl Entry {
    fn id(&self)->u16 {u16::from_le_bytes(self.header[..2].try_into().unwrap())}
    fn charset(&self)->u16 {u16::from_le_bytes(self.header[2..4].try_into().unwrap())}
    fn fixed(&self)->Result<Vec<u8>> {
        if ![2,4,5].contains(&self.technology) {return Err(unsupported("unmeasured native font-cache technology"));}
        fn field(out:&mut Vec<u8>,s:&str,size:usize)->Result<()> {
            let d=wide(s);if s.contains('\0') || d.len()>size {return Err(unsupported("font declaration exceeds native fixed field; no truncation performed"));}out.extend(&d);out.resize(out.len()+size-d.len(),0);Ok(())
        }
        let mut d=self.header.to_vec();field(&mut d,&self.family,64)?;d.extend(0u32.to_le_bytes());d.extend(self.charset().to_le_bytes());field(&mut d,&self.script,64)?;
        d.extend(0u32.to_le_bytes());d.extend(self.used.to_le_bytes());d.extend(2u32.to_le_bytes());field(&mut d,&self.postscript,128)?;d.extend(0u32.to_le_bytes());Ok(d)
    }
}
fn counted(data:&[u8],at:&mut usize)->Result<String> {
    let n=u32_at(data,*at)? as usize;*at+=4;if n==0 || n>4096 {return Err(invalid("invalid native font-cache string size"));}
    let d=bytes(data,*at,n*2)?;*at+=n*2;let value=read_wide(d)?;
    if wide(&value)!=d {return Err(invalid("embedded NUL or missing native font-cache string terminator"));}Ok(value)
}
fn cache(data:&[u8])->Result<Vec<Entry>> {
    if u32_at(data,0)?!=0 || u32_at(data,4)? as usize+8!=data.len() {return Err(invalid("native font-cache envelope differs"));}
    let count=u32_at(data,8)? as usize;if count>4096 {return Err(invalid("too many native font-cache entries"));}let mut at=12;let mut entries=vec![];
    for _ in 0..count {
        if u32_at(data,at)?!=0 {return Err(invalid("nonzero native font-cache row prefix"));}let size=u32_at(data,at+4)? as usize;at+=8;let row=bytes(data,at,size)?;at+=size;let mut p=18;
        let header: [u8;18]=bytes(row,0,18)?.try_into().unwrap();let family=counted(row,&mut p)?;let encoding=*bytes(row,p,1)?.first().unwrap();p+=1;
        if encoding as u16!=u16_at(&header,2)? {return Err(invalid("native font-cache encodings differ"));}let script=counted(row,&mut p)?;
        let used=u32_at(row,p)?;let technology=u32_at(row,p+4)?;p+=8;let postscript=counted(row,&mut p)?;let version=counted(row,&mut p)?;let subfamily=counted(row,&mut p)?;
        if p!=row.len() || entries.iter().any(|e:&Entry|e.header[..4]==header[..4]) {return Err(invalid("native font-cache trailer or duplicate declaration"));}
        entries.push(Entry{header,family,script,used,technology,postscript,version,subfamily,cached:true});
    }
    if at!=data.len(){return Err(invalid("native font-cache trailing bytes"));}Ok(entries)
}
fn encode_cache(entries:&[Entry])->Vec<u8> {
    fn string(out:&mut Vec<u8>,s:&str){out.extend((s.encode_utf16().count() as u32+1).to_le_bytes());out.extend(wide(s));}
    let mut out=vec![];out.extend(0u32.to_le_bytes());out.extend(0u32.to_le_bytes());out.extend((entries.iter().filter(|e|e.cached).count() as u32).to_le_bytes());
    for e in entries.iter().filter(|e|e.cached) {let mut row=e.header.to_vec();string(&mut row,&e.family);row.push(e.charset() as u8);string(&mut row,&e.script);row.extend(e.used.to_le_bytes());row.extend(e.technology.to_le_bytes());for s in [&e.postscript,&e.version,&e.subfamily] {string(&mut row,s);}out.extend(0u32.to_le_bytes());out.extend((row.len() as u32).to_le_bytes());out.extend(row);}
    let len=out.len() as u32-8;out[4..8].copy_from_slice(&len.to_le_bytes());out
}
fn profile_cache(data:&[u8])->Result<Vec<Entry>> {let members=unzip(data)?;cache(members.get("font/fontTable.dat").ok_or_else(||invalid("native font cache absent"))?)}
fn baseline()->Result<Vec<Entry>> {profile_cache(super::write::CONTENT)}
pub(super) fn verified_cff(family:&str)->Result<bool> {Ok(reference(family)?.is_some_and(|e|e.technology==5))}
pub(super) fn verified_legacy(family:&str)->Result<bool> {Ok(known(family)?.is_some_and(|e|e.technology==2))}
fn known(family:&str)->Result<Option<Entry>> {
    let scalar_profile:Option<&[u8]>=match family {
        "SimSun-ExtB"=>Some(include_bytes!("../../../../examples/assets/cdr/corel2019/extb-scalar-native-v21.cdr")),
        "SimSun-ExtG"=>Some(include_bytes!("../../../../examples/assets/cdr/corel2019/extg-scalar-native-v21.cdr")),
        _=>None,
    };
    if let Some(data)=scalar_profile {
        let mut entry=profile_cache(data)?.into_iter().find(|e|e.family==family&&e.charset()==134&&e.technology==2).ok_or_else(||invalid("verified supplementary legacy font declaration absent"))?;
        entry.header[..2].copy_from_slice(&0u16.to_le_bytes());return Ok(Some(entry));
    }
    if matches!(family,"FZHei-B01S"|"方正黑体简体") {
        let mut entry=profile_cache(include_bytes!("../../../../examples/assets/cdr/corel2019/fz-legacy-cache-v14.cdr"))?.into_iter().find(|e|e.family=="方正黑体简体"&&e.charset()==134&&e.technology==2).ok_or_else(||invalid("verified legacy font declaration absent"))?;
        entry.header[..2].copy_from_slice(&0u16.to_le_bytes());return Ok(Some(entry));
    }
    let (native,charset)=match family {"Microsoft YaHei"|"微软雅黑"=>("微软雅黑",134),"宋体"|"SimSun"=>("宋体",134),"Arial"=>("Arial",0),_=>return Ok(None)};
    let entries=if native=="Arial" {profile_cache(include_bytes!("../../../../reports/windows/WIN-SAVE-ROUND-20261024-01/fonts-native/font-arial-v14.cdr"))?}else{baseline()?};
    Ok(Some(entries.into_iter().find(|e|e.family==native&&e.charset()==charset).ok_or_else(||invalid("verified native font declaration absent"))?))
}
// Missing local fonts can still use an exact declaration already proven by
// Corel. This is a fallback metadata registry, not an export font whitelist.
fn reference(family:&str)->Result<Option<Entry>> {
    for data in [include_bytes!("../../../../reports/windows/WIN-SAVE-ROUND-20261024-01/fonts-native/font-arial-v14.cdr").as_slice(),include_bytes!("../../../../reports/windows/WIN-SAVE-ROUND-20261024-01/fonts-native/font-calibri-v14.cdr").as_slice(),include_bytes!("../../../../reports/windows/WIN-SAVE-ROUND-20261024-01/fonts-native/font-times-v14.cdr").as_slice(),include_bytes!("../../../../reports/windows/WIN-SAVE-CLOSURE-20261024-01/completion-controls/font-variable/font-variable-v14.cdr").as_slice(),include_bytes!("../../../../reports/windows/WIN-SAVE-CLOSURE-20261024-01/completion-controls/font-cff/font-cff-v14-edited.cdr").as_slice()] {
        if let Some(mut e)=profile_cache(data)?.into_iter().find(|e|e.family==family&&e.charset()==0&&e.used==1) {
            if e.technology==5 {
                let original=read_native(include_bytes!("../../../../reports/windows/WIN-SAVE-CLOSURE-20261024-01/completion-controls/font-cff/font-cff-v14.cdr"))?;
                let declaration=all(&original.nodes).into_iter().find(|n|n.tag==*b"font"&&read_wide(bytes(&n.data,18,64).unwrap_or_default()).ok().as_deref()==Some(family)).ok_or_else(||invalid("verified CFF original declaration absent"))?;
                e.header.copy_from_slice(bytes(&declaration.data,0,18)?);e.cached=false;
            }
            e.header[..2].copy_from_slice(&0u16.to_le_bytes());return Ok(Some(e));
        }
    }
    Ok(None)
}
// The existing model selects no named/custom variation coordinates. The
// current shaper uses defaults except automatic optical sizing; opsz therefore
// remains rejected until its per-size coordinates have a native representation.
fn default_variations(data:&[u8])->Result<()> {
    if be16(data,0)?!=1||be16(data,2)?!=0||be16(data,6)?!=2||be16(data,10)?!=20 {return Err(unsupported("unknown installed fvar record layout"));}
    let at=be16(data,4)? as usize;let count=be16(data,8)? as usize;let instances=be16(data,12)? as usize;let instance_size=be16(data,14)? as usize;
    if at<16||count>64||![4+count*4,6+count*4].contains(&instance_size) {return Err(unsupported("variable font axes/instance bounds unsupported"));}
    bytes(data,at,count*20+instances*instance_size)?;let mut tags=std::collections::BTreeSet::new();
    for i in 0..count {let row=bytes(data,at+i*20,20)?;let tag=bytes(row,0,4)?;
        if !tags.insert(tag.to_vec())||be16(row,16)?&!1!=0 {return Err(invalid("duplicate/unknown variable font axis"));}
        let values=[4,8,12].map(|p|be32(row,p).map(|v|v as i32));let [min,default,max]=values.map(|r|r.unwrap());
        if min>default||default>max {return Err(invalid("variable font default outside its axis range"));}
        if tag==b"opsz" {return Err(unsupported("automatic optical-size coordinates require native font-axis preservation"));}
    }Ok(())
}
fn be16(data:&[u8],at:usize)->Result<u16>{Ok(u16::from_be_bytes(bytes(data,at,2)?.try_into().unwrap()))}
fn be32(data:&[u8],at:usize)->Result<u32>{Ok(u32::from_be_bytes(bytes(data,at,4)?.try_into().unwrap()))}
fn sfnt(data:&[u8],index:u32)->Result<BTreeMap<[u8;4],&[u8]>> {
    let at=if data.starts_with(b"ttcf") {let n=be32(data,8)?;if index>=n {return Err(invalid("font collection face index differs"));}be32(data,12+index as usize*4)? as usize}else{if index!=0 {return Err(invalid("non-collection font has a nonzero index"));}0};
    let signature=bytes(data,at,4)?;if signature!=[0,1,0,0]&&signature!=b"OTTO" {return Err(unsupported("installed bitmap/other font technology needs its own native profile"));}
    let count=be16(data,at+4)? as usize;let mut tables=BTreeMap::new();
    for i in 0..count {let p=at+12+i*16;let tag=bytes(data,p,4)?.try_into().unwrap();let offset=be32(data,p+8)? as usize;let len=be32(data,p+12)? as usize;if tables.insert(tag,bytes(data,offset,len)?).is_some(){return Err(invalid("duplicate installed SFNT table"));}}
    let cff=tables.contains_key(b"CFF ");
    if tables.contains_key(b"CFF2")||cff&&tables.contains_key(b"fvar")||!cff&&!tables.contains_key(b"glyf") {return Err(unsupported("CFF2/variable CFF/non-outline declaration remains unverified"));}
    if let Some(fvar)=tables.get(b"fvar"){default_variations(fvar)?;}Ok(tables)
}
fn names(data:&[u8])->Result<BTreeMap<u16,String>> {
    if be16(data,0)?>1 {return Err(unsupported("installed name table version is unsupported"));}let count=be16(data,2)? as usize;let start=be16(data,4)? as usize;let mut result=BTreeMap::new();let mut ranks=BTreeMap::new();
    for i in 0..count {let p=6+i*12;let platform=be16(data,p)?;let encoding=be16(data,p+2)?;let language=be16(data,p+4)?;let key=be16(data,p+6)?;
        let unicode=platform==0 || (platform==3 && matches!(encoding,0|1|10));let mac_ascii=platform==1 && encoding==0 && language==0;
        if !unicode && !mac_ascii {continue;}
        let raw=bytes(data,start+be16(data,p+10)? as usize,be16(data,p+8)? as usize)?;
        let value=if unicode {if raw.len()%2!=0 {return Err(invalid("odd UTF16BE installed font name"));}let units=raw.chunks_exact(2).map(|b|u16::from_be_bytes([b[0],b[1]])).collect::<Vec<_>>();String::from_utf16(&units).map_err(|_|invalid("invalid installed UTF16 font name"))?}else{if !raw.is_ascii(){continue;}String::from_utf8(raw.to_vec()).unwrap()};
        if value.is_empty() || value.contains('\0'){continue;}let rank=if platform==3&&language==0x409 {4}else if platform==0 {3}else if unicode {2}else{1};
        if rank>*ranks.get(&key).unwrap_or(&0){ranks.insert(key,rank);result.insert(key,value);}
    }Ok(result)
}
fn installed(family:&str)->Result<Entry> {
    let db=crate::text::font_database();let id=db.query(&fontdb::Query{families:&[fontdb::Family::Name(family)],..Default::default()}).ok_or_else(||unsupported(format!("{family}: no installed face or verified native cache; install/substitution is not automatic")))?;
    let face=db.face(id).ok_or_else(||invalid("resolved font face vanished"))?;
    if !face.families.iter().any(|(name,_)|name.eq_ignore_ascii_case(family)) {return Err(unsupported("font cache cannot use a substituted family"));}
    if face.style!=fontdb::Style::Normal {return Err(unsupported("a non-upright resolved font needs explicit native face styling"));}
    let mut e=db.with_face_data(id,|data,index|->Result<Entry>{
        let tables=sfnt(data,index)?;let os2=tables.get(b"OS/2").ok_or_else(||unsupported("installed font has no OS/2 metadata"))?;let name=names(tables.get(b"name").ok_or_else(||unsupported("installed font has no name table"))?)?;
        let cff=tables.contains_key(b"CFF ");let mut header=[0;18];header[4..8].copy_from_slice(&64u32.to_le_bytes());if !cff{header[8..18].copy_from_slice(bytes(os2,32,10)?);}
        // CP936 is an actual SFNT code-page declaration, not a guess from the
        // user's language. Other scripts retain Unicode text under Western.
        let chinese=!cff&&be16(os2,0)?>=1 && be32(os2,78)?&(1<<18)!=0;let charset=if chinese {134u16}else{0};header[2..4].copy_from_slice(&charset.to_le_bytes());
        // Corel's original CFF files declare face64/PANOSE0 and omit the
        // cache entry. After Corel edits them, BOTH declaration and cache
        // change to actual face metrics/type5. Preserve verified cached rows
        // verbatim, but use the original declaration-only form for a new CFF.
        Ok(Entry{header,family:family.into(),script:if chinese {"CHINESE_GB2312"}else{"Western"}.into(),used:1,technology:if cff{5}else{4},postscript:face.post_script_name.clone(),version:name.get(&5).cloned().ok_or_else(||unsupported("installed font version name absent"))?,subfamily:name.get(&2).cloned().ok_or_else(||unsupported("installed font subfamily absent"))?,cached:!cff})
    }).ok_or_else(||unsupported("installed font bytes unavailable"))??;
    // Check all bounded native fields before a file can be created.
    e.header[..2].copy_from_slice(&0u16.to_le_bytes());e.fixed()?;Ok(e)
}
fn definition(family:&str)->Result<Entry> {
    static CACHE:OnceLock<Mutex<BTreeMap<String,Result<Entry>>>>=OnceLock::new();let mut cache=CACHE.get_or_init(Default::default).lock().expect("CDR font metadata cache");
    cache.entry(family.into()).or_insert_with(|| {
        if let Some(e)=known(family)? {return Ok(e);}
        let db=crate::text::font_database();
        if db.query(&fontdb::Query{families:&[fontdb::Family::Name(family)],..Default::default()}).is_none() {if let Some(e)=reference(family)? {return Ok(e);}}
        installed(family)
    }).clone()
}
pub(super) fn check(family:&str)->Result<()> {definition(family).map(|_|())}
pub(super) fn native_family(family:&str)->Result<String> {definition(family).map(|e|e.family)}
#[derive(Clone,Copy)]
pub(super) struct Choice {pub id:u16,pub charset:u16,pub legacy:bool}
impl Choice {pub fn language(self)->[u8;6]{if self.charset==134 {[b'C',0,b'H',0,b'C',0]}else{[b'E',0,b'N',0,b'U',0]}}}
pub(super) fn script_runs(text:&str,charset:u16)->Vec<(Vec<u16>,u16)> {
    if charset!=134||text.is_empty(){return vec![(text.encode_utf16().collect(),charset)];}
    let mut runs:Vec<(Vec<u16>,u16)>=vec![];let mut current=charset;
    for ch in text.chars() {
        let code=ch as u32;
        if (0x2e80..=0x9fff).contains(&code)||(0xf900..=0xfaff).contains(&code)||(0x20000..=0x3ffff).contains(&code){current=134;}
        else if code<0x2e80&&ch.is_alphanumeric(){current=0;}
        if runs.last().is_none_or(|r|r.1!=current){runs.push((vec![],current));}
        let mut units=[0u16;2];runs.last_mut().unwrap().0.extend_from_slice(ch.encode_utf16(&mut units));
    }runs
}
pub(super) struct Fonts {entries:Vec<Entry>,choices:BTreeMap<String,Choice>}
impl Fonts {
    pub fn new(doc:&Document)->Result<Self> {
        let mut entries=baseline()?;let mut choices=BTreeMap::new();let mut next=entries.iter().map(Entry::id).max().unwrap_or(0).checked_add(1).ok_or_else(||unsupported("font ID overflow"))?;
        for o in &doc.objects {if let Geometry::Text{font_family,..}=&o.geometry {
            if choices.contains_key(font_family) {continue;}let mut entry=definition(font_family)?;
            if entry.id()==0 {entry.header[..2].copy_from_slice(&next.to_le_bytes());next=next.checked_add(1).ok_or_else(||unsupported("font ID overflow"))?;}
            let choice=Choice{id:entry.id(),charset:entry.charset(),legacy:entry.technology==2};
            if !entries.iter().any(|e|e.header[..4]==entry.header[..4]) {entries.push(entry);}choices.insert(font_family.clone(),choice);
        }}Ok(Self{entries,choices})
    }
    pub fn choice(&self,family:&str)->Result<Choice>{self.choices.get(family).copied().ok_or_else(||invalid("encoded text font was not registered"))}
    pub fn apply(&self,tree:&mut [Node])->Result<()> {
        fn replace(nodes:&mut [Node],records:&[Node])->bool {for n in nodes {if n.kind()==Some(b"fntt") {n.children=Some(records.to_vec());return true;}if let Some(c)=&mut n.children {if replace(c,records){return true;}}}false}
        let records=self.entries.iter().map(|e|e.fixed().map(|d|Node::leaf(b"font",d))).collect::<Result<Vec<_>>>()?;
        if !replace(tree,&records){return Err(invalid("native font declaration list absent"));}Ok(())
    }
    pub fn native_face(&self,family:&str,size:f64)->Result<serde_json::Value> {
        let choice=self.choice(family)?;let e=self.entries.iter().find(|e|e.id()==choice.id&&e.charset()==choice.charset).ok_or_else(||invalid("native21 font declaration absent"))?;
        let mut value=serde_json::json!({"font":e.family,"size":coord(size)?.to_string(),"charset":choice.charset.to_string(),"effect":"0","overline":"0","script":"0","strikeout":"0","underline":"0","weight":"64","italic":"0"});
        if [2,4].contains(&e.technology){value["version"]=e.version.clone().into();value["technology"]=e.technology.to_string().into();}Ok(value)
    }
    pub fn encoded_cache(&self)->Vec<u8>{encode_cache(&self.entries)}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn measured_legacy_font_keeps_its_actual_cache_technology_and_alias() {
        let entry=definition("FZHei-B01S").unwrap();assert_eq!(entry.family,"方正黑体简体");assert_eq!(entry.technology,2);assert_eq!(entry.version,"2.00");assert_eq!(entry.charset(),134);
        assert_eq!(cache(&encode_cache(&[entry.clone()])).unwrap(),vec![entry]);
        assert!(!verified_legacy("Arial").unwrap());
    }
    #[test]
    fn chinese_and_latin_character_runs_keep_native_script_slots_and_surrogate_pairs() {
        let actual=script_runs("沿路径 / Vector",134);
        assert_eq!(actual,vec![("沿路径 / ".encode_utf16().collect(),134),("Vector".encode_utf16().collect(),0)]);
        let actual=script_runs("A𠮷B",134);
        assert_eq!(actual,vec![(vec![65],0),("𠮷".encode_utf16().collect(),134),(vec![66],0)]);
    }
    #[test]
    fn measured_supplementary_font_caches_keep_native_technology_and_metrics() {
        for (family,version) in [("SimSun-ExtB","Version 5.08"),("SimSun-ExtG","Version 1.02")] {
            let entry=definition(family).unwrap();assert_eq!(entry.family,family);assert_eq!(entry.technology,2);assert_eq!(entry.charset(),134);assert_eq!(entry.version,version);assert_eq!(entry.postscript,family);
            assert_eq!(cache(&encode_cache(&[entry.clone()])).unwrap(),vec![entry]);
        }
    }
    #[test]
    fn original_font_caches_and_fixed_declarations_roundtrip_without_guesses() {
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports/windows/WIN-SAVE-ROUND-20261024-01/fonts-native");
        for item in std::fs::read_dir(root).unwrap() {let p=item.unwrap().path();if p.extension().is_none_or(|s|s!="cdr"){continue;}let raw=std::fs::read(p).unwrap();let members=unzip(&raw).unwrap();let data=&members["font/fontTable.dat"];let entries=cache(data).unwrap();assert_eq!(encode_cache(&entries),*data);
            let tree=read_native(&raw).unwrap();let records=all(&tree.nodes).into_iter().filter(|n|n.tag==*b"font").collect::<Vec<_>>();assert_eq!(records.len(),entries.len());for e in entries {assert_eq!(records.iter().find(|n|n.data[..4]==e.header[..4]).unwrap().data,e.fixed().unwrap());}
        }
    }
    #[test]
    fn real_variable_default_cache_matches_fixed_declarations_and_local_default_axes() {
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports/windows/WIN-SAVE-CLOSURE-20261024-01/completion-controls/font-variable");
        for version in [14,21] {for edit in ["","-edited"] {let raw=std::fs::read(root.join(format!("font-variable-v{version}{edit}.cdr"))).unwrap();let native=read_native(&raw).unwrap();let entries=profile_cache(&raw).unwrap();
            let entry=entries.iter().find(|e|e.family=="Bahnschrift"&&e.used==1).unwrap();assert_eq!(entry.technology,4);assert_eq!(entry.postscript,"Bahnschrift");
            assert_eq!(all(&native.nodes).into_iter().find(|n|n.tag==*b"font"&&n.data[..4]==entry.header[..4]).unwrap().data,entry.fixed().unwrap());
        }}
        let db=crate::text::font_database();let mut checked=std::collections::BTreeSet::new();let mut accepted=vec![];let mut rejected=vec![];
        for face in db.faces() {let Some((family,_))=face.families.first() else{continue};if !checked.insert(family.clone()){continue;}
            let Some((variable,outline))=db.with_face_data(face.id,|data,index| {
                let offset=if data.starts_with(b"ttcf"){be32(data,12+index as usize*4).unwrap_or(0) as usize}else{0};let count=be16(data,offset+4).unwrap_or(0) as usize;
                let tags=(0..count).filter_map(|i|bytes(data,offset+12+i*16,4).ok()).collect::<Vec<_>>();(tags.contains(&b"fvar".as_slice()),tags.contains(&b"glyf".as_slice()))
            }) else{continue};if !variable||!outline{continue;}
            match installed(family) {Ok(e)=>{assert_eq!(e.technology,4);assert_eq!(cache(&encode_cache(&[e.clone()])).unwrap(),vec![e]);accepted.push(family.clone());},Err(e)=>rejected.push(format!("{family}: {}",e.message))}
        }
        println!("VARIABLE_DEFAULT_LOCAL={}",serde_json::json!({"accepted":accepted,"rejected":rejected}));
    }
    #[test]
    fn genuine_cff_cache5_and_declarations_never_invent_true_type4() {
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports/windows/WIN-SAVE-CLOSURE-20261024-01/completion-controls/font-cff");
        for version in [14,21] {for suffix in ["","-edited"] {
            let raw=std::fs::read(root.join(format!("font-cff-v{version}{suffix}.cdr"))).unwrap();let native=read_native(&raw).unwrap();let entries=profile_cache(&raw).unwrap();
            let record=all(&native.nodes).into_iter().find(|n|n.tag==*b"font"&&read_wide(bytes(&n.data,18,64).unwrap()).unwrap()=="Source Han Sans JP").unwrap();
            let mut entry=if suffix.is_empty(){reference("Source Han Sans JP").unwrap().unwrap()}else{entries.iter().find(|e|e.family=="Source Han Sans JP").unwrap().clone()};assert_eq!(entry.technology,5);entry.header[..2].copy_from_slice(&record.data[..2]);
            assert_eq!(entry.fixed().unwrap(),record.data);
            assert_eq!(encode_cache(&entries),unzip(&raw).unwrap()["font/fontTable.dat"]);
            if let Some(actual)=entries.iter().find(|e|e.family=="Source Han Sans JP"){assert_eq!(actual.technology,5);assert_eq!(actual.fixed().unwrap(),record.data);}
        }}
        let db=crate::text::font_database();let mut checked=vec![];
        for family in ["PingFang SC","Hiragino Sans"] {if db.query(&fontdb::Query{families:&[fontdb::Family::Name(family)],..Default::default()}).is_none(){continue;}
            let e=installed(family).unwrap();assert_eq!(e.technology,5);assert!(!e.cached);assert_eq!(e.header[8..18],[0;10]);assert_eq!(e.fixed().unwrap().len(),296);assert!(cache(&encode_cache(&[e])).unwrap().is_empty());checked.push(family);
        }
        println!("CFF_DECLARATION_LOCAL={}",serde_json::json!(checked));
    }
    #[test]
    fn installed_true_type_metadata_and_corrupt_cache_failures() {
        let db=crate::text::font_database();let mut count=0;
        for family in ["Times New Roman","Helvetica","DejaVu Sans","Liberation Sans","Calibri"] {if db.query(&fontdb::Query{families:&[fontdb::Family::Name(family)],..Default::default()}).is_none(){continue;}let e=installed(family).unwrap_or_else(|e|panic!("{family}: {e}"));assert_eq!(e.family,family);assert!(!e.postscript.is_empty());assert_eq!(e.fixed().unwrap().len(),296);assert_eq!(cache(&encode_cache(&[e.clone()])).unwrap(),vec![e]);count+=1;}
        assert!(count>0,"one installed common TrueType font is required for this platform test");
        assert!(check("Arranger Missing Font 6673f2d0").is_err());let original=encode_cache(&baseline().unwrap());for len in [0,4,11,original.len()-1] {assert!(cache(&original[..len]).is_err());}
    }
}
