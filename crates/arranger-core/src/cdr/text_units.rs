//! A native description addresses one scalar. Bit32 extends its UTF16 payload
//! from two to four bytes; measured in the Owner's actual UI-entered control.
//! Legacy two-description UTF16 pairs remain readable for existing exports.
use super::{binary::*,invalid,unsupported,Result};
const PAIR:u64=1<<32;

pub(super) fn count(units:&[u16])->Result<usize> {
    std::char::decode_utf16(units.iter().copied()).try_fold(0,|n,c|c.map(|_|n+1).map_err(|_|invalid("invalid source UTF16 run")))
}
pub(super) fn descriptions(out:&mut Vec<u8>,units:&[u16],style:usize)->Result<()> {
    if style>0x7fff {return Err(unsupported("native character style index exceeds this encoder's 15-bit field; no style bits may overlap the four-byte scalar marker"));}
    for c in std::char::decode_utf16(units.iter().copied()) {
        let c=c.map_err(|_|invalid("invalid source UTF16 run"))?;
        let d=((style as u64)*0x20000)|1|if c as u32>0xffff {PAIR}else{0};
        out.extend(d.to_le_bytes());
    }Ok(())
}
/// Actual Corel109 resaves use one-byte ASCII descriptions. Only ASCII is
/// independent of the declared native charset; keep every other scalar in
/// UTF16, including legacy CJK declarations and supplementary pairs.
pub(super) fn encoded(units:&[u16],style:usize,compact_ascii:bool)->Result<(Vec<u8>,Vec<u8>)> {
    let mut records=vec![];descriptions(&mut records,units,style)?;let mut payload=vec![];
    for (record,ch) in records.chunks_exact_mut(8).zip(std::char::decode_utf16(units.iter().copied())) {
        let ch=ch.map_err(|_|invalid("invalid source UTF16 run"))?;
        if compact_ascii&&ch.is_ascii(){record[0]&=!1;payload.push(ch as u8);}else{let mut pair=[0;2];for unit in ch.encode_utf16(&mut pair){payload.extend(unit.to_le_bytes());}}
    }Ok((records,payload))
}
pub(super) fn decode(d:u64,encoded:&[u8],at:&mut usize,units:&mut Vec<u16>)->Result<()> {
    if d&PAIR!=0 {
        if d&1==0 {return Err(invalid("four-byte native text scalar lacks UTF16 width flag"));}
        let high=u16_at(encoded,*at)?;let low=u16_at(encoded,*at+2)?;
        if !(0xd800..=0xdbff).contains(&high)||!(0xdc00..=0xdfff).contains(&low){return Err(invalid("four-byte native text scalar is not a surrogate pair"));}
        units.extend([high,low]);*at+=4;
    }else if d&1!=0 {units.push(u16_at(encoded,*at)?);*at+=2;}
    else{units.push(bytes(encoded,*at,1)?[0] as u16);*at+=1;}
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_ascii_keeps_style_and_non_ascii_scalar_boundaries_exact() {
        let units="One é中𝄞".encode_utf16().collect::<Vec<_>>();
        let (records,payload)=encoded(&units,3,true).unwrap();let mut at=0;let mut actual=vec![];
        for (record,ch) in records.chunks_exact(8).zip("One é中𝄞".chars()) {let d=u64::from_le_bytes(record.try_into().unwrap());assert_eq!((d as u32>>16)/2,3);assert_eq!(d&1,if ch.is_ascii(){0}else{1});decode(d,&payload,&mut at,&mut actual).unwrap();}
        assert_eq!(actual,units);assert_eq!(at,payload.len());assert_eq!(payload.len(),12);
        let (_,wide)=encoded(&units,3,false).unwrap();assert_eq!(wide.len(),units.len()*2);
    }
    #[test]
    fn actual_native_ui_pair_marker_is_one_description_and_four_encoded_bytes() {
        // ui-supplementary-v21.cdr: exact description 0100000001000000,
        // followed by UTF16 34d81edd (U+1D11E), not a pair of descriptions.
        let raw=[0x34,0xd8,0x1e,0xdd];let mut at=0;let mut units=vec![];
        decode(0x100000001,&raw,&mut at,&mut units).unwrap();
        assert_eq!(String::from_utf16(&units).unwrap(),"𝄞");assert_eq!(at,4);assert_eq!(count(&units).unwrap(),1);
        let mut out=vec![];descriptions(&mut out,&units,0).unwrap();assert_eq!(out,[1,0,0,0,1,0,0,0]);
    }
    #[test]
    fn legacy_pairs_stay_readable_but_malformed_explicit_pairs_reject() {
        let mut at=0;let mut units=vec![];let raw=[0x34,0xd8,0x1e,0xdd];
        decode(1,&raw,&mut at,&mut units).unwrap();decode(1,&raw,&mut at,&mut units).unwrap();assert_eq!(String::from_utf16(&units).unwrap(),"𝄞");
        for (d,data) in [(PAIR,raw.as_slice()),(PAIR|1,&raw[..2]),(PAIR|1,&[65,0,66,0]),(PAIR|1,&[0x1e,0xdd,0x34,0xd8])] {
            assert!(decode(d,data,&mut 0,&mut vec![]).is_err());
        }
        let units="A𠮷𝄞B".encode_utf16().collect::<Vec<_>>();assert_eq!(count(&units).unwrap(),4);
        let mut out=vec![];descriptions(&mut out,&units,2).unwrap();assert_eq!(out.len(),32);
        let mut out=vec![];descriptions(&mut out,&[65],0x7fff).unwrap();assert_eq!(u64::from_le_bytes(out[..8].try_into().unwrap())&PAIR,0);
        let before=out.clone();assert_eq!(descriptions(&mut out,&[65],0x8000).unwrap_err().code,"CDR_UNSUPPORTED");assert_eq!(out,before);
    }
}
