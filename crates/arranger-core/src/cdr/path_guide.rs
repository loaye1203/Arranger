//! Equivalent local coordinates for a live straight guide. No text or guide
//! copy is created: native geometry and its matrix still own every endpoint.
use super::{Result,path_precision};
use crate::{Document,Geometry,Object};
use kurbo::{Affine,PathEl};
use std::collections::{BTreeMap,BTreeSet};

/// An open, single-contour straight span, including monotone Bezier pieces.
/// Exact collinearity and the Bezier derivative prove a non-reversing span;
/// no flatness tolerance may turn a real bend into a different live guide.
pub(super) fn linear_span(path:&kurbo::BezPath,units:u32)->Option<[kurbo::Point;2]>{
    let elements=path.elements();let PathEl::MoveTo(start)=*elements.first()?else{return None};
    let end=elements.last()?.end_point()?;
    if !path_precision::valid_units(units){return None;}
    // Native points are signed 32-bit integers. Their exact i128 determinant
    // also catches a one-unit bend that floating products could cancel away.
    // Requiring reversible coordinates prevents quantization from flattening
    // a real source bend before this predicate is called.
    let grid=|p:kurbo::Point|->Option<[i128;2]>{
        let mut result=[0;2];for(i,v)in[p.x,p.y].into_iter().enumerate(){let n=super::binary::coord(v*f64::from(units)).ok()?;if n as f64/10000./f64::from(units)!=v{return None;}result[i]=i128::from(n);}Some(result)
    };
    let first=grid(start)?;let last=grid(end)?;let delta=[last[0]-first[0],last[1]-first[1]];
    if delta==[0,0]{return None;}
    let axis=usize::from(delta[0].abs()<delta[1].abs());
    let sign=delta[axis].signum();let mut previous=first[axis];let mut count=0;
    for element in &elements[1..]{
        let points:[Option<kurbo::Point>;3]=match *element{
            PathEl::LineTo(p)=>[Some(p),None,None],PathEl::QuadTo(c,p)=>[Some(c),Some(p),None],
            PathEl::CurveTo(a,b,p)=>[Some(a),Some(b),Some(p)],_=>return None,
        };
        let mut coordinates=vec![previous];
        for p in points.into_iter().flatten(){
            let point=grid(p)?;let v=[point[0]-first[0],point[1]-first[1]];
            if v[0]*delta[1]!=v[1]*delta[0]{return None;}
            coordinates.push(point[axis]);count+=1;
        }
        let differences=coordinates.windows(2).map(|v|(v[1]-v[0])*sign).collect::<Vec<_>>();
        let monotone=match differences.as_slice(){
            [a]=>*a>=0,[a,b]=>*a>=0&&*b>=0,
            // A quadratic Bernstein derivative with nonnegative endpoints is
            // nonnegative throughout iff its middle term >= -sqrt(a*c).
            // Square only a negative middle term; all integer products fit.
            [a,b,c]=>*a>=0&&*c>=0&&(*b>=0||b*b<=a*c),_=>false,
        };
        if !monotone{return None;}previous=*coordinates.last()?;
    }
    (count>0).then_some([start,end])
}

