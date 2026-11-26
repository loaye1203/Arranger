//! Editable PowerClip point charts, without coordinate or residual metadata.
use super::{binary::coord,invalid,Result};
use crate::{Clip,Document,Geometry,Object};
use kurbo::{Affine,BezPath,PathSeg,Point,Shape};

pub(super) struct Frame {
    pub geometry:Vec<u8>,
    /// The actual common14 native affine record, including its paper origin.
    pub matrix:[f64;6],
    pub world_path:BezPath,
    pub units:u32,
}
fn controls(path:&BezPath)->Vec<[Point;4]> {
    path.segments().map(|s|match s {
        PathSeg::Line(l)=>[l.p0,l.p0+(l.p1-l.p0)/3.,l.p0+(l.p1-l.p0)*(2./3.),l.p1],
        PathSeg::Quad(q)=>{let c=q.raise();[c.p0,c.p1,c.p2,c.p3]},
        PathSeg::Cubic(c)=>[c.p0,c.p1,c.p2,c.p3],
    }).collect()
}
fn same(a:&BezPath,b:&BezPath,tolerance:f64)->bool {
    let(a,b)=(controls(a),controls(b));a.len()==b.len()&&a.into_iter().zip(b).all(|(a,b)|a.into_iter().zip(b).all(|(a,b)|(a.x-b.x).abs()<=tolerance&&(a.y-b.y).abs()<=tolerance))
}
fn bounds_ok(path:&BezPath,size:[f64;2])->bool {
    let b=path.bounding_box();[b.x0-size[0]/2.,b.x1-size[0]/2.,size[1]/2.-b.y0,size[1]/2.-b.y1].into_iter().all(|v|coord(v).is_ok())
}
fn frame(local:&BezPath,world:Affine,units:u32,size:[f64;2])->Result<Frame> {
    let scale=f64::from(units);let m=world*Affine::scale(1./scale);let[a,b,c,d,e,f]=m.as_coeffs();
    if m.as_coeffs().iter().any(|v|!v.is_finite())||m.determinant().abs()<1e-12{return Err(invalid("native clip frame has a singular or non-finite point chart"));}
    for v in [e,f,e-size[0]/2.,size[1]/2.-f]{coord(v)?;}
    let geometry=super::write::path_geometry(&(Affine::scale(scale)*local.clone()))?;
    // Preserve the exact legacy bytes, including positive zero off-diagonals.
    let matrix=if units==1&&world==Affine::IDENTITY{[1.,0.,-size[0]/2.*10000.,0.,1.,size[1]/2.*10000.]}
        else{[a,-c,e*10000.-size[0]*5000.,-b,d,size[1]*5000.-f*10000.]};
    // Match read::matrix and its page-origin arithmetic exactly. In particular,
    // hashing world*local would differ by floating-point ULPs for translations.
    let decoded=Affine::new([matrix[0],-matrix[3],-matrix[1],matrix[4],(matrix[2]+size[0]/2.*10000.)/10000.,(size[1]/2.*10000.-matrix[5])/10000.]);
    let world_path=decoded*super::read::curve(&geometry)?;
    if !bounds_ok(&world_path,size){return Err(invalid("native clip frame world bounds exceed signed coordinate range"));}
    Ok(Frame{geometry,matrix,world_path,units})
}
fn notation(d:&str)->Result<Option<super::path_notation::Notation>> {
    for units in std::iter::once(1).chain(super::path_precision::UNITS){if let Ok(Some(n))=super::path_notation::Notation::from_source_in_units(d,units){return Ok(Some(n));}}
    Ok(None)
}
fn source_restores(clip:&Clip,world:Affine,projected:&BezPath)->Result<bool> {
    if let Clip::Path{d,..}=clip {if let Some(notation)=notation(d)? {
        return Ok(notation.restore(&(world.inverse()*projected.clone()).to_svg())?.as_deref()==Some(d));
    }}Ok(true)
}
/// Recover the source-local chart when normalization flattened a group or
/// introduced an image-fit wrapper. Match the actual world clip too: an inner
/// image viewport must never borrow the unrelated outer source clip's chart.
fn preferred<'a>(source:&'a Document,native:&Document,owner:&Object,world_path:&BezPath)->Result<Option<(&'a Clip,Affine)>> {
    let direct=source.objects.iter().find(|o|o.id==owner.id);
    let wrapped=native.objects.iter().filter(|o|o.parent_id.as_deref()==Some(&owner.id)).filter_map(|o|source.objects.iter().find(|old|old.id==o.id&&matches!(old.geometry,Geometry::Image{..})));
    for original in direct.into_iter().chain(wrapped) {if let Some(clip)=&original.clip {
        let world=source.world_transform(original)?;
        if clip.rule()==owner.clip.as_ref().unwrap().rule()&&same(&(world*clip.path()?),world_path,1e-9){return Ok(Some((clip,world)));}
    }}Ok(None)
}
/// Keep the existing world-grid bytes when they really recover the source;
/// otherwise choose the smallest safe native point chart. These frames have no
/// paint, so reciprocal scaling does not assume any outline-scaling flag.
pub(super) fn plan(source:&Document,native:&Document,owner:&Object)->Result<Frame> {
    let world_path=super::clipping::world_clip(native,owner)?.ok_or_else(||invalid("clip frame missing"))?;
    let size=super::pages::size(native,&owner.page_id)?;
    let selected=preferred(source,native,owner,&world_path)?;
    let (clip,world)=selected.unwrap_or((owner.clip.as_ref().unwrap(),native.world_transform(owner)?));
    let legacy=frame(&world_path,Affine::IDENTITY,1,size).ok();
    // Prefer a genuinely accurate native chart, including rectangular clips.
    // Rect scalars in Notes can otherwise hide a coarser active point table.
    // 1e-10 selects an effectively exact chart; the independent maximum safety
    // budget remains0.00021mm for conservative partial-interchange fallback.
    if let Some(legacy)=legacy.as_ref(){if same(&world_path,&legacy.world_path,1e-10)&&source_restores(clip,world,&legacy.world_path)? {
        return Ok(Frame{geometry:legacy.geometry.clone(),matrix:legacy.matrix,world_path:legacy.world_path.clone(),units:1});
    }}
    let local=clip.path()?;
    for units in std::iter::once(1).chain(super::path_precision::UNITS) {
        let Ok(candidate)=frame(&local,world,units,size)else{continue};
        if same(&world_path,&candidate.world_path,1e-10)&&source_restores(clip,world,&candidate.world_path)?{return Ok(candidate);}
    }
    // Partial interchange must retain its existing conservative fallback. The
    // source notation is not claimed as recovered if no exact chart exists.
    if let Some(legacy)=legacy {if same(&world_path,&legacy.world_path,0.00021){return Ok(legacy);}}
    Err(invalid("no bounded native clip point chart satisfies the world geometry budget"))
}
pub(super) fn projected_native_clip(source:&Document,native:&Document,owner:&Object,modern:bool)->Result<BezPath>{
    let frame=plan(source,native,owner)?;if !modern{return Ok(frame.world_path);}
    // Upgrade performs these exact origin shifts before the21 decoder adds its
    // snapped origin. Keep every ULP so even sub-point native edits invalidate
    // the source-parameter hash, rather than quantizing those edits away.
    static CONFIG:std::sync::OnceLock<Vec<u8>>=std::sync::OnceLock::new();
    if CONFIG.get().is_none(){
        let profile=super::binary::read_native(include_bytes!("../../../../reports/windows/WIN-CDR-SAVE-20261022-01/focused/native-preservation/identity-v21.cdr"))?;
        let config=super::binary::all(&profile.nodes).into_iter().find(|n|n.tag==*b"mcfg").ok_or_else(||invalid("native21 clip origin configuration absent"))?.data.clone();let _=CONFIG.set(config);
    }
    let size=super::pages::size(native,&owner.page_id)?;let origin=super::origin::paper(CONFIG.get().unwrap(),size,true)?;
    let delta=[coord(origin[0])? as f64-size[0]*5000.,coord(origin[1])? as f64-size[1]*5000.];let m=frame.matrix;
    let world=Affine::new([m[0],-m[3],-m[1],m[4],((m[2]-delta[0])+coord(origin[0])? as f64)/10000.,(coord(origin[1])? as f64-(m[5]+delta[1]))/10000.]);
    Ok(world*super::read::curve(&frame.geometry)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Rect;
    use serde_json::json;
    fn source(d:&str)->Document{serde_json::from_value(json!({"id":"clip-chart","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"shape","page_id":"page-1","x_mm":20.25,"y_mm":30.5,"width_mm":12.,"height_mm":8.,"geometry":{"kind":"rect","radius_mm":0.},"clip":{"kind":"path","d":d}}]})).unwrap()}
    #[test]
    fn seven_decimal_local_clip_points_are_active_coordinates(){
        let d="M0.1234567 0 C1.1234567 2.7654321 3.3333333 -1.1111111 5 0 L5 6 L0 6 Z";
        for matrix in [[1.,0.,0.,1.,0.,0.],[1.1,0.2,-0.125,0.9,0.125,-0.25]] {
            let mut source=source(d);source.objects[0].transform=matrix;let native=super::super::write::normalized(&source).unwrap();let plan=plan(&source,&native,&native.objects[0]).unwrap();
            assert_eq!(plan.units,1000);let points=super::super::read::curve(&plan.geometry).unwrap();let first=points.elements()[0];assert_eq!(first,kurbo::PathEl::MoveTo(Point::new(123.4567,0.)));
            assert!(same(&plan.world_path,&(source.world_transform(&source.objects[0]).unwrap()*crate::path::parse(d).unwrap()),1e-10));
            assert!(source_restores(source.objects[0].clip.as_ref().unwrap(),source.world_transform(&source.objects[0]).unwrap(),&plan.world_path).unwrap());
        }
    }
    #[test]
    fn existing_world_grid_clips_keep_identity_frames_and_bytes(){
        let source=source("M0 0H5V6H0Z");let native=super::super::write::normalized(&source).unwrap();let plan=plan(&source,&native,&native.objects[0]).unwrap();let world=super::super::clipping::world_clip(&native,&native.objects[0]).unwrap().unwrap();
        assert_eq!(plan.units,1);let legacy=[1f64,0.,-1050000.,0.,1.,1485000.];assert_eq!(plan.matrix.map(f64::to_bits),legacy.map(f64::to_bits));assert_eq!(plan.geometry,super::super::write::path_geometry(&world).unwrap());
    }
    #[test]
    fn seven_decimal_rectangles_improve_native_points_without_parameter_notes(){
        let mut source=source("M0 0H5V6H0Z");source.objects[0].clip=Some(Clip::Rect{rect_mm:[0.1234567,0.2345678,5.3456789,6.4567891]});source.objects[0].transform=[1.1,0.2,-0.125,0.9,0.125,-0.25];
        let native=super::super::write::normalized(&source).unwrap();let frame=plan(&source,&native,&native.objects[0]).unwrap();assert_eq!(frame.units,1000);
        assert!(same(&frame.world_path,&(source.world_transform(&source.objects[0]).unwrap()*source.objects[0].clip.as_ref().unwrap().path().unwrap()),1e-10));
    }
    fn pack(native:&mut super::super::binary::NativeRecords)->Vec<u8>{
        use super::super::binary::*;
        if native.version==2100{repack21(native).unwrap()}else{native.members.insert("content/riffData.cdr".into(),write(&native.nodes).unwrap());zip(&native.members.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>())}
    }
    #[test]
    fn removed_notes_keep_active_fine_clip_geometry_in_both_versions(){
        use super::super::binary::*;
        for rectangular in [false,true] {
            let mut source=source("M0.1234567 0 C1.1234567 2.7654321 3.3333333 -1.1111111 5 0 L5 6 L0 6 Z");source.objects[0].transform=[1.1,0.2,-0.125,0.9,0.1234567,-0.2345678];
            if rectangular{source.objects[0].clip=Some(Clip::Rect{rect_mm:[0.1234567,0.2345678,5.3456789,6.4567891]});}
            let expected=source.world_transform(&source.objects[0]).unwrap()*source.objects[0].clip.as_ref().unwrap().path().unwrap();let normalized=super::super::write::normalized(&source).unwrap();
            for version in [14,21] {
                let bytes=if version==14{crate::cdr::encode_document(&source)}else{crate::cdr::encode_document21(&source)}.unwrap().0;let mut native=read_native(&bytes).unwrap();
                native.members.remove("metadata/metadata.xml");native.members.remove("META-INF/metadata.xml");let doc=crate::cdr::decode(&pack(&mut native),"notes-removed").unwrap().0;let object=doc.object("shape").unwrap();
                let actual=doc.world_transform(object).unwrap()*object.clip.as_ref().unwrap().path().unwrap();assert!(same(&expected,&actual,1e-10),"native{version} rect{rectangular}");
                let projected=projected_native_clip(&source,&normalized,&normalized.objects[0],version==21).unwrap();assert!(same(&projected,&actual,1e-10));
            }
        }
    }
    #[test]
    fn genuine_native_fine_point_and_double_affine_edits_override_old_clip_parameters(){
        use super::super::binary::*;
        fn edit(nodes:&mut[Node],point:bool)->bool{for node in nodes {
            if node.kind()==Some(b"obj ") {
                let(kind,mut args)=super::super::write::arguments(&node.find(b"loda").unwrap().data).unwrap();
                if kind==3&&args.iter().any(|(k,_)|*k==8005) {
                    if point {let data=&mut args.iter_mut().find(|(k,_)|*k==30).unwrap().1;let n=u16_at(data,0).unwrap() as usize;for i in 0..n{let at=4+8*i;let x=i32_at(data,at).unwrap();set32(data,at,(x+1) as u32).unwrap();}node.find_mut(b"loda").unwrap().data=super::super::write::loda(kind,&args);}
                    else{let data=&mut node.find_mut(b"trfd").unwrap().data;let at=u32_at(data,u32_at(data,8).unwrap() as usize).unwrap() as usize+16+2*8;let value=f64_at(data,at).unwrap()+0.001;data[at..at+8].copy_from_slice(&value.to_le_bytes());}return true;
                }
            }if let Some(children)=&mut node.children{if edit(children,point){return true;}}
        }false}
        let mut source=source("M0.1234567 0 C1.1234567 2.7654321 3.3333333 -1.1111111 5 0 L5 6 L0 6 Z");source.objects[0].transform=[1.1,0.2,-0.125,0.9,0.125,-0.25];
        for version in [14,21]{for point in [true,false]{
            let bytes=if version==14{crate::cdr::encode_document(&source)}else{crate::cdr::encode_document21(&source)}.unwrap().0;let mut native=read_native(&bytes).unwrap();assert!(edit(&mut native.nodes,point));let with_notes=pack(&mut native);
            native.members.remove("metadata/metadata.xml");native.members.remove("META-INF/metadata.xml");let reference=crate::cdr::decode(&pack(&mut native),"native-reference").unwrap().0;let(actual,warnings)=crate::cdr::decode(&with_notes,"edited").unwrap();
            let(a,b)=(actual.object("shape").unwrap(),reference.object("shape").unwrap());let(pa,pb)=(actual.world_transform(a).unwrap()*a.clip.as_ref().unwrap().path().unwrap(),reference.world_transform(b).unwrap()*b.clip.as_ref().unwrap().path().unwrap());
            assert!(same(&pa,&pb,1e-10),"native{version} point{point}");assert_ne!(a.clip,source.objects[0].clip);assert!(warnings.iter().any(|n|n.code=="CDR_GEOMETRY_PARAMETERS_NATIVE"));
        }}
    }
    #[test]
    fn unsafe_charts_cannot_bypass_native_determinant_or_world_bounds(){
        let p=Rect::new(0.,0.,1.,1.).to_path(1e-6);assert!(frame(&p,Affine::scale(0.0001),1000,[210.,297.]).is_err());assert!(frame(&p,Affine::translate((500000.,0.)),1,[210.,297.]).is_err());
        assert!(frame(&p,Affine::new([f64::INFINITY,0.,0.,1.,0.,0.]),1,[210.,297.]).is_err());
    }
}
