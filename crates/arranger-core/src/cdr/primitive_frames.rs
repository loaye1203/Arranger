//! Preserve fixed-world pens on orthogonal native primitive frames without
//! replacing a live rectangle/ellipse with a path or an outlined stroke.
use super::{binary::*,invalid,Result};
use crate::{Geometry,Object};

/// Report100's real14/21 SaveAs caches use a four-grid-unit minimum box
/// when both analytic ellipse extents round to the same final grid point.
/// The active dimensions and affine frame remain unchanged. Other cache
/// profiles are untouched; this is not a geometry/pen tolerance increase.
pub(super) fn minimum_ellipse_cache(node:&mut Node)->Result<()> {
    let raw=&node.find(b"bbox")?.data;let x=i32_at(raw,0)?;let y=i32_at(raw,4)?;
    if x!=i32_at(raw,8)?||y!=i32_at(raw,12)?{return Ok(());}
    let checked=|n:i32,delta:i32|n.checked_add(delta).ok_or_else(||super::unsupported("minimum ellipse cache exceeds the signed grid"));
    let mut cache=vec![];for value in [checked(x,-2)?,checked(y,2)?,checked(x,2)?,checked(y,-2)?]{cache.extend(value.to_le_bytes());}
    let old=&node.find(b"obbx")?.data;let outer=[i32_at(old,16)?.min(i32_at(&cache,0)?),i32_at(old,20)?.max(i32_at(&cache,4)?),i32_at(old,24)?.max(i32_at(&cache,8)?),i32_at(old,28)?.min(i32_at(&cache,12)?)];
    node.find_mut(b"bbox")?.data=cache.clone();let mut obbx=cache;for v in outer{obbx.extend(v.to_le_bytes());}node.find_mut(b"obbx")?.data=obbx;Ok(())
}

/// The actual Corel2019 controls preserve a live unrounded rectangle with an
/// identity geometry frame and an independent percentage pen. Use the same
/// canonical representation for full ellipses and signed axis permutations
/// only when both axes reconstruct exactly from an integer
/// percentage and the resulting dimensions/width stay on the native grid.
/// Exact signed axis permutations only. No near-zero/shear epsilon.
pub(super) fn axis_scales(m:[f64;6])->Option<[f64;2]>{
    let[a,b,c,d,_,_]=m;
    if !m.into_iter().all(f64::is_finite)||!((a!=0.&&d!=0.&&b==0.&&c==0.)||(a==0.&&d==0.&&b!=0.&&c!=0.)){return None;}
    Some([a.abs()+c.abs(),b.abs()+d.abs()])
}
/// Column lengths change local units without removing the angle between the
/// axes. A shear remains in the live native frame, never snapped to a rotation.
pub(super) fn column_scales(m:[f64;6])->Option<[f64;2]>{
    let[a,b,c,d,_,_]=m;let det=a*d-b*c;if !m.into_iter().all(f64::is_finite)||!det.is_finite()||det==0.{return None;}
    let sx=a.hypot(b);let sy=c.hypot(d);(sx>0.&&sy>0.&&sx.is_finite()&&sy.is_finite()).then_some([sx,sy])
}
pub(super) fn axis_bounds(m:[f64;6],size:[f64;2])->Option<[f64;4]>{
    axis_scales(m)?;let[a,b,c,d,x,y]=m;let[w,h]=size;
    Some([x+(a*w).min(0.)+(c*h).min(0.),y+(b*w).min(0.)+(d*h).min(0.),a.abs()*w+c.abs()*h,b.abs()*w+d.abs()*h])
}
pub(super) fn independent_axis_nib(object:&mut Object)->Result<Option<(u16,i32)>> {
    if !object.style.stroke_scale_with_object{return Ok(None);}
    if !matches!(object.geometry,Geometry::Rect{radius_mm:0.}|Geometry::Ellipse)||object.clip.is_some()||object.style.stroke=="none"||object.style.stroke_width_mm<=0.||object.style.opacity!=1.{return Ok(None);}
    let m=kurbo::Affine::new(object.transform)*kurbo::Affine::translate((object.x_mm,object.y_mm));
    let axis=axis_scales(m.as_coeffs());let Some([sx,sy])=axis.or_else(||column_scales(m.as_coeffs()))else{return Ok(None)};
    if sx==sy{return Ok(None);}
    let major=sx.max(sy);let minor=sx.min(sy);let aspect=(minor/major*100.).round_ties_even();
    if !(1.0..100.).contains(&aspect)||major*(aspect/100.)!=minor{return Ok(None);}
    let angle=if sx>sy{-90_000_000}else{0};
    // Keep fine source charts in their existing adaptive-unit route. A
    // half-grid dimension after nib recharting must not create a new loss of
    // spelling/precision in the established local-frame recovery contract.
    if [object.width_mm,object.height_mm,object.style.stroke_width_mm].into_iter().any(|v|!coord(v).is_ok_and(|q|q as f64/10000.==v)){return Ok(None);}
    let width=object.style.stroke_width_mm*major;if coord(width)? as f64/10000.!=width{return Ok(None);}
    let [a,b,c,d,x,y]=m.as_coeffs();
    let (frame,w,h)=if axis.is_some(){let[x,y,w,h]=axis_bounds(m.as_coeffs(),[object.width_mm,object.height_mm]).unwrap();([1.,0.,0.,1.,x,y],w,h)}else{([a/sx,b/sx,c/sy,d/sy,x,y],object.width_mm*sx,object.height_mm*sy)};
    let mut frame=frame;
    if axis.is_none(){
        if frame[0].hypot(frame[1])!=1.||frame[2].hypot(frame[3])!=1.{return Ok(None);}
        if frame[0]*frame[3]-frame[1]*frame[2]<0.{frame[4]+=frame[0]*w;frame[5]+=frame[1]*w;frame[0]= -frame[0];frame[1]= -frame[1];}
    }
    if [w,h].into_iter().any(|v|!coord(v).is_ok_and(|q|q>0&&q as f64/10000.==v)){return Ok(None);}
    object.width_mm=w;object.height_mm=h;object.transform=frame;object.x_mm=0.;object.y_mm=0.;object.style.stroke_width_mm=width;Ok(Some((aspect as u16,angle)))
}

