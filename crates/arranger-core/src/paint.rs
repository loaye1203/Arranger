//! Solid paint values. Native CMYK channels survive editing and saving;
//! RGB previews/exports are explicitly unmanaged approximations, not ICC.
use crate::{Document,Error,Result,command::Notice};

#[derive(Clone,Copy,Debug,PartialEq)]
pub enum Paint { None, Rgba([u8;4]), RgbaFloat{rgb:[u8;3],alpha:f64}, Cmyk{channels:[u8;4],scale:u16} }
pub fn parse(value:&str)->Result<Paint> {
    let invalid=||Error::new("INVALID_STYLE","Paint must be none, #RRGGBB/#RRGGBBAA, rgba(R,G,B,A), cmyk(0..100,...) or cmyk255(0..255,...); RGB/CMYK use canonical integer channels, rgba alpha a canonical finite number in0..1");
    if value=="none" {return Ok(Paint::None);}
    if let Some(hex)=value.strip_prefix('#') {
        if ![6,8].contains(&hex.len()) || !hex.bytes().all(|b|b.is_ascii_hexdigit()) {return Err(invalid());}
        let mut rgba=[0,0,0,255];for (i,pair) in hex.as_bytes().chunks_exact(2).enumerate(){rgba[i]=u8::from_str_radix(std::str::from_utf8(pair).unwrap(),16).map_err(|_|invalid())?;}return Ok(Paint::Rgba(rgba));
    }
    if let Some(raw)=value.strip_prefix("rgba(").and_then(|s|s.strip_suffix(')')) {
        let parts=raw.split(',').collect::<Vec<_>>();if parts.len()!=4{return Err(invalid());}let mut rgb=[0;3];
        for(i,part)in parts[..3].iter().enumerate(){let v=part.parse::<u8>().map_err(|_|invalid())?;if v.to_string()!=*part{return Err(invalid());}rgb[i]=v;}
        let alpha=parts[3].parse::<f64>().map_err(|_|invalid())?;
        if !alpha.is_finite()||!(0.0..=1.0).contains(&alpha)||alpha.to_string()!=parts[3]||alpha.is_sign_negative(){return Err(invalid());}
        return Ok(Paint::RgbaFloat{rgb,alpha});
    }
    for (prefix,scale) in [("cmyk(",100u16),("cmyk255(",255)] {
        if let Some(raw)=value.strip_prefix(prefix).and_then(|s|s.strip_suffix(')')) {
            let parts=raw.split(',').collect::<Vec<_>>();if parts.len()!=4 {return Err(invalid());}let mut channels=[0;4];
            for (i,part) in parts.iter().enumerate(){let v=part.parse::<u8>().map_err(|_|invalid())?;if u16::from(v)>scale || v.to_string()!=*part {return Err(invalid());}channels[i]=v;}
            return Ok(Paint::Cmyk{channels,scale});
        }
    }Err(invalid())
}
impl Paint {
    pub fn alpha(self)->f64 {match self{Self::None=>0.,Self::Rgba(v)=>f64::from(v[3])/255.,Self::RgbaFloat{alpha,..}=>alpha,Self::Cmyk{..}=>1.}}
    /// Quantized swatches only. Rendering and serialization use alpha() directly.
    pub fn rgba(self)->[u8;4] {match self {
        Self::None=>[0,0,0,0],Self::Rgba(v)=>v,
        Self::RgbaFloat{rgb:[r,g,b],alpha}=>[r,g,b,(alpha*255.).round_ties_even() as u8],
        Self::Cmyk{channels:[c,m,y,k],scale}=>{
            let s=u32::from(scale);let den=s*s;let rgb=|v:u8|(((s-u32::from(v))*(s-u32::from(k))*255+den/2)/den) as u8;
            [rgb(c),rgb(m),rgb(y),255]
        },
    }}
    pub fn is_cmyk(self)->bool {matches!(self,Self::Cmyk{..})}
}
pub fn has_cmyk(doc:&Document)->bool {doc.objects.iter().any(|o|[&o.style.fill,&o.style.stroke].into_iter().any(|s|parse(s).is_ok_and(Paint::is_cmyk)))}
pub fn warnings(doc:&Document)->Vec<Notice> {
    if has_cmyk(doc) {vec![Notice::new("CMYK_RGB_APPROXIMATION","CMYK channel values remain in the document and supported native CDR paint records. Screen, SVG, PNG and PDF use an unmanaged RGB approximation; no ICC conversion, separation or print-color guarantee.")]}else{vec![]}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn channels_and_preview_have_strict_ranges_and_preserve_existing_rgba() {
        assert_eq!(parse("#12345680").unwrap().rgba(),[18,52,86,128]);
        for (value,rgb) in [("cmyk(0,0,0,100)",[0,0,0,255]),("cmyk(100,0,0,0)",[0,255,255,255]),("cmyk255(128,0,0,0)",[127,255,255,255])] {assert_eq!(parse(value).unwrap().rgba(),rgb);}
        for value in ["cmyk(101,0,0,0)","cmyk255(256,0,0,0)","cmyk(1.5,0,0,0)","cmyk(0,0,0)","cmyk(00,0,0,0)","cmyk(0, 0,0,0)","cmyk(-1,0,0,0)","#1234567","rgb(1,2,3)"] {assert!(parse(value).is_err(),"{value}");}
    }
    #[test]
    fn decimal_alpha_is_finite_canonical_and_not_quantized_to_a_byte() {
        for alpha in [0.,0.5,0.12345678901234568,1.,f64::MIN_POSITIVE] {let value=format!("rgba(30,90,180,{alpha})");let p=parse(&value).unwrap();assert_eq!(p.alpha(),alpha);assert!(matches!(p,Paint::RgbaFloat{rgb:[30,90,180],..}));}
        assert_ne!(parse("rgba(30,90,180,0.5)").unwrap().alpha(),128./255.);
        for value in ["rgba(1,2,3,NaN)","rgba(1,2,3,inf)","rgba(1,2,3,-0)","rgba(1,2,3,1.1)","rgba(1,2,3,0.50)","rgba(1,2,3,1e-2)","rgba(01,2,3,0.5)","rgba(1,2,3, 0.5)","rgba(256,2,3,0.5)","rgba(1,2,3)"] {assert!(parse(value).is_err(),"{value}");}
    }
}
