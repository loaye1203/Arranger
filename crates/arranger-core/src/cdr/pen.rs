//! Corel's effective binary nib, which can differ from the retained21 style.
//! Rechart real curves; never replace the curve with a stroked outline/cache.
use super::{binary::*, invalid, unsupported, Result};
use crate::{Geometry,Object};
use kurbo::Affine;

#[derive(Clone,Copy,Debug)]
pub(super) struct Nib {pub width:f64,pub scale_with_object:bool,pub basis:Affine}

// Symmetric square root of the actual world pen covariance. Normalizing the
// linear coefficients avoids overflow; using |det(X)| avoids subtracting two
// nearly equal covariance products. Native coordinates here point upwards.
fn world_root(width:f64,aspect:f64,angle:f64,m:[f64;6])->Result<[f64;3]>{
    let[a,c,_,b,d,_]=m;
    if width<=0.||!(1.0..=100.).contains(&aspect)||angle.rem_euclid(90.)!=0.||!m.into_iter().all(f64::is_finite){return Err(unsupported("world percentage pen requires a finite nonsingular frame and quarter-turn local axes"));}
    let(qx,qy)=if angle.rem_euclid(180.)==0.{(aspect/100.,1.)}else{(1.,aspect/100.)};
    let(a,b,c,d)=(a*qx,b*qx,c*qy,d*qy);let scale=a.abs().max(b.abs()).max(c.abs()).max(d.abs());
    if scale<=0.||!scale.is_finite(){return Err(unsupported("world pen has no finite positive scale"));}
    let(a,b,c,d)=(a/scale,b/scale,c/scale,d/scale);let det=(a*d-b*c).abs();let xx=a*a+c*c;let xy=a*b+c*d;let yy=b*b+d*d;
    let denom=(xx+yy+2.*det).sqrt();let factor=width*scale/2./denom;
    let root=[(xx+det)*factor,xy*factor,(yy+det)*factor];
    if det==0.||!root.into_iter().all(f64::is_finite){return Err(unsupported("world pen is singular or exceeds finite precision"));}Ok(root)
}
fn preset_root(width:f64,aspect:f64,angle:f64)->[f64;3]{
    let(s,c)=(angle-90.).to_radians().sin_cos();let(u,v)=(width/2.,width*aspect/200.);[c*c*u+s*s*v,c*s*(u-v),s*s*u+c*c*v]
}
fn root_error(a:[f64;3],b:[f64;3])->f64{((a[0]-b[0]).powi(2)+2.*(a[1]-b[1]).powi(2)+(a[2]-b[2]).powi(2)).sqrt()}
/// Report122's actual reflected21 local-circle references quantize only the
/// UI preset: width to the coordinate grid, aspect down to an integer and
/// angle to millionths. The effective pen remains the full binary circle and
/// body matrix; this does not round or replace that physical ellipse.
pub(super) fn quantized_reflected_circle(width:f64,nominal:f64,aspect:f64,angle:f64,m:[f64;6])->Result<bool>{
    if m[0]*m[4]-m[1]*m[3]>=0.||!(1.0..=100.).contains(&aspect){return Ok(false);}
    let r=world_root(width,100.,0.,m)?;let delta=(r[0]-r[2]).hypot(2.*r[1]);let major=(r[0]+r[2]+delta)/2.;let minor=(r[0]*r[2]-r[1]*r[1])/major;
    let expected=90.+0.5*(2.*r[1]).atan2(r[0]-r[2]).to_degrees();let expected=(expected*1_000_000.).round_ties_even()/1_000_000.;let difference=(angle-expected+90.).rem_euclid(180.)-90.;
    Ok(nominal==coord(2.*major)? as f64/10000.&&aspect==(100.*minor/major).floor()&&difference.abs()<=64.*f64::EPSILON*180.)
}