/// General affine primitives need no rounded percentage approximation. Double
/// one local dimension and the nominal pen, halve the corresponding frame
/// column, and use the exact 50% nib on the other axis. Body and pen both keep
/// the complete original affine map. These power-of-two operations preserve
/// every finite binary coefficient; a genuine shear/rotation stays native.
pub(super) fn independent_nib(object:&mut Object)->Result<Option<(u16,i32)>>{
    independent_nib_profile(object,false)
}
pub(super) fn local_circle21(object:&Object)->Result<bool>{
    let[a,b,c,d,_,_]=object.transform;
    Ok(matches!(object.geometry,Geometry::Rect{radius_mm:0.}|Geometry::Ellipse)
        &&object.clip.is_none()&&object.style.stroke_scale_with_object
        &&object.style.stroke!="none"&&object.style.stroke_width_mm>0.&&object.style.opacity==1.
        &&a*c+b*d!=0.&&column_scales(object.transform).is_some()
        &&[object.width_mm,object.height_mm,object.style.stroke_width_mm].into_iter().all(|v|coord(v).is_ok_and(|q|q>0&&q as f64/10000.==v)))
}
/// Import uses the original chart only to compare genuine source fields with
/// native records. Export must also enforce the measured Corel profile gate.
pub(super) fn independent_nib_for_write(object:&mut Object)->Result<Option<(u16,i32)>>{
    independent_nib_profile(object,true)
}
fn independent_nib_profile(object:&mut Object,writing:bool)->Result<Option<(u16,i32)>>{
    if !object.style.stroke_scale_with_object{return Ok(None);}
    if let Some(nib)=independent_axis_nib(object)?{return Ok(Some(nib));}
    if !matches!(object.geometry,Geometry::Rect{radius_mm:0.}|Geometry::Ellipse)||object.clip.is_some()||object.style.stroke=="none"||object.style.stroke_width_mm<=0.||object.style.opacity!=1.{return Ok(None);}
    let m=kurbo::Affine::new(object.transform)*kurbo::Affine::translate((object.x_mm,object.y_mm));let[a,b,c,d,x,y]=m.as_coeffs();
    let Some([sx,sy])=column_scales(m.as_coeffs())else{return Ok(None)};
    if sx==sy&&a*c+b*d==0.{return Ok(None);}
    let grid=|v:f64|coord(v).is_ok_and(|q|q>0&&q as f64/10000.==v);
    if ![object.width_mm,object.height_mm,object.style.stroke_width_mm,2.*object.style.stroke_width_mm].into_iter().all(grid){return Ok(None);}
    // Windows120: all16 oblique rectangles using this unmeasured binary
    // chart became native curves onOpen. Keep the source, reject this route;
    // exact percentage charts and proven ellipse files are separate profiles.
    if writing&&matches!(object.geometry,Geometry::Rect{radius_mm:0.})&&a*c+b*d!=0.{return Err(crate::Error::new("CDR_NATIVE_PROFILE_QUARANTINED",format!("{}: report120 general rectangle binary chart loses the native rectangle type; keep the complete .arranger source",object.id)));}
    let reversible=|v:f64|(v/2.)*2.==v;
    let (mut frame,w,h,angle)=if grid(2.*object.width_mm)&&[a,b].into_iter().all(reversible){([a/2.,b/2.,c,d,x,y],2.*object.width_mm,object.height_mm,-90_000_000)}else if grid(2.*object.height_mm)&&[c,d].into_iter().all(reversible){([a,b,c/2.,d/2.,x,y],object.width_mm,2.*object.height_mm,0)}else{return Ok(None)};
    if frame[0]*frame[3]-frame[1]*frame[2]<0.{frame[4]+=frame[0]*w;frame[5]+=frame[1]*w;frame[0]= -frame[0];frame[1]= -frame[1];}
    object.width_mm=w;object.height_mm=h;object.transform=frame;object.x_mm=0.;object.y_mm=0.;object.style.stroke_width_mm*=2.;Ok(Some((50,angle)))
}

