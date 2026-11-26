//! Native-derived coordinates and source formats; bounded local arc curvature.
use super::{invalid,Result};
use kurbo::{Affine,Arc,BezPath,PathEl,PathSeg,Point,SvgArc,Vec2};
use serde::{Deserialize,Serialize};
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Notation{syntax:String,numbers:Vec<Number>,native_sha256:String,#[serde(default="one",skip_serializing_if="is_one")]units:u32,#[serde(default,skip_serializing_if="Vec::is_empty")]arcs:Vec<ArcShape>}
fn one()->u32{1}fn is_one(value:&u32)->bool{*value==1}
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct Number{decimals:u8,plus:bool,leading_dot:bool,trailing_dot:bool,negative_zero:bool,#[serde(default,skip_serializing_if="Option::is_none")]exponent:Option<Exponent>}
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct Exponent{power:i16,upper:bool,plus:bool,digits:u8,negative_zero:bool}
// Per-command curvature scalars and flags, never an endpoint/control array.
// SVG may enlarge undersized radii; those original local radii cannot be
// recovered uniquely from the native cubic approximation alone.
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct ArcShape{radii:[f64;2],rotation_degrees:f64,large:bool,sweep:bool,segments:u16}
fn arc_path(from:Point,to:Point,shape:&ArcShape)->Result<BezPath>{
    if !shape.radii.iter().chain([&shape.rotation_degrees]).all(|v|v.is_finite())||shape.segments==0{return Err(invalid("invalid local arc curvature parameters"));}
    let svg=SvgArc{from,to,radii:Vec2::new(shape.radii[0],shape.radii[1]),x_rotation:shape.rotation_degrees.to_radians(),large_arc:shape.large,sweep:shape.sweep};let mut path=BezPath::new();path.move_to(from);
    if let Some(arc)=Arc::from_svg_arc(&svg){arc.to_cubic_beziers(0.1,|a,b,p|path.curve_to(a,b,p));}else{path.line_to(to);}Ok(path)
}
fn arc_shapes(syntax:&str,values:&[f64])->Result<Vec<ArcShape>>{
    let mut result=vec![];let mut command=None;let mut at=0;let mut point=Point::ZERO;let mut start=point;
    let mut tokens=syntax.chars().filter(|c|!c.is_ascii_whitespace()&&*c!=',').peekable();
    while let Some(c)=tokens.next(){if c.is_ascii_alphabetic(){if c.eq_ignore_ascii_case(&'Z'){point=start;command=None;}else{command=Some(c);}continue;}
        if c!='@'{return Err(invalid("arc notation literal differs"));}let cmd=command.ok_or_else(||invalid("arc argument lacks command"))?;let kind=cmd.to_ascii_uppercase();let count=match kind{'M'|'L'|'T'=>2,'H'|'V'=>1,'Q'|'S'=>4,'C'=>6,'A'=>7,_=>return Err(invalid("arc syntax command differs"))};
        for _ in 1..count{if tokens.next()!=Some('@'){return Err(invalid("arc syntax argument count differs"));}}let v=values.get(at..at+count).ok_or_else(||invalid("arc number count differs"))?;at+=count;
        let base=if cmd.is_ascii_lowercase(){point}else{Point::ZERO};let pair=|i|Point::new(base.x+v[i],base.y+v[i+1]);
        let end=match kind{'H'=>Point::new(base.x+v[0],point.y),'V'=>Point::new(point.x,base.y+v[0]),'C'=>pair(4),'Q'|'S'=>pair(2),'A'=>pair(5),_=>pair(0)};
        if kind=='A'{if ![0.,1.].contains(&v[3])||![0.,1.].contains(&v[4]){return Err(invalid("arc flags must be binary"));}let mut shape=ArcShape{radii:[v[0],v[1]],rotation_degrees:v[2],large:v[3]==1.,sweep:v[4]==1.,segments:1};let n=arc_path(point,end,&shape)?.elements().len()-1;shape.segments=u16::try_from(n).map_err(|_|invalid("arc native subdivision exceeds range"))?;result.push(shape);}
        point=end;if kind=='M'{start=end;command=Some(if cmd.is_ascii_lowercase(){'l'}else{'L'});}
    }Ok(result)
}
#[cfg(test)]fn projected(d:&str)->Result<BezPath>{super::read::curve(&super::write::path_geometry(&crate::path::parse(d)?)?)}
fn hash(path:&BezPath)->String{crate::model::sha256(path.to_svg().as_bytes())}
pub(super) fn same_world_geometry(actual:&str,candidate:&str,world:Affine)->Result<bool> {
    fn controls(path:&BezPath,world:Affine)->Vec<[Point;4]>{path.segments().map(|s|{
        let points=match s{PathSeg::Line(l)=>[l.p0,l.p0+(l.p1-l.p0)/3.,l.p0+(l.p1-l.p0)*(2./3.),l.p1],PathSeg::Quad(q)=>{let c=q.raise();[c.p0,c.p1,c.p2,c.p3]},PathSeg::Cubic(c)=>[c.p0,c.p1,c.p2,c.p3]};points.map(|p|world*p)
    }).collect()}
    // Compare unquantized controls in page millimetres. A small local change
    // may become a material native edit under a large affine scale.
    let(a,b)=(controls(&crate::path::parse(actual)?,world),controls(&crate::path::parse(candidate)?,world));
    Ok(a.len()==b.len()&&a.into_iter().zip(b).all(|(a,b)|a.into_iter().zip(b).all(|(a,b)|(a.x-b.x).abs()<=0.00021&&(a.y-b.y).abs()<=0.00021)))
}
impl Notation {
    pub fn from_source(d:&str)->Result<Option<Self>> {
        Self::from_source_in_units(d,1)
    }
    pub fn from_source_in_units(d:&str,units:u32)->Result<Option<Self>> {
        let b=d.as_bytes();let mut at=0;let mut syntax=String::new();let mut numbers=vec![];let mut numeric_values=vec![];let mut active=None;let mut arguments=0usize;
        while at<b.len(){
            if matches!(b[at],b'+'|b'-'|b'.'|b'0'..=b'9') {
                let flag=active==Some('A')&&matches!(arguments%7,3|4);
                let start=at;if matches!(b[at],b'+'|b'-'){at+=1;}let mut digits=0;
                while at<b.len()&&b[at].is_ascii_digit(){at+=1;digits+=1;if flag{break;}}
                let mut decimals=0usize;if !flag&&at<b.len()&&b[at]==b'.'{at+=1;while at<b.len()&&b[at].is_ascii_digit(){at+=1;digits+=1;decimals+=1;}}
                if digits==0||decimals>14{return Ok(None);}let mantissa_end=at;
                let exponent=if !flag&&at<b.len()&&matches!(b[at],b'e'|b'E') {
                    let upper=b[at]==b'E';at+=1;let sign=at;if at<b.len()&&matches!(b[at],b'+'|b'-'){at+=1;}let first=at;
                    while at<b.len()&&b[at].is_ascii_digit(){at+=1;}
                    let width=at-first;if !(1..=32).contains(&width){return Ok(None);}
                    let Ok(power)=d[sign..at].parse::<i16>()else{return Ok(None)};if !(-308..=308).contains(&power){return Ok(None);}
                    Some(Exponent{power,upper,plus:b[sign]==b'+',digits:width as u8,negative_zero:b[sign]==b'-'&&power==0})
                }else{None};
                let raw=&d[start..mantissa_end];let value=d[start..at].parse::<f64>().map_err(|_|invalid("invalid path notation number"))?;
                numeric_values.push(value);arguments+=1;
                numbers.push(Number{decimals:decimals as u8,plus:raw.starts_with('+'),leading_dot:raw.trim_start_matches(['+','-']).starts_with('.'),trailing_dot:raw.ends_with('.'),negative_zero:raw.starts_with('-')&&value==0.,exponent});syntax.push('@');
            }else{
                let ch=b[at] as char;if !("MmLlHhVvCcSsQqTtAaZz".contains(ch)||ch.is_ascii_whitespace()||ch==','){return Ok(None);}
                if ch.is_ascii_alphabetic(){active=Some(ch.to_ascii_uppercase());arguments=0;}syntax.push(ch);at+=1;
            }
        }
        let native=super::path_precision::projected(d,units)?;let arcs=arc_shapes(&syntax,&numeric_values)?;let value=Self{syntax,numbers,native_sha256:hash(&native),units,arcs};
        // No off-grid residuals or original coordinate values are stored.
        // Only admit notation that recovers the exact source from native data.
        Ok(value.reconstruct(&native)?.filter(|s|s==d).map(|_|value))
    }
    pub fn restore(&self,d:&str)->Result<Option<String>> {
        let native=super::path_precision::projected(d,self.units)?;if hash(&native)!=self.native_sha256{return Ok(None);}
        let Some(source)=self.reconstruct(&native)?else{return Ok(None)};
        if hash(&super::path_precision::projected(&source,self.units)?)!=self.native_sha256{return Err(invalid("path notation does not reproduce native coordinates"));}Ok(Some(source))
    }
    fn reconstruct(&self,path:&BezPath)->Result<Option<String>> {
        let bytes=self.syntax.as_bytes();let mut at=0;let mut command=None;let mut index=0;let mut current=Point::ZERO;let mut start=Point::ZERO;let mut values=vec![];let mut element=0;let(mut cubic_control,mut quadratic_control)=(None::<Point>,None::<Point>);let mut after_close=false;let mut arc_index=0;
        fn take(path:&BezPath,at:&mut usize)->Option<PathEl>{let el=path.elements().get(*at).copied();if el.is_some(){*at+=1;}el}
        fn near(a:Point,b:Point)->bool{(a.x-b.x).abs()<=0.00021&&(a.y-b.y).abs()<=0.00021}
        while at<bytes.len(){
            let ch=bytes[at] as char;
            if ch.is_ascii_whitespace()||ch==','{at+=1;continue;}
            if ch.is_ascii_alphabetic(){
                if !"MmLlHhVvCcSsQqTtAaZz".contains(ch){return Err(invalid("unmeasured path notation command"));}at+=1;
                if ch.eq_ignore_ascii_case(&'Z') {
                    if let Some(PathEl::LineTo(p))=path.elements().get(element){if near(*p,start){element+=1;}}
                    if take(path,&mut element)!=Some(PathEl::ClosePath){return Ok(None);}current=start;command=None;cubic_control=None;quadratic_control=None;after_close=true;continue;
                }command=Some(ch);continue;
            }
            if ch!='@'{return Err(invalid("path notation contains a coordinate or unknown literal"));}
            let cmd=command.ok_or_else(||invalid("path notation argument lacks command"))?;let count=match cmd.to_ascii_uppercase(){'M'|'L'|'T'=>2,'H'|'V'=>1,'Q'|'S'=>4,'C'=>6,'A'=>7,_=>return Err(invalid("invalid path notation command"))};
            for _ in 0..count {while at<bytes.len()&&(bytes[at].is_ascii_whitespace()||bytes[at]==b','){at+=1;}if bytes.get(at)!=Some(&b'@'){return Err(invalid("path notation argument count differs"));}at+=1;index+=1;}
            if after_close&&!cmd.eq_ignore_ascii_case(&'M') {if let Some(PathEl::MoveTo(p))=path.elements().get(element){if near(*p,current){element+=1;}}}after_close=false;
            let base=if cmd.is_ascii_lowercase(){current}else{Point::ZERO};
            match (cmd.to_ascii_uppercase(),take(path,&mut element)) {
                ('M',Some(PathEl::MoveTo(p)))=>{values.extend([p.x-base.x,p.y-base.y]);current=p;start=p;command=Some(if cmd.is_ascii_lowercase(){'l'}else{'L'});},
                ('L',Some(PathEl::LineTo(p)))=>{values.extend([p.x-base.x,p.y-base.y]);current=p;},
                ('H',Some(PathEl::LineTo(p))) if (p.y-current.y).abs()<1e-8=>{values.push(p.x-base.x);current=p;},
                ('V',Some(PathEl::LineTo(p))) if (p.x-current.x).abs()<1e-8=>{values.push(p.y-base.y);current=p;},
                ('A',Some(first))=>{
                    let shape=self.arcs.get(arc_index).ok_or_else(||invalid("arc curvature entry absent"))?;arc_index+=1;
                    let mut actual=vec![first];for _ in 1..shape.segments{let Some(next)=take(path,&mut element)else{return Ok(None)};actual.push(next);}
                    let end=match actual.last().unwrap(){PathEl::LineTo(p)|PathEl::CurveTo(_,_,p)=>*p,_=>return Ok(None)};
                    let expected=arc_path(current,end,shape)?;if expected.elements().get(1..)!=Some(actual.as_slice()) {
                        let mut observed=BezPath::new();observed.move_to(current);observed.extend(actual.iter().copied());
                        if !same_world_geometry(&observed.to_svg(),&expected.to_svg(),Affine::IDENTITY)?{return Ok(None);}
                    }
                    values.extend([shape.radii[0],shape.radii[1],shape.rotation_degrees,f64::from(shape.large),f64::from(shape.sweep),end.x-base.x,end.y-base.y]);current=end;
                },
                ('C',Some(PathEl::CurveTo(a,b,p)))=>{values.extend([a.x-base.x,a.y-base.y,b.x-base.x,b.y-base.y,p.x-base.x,p.y-base.y]);current=p;cubic_control=Some(b);},
                ('S',Some(PathEl::CurveTo(a,b,p)))=>{let reflected=cubic_control.map_or(current,|last|Point::new(2.*current.x-last.x,2.*current.y-last.y));if !near(a,reflected){return Ok(None);}values.extend([b.x-base.x,b.y-base.y,p.x-base.x,p.y-base.y]);current=p;cubic_control=Some(b);},
                ('Q'|'T',Some(PathEl::CurveTo(a,b,p)))=>{let q1=current+(a-current)*1.5;let q2=p+(b-p)*1.5;if !near(q1,q2){return Ok(None);}let q=Point::new((q1.x+q2.x)/2.,(q1.y+q2.y)/2.);
                    if cmd.eq_ignore_ascii_case(&'T'){let reflected=quadratic_control.map_or(current,|last|Point::new(2.*current.x-last.x,2.*current.y-last.y));if !near(q,reflected){return Ok(None);}}else{values.extend([q.x-base.x,q.y-base.y]);}
                    values.extend([p.x-base.x,p.y-base.y]);current=p;quadratic_control=Some(q);},
                _=>return Ok(None),
            }
            if !matches!(cmd.to_ascii_uppercase(),'C'|'S'){cubic_control=None;}
            if !matches!(cmd.to_ascii_uppercase(),'Q'|'T'){quadratic_control=None;}
        }
        if arc_index!=self.arcs.len()||index!=self.numbers.len()||element!=path.elements().len(){return Ok(None);}
        let mut rendered=values.into_iter().zip(&self.numbers).map(|(value,format)|{
            if format.decimals>14||!value.is_finite(){return Err(invalid("invalid path number format"));}
            let mantissa=if let Some(e)=&format.exponent {
                if !(-308..=308).contains(&e.power)||!(1..=32).contains(&e.digits)||e.power.unsigned_abs().to_string().len()>e.digits as usize||e.plus&&e.power<0||e.negative_zero&&(e.power!=0||e.plus){return Err(invalid("invalid path exponent format"));}
                value/10f64.powi(i32::from(e.power))
            }else{value};
            if !mantissa.is_finite(){return Err(invalid("unrepresentable path mantissa"));}
            let mut number=format!("{:.*}",format.decimals as usize,mantissa);
            if number.parse::<f64>().map_err(|_|invalid("invalid reconstructed path number"))?==0. {
                number=number.trim_start_matches('-').to_owned();if format.negative_zero{number.insert(0,'-');}
            }
            if format.trailing_dot{number.push('.');}
            if format.leading_dot {if number.starts_with("0."){number.remove(0);}else if number.starts_with("-0."){number.remove(1);}}
            if format.plus&&!number.starts_with('-'){number.insert(0,'+');}
            if let Some(e)=&format.exponent {number.push(if e.upper{'E'}else{'e'});if e.power<0||e.negative_zero{number.push('-');}else if e.plus{number.push('+');}number.push_str(&format!("{:0width$}",e.power.unsigned_abs(),width=e.digits as usize));}Ok(number)
        }).collect::<Result<Vec<_>>>()?.into_iter();
        let mut result=String::new();for ch in self.syntax.chars(){if ch=='@'{result.push_str(&rendered.next().ok_or_else(||invalid("missing path notation number"))?);}else{result.push(ch);}}
        Ok(Some(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn smooth_and_scientific_formats_are_coordinate_free_and_native_edits_stay_native() {
        for d in ["M0 0 C1 2 3 2 4 0 S7 -2 8 0", "M0 0 Q2 3 4 0 T8 0", "M+1.25e-01 -0.E+00 L+2.500E+00 .75e+001"] {
            let p=Notation::from_source(d).unwrap().expect(d);let raw=serde_json::to_value(&p).unwrap();
            assert!(!p.syntax.chars().any(|c|c.is_ascii_digit()));assert!(raw.get("d").is_none());assert!(!raw.to_string().contains("1.25"));
            assert_eq!(p.restore(&projected(d).unwrap().to_svg()).unwrap(),Some(d.into()));
            let mut changed=projected(d).unwrap();let element=changed.elements_mut().iter_mut().find(|e|matches!(e,PathEl::LineTo(_)|PathEl::CurveTo(..))).unwrap();
            match element{PathEl::LineTo(p)=>p.x+=0.0001,PathEl::CurveTo(a,_,_)=>a.x+=0.0001,_=>unreachable!()}
            assert_eq!(p.restore(&changed.to_svg()).unwrap(),None);
        }
    }
    #[test]
    fn smooth_metadata_cannot_invent_an_unmeasured_reflected_control() {
        for(d,syntax)in [("M0 0 C1 2 3 2 4 0","M@ @ S@ @ @ @"),("M0 0 Q2 3 4 0","M@ @ T@ @")] {
            let mut p=Notation::from_source(d).unwrap().unwrap();p.syntax=syntax.into();p.numbers.drain(2..4);
            assert_eq!(p.restore(&projected(d).unwrap().to_svg()).unwrap(),None);
        }
    }
    #[test]
    fn arc_curvature_is_bounded_by_actual_controls_and_never_overwrites_native_edits() {
        let d="M0 0 A30 20 30 0 1 100 0";
        let notation=Notation::from_source(d).unwrap().unwrap();
        let raw=serde_json::to_value(&notation).unwrap();
        assert_eq!(raw["arcs"][0]["radii"],serde_json::json!([30.,20.]));
        assert!(raw["arcs"][0].get("points").is_none());
        for mode in 0..3 {
            let mut changed=raw.clone();
            match mode {0=>changed["arcs"][0]["radii"][0]=serde_json::json!(60.),1=>changed["arcs"][0]["segments"]=serde_json::json!(0),_=>changed["arcs"][0]["sweep"]=serde_json::json!(false)}
            let changed:Notation=serde_json::from_value(changed).unwrap();
            assert_ne!(changed.restore(&projected(d).unwrap().to_svg()).ok().flatten(),Some(d.into()));
        }
        let mut edited=projected(d).unwrap();
        let PathEl::CurveTo(a,_,_)=&mut edited.elements_mut()[1] else{panic!("arc cubic absent")};a.x+=0.0001;
        assert_eq!(notation.restore(&edited.to_svg()).unwrap(),None);
    }
    #[test]
    fn old_numbers_stay_readable_and_invalid_exponent_descriptors_fail_explicitly() {
        let d="M0 0 L1 1";let p=Notation::from_source(d).unwrap().unwrap();let raw=serde_json::to_value(p).unwrap();assert!(raw["numbers"][0].get("exponent").is_none());
        let old:Notation=serde_json::from_value(raw).unwrap();assert_eq!(old.restore(&projected(d).unwrap().to_svg()).unwrap(),Some(d.into()));
        let d="M1e0 0 L2e0 1";for(power,digits)in [(309,1),(0,0),(0,33),(12,1)] {
            let mut raw=serde_json::to_value(Notation::from_source(d).unwrap().unwrap()).unwrap();raw["numbers"][0]["exponent"]["power"]=serde_json::json!(power);raw["numbers"][0]["exponent"]["digits"]=serde_json::json!(digits);
            let p:Notation=serde_json::from_value(raw).unwrap();assert!(p.restore(&projected(d).unwrap().to_svg()).is_err());
        }
    }
    #[test]
    fn coordinate_free_notation_recovers_relative_quadratic_and_number_spelling() {
        for d in ["M0 20 C12 -15 42 35 70 0","M0 0H35V35H0Z M10 10H25V25H10Z","m 1.25 2.50 l +3.5 -0 h 2 v -3 q .50 -.75 2.0 1.0 z"] {
            let p=Notation::from_source(d).unwrap().expect(d);assert!(!p.syntax.chars().any(|c|c.is_ascii_digit()));assert_eq!(p.restore(&projected(d).unwrap().to_svg()).unwrap(),Some(d.into()));
            assert_eq!(p.restore("M0 0 L1 1").unwrap(),None);
        }
        assert!(Notation::from_source("M0.1234567 0 L10 10").unwrap().is_none());
        assert!(Notation::from_source("M0 0 A10 10 0 0 1 20 0").unwrap().is_some());
    }
}
