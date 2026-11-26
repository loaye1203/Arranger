//! Bounded CDR14 RIFF/CPng and ZIP containers. No external executable is used.
use super::{invalid, Result};
use flate2::{read::{DeflateDecoder, ZlibDecoder}, write::ZlibEncoder, Compression};
use std::{collections::BTreeMap, io::{Read, Write}};

pub const LIMIT: usize = 64 * 1024 * 1024;
const EXPANDED_LIMIT: usize = 128 * 1024 * 1024;

pub fn bytes(data: &[u8], at: usize, count: usize) -> Result<&[u8]> {
    data.get(at..at.checked_add(count).ok_or_else(|| invalid("record size overflow"))?)
        .ok_or_else(|| invalid("truncated CDR record"))
}
pub fn u16_at(data: &[u8], at: usize) -> Result<u16> { Ok(u16::from_le_bytes(bytes(data,at,2)?.try_into().unwrap())) }
pub fn u32_at(data: &[u8], at: usize) -> Result<u32> { Ok(u32::from_le_bytes(bytes(data,at,4)?.try_into().unwrap())) }
pub fn i32_at(data: &[u8], at: usize) -> Result<i32> { Ok(u32_at(data,at)? as i32) }
pub fn f64_at(data: &[u8], at: usize) -> Result<f64> {
    let value = f64::from_le_bytes(bytes(data,at,8)?.try_into().unwrap());
    if value.is_finite() { Ok(value) } else { Err(invalid("non-finite CDR matrix")) }
}
pub fn set32(data: &mut [u8], at: usize, value: u32) -> Result<()> {
    data.get_mut(at..at+4).ok_or_else(|| invalid("truncated native template"))?.copy_from_slice(&value.to_le_bytes()); Ok(())
}
pub fn set_coord(data: &mut [u8], at: usize, mm: f64) -> Result<()> { set32(data,at,coord(mm)? as u32) }
pub fn coord(mm: f64) -> Result<i32> {
    let v = (mm * 10000.).round_ties_even();
    if !v.is_finite() || v < i32::MIN as f64 || v > i32::MAX as f64 { return Err(invalid("coordinate exceeds CDR14's signed 0.0001 mm range")); }
    Ok(v as i32)
}
pub fn wide(value: &str) -> Vec<u8> { value.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect() }
pub fn read_wide(data: &[u8]) -> Result<String> {
    if data.len()%2 != 0 { return Err(invalid("odd UTF16 string length")); }
    let units: Vec<_> = data.chunks_exact(2).map(|x|u16::from_le_bytes([x[0],x[1]])).take_while(|v| *v != 0).collect();
    String::from_utf16(&units).map_err(|_|invalid("invalid UTF16 string"))
}
fn expand<R: Read>(reader: R, expected: usize) -> Result<Vec<u8>> {
    if expected > EXPANDED_LIMIT { return Err(invalid("CDR expansion exceeds 128 MiB")); }
    let mut out=Vec::new(); reader.take(expected as u64+1).read_to_end(&mut out).map_err(|e|invalid(e.to_string()))?;
    if out.len()!=expected { return Err(invalid("CDR compressed size mismatch")); } Ok(out)
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node { pub tag: [u8;4], pub data: Vec<u8>, pub children: Option<Vec<Node>> }
impl Node {
    pub fn kind(&self) -> Option<&[u8]> { (self.tag==*b"LIST").then(||self.data.get(..4)).flatten() }
    pub fn leaf(tag: &[u8;4], data: Vec<u8>) -> Self { Self {tag:*tag,data,children:None} }
    pub fn list(kind: &[u8;4], children: Vec<Node>) -> Self { Self {tag:*b"LIST",data:kind.to_vec(),children:Some(children)} }
    pub fn all<'a>(&'a self, out: &mut Vec<&'a Node>) { out.push(self); if let Some(c)=&self.children { for n in c { n.all(out); } } }
    pub fn find(&self, tag: &[u8;4]) -> Result<&Node> {
        let mut all=vec![];self.all(&mut all); let found: Vec<_>=all.into_iter().filter(|n|n.tag==*tag).collect();
        if found.len()!=1 { return Err(invalid(format!("missing or ambiguous {} record",String::from_utf8_lossy(tag)))); } Ok(found[0])
    }
    pub fn find_mut(&mut self, tag: &[u8;4]) -> Result<&mut Node> {
        if self.tag==*tag { return Ok(self); }
        if let Some(children)=&mut self.children { for child in children { if child.has_tag(tag) {return child.find_mut(tag);} } }
        Err(invalid("missing mutable template record"))
    }
    fn has_tag(&self,tag:&[u8;4])->bool { self.tag==*tag || self.children.as_ref().is_some_and(|c|c.iter().any(|n|n.has_tag(tag))) }
    pub fn list_mut(&mut self, kind:&[u8;4])->Result<&mut Node> {
        if self.kind()==Some(kind) {return Ok(self);}
        if let Some(children)=&mut self.children {for child in children {if child.has_kind(kind) {return child.list_mut(kind);}}}
        Err(invalid("missing native template list"))
    }
    fn has_kind(&self,kind:&[u8;4])->bool { self.kind()==Some(kind) || self.children.as_ref().is_some_and(|c|c.iter().any(|n|n.has_kind(kind))) }
}
pub fn all(nodes: &[Node]) -> Vec<&Node> { let mut out=vec![]; for n in nodes { n.all(&mut out); } out }
fn parse(raw: &[u8], sizes: Option<&[u32]>, depth: usize, budget: &mut usize, count: &mut usize) -> Result<Vec<Node>> {
    if depth>32 { return Err(invalid("CDR nesting exceeds 32 levels")); }
    let mut at=0;let mut out=vec![];
    while at<raw.len() {
        *count+=1; if *count>200_000 {return Err(invalid("too many CDR records"));}
        let tag: [u8;4]=bytes(raw,at,4)?.try_into().unwrap(); let number=u32_at(raw,at+4)? as usize;
        let length=if let Some(s)=sizes {*s.get(number).ok_or_else(||invalid("invalid CPng length index"))? as usize} else {number};
        let data=bytes(raw,at+8,length)?; bytes(raw,at+8+length,length&1)?;
        let mut node=Node::leaf(&tag,data.to_vec());
        if tag==*b"LIST" {
            let kind=bytes(data,0,4)?;
            if kind==b"cmpr" {
                let c1=u32_at(data,4)? as usize;let u1=u32_at(data,8)? as usize;
                let c2=u32_at(data,12)? as usize;let u2=u32_at(data,16)? as usize;
                if c1<8 || c2<8 || 20+c1+c2!=data.len() || u2%4!=0 {return Err(invalid("invalid CPng sizes"));}
                let expanded=u1.checked_add(u2).ok_or_else(||invalid("CPng size overflow"))?;
                if expanded>*budget {return Err(invalid("cumulative CDR expansion exceeds 128 MiB"));} *budget-=expanded;
                let mut streams=vec![];let mut pos=20;
                for (c,u) in [(c1,u1),(c2,u2)] {
                    if bytes(data,pos,8)?!=b"CPng\x01\0\x04\0" {return Err(invalid("invalid CPng header"));}
                    let compressed=bytes(data,pos+8,c-8)?;let mut decoder=ZlibDecoder::new(compressed);
                    let expanded=expand(&mut decoder,u)?;
                    if decoder.total_in()!=compressed.len() as u64 {return Err(invalid("extra bytes after CPng stream"));}
                    streams.push(expanded);pos+=c;
                }
                let sizes=streams[1].chunks_exact(4).map(|v|u32::from_le_bytes(v.try_into().unwrap())).collect::<Vec<_>>();
                node.children=Some(parse(&streams[0],Some(&sizes),depth+1,budget,count)?);
            } else if kind!=b"stlt" {
                node.children=Some(parse(&data[4..],sizes,depth+1,budget,count)?);
            }
        }
        // Parsed lists retain their kind and children, not another copy of
        // every descendant payload at each nesting level. stlt stays opaque.
        if node.children.is_some() {node.data.truncate(4);}
        out.push(node); at+=8+length+(length&1);
    }
    Ok(out)
}
pub(super) struct NativeRecords {
    pub version:u16, pub nodes:Vec<Node>,
    #[allow(dead_code)] pub form:[u8;4],
    pub members:BTreeMap<String,Vec<u8>>,
}
/// Resolve the genuine native container first. Version 21 stores every leaf
/// behind an owner/length/offset reference, including opaque stlt tables.
/// Resolving these records is separate from interpreting object semantics.
pub(super) fn read_native(data:&[u8])->Result<NativeRecords> {
    if data.len()>LIMIT {return Err(invalid("CDR input exceeds 64 MiB"));}
    let members=if data.starts_with(b"PK") {unzip(data)?}else{BTreeMap::new()};
    let modern=members.contains_key("content/root.dat");
    if modern && members.contains_key("content/riffData.cdr") {return Err(invalid("ambiguous native CDR root"));}
    let raw=if members.is_empty(){data}else{members.get(if modern {"content/root.dat"}else{"content/riffData.cdr"}).ok_or_else(||invalid("native ZIP has no recognized CDR root"))?.as_slice()};
    let form:[u8;4]=bytes(raw,8,4)?.try_into().unwrap();
    if bytes(raw,0,4)?!=b"RIFF" || ![b"CDRE",b"CDRM"].contains(&&form) || u32_at(raw,4)? as usize+8!=raw.len() {return Err(invalid("requires a genuine bounded CDR14/21 RIFF container"));}
    let mut budget=EXPANDED_LIMIT;let mut count=0;let mut nodes=parse(&raw[12..],None,0,&mut budget,&mut count)?;
    if modern {
        let listing=members.get("content/dataFileList.dat").ok_or_else(||invalid("native external stream catalogue missing"))?;
        let listing=std::str::from_utf8(listing).map_err(|_|invalid("native external catalogue is not UTF8"))?;
        let names=listing.lines().collect::<Vec<_>>();let mut unique=std::collections::BTreeSet::new();
        if names.is_empty() || names.len()>256 {return Err(invalid("invalid native external stream count"));}
        let mut streams=vec![];
        for name in names {
            if name.is_empty() || name.contains(['/', '\\']) || matches!(name,"."|"..") || !unique.insert(name) {return Err(invalid("ambiguous native external stream name"));}
            streams.push(members.get(&format!("content/data/{name}")).ok_or_else(||invalid("referenced native data stream missing"))?);
        }
        fn resolve(nodes:&mut [Node],streams:&[&Vec<u8>],budget:&mut usize)->Result<()> {
            for node in nodes {
                if let Some(children)=&mut node.children {resolve(children,streams,budget)?;continue;}
                let prefix=if node.kind()==Some(b"stlt"){4}else{0};let reference=bytes(&node.data,prefix,16)?;
                if node.data.len()!=prefix+16 {return Err(invalid("unsupported native external reference size"));}
                let owner=u32_at(reference,0)?;let length=u32_at(reference,4)? as usize;
                let payload=if owner==u32::MAX {
                    if length>8 {return Err(invalid("native inline reference exceeds eight bytes"));}bytes(reference,8,length)?
                }else{
                    if u32_at(reference,12)?!=0 {return Err(invalid("nonzero native external reference reserved field"));}
                    let stream=streams.get(owner as usize).ok_or_else(||invalid("native external owner exceeds catalogue"))?;bytes(stream,u32_at(reference,8)? as usize,length)?
                };
                if length>*budget {return Err(invalid("native resolved records exceed 128 MiB"));}*budget-=length;
                let mut data=node.data[..prefix].to_vec();data.extend(payload);node.data=data;
            }Ok(())
        }
        let mut resolved_budget=EXPANDED_LIMIT;resolve(&mut nodes,&streams,&mut resolved_budget)?;
    }
    let versions=all(&nodes).into_iter().filter(|n|n.tag==*b"vrsn").collect::<Vec<_>>();
    if versions.len()!=1 || versions[0].data.len()!=2 {return Err(invalid("requires one exact native version record"));}
    let version=u16_at(&versions[0].data,0)?;
    if !matches!((version,modern,form),(1400,false,[b'C',b'D',b'R',b'E'])|(2100,true,[b'C',b'D',b'R',b'M'])) {return Err(invalid("native version, RIFF form and external-container layout differ"));}
    Ok(NativeRecords{version,nodes,form,members})
}
pub fn read(data: &[u8]) -> Result<Vec<Node>> {
    let native=read_native(data)?;
    if native.version!=1400 {return Err(super::unsupported("Genuine CDR21 external records resolved; its modern object/text styles still require the CDR21 model codec. No partial import was created."));}
    Ok(native.nodes)
}
/// Container proof only: preserve every original native payload and auxiliary
/// member. Keep old streams in their original catalogue slots so opaque native
/// caches cannot accidentally acquire a different owner. This is not a writer
/// from an Arranger model and is deliberately unavailable to product commands.
#[cfg(test)]
pub(super) fn repack21(native:&NativeRecords)->Result<Vec<u8>> {
    if native.version!=2100 {return Err(invalid("native 21 record source required"));}
    let mut members=native.members.clone();let listing=std::str::from_utf8(members.get("content/dataFileList.dat").ok_or_else(||invalid("missing native catalogue"))?).map_err(|_|invalid("invalid native catalogue"))?;
    let names=listing.lines().map(str::to_string).collect::<Vec<_>>();if names.len()>=256 {return Err(invalid("native catalogue has no free owner slot"));}
    let mut name="arranger-records.dat".to_string();while members.contains_key(&format!("content/data/{name}")) {name.insert(0,'_');}
    let owner=names.len() as u32;let mut stream=vec![];let mut nodes=native.nodes.clone();
    fn refs(nodes:&mut [Node],owner:u32,stream:&mut Vec<u8>)->Result<()> {
        for node in nodes {
            if let Some(children)=&mut node.children {refs(children,owner,stream)?;continue;}
            let prefix=if node.kind()==Some(b"stlt"){4}else{0};let raw=&node.data[prefix..];
            let mut reference=vec![];
            if raw.len()<=8 {reference.extend(u32::MAX.to_le_bytes());reference.extend((raw.len() as u32).to_le_bytes());reference.extend(raw);reference.resize(16,0);}
            else{if stream.len().checked_add(raw.len()).is_none_or(|v|v>LIMIT){return Err(invalid("native proof stream exceeds limit"));}reference.extend(owner.to_le_bytes());reference.extend((raw.len() as u32).to_le_bytes());reference.extend((stream.len() as u32).to_le_bytes());reference.extend(0u32.to_le_bytes());stream.extend(raw);}
            let mut data=node.data[..prefix].to_vec();data.extend(reference);node.data=data;
        }Ok(())
    }
    refs(&mut nodes,owner,&mut stream)?;
    let mut body=native.form.to_vec();for node in &nodes {body.extend(encoded(node)?);}members.insert("content/root.dat".into(),record(b"RIFF",&body));
    members.insert("content/dataFileList.dat".into(),(names.into_iter().chain([name.clone()]).collect::<Vec<_>>().join("\n")+"\n").into_bytes());members.insert(format!("content/data/{name}"),stream);
    let output=zip(&members.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>());if output.len()>LIMIT{return Err(invalid("native proof package exceeds 64 MiB"));}Ok(output)
}

/// Emit a genuine native21 root with an explicit, fresh external owner stream.
/// All caller data is typed native content; no original drawing/preview/cache
/// members or external references from a reference document are copied here.
pub(super) fn write21(nodes:&[Node],mut members:BTreeMap<String,Vec<u8>>)->Result<Vec<u8>> {
    write_limits(nodes,EXPANDED_LIMIT,200_000)?;
    fn reference(nodes:&mut [Node],stream:&mut Vec<u8>)->Result<()> {
        for n in nodes {if let Some(c)=&mut n.children {reference(c,stream)?;continue;}
            if n.kind()==Some(b"stlt"){return Err(invalid("native21 style table must be a native leaf"));}
            let raw=&n.data;let mut out=vec![];
            if raw.len()<=8 {out.extend(u32::MAX.to_le_bytes());out.extend((raw.len() as u32).to_le_bytes());out.extend(raw);out.resize(16,0);}
            else {if stream.len().checked_add(raw.len()).is_none_or(|v|v>EXPANDED_LIMIT){return Err(invalid("native21 expanded stream exceeds bounded range"));}out.extend(0u32.to_le_bytes());out.extend((raw.len() as u32).to_le_bytes());out.extend((stream.len() as u32).to_le_bytes());out.extend(0u32.to_le_bytes());stream.extend(raw);}
            n.data=out;
        }Ok(())
    }
    let mut nodes=nodes.to_vec();let mut stream=vec![];reference(&mut nodes,&mut stream)?;let mut body=b"CDRM".to_vec();for n in &nodes {body.extend(encoded(n)?);}
    members.insert("mimetype".into(),b"application/x-vnd.corel.zcf.draw.document+zip".to_vec());members.insert("content/root.dat".into(),record(b"RIFF",&body));members.insert("content/dataFileList.dat".into(),b"arranger.dat\n".to_vec());members.insert("content/data/arranger.dat".into(),stream);
    let out=zip(&members.iter().map(|(n,d)|(n.as_str(),d.as_slice())).collect::<Vec<_>>());if out.len()>LIMIT{return Err(invalid("native21 output exceeds 64 MiB"));}Ok(out)
}

pub fn record(tag:&[u8;4],data:&[u8])->Vec<u8> {
    let mut out=tag.to_vec();out.extend((data.len() as u32).to_le_bytes());out.extend(data);if data.len()%2!=0 {out.push(0);}out
}
fn indexed(node:&Node,sizes:&mut Vec<u32>)->Vec<u8> {
    let data=if let Some(c)=&node.children {let mut d=node.data[..4].to_vec();for n in c {d.extend(indexed(n,sizes));}d} else {node.data.clone()};
    let index=sizes.len() as u32;sizes.push(data.len() as u32);
    let mut out=node.tag.to_vec();out.extend(index.to_le_bytes());out.extend(&data);if data.len()%2!=0 {out.push(0);}out
}
fn encoded(node:&Node)->Result<Vec<u8>> {
    let data=if let Some(c)=&node.children {
        if node.kind()==Some(b"cmpr") {
            let mut sizes=vec![];let body:Vec<_>=c.iter().flat_map(|n|indexed(n,&mut sizes)).collect();let table:Vec<_>=sizes.into_iter().flat_map(u32::to_le_bytes).collect();
            let mut compressed=vec![];
            for value in [&body,&table] {let mut z=ZlibEncoder::new(Vec::new(),Compression::best());z.write_all(value).map_err(|e|invalid(e.to_string()))?;let mut p=b"CPng\x01\0\x04\0".to_vec();p.extend(z.finish().map_err(|e|invalid(e.to_string()))?);compressed.push(p);}
            let mut d=b"cmpr".to_vec();for v in [compressed[0].len(),body.len(),compressed[1].len(),table.len()] {d.extend((v as u32).to_le_bytes());}for p in compressed {d.extend(p);}d
        } else {let mut d=node.data[..4].to_vec();for n in c {d.extend(encoded(n)?);}d}
    } else {node.data.clone()};Ok(record(&node.tag,&data))
}
// Match the reader's cumulative CPng and record bounds before allocating its
// uncompressed streams. Many image instances may repeat source attachments.
fn write_limits(nodes:&[Node],expanded_limit:usize,record_limit:usize)->Result<()> {
    fn size(n:&Node,count:&mut usize,expanded:&mut usize)->Result<usize> {
        *count=count.checked_add(1).ok_or_else(||invalid("CDR record count overflow"))?;
        let payload=if let Some(children)=&n.children {
            let before=*count;let mut body=0usize;
            for child in children {body=body.checked_add(size(child,count,expanded)?).ok_or_else(||invalid("CDR expanded size overflow"))?;}
            if n.kind()==Some(b"cmpr") {
                *expanded=expanded.checked_add(body).and_then(|v|v.checked_add((*count-before)*4)).ok_or_else(||invalid("CDR expanded size overflow"))?;
            }
            body.checked_add(4).ok_or_else(||invalid("CDR list size overflow"))?
        }else{n.data.len()};
        payload.checked_add(8+(payload&1)).ok_or_else(||invalid("CDR record size overflow"))
    }
    let mut count=0;let mut expanded=0;
    for node in nodes {size(node,&mut count,&mut expanded)?;}
    if count>record_limit||expanded>expanded_limit{return Err(invalid("CDR output exceeds the reader's expanded/record bounds"));}Ok(())
}
#[cfg(test)]
mod write_limit_tests {
    use super::*;
    #[test]
    fn repeated_compressed_values_obey_cumulative_reader_bounds() {
        let part=Node::list(b"cmpr",vec![Node::leaf(b"udta",vec![0;9])]);
        // One indexed record occupies 18 bytes plus its four-byte size entry.
        assert!(write_limits(std::slice::from_ref(&part),22,2).is_ok());
        assert!(write_limits(std::slice::from_ref(&part),21,2).is_err());
        assert!(write_limits(&[part.clone(),part],43,4).is_err());
        assert!(write_limits(&[Node::list(b"doc ",vec![Node::leaf(b"udta",vec![])])],100,1).is_err());
    }
}
pub fn write(nodes:&[Node])->Result<Vec<u8>> {
    write_limits(nodes,EXPANDED_LIMIT,200_000)?;
    let mut body=b"CDRE".to_vec();for n in nodes {body.extend(encoded(n)?);}if body.len()>LIMIT-8 {return Err(invalid("CDR output exceeds 64 MiB"));}Ok(record(b"RIFF",&body))
}
fn crc32(data:&[u8])->u32 {let mut v=!0u32;for b in data {v^=*b as u32;for _ in 0..8 {v=(v>>1)^if v&1!=0 {0xedb88320}else{0};}}!v}
pub fn zip(members:&[(&str,&[u8])])->Vec<u8> {
    let mut out=vec![];let mut central=vec![];
    for (name,data) in members {
        let offset=out.len() as u32;let crc=crc32(data);let length=data.len() as u32;
        out.extend(0x04034b50u32.to_le_bytes());out.extend([20u16,0,0,0,0].into_iter().flat_map(u16::to_le_bytes));
        out.extend([crc,length,length].into_iter().flat_map(u32::to_le_bytes));out.extend((name.len() as u16).to_le_bytes());out.extend(0u16.to_le_bytes());out.extend(name.as_bytes());out.extend(*data);
        central.extend(0x02014b50u32.to_le_bytes());central.extend([20u16,20,0,0,0,0].into_iter().flat_map(u16::to_le_bytes));
        central.extend([crc,length,length].into_iter().flat_map(u32::to_le_bytes));central.extend([name.len() as u16,0,0,0,0].into_iter().flat_map(u16::to_le_bytes));central.extend(0u32.to_le_bytes());central.extend(offset.to_le_bytes());central.extend(name.as_bytes());
    }
    let at=out.len() as u32;let size=central.len() as u32;out.extend(central);out.extend(0x06054b50u32.to_le_bytes());out.extend([0u16,0,members.len() as u16,members.len() as u16].into_iter().flat_map(u16::to_le_bytes));out.extend(size.to_le_bytes());out.extend(at.to_le_bytes());out.extend(0u16.to_le_bytes());out
}
pub fn unzip(data:&[u8])->Result<BTreeMap<String,Vec<u8>>> {
    if data.len()>LIMIT || data.len()<22 {return Err(invalid("invalid ZIP size"));}
    let end=(data.len().saturating_sub(65557)..=data.len()-22).rev().find(|&i|data.get(i..i+4)==Some(&[0x50,0x4b,0x05,0x06]) && u16_at(data,i+20).is_ok_and(|n|i+22+n as usize==data.len())).ok_or_else(||invalid("missing ZIP directory"))?;
    if u16_at(data,end+4)?!=0 || u16_at(data,end+6)?!=0 || u16_at(data,end+8)?!=u16_at(data,end+10)? {return Err(invalid("multi-disk ZIP unsupported"));}
    let count=u16_at(data,end+10)? as usize;let mut at=u32_at(data,end+16)? as usize;
    if count>4096 || at.checked_add(u32_at(data,end+12)? as usize)!=Some(end) {return Err(invalid("invalid ZIP directory size"));}
    let mut out=BTreeMap::new();let mut total=0usize;
    for _ in 0..count {
        if u32_at(data,at)?!=0x02014b50 {return Err(invalid("invalid ZIP directory entry"));}
        let flags=u16_at(data,at+8)?;let method=u16_at(data,at+10)?;let crc=u32_at(data,at+16)?;let packed=u32_at(data,at+20)? as usize;let size=u32_at(data,at+24)? as usize;
        let n=u16_at(data,at+28)? as usize;let extra=u16_at(data,at+30)? as usize;let comment=u16_at(data,at+32)? as usize;let pos=u32_at(data,at+42)? as usize;
        total=total.checked_add(size).ok_or_else(||invalid("ZIP size overflow"))?;
        if total>EXPANDED_LIMIT || flags&1!=0 || ![0,8].contains(&method) || u16_at(data,at+34)?!=0 {return Err(invalid("unsupported or oversized ZIP member"));}
        let name=std::str::from_utf8(bytes(data,at+46,n)?).map_err(|_|invalid("invalid ZIP name"))?.to_owned();
        if out.contains_key(&name) {return Err(invalid("duplicate ZIP member"));}
        if u32_at(data,pos)?!=0x04034b50 || u16_at(data,pos+6)?!=flags || u16_at(data,pos+8)?!=method {return Err(invalid("ZIP local header differs"));}
        let local_n=u16_at(data,pos+26)? as usize;let local_extra=u16_at(data,pos+28)? as usize;
        if bytes(data,pos+30,local_n)?!=name.as_bytes() {return Err(invalid("ZIP member names differ"));}
        let start=pos+30+local_n+local_extra;
        if start.checked_add(packed).is_none_or(|e|e>u32_at(data,end+16).unwrap_or(0) as usize) {return Err(invalid("ZIP member exceeds content area"));}
        let source=bytes(data,start,packed)?;let raw=if method==0 {if packed!=size {return Err(invalid("stored ZIP size differs"));}source.to_vec()} else {let mut d=DeflateDecoder::new(source);let raw=expand(&mut d,size)?;if d.total_in()!=packed as u64 {return Err(invalid("trailing ZIP compressed bytes"));}raw};
        if crc32(&raw)!=crc {return Err(invalid("ZIP CRC differs"));}out.insert(name,raw);at+=46+n+extra+comment;
    }
    if at!=end {return Err(invalid("ZIP directory entry sizes differ"));}Ok(out)
}

#[cfg(test)]
mod native21_tests {
    use super::*;
    use std::{fs,path::PathBuf};
    #[test]
    fn actual_21_containers_repack_all_payloads_without_claiming_model_support() {
        let root=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let folders=["reports/windows/WIN-CDR-SAVE-20261022-01/focused/native-preservation","reports/windows/WIN-SAVE-ROUND-20261024-01/layout-native","reports/windows/WIN-SAVE-ROUND-20261024-01/fonts-native"];
        let output=root.join(format!("artifacts/cdr21-container-{}",std::process::id()));fs::create_dir_all(&output).unwrap();let mut cases=vec![];
        for folder in folders {
            let mut files=fs::read_dir(root.join(folder)).unwrap().map(|x|x.unwrap().path()).filter(|p|p.file_name().unwrap().to_string_lossy().contains("-v21")&&p.extension().is_some_and(|e|e=="cdr")).collect::<Vec<_>>();files.sort();
            for path in files {
                let original=fs::read(&path).unwrap();let native=read_native(&original).unwrap();assert_eq!(native.version,2100);
                let packed=repack21(&native).unwrap();let again=read_native(&packed).unwrap();assert_eq!(native.nodes,again.nodes);assert_eq!(native.version,again.version);
                for(key,value)in &native.members {if key!="content/root.dat"&&key!="content/dataFileList.dat" {assert_eq!(again.members.get(key),Some(value));}}
                assert_eq!(read(&packed).unwrap_err().code,"CDR_UNSUPPORTED");
                let target=output.join(format!("{:02}-{}",cases.len(),path.file_name().unwrap().to_string_lossy()));fs::write(&target,&packed).unwrap();
                cases.push(serde_json::json!({"source":path.strip_prefix(&root).unwrap(),"output":target.strip_prefix(&root).unwrap(),"source_sha256":crate::model::sha256(&original),"sha256":crate::model::sha256(&packed),"version_x100":2100,"record_count":all(&native.nodes).len()}));
            }
        }
        assert_eq!(cases.len(),42);
        fs::write(output.join("manifest.json"),serde_json::to_vec_pretty(&serde_json::json!({"cases":cases,"scope":"Native CDR21 container reserialization only; object model read/write and new Corel execution are not claimed"})).unwrap()).unwrap();
        fs::write(root.join("artifacts/latest-cdr21-container.json"),serde_json::to_vec(&serde_json::json!({"manifest":output.join("manifest.json").strip_prefix(&root).unwrap()})).unwrap()).unwrap();
        println!("CDR21_RECORD_PROOF={}",output.display());
    }
    #[test]
    fn external_catalogue_and_reference_corruption_never_become_partial_records() {
        let root=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");let raw=fs::read(root.join("reports/windows/WIN-SAVE-ROUND-20261024-01/fonts-native/font-arial-v21.cdr")).unwrap();let original=unzip(&raw).unwrap();
        for change in 0..3 {
            let mut m=original.clone();
            match change {0=>{m.insert("content/dataFileList.dat".into(),b"missing.dat\n".to_vec());},1=>{m.insert("content/dataFileList.dat".into(),b"../escape.dat\n".to_vec());},_=>{let name=std::str::from_utf8(&m["content/dataFileList.dat"]).unwrap().lines().next().unwrap().to_string();m.insert(format!("content/data/{name}"),vec![]);}}
            let bad=zip(&m.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>());assert_eq!(read_native(&bad).err().unwrap().code,"INVALID_CDR");
        }
    }
}