pub(super) fn straight(object:&Object)->Result<Option<[kurbo::Point;2]>>{
    let Geometry::Path{d,..}=&object.geometry else{return Ok(None)};
    Ok(match crate::path::parse(d)?.elements(){[PathEl::MoveTo(p),PathEl::LineTo(q)]=>Some([*p,*q]),_=>None})
}
pub(super) fn planned(doc:&Document)->Result<BTreeMap<String,Object>>{
    let referenced=doc.objects.iter().filter_map(|o|if let Geometry::Text{text_path:Some(p),..}=&o.geometry{Some(p.path_id.clone())}else{None}).collect::<BTreeSet<_>>();
    let mut charts=BTreeMap::new();
    for id in referenced{let o=doc.object(&id)?;if let Some(chart)=plan(doc,o)?{charts.insert(id,chart);}}
    Ok(charts)
}
fn plan(doc:&Document,o:&Object)->Result<Option<Object>>{
    let path=o.local_path()?;
    let Some([p,q])=straight(o)?.or_else(||linear_span(&path,1))else{return Ok(None)};
    let world=doc.world_transform(o)?;let [a,b,c,d,_,_]=world.as_coeffs();
    // A unit rotation preserves the existing circular pen. Adaptive coordinate
    // units, elliptical nibs and clipping keep their current native charts.
    if [a,b,c,d]!=[1.,0.,0.,1.]||o.clip.is_some()||o.style.stroke_fixed_nib.is_some()||o.style.stroke!="none"&&!super::binary::coord(o.style.stroke_width_mm).is_ok_and(|n|n as f64/10000.==o.style.stroke_width_mm)||path_precision::plan(doc,o)?.is_some(){return Ok(None);}
    if [p.x,p.y,q.x,q.y].into_iter().any(|v|!super::binary::coord(v).is_ok_and(|n|n as f64/10000.==v)){return Ok(None);}
    let delta=q-p;let length=delta.hypot();
    if delta.x==0.||delta.y==0.||!length.is_finite()||length<=0.||!super::binary::coord(length).is_ok_and(|n|n as f64/10000.==length){return Ok(None);}
    let(u,v)=(delta.x/length,delta.y/length);if u.hypot(v)!=1.{return Ok(None);}
    // Every original control is preserved. Use integer dot products and
    // exact divisibility to prove a native-grid horizontal coordinate; never
    // round a small bend or a non-representable control onto the axis.
    let grid=|v:f64|->Option<i128>{let n=super::binary::coord(v).ok()?;(n as f64/10000.==v).then_some(i128::from(n))};
    let (Some(px),Some(py),Some(qx),Some(qy),Some(den))=(grid(p.x),grid(p.y),grid(q.x),grid(q.y),grid(length))else{return Ok(None)};
    let project=|point:kurbo::Point|->Option<kurbo::Point>{let(x,y)=(grid(point.x)?-px,grid(point.y)?-py);let(dx,dy)=(qx-px,qy-py);if x*dy!=y*dx{return None;}let dot=x*dx+y*dy;if dot%den!=0{return None;}let raw=i32::try_from(dot/den).ok()?;Some(kurbo::Point::new(f64::from(raw)/10000.,0.))};
    let mut horizontal=kurbo::BezPath::new();
    for el in path.elements(){let lowered=match *el{
        PathEl::MoveTo(p)=>project(p).map(PathEl::MoveTo),PathEl::LineTo(p)=>project(p).map(PathEl::LineTo),
        PathEl::QuadTo(a,b)=>project(a).zip(project(b)).map(|(a,b)|PathEl::QuadTo(a,b)),
        PathEl::CurveTo(a,b,c)=>project(a).zip(project(b)).zip(project(c)).map(|((a,b),c)|PathEl::CurveTo(a,b,c)),_=>None,
    };let Some(lowered)=lowered else{return Ok(None)};horizontal.push(lowered);}
    let start=world*p;let mut chart=o.clone();chart.x_mm=0.;chart.y_mm=0.;chart.transform=[u,v,-v,u,start.x,start.y];
    let Geometry::Path{fill_rule,..}=o.geometry else{unreachable!()};let d=if matches!(path.elements(),[PathEl::MoveTo(_),PathEl::LineTo(_)]){format!("M0 0L{length} 0")}else{horizontal.to_svg()};chart.geometry=Geometry::Path{d,fill_rule};Ok(Some(chart))
}
pub(super) fn lower(source:&Document,native:&mut Document)->Result<()>{
    for(id,chart)in planned(source)?{*native.objects.iter_mut().find(|o|o.id==id).expect("normalized guide")=chart;}Ok(())
}
pub(super) fn signature(path:&kurbo::BezPath)->Vec<u8>{path.elements().iter().map(|el|match el{PathEl::MoveTo(_)=>0,PathEl::LineTo(_)=>1,PathEl::QuadTo(..)|PathEl::CurveTo(..)=>3,PathEl::ClosePath=>4}).collect()}
/// Re-express the actual edited native line in the old source coordinate chart.
/// Never restore old endpoints. Non-unit matrices or nonlinear edits retain
/// their native chart, including its pen units.
#[cfg(test)]
pub(super) fn restore(native:&Object,actual:Affine,source:Affine)->Result<Option<Geometry>>{
    restore_contour(native,actual,source,None)
}
pub(super) fn restore_contour(native:&Object,actual:Affine,source:Affine,declared:Option<&[u8]>)->Result<Option<Geometry>>{
    let path=native.local_path()?;
    if let Some(declared)=declared{if signature(&path)!=declared||linear_span(&path,1).is_none(){return Ok(None);}}
    else if straight(native)?.is_none(){return Ok(None);}
    let [a,b,c,d,_,_]=actual.as_coeffs();let s=source.as_coeffs();
    if [s[0],s[1],s[2],s[3]]!=[1.,0.,0.,1.]||a!=d||b!= -c||a.hypot(b)!=1.||native.style.stroke_fixed_nib.is_some(){return Ok(None);}
    let Geometry::Path{fill_rule,..}=native.geometry else{unreachable!()};
    let path=source.inverse()*actual*path;
    // Coordinate-free notation later reconstructs source-grid numbers. It must
    // not round away a real sub-grid affine edit in this re-expressed chart.
    // Only arithmetic roundoff around an exact native point is reversible;
    // otherwise keep the actual native frame and points together.
    for el in path.elements(){let points=match *el{PathEl::MoveTo(p)|PathEl::LineTo(p)=>vec![p],PathEl::QuadTo(a,b)=>vec![a,b],PathEl::CurveTo(a,b,c)=>vec![a,b,c],_=>return Ok(None)};for p in points{for value in[p.x,p.y]{
        let Ok(grid)=super::binary::coord(value)else{return Ok(None)};let projected=grid as f64/10000.;
        if (value-projected).abs()>64.*f64::EPSILON*value.abs().max(1.){return Ok(None);}
    }}}
    Ok(Some(Geometry::Path{d:path.to_svg(),fill_rule}))
}