/// Corel's live rounded primitive uses the native .55 quarter-cubic chart.
/// This is only its bbox cache, not a replacement for Arranger's curve model.
pub(super) fn rectangle_bounds(width:f64,height:f64,radius:f64,m:kurbo::Affine)->kurbo::Rect {
    use kurbo::{BezPath,Rect,Shape};
    if radius==0.{return m.transform_rect_bbox(Rect::new(0.,0.,width,height));}
    let r=radius;let k=0.55*r;let mut p=BezPath::new();
    p.move_to((r,0.));p.line_to((width-r,0.));p.curve_to((width-r+k,0.),(width,r-k),(width,r));
    p.line_to((width,height-r));p.curve_to((width,height-r+k),(width-r+k,height),(width-r,height));
    p.line_to((r,height));p.curve_to((r-k,height),(0.,height-r+k),(0.,height-r));
    p.line_to((0.,r));p.curve_to((0.,r-k),(r-k,0.),(r,0.));p.close_path();(m*p).bounding_box()
}

fn property(value:&serde_json::Value,key:&str,default:f64)->Result<f64>{
    let Some(v)=value.get(key)else{return Ok(default)};let n=v.as_str().ok_or_else(||invalid("native vector pen property is not a string"))?.parse::<f64>().map_err(|_|invalid("invalid native vector pen property"))?;if !n.is_finite(){return Err(invalid("nonfinite native vector pen property"));}Ok(n)
}

