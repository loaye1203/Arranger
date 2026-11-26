//! Expand smooth SVG commands before kurbo0.13.1's shared-control parser.
//! Original SVG grammar is checked by kurbo first; this adapter changes only
//! derived geometry. The document keeps its exact original source spelling.
use crate::{Error,Result};
use kurbo::Point;
use std::fmt::Write;
fn invalid()->Error{Error::new("INVALID_PATH","Invalid smooth SVG path data")}
struct Cursor<'a>{bytes:&'a[u8],at:usize}
impl Cursor<'_>{
    fn skip(&mut self){while self.at<self.bytes.len()&&(self.bytes[self.at].is_ascii_whitespace()||self.bytes[self.at]==b','){self.at+=1;}}
    fn number(&mut self)->Result<f64>{
        self.skip();let start=self.at;if self.bytes.get(self.at).is_some_and(|b|matches!(b,b'+'|b'-')){self.at+=1;}
        let mut digits=0;while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit){self.at+=1;digits+=1;}
        if self.bytes.get(self.at)==Some(&b'.'){self.at+=1;while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit){self.at+=1;digits+=1;}}
        if digits==0{return Err(invalid());}
        if self.bytes.get(self.at).is_some_and(|b|matches!(b,b'e'|b'E')){
            self.at+=1;if self.bytes.get(self.at).is_some_and(|b|matches!(b,b'+'|b'-')){self.at+=1;}let start=self.at;
            while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit){self.at+=1;}if self.at==start{return Err(invalid());}
        }
        let value=std::str::from_utf8(&self.bytes[start..self.at]).map_err(|_|invalid())?.parse::<f64>().map_err(|_|invalid())?;
        if !value.is_finite(){return Err(invalid());}Ok(value)
    }
    fn flag(&mut self)->Result<f64>{self.skip();let value=match self.bytes.get(self.at){Some(b'0')=>0.,Some(b'1')=>1.,_=>return Err(invalid())};self.at+=1;Ok(value)}
}
pub(super) fn expand(d:&str)->Result<Option<String>>{
    if !d.bytes().any(|b|matches!(b,b'S'|b's'|b'T'|b't')){return Ok(None);}
    let mut input=Cursor{bytes:d.as_bytes(),at:0};let mut command=None;let mut output=String::new();
    let(mut current,mut start)=(Point::ZERO,Point::ZERO);let(mut cubic,mut quadratic)=(None::<Point>,None::<Point>);
    loop{
        input.skip();if input.at==input.bytes.len(){break;}
        if input.bytes[input.at].is_ascii_alphabetic(){command=Some(input.bytes[input.at]);input.at+=1;}
        let cmd=command.ok_or_else(invalid)?;let kind=cmd.to_ascii_uppercase();
        if kind==b'Z'{output.push_str(" Z");current=start;cubic=None;quadratic=None;command=None;continue;}
        let count=match kind{b'M'|b'L'|b'T'=>2,b'H'|b'V'=>1,b'C'=>6,b'Q'|b'S'=>4,b'A'=>7,_=>return Err(invalid())};
        let mut args=[0.;7];for(i,arg)in args[..count].iter_mut().enumerate(){*arg=if kind==b'A'&&matches!(i,3|4){input.flag()?}else{input.number()?};}
        let base=if cmd.is_ascii_lowercase(){current}else{Point::ZERO};let point=|i|Point::new(base.x+args[i],base.y+args[i+1]);
        match kind{
            b'M'|b'L'=>{let p=point(0);write!(output," {} {} {}",kind as char,p.x,p.y).unwrap();current=p;if kind==b'M'{start=p;command=Some(if cmd.is_ascii_lowercase(){b'l'}else{b'L'});}},
            b'H'|b'V'=>{let p=if kind==b'H'{Point::new(base.x+args[0],current.y)}else{Point::new(current.x,base.y+args[0])};write!(output," L {} {}",p.x,p.y).unwrap();current=p;},
            b'C'|b'S'=>{let(a,b,p)=if kind==b'C'{(point(0),point(2),point(4))}else{(cubic.map_or(current,|c|Point::new(2.*current.x-c.x,2.*current.y-c.y)),point(0),point(2))};write!(output," C {} {} {} {} {} {}",a.x,a.y,b.x,b.y,p.x,p.y).unwrap();cubic=Some(b);current=p;},
            b'Q'|b'T'=>{let(q,p)=if kind==b'Q'{(point(0),point(2))}else{(quadratic.map_or(current,|q|Point::new(2.*current.x-q.x,2.*current.y-q.y)),point(0))};write!(output," Q {} {} {} {}",q.x,q.y,p.x,p.y).unwrap();quadratic=Some(q);current=p;},
            b'A'=>{let p=point(5);write!(output," A {} {} {} {} {} {} {}",args[0],args[1],args[2],args[3],args[4],p.x,p.y).unwrap();current=p;},
            _=>unreachable!(),
        }
        if !matches!(kind,b'C'|b'S'){cubic=None;}if !matches!(kind,b'Q'|b'T'){quadratic=None;}
    }Ok(Some(output))
}
#[cfg(test)]
mod tests{
    use super::*;use kurbo::{BezPath,PathEl};
    #[test]
    fn existing_same_family_reflection_keeps_its_native_rounding_order(){for d in ["M0 0 C1 2 17.40123 2 51.64424 0 S90 -2 100 0","M0 0 Q17.40123 2 51.64424 0 T100 0"]{assert_eq!(crate::path::parse(d).unwrap(),BezPath::from_svg(d).unwrap());}}
    #[test]
    fn mixed_curve_families_reset_the_reflection_control(){let mut expected=BezPath::new();expected.move_to((0.,0.));expected.curve_to((1.,2.),(3.,2.),(4.,0.));expected.quad_to((4.,0.),(8.,0.));expected.curve_to((8.,0.),(10.,2.),(12.,0.));assert_eq!(crate::path::parse("M0 0 C1 2 3 2 4 0 T8 0 S10 2 12 0").unwrap(),expected);}
    #[test]
    fn arc_flags_relative_runs_and_close_reset_remain_valid(){
        let path=crate::path::parse("M0 0 A4 4 0 0110 0 t2e0 0").unwrap();assert_eq!(path.elements().last(),Some(&PathEl::QuadTo(Point::new(10.,0.),Point::new(12.,0.))));
        let path=crate::path::parse("m1 2 q1 2 2 0z s1 2 2 0").unwrap();assert_eq!(path.elements().last(),Some(&PathEl::CurveTo(Point::new(1.,2.),Point::new(2.,4.),Point::new(3.,2.))));
        assert!(expand("M0 0 C1 2 3 4 5 6").unwrap().is_none());assert!(crate::path::parse("M0 0 S1,,2 3").is_err());
    }
}
