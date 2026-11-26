//! Adaptive native point units; never a coordinate/residual payload.
use super::{binary::coord,invalid,Result};
use crate::{Clip,Document,Geometry,Object};
use kurbo::{Affine,BezPath,PathEl,Point,Shape};

pub(super) const UNITS:[u32;10]=[10,30,100,300,1000,3000,10000,30000,100000,300000];
pub(super) fn valid_units(units:u32)->bool{units==1||UNITS.contains(&units)}
pub(super) fn eligible(_doc:&Document,o:&Object)->bool {
    // Native type34 (0x22, bit0x20) and21 scaleWithObject=1 scale active pens
    // coherently with the curve chart. New output still requires Corel proof.
    matches!(o.geometry,Geometry::Path{..}|Geometry::Ellipse|Geometry::Rect{..})
}
fn same_primitive(a:&Object,am:Affine,b:&Object,bm:Affine)->bool {
    let pad=|o:&Object|if o.style.stroke=="none"{0.}else{o.style.stroke_width_mm*o.style.stroke_miterlimit.max(1.)};let(pa,pb)=(pad(a),pad(b));
    let close=|p:Point,q:Point|[(-1.,-1.),(1.,-1.),(1.,1.),(-1.,1.)].into_iter().all(|(x,y)|{let p=if a.style.stroke_scale_with_object{am*Point::new(p.x+pa*x,p.y+pa*y)}else{am*p+kurbo::Vec2::new(pa*x,pa*y)};let q=if b.style.stroke_scale_with_object{bm*Point::new(q.x+pb*x,q.y+pb*y)}else{bm*q+kurbo::Vec2::new(pb*x,pb*y)};(p.x-q.x).abs()<=0.00021&&(p.y-q.y).abs()<=0.00021});
    if let(Geometry::Rect{radius_mm:ra},Geometry::Rect{radius_mm:rb})=(&a.geometry,&b.geometry){
        let centers=|o:&Object,r:f64|[(r,r),(o.width_mm-r,r),(o.width_mm-r,o.height_mm-r),(r,o.height_mm-r)];
        // Every circular quarter lies in its unit quadrant box. The affine
        // difference at its vertices bounds all points, without tessellation.
        for((ca,cb),[qx,qy])in centers(a,*ra).into_iter().zip(centers(b,*rb)).zip([[-1.,-1.],[1.,-1.],[1.,1.],[-1.,1.]]){
            for(u,v)in [(0.,0.),(qx,0.),(0.,qy),(qx,qy)]{if !close(Point::new(ca.0+ra*u,ca.1+ra*v),Point::new(cb.0+rb*u,cb.1+rb*v)){return false;}}
        }true
    }else{[(0.,0.),(1.,0.),(1.,1.),(0.,1.)].into_iter().all(|(x,y)|close(Point::new(a.width_mm*x,a.height_mm*y),Point::new(b.width_mm*x,b.height_mm*y)))}
}
pub(super) fn projected(d:&str,units:u32)->Result<BezPath> {
    if !valid_units(units){return Err(invalid("unsupported native path local-unit scale"));}
    let scaled=Affine::scale(f64::from(units))*crate::path::parse(d)?;
    let native=super::read::curve(&super::write::path_geometry(&scaled)?)?;
    Ok(Affine::scale(1./f64::from(units))*native)
}
fn controls(path:&BezPath)->Vec<Point>{path.elements().iter().flat_map(|e|match *e {
    PathEl::MoveTo(p)|PathEl::LineTo(p)=>vec![p],PathEl::QuadTo(c,p)=>vec![c,p],PathEl::CurveTo(a,b,p)=>vec![a,b,p],PathEl::ClosePath=>vec![],
}).collect()}
pub(super) fn same_world(a:&BezPath,am:Affine,b:&BezPath,bm:Affine)->bool {
    let(a,b)=(controls(a),controls(b));a.len()==b.len()&&a.into_iter().zip(b).all(|(a,b)|{let(a,b)=(am*a,bm*b);(a.x-b.x).abs()<=0.00021&&(a.y-b.y).abs()<=0.00021})
}
fn cubic_path(original:&BezPath)->BezPath {
    let mut cubic=BezPath::new();let(mut current,mut start)=(Point::ZERO,Point::ZERO);for element in original.elements(){match *element {
        PathEl::MoveTo(p)=>{cubic.move_to(p);current=p;start=p;},
        PathEl::LineTo(p)=>{cubic.line_to(p);current=p;},
        PathEl::CurveTo(a,b,p)=>{cubic.curve_to(a,b,p);current=p;},
        PathEl::QuadTo(c,p)=>{let raised=kurbo::QuadBez::new(current,c,p).raise();cubic.curve_to(raised.p1,raised.p2,p);current=p;},
        PathEl::ClosePath=>{if current!=start{cubic.line_to(start);}cubic.close_path();current=start;},
    }}cubic
}
fn scale_clip(clip:&Clip,scale:f64)->Result<Clip>{Ok(match clip {
    Clip::Rect{rect_mm}=>Clip::Rect{rect_mm:rect_mm.map(|v|v*scale)},
    Clip::Path{d,fill_rule}=>Clip::Path{d:(Affine::scale(scale)*crate::path::parse(d)?).to_svg(),fill_rule:*fill_rule},
})}
pub(super) fn plan(doc:&Document,o:&Object)->Result<Option<u32>> {
    let Geometry::Path{d,..}=&o.geometry else{return primitive_plan(doc,o)};
    let fine_pen=o.style.stroke!="none"&&coord(o.style.stroke_width_mm)? as f64/10000.!=o.style.stroke_width_mm;
    if !eligible(doc,o)||!fine_pen&&super::path_notation::Notation::from_source(d)?.is_some(){return Ok(None);}
    let world=doc.world_transform(o)?;let original=crate::path::parse(d)?;
    // Cubic controls are the active representation of quadratic segments too.
    // Use an unquantized cubic reference for the physical error comparison.
    let cubic=cubic_path(&original);
    if let Some(clip)=super::clipping::world_clip(doc,o)? {
        let raw=match super::write::path_geometry(&clip){Ok(v)=>v,Err(_)=>return Ok(None)};
        let native=super::read::curve(&raw)?;
        if !same_world(&cubic_path(&clip),Affine::IDENTITY,&native,Affine::IDENTITY){return Ok(None);}
    }
    let size=super::pages::size(doc,&o.page_id)?;
    let mut physical_fallback=None;let mut ordinary_fallback=false;
    for units in std::iter::once(1).chain(UNITS) {
        // The existing zero-width outline exchange has explicit source-field
        // limits. Do not introduce a newly scaled native hairline profile.
        if units!=1&&o.style.stroke!="none"&&o.style.stroke_width_mm==0.{continue;}
        let scale=f64::from(units);let m=world*Affine::scale(1./scale);let c=m.as_coeffs();
        if c.iter().any(|v|!v.is_finite())||m.determinant().abs()<1e-12{continue;}
        // Native translation/paper-relative caches keep their existing signed
        // coordinate limits even though the affine record itself uses doubles.
        if [c[4],c[5],c[4]-size[0]/2.,size[1]/2.-c[5]].into_iter().any(|v|coord(v).is_err()){continue;}
        let logical=match projected(d,units){Ok(v)=>v,Err(_)=>continue};
        let bounds=(world*logical.clone()).bounding_box();
        if [bounds.x0-size[0]/2.,bounds.x1-size[0]/2.,size[1]/2.-bounds.y0,size[1]/2.-bounds.y1].into_iter().any(|v|coord(v).is_err()){continue;}
        if !same_world(&cubic,world,&logical,world){continue;}
        let mut pen_error=[0.;2];if o.style.stroke!="none" {
            let pen_scale=if o.style.stroke_scale_with_object{scale}else{1.};let Ok(raw)=coord(o.style.stroke_width_mm*pen_scale)else{continue};if raw==0&&o.style.stroke_width_mm>0.{continue;}
            let delta=(raw as f64/10000./pen_scale-o.style.stroke_width_mm).abs()*o.style.stroke_miterlimit.max(1.);
            let c=world.as_coeffs();pen_error=if o.style.stroke_scale_with_object{[delta*(c[0].abs()+c[2].abs()),delta*(c[1].abs()+c[3].abs())]}else{[delta,delta]};
        }
        if !controls(&cubic).into_iter().zip(controls(&logical)).all(|(a,b)|{let(a,b)=(world*a,world*b);(a.x-b.x).abs()+pen_error[0]<=0.00021&&(a.y-b.y).abs()+pen_error[1]<=0.00021}){continue;}
        // A derived affine/nib chart may have long float spellings that our
        // coordinate-free notation cannot reproduce exactly. That is not a
        // reason to drop its physical precision. The native point table and
        // this scalar carry the chart; optional source spelling stays separate.
        if units==1{ordinary_fallback=true;}else{physical_fallback.get_or_insert(units);}
        if !(units==1&&fine_pen)&&super::path_notation::Notation::from_source_in_units(d,units)?.is_some(){return Ok((units!=1).then_some(units));}
    }
    match physical_fallback.or(ordinary_fallback.then_some(1)) {
        Some(units)=>Ok((units!=1).then_some(units)),
        None=>Err(super::unsupported(format!("{}: no native curve chart within the joint geometry/pen physical bound",o.id))),
    }
}
fn primitive_plan(doc:&Document,o:&Object)->Result<Option<u32>> {
    if !eligible(doc,o)||o.width_mm<=0.||o.height_mm<=0.{return Ok(None);}
    let world=doc.world_transform(o)?;let size=super::pages::size(doc,&o.page_id)?;
    let outlined=o.style.stroke!="none";
    let fine_pen=outlined&&coord(o.style.stroke_width_mm)? as f64/10000.!=o.style.stroke_width_mm;
    let project=|scale:f64|->Result<Object>{let mut p=o.clone();p.width_mm=coord(o.width_mm*scale)? as f64/10000./scale;p.height_mm=coord(o.height_mm*scale)? as f64/10000./scale;if let Geometry::Rect{radius_mm}=o.geometry{p.geometry=Geometry::Rect{radius_mm:coord(radius_mm*scale)? as f64/10000./scale};}if outlined{let pen_scale=if o.style.stroke_scale_with_object{scale}else{1.};p.style.stroke_width_mm=coord(o.style.stroke_width_mm*pen_scale)? as f64/10000./pen_scale;}Ok(p)};
    let ordinary=project(1.)?;
    // Independent ties-to-even rounding can put a maximal corner radius
    // outside an odd native dimension. It is not a usable ordinary chart,
    // even when its displacement happens to be below the physical budget.
    let ordinary_valid=ordinary.width_mm>0.&&ordinary.height_mm>0.&& !matches!(ordinary.geometry,Geometry::Rect{radius_mm} if radius_mm>ordinary.width_mm.min(ordinary.height_mm)/2.);
    let ordinary_fits=ordinary_valid&&same_primitive(o,world,&ordinary,world);
    if (!fine_pen||!o.style.stroke_scale_with_object)&&ordinary_fits{return Ok(None);}
    for units in UNITS {let scale=f64::from(units);let matrix=world*Affine::scale(1./scale);let c=matrix.as_coeffs();if c.iter().any(|v|!v.is_finite())||matrix.determinant().abs()<1e-12{continue;}
        if [c[4],c[5],c[4]-size[0]/2.,size[1]/2.-c[5]].into_iter().any(|v|coord(v).is_err()){continue;}
        let Ok(projected)=project(scale)else{continue};if projected.width_mm<=0.||projected.height_mm<=0.||matches!(projected.geometry,Geometry::Rect{radius_mm} if radius_mm>projected.width_mm.min(projected.height_mm)/2.){continue;}
        if !same_primitive(o,world,&projected,world){continue;}
        if outlined&&o.style.stroke_scale_with_object{let Ok(pen)=coord(o.style.stroke_width_mm*scale)else{continue};if pen<=0{continue;}let delta=(pen as f64/10000./scale-o.style.stroke_width_mm).abs()*o.style.stroke_miterlimit.max(1.);let m=world.as_coeffs();if delta*(m[0].abs()+m[2].abs()).max(m[1].abs()+m[3].abs())>0.00021{continue;}}
        return Ok(Some(units));
    }
    if ordinary_fits&&(!outlined||coord(o.style.stroke_width_mm)?>0){return Ok(None);}
    Err(super::unsupported(format!("{}: no valid native primitive chart within the joint geometry/pen physical bound",o.id)))
}
pub(super) fn lower(source:&Document,native:&mut Document)->Result<usize> {
    let mut changed=0;
    for original in &source.objects {let Some(units)=plan(source,original)?else{continue};
        // Groups never change basis here; read the normalized world matrix
        // before borrowing the member, without cloning resource containers.
        let native_world=native.world_transform(native.object(&original.id)?)?;
        let object=native.objects.iter_mut().find(|o|o.id==original.id).ok_or_else(||invalid("native path unit target missing"))?;
        if let Geometry::Path{d,fill_rule}=&original.geometry{object.geometry=Geometry::Path{d:(Affine::scale(f64::from(units))*crate::path::parse(d)?).to_svg(),fill_rule:*fill_rule};}
        else{object.width_mm*=f64::from(units);object.height_mm*=f64::from(units);if let Geometry::Rect{radius_mm}=&mut object.geometry{*radius_mm*=f64::from(units);}}
        object.clip=original.clip.as_ref().map(|clip|scale_clip(clip,f64::from(units))).transpose()?;
        object.transform=(native_world*Affine::scale(1./f64::from(units))).as_coeffs();object.x_mm=0.;object.y_mm=0.;if object.style.stroke!="none"&&object.style.stroke_scale_with_object{object.style.stroke_width_mm*=f64::from(units);}changed+=1;
    }Ok(changed)
}
#[cfg(test)]
pub(super) fn rebase(doc:&mut Document,id:&str,units:u32)->Result<bool> {rebase_with_outline(doc,id,units,None,false)}
pub(super) fn rebase_with_outline(doc:&mut Document,id:&str,units:u32,width:Option<f64>,scales:bool)->Result<bool> {
    if !valid_units(units)||units==1{return Err(invalid("invalid native path unit scalar"));}
    let Some(index)=doc.objects.iter().position(|o|o.id==id)else{return Ok(false)};
    let object=&doc.objects[index];if !eligible(doc,object){return Ok(false);}
    let outlined=object.style.stroke!="none";if outlined&&object.style.stroke_scale_with_object&&!scales{return Ok(false);}
    let pen_units=if object.style.stroke_scale_with_object{f64::from(units)}else{1.};let mut outline=object.style.stroke_width_mm/pen_units;
    if outlined {if let Some(width)=width {if coord(width*pen_units)? as f64/10000.==object.style.stroke_width_mm{outline=width;}}}
    let original=object.local_path()?;let logical=Affine::scale(1./f64::from(units))*original.clone();let actual=doc.world_transform(object)?;let recovered=actual*Affine::scale(f64::from(units));
    if outlined {let a=if object.style.stroke_scale_with_object{actual.as_coeffs()}else{Affine::IDENTITY.as_coeffs()};let b=if object.style.stroke_scale_with_object{recovered.as_coeffs()}else{Affine::IDENTITY.as_coeffs()};let pad=object.style.stroke_miterlimit.max(1.);for (x,y) in [(-1.,-1.),(1.,-1.),(1.,1.),(-1.,1.)]{let old=Point::new((a[0]*x+a[2]*y)*object.style.stroke_width_mm*pad,(a[1]*x+a[3]*y)*object.style.stroke_width_mm*pad);let new=Point::new((b[0]*x+b[2]*y)*outline*pad,(b[1]*x+b[3]*y)*outline*pad);if (old.x-new.x).abs()>0.00021||(old.y-new.y).abs()>0.00021{return Ok(false);}}}
    if !same_world(&original,actual,&logical,recovered){return Ok(false);}
    let clip=object.clip.as_ref().map(|clip|scale_clip(clip,1./f64::from(units))).transpose()?;
    if let (Some(old),Some(new))=(&object.clip,&clip){if !same_world(&old.path()?,actual,&new.path()?,recovered){return Ok(false);}}
    let parent=object.parent_id.as_ref().map(|id|doc.world_transform(doc.object(id)?)).transpose()?.unwrap_or(Affine::IDENTITY);
    let own=Affine::translate((object.x_mm,object.y_mm));let transformed=(parent.inverse()*recovered*own.inverse()).as_coeffs();
    if transformed.iter().any(|v|!v.is_finite()){return Err(invalid("non-finite rebased path frame"));}
    let geometry=match &object.geometry{Geometry::Path{fill_rule,..}=>Geometry::Path{d:logical.to_svg(),fill_rule:*fill_rule},Geometry::Rect{radius_mm}=>Geometry::Rect{radius_mm:radius_mm/f64::from(units)},other=>other.clone()};let primitive=!matches!(geometry,Geometry::Path{..});doc.objects[index].transform=transformed;doc.objects[index].geometry=geometry;if primitive{doc.objects[index].width_mm/=f64::from(units);doc.objects[index].height_mm/=f64::from(units);}doc.objects[index].clip=clip;if outlined{doc.objects[index].style.stroke_width_mm=outline;}doc.validate()?;Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::binary::*;
    use serde_json::json;
    const FINE:&str="M0.1234567 0 C1.1234567 2.7654321 3.3333333 -1.1111111 5 0";
    fn source()->Document{serde_json::from_value(json!({"id":"native-units","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"path","page_id":"page-1","x_mm":20.25,"y_mm":30.5,"width_mm":12.,"height_mm":8.,"geometry":{"kind":"path","d":FINE}}]})).unwrap()}
    #[test]
    fn primitive_units_cannot_spend_separate_geometry_and_pen_budgets(){
        let mut doc=source();let o=&mut doc.objects[0];o.geometry=Geometry::Rect{radius_mm:0.125};o.width_mm=12.1234551;o.style.stroke="#000000".into();o.style.stroke_width_mm=0.123451;o.transform=[30.,0.,0.,1.1e-10/30.,0.,0.];
        // Units10 would move the right curve by .000147mm and an opposing
        // miter edge by another .000120mm. Neither may spend a fresh budget.
        // Larger charts exceed the existing determinant bound.
        assert_eq!(plan(&doc,&doc.objects[0]).unwrap_err().code,"CDR_UNSUPPORTED");
    }
    fn primitive_source(reflected:bool)->Document{
        let mut doc=source();let o=&mut doc.objects[0];o.geometry=Geometry::Rect{radius_mm:0.1234567};o.width_mm=12.1234567;o.height_mm=8.2345678;o.style.stroke="#000000".into();o.style.stroke_width_mm=0.1234567;o.transform=if reflected{[-1.25,0.25,0.125,0.875,50.,60.]}else{[2.,0.25,0.125,1.5,1.25,2.5]};doc
    }
    #[test]
    fn primitive_curve_and_pen_exist_without_any_arranger_notes(){
        for reflected in [false,true]{let source=primitive_source(reflected);let expected=super::super::write::normalized(&source).unwrap();for version in [14,21]{
            let bytes=if version==14{crate::cdr::encode_document(&source)}else{crate::cdr::encode_document21(&source)}.unwrap().0;let mut native=read_native(&bytes).unwrap();native.members.remove("metadata/metadata.xml");native.members.remove("META-INF/metadata.xml");let actual=crate::cdr::decode(&pack(&mut native),"actual").unwrap().0;
            let(a,b)=(expected.object("path").unwrap(),actual.object("path").unwrap());assert!(same_primitive(a,expected.world_transform(a).unwrap(),b,actual.world_transform(b).unwrap()));assert_ne!(a.width_mm,b.width_mm,"actual native chart is scaled");
        }}
    }
    #[test]
    fn ordinary_grid_primitives_and_paths_also_carry_real_scaling_pens(){
        for geometry in [Geometry::Rect{radius_mm:0.125},Geometry::Ellipse,Geometry::Path{d:"M0 0 L12 0 L12 8 Z".into(),fill_rule:Default::default()}]{
            let mut source=primitive_source(false);source.objects[0].geometry=geometry;source.objects[0].width_mm=12.;source.objects[0].height_mm=8.;source.objects[0].style.stroke_width_mm=0.25;assert_eq!(plan(&source,&source.objects[0]).unwrap(),None);
            for version in [14,21]{let encoded=if version==14{crate::cdr::encode_document(&source)}else{crate::cdr::encode_document21(&source)};let bytes=encoded.unwrap().0;let native=read_native(&bytes).unwrap();let outline=all(&native.nodes).into_iter().find(|n|n.tag==*b"outl"&&super::super::read::outline(&n.data).unwrap().0!="none").unwrap();let at=super::super::read::outline_body(&outline.data).unwrap();assert_eq!(u16_at(&outline.data,at).unwrap(),34);
                if version==21{let obj=all(&native.nodes).into_iter().find(|n|n.kind()==Some(b"obj ")).unwrap();let(_,args)=super::super::write::arguments(&obj.find(b"loda").unwrap().data).unwrap();let raw=&args.iter().find(|(k,_)|*k==201).unwrap().1;assert_eq!(super::super::modern::style_record(raw).unwrap()["outline"]["scaleWithObject"],"1");}
            }
        }
    }
    #[test]
    fn actual_primitive_radius_dimension_and_pen_edits_are_never_replaced_by_old_notes(){
        let source=primitive_source(false);
        for version in [14,21]{for mode in [0,1,2]{for delta in [1,10000]{
            let bytes=if version==14{crate::cdr::encode_document(&source)}else{crate::cdr::encode_document21(&source)}.unwrap().0;let mut native=read_native(&bytes).unwrap();
            fn edit(nodes:&mut[Node],version:u32,mode:u32,delta:i32){for n in nodes{
                if mode==2&&n.tag==*b"outl"&&super::super::read::outline(&n.data).unwrap().0!="none"{let at=super::super::read::outline_body(&n.data).unwrap()+6;let old=i32_at(&n.data,at).unwrap();set32(&mut n.data,at,(old+delta) as u32).unwrap();}
                if n.kind()==Some(b"obj "){let(kind,mut args)=super::super::write::arguments(&n.find(b"loda").unwrap().data).unwrap();if kind==1{
                    if mode!=2{let data=&mut args.iter_mut().find(|(k,_)|*k==30).unwrap().1;if version==14{for at in if mode==0{vec![8,12,16,20]}else{vec![0]}{let old=i32_at(data,at).unwrap();set32(data,at,(old+delta) as u32).unwrap();}}else{for at in if mode==0{vec![40,64,88,112]}else{vec![0]}{let old=f64_at(data,at).unwrap();let step=if mode==0&&u32_at(data,32).unwrap()==0{f64::from(delta)/(f64_at(data,0).unwrap().abs().min(f64_at(data,8).unwrap().abs())*f64_at(data,16).unwrap()/2.)}else{f64::from(delta)};data[at..at+8].copy_from_slice(&(old+step).to_le_bytes());}}}
                    else if let Some((_,raw))=args.iter_mut().find(|(k,_)|*k==201){let mut style=super::super::modern::style_record(raw).unwrap();let old=style["outline"]["width"].as_str().unwrap().parse::<i32>().unwrap();style["outline"]["width"]=json!((old+delta).to_string());*raw=super::super::modern::encoded_style(&style).unwrap();}
                    n.find_mut(b"loda").unwrap().data=super::super::write::loda(kind,&args);
                }}if let Some(c)=&mut n.children{edit(c,version,mode,delta);}
            }}edit(&mut native.nodes,version,mode,delta);
            let with_notes=pack(&mut native);native.members.remove("metadata/metadata.xml");native.members.remove("META-INF/metadata.xml");let reference=crate::cdr::decode(&pack(&mut native),"reference").unwrap().0;let actual=crate::cdr::decode(&with_notes,"actual").unwrap().0;
            let(a,b)=(actual.object("path").unwrap(),reference.object("path").unwrap());assert!(same_primitive(a,actual.world_transform(a).unwrap(),b,reference.world_transform(b).unwrap()),"native{version} mode{mode} delta{delta}");
            if mode==0{assert_ne!(a.geometry,source.objects[0].geometry);}if mode==1{assert_ne!(a.width_mm,source.objects[0].width_mm);}if mode==2{assert_ne!(a.style.stroke_width_mm,source.objects[0].style.stroke_width_mm);}
        }}}
    }
    fn native(version:u32)->NativeRecords{let doc=source();read_native(&if version==14{crate::cdr::encode_document(&doc)}else{crate::cdr::encode_document21(&doc)}.unwrap().0).unwrap()}
    fn pack(native:&mut NativeRecords)->Vec<u8>{if native.version==2100{repack21(native).unwrap()}else{native.members.insert("content/riffData.cdr".into(),write(&native.nodes).unwrap());zip(&native.members.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>())}}
    fn edit(nodes:&mut [Node],point:bool)->bool {
        for node in nodes {
            if node.kind()==Some(b"obj ") {
                let (kind,mut args)=super::super::write::arguments(&node.find(b"loda").unwrap().data).unwrap();
                if kind==3 {
                    if point {let geometry=&mut args.iter_mut().find(|(key,_)|*key==30).unwrap().1;let x=i32_at(geometry,12).unwrap();set32(geometry,12,(x+1) as u32).unwrap();node.find_mut(b"loda").unwrap().data=super::super::write::loda(kind,&args);}
                    else{let data=&mut node.find_mut(b"trfd").unwrap().data;let at=u32_at(data,u32_at(data,8).unwrap() as usize).unwrap() as usize+16+2*8;let value=f64_at(data,at).unwrap()+25000.;data[at..at+8].copy_from_slice(&value.to_le_bytes());}
                    return true;
                }
            }
            if let Some(children)=&mut node.children{if edit(children,point){return true;}}
        }false
    }
    fn same_points(doca:&Document,docb:&Document){let (ao,bo)=(&doca.objects[0],&docb.objects[0]);let(ap,bp)=(controls(&ao.local_path().unwrap()),controls(&bo.local_path().unwrap()));assert_eq!(ap.len(),bp.len());for(a,b)in ap.into_iter().zip(bp){let(a,b)=(doca.world_transform(ao).unwrap()*a,docb.world_transform(bo).unwrap()*b);assert!((a.x-b.x).abs()<1e-10&&(a.y-b.y).abs()<1e-10);}}
    #[test]
    fn adaptive_points_are_genuine_active_coordinates_even_when_notes_are_removed() {
        let expected=source();assert_eq!(plan(&expected,&expected.objects[0]).unwrap(),Some(1000));
        for version in [14,21] {
            let mut native=native(version);let object=all(&native.nodes).into_iter().find(|n|n.kind()==Some(b"obj ")).unwrap();let (_,args)=super::super::write::arguments(&object.find(b"loda").unwrap().data).unwrap();let geometry=&args.iter().find(|(key,_)|*key==30).unwrap().1;assert_eq!(i32_at(geometry,4).unwrap(),1234567);
            native.members.remove("metadata/metadata.xml");native.members.remove("META-INF/metadata.xml");let raw=pack(&mut native);let actual=crate::cdr::decode(&raw,"actual").unwrap().0;same_points(&expected,&actual);
        }
    }
    #[test]
    fn actual_native_point_and_affine_edits_survive_the_unit_rebase() {
        let source=source();
        for version in [14,21] {for point in [true,false] {
            let mut native=native(version);assert!(edit(&mut native.nodes,point));let raw=pack(&mut native);let actual=crate::cdr::decode(&raw,&source.id).unwrap().0;
            let mut expected=source.clone();if point {expected.objects[0].geometry=Geometry::Path{d:FINE.replace("C1.1234567","C1.1234568"),fill_rule:Default::default()};assert_ne!(actual.objects[0].geometry,source.objects[0].geometry);}else{expected.objects[0].transform[4]=2.5;assert_eq!(actual.objects[0].geometry,source.objects[0].geometry);}
            same_points(&expected,&actual);
        }}
    }
    #[test]
    fn scaling_pen_has_real_native_width_and_affine_covariance_without_notes() {
        let mut expected=source();expected.objects[0].style.stroke="#000000".into();expected.objects[0].style.stroke_width_mm=0.1234567;expected.objects[0].transform=[2.,0.25,0.125,1.5,0.,0.];
        let units=plan(&expected,&expected.objects[0]).unwrap().unwrap();
        for version in [14,21] {
            let bytes=if version==14{crate::cdr::encode_document(&expected)}else{crate::cdr::encode_document21(&expected)}.unwrap().0;let mut native=read_native(&bytes).unwrap();
            let pen=all(&native.nodes).into_iter().find(|n|n.tag==*b"outl"&&super::super::read::outline(&n.data).unwrap().0!="none").unwrap();let at=super::super::read::outline_body(&pen.data).unwrap();assert_eq!(u16_at(&pen.data,at).unwrap(),34);assert_eq!(i32_at(&pen.data,at+6).unwrap(),coord(0.1234567*f64::from(units)).unwrap());
            native.members.remove("metadata/metadata.xml");native.members.remove("META-INF/metadata.xml");let actual=crate::cdr::decode(&pack(&mut native),"bare-native").unwrap().0;
            same_points(&expected,&actual);
            let(a,b)=(&expected.objects[0],&actual.objects[0]);let(am,bm)=(expected.world_transform(a).unwrap().as_coeffs(),actual.world_transform(b).unwrap().as_coeffs());
            for i in 0..4 {assert!((am[i]*a.style.stroke_width_mm-bm[i]*b.style.stroke_width_mm).abs()<=0.00021,"native{version} pen basis{i}");}
        }
    }
    #[test]
    fn removing_native_pen_scaling_keeps_actual_width_and_matrix_instead_of_old_units() {
        let mut expected=source();expected.objects[0].style.stroke="#000000".into();expected.objects[0].style.stroke_width_mm=0.1234567;
        for version in [14,21] {
            let bytes=if version==14{crate::cdr::encode_document(&expected)}else{crate::cdr::encode_document21(&expected)}.unwrap().0;let mut native=read_native(&bytes).unwrap();
            fn unscale(nodes:&mut[Node]){for n in nodes{
                if n.tag==*b"outl" {let at=super::super::read::outline_body(&n.data).unwrap();if u16_at(&n.data,at).unwrap()==34{n.data[at..at+2].copy_from_slice(&2u16.to_le_bytes());}}
                if n.kind()==Some(b"obj "){let(kind,mut args)=super::super::write::arguments(&n.find(b"loda").unwrap().data).unwrap();if let Some((_,raw))=args.iter_mut().find(|(k,_)|*k==201){let mut style=super::super::modern::style_record(raw).unwrap();style["outline"]["scaleWithObject"]=serde_json::json!("0");*raw=super::super::modern::encoded_style(&style).unwrap();n.find_mut(b"loda").unwrap().data=super::super::write::loda(kind,&args);}}
                if let Some(c)=&mut n.children{unscale(c);}
            }}unscale(&mut native.nodes);
            let with_notes=pack(&mut native);native.members.remove("metadata/metadata.xml");native.members.remove("META-INF/metadata.xml");let without_notes=pack(&mut native);
            let actual=crate::cdr::decode(&with_notes,"edited").unwrap().0;let reference=crate::cdr::decode(&without_notes,"reference").unwrap().0;same_points(&actual,&reference);
            assert_eq!(actual.objects[0].style.stroke_width_mm,reference.objects[0].style.stroke_width_mm);assert!(!actual.objects[0].style.stroke_scale_with_object);assert!(!reference.objects[0].style.stroke_scale_with_object);
        }
    }
    #[test]
    fn units_are_only_a_scalar_and_native_clips_and_parents_rebase_without_changing_world_content() {
        let source=source();let units=plan(&source,&source.objects[0]).unwrap().unwrap();
        let mut lowered=source.clone();lower(&source,&mut lowered).unwrap();
        for mode in 0..4 {
            let mut changed=lowered.clone();match mode {
                0=>{changed.objects[0].style.stroke="#000000".into();changed.objects[0].style.stroke_width_mm=1.;},
                1=>changed.objects[0].clip=Some(crate::Clip::Rect{rect_mm:[0.,0.,1.,1.]}),
                2=>{let mut parent=source.objects[0].clone();parent.id="parent".into();parent.geometry=Geometry::Group;changed.objects[0].parent_id=Some("parent".into());changed.objects.push(parent);},
                _=>{let mut text=source.objects[0].clone();text.id="text".into();text.x_mm=0.;text.y_mm=0.;text.geometry=Geometry::Text{content:"Live".into(),font_family:"Microsoft YaHei".into(),font_size_mm:4.,text_path:Some(crate::TextOnPath{path_id:"path".into(),start_offset_mm:0.,baseline_offset_mm:0.,anchor:crate::TextAnchor::Start}),layout:Default::default()};changed.objects.push(text);},
            }
            let expected=changed.clone();
            if mode==0 {assert!(!rebase(&mut changed,"path",units).unwrap());assert_eq!(changed,expected);}
            else {
                assert!(rebase(&mut changed,"path",units).unwrap());same_points(&changed,&expected);
                if let(Some(a),Some(b))=(&changed.objects[0].clip,&expected.objects[0].clip){assert!(same_world(&a.path().unwrap(),changed.world_transform(&changed.objects[0]).unwrap(),&b.path().unwrap(),expected.world_transform(&expected.objects[0]).unwrap()));}
            }
        }
        let notes=super::super::geometry_params::notes(&source).unwrap().unwrap();let value:serde_json::Value=super::super::metadata::decode("ArrangerGeometryParametersV1|sha256:",&notes).unwrap();let p=&value["objects"]["path"];assert_eq!(p["path_units"],1000);assert_eq!(p["path"]["units"],1000);assert!(!p.to_string().contains("0.1234567"));assert!(!p["path"]["syntax"].as_str().unwrap().chars().any(|ch|ch.is_ascii_digit()));
        assert!(rebase(&mut lowered,"path",0).is_err());assert!(rebase(&mut lowered,"path",2).is_err());
    }
    #[test]
    fn native_added_fixed_outline_can_rebase_geometry_without_rescaling_the_pen() {
        let mut source=source();let mut template=source.objects[0].clone();template.id="outline-template".into();template.geometry=Geometry::Rect{radius_mm:0.};template.style.stroke="#000000".into();template.style.stroke_width_mm=1.;source.objects.push(template);
        for version in [14,21] {
            let bytes=if version==14{crate::cdr::encode_document(&source)}else{crate::cdr::encode_document21(&source)}.unwrap().0;let mut native=read_native(&bytes).unwrap();
            let mut path_key=None;let mut outline_key=None;let mut modern_outline=None;
            for object in all(&native.nodes).into_iter().filter(|n|n.kind()==Some(b"obj ")) {
                let(kind,args)=super::super::write::arguments(&object.find(b"loda").unwrap().data).unwrap();let key=u32_at(&args.iter().find(|(k,_)|*k==10).unwrap().1,0).unwrap();
                if kind==3{path_key=Some(key);}else{outline_key=Some(key);modern_outline=args.iter().find(|(k,_)|*k==201).map(|(_,raw)|super::super::modern::style_record(raw).unwrap()["outline"].clone());}
            }
            let path_key=path_key.unwrap();let mut template=all(&native.nodes).into_iter().find(|n|n.tag==*b"outl"&&u32_at(&n.data,0).unwrap()==outline_key.unwrap()).unwrap().data.clone();let at=super::super::read::outline_body(&template).unwrap();template[at..at+2].copy_from_slice(&2u16.to_le_bytes());if let Some(outline)=&mut modern_outline{outline["scaleWithObject"]=json!("0");}
            fn add(nodes:&mut[Node],path_key:u32,template:&[u8],modern:&Option<serde_json::Value>){for node in nodes {
                if node.tag==*b"outl"&&u32_at(&node.data,0).unwrap()==path_key{node.data=template.to_vec();set32(&mut node.data,0,path_key).unwrap();}
                if node.kind()==Some(b"obj ") {let(kind,mut args)=super::super::write::arguments(&node.find(b"loda").unwrap().data).unwrap();if kind==3{if let(Some((_,raw)),Some(outline))=(args.iter_mut().find(|(k,_)|*k==201),modern){let mut style=super::super::modern::style_record(raw).unwrap();style["outline"]=outline.clone();*raw=super::super::modern::encoded_style(&style).unwrap();node.find_mut(b"loda").unwrap().data=super::super::write::loda(kind,&args);}}}
                if let Some(children)=&mut node.children{add(children,path_key,template,modern);}
            }}
            add(&mut native.nodes,path_key,&template,&modern_outline);let with_notes=pack(&mut native);
            native.members.remove("metadata/metadata.xml");native.members.remove("META-INF/metadata.xml");let without_notes=pack(&mut native);let reference=crate::cdr::decode(&without_notes,"reference").unwrap().0;
            let result=crate::cdr::decode(&with_notes,"edited");assert!(result.is_ok(),"native{version}: {result:?}");let(actual,warnings)=result.unwrap();
            let(a,b)=(actual.object("path").unwrap(),reference.object("path").unwrap());assert!(same_world(&a.local_path().unwrap(),actual.world_transform(a).unwrap(),&b.local_path().unwrap(),reference.world_transform(b).unwrap()));assert_eq!(a.style.stroke,b.style.stroke);assert_eq!(a.style.stroke_width_mm,1.);assert!(!a.style.stroke_scale_with_object);
            assert!(!warnings.iter().any(|w|w.code=="CDR_PATH_UNITS_NATIVE"));
            for(pa,pb)in controls(&a.local_path().unwrap()).into_iter().zip(controls(&b.local_path().unwrap())){let(pa,pb)=(actual.world_transform(a).unwrap()*pa,reference.world_transform(b).unwrap()*pb);assert!((pa.x-pb.x).abs()<1e-10&&(pa.y-pb.y).abs()<1e-10);}
            assert!(!a.style.stroke_scale_with_object&&!b.style.stroke_scale_with_object,"recharting must keep the fixed world pen");
        }
    }
    fn clipped_parent_source()->Document {
        let mut doc=source();doc.objects[0].transform=[1.1,0.2,0.1,0.9,0.5,-0.25];doc.objects[0].parent_id=Some("parent".into());doc.objects[0].clip=Some(Clip::Rect{rect_mm:[0.125,-1.75,4.5,3.]});
        let mut parent=doc.objects[0].clone();parent.id="parent".into();parent.parent_id=None;parent.x_mm=3.;parent.y_mm=4.;parent.transform=[0.9,0.1,-0.1,1.1,4.,5.];parent.geometry=Geometry::Group;parent.clip=Some(Clip::Rect{rect_mm:[0.,0.,60.,60.]});parent.style=Default::default();doc.objects.push(parent);doc
    }
    fn compare_named_points(a:&Document,b:&Document,id:&str,tolerance:f64) {
        let(oa,ob)=(a.object(id).unwrap(),b.object(id).unwrap());let(pa,pb)=(controls(&oa.local_path().unwrap()),controls(&ob.local_path().unwrap()));assert_eq!(pa.len(),pb.len());
        for(p,q)in pa.into_iter().zip(pb){let(p,q)=(a.world_transform(oa).unwrap()*p,b.world_transform(ob).unwrap()*q);assert!((p.x-q.x).abs()<=tolerance&&(p.y-q.y).abs()<=tolerance,"{id}: {p:?} vs {q:?}");}
    }
    fn compare_named_clip(a:&Document,b:&Document,id:&str,tolerance:f64) {
        let(oa,ob)=(a.object(id).unwrap(),b.object(id).unwrap());let(mut pa,mut pb)=(controls(&oa.clip.as_ref().unwrap().path().unwrap()),controls(&ob.clip.as_ref().unwrap().path().unwrap()));
        if pa.len()>1&&pa.first()==pa.last(){pa.pop();}if pb.len()>1&&pb.first()==pb.last(){pb.pop();}assert_eq!(pa.len(),pb.len());
        for(p,q)in pa.into_iter().zip(pb){let(p,q)=(a.world_transform(oa).unwrap()*p,b.world_transform(ob).unwrap()*q);assert!((p.x-q.x).abs()<=tolerance&&(p.y-q.y).abs()<=tolerance,"clip {id}: {p:?} vs {q:?}");}
    }
    fn shift_native_frames(nodes:&mut[Node],delta:i32)->usize {
        let mut count=0;for node in nodes {
            if node.kind()==Some(b"obj ") {let(kind,mut args)=super::super::write::arguments(&node.find(b"loda").unwrap().data).unwrap();let name=read_wide(&args.iter().find(|(k,_)|*k==1000).unwrap().1).unwrap();
                if kind==3&&name.starts_with(super::super::clipping::PREFIX){let data=&mut args.iter_mut().find(|(k,_)|*k==30).unwrap().1;let n=u16_at(data,0).unwrap() as usize;for i in 0..n{let at=4+8*i;let x=i32_at(data,at).unwrap();set32(data,at,(x+delta) as u32).unwrap();}node.find_mut(b"loda").unwrap().data=super::super::write::loda(kind,&args);count+=1;}
            }
            if let Some(children)=&mut node.children{count+=shift_native_frames(children,delta);}
        }count
    }
    fn shift_native_frame_affines(nodes:&mut[Node],delta_mm:f64)->usize {
        let mut count=0;for node in nodes {
            if node.kind()==Some(b"obj ") {let(kind,args)=super::super::write::arguments(&node.find(b"loda").unwrap().data).unwrap();let name=read_wide(&args.iter().find(|(k,_)|*k==1000).unwrap().1).unwrap();
                if kind==3&&name.starts_with(super::super::clipping::PREFIX){let data=&mut node.find_mut(b"trfd").unwrap().data;let at=u32_at(data,u32_at(data,8).unwrap() as usize).unwrap() as usize+16+2*8;let value=f64_at(data,at).unwrap()+delta_mm*10000.;data[at..at+8].copy_from_slice(&value.to_le_bytes());count+=1;}
            }
            if let Some(children)=&mut node.children{count+=shift_native_frame_affines(children,delta_mm);}
        }count
    }
    #[test]
    fn native_clip_frame_double_micro_moves_are_not_quantized_away_by_projection_guards() {
        let source=clipped_parent_source();for version in [14,21] {
            let bytes=if version==14{crate::cdr::encode_document(&source)}else{crate::cdr::encode_document21(&source)}.unwrap().0;let mut native=read_native(&bytes).unwrap();assert_eq!(shift_native_frame_affines(&mut native.nodes,0.0000001),2);let with_notes=pack(&mut native);
            native.members.remove("metadata/metadata.xml");native.members.remove("META-INF/metadata.xml");let reference=crate::cdr::decode(&pack(&mut native),"reference").unwrap().0;let(actual,warnings)=crate::cdr::decode(&with_notes,"actual").unwrap();
            compare_named_clip(&actual,&reference,"path",1e-10);compare_named_clip(&actual,&reference,"parent",1e-10);
            assert!(warnings.iter().any(|w|w.code=="CDR_GEOMETRY_PARAMETERS_NATIVE"));
            assert!(matches!(actual.object("path").unwrap().clip,Some(Clip::Path{..})));assert!(matches!(actual.object("parent").unwrap().clip,Some(Clip::Path{..})));
        }
    }
    #[test]
    fn parent_and_own_clips_remain_real_native_geometry_without_notes() {
        let expected=clipped_parent_source();for version in [14,21] {
            let bytes=if version==14{crate::cdr::encode_document(&expected)}else{crate::cdr::encode_document21(&expected)}.unwrap().0;let mut native=read_native(&bytes).unwrap();native.members.remove("metadata/metadata.xml");native.members.remove("META-INF/metadata.xml");let actual=crate::cdr::decode(&pack(&mut native),"native").unwrap().0;
            compare_named_points(&actual,&expected,"path",1e-10);compare_named_clip(&actual,&expected,"path",0.00021);compare_named_clip(&actual,&expected,"parent",0.00021);
        }
    }
    #[test]
    fn actual_native_clip_edits_are_not_replaced_by_old_source_rectangles() {
        let source=clipped_parent_source();for version in [14,21] {for delta in [1,2500] {
            let bytes=if version==14{crate::cdr::encode_document(&source)}else{crate::cdr::encode_document21(&source)}.unwrap().0;let mut native=read_native(&bytes).unwrap();assert_eq!(shift_native_frames(&mut native.nodes,delta),2);let with_notes=pack(&mut native);
            native.members.remove("metadata/metadata.xml");native.members.remove("META-INF/metadata.xml");let reference=crate::cdr::decode(&pack(&mut native),"reference").unwrap().0;let actual=crate::cdr::decode(&with_notes,"actual").unwrap().0;
            compare_named_points(&actual,&reference,"path",1e-10);compare_named_clip(&actual,&reference,"path",1e-10);compare_named_clip(&actual,&reference,"parent",1e-10);
        }}
    }
}