/// Measured Corel SaveAs profiles: identity frames and positive-axis resize
/// frames with quantized UI scalars. Keep the effective binary pen; validate
/// its UI projection before normalizing the preset for regular style parsing.
pub(super) fn typed_style(value:&serde_json::Value,outline:&[u8],matrix:[f64;6])->Result<Option<serde_json::Value>> {
    let Some(pen)=value.get("outline")else{return Ok(None)};
    let number=|key:&str,default:f64|->Result<f64>{let Some(v)=pen.get(key)else{return Ok(default)};let n=v.as_str().and_then(|s|s.parse::<f64>().ok()).filter(|n|n.is_finite()).ok_or_else(||invalid("invalid native typed nib scalar"))?;Ok(n)};
    let angle=number("angle",0.)?;let aspect=number("aspect",100.)?;
    if angle==0.&&aspect==100.{let at=super::read::outline_body(outline)?;if u16_at(outline,at)?!=2||u16_at(outline,at+10)?==100{return Ok(None);}}
    let at=super::read::outline_body(outline)?;let kind=u16_at(outline,at)?;let raw_angle=i32_at(outline,at+12)? as f64/1_000_000.;let raw_aspect=f64::from(u16_at(outline,at+10)?);let width=i32_at(outline,at+6)? as f64/10000.;let typed=number("width",width*10000.)?/10000.;
    if kind==2&&pen.get("scaleWithObject").and_then(serde_json::Value::as_str)==Some("0")&&width>0.&&typed>0.{
        if aspect!=raw_aspect||angle.rem_euclid(180.)!=raw_angle.rem_euclid(180.)||(width-typed).abs()*super::read::miter_limit(outline)?.max(1.)>0.00021{return Err(unsupported("fixed native typed nib differs from the actual page ellipse"));}
        let mut result=value.clone();result["outline"]["width"]=serde_json::json!(i32_at(outline,at+6)?.to_string());result["outline"]["angle"]=serde_json::json!("0");result["outline"]["aspect"]=serde_json::json!("100");return Ok(Some(result));
    }
    if kind!=34||pen.get("scaleWithObject").and_then(serde_json::Value::as_str)!=Some("1")||width<=0.||typed<=0. {
        return Err(unsupported("native elliptical typed nib and effective object frame require preservation support"));
    }
    if [matrix[0],matrix[1],matrix[3],matrix[4]]!=[1.,0.,0.,1.] {
        // Four actual report100 SaveAs21 controls retain the exact local
        // binary pen while their UI preset rounds width and truncates aspect.
        // Validate that measured projection exactly; it is not a substitute
        // for the effective binary ellipse or a larger physical error budget.
        let [a,c,_,b,d,_]=matrix;
        let scale=a.hypot(b);let uniform=scale>0.&&scale==c.hypot(d)&&a*c+b*d==0.;
        if uniform&&raw_angle.rem_euclid(90.)==0. {
            let root=|w:f64,q:f64,t:f64|{let(s,c)=(t-90.).to_radians().sin_cos();let(u,v)=(w/2.,w*q/200.);[c*c*u+s*s*v,c*s*(u-v),s*s*u+c*c*v]};
            let (vx,vy)=if raw_angle.rem_euclid(180.)==0.{(c,d)}else{(a,b)};
            let actual=root(width*scale,raw_aspect,90.+vy.atan2(vx).to_degrees());let declared=root(typed,aspect,angle);
            let error=((actual[0]-declared[0]).powi(2)+2.*(actual[1]-declared[1]).powi(2)+(actual[2]-declared[2]).powi(2)).sqrt()*super::read::miter_limit(outline)?.max(1.);
            if !(1.0..=100.).contains(&aspect)||!error.is_finite()||error>0.00021{return Err(unsupported("native rotated nib preset differs from its actual world ellipse"));}
        }else if matrix[1]==0.&&matrix[3]==0.&&a>0.&&d>0.&&raw_aspect==50.&&raw_angle.rem_euclid(90.)==0.&&root_error(world_root(width,raw_aspect,raw_angle,matrix)?,preset_root(typed,aspect,angle))*super::read::miter_limit(outline)?.max(1.)>0.00021{
        let (px,py)=if raw_angle.rem_euclid(180.)==0.{(width*a/2.,width*d)}else{(width*a,width*d/2.)};
        let (major,minor,expected_angle)=if px>py{(px,py,90.)}else{(py,px,0.)};
        if typed!=coord(major)? as f64/10000.||aspect!=(minor/major*100.).floor()||angle.rem_euclid(180.)!=expected_angle {
            return Err(unsupported("native typed nib does not match its measured quantized world projection"));
        }
        }else{
            let actual=world_root(width,raw_aspect,raw_angle,matrix)?;
            let error=root_error(actual,preset_root(typed,aspect,angle))*super::read::miter_limit(outline)?.max(1.);
            if !(1.0..=100.).contains(&aspect)||!error.is_finite()||error>0.00021{return Err(unsupported("native affine nib preset differs from its actual world ellipse"));}
        }
    }else{
        if angle.rem_euclid(180.)!=raw_angle.rem_euclid(180.)||aspect!=raw_aspect {return Err(unsupported("native identity-frame typed nib differs from its binary ellipse"));}
    // The angle and aspect are exact; width quantization therefore bounds
    // every direction of the ellipse. Do not use a dimensionless epsilon.
    if (width-typed).abs()*super::read::miter_limit(outline)?.max(1.)>0.00021{return Err(unsupported("native typed and binary nib widths differ beyond the physical bound"));}
    }
    let mut result=value.clone();let pen=result.get_mut("outline").and_then(serde_json::Value::as_object_mut).ok_or_else(||invalid("native nib style is not an object"))?;
    pen.insert("width".into(),serde_json::json!(i32_at(outline,at+6)?.to_string()));pen.insert("angle".into(),serde_json::json!("0"));pen.insert("aspect".into(),serde_json::json!("100"));Ok(Some(result))
}

pub(super) fn read(data:&[u8])->Result<Option<Nib>> {
    let at=super::read::outline_body(data)?;let kind=u16_at(data,at)?;
    if kind&1!=0{return Ok(None);}
    let width=i32_at(data,at+6)? as f64/10000.;
    let aspect=u16_at(data,at+10)?;
    if !(1..=100).contains(&aspect){return Err(invalid("native outline nib stretch is outside1..100"));}
    // The six doubles start immediately after the millionth-degree angle.
    // These controlled files carry an identity additional nib matrix. Its
    // other meanings are not inferred from a stale typed style/default.
    for (index,wanted) in [1.,0.,0.,0.,1.,0.].into_iter().enumerate(){
        if f64_at(data,at+16+8*index)?!=wanted{return Err(unsupported("native additional outline nib matrix requires preservation support"));}
    }
    if aspect==100||width==0.{return Ok(None);}
    if width<0.{return Err(invalid("negative native nib width"));}
    // Corel's90-degree nib is horizontal. Arranger's page y points down.
    let angle=i32_at(data,at+12)?;
    // Exact integer quarter turns must not create tiny off-axis coordinates
    // which disappear on the next native grid exchange. Nearby angles remain
    // distinct; this is not an epsilon/snap of a real native deformation.
    let (s,c)=match angle.rem_euclid(360_000_000){0=>(1.,0.),90_000_000=>(0.,1.),180_000_000=>(-1.,0.),270_000_000=>(0.,-1.),_=>(90.-angle as f64/1_000_000.).to_radians().sin_cos()};let ratio=f64::from(aspect)/100.;
    Ok(Some(Nib{width,scale_with_object:kind&32!=0,basis:Affine::new([c,s,-s*ratio,c*ratio,0.,0.])}))
}