pub(super) fn local_style(value:&serde_json::Value,outline:&[u8],m:[f64;6],parent:Option<[f64;6]>)->Result<Option<serde_json::Value>>{
    let at=super::read::outline_body(outline)?;let kind=u16_at(outline,at)?;if kind&1!=0{return Ok(None);}
    let Some(pen)=value.get("outline")else{return Ok(None)};let width=i32_at(outline,at+6)? as f64/10000.;let nominal=property(pen,"width",width*10000.)?/10000.;let angle=property(pen,"angle",0.)?;let aspect=property(pen,"aspect",100.)?;
    if width==nominal&&angle==0.&&aspect==100.{return Ok(None);}
    let[a,c,_,b,d,_]=m;let sx=a.hypot(b);let sy=c.hypot(d);
    let quantized=kind==34&&u16_at(outline,at+10)?==100&&pen.get("scaleWithObject").and_then(serde_json::Value::as_str)==Some("1")&&width>0.&&nominal>0.&&super::pen::quantized_reflected_circle(width,nominal,aspect,angle,m)?;
    // Corel122's grouped Rotate after Stretch retains a preset in the
    // parent's axes before rotation. Require that actual parent linear frame,
    // not just a same-size outer box or an arbitrary cache discrepancy.
    let group_axes=kind==34&&u16_at(outline,at+10)?==100&&pen.get("scaleWithObject").and_then(serde_json::Value::as_str)==Some("1")&&width>0.&&nominal>0.&&a*c+b*d==0.&&parent.is_some_and(|p|[p[0],p[1],p[3],p[4]]==[a,c,b,d])&&sx>0.&&sy>0.&&sx!=sy&&nominal==super::binary::coord(width*sx.max(sy))? as f64/10000.&&aspect==sx.min(sy)/sx.max(sy)*100.&&angle.rem_euclid(180.)==if sx>sy{90.}else{0.};
    if !quantized&&!group_axes&&(kind!=34||pen.get("scaleWithObject").and_then(serde_json::Value::as_str)!=Some("1")||a*c+b*d!=0.||sx<=0.||sy<=0.||width<=0.||nominal<=0.||!(1.0..=100.).contains(&aspect)){return Err(super::unsupported("native vector pen requires an unmeasured frame/style profile"));}
    // Unique symmetric square roots of the two ellipse covariance matrices.
    // Their Frobenius distance bounds the pen at every direction. Orthogonal
    // columns let us construct the raw root directly without determinant
    // cancellation for long/thin frames. All values are in native y-up space.
    let raw=[(a*a/sx+c*c/sy)*width/2.,(a*b/sx+c*d/sy)*width/2.,(b*b/sx+d*d/sy)*width/2.];
    let(s,c)=(angle-90.).to_radians().sin_cos();let major=nominal/2.;let minor=major*aspect/100.;let typed=[c*c*major+s*s*minor,c*s*(major-minor),s*s*major+c*c*minor];
    let error=((raw[0]-typed[0]).powi(2)+2.*(raw[1]-typed[1]).powi(2)+(raw[2]-typed[2]).powi(2)).sqrt()*super::read::miter_limit(outline)?;
    if !quantized&&!group_axes&&(!error.is_finite()||error>0.00021){return Err(super::unsupported("native local and typed world pens differ beyond the physical bound"));}
    let mut result=value.clone();let pen=result.get_mut("outline").and_then(serde_json::Value::as_object_mut).ok_or_else(||invalid("native outline style is not an object"))?;
    pen.insert("width".into(),serde_json::json!(i32_at(outline,at+6)?.to_string()));if pen.contains_key("angle"){pen.insert("angle".into(),serde_json::json!("0"));}if pen.contains_key("aspect"){pen.insert("aspect".into(),serde_json::json!("100"));}Ok(Some(result))
}

pub(super) fn world_style(value:&mut serde_json::Value,m:[f64;6])->Result<bool>{
    let Some(pen)=value.get("outline")else{return Ok(false)};if pen.get("scaleWithObject").and_then(serde_json::Value::as_str)!=Some("1"){return Ok(false);}
    let width=property(pen,"width",0.)?;if width<=0.{return Ok(false);}
    let[a,c,_,b,d,_]=m;
    // Emit only exact UI scalars in the measured uniform/positive-axis
    // profiles. Unmeasured skew/calligraphy precision is not approximated.
    let(scale,aspect,angle)=if a==d&&b== -c{(a.hypot(b),100.,0.)}else if b==0.&&c==0.&&a>0.&&d>0.{(a.max(d),a.min(d)/a.max(d)*100.,if a>d{-90.}else{0.})}else{return Ok(false);};
    let nominal=width*scale;if scale==1.&&aspect==100.||!nominal.is_finite()||nominal.fract()!=0.||aspect.fract()!=0.||!(1.0..=100.).contains(&aspect){return Ok(false);}
    let pen=value.get_mut("outline").unwrap().as_object_mut().ok_or_else(||invalid("native outline style is not an object"))?;pen.insert("width".into(),serde_json::json!(nominal.to_string()));pen.insert("aspect".into(),serde_json::json!(aspect.to_string()));pen.insert("angle".into(),serde_json::json!(angle.to_string()));Ok(true)
}


