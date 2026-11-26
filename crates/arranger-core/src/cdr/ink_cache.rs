//! Native obbx stores geometric bounds followed by painted bounds. Stroke
//! outlines below are cache calculations only; editable geometry stays intact.
use super::{Result,unsupported};
use crate::{Document,Object,Geometry,LineCap,LineJoin};
use kurbo::{Rect,Shape,Stroke,StrokeOpts,Cap,Join,Affine};
use std::collections::BTreeMap;

#[cfg(test)]
pub(super) fn all(doc:&Document)->Result<BTreeMap<String,Rect>>{with_nibs(doc,&BTreeMap::new())}
pub(super) fn with_nibs(doc:&Document,nibs:&BTreeMap<String,(u16,i32)>)->Result<BTreeMap<String,Rect>>{
    fn ink(doc:&Document,o:&Object,nibs:&BTreeMap<String,(u16,i32)>,cache:&mut BTreeMap<String,Rect>)->Result<Option<Rect>>{
        if let Some(r)=cache.get(&o.id){return Ok(Some(*r));}
        let children=if matches!(o.geometry,Geometry::Group){doc.objects.iter().filter(|c|c.parent_id.as_deref()==Some(&o.id)).map(|c|ink(doc,c,nibs,cache)).collect::<Result<Vec<_>>>()?.into_iter().flatten().collect::<Vec<_>>()}else{vec![]};
        if children.is_empty()&&(matches!(o.geometry,Geometry::Group|Geometry::Image{..})||o.style.stroke=="none"||o.style.stroke_width_mm<=0.){return Ok(None);}
        let [x,y,w,h]=doc.bounds(o)?;let mut result=Rect::new(x,y,x+w,y+h);
        if matches!(o.geometry,Geometry::Group){
            for child in children{result=result.union(child);}
        }else if o.style.stroke!="none"&&o.style.stroke_width_mm>0.&&!matches!(o.geometry,Geometry::Image{..}){
            let m=doc.world_transform(o)?;let[a,b,c,d,_,_]=m.as_coeffs();let width=super::binary::coord(o.style.stroke_width_mm)? as f64/10000.;let half=width/2.;
            let native_primitive=if !o.style.stroke_scale_with_object&&matches!(o.geometry,Geometry::Rect{..}|Geometry::Ellipse){
                let w=super::binary::coord(o.width_mm)? as f64/10000.;let h=super::binary::coord(o.height_mm)? as f64/10000.;let frame=m*Affine::translate((0.,o.height_mm-h));
                result=if let Geometry::Rect{radius_mm}=o.geometry{super::primitive_frames::rectangle_bounds(w,h,super::binary::coord(radius_mm)? as f64/10000.,frame)}else{let[a,b,c,d,_,_]=frame.as_coeffs();let p=frame*kurbo::Point::new(w/2.,h/2.);let rx=(a*w/2.).hypot(c*h/2.);let ry=(b*w/2.).hypot(d*h/2.);Rect::new(p.x-rx,p.y-ry,p.x+rx,p.y+ry)};Some((frame,w,h))
            }else{None};
            if matches!(o.geometry,Geometry::Text{layout:crate::TextLayoutOptions{mode:crate::TextMode::Area,..},..}){
                // Actual paragraph obbx includes its logical frame, even when
                // the current glyphs occupy only a small part of that frame.
                result=result.union(m.transform_rect_bbox(Rect::new(-half,-width,o.width_mm+half,o.height_mm+half)));
            }
            let pen=if !o.style.stroke_scale_with_object{o.style.page_pen_basis()}else{match nibs.get(&o.id){Some((aspect,angle))=>{let ratio=f64::from(*aspect)/100.;if angle.rem_euclid(180_000_000)==0{Affine::scale_non_uniform(ratio,1.)}else{Affine::scale_non_uniform(1.,ratio)}},None=>Affine::IDENTITY}};
            if !o.style.stroke_scale_with_object&&matches!(o.geometry,Geometry::Rect{radius_mm}if radius_mm>0.){let[u,v,w,z,_,_]=pen.as_coeffs();result=result.inflate(half*u.hypot(w),half*v.hypot(z));}
            else if matches!(o.geometry,Geometry::Ellipse){
                // Exact support of the centre ellipse plus the effective pen.
                let[u,v,w,z,_,_]=if o.style.stroke_scale_with_object{(m*pen).as_coeffs()}else{pen.as_coeffs()};
                result=Rect::new(result.x0-half*u.hypot(w),result.y0-half*v.hypot(z),result.x1+half*u.hypot(w),result.y1+half*v.hypot(z));
            }else{
                let cap=match o.style.stroke_linecap{LineCap::Butt=>Cap::Butt,LineCap::Round=>Cap::Round,LineCap::Square=>Cap::Square};
                let mut style=Stroke::new(width);style.join=match o.style.stroke_linejoin{LineJoin::Miter=>Join::Miter,LineJoin::Round=>Join::Round,LineJoin::Bevel=>Join::Bevel};style.miter_limit=o.style.stroke_miterlimit;style.start_cap=cap;style.end_cap=cap;
                let path=if o.style.stroke_scale_with_object{pen.inverse()*doc.object_path(o)?}else if let Some((frame,w,h))=native_primitive{pen.inverse()*frame*Rect::new(0.,0.,w,h).to_path(1e-4)}else{pen.inverse()*m*super::read::curve(&super::write::path_geometry(&doc.object_path(o)?)?)?};
                if !path.elements().is_empty(){
                    let scale=if o.style.stroke_scale_with_object{(a*a+b*b+c*c+d*d).sqrt().max(1.)}else{1.};let tolerance=0.00001/scale;
                    let stroke=kurbo::stroke(path.iter(),&style,&StrokeOpts::default(),tolerance);
                    if !stroke.elements().is_empty(){let frame=if o.style.stroke_scale_with_object{m*pen}else{pen};result=result.union((frame*stroke).bounding_box().inflate(0.00001,0.00001));}
                }
            }
        }
        if [result.x0,result.y0,result.x1,result.y1].iter().any(|v|!v.is_finite()){return Err(unsupported("painted native cache bounds are not finite"));}
        cache.insert(o.id.clone(),result);Ok(Some(result))
    }
    let mut cache=BTreeMap::new();for o in &doc.objects{ink(doc,o,nibs,&mut cache)?;}Ok(cache)
}