impl Nib {
    pub fn apply(self,object:&mut Object)->Result<()> {
        if matches!(object.geometry,Geometry::Rect{radius_mm:0.}|Geometry::Ellipse) {
            let [a,b,c,d,x,y]=object.transform;
            let [u,v,w,z,_,_]=self.basis.as_coeffs();
            let sx=u.hypot(w);let sy=v.hypot(z);let minor=sx.min(sy);
            // Horizontal/vertical percentage nibs have a positive axis
            // chart that keeps either live primitive and the same ellipse covariance.
            // Rounded, oblique and fixed-world profiles remain
            // separate; none is replaced with a path or a stroke outline.
            if !self.scale_with_object||super::primitive_frames::column_scales(object.transform).is_none()||!((v==0.&&w==0.)||(u==0.&&z==0.))||minor<=0. {
                return Err(unsupported("native primitive nib requires an invertible percentage chart"));
            }
            let (nx,ny)=(sx/minor,sy/minor);object.width_mm/=nx;object.height_mm/=ny;
            object.transform=[a*nx,b*nx,c*ny,d*ny,x+a*object.x_mm+c*object.y_mm,y+b*object.x_mm+d*object.y_mm];object.x_mm=0.;object.y_mm=0.;object.style.stroke_width_mm=self.width*minor;return Ok(());
        }
        let Geometry::Path{d,fill_rule}=&object.geometry else{return Err(unsupported("noncircular native nib currently requires a real curve; primitive/text nib preservation is not implemented"));};
        let path=crate::path::parse(d)?;let matrix=Affine::new(object.transform)*Affine::translate((object.x_mm,object.y_mm));
        let linear=if self.scale_with_object{Affine::IDENTITY}else{let [a,b,c,d,_,_]=matrix.as_coeffs();Affine::new([a,b,c,d,0.,0.])};
        let chart=self.basis.inverse()*linear;
        let frame=if self.scale_with_object{matrix*self.basis}else{let [_,_,_,_,x,y]=matrix.as_coeffs();Affine::translate((x,y))*self.basis};
        object.geometry=Geometry::Path{d:(chart*path).to_svg(),fill_rule:*fill_rule};
        object.style.stroke_scale_with_object=true;
        object.transform=frame.as_coeffs();object.x_mm=0.;object.y_mm=0.;object.style.stroke_width_mm=self.width;
        Ok(())
    }
}