#[cfg(test)]
mod contour_tests {
    use super::*;
    use super::super::binary::*;
    fn source(path:&str,parent:bool,scale:f64)->Document {
        let mut objects=vec![serde_json::json!({"id":"guide","page_id":"page-1","x_mm":35.,"y_mm":95.,"width_mm":120.,"height_mm":96.,"geometry":{"kind":"path","d":path},"style":{"fill":"none","stroke":"#888888","stroke_width_mm":0.2,"opacity":1.}})];
        if parent{objects.insert(0,serde_json::json!({"id":"parent","page_id":"page-1","x_mm":3.,"y_mm":4.,"width_mm":0.,"height_mm":0.,"transform":[0.,2.,-2.,0.,0.,0.],"geometry":{"kind":"group"}}));}
        let first=crate::path::parse(path).unwrap().elements()[0].end_point().unwrap();let delta=[(1.-scale)*(35.+first.x)+1.,(1.-scale)*(95.+first.y)+0.5];
        let(px,py)=if parent{(-8.,6.)}else{(0.,0.)};let local=[delta[0]-(1.-scale)*px,delta[1]-(1.-scale)*py];let local=if parent{[local[1]/2.,-local[0]/2.]}else{local};
        let mut text=serde_json::json!({"id":"text","page_id":"page-1","x_mm":0.25,"y_mm":-0.5,"width_mm":60.,"height_mm":12.,"transform":[scale,0.,0.,scale,local[0]-scale*0.25,local[1]+scale*0.5],"geometry":{"kind":"text","content":"Live 42","font_family":"Arial","font_size_mm":4.,"layout":{"letter_spacing_mm":0.125,"line_height_mm":5.25},"text_path":{"path_id":"guide","start_offset_mm":25.,"baseline_offset_mm":0.125,"anchor":"end"}},"style":{"fill":"#123456","stroke":"#123456","stroke_width_mm":0.125,"opacity":1.}});
        if parent{text["parent_id"]="parent".into();}objects.push(text);
        serde_json::from_value(serde_json::json!({"id":"multi-axis","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":objects})).unwrap()
    }
    fn controls(path:&kurbo::BezPath)->Vec<kurbo::Point>{path.elements().iter().flat_map(|e|match *e{PathEl::MoveTo(p)|PathEl::LineTo(p)=>vec![p],PathEl::QuadTo(a,b)=>vec![a,b],PathEl::CurveTo(a,b,c)=>vec![a,b,c],_=>vec![]}).collect()}
    fn same_controls(a:&Document,b:&Document,id:&str,bound:f64){let(x,y)=(a.object(id).unwrap(),b.object(id).unwrap());let(a,b)=(controls(&(a.world_transform(x).unwrap()*x.local_path().unwrap())),controls(&(b.world_transform(y).unwrap()*y.local_path().unwrap())));assert_eq!(a.len(),b.len());for(p,q)in a.into_iter().zip(b){assert!((p.x-q.x).abs()<=bound&&(p.y-q.y).abs()<=bound,"{p:?} vs {q:?}");}}
    #[test]
    fn diagonal_multi_node_charts_preserve_all_controls_commands_live_text_and_pen(){
        for path in ["M0 0L18 24C54 72 36 48 72 96","M0 0Q36 48 72 96","M72 96C54 72 36 48 18 24L0 0","M5 10L23 34C59 82 41 58 77 106","M0 0L0 0L18 24L72 96"] {for parent in [false,true]{for scale in [1.,1.5]{
            let s=source(path,parent,scale);let chart=planned(&s).unwrap().remove("guide").expect("exact diagonal chart");let original=crate::path::parse(path).unwrap();let lowered=chart.local_path().unwrap();assert_eq!(original.elements().len(),lowered.elements().len());assert!(controls(&lowered).iter().all(|p|p.y==0.));
            for(a,b)in original.elements().iter().zip(lowered.elements()){assert_eq!(std::mem::discriminant(a),std::mem::discriminant(b));}
            for version in [14,21]{let encode=if version==14{crate::cdr::encode_document}else{crate::cdr::encode_document21};let bytes=encode(&s).unwrap().0;let complete=crate::cdr::decode(&bytes,&s.id).unwrap().0;assert_eq!(complete,s,"{path}, parent{parent}, scale{scale}, v{version}");assert_eq!(crate::cdr::decode(&encode(&complete).unwrap().0,&s.id).unwrap().0,s);
                let actual=super::super::read::decode_geometry(&bytes,"bare").unwrap().0;assert_eq!(actual.objects.len(),s.objects.len());assert!(super::super::path_outline::preserves(&actual,actual.object("text").unwrap(),&s,s.object("text").unwrap()).unwrap());let mut projected=s.clone();let Geometry::Path{d,..}=&mut projected.objects.iter_mut().find(|o|o.id=="guide").unwrap().geometry else{unreachable!()};*d=super::super::path_precision::projected(path,1).unwrap().to_svg();same_controls(&actual,&projected,"guide",1e-11);
            }
        }}}
    }
    #[test]
    fn axis_lowering_does_not_flatten_bends_reverse_spans_or_round_irrational_lengths(){
        for path in ["M0 0L18 24.0000001L72 96","M0 0L54 72L18 24L72 96","M0 0L10 10L20 20","M0 0L18 24L72 96Z","M0 0L18 24M36 48L72 96"] {
            let s=source(path,false,1.);assert!(!planned(&s).unwrap().contains_key("guide"),"{path}");
        }
    }
    fn pack(n:&mut NativeRecords)->Vec<u8>{if n.version==2100{repack21(n).unwrap()}else{n.members.insert("content/riffData.cdr".into(),write(&n.nodes).unwrap());zip(&n.members.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>())}}
    #[test]
    fn actual_curve_control_and_subgrid_matrix_edits_keep_native_geometry_and_text(){
        for version in [14,21]{for mode in 0..3{
            let s=source("M0 0L18 24C54 72 36 48 72 96",true,1.5);let encode=if version==14{crate::cdr::encode_document}else{crate::cdr::encode_document21};let mut n=read_native(&encode(&s).unwrap().0).unwrap();
            fn edit(nodes:&mut[Node],mode:usize)->bool{for node in nodes{
                if node.kind()==Some(b"obj "){let(kind,mut args)=super::super::write::arguments(&node.find(b"loda").unwrap().data).unwrap();if args.iter().any(|(k,v)|*k==1000&&read_wide(v).unwrap()=="guide"){
                    if mode<2{let raw=&mut args.iter_mut().find(|(k,_)|*k==30).unwrap().1;let at=24+mode*4;let value=i32_at(raw,at).unwrap();set32(raw,at,(value+1)as u32).unwrap();node.find_mut(b"loda").unwrap().data=super::super::write::loda(kind,&args);}
                    else{let raw=&mut node.find_mut(b"trfd").unwrap().data;let at=u32_at(raw,u32_at(raw,8).unwrap()as usize).unwrap()as usize+32;let v=f64_at(raw,at).unwrap()+0.001;raw[at..at+8].copy_from_slice(&v.to_le_bytes());}return true;
                }}if let Some(children)=&mut node.children{if edit(children,mode){return true;}}
            }false}
            assert!(edit(&mut n.nodes,mode));let bytes=pack(&mut n);let full=crate::cdr::decode(&bytes,"full").unwrap().0;let native=super::super::read::decode_geometry(&bytes,"bare").unwrap().0;same_controls(&full,&native,"guide",1e-11);assert!(super::super::path_outline::preserves(&full,full.object("text").unwrap(),&native,native.object("text").unwrap()).unwrap(),"v{version} mode{mode}");
        }}
    }
    #[test]
    fn missing_or_changed_element_contract_keeps_the_actual_native_guide(){
        for version in [14,21]{for mode in 0..2{
            let s=source("M0 0L18 24C54 72 36 48 72 96",true,1.5);let encode=if version==14{crate::cdr::encode_document}else{crate::cdr::encode_document21};let mut n=read_native(&encode(&s).unwrap().0).unwrap();
            let prefix="ArrangerLocalFramesV1|sha256:";
            for name in ["metadata/metadata.xml","META-INF/metadata.xml"]{if let Some(raw)=n.members.get_mut(name){let text=String::from_utf8(raw.clone()).unwrap();if let Some(start)=text.find(prefix){let end=start+text[start..].find([';','<']).unwrap();let mut value:serde_json::Value=super::super::metadata::decode(prefix,&text[start..end]).unwrap();
                if mode==0{value["frames"]["guide"].as_object_mut().unwrap().remove("guide_elements");}else{value["frames"]["guide"]["guide_elements"]=serde_json::json!([0,1,1]);}
                let changed=super::super::metadata::encode(prefix,&value).unwrap();*raw=text.replacen(&text[start..end],&changed,1).into_bytes();
            }}}
            let bytes=pack(&mut n);let full=crate::cdr::decode(&bytes,"full").unwrap().0;let native=super::super::read::decode_geometry(&bytes,"bare").unwrap().0;same_controls(&full,&native,"guide",1e-11);assert!(super::super::path_outline::preserves(&full,full.object("text").unwrap(),&native,native.object("text").unwrap()).unwrap());
        }}
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    use super::super::binary::*;
    #[test]
    fn straight_contour_proof_keeps_monotone_controls_and_rejects_bends_and_reversals(){
        for d in["M0 0L30 0L120 0","M0 0C20 0 80 0 120 0","M0 0Q60 0 120 0","M120 0C90 0 60 0 30 0L0 0","M0 0L18 24C54 72 36 48 72 96","M0 0L0 0L120 0","M0 0C80 0 40 0 120 0","M0 0C120 0 0 0 120 0"]{
            assert!(linear_span(&crate::path::parse(d).unwrap(),1).is_some(),"{d}");
        }
        for d in["M0 0L30 0.0001L120 0","M0 0L30 0.000000001L120 0","M0 0C20 0.0001 80 0 120 0","M0 0L80 0L40 0L120 0","M0 0C160 0 -40 0 120 0","M0 0L120 0Z","M0 0L60 0M60 0L120 0","M0 0L0 0","M0 0C-1 0 80 0 120 0"]{
            assert!(linear_span(&crate::path::parse(d).unwrap(),1).is_none(),"{d}");
        }
        // Near the native integer limits, a one-unit determinant can round to
        // zero in f64 multiplication. The exact predicate must still reject.
        let mut p=kurbo::BezPath::new();p.move_to((0.,0.));p.line_to((2147483646f64/10000.,2147483645f64/10000.));p.line_to((2147483647f64/10000.,2147483646f64/10000.));
        assert!(linear_span(&p,1).is_none());
    }
    fn source()->Document{serde_json::from_value(serde_json::json!({"id":"axis-guide","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"guide","page_id":"page-1","x_mm":35.,"y_mm":95.,"width_mm":120.,"height_mm":96.,"geometry":{"kind":"path","d":"M0 0L72 96"},"style":{"fill":"none","stroke":"#888888","stroke_width_mm":0.2,"opacity":1.}},{"id":"text","page_id":"page-1","x_mm":0.25,"y_mm":-0.5,"width_mm":60.,"height_mm":12.,"transform":[1.,0.,0.,1.,0.75,1.],"geometry":{"kind":"text","content":"Live 42","font_family":if cfg!(target_os="macos"){"PingFang SC"}else{"Microsoft YaHei"},"font_size_mm":4.,"text_path":{"path_id":"guide","start_offset_mm":25.,"baseline_offset_mm":0.125,"anchor":"start"}}}]})).unwrap()}
    fn pack(native:&mut NativeRecords)->Vec<u8>{if native.version==2100{repack21(native).unwrap()}else{native.members.insert("content/riffData.cdr".into(),write(&native.nodes).unwrap());zip(&native.members.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>())}}
    fn same(a:&Document,b:&Document,id:&str){let(x,y)=(a.object(id).unwrap(),b.object(id).unwrap());assert!(super::super::path_precision::same_world(&a.object_path(x).unwrap(),a.world_transform(x).unwrap(),&b.object_path(y).unwrap(),b.world_transform(y).unwrap()),"{id}");}
    #[test]
    fn guide_has_horizontal_native_points_and_same_live_world_endpoints_and_pen(){
        let s=source();let c=planned(&s).unwrap();let g=&c["guide"];assert_eq!(g.transform,[0.6,0.8,-0.8,0.6,35.,95.]);assert_eq!(g.geometry,Geometry::Path{d:"M0 0L120 0".into(),fill_rule:Default::default()});assert_eq!(g.style,s.objects[0].style);
        for version in[14,21]{let encode=if version==14{crate::cdr::encode_document}else{crate::cdr::encode_document21};let raw=encode(&s).unwrap().0;assert_eq!(crate::cdr::decode(&raw,&s.id).unwrap().0,s);let mut n=read_native(&raw).unwrap();n.members.remove("metadata/metadata.xml");n.members.remove("META-INF/metadata.xml");let b=crate::cdr::decode(&pack(&mut n),"bare").unwrap().0;assert_eq!(b.objects.len(),2);same(&s,&b,"guide");same(&s,&b,"text");let g=b.object("guide").unwrap();let[p,q]=straight(g).unwrap().unwrap();assert_eq!(p.y,q.y);assert_eq!(g.style.stroke_width_mm,0.2);}
    }
    #[test]
    fn actual_one_unit_guide_edit_is_not_replaced_by_source_endpoints(){
        for version in[14,21]{for axis in[0,1]{let s=source();let encode=if version==14{crate::cdr::encode_document}else{crate::cdr::encode_document21};let mut n=read_native(&encode(&s).unwrap().0).unwrap();
            fn edit(nodes:&mut[Node],axis:usize)->bool{for n in nodes{if n.kind()==Some(b"obj "){let(kind,mut args)=super::super::write::arguments(&n.find(b"loda").unwrap().data).unwrap();if args.iter().any(|(k,v)|*k==1000&&read_wide(v).unwrap()=="guide"){let raw=&mut args.iter_mut().find(|(k,_)|*k==30).unwrap().1;let at=12+4*axis;let old=i32_at(raw,at).unwrap();set32(raw,at,(old+1)as u32).unwrap();n.find_mut(b"loda").unwrap().data=super::super::write::loda(kind,&args);return true;}}if let Some(c)=&mut n.children{if edit(c,axis){return true;}}}false}
            assert!(edit(&mut n.nodes,axis));let actual=crate::cdr::decode(&pack(&mut n),"edited").unwrap().0;n.members.remove("metadata/metadata.xml");n.members.remove("META-INF/metadata.xml");let bare=crate::cdr::decode(&pack(&mut n),"bare").unwrap().0;same(&actual,&bare,"guide");same(&actual,&bare,"text");assert_ne!(actual.object("guide").unwrap().geometry,s.objects[0].geometry);
            let again=crate::cdr::decode(&encode(&actual).unwrap().0,"again").unwrap().0;same(&actual,&again,"guide");same(&actual,&again,"text");
        }}
    }
    #[test]
    fn actual_native_guide_micro_affine_edits_survive_full_import_with_old_notes(){
        for version in[14,21]{for mode in 0..3{let s=source();let encode=if version==14{crate::cdr::encode_document}else{crate::cdr::encode_document21};let mut n=read_native(&encode(&s).unwrap().0).unwrap();
            fn edit(nodes:&mut[Node],mode:u8)->bool{for n in nodes{if n.kind()==Some(b"obj "){let(_,args)=super::super::write::arguments(&n.find(b"loda").unwrap().data).unwrap();if args.iter().any(|(k,v)|*k==1000&&read_wide(v).unwrap()=="guide"){let raw=&mut n.find_mut(b"trfd").unwrap().data;let start=u32_at(raw,u32_at(raw,8).unwrap()as usize).unwrap()as usize+16;let at=start+8*match mode{0=>0,1=>1,_=>2};let old=f64_at(raw,at).unwrap();let delta=if mode==2{0.001}else{1e-8};raw[at..at+8].copy_from_slice(&(old+delta).to_le_bytes());return true;}}if let Some(c)=&mut n.children{if edit(c,mode){return true;}}}false}
            assert!(edit(&mut n.nodes,mode));let actual=crate::cdr::decode(&pack(&mut n),"edited").unwrap().0;n.members.remove("metadata/metadata.xml");n.members.remove("META-INF/metadata.xml");let bare=crate::cdr::decode(&pack(&mut n),"bare").unwrap().0;
            let(a,b)=(actual.object("guide").unwrap(),bare.object("guide").unwrap());let(pa,pb)=(straight(a).unwrap().unwrap(),straight(b).unwrap().unwrap());for(p,q)in pa.into_iter().zip(pb){let(p,q)=(actual.world_transform(a).unwrap()*p,bare.world_transform(b).unwrap()*q);assert!((p.x-q.x).abs()<1e-10&&(p.y-q.y).abs()<1e-10,"native{version} edit{mode}");}
            same(&actual,&bare,"text");
        }}
    }
    #[test]
    fn guide_rechart_never_erases_real_affine_or_nonlinear_edits(){
        let s=source();let mut guide=planned(&s).unwrap().remove("guide").unwrap();let source=s.world_transform(&s.objects[0]).unwrap();
        for mode in 0..3{let mut g=guide.clone();match mode{0=>g.transform[0]+=1e-8,1=>g.transform[2]+=0.01,_=>g.geometry=Geometry::Path{d:"M0 0C30 0 60 0 120 0".into(),fill_rule:Default::default()}}assert!(restore(&g,Affine::new(g.transform),source).unwrap().is_none());}
        guide.transform[4]+=0.0000001;assert!(restore(&guide,Affine::new(guide.transform),source).unwrap().is_none(),"a sub-grid move must keep its actual native chart");
    }
    #[test]
    fn axis_guides_unreferenced_curves_fine_units_and_elliptical_pens_are_unchanged(){
        for mode in 0..6{let mut s=source();match mode{0=>s.objects.pop().map(|_|()).unwrap(),1=>s.objects[0].geometry=Geometry::Path{d:"M0 0L120 0".into(),fill_rule:Default::default()},2=>s.objects[0].geometry=Geometry::Path{d:"M0 0L72.1234567 96".into(),fill_rule:Default::default()},3=>s.objects[0].transform=[2.,0.,0.,1.,0.,0.],4=>s.objects[0].clip=Some(crate::Clip::Rect{rect_mm:[0.,0.,10.,10.]}),_=>s.objects[0].style.stroke_width_mm=0.1234567}assert!(planned(&s).unwrap().is_empty(),"mode{mode}");}
    }
}