#[cfg(test)]
mod tests{
    use super::*;
    use serde_json::json;
    #[test]
    fn percentage_projection_never_rounds_a_neighbouring_axis_or_native_width(){
        let o:crate::Object=serde_json::from_value(json!({"id":"r","page_id":"page-1","x_mm":0.,"y_mm":0.,"width_mm":20.,"height_mm":10.,"transform":[1.,0.,0.,0.6,0.,0.],"geometry":{"kind":"rect","radius_mm":0.},"style":{"fill":"none","stroke":"#000000","stroke_width_mm":1.,"opacity":1.}})).unwrap();
        let mut valid=o.clone();assert_eq!(independent_axis_nib(&mut valid).unwrap(),Some((60,-90_000_000)));
        for change in 0..4{let mut bad=o.clone();match change{0=>bad.transform[3]=f64::from_bits(0.6f64.to_bits()+1),1=>bad.transform[3]=2./3.,2=>{bad.transform[0]=1.25;bad.transform[3]=0.75;bad.style.stroke_width_mm=0.0001;},_=>bad.transform[0]=0.};let before=bad.clone();assert!(independent_axis_nib(&mut bad).unwrap().is_none());assert_eq!(bad,before);}
        let mut shear=o.clone();shear.transform[1]=1e-12;assert_eq!(independent_axis_nib(&mut shear).unwrap(),Some((60,-90_000_000)));assert_eq!(shear.transform[1],1e-12,"a real small shear remains in the actual frame");
    }
    #[test]
    fn signed_axis_projection_is_exact_and_unsupported_cases_stay_unchanged(){
        let o:crate::Object=serde_json::from_value(json!({"id":"r","page_id":"page-1","x_mm":10.,"y_mm":12.,"width_mm":20.,"height_mm":10.,"transform":[0.,0.5,-1.,0.,100.,100.],"geometry":{"kind":"ellipse"},"style":{"fill":"#123456","stroke":"#000000","stroke_width_mm":1.,"opacity":1.}})).unwrap();
        let mut good=o.clone();assert_eq!(independent_axis_nib(&mut good).unwrap(),Some((50,-90_000_000)));assert_eq!(good.transform,[1.,0.,0.,1.,78.,105.]);assert_eq!([good.width_mm,good.height_mm],[10.,10.]);
        for kind in 0..6{let mut bad=o.clone();match kind{0=>bad.transform[0]=0.1,1=>bad.transform[1]=2./3.,2=>bad.width_mm=20.00001,3=>bad.geometry=Geometry::Rect{radius_mm:1.},4=>bad.transform[1]=0.,_=>bad.clip=Some(crate::Clip::Rect{rect_mm:[0.,0.,1.,1.]})};let before=bad.clone();assert!(independent_axis_nib(&mut bad).unwrap().is_none());assert_eq!(bad,before);}
    }
    #[test]
    fn collapsed_ellipse_cache_matches_both_genuine_corel_returned_records(){
        for version in [14,21]{let bytes=std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../examples/assets/cdr/corel2019/save100-controls/tiny-ellipse{version}.cdr"))).unwrap();let native=read_native(&bytes).unwrap();let node=all(&native.nodes).into_iter().find(|n|n.kind()==Some(b"obj ")&&super::super::write::arguments(&n.find(b"loda").unwrap().data).is_ok_and(|(_,a)|a.iter().any(|(k,v)|*k==1000&&read_wide(v).is_ok_and(|s|s=="content")))).unwrap();let actual=node.find(b"bbox").unwrap().data.clone();let mut zero=node.clone();let x=i32_at(&actual,0).unwrap()+2;let y=i32_at(&actual,4).unwrap()-2;zero.find_mut(b"bbox").unwrap().data=[x,y,x,y].into_iter().flat_map(i32::to_le_bytes).collect();minimum_ellipse_cache(&mut zero).unwrap();assert_eq!(zero.find(b"bbox").unwrap().data,actual);let mut obbx=actual.clone();obbx.extend(&actual);assert_eq!(zero.find(b"obbx").unwrap().data,obbx);}
        let mut ordinary=Node::list(b"obj ",vec![Node::leaf(b"bbox",[0i32,4,3,0].into_iter().flat_map(i32::to_le_bytes).collect()),Node::leaf(b"obbx",vec![42])]);let before=ordinary.clone();minimum_ellipse_cache(&mut ordinary).unwrap();assert_eq!(ordinary.find(b"bbox").unwrap().data,before.find(b"bbox").unwrap().data);assert_eq!(ordinary.find(b"obbx").unwrap().data,vec![42]);
    }
    #[test]
    fn rounded_cache_extrema_match_real_converted_native_curves_in_rotated_frames(){
        use kurbo::{Affine,Shape};
        let doc=crate::cdr::decode(include_bytes!("../../../../examples/assets/cdr/corel2019/rounded-scaled-v21.cdr"),"round").unwrap().0;let object=doc.object("edit-shape").unwrap();let Geometry::Rect{radius_mm}=object.geometry else{panic!()};
        let native=read_native(include_bytes!("../../../../examples/assets/cdr/corel2019/rounded-scaled-with-curve-v21.cdr")).unwrap();let nodes=all(&native.nodes);let config=nodes.iter().find(|n|n.tag==*b"mcfg").unwrap();let origin=super::super::origin::paper(&config.data,[210.,297.],true).unwrap();
        let node=nodes.iter().find(|n|n.kind()==Some(b"obj ")&&super::super::write::arguments(&n.find(b"loda").unwrap().data).is_ok_and(|(k,_)|k==3)).unwrap();let(_,args)=super::super::write::arguments(&node.find(b"loda").unwrap().data).unwrap();let g=&args.iter().find(|(k,_)|*k==30).unwrap().1;
        let tr=&node.find(b"trfd").unwrap().data;let at=u32_at(tr,u32_at(tr,8).unwrap() as usize).unwrap() as usize+16;let m:[f64;6]=std::array::from_fn(|i|f64_at(tr,at+i*8).unwrap());let frame=Affine::new([m[0],-m[3],-m[1],m[4],m[2]/10000.+origin[0],origin[1]-m[5]/10000.]);let curve=frame*super::super::read::curve(g).unwrap();
        for m in [Affine::IDENTITY,Affine::new([0.6,0.8,-0.8,0.6,5.,6.]),Affine::new([1.,0.25,0.5,1.,0.,0.])]{let a=rectangle_bounds(object.width_mm,object.height_mm,radius_mm,m*doc.world_transform(object).unwrap());let b=(m*curve.clone()).bounding_box();for(x,y)in [a.x0,a.y0,a.x1,a.y1].into_iter().zip([b.x0,b.y0,b.x1,b.y1]){assert!((x-y).abs()<=0.00021,"native bbox extrema differ: {x}/{y}");}}
        let b=rectangle_bounds(20.,10.,0.8,Affine::new([1.5,2.,-2.,1.5,60.,40.]));assert_eq!([coord(b.x0).unwrap(),coord(b.y0).unwrap(),coord(b.x1).unwrap(),coord(b.y1).unwrap()],[408021,408021,891979,941979]);
    }
    #[test]
    fn native_local_and_world_pen_scalars_must_describe_the_same_ellipse(){
        let bytes=include_bytes!("../../../../examples/assets/cdr/corel2019/rectangle-scales/outline-scaled-nonuniform2x1-v21.cdr");let native=read_native(bytes).unwrap();let line=all(&native.nodes).into_iter().find(|n|n.tag==*b"outl"&&super::super::read::outline(&n.data).is_ok_and(|(s,_)|s!="none")).unwrap();
        let value=json!({"outline":{"width":"20000","angle":"-90","aspect":"50","scaleWithObject":"1"}});let m=[2.,0.,0.,0.,1.,0.];let local=local_style(&value,&line.data,m,None).unwrap().unwrap();assert_eq!(local["outline"]["width"],"10000");assert_eq!(local["outline"]["angle"],"0");assert_eq!(local["outline"]["aspect"],"100");
        for (key,bad) in [("width","10000"),("angle","0"),("aspect","100"),("scaleWithObject","0")]{let mut changed=value.clone();changed["outline"][key]=json!(bad);assert!(local_style(&changed,&line.data,m,None).is_err(),"{key}");}
        let mut emitted=local;assert!(world_style(&mut emitted,m).unwrap());assert_eq!(emitted,value);
    }

}