// Corel can retain winding=1 in a normalized rectangle's typed preset.
// Accept the discrepancy only after proving that the actual curve is one
// simple closed contour. Each Bezier is monotone along its chord; disjoint
// control hulls separate nonadjacent segments; a line through each join
// separates the neighboring hulls. No flattening tolerance or point sampling.
pub(super) fn equivalent_fill(path:&kurbo::BezPath)->bool {
    use kurbo::{PathEl,Point};
    let(mut start,mut current)=(None,None);let mut segments:Vec<Vec<Point>>=vec![];let mut closed=false;
    for element in path.elements(){match *element {
        PathEl::MoveTo(p) if start.is_none()=>{start=Some(p);current=Some(p);},
        PathEl::LineTo(p) if !closed&&current.is_some()=>{segments.push(vec![current.unwrap(),p]);current=Some(p);},
        PathEl::CurveTo(a,b,p) if !closed&&current.is_some()=>{segments.push(vec![current.unwrap(),a,b,p]);current=Some(p);},
        PathEl::ClosePath if !closed&&current.is_some()=>{if current!=start{segments.push(vec![current.unwrap(),start.unwrap()]);}closed=true;},
        _=>return false,
    }if segments.len()>128{return false;}}
    if !closed||segments.len()<3{return false;}
    // Native control points are signed integers in1/10000mm. Keep every
    // separating-line sign exact even for distant, nearly touching hulls.
    let Some(segments)=segments.into_iter().map(|points|points.into_iter().map(|p|{
        let x=coord(p.x).ok()?;let y=coord(p.y).ok()?;
        if x as f64/10000.!=p.x||y as f64/10000.!=p.y{return None;}
        Some([i128::from(x),i128::from(y)])
    }).collect::<Option<Vec<_>>>()).collect::<Option<Vec<_>>>()else{return false;};
    let sub=|a:[i128;2],b:[i128;2]|[a[0]-b[0],a[1]-b[1]];
    let dot=|a:[i128;2],b:[i128;2]|a[0]*b[0]+a[1]*b[1];
    for points in &segments {let chord=sub(*points.last().unwrap(),points[0]);if chord==[0,0]||points.windows(2).any(|p|dot(sub(p[1],p[0]),chord)<0){return false;}}
    for i in 0..segments.len(){let a=&segments[i];let b=&segments[(i+1)%segments.len()];let join=*a.last().unwrap();if b[0]!=join{return false;}
        let before=sub(join,a[0]);let after=sub(*b.last().unwrap(),join);
        // Integer square roots only propose a direction; every separating
        // inequality below is exact, so truncation can only cause refusal.
        let len_before=(dot(before,before) as u128).isqrt() as i128;let len_after=(dot(after,after) as u128).isqrt() as i128;
        let axis=[before[0]*len_after+after[0]*len_before,before[1]*len_after+after[1]*len_before];
        if a[..a.len()-1].iter().any(|p|dot(sub(*p,join),axis)>=0)||b[1..].iter().any(|p|dot(sub(*p,join),axis)<=0){return false;}
        for j in i+2..segments.len(){if i==0&&j==segments.len()-1{continue;}let b=&segments[j];let all=a.iter().chain(b).copied().collect::<Vec<_>>();
            let disjoint=all.iter().enumerate().any(|(u,p)|all[u+1..].iter().any(|q|{
                let edge=sub(*q,*p);let axis=[-edge[1],edge[0]];if axis==[0,0]{return false;}
                let extent=|points:&[[i128;2]]|points.iter().fold((i128::MAX,i128::MIN),|(lo,hi),p|{let v=dot(*p,axis);(lo.min(v),hi.max(v))});let(amin,amax)=extent(a);let(bmin,bmax)=extent(b);amax<bmin||bmax<amin
            }));if !disjoint{return false;}
        }
    }true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn page_fixed_primitive_nib_edits_remain_native_and_conflicting_typed_modes_reject(){
        for version in [14,21]{for mode in [0,1,2,3]{
            let mut source=crate::Document::new("fixed-native-edit".into(),210.,297.).unwrap();let mut object=source_object_rect();object.transform=[1.17,0.23,0.31,0.91,40.,50.];object.style.stroke_scale_with_object=false;object.style.stroke_fixed_nib=Some(crate::FixedStrokeNib{stretch_percent:70,angle_millionth_degrees:30_000_000});source.objects.push(object);
            let encode=if version==14{crate::cdr::encode_document}else{crate::cdr::encode_document21};let mut native=read_native(&encode(&source).unwrap().0).unwrap();
            fn change(nodes:&mut[Node],mode:u8){for n in nodes{
                if n.tag==*b"outl"{let at=super::super::read::outline_body(&n.data).unwrap();if u16_at(&n.data,at).unwrap()==2{if mode==0{n.data[at+10..at+12].copy_from_slice(&60u16.to_le_bytes());}else if mode==1{n.data[at+12..at+16].copy_from_slice(&30_000_001i32.to_le_bytes());}}}
                if n.tag==*b"loda"{let(kind,mut args)=super::super::write::arguments(&n.data).unwrap();if let Some((_,v))=args.iter_mut().find(|(key,_)|*key==201){let mut value=super::super::modern::style_record(v).unwrap();value["outline"][if mode==0{"aspect"}else if mode==2{"scaleWithObject"}else{"angle"}]=serde_json::json!(if mode==0{"60"}else if mode==2{"1"}else if mode==3{"0"}else{"30.000001"});if mode==3{value["outline"]["aspect"]=serde_json::json!("100");}*v=super::super::modern::encoded_style(&value).unwrap();n.data=super::super::write::loda(kind,&args);}}
                if let Some(c)=&mut n.children{change(c,mode);}
            }}change(&mut native.nodes,mode);
            let raw=if version==21{repack21(&native).unwrap()}else{native.members.insert("content/riffData.cdr".into(),write(&native.nodes).unwrap());zip(&native.members.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>())};let decoded=crate::cdr::decode(&raw,&source.id);
            if version==21&&mode>=2{assert!(decoded.is_err(),"conflicting native typed nib mode{mode}");continue;}
            let doc=decoded.unwrap().0;let o=doc.object("rect").unwrap();assert!(matches!(o.geometry,Geometry::Rect{radius_mm:0.}));assert!(!o.style.stroke_scale_with_object);let nib=o.style.stroke_fixed_nib.unwrap();assert_eq!(nib.stretch_percent,if mode==0{60}else{70});assert_eq!(nib.angle_millionth_degrees,if mode==1{30_000_001}else{30_000_000});
            assert_eq!(crate::cdr::decode(&encode(&doc).unwrap().0,&doc.id).unwrap().0,doc);
        }}
    }

    #[test]
    fn resized_typed_presets_are_checked_against_the_actual_binary_world_projection(){
        let raw=std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/assets/cdr/corel2019/save100-controls/product-x-resized21.cdr")).unwrap();let native=read_native(&raw).unwrap();let node=all(&native.nodes).into_iter().find(|n|n.kind()==Some(b"obj ")&&super::super::write::arguments(&n.find(b"loda").unwrap().data).is_ok_and(|(_,a)|a.iter().any(|(k,v)|*k==1000&&read_wide(v).is_ok_and(|v|v=="content")))).unwrap();
        let (_,args)=super::super::write::arguments(&node.find(b"loda").unwrap().data).unwrap();let key=u32_at(&args.iter().find(|(k,_)|*k==10).unwrap().1,0).unwrap();let line=all(&native.nodes).into_iter().find(|n|n.tag==*b"outl"&&u32_at(&n.data,0).ok()==Some(key)).unwrap();let value=super::super::modern::style_record(&args.iter().find(|(k,_)|*k==201).unwrap().1).unwrap();let frame=[1.0499999523162842,0.,0.,0.,1.100000023841858,0.];assert!(typed_style(&value,&line.data,frame).unwrap().is_some());
        for (k,v) in [("width","20999"),("aspect","51"),("angle","0"),("scaleWithObject","0")]{let mut bad=value.clone();bad["outline"][k]=serde_json::json!(v);assert!(typed_style(&bad,&line.data,frame).is_err(),"{k}");}
        let mut shear=frame;shear[1]=0.000001;assert!(typed_style(&value,&line.data,shear).is_err());
    }
    #[test]
    fn rotated_world_preset_is_checked_against_the_effective_native_ellipse(){
        let mut doc=crate::Document::new("rotated".into(),210.,297.).unwrap();let mut o=source_object_rect();o.transform=[0.8,0.6,-0.3,0.4,60.,60.];doc.objects.push(o);let native=read_native(&crate::cdr::encode_document21(&doc).unwrap().0).unwrap();let node=all(&native.nodes).into_iter().find(|n|n.kind()==Some(b"obj ")).unwrap();let(_,args)=super::super::write::arguments(&node.find(b"loda").unwrap().data).unwrap();let value=super::super::modern::style_record(&args.iter().find(|(k,_)|*k==201).unwrap().1).unwrap();let key=u32_at(&args.iter().find(|(k,_)|*k==10).unwrap().1,0).unwrap();let outl=all(&native.nodes).into_iter().find(|n|n.tag==*b"outl"&&u32_at(&n.data,0).ok()==Some(key)).unwrap();let m=[0.8,0.6,0.,-0.6,0.8,0.];assert!(typed_style(&value,&outl.data,m).unwrap().is_some());
        for key in ["width","angle","aspect"]{let mut bad=value.clone();let v=bad["outline"][key].as_str().unwrap().parse::<f64>().unwrap();bad["outline"][key]=serde_json::json!((v+if key=="width"{10000.}else{1.}).to_string());assert!(typed_style(&bad,&outl.data,m).is_err(),"{key}");}
        let mut shear=m;shear[1]+=0.01;assert!(typed_style(&value,&outl.data,shear).is_err());
    }
    #[test]
    fn float32_rotated_frame_rounding_preserves_source_but_not_a_native_width_edit(){
        for version in [14,21]{for geometry in [Geometry::Rect{radius_mm:0.},Geometry::Ellipse]{for shear in [false,true]{for edit_width in [false,true]{
            let mut doc=crate::Document::new("f32-frame".into(),210.,297.).unwrap();let mut o=source_object_rect();o.geometry=geometry.clone();o.style.stroke_width_mm=0.25;o.transform=[1.6,1.2,if shear{0.6}else{-0.6},0.8,60.,60.];doc.objects.push(o);let encoded=(if version==14{crate::cdr::encode_document}else{crate::cdr::encode_document21})(&doc);let bytes=encoded.unwrap().0;let mut native=read_native(&bytes).unwrap();
            fn mutate(nodes:&mut[Node],edit:bool){for n in nodes{if edit&&n.tag==*b"loda"{let(kind,mut args)=super::super::write::arguments(&n.data).unwrap();if matches!(kind,1|2){if let Some((_,data))=args.iter_mut().find(|(key,_)|*key==201){let mut value=super::super::modern::style_record(data).unwrap();let width=value["outline"]["width"].as_str().unwrap().parse::<i32>().unwrap();value["outline"]["width"]=serde_json::json!((width+1).to_string());*data=super::super::modern::encoded_style(&value).unwrap();n.data=super::super::write::loda(kind,&args);}}}if n.tag==*b"trfd"{let at=u32_at(&n.data,u32_at(&n.data,8).unwrap()as usize).unwrap()as usize+16;for i in [0,1,3,4]{let pos=at+i*8;let v=f64_at(&n.data,pos).unwrap()as f32 as f64;n.data[pos..pos+8].copy_from_slice(&v.to_le_bytes());}}if edit&&n.tag==*b"outl"{let at=super::super::read::outline_body(&n.data).unwrap();if u16_at(&n.data,at).unwrap()&1==0{let w=i32_at(&n.data,at+6).unwrap();n.data[at+6..at+10].copy_from_slice(&(w+1).to_le_bytes());}}if let Some(c)=&mut n.children{mutate(c,edit);}}}
            mutate(&mut native.nodes,edit_width);let raw=if version==21{repack21(&native).unwrap()}else{native.members.insert("content/riffData.cdr".into(),write(&native.nodes).unwrap());zip(&native.members.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>())};let actual=crate::cdr::decode(&raw,&doc.id).unwrap().0;
            if edit_width{let o=actual.object("rect").unwrap();let m=actual.world_transform(o).unwrap().as_coeffs();assert!((m[0].hypot(m[1])*o.style.stroke_width_mm-if version==21&&shear{0.5002}else{0.5001}).abs()<1e-6,"actual stored width edit wins");assert_ne!(actual,doc);}else{assert_eq!(actual,doc);}
        }}}}
    }
    #[test]
    fn model_axis_rectangles_emit_native_shape1_identity_frames_and_the_measured_pen(){
        for vertical in [false,true]{let mut source=crate::Document::new("independent-native".into(),210.,297.).unwrap();let mut object=source_object_rect();object.transform=if vertical{[1.,0.,0.,2.,0.,0.]}else{[2.,0.,0.,1.,0.,0.]};source.objects.push(object);
            for encode in [crate::cdr::encode_document,crate::cdr::encode_document21]{let raw=encode(&source).unwrap().0;let native=read_native(&raw).unwrap();let n=all(&native.nodes).into_iter().find(|n|n.kind()==Some(b"obj ")).unwrap();let(kind,args)=super::super::write::arguments(&n.find(b"loda").unwrap().data).unwrap();assert_eq!(kind,1);
                let tr=&n.find(b"trfd").unwrap().data;let at=u32_at(tr,u32_at(tr,8).unwrap() as usize).unwrap() as usize+16;let matrix:[f64;6]=std::array::from_fn(|i|f64_at(tr,at+i*8).unwrap());assert_eq!([matrix[0],matrix[1],matrix[3],matrix[4]],[1.,0.,0.,1.]);let key=u32_at(&args.iter().find(|(k,_)|*k==10).unwrap().1,0).unwrap();let line=all(&native.nodes).into_iter().find(|n|n.tag==*b"outl"&&u32_at(&n.data,0).ok()==Some(key)).unwrap();let at=super::super::read::outline_body(&line.data).unwrap();assert_eq!(u16_at(&line.data,at).unwrap(),34);assert_eq!(u16_at(&line.data,at+10).unwrap(),50);assert_eq!(i32_at(&line.data,at+12).unwrap(),if vertical{0}else{-90_000_000});assert_eq!(i32_at(&line.data,at+6).unwrap(),20000);
                if native.version==2100{let style=super::super::modern::style_record(&args.iter().find(|(k,_)|*k==201).unwrap().1).unwrap();assert_eq!(style["outline"]["width"],"20000");assert_eq!(style["outline"]["aspect"],"50");}
            }
        }
    }
    fn source_object_rect()->Object{serde_json::from_value(serde_json::json!({"id":"rect","page_id":"page-1","x_mm":20.,"y_mm":30.,"width_mm":20.,"height_mm":10.,"geometry":{"kind":"rect","radius_mm":0.},"style":{"fill":"none","stroke":"#000000","stroke_width_mm":1.,"opacity":1.}})).unwrap()}
    #[test]
    fn independent_rect_emission_keeps_unmeasured_or_fine_profiles_in_the_existing_route(){
        for mode in 0..6{let mut object=source_object_rect();object.transform=[2.,0.,0.,1.,0.,0.];match mode{0=>object.geometry=Geometry::Rect{radius_mm:1.},1=>object.transform[1]=0.125,2=>object.transform[0]=3.,3=>object.width_mm=20.1234551,4=>object.style.stroke_width_mm=0.1234567,_=>object.style.opacity=0.5};let before=object.clone();assert!(super::super::primitive_frames::independent_axis_nib(&mut object).unwrap().is_none());assert_eq!(object,before);}
    }
    #[test]
    fn exact_native_quarter_turns_keep_axis_points_exact_but_nearby_angles_are_not_snapped(){
        let native=read_native(include_bytes!("../../../../examples/assets/cdr/corel2019/save88-nibs/primitive21-ordinary-grid-rect-corel-resaved.cdr")).unwrap();let node=all(&native.nodes).into_iter().find(|n|n.tag==*b"outl"&&read(&n.data).is_ok_and(|n|n.is_some())).unwrap();let mut raw=node.data.clone();let at=super::super::read::outline_body(&raw).unwrap();raw[at+12..at+16].copy_from_slice(&(-90_000_000i32).to_le_bytes());let n=read(&raw).unwrap().unwrap();let m=n.basis.as_coeffs();assert_eq!(m[1],0.);assert_eq!(m[2],0.);assert_eq!(m[0],-1.);
        raw[at+12..at+16].copy_from_slice(&(-90_000_001i32).to_le_bytes());let m=read(&raw).unwrap().unwrap().basis.as_coeffs();assert_ne!(m[1],0.);assert_ne!(m[2],0.);
    }
    #[test]
    fn measured_world_nib_presets_cannot_hide_changed_angle_aspect_or_large_width(){
        let native=read_native(include_bytes!("../../../../examples/assets/cdr/corel2019/save88-nibs/primitive21-ordinary-grid-rect-corel-resaved.cdr")).unwrap();let node=all(&native.nodes).into_iter().find(|n|n.tag==*b"outl"&&read(&n.data).is_ok_and(|n|n.is_some())).unwrap();let raw=&node.data;let at=super::super::read::outline_body(raw).unwrap();let width=i32_at(raw,at+6).unwrap();let angle=i32_at(raw,at+12).unwrap() as f64/1_000_000.;let aspect=u16_at(raw,at+10).unwrap();let value=serde_json::json!({"outline":{"width":width.to_string(),"angle":angle.to_string(),"aspect":aspect.to_string(),"scaleWithObject":"1"}});let m=[1.,0.,0.,0.,1.,0.];
        let v=typed_style(&value,raw,m).unwrap().unwrap();assert_eq!(v["outline"]["width"],width.to_string());assert_eq!(v["outline"]["angle"],"0");assert_eq!(v["outline"]["aspect"],"100");
        for(key,number)in[("width",f64::from(width)+10000.),("angle",angle+1.),("aspect",f64::from(aspect)+1.)]{let mut bad=value.clone();bad["outline"][key]=serde_json::json!(number.to_string());assert!(typed_style(&bad,raw,m).is_err(),"{key}");}
        let mut bad=value;bad["outline"]["dashDotSpec"]=serde_json::json!("1");let v=typed_style(&bad,raw,m).unwrap().unwrap();assert!(super::super::modern::geometry_style(&v,crate::Style::default()).is_err());
    }
    use serde_json::json;
    fn source()->Object {serde_json::from_value(json!({"id":"curve","page_id":"page-1","x_mm":2.,"y_mm":3.,"width_mm":20.,"height_mm":10.,"transform":[2.,0.25,0.125,1.5,5.,6.],"geometry":{"kind":"path","d":"M0 0C3 8 10 -4 20 10","fill_rule":"evenodd"},"style":{"fill":"none","stroke":"#000000","stroke_width_mm":7.,"opacity":1.}})).unwrap()}
    #[test]
    fn real_curve_and_both_scaling_modes_preserve_centerline_and_elliptic_pen(){
        for scaled in [false,true]{let mut object=source();let before=object.clone();let m=Affine::new(before.transform)*Affine::translate((before.x_mm,before.y_mm));let nib=Nib{width:0.5159,scale_with_object:scaled,basis:Affine::rotate(0.3)*Affine::scale_non_uniform(1.,0.7)};
            let path=before.local_path().unwrap();nib.apply(&mut object).unwrap();assert!(matches!(object.geometry,Geometry::Path{fill_rule:crate::FillRule::EvenOdd,..}));assert_eq!(object.style.stroke_width_mm,0.5159);
            assert!(super::super::path_precision::same_world(&path,m,&object.local_path().unwrap(),Affine::new(object.transform)));
            for theta in [0f64,0.2,1.2,2.4,5.8]{let vector=kurbo::Vec2::new(theta.cos()*nib.width/2.,theta.sin()*nib.width/2.);let apply=|m:Affine,v:kurbo::Vec2|m*v.to_point()-m*kurbo::Point::ORIGIN;let native=apply(nib.basis,vector);let wanted=if scaled{apply(m,native)}else{native};let actual=apply(Affine::new(object.transform),vector);assert!((wanted-actual).hypot()<1e-12);}
        }
    }
    #[test]
    fn unsupported_nib_targets_fail_without_mutating_them(){let mut object=source();object.geometry=Geometry::Rect{radius_mm:0.5};let before=object.clone();assert!(Nib{width:1.,scale_with_object:true,basis:Affine::IDENTITY}.apply(&mut object).is_err());assert_eq!(object,before);}
    #[test]
    fn sheared14_percentage_and21_local_circle_preserve_live_geometry_and_source(){
        for version in [14,21]{for geometry in [Geometry::Rect{radius_mm:0.},Geometry::Ellipse]{for reflected in [false,true]{
            let mut doc=crate::Document::new("affine-primitive".into(),210.,297.).unwrap();let mut o=source_object_rect();o.geometry=geometry.clone();let sign=if reflected{-1.}else{1.};o.transform=[0.8,0.6,0.3*sign,0.4*sign,60.,60.];o.style.stroke_width_mm=0.5;doc.objects.push(o);
            let encode=if version==14{crate::cdr::encode_document}else{crate::cdr::encode_document21};let raw=encode(&doc).unwrap().0;let native=read_native(&raw).unwrap();let node=all(&native.nodes).into_iter().find(|n|n.kind()==Some(b"obj ")).unwrap();let(kind,_args)=super::super::write::arguments(&node.find(b"loda").unwrap().data).unwrap();assert_eq!(kind,if matches!(geometry,Geometry::Ellipse){2}else{1});
            let t=&node.find(b"trfd").unwrap().data;let at=u32_at(t,u32_at(t,8).unwrap()as usize).unwrap()as usize+16;let m:[f64;6]=std::array::from_fn(|i|f64_at(t,at+i*8).unwrap());assert_ne!(m[0]*m[1]+m[3]*m[4],0.,"the actual native frame retains the shear");
            let result=crate::cdr::decode(&raw,&doc.id).unwrap().0;assert_eq!(result,doc);assert_eq!(crate::cdr::decode(&encode(&result).unwrap().0,&doc.id).unwrap().0,doc);

        }}}
    }
    #[test]
    fn admitted_rotated_chart_keeps_a_real_native_dimension_edit_even_below_one_world_grid(){
        for version in [14,21]{for geometry in [Geometry::Rect{radius_mm:0.},Geometry::Ellipse]{
            let mut doc=crate::Document::new("affine-edits".into(),210.,297.).unwrap();let mut o=source_object_rect();o.geometry=geometry;o.transform=[0.08,0.06,-0.03,0.04,60.,60.];o.style.stroke_width_mm=0.5;doc.objects.push(o);
            let encode=if version==14{crate::cdr::encode_document}else{crate::cdr::encode_document21};let bytes=encode(&doc).unwrap().0;assert_eq!(crate::cdr::decode(&bytes,&doc.id).unwrap().0,doc);let mut native=read_native(&bytes).unwrap();
            fn change(nodes:&mut[Node],modern:bool){for n in nodes{if n.tag==*b"loda"{let(kind,mut args)=super::super::write::arguments(&n.data).unwrap();if matches!(kind,1|2){let g=&mut args.iter_mut().find(|(k,_)|*k==30).unwrap().1;if modern&&kind==1{let w=f64_at(g,0).unwrap();g[..8].copy_from_slice(&(w+1.).to_le_bytes());}else{let w=i32_at(g,0).unwrap();g[..4].copy_from_slice(&(w+1).to_le_bytes());}n.data=super::super::write::loda(kind,&args);}}if let Some(c)=&mut n.children{change(c,modern);}}}change(&mut native.nodes,version==21);
            let raw=if version==21{repack21(&native).unwrap()}else{native.members.insert("content/riffData.cdr".into(),write(&native.nodes).unwrap());zip(&native.members.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>())};let actual=crate::cdr::decode(&raw,&doc.id).unwrap().0;
            assert_ne!(actual,doc,"an actual one-grid local dimension change cannot be replaced by stale source Notes");let o=actual.object("rect").unwrap();assert!((o.width_mm-20.00005).abs()<1e-12,"actual width {}",o.width_mm);
        }}
    }
    #[test]
    fn differing_fill_modes_require_a_proven_single_simple_closed_contour(){
        for d in ["M0 0L10 0L10 8L0 8Z","M0 0L10 0L5 4L10 8L0 8Z","M1 0L9 0C9.5 0 10 0.5 10 1L10 7C10 7.5 9.5 8 9 8L0 8L0 0Z"]{assert!(equivalent_fill(&crate::path::parse(d).unwrap()));}
        for d in ["M0 0L10 0L10 8L0 8Z M2 2L8 2L8 6L2 6Z","M0 0L10 8L0 8L10 0Z","M0 0L10 0L10 8"]{assert!(!equivalent_fill(&crate::path::parse(d).unwrap()));}
    }
    #[test]
    fn actual_corel_resaves_keep_the_effective_nib_even_with_old_typed_width_and_notes(){
        let fixtures:[(&str,&[u8]);4]=[
            ("primitive14-rounded-fine-corel-resaved.cdr",include_bytes!("../../../../examples/assets/cdr/corel2019/save88-nibs/primitive14-rounded-fine-corel-resaved.cdr")),
            ("primitive14-ordinary-grid-rect-corel-resaved.cdr",include_bytes!("../../../../examples/assets/cdr/corel2019/save88-nibs/primitive14-ordinary-grid-rect-corel-resaved.cdr")),
            ("primitive21-rounded-fine-corel-resaved.cdr",include_bytes!("../../../../examples/assets/cdr/corel2019/save88-nibs/primitive21-rounded-fine-corel-resaved.cdr")),
            ("primitive21-ordinary-grid-rect-corel-resaved.cdr",include_bytes!("../../../../examples/assets/cdr/corel2019/save88-nibs/primitive21-ordinary-grid-rect-corel-resaved.cdr")),
        ];
        for version in [14,21]{for scene in ["rounded-fine","ordinary-grid-rect"]{
            let name=format!("primitive{version}-{scene}-corel-resaved.cdr");let bytes=fixtures.iter().find(|(file,_)|*file==name).unwrap().1.to_vec();let mut native=read_native(&bytes).unwrap();
            let node=all(&native.nodes).into_iter().find(|n|n.kind()==Some(b"obj ")&&super::super::write::arguments(&super::super::read::own_record(n,b"loda").unwrap().data).is_ok_and(|(_,args)|args.iter().any(|(k,v)|*k==1000&&read_wide(v).ok().as_deref()==Some("content")))).unwrap();
            let(kind,args)=super::super::write::arguments(&super::super::read::own_record(node,b"loda").unwrap().data).unwrap();assert_eq!(kind,3,"Corel's conversion is not undone by Notes");
            let key=u32_at(&args.iter().find(|(k,_)|*k==10).unwrap().1,0).unwrap();let record=all(&native.nodes).into_iter().find(|n|n.tag==*b"outl"&&u32_at(&n.data,0).ok()==Some(key)).unwrap();let nib=read(&record.data).unwrap().unwrap();
            assert_eq!(nib.width,if scene=="rounded-fine"{0.2548}else{0.5159});assert!(nib.scale_with_object);
            let path=super::super::read::curve(&args.iter().find(|(k,_)|*k==30).unwrap().1).unwrap();let t=&super::super::read::own_record(node,b"trfd").unwrap().data;let at=u32_at(t,u32_at(t,8).unwrap() as usize).unwrap() as usize+16;let[a,c,x,b,d,y]=std::array::from_fn(|i|f64_at(t,at+i*8).unwrap());let expected=Affine::new([a,-b,-c,d,x,y]);
            // Native translation uses page units; compare relative controls
            // and the pen's linear basis, independently of local Notes origin.
            native.members.remove("metadata/metadata.xml");native.members.remove("META-INF/metadata.xml");let bare=if version==21{repack21(&native).unwrap()}else{native.members.insert("content/riffData.cdr".into(),write(&native.nodes).unwrap());zip(&native.members.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>())};
            for input in [&bytes,&bare]{let(doc,warnings)=crate::cdr::decode(input,"nib").unwrap();let object=doc.object("content").unwrap();assert!(matches!(object.geometry,Geometry::Path{..}));assert!(object.style.stroke_width_mm>0.);assert!(warnings.iter().any(|w|w.code=="CDR_NIB_RECHARTED"));
                let m=doc.world_transform(object).unwrap().as_coeffs();let target=(expected*nib.basis).as_coeffs();for i in 0..4{assert!((m[i]*object.style.stroke_width_mm-target[i]*nib.width).abs()<1e-12);}
                let actual=object.local_path().unwrap();let mut linear=m;linear[4]=0.;linear[5]=0.;let mut reference=expected.as_coeffs();reference[4]=0.;reference[5]=0.;assert!(super::super::path_precision::same_world(&path,Affine::new(reference),&actual,Affine::new(linear)));
            }
        }}
    }
    #[test]
    fn changed_effective_percentage_or_width_is_never_replaced_by_source_notes(){
        for geometry in [Geometry::Rect{radius_mm:0.},Geometry::Ellipse]{
        let mut source=crate::Document::new("edited-nib".into(),210.,297.).unwrap();let mut object=source_object_rect();object.geometry=geometry.clone();object.transform=[1.,0.,0.,0.6,0.,0.];source.objects.push(object);
        let raw=crate::cdr::encode_document(&source).unwrap().0;
        for edit in [0,1,2]{let mut native=read_native(&raw).unwrap();
            fn change(nodes:&mut [Node],edit:usize){for n in nodes{if edit==2&&n.tag==*b"loda"{let(kind,mut args)=super::super::write::arguments(&n.data).unwrap();if kind==1||kind==2{let g=&mut args.iter_mut().find(|(k,_)|*k==30).unwrap().1;let width=i32_at(g,0).unwrap();g[..4].copy_from_slice(&(width+1).to_le_bytes());n.data=super::super::write::loda(kind,&args);}}if edit<2&&n.tag==*b"outl"{let at=super::super::read::outline_body(&n.data).unwrap();if u16_at(&n.data,at).unwrap()&1==0{if edit==0{n.data[at+10..at+12].copy_from_slice(&59u16.to_le_bytes());}else{n.data[at+6..at+10].copy_from_slice(&20000i32.to_le_bytes());}}}if let Some(children)=&mut n.children{change(children,edit);}}}
            change(&mut native.nodes,edit);native.members.insert("content/riffData.cdr".into(),write(&native.nodes).unwrap());let bytes=zip(&native.members.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>());
            let actual=crate::cdr::decode(&bytes,"edited-nib").unwrap().0;let o=actual.object("rect").unwrap();assert_eq!(o.geometry,geometry);let m=actual.world_transform(o).unwrap().as_coeffs();let axes=[m[0]*o.style.stroke_width_mm,m[3]*o.style.stroke_width_mm];let wanted=match edit{0=>[1.,0.59],1=>[2.,1.2],_=>[1.,0.6]};for(a,b)in axes.into_iter().zip(wanted){assert!((a-b).abs()<1e-12,"native edited pen must win");}if edit==2{assert!((actual.bounds(o).unwrap()[2]-20.0001).abs()<1e-12,"one-grid native size edit must win");}
        }
        }
    }
    #[test]
    fn derived_nib_chart_keeps_physical_precision_without_a_source_spelling_payload(){
        let bytes=include_bytes!("../../../../examples/assets/cdr/corel2019/save88-nibs/primitive14-rounded-fine-corel-resaved.cdr");let doc=crate::cdr::decode(bytes,"fine-nib").unwrap().0;let object=doc.object("content").unwrap();
        assert!(super::super::path_precision::plan(&doc,object).unwrap().is_some(),"derived float spelling cannot force coarse native geometry or nib width");
    }
}