pub(super) fn encoded(rect:Rect,size:[f64;2])->Result<Vec<u8>>{
    let [w,h]=size;let values=[rect.x0-w/2.,h/2.-rect.y0,rect.x1-w/2.,h/2.-rect.y1];let mut out=vec![];
    for v in values{
        let q=(v*10000.).round_ties_even();
        if !q.is_finite()||q<f64::from(i32::MIN)||q>f64::from(i32::MAX){return Err(unsupported("painted native cache exceeds the coordinate range"));}
        out.extend((q as i32).to_le_bytes());
    }Ok(out)
}
#[cfg(test)]
mod tests{
 use super::*;use super::super::binary::{read_native,read_wide,i32_at};
 fn document()->Document{serde_json::from_value(serde_json::json!({"id":"ink","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"group","page_id":"page-1","x_mm":0.,"y_mm":0.,"width_mm":0.,"height_mm":0.,"geometry":{"kind":"group"}},{"id":"shape","page_id":"page-1","parent_id":"group","x_mm":40.,"y_mm":60.,"width_mm":60.,"height_mm":40.,"geometry":{"kind":"rect","radius_mm":0.},"style":{"fill":"#12345680","stroke":"#abcdef40","stroke_width_mm":12.,"opacity":1.}}]})).unwrap()}
 #[test]
 fn stroke_cache_contains_outer_ink_without_changing_group_geometry(){
  let doc=document();let cache=all(&doc).unwrap();assert_eq!(doc.bounds(doc.object("group").unwrap()).unwrap(),[40.,60.,60.,40.]);assert_eq!(cache["shape"],cache["group"]);let r=cache["group"];assert!(r.x0<=34.&&r.y0<=54.&&r.x1>=106.&&r.y1>=106.);assert!(r.x0>33.9999&&r.x1<106.0001);
  for encode in [crate::cdr::encode_document,crate::cdr::encode_document21]{let raw=encode(&doc).unwrap().0;let native=read_native(&raw).unwrap();for n in super::super::binary::all(&native.nodes).into_iter().filter(|n|matches!(n.kind(),Some(b"obj "|b"grp "))){let(_,a)=super::super::write::arguments(&super::super::read::own_record(n,b"loda").unwrap().data).unwrap();let name=a.iter().find(|(k,_)|*k==1000).map(|(_,v)|read_wide(v).unwrap()).unwrap();if name=="group"||name=="shape"{let b=&super::super::read::own_record(n,b"bbox").unwrap().data;let o=&super::super::read::own_record(n,b"obbx").unwrap().data;assert_eq!(&o[..16],b);assert!(i32_at(o,16).unwrap()<i32_at(b,0).unwrap());assert!(i32_at(o,24).unwrap()>i32_at(b,8).unwrap());}}assert_eq!(crate::cdr::decode(&raw,&doc.id).unwrap().0,doc);}
 }
 #[test]
 fn unstroked_long_text_does_not_request_a_new_ink_measurement(){let mut doc=document();let o=doc.objects.iter_mut().find(|o|o.id=="shape").unwrap();o.geometry=Geometry::Text{content:"A".repeat(65534),font_family:"Arial".into(),font_size_mm:3.,layout:Default::default(),text_path:None};o.style.stroke="none".into();assert!(all(&doc).unwrap().is_empty());}
 #[test]
 fn percentage_nib_cache_tracks_both_actual_stroke_axes(){
  let mut doc=document();let o=doc.objects.iter_mut().find(|o|o.id=="shape").unwrap();o.style.stroke_width_mm=1.;let nibs=BTreeMap::from([("shape".into(),(25,-90_000_000))]);let r=with_nibs(&doc,&nibs).unwrap()["shape"];
  assert!((r.x0-39.5).abs()<0.00002&&(r.x1-100.5).abs()<0.00002);assert!((r.y0-59.875).abs()<0.00002&&(r.y1-100.125).abs()<0.00002);
 }
 #[test]
 fn independent_ellipse_cache_uses_the_smaller_effective_pen_axis(){let mut doc=document();let o=doc.objects.iter_mut().find(|o|o.id=="shape").unwrap();o.geometry=Geometry::Ellipse;o.style.stroke_width_mm=1.;let nibs=BTreeMap::from([("shape".into(),(25,-90_000_000))]);let r=with_nibs(&doc,&nibs).unwrap()["shape"];assert_eq!(r,Rect::new(39.5,59.875,100.5,100.125));}
 #[test]
 fn affine_ellipse_uses_analytic_body_and_transformed_round_pen(){let mut doc=document();let o=doc.objects.iter_mut().find(|o|o.id=="shape").unwrap();o.geometry=Geometry::Ellipse;o.transform=[2.,0.25,0.125,1.5,3.,4.];let[x,y,w,h]=doc.bounds(doc.object("shape").unwrap()).unwrap();let r=all(&doc).unwrap()["shape"];let px=6.*2f64.hypot(0.125);let py=6.*0.25f64.hypot(1.5);assert_eq!(r,Rect::new(x-px,y-py,x+w+px,y+h+py));}
}