pub(super) fn rotated_preset(value:&mut serde_json::Value,m:[f64;6],outline:&[u8])->Result<()> {
    let at=super::read::outline_body(outline)?;if u16_at(outline,at)?&32==0{return Ok(());}
    let[a,c,_,b,d,_]=m;if [a,c,b,d]==[1.,0.,0.,1.]{return Ok(());}
    let at=super::read::outline_body(outline)?;let angle=i32_at(outline,at+12)?;if angle.rem_euclid(90_000_000)!=0{return Ok(());}
    if a.hypot(b)==1.&&c.hypot(d)==1.&&a*c+b*d==0.{
        let(x,y)=if angle.rem_euclid(180_000_000)==0{(c,d)}else{(a,b)};
        value["outline"]["angle"]=serde_json::json!((90.+y.atan2(x).to_degrees()).to_string());return Ok(());
    }
    // The first nonorthogonal21 preset from this unmeasured route crashed
    // Corel2019 in Windows120. Do not emit the profile while its correct
    // native construction remains unknown; no assertion about the root cause.
    if a*c+b*d!=0.{return Err(crate::Error::new("CDR_NATIVE_PROFILE_QUARANTINED","Report120: scaled oblique CDR21 nib preset is quarantined after a native access violation; keep the complete .arranger source"));}
    let root=world_root(i32_at(outline,at+6)? as f64/10000.,f64::from(u16_at(outline,at+10)?),angle as f64/1_000_000.,m)?;
    let delta=(root[0]-root[2]).hypot(2.*root[1]);let major=(root[0]+root[2]+delta)/2.;let minor=(root[0]*root[2]-root[1]*root[1])/major;
    let width=coord(2.*major)? as f64/10000.;let aspect=minor/major*100.;let angle=90.+0.5*(2.*root[1]).atan2(root[0]-root[2]).to_degrees();
    let error=root_error(root,preset_root(width,aspect,angle))*super::read::miter_limit(outline)?.max(1.);
    if width<=0.||!(1.0..=100.).contains(&aspect)||!error.is_finite()||error>0.00021{return Err(unsupported("affine world nib preset cannot fit the existing physical budget"));}
    value["outline"]["width"]=serde_json::json!(coord(width)?.to_string());value["outline"]["aspect"]=serde_json::json!(aspect.to_string());value["outline"]["angle"]=serde_json::json!(angle.to_string());Ok(())
}
