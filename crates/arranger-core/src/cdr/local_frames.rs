//! Source coordinate charts only; native objects own geometry and hierarchy.
use super::{invalid,unsupported,Result};
use crate::{Document,Geometry,ImageFit,Clip,Style,LineCap,LineJoin,model::valid_id};
use kurbo::{Affine,BezPath,PathEl,Point};
use serde::{Serialize,Deserialize};
use std::collections::{BTreeMap,BTreeSet};
fn is_false(value:&bool)->bool{!*value}
const PREFIX:&str="ArrangerLocalFramesV1|sha256:";
pub(super) struct NativeNib {pub nominal:f64,pub raw:f64,pub grid_scale:[f64;2]}
#[derive(Clone,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Frames{version:u32,frames:BTreeMap<String,Frame>,#[serde(default,skip_serializing_if="Option::is_none")]cyclic_source_sha256:Option<String>}
#[derive(Clone,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct Frame{#[serde(default,skip_serializing_if="is_false")]line_guide_chart:bool,#[serde(default,skip_serializing_if="Option::is_none")]guide_elements:Option<Vec<u8>>,position_mm:[f64;2],size_mm:[f64;2],matrix:[f64;6],#[serde(default,skip_serializing_if="Option::is_none")]inert_paint:Option<GroupPaint>,#[serde(default,skip_serializing_if="Option::is_none")]source_index:Option<u32>,#[serde(default,skip_serializing_if="Option::is_none")]rect_radius_mm:Option<f64>}
#[derive(Clone,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupPaint{fill:String,stroke:String,width_mm:f64,cap:LineCap,join:LineJoin,miter:f64,#[serde(default="crate::model::default_true",skip_serializing_if="crate::model::is_true")]scale_with_object:bool,#[serde(default,skip_serializing_if="Option::is_none")]fixed_nib:Option<crate::FixedStrokeNib>}
pub(super) fn notes(doc:&Document)->Result<Option<String>> {
    let guide_charts=super::path_guide::planned(doc)?;
    let frames=doc.objects.iter().enumerate().filter(|(_,o)|!matches!(o.geometry,Geometry::Image{fit,clip_mm,..} if fit!=ImageFit::Stretch||clip_mm.is_some())).map(|(index,o)|{
        let s=&o.style;let inert_paint=matches!(o.geometry,Geometry::Group).then(||GroupPaint{fill:s.fill.clone(),stroke:s.stroke.clone(),width_mm:s.stroke_width_mm,cap:s.stroke_linecap,join:s.stroke_linejoin,miter:s.stroke_miterlimit,scale_with_object:s.stroke_scale_with_object,fixed_nib:s.stroke_fixed_nib});
        let rect_radius_mm=if let Geometry::Rect{radius_mm}=o.geometry {(radius_mm!=(radius_mm*10000.).round_ties_even()/10000.).then_some(radius_mm)}else{None};
        let guide_elements=guide_charts.get(&o.id).and_then(|c|if super::path_guide::straight(c).ok().flatten().is_none(){Some(super::path_guide::signature(&c.local_path().expect("validated generated guide")))}else{None});
        (o.id.clone(),Frame{guide_elements,line_guide_chart:guide_charts.contains_key(&o.id),rect_radius_mm,position_mm:[o.x_mm,o.y_mm],size_mm:[o.width_mm,o.height_mm],matrix:o.transform,inert_paint,source_index:Some(index as u32+1)})
    }).collect::<BTreeMap<_,_>>();
    if frames.is_empty(){return Ok(None);}let version=if frames.values().any(|f|f.guide_elements.is_some()){2}else{1};super::metadata::encode(PREFIX,&Frames{version,frames,cyclic_source_sha256:None}).map(Some)
}
// Version 3 is only an experimental cyclic-source binding. Older readers
// reject it rather than silently presenting the carrier as the original.
pub(super) fn cyclic_notes(notes:&str,contract:&str)->Result<String>{
    let mut frames:Frames=super::metadata::decode(PREFIX,notes)?;
    if ![1,2].contains(&frames.version)||frames.cyclic_source_sha256.is_some(){return Err(invalid("cyclic source requires an ordinary local frame contract"));}
    frames.version=3;frames.cyclic_source_sha256=Some(crate::model::sha256(contract.as_bytes()));
    super::metadata::encode(PREFIX,&frames)
}
pub(super) fn unseal_cyclic(frames:&mut Option<Frames>,contract:Option<&str>)->Result<()>{
    match (frames.as_mut(),contract){
        (Some(f),Some(c)) if f.version==3&&f.cyclic_source_sha256.as_deref()==Some(crate::model::sha256(c.as_bytes()).as_str())=>{
            f.version=if f.frames.values().any(|v|v.guide_elements.is_some()){2}else{1};f.cyclic_source_sha256=None;Ok(())
        },
        (Some(f),None) if f.version!=3&&f.cyclic_source_sha256.is_none()=>Ok(()),
        (None,None)=>Ok(()),
        _=>Err(invalid("cyclic source/local frame binding missing or changed")),
    }
}
pub(super) fn read(members:&BTreeMap<String,Vec<u8>>)->Result<Option<Frames>> {
    super::metadata::find(members,PREFIX)?.map(|v|super::metadata::decode(PREFIX,&v)).transpose()
}
// Use current native charts for edited generated paint layers, before the
// normal guarded width pass. This keeps tiny real moves and edited geometry
// while allowing an unchanged fine line width to share the current pen budget.
pub(super) fn prefer_native(doc:&Document,frames:&mut Frames,ids:&BTreeSet<String>){
    for id in ids{if let Some(frame)=frames.frames.get_mut(id){if let Ok(o)=doc.object(id){frame.position_mm=[o.x_mm,o.y_mm];frame.size_mm=[o.width_mm,o.height_mm];frame.matrix=o.transform;frame.rect_radius_mm=None;frame.inert_paint=None;frame.line_guide_chart=false;frame.guide_elements=None;}else{frames.frames.remove(id);}}}
}
pub(super) fn source_names(frames:&Frames)->BTreeSet<String>{frames.frames.keys().cloned().collect()}
pub(super) fn suppress_path_placements(frames:&mut Frames,ids:&BTreeSet<String>){
    for id in ids{if let Some(frame)=frames.frames.get_mut(id){frame.position_mm=[0.,0.];frame.matrix=Affine::IDENTITY.as_coeffs();}}
}
/// Local-frame hints alone cannot restore a fitted quarter turn. Its scalar
/// fit contract may have been removed by a native editor or an older tool.
pub(super) fn suppress_unbound_quarter_turns(doc:&Document,frames:&mut Frames,declared:&BTreeSet<String>){
    let ids=doc.objects.iter().filter(|o|matches!(o.geometry,Geometry::Text{text_path:Some(_),..})&&!declared.contains(&o.id)&&frames.frames.get(&o.id).is_some_and(|f|{
        let m=&f.matrix[..4];m==[0.,1.,-1.,0.]||m==[-1.,0.,0.,-1.]||m==[0.,-1.,1.,0.]
    })).map(|o|o.id.clone()).collect();suppress_path_placements(frames,&ids);
}
// Fitted shear is an active native frame coefficient. Never resurrect an old
// Own from source hints, even if a native editor clears the shear/direction.
pub(super) fn suppress_source_shears(doc:&Document,frames:&mut Frames){
    for(id,frame)in &mut frames.frames{let[a,b,k,d,_,_]=frame.matrix;if a==1.&&b==0.&&d==1.&&k!=0.&&doc.object(id).is_ok_and(|o|matches!(o.geometry,Geometry::Text{text_path:Some(_),..})){frame.matrix=Affine::IDENTITY.as_coeffs();frame.position_mm=[0.,0.];}}
}
pub(super) fn source_indices(frames:&Frames)->BTreeMap<String,u32>{frames.frames.iter().filter_map(|(id,f)|f.source_index.map(|n|(id.clone(),n))).collect()}
pub(super) fn parent_chart(doc:&Document,frames:Option<&Frames>,object:&crate::Object)->Result<Affine>{
    if frames.is_some_and(|f|![1,2].contains(&f.version)){return Err(unsupported("unknown native local frame version"));}
    let mut chain=vec![];let mut parent=object.parent_id.as_ref();let mut depth=0;
    while let Some(id)=parent{depth+=1;if depth>16{return Err(invalid("path text parent chart exceeds native depth"));}let object=doc.object(id)?;let frame=frames.and_then(|f|f.frames.get(id));let own=if let Some(f)=frame{if f.position_mm.iter().chain(&f.matrix).any(|v|!v.is_finite()){return Err(invalid("non-finite path parent chart"));}Affine::new(f.matrix)*Affine::translate((f.position_mm[0],f.position_mm[1]))}else{Affine::new(object.transform)*Affine::translate((object.x_mm,object.y_mm))};chain.push(own);parent=object.parent_id.as_ref();
    }let mut chain=chain.into_iter().rev();let first=chain.next().unwrap_or(Affine::IDENTITY);Ok(chain.fold(first,|world,local|world*local))
}
fn same_point(a:Affine,b:Affine,p:Point)->bool {
    let(a,b)=(a*p,b*p);(a.x-b.x).abs()<=0.00021&&(a.y-b.y).abs()<=0.00021
}
fn same_corners(a:Affine,b:Affine,size:[f64;2])->bool {
    // A dimensionless frame cannot bound displacement along its missing axis.
    if size.iter().any(|v|*v<=0.){return a==b;}
    [[0.,0.],[size[0],0.],[size[0],size[1]],[0.,size[1]]].into_iter().all(|p|same_point(a,b,Point::new(p[0],p[1])))
}
fn same_size_in_world(world:Affine,a:[f64;2],b:[f64;2])->bool {
    let m=world.as_coeffs();let dx=(a[0]-b[0]).abs();let dy=(a[1]-b[1]).abs();
    m[0].abs()*dx+m[2].abs()*dy<=0.00021&&m[1].abs()*dx+m[3].abs()*dy<=0.00021
}
fn positive_primitive_frame(m:Affine,w:f64)->Affine{
    if m.determinant()<0.{m*Affine::new([-1.,0.,0.,1.,w,0.])}else{m}
}
// Report122 normalizes a fixed-page-pen rectangle into a QR body chart. Its
// pen does not change local units. Recognize that specific reversible chart,
// including the actual14 point-table rounding, before restoring source units.
// Translation and axes use arithmetic roundoff bounds, not the drawing budget:
// a tiny real native move/axis edit must not become an old source placement.
fn fixed_rectangle_rechart(actual:Affine,observed:&crate::Object,source:Affine,candidate:&crate::Object)->bool{
    if observed.style.stroke_scale_with_object||candidate.style.stroke_scale_with_object||!matches!((&observed.geometry,&candidate.geometry),(Geometry::Rect{radius_mm:0.},Geometry::Rect{radius_mm:0.})){return false;}
    let a=positive_primitive_frame(actual,observed.width_mm);let b=positive_primitive_frame(source,candidate.width_mm);
    let [x,y,u,v,_,_]=b.as_coeffs();let sx=x.hypot(y);let sy=b.determinant()/sx;
    if !sx.is_finite()||!sy.is_finite()||sx<=0.||sy<=0.{return false;}
    let close=|x:f64,y:f64|(x-y).abs()<=64.*f64::EPSILON*x.abs().max(y.abs()).max(1.);
    let dims=[candidate.width_mm*sx,candidate.height_mm*sy];
    // Native14 stores integers;21 may retain the unrounded doubles. No
    // physical tolerance is used to swallow an actual local dimension edit.
    if ![observed.width_mm,observed.height_mm].into_iter().zip(dims).all(|(a,b)|close(a,b)||super::binary::coord(b).is_ok_and(|q|a==q as f64/10000.)){return false;}
    let m=a.as_coeffs();if ![m[0],m[1],m[2],m[3]].into_iter().zip([x/sx,y/sx,u/sy,v/sy]).all(|(x,y)|close(x,y)){return false;}
    let p=a*Point::new(0.,observed.height_mm);let q=b*Point::new(0.,candidate.height_mm);
    close(p.x,q.x)&&close(p.y,q.y)&&same_primitive(actual,observed,source,candidate,false)
}
fn same_primitive(a:Affine,observed:&crate::Object,b:Affine,candidate:&crate::Object,scaled_pen:bool)->bool {
    fn close(a:Point,b:Point)->bool{(a.x-b.x).abs()<=0.00021&&(a.y-b.y).abs()<=0.00021}
    let pad=|o:&crate::Object|if o.style.stroke=="none"{0.}else{o.style.stroke_width_mm*o.style.stroke_miterlimit.max(1.)};
    let(pa,pb)=(pad(observed),pad(candidate));
    if scaled_pen&&matches!((&observed.geometry,&candidate.geometry),(Geometry::Rect{radius_mm:0.},Geometry::Rect{radius_mm:0.})|(Geometry::Ellipse,Geometry::Ellipse)){
        // Rectangles and round-pen ellipses are unchanged by a signed axis
        // permutation. Compare physical canonical corners and both pen axes;
        // do not pair reversed source corners or signed pen directions.
        if let (Some(x),Some(y),Some(sa),Some(sb))=(super::primitive_frames::axis_bounds(a.as_coeffs(),[observed.width_mm,observed.height_mm]),super::primitive_frames::axis_bounds(b.as_coeffs(),[candidate.width_mm,candidate.height_mm]),super::primitive_frames::axis_scales(a.as_coeffs()),super::primitive_frames::axis_scales(b.as_coeffs())){
            for (u,v) in [(0.,0.),(1.,0.),(1.,1.),(0.,1.)]{for(px,py)in[(-1.,-1.),(1.,-1.),(1.,1.),(-1.,1.)]{
                if !close(Point::new(x[0]+u*x[2]+px*pa*sa[0],x[1]+v*x[3]+py*pa*sa[1]),Point::new(y[0]+u*y[2]+px*pb*sb[0],y[1]+v*y[3]+py*pb*sb[1])){return false;}
            }}return true;
        }
    }
    let symmetric=matches!((&observed.geometry,&candidate.geometry),(Geometry::Rect{radius_mm:0.},Geometry::Rect{radius_mm:0.})|(Geometry::Ellipse,Geometry::Ellipse));
    let(a,b)=if scaled_pen||symmetric{(positive_primitive_frame(a,observed.width_mm),positive_primitive_frame(b,candidate.width_mm))}else{(a,b)};
    let pens=if pa==0.&&pb==0.{vec![(0.,0.)]}else{vec![(-1.,-1.),(1.,-1.),(1.,1.),(-1.,1.)]};
    let pen_point=|p:Point,q:Point,px:f64,py:f64|{
        let(ap,bq)=(a*p,b*q);
        // A decoded scaled percentage pen changes its local units together
        // with the rectangle. Other profiles must also pass the fixed-world
        // bound; a shrinking matrix alone cannot waive that pen budget.
        close(if observed.style.stroke_scale_with_object{a*Point::new(p.x+pa*px,p.y+pa*py)}else{ap+kurbo::Vec2::new(pa*px,pa*py)},if candidate.style.stroke_scale_with_object{b*Point::new(q.x+pb*px,q.y+pb*py)}else{bq+kurbo::Vec2::new(pb*px,pb*py)})&&
        (scaled_pen||close(Point::new(ap.x+pa*px,ap.y+pa*py),Point::new(bq.x+pb*px,bq.y+pb*py)))
    };
    if let (Geometry::Rect{radius_mm:ra},Geometry::Rect{radius_mm:rb})=(&observed.geometry,&candidate.geometry) {
        let centers=|o:&crate::Object,r:f64|[(r,r),(o.width_mm-r,r),(o.width_mm-r,o.height_mm-r),(r,o.height_mm-r)];
        // Each circular quarter lies in its fixed unit quadrant box. The
        // difference is affine in these two parameters and the independent pen
        // offsets, so all box vertices bound every point without sampling.
        let quadrants=[[-1.,-1.],[1.,-1.],[1.,1.],[-1.,1.]];
        for ((ca,cb),q) in centers(observed,*ra).into_iter().zip(centers(candidate,*rb)).zip(quadrants) {
            for (u,v) in [(0.,0.),(q[0],0.),(0.,q[1]),(q[0],q[1])] {
                for &(px,py) in &pens {
                    if !pen_point(Point::new(ca.0+ra*u,ca.1+ra*v),Point::new(cb.0+rb*u,cb.1+rb*v),px,py){return false;}
                }
            }
        }true
    }else{
        for (x,y) in [(0.,0.),(1.,0.),(1.,1.),(0.,1.)] {for &(px,py) in &pens {
            if !pen_point(Point::new(observed.width_mm*x,observed.height_mm*y),Point::new(candidate.width_mm*x,candidate.height_mm*y),px,py){return false;}
        }}true
    }
}
fn same_path(a:Affine,b:Affine,path:&BezPath)->bool {
    // Affine coefficient epsilon alone can hide a material edit at distant
    // controls. Bound each native control's displacement in page millimetres.
    path.elements().iter().all(|el|match *el {
        PathEl::MoveTo(p)|PathEl::LineTo(p)=>same_point(a,b,p),
        PathEl::QuadTo(c,p)=>[c,p].into_iter().all(|p|same_point(a,b,p)),
        PathEl::CurveTo(c1,c2,p)=>[c1,c2,p].into_iter().all(|p|same_point(a,b,p)),
        PathEl::ClosePath=>true,
    })
}
fn same_path_outline(a:Affine,b:Affine,path:&BezPath,observed:&Style,candidate:&Style)->bool {
    let pad=|s:&Style|if s.stroke=="none"{0.}else{s.stroke_width_mm*s.stroke_miterlimit.max(1.)};
    let(pa,pb)=(pad(observed),pad(candidate));
    if pa==0.&&pb==0.{return same_path(a,b,path);}
    let close=|p:Point|[[-1.,-1.],[1.,-1.],[1.,1.],[-1.,1.]].into_iter().all(|q|{
        let(ap,bp)=(a*p,b*p);let x=if observed.stroke_scale_with_object{a*Point::new(p.x+pa*q[0],p.y+pa*q[1])}else{ap+kurbo::Vec2::new(pa*q[0],pa*q[1])};let y=if candidate.stroke_scale_with_object{b*Point::new(p.x+pb*q[0],p.y+pb*q[1])}else{bp+kurbo::Vec2::new(pb*q[0],pb*q[1])};
        (x.x-y.x).abs()<=0.00021&&(x.y-y.y).abs()<=0.00021&&
        (ap.x+pa*q[0]-bp.x-pb*q[0]).abs()<=0.00021&&(ap.y+pa*q[1]-bp.y-pb*q[1]).abs()<=0.00021
    });
    // A path segment lies in its control hull. Add a conservative pen box at
    // every native control, so a stationary centreline cannot hide a pen edit.
    path.elements().iter().all(|el|match *el{PathEl::MoveTo(p)|PathEl::LineTo(p)=>close(p),PathEl::QuadTo(c,p)=>[c,p].into_iter().all(close),PathEl::CurveTo(a,b,p)=>[a,b,p].into_iter().all(close),PathEl::ClosePath=>true})
}
// Text width and its frame share the same physical budget. Shape both
// current native and candidate paragraphs, including area reflow, and compare
// the actual outline control hulls plus conservative pen boxes. An ink bbox or
// a frame rectangle alone cannot justify restoring a fine active text stroke.
fn same_text_outline(native:&Document,observed:&crate::Object,candidate:&Document,normal:&crate::Object,a:Affine,b:Affine)->Result<bool>{
    let left=crate::text::layout(native,observed)?;let right=crate::text::layout(candidate,normal)?;
    if !left.outline_supported||!right.outline_supported||left.outline.elements().len()!=right.outline.elements().len(){return Ok(false);}
    if left.outline.elements().is_empty(){return Ok(a==b);}
    let pad=|s:&Style|if s.stroke=="none"{0.}else{s.stroke_width_mm*s.stroke_miterlimit.max(1.)};let(pa,pb)=(pad(&observed.style),pad(&normal.style));
    let close=|p:Point,q:Point|[[-1.,-1.],[1.,-1.],[1.,1.],[-1.,1.]].into_iter().all(|v|{
        let x=a*Point::new(p.x+pa*v[0],p.y+pa*v[1]);let y=b*Point::new(q.x+pb*v[0],q.y+pb*v[1]);let(ap,bq)=(a*p,b*q);
        (x.x-y.x).abs()<=0.00021&&(x.y-y.y).abs()<=0.00021&&(ap.x+pa*v[0]-bq.x-pb*v[0]).abs()<=0.00021&&(ap.y+pa*v[1]-bq.y-pb*v[1]).abs()<=0.00021
    });
    Ok(left.outline.elements().iter().zip(right.outline.elements()).all(|(l,r)|match(*l,*r){
        (PathEl::MoveTo(a),PathEl::MoveTo(b))|(PathEl::LineTo(a),PathEl::LineTo(b))=>close(a,b),
        (PathEl::QuadTo(a,b),PathEl::QuadTo(c,d))=>close(a,c)&&close(b,d),
        (PathEl::CurveTo(a,b,c),PathEl::CurveTo(d,e,f))=>close(a,d)&&close(b,e)&&close(c,f),
        (PathEl::ClosePath,PathEl::ClosePath)=>true,_=>false,
    }))
}
#[cfg(test)]
pub(super) fn restore(doc:&mut Document,frames:Frames)->Result<(usize,usize)> {
    restore_with_widths(doc,frames,&BTreeMap::new())
}
#[cfg(test)]
pub(super) fn restore_with_widths(doc:&mut Document,frames:Frames,widths:&BTreeMap<String,f64>)->Result<(usize,usize)> {
    restore_with_units(doc,frames,widths,&BTreeMap::new())
}
pub(super) fn restore_with_units(doc:&mut Document,frames:Frames,widths:&BTreeMap<String,f64>,units:&BTreeMap<String,u32>)->Result<(usize,usize)> {
    restore_with_nibs(doc,frames,widths,units,&BTreeMap::new())
}
pub(super) fn restore_with_nibs(doc:&mut Document,mut frames:Frames,widths:&BTreeMap<String,f64>,units:&BTreeMap<String,u32>,nibs:&BTreeMap<String,NativeNib>)->Result<(usize,usize)> {
    if ![1,2].contains(&frames.version){return Err(unsupported("unknown native local frame version"));}let native=doc.clone();
    // Deleted/renamed native objects must not be recreated by old Notes.
    frames.frames.retain(|id,_|doc.objects.iter().any(|o|&o.id==id));let count=frames.frames.len();let mut native_frames=BTreeSet::new();let mut guide_frames=BTreeSet::new();
    let actual_world=native.objects.iter().map(|o|native.world_transform(o).map(|m|(o.id.clone(),m))).collect::<Result<BTreeMap<_,_>>>()?;
    let live_guides=native.objects.iter().filter_map(|o|if let Geometry::Text{text_path:Some(p),..}=&o.geometry{Some(p.path_id.clone())}else{None}).collect::<BTreeSet<_>>();
    for (id,frame) in &frames.frames {
        if frame.guide_elements.as_ref().is_some_and(|s|frames.version!=2||!frame.line_guide_chart||s.len()<2||s.len()>65535||s[0]!=0||s[1..].iter().any(|v|![1,3].contains(v))){return Err(invalid("invalid native guide element contract"));}
        if !valid_id(id)||frame.position_mm.iter().chain(&frame.size_mm).chain(&frame.matrix).any(|n|!n.is_finite())||frame.size_mm.iter().any(|n|*n<0.){return Err(invalid("invalid local frame scalar"));}
        let source_matrix=(parent_chart(&native,Some(&frames),native.object(id)?)?*Affine::new(frame.matrix)).as_coeffs();
        if live_guides.contains(id)&&!frame.line_guide_chart{
            let expected=Affine::new(source_matrix)*Affine::translate((frame.position_mm[0],frame.position_mm[1]));
            // A source chart must not spend the drawing tolerance to reverse
            // a real guide move/deformation. Only unit-rebase roundoff fits.
            if !actual_world[id].as_coeffs().into_iter().zip(expected.as_coeffs()).all(|(a,b)|(a-b).abs()<=64.*f64::EPSILON*a.abs().max(b.abs()).max(1.)){native_frames.insert(id.clone());}
        }
        if frame.line_guide_chart {
            let source=Affine::new(source_matrix)*Affine::translate((frame.position_mm[0],frame.position_mm[1]));
            if let Some(geometry)=super::path_guide::restore_contour(native.object(id)?,actual_world[id],source,frame.guide_elements.as_deref())? {
                let o=doc.objects.iter_mut().find(|o|&o.id==id).unwrap();o.geometry=geometry;o.x_mm=frame.position_mm[0];o.y_mm=frame.position_mm[1];o.width_mm=frame.size_mm[0];o.height_mm=frame.size_mm[1];o.transform=frame.matrix;
                guide_frames.insert(id.clone());continue;
            }
            native_frames.insert(id.clone());
        }

        let nib_width=nibs.get(id).and_then(|nib|super::primitive_frames::column_scales(source_matrix).and_then(|s|{
            let major=s[0].max(s[1]);let proposed=super::binary::coord(nib.nominal/major).ok()? as f64/10000.;
            let native_projection=proposed*major*nib.raw/nib.nominal;
            (super::binary::coord(native_projection).ok()==super::binary::coord(nib.raw).ok()).then_some(proposed)
        }));
        let object=doc.objects.iter_mut().find(|o|&o.id==id).ok_or_else(||unsupported("local frame target missing from native objects"))?;
        let unit=f64::from(units.get(id).copied().unwrap_or(1));
        let dimensions_changed=object.width_mm!=frame.size_mm[0]||object.height_mm!=frame.size_mm[1];
        let rounded=matches!(object.geometry,Geometry::Rect{radius_mm} if radius_mm>0.);
        let outlined=matches!(object.geometry,Geometry::Rect{..}|Geometry::Ellipse|Geometry::Image{..})&&object.style.stroke!="none";
        let area=matches!(object.geometry,Geometry::Text{layout:crate::TextLayoutOptions{mode:crate::TextMode::Area,..},..});
        let original_grid_size=[object.width_mm,object.height_mm].into_iter().zip(frame.size_mm).all(|(a,b)|super::binary::coord(b*unit).is_ok_and(|v|a==v as f64/10000./unit));
        let grid_displacement=same_size_in_world(actual_world[id],[object.width_mm,object.height_mm],frame.size_mm);
        // Only a decoded effective scaled nib can authorize this chart. The
        // source proposal must itself project to the exact integer-percent
        // primitive profile. Final body and transformed pen bounds are checked
        // together below against the actual native object, never old Notes.
        let percentage=nib_width.is_some_and(|width|{
            let mut proposal=object.clone();proposal.x_mm=frame.position_mm[0];proposal.y_mm=frame.position_mm[1];proposal.width_mm=frame.size_mm[0];proposal.height_mm=frame.size_mm[1];proposal.transform=source_matrix;proposal.style.stroke_width_mm=width;
            super::primitive_frames::independent_nib(&mut proposal).is_ok_and(|v|v.is_some())
        });
        let radius_fits= !matches!(object.geometry,Geometry::Rect{radius_mm} if radius_mm>frame.size_mm[0].min(frame.size_mm[1])/2.);
        let fixed_chart=if unit==1.{let mut proposal=object.clone();proposal.width_mm=frame.size_mm[0];proposal.height_mm=frame.size_mm[1];fixed_rectangle_rechart(actual_world[id],object,Affine::new(source_matrix)*Affine::translate((frame.position_mm[0],frame.position_mm[1])),&proposal)}else{false};
        if dimensions_changed&&(rounded||outlined||area)&&!percentage&&!fixed_chart&&!(original_grid_size&&radius_fits&&grid_displacement) {
            // A local basis change also changes corner radii and stroke units.
            // A plain grid projection is reversible from the source frame;
            // other basis changes must keep native editable units. Also avoid
            // an intermediate invalid radius at a half-dimension boundary.
            native_frames.insert(id.clone());
        }else{object.width_mm=frame.size_mm[0];object.height_mm=frame.size_mm[1];}
        if let Some(radius)=frame.rect_radius_mm {
            if !radius.is_finite()||radius<0.||radius>frame.size_mm[0].min(frame.size_mm[1])/2.{return Err(invalid("invalid source rectangle radius scalar"));}
            if !native_frames.contains(id) {if let Geometry::Rect{radius_mm}=&mut object.geometry {
                if super::binary::coord(radius*unit).is_ok_and(|v|*radius_mm==v as f64/10000./unit)&&radius<=object.width_mm.min(object.height_mm)/2.{*radius_mm=radius;}
            }}
        }
        if percentage{object.style.stroke_width_mm=nib_width.unwrap();}
        if !native_frames.contains(id)&&matches!(object.geometry,Geometry::Rect{..}|Geometry::Ellipse|Geometry::Path{..}|Geometry::Text{text_path:None,..})&&object.style.stroke!="none" {
            if let Some(width)=widths.get(id) {
                let projected=super::binary::coord(*width)? as f64/10000.;
                if projected>0.&&(percentage||object.style.stroke_width_mm==projected){object.style.stroke_width_mm=*width;}
            }
        }
        object.x_mm=frame.position_mm[0];object.y_mm=frame.position_mm[1];object.transform=frame.matrix;
        if let Some(p)=&frame.inert_paint {
            if !matches!(object.geometry,Geometry::Group){return Err(invalid("inert paint target is not a native group"));}
            object.style=Style{fill:p.fill.clone(),stroke:p.stroke.clone(),stroke_width_mm:p.width_mm,stroke_linecap:p.cap,stroke_linejoin:p.join,stroke_miterlimit:p.miter,stroke_scale_with_object:p.scale_with_object,stroke_fixed_nib:p.fixed_nib,opacity:object.style.opacity};
        }
    }
    doc.validate()?;let normalized=super::write::normalized_without_guide_charts(doc)?;
    let expected_world=doc.objects.iter().map(|o|doc.world_transform(o).map(|m|(o.id.clone(),m))).collect::<Result<BTreeMap<_,_>>>()?;
    for index in 0..doc.objects.len(){
        let object=doc.objects[index].clone();if guide_frames.contains(&object.id){continue;}let tracked=frames.frames.contains_key(&object.id)&&!native_frames.contains(&object.id);
        if tracked&&matches!(object.geometry,Geometry::Group){continue;}
        let parent=object.parent_id.as_ref().map(|id|doc.world_transform(doc.object(id)?)).transpose()?.unwrap_or(Affine::IDENTITY);
        let mut actual=actual_world[&object.id];let own=Affine::translate((object.x_mm,object.y_mm));
        if tracked {
            let normal=normalized.object(&object.id)?;let expected=normalized.world_transform(normal)?;
            let primitive=matches!(object.geometry,Geometry::Rect{..}|Geometry::Ellipse|Geometry::Image{..});
            if primitive {
                let observed=native.object(&object.id)?;
                if normal.width_mm<=0.||normal.height_mm<=0.{return Err(invalid("local primitive frame must be positive"));}
                // Corel may normalize affine axis lengths into primitive
                // dimensions. Compare physical corners at the existing native
                // tolerance, then preserve the exact source coordinate chart.
                actual*=Affine::scale_non_uniform(observed.width_mm/normal.width_mm,observed.height_mm/normal.height_mm);
            }
            let size=[normal.width_mm,normal.height_mm];
            let observed=native.object(&object.id)?;
            let text_width_changed=matches!(normal.geometry,Geometry::Text{text_path:None,..})&&normal.style.stroke_width_mm!=observed.style.stroke_width_mm;
            let same=if text_width_changed {same_text_outline(&native,observed,&normalized,normal,actual,expected)?}else if let Geometry::Path{d,..}=&normal.geometry {same_path_outline(actual,expected,&crate::path::parse(d)?,&observed.style,&normal.style)}else if primitive {same_primitive(actual_world[&object.id],observed,expected,normal,nibs.contains_key(&object.id))}else{same_corners(actual,expected,size)};
            // A nib rechart must not spend the geometric tolerance to undo
            // an actual one-grid-unit dimension edit. Compare both physical
            // axes on the native grid in addition to the combined pen bound.
            let nib_size_same=if primitive&&nibs.contains_key(&object.id){
                let sizes=super::primitive_frames::axis_bounds(actual_world[&object.id].as_coeffs(),[observed.width_mm,observed.height_mm]).zip(super::primitive_frames::axis_bounds(expected.as_coeffs(),[normal.width_mm,normal.height_mm]));
                if let Some((a,b))=sizes{[(a[2],b[2]),(a[3],b[3])].into_iter().all(|(x,y)|super::binary::coord(x).ok().zip(super::binary::coord(y).ok()).is_some_and(|(x,y)|x==y))}else{
                    let(a,b)=(positive_primitive_frame(actual_world[&object.id],observed.width_mm).as_coeffs(),positive_primitive_frame(expected,normal.width_mm).as_coeffs());
                    [(a[0]*observed.width_mm,b[0]*normal.width_mm),(a[1]*observed.width_mm,b[1]*normal.width_mm),(a[2]*observed.height_mm,b[2]*normal.height_mm),(a[3]*observed.height_mm,b[3]*normal.height_mm),(a[0].hypot(a[1])*observed.width_mm,b[0].hypot(b[1])*normal.width_mm),(a[2].hypot(a[3])*observed.height_mm,b[2].hypot(b[3])*normal.height_mm)].into_iter().all(|(x,y)|super::binary::coord(x).ok().zip(super::binary::coord(y).ok()).is_some_and(|(x,y)|x==y))
                }
            }else{true};
            // A reversible binary chart can make one native dimension grid
            // smaller than one world grid. Check in the actual native chart
            // as well, so the physical error budget cannot undo that edit.
            let local_nib_size_same=if primitive&&nibs.contains_key(&object.id){
                let mut proposed=normal.clone();let general=super::primitive_frames::independent_axis_nib(&mut proposed)?.is_none()&&super::primitive_frames::independent_nib(&mut proposed)?.is_some();
                if general{let a=positive_primitive_frame(actual_world[&object.id],observed.width_mm);let b=positive_primitive_frame(expected,normal.width_mm);let r=(a.inverse()*b).as_coeffs();let[nx,ny]=nibs[&object.id].grid_scale;
                    [(observed.width_mm*nx,r[0]*normal.width_mm*nx),(0.,r[1]*normal.width_mm*ny),(0.,r[2]*normal.height_mm*nx),(observed.height_mm*ny,r[3]*normal.height_mm*ny)].into_iter().all(|(x,y)|super::binary::coord(x).ok().zip(super::binary::coord(y).ok()).is_some_and(|(a,b)|a==b))
                }else{true}
            }else{true};
            if same&&nib_size_same&&local_nib_size_same{continue;}
            let path_width_changed=matches!(normal.geometry,Geometry::Path{..})&&normal.style.stroke_width_mm!=observed.style.stroke_width_mm;
            if path_width_changed||text_width_changed||primitive&&(normal.style.stroke!="none"||matches!(normal.geometry,Geometry::Rect{radius_mm} if radius_mm>0.)||frames.frames[&object.id].rect_radius_mm.is_some()) {
                // Radius, dimensions, affine edits and pen units share a single
                // comparison with the original native shape. A failed bound
                // restores all of them together; no later stage spends another
                // independent tolerance budget against a modified reference.
                doc.objects[index].width_mm=observed.width_mm;doc.objects[index].height_mm=observed.height_mm;doc.objects[index].geometry=observed.geometry.clone();doc.objects[index].style.stroke_width_mm=observed.style.stroke_width_mm;
                doc.objects[index].transform=(parent.inverse()*actual_world[&object.id]*own.inverse()).as_coeffs();native_frames.insert(object.id.clone());continue;
            }
            doc.objects[index].transform=(parent.inverse()*actual*expected.inverse()*expected_world[&object.id]*own.inverse()).as_coeffs();
        }else{doc.objects[index].transform=(parent.inverse()*actual*own.inverse()).as_coeffs();}
    }
    for index in 0..doc.objects.len(){
        let object=&doc.objects[index];let Some(clip)=&native.object(&object.id)?.clip else{continue};let actual=actual_world[&object.id];let current=doc.world_transform(object)?;
        let path=clip.path()?;
        if !same_path(actual,current,&path){doc.objects[index].clip=Some(Clip::Path{d:(current.inverse()*actual*path).to_svg(),fill_rule:clip.rule()});}
    }doc.validate()?;Ok((count-native_frames.len(),native_frames.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn native(radius:f64,stroke:&str)->Document {
        serde_json::from_value(json!({"id":"native","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"rect","page_id":"page-1","x_mm":0.,"y_mm":0.,"width_mm":24.,"height_mm":16.,"geometry":{"kind":"rect","radius_mm":radius},"style":{"fill":"#123456","stroke":stroke,"stroke_width_mm":1.,"opacity":1.}}]})).unwrap()
    }
    fn source_frames()->Frames {Frames{version:1,cyclic_source_sha256:None,frames:BTreeMap::from([("rect".into(),Frame{guide_elements:None,line_guide_chart:false,position_mm:[0.,0.],size_mm:[12.,8.],matrix:[2.,0.,0.,2.,0.,0.],inert_paint:None,source_index:None,rect_radius_mm:None})])}}
    #[test]
    fn guide_element_contract_has_an_explicit_version_and_bounded_command_sequence(){
        for mode in 0..6{let mut f=source_frames();f.version=2;let frame=f.frames.get_mut("rect").unwrap();frame.line_guide_chart=true;frame.guide_elements=Some(vec![0,1,3]);match mode{0=>f.version=1,1=>frame.line_guide_chart=false,2=>frame.guide_elements=Some(vec![0]),3=>frame.guide_elements=Some(vec![1,3]),4=>frame.guide_elements=Some(vec![0,4]),_=>frame.guide_elements=Some(vec![0,2])};let mut doc=native(0.,"none");assert!(restore(&mut doc,f).is_err(),"mode{mode}");}
    }
    #[test]
    fn native_rounded_and_outlined_frames_do_not_change_curves_or_stroke_units() {
        for (radius,stroke) in [(2.,"none"),(6.,"none"),(0.,"#000000"),(2.,"#000000")] {
            let mut doc=native(radius,stroke);let before=doc.clone();let (_,normalized)=restore(&mut doc,source_frames()).unwrap();assert_eq!(normalized,1);
            assert_eq!(doc.objects,before.objects,"native corners, local radius and stroke must remain the same drawing");
        }
    }
    #[test]
    fn removed_objects_leave_no_ghosts_and_do_not_block_native_reopen() {
        let mut doc=native(0.,"none");doc.objects.clear();assert_eq!(restore(&mut doc,source_frames()).unwrap(),(0,0));assert!(doc.objects.is_empty());
    }
    #[test]
    fn resized_native_area_keeps_its_live_reflow_frame() {
        let mut doc=native(0.,"none");doc.objects[0].geometry=Geometry::Text{content:"Area text 中文".into(),font_family:"Microsoft YaHei".into(),font_size_mm:4.1,text_path:None,layout:crate::TextLayoutOptions{mode:crate::TextMode::Area,..Default::default()}};
        let before=doc.clone();assert_eq!(restore(&mut doc,source_frames()).unwrap().1,1);assert_eq!(doc.objects,before.objects);
    }
    #[test]
    fn native_radius_at_half_dimension_is_not_invalidated_by_small_old_frame_differences() {
        let mut doc=native(4.0001,"none");doc.objects[0].width_mm=8.0002;doc.objects[0].height_mm=8.0002;doc.validate().unwrap();let before=doc.clone();
        let mut frames=source_frames();let f=frames.frames.get_mut("rect").unwrap();f.size_mm=[8.,8.];f.matrix=[1.,0.,0.,1.,0.,0.];
        assert_eq!(restore(&mut doc,frames).unwrap().1,1);assert_eq!(doc.objects,before.objects);
    }
    fn frames(doc:&Document)->Frames {super::super::metadata::decode(PREFIX,&notes(doc).unwrap().unwrap()).unwrap()}
    #[test]
    fn distant_native_curve_controls_keep_small_affine_edits_in_page_units() {
        let mut source=native(0.,"none");
        source.objects[0].width_mm=1.;source.objects[0].height_mm=1.;
        // The frame hint and endpoints are small; the actual cubic controls
        // still expose a 0.001 mm displacement from this tiny native scale.
        source.objects[0].geometry=Geometry::Path{d:"M0 0 C2000 20 2000 -20 1 0".into(),fill_rule:Default::default()};
        let mut edited=source.clone();edited.objects[0].transform[0]=1.0000005;let expected=edited.clone();
        restore(&mut edited,frames(&source)).unwrap();assert_eq!(edited.objects,expected.objects);
        let before=expected.world_transform(&expected.objects[0]).unwrap()*Point::new(2000.,20.);
        let after=edited.world_transform(&edited.objects[0]).unwrap()*Point::new(2000.,20.);
        assert!((before-after).hypot()<1e-12);
    }
    #[test]
    fn native_text_frame_edits_use_displacement_and_keep_degenerate_axes() {
        for(mode,width)in [(crate::TextMode::Area,2000.),(crate::TextMode::Point,2000.)] {
            let mut source=native(0.,"none");source.objects[0].width_mm=width;
            source.objects[0].geometry=Geometry::Text{content:"Live text 中文".into(),font_family:"Microsoft YaHei".into(),font_size_mm:4.1,text_path:None,layout:crate::TextLayoutOptions{mode,..Default::default()}};
            let mut edited=source.clone();edited.objects[0].transform[0]=1.0000005;let expected=edited.clone();
            restore(&mut edited,frames(&source)).unwrap();assert_eq!(edited.objects,expected.objects,"{mode:?}, width={width}");
        }
        assert!(!same_corners(Affine::IDENTITY,Affine::scale_non_uniform(1.0000005,1.),[0.,16.]));
    }
    #[test]
    fn native_group_clip_is_rebased_when_small_coefficients_move_distant_controls() {
        let mut source=native(0.,"none");let mut child=source.objects[0].clone();child.id="child".into();child.parent_id=Some("rect".into());
        source.objects[0].geometry=Geometry::Group;source.objects[0].clip=Some(Clip::Path{d:"M0 0 C2000 20 2000 -20 1 0 Z".into(),fill_rule:Default::default()});source.objects.push(child);
        let mut edited=source.clone();edited.objects[0].transform[0]=1.0000005;
        let native=edited.world_transform(&edited.objects[0]).unwrap()*edited.objects[0].clip.as_ref().unwrap().path().unwrap();
        restore(&mut edited,frames(&source)).unwrap();
        let restored=edited.world_transform(&edited.objects[0]).unwrap()*edited.objects[0].clip.as_ref().unwrap().path().unwrap();
        assert_eq!(native.elements().len(),restored.elements().len());
        for(a,b)in native.elements().iter().zip(restored.elements()) {assert_eq!(a,b);}
    }
    #[test]
    fn source_chart_restore_keeps_the_existing_page_unit_tolerance() {
        let mut source=native(0.,"none");source.objects[0].geometry=Geometry::Path{d:"M0 0 L2000 0".into(),fill_rule:Default::default()};
        for(displacement,source_chart)in [(0.00020,true),(0.00022,false)] {
            let mut edited=source.clone();edited.objects[0].transform[0]=1.+displacement/2000.;let actual=edited.objects[0].transform;
            restore(&mut edited,frames(&source)).unwrap();assert_eq!(edited.objects[0].transform,if source_chart{source.objects[0].transform}else{actual});
        }
    }
    #[test]
    fn wide_outlines_expose_native_affine_edits_that_the_small_centerline_cannot_bound() {
        let mut source=native(0.,"#000000");source.objects[0].style.stroke_width_mm=1000.;
        let mut edited=source.clone();edited.objects[0].transform[0]=1.0000005;let expected=edited.clone();
        restore(&mut edited,frames(&source)).unwrap();assert_eq!(edited.objects,expected.objects);
    }
    #[test]
    fn primitive_dimension_rechart_never_changes_live_outline_units_when_matrix_compensation_is_needed() {
        let mut source=native(0.,"#000000");source.objects[0].width_mm=0.000149;
        let mut edited=source.clone();edited.objects[0].width_mm=0.0001;edited.objects[0].transform[0]=1.0001;let expected=edited.clone();
        let (_,native_units)=restore(&mut edited,frames(&source)).unwrap();assert_eq!(native_units,1);assert_eq!(edited.objects,expected.objects);
    }
    #[test]
    fn explicit_native_radius_and_coordinate_unit_edits_override_old_radius_scalars() {
        let mut source=native(0.1234567,"none");source.objects[0].width_mm=12.;source.objects[0].height_mm=8.;
        for(radius,size)in [(0.1236,[12.,8.]),(0.1235,[24.,16.])] {
            let mut edited=source.clone();edited.objects[0].width_mm=size[0];edited.objects[0].height_mm=size[1];edited.objects[0].geometry=Geometry::Rect{radius_mm:radius};let expected=edited.clone();
            restore(&mut edited,frames(&source)).unwrap();assert_eq!(edited.objects,expected.objects);
        }
        let mut edited=source.clone();edited.objects[0].geometry=Geometry::Ellipse;let expected=edited.clone();restore(&mut edited,frames(&source)).unwrap();assert_eq!(edited.objects,expected.objects);
    }
    #[test]
    fn grid_aligned_radius_does_not_store_a_redundant_scalar() {
        let source=native(0.1235,"none");let value=serde_json::to_value(frames(&source)).unwrap();assert!(value["frames"]["rect"].get("rect_radius_mm").is_none());
    }
    #[test]
    fn affine_and_radius_restore_share_one_native_curve_budget() {
        let mut source=native(0.1234567,"none");source.objects[0].width_mm=12.;source.objects[0].height_mm=8.;source.objects[0].transform=[4.,0.,0.,4.,0.,0.];
        let mut edited=source.clone();edited.objects[0].geometry=Geometry::Rect{radius_mm:0.1235};edited.objects[0].transform[0]=4.-0.00020/12.;let expected=edited.clone();
        restore(&mut edited,frames(&source)).unwrap();assert_eq!(edited.objects,expected.objects);
    }
    #[test]
    fn compensated_native_dimensions_do_not_prove_equal_outline_units() {
        let mut source=native(0.,"#000000");source.objects[0].width_mm=12.123449;source.objects[0].height_mm=8.;source.objects[0].transform=[4.,0.,0.,4.,0.,0.];source.objects[0].style.stroke_width_mm=1000.;
        let mut edited=source.clone();edited.objects[0].width_mm=12.1234;edited.objects[0].transform[0]=4.*source.objects[0].width_mm/edited.objects[0].width_mm;let expected=edited.clone();
        restore(&mut edited,frames(&source)).unwrap();assert_eq!(edited.objects,expected.objects);
    }
    #[test]
    fn vertical_path_native_affine_edits_are_bounded_by_the_live_outline() {
        let mut source=native(0.,"#000000");source.objects[0].geometry=Geometry::Path{d:"M0 0 L0 1".into(),fill_rule:Default::default()};source.objects[0].width_mm=1.;source.objects[0].height_mm=1.;source.objects[0].style.stroke_width_mm=1000.;
        let mut actual=source.clone();actual.objects[0].transform[0]=1.0000005;let expected=actual.clone();
        restore(&mut actual,frames(&source)).unwrap();assert_eq!(actual.objects,expected.objects,"a stationary centerline must not hide an edited live pen");
    }
    #[test]
    fn a_path_width_candidate_cannot_hide_large_outline_or_recharted_native_edits() {
        let mut source=native(0.,"#000000");source.objects[0].width_mm=12.;source.objects[0].height_mm=8.;source.objects[0].geometry=Geometry::Path{d:"M0 0L12 0L12 8Z".into(),fill_rule:Default::default()};source.objects[0].style.stroke_width_mm=1000.1234567;
        let widths=BTreeMap::from([("rect".into(),1000.1234567)]);
        let mut edited=source.clone();edited.objects[0].style.stroke_width_mm=1000.1235;edited.objects[0].transform[0]=1.00000005;let expected=edited.clone();
        restore_with_widths(&mut edited,frames(&source),&widths).unwrap();assert_eq!(edited.objects,expected.objects,"small centreline motion still changes the live broad pen");
        source.objects[0].style.stroke_width_mm=0.1234567;source.objects[0].transform=[2.,0.,0.,2.,0.,0.];
        let mut edited=source.clone();edited.objects[0].width_mm=24.;edited.objects[0].height_mm=16.;edited.objects[0].geometry=Geometry::Path{d:"M0 0L24 0L24 16Z".into(),fill_rule:Default::default()};edited.objects[0].transform=[1.,0.,0.,1.,0.,0.];edited.objects[0].style.stroke_width_mm=0.1235;let expected=edited.clone();
        restore_with_widths(&mut edited,frames(&source),&BTreeMap::from([("rect".into(),0.1234567)])).unwrap();assert_eq!(edited.objects,expected.objects,"a native coordinate rechart must retain its own pen units");
    }
    #[test]
    fn shrinking_shapes_do_not_waive_the_fixed_world_pen_width_budget() {
        for geometry in [Geometry::Rect{radius_mm:0.},Geometry::Ellipse,Geometry::Path{d:"M0 0L12 0L12 8Z".into(),fill_rule:Default::default()}] {
        let mut source=native(0.,"#000000");source.objects[0].geometry=geometry;source.objects[0].transform=[0.001,0.,0.,0.001,0.,0.];source.objects[0].style.stroke_width_mm=0.1234567;source.objects[0].style.stroke_miterlimit=1000.;
        let mut edited=source.clone();edited.objects[0].style.stroke_width_mm=0.1235;let expected=edited.clone();
        restore_with_widths(&mut edited,frames(&source),&BTreeMap::from([("rect".into(),0.1234567)])).unwrap();assert_eq!(edited.objects,expected.objects,"a native fixed-world pen may not be scaled down by the path matrix in this bound");
    }}
    #[test]
    fn actual_fixed_qr_return_does_not_erase_tiny_native_moves_axes_or_dimensions(){
        use super::super::binary::*;
        for version in [14,21]{for mode in [0,1,2]{
            let path=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../examples/assets/cdr/corel2019/save122-fixed/fixed-strokes-fixedstroke{version}-rect-shear-corel-resaved.cdr"));let mut file=read_native(&std::fs::read(path).unwrap()).unwrap();
            fn edit(nodes:&mut[Node],modern:bool,mode:u8)->bool{for n in nodes{
                if n.kind()==Some(b"obj "){let(kind,mut args)=super::super::write::arguments(&n.find(b"loda").unwrap().data).unwrap();if kind==1&&args.iter().any(|(k,v)|*k==1000&&read_wide(v).unwrap()=="content"){
                    if mode==2{let g=&mut args.iter_mut().find(|(k,_)|*k==30).unwrap().1;if modern{let w=f64_at(g,0).unwrap();g[..8].copy_from_slice(&(w+1.).to_le_bytes());}else{let w=i32_at(g,0).unwrap();g[..4].copy_from_slice(&(w+1).to_le_bytes());}n.find_mut(b"loda").unwrap().data=super::super::write::loda(kind,&args);}
                    else{let raw=&mut n.find_mut(b"trfd").unwrap().data;let at=u32_at(raw,u32_at(raw,8).unwrap()as usize).unwrap()as usize+16;let pos=at+if mode==0{16}else{0};let v=f64_at(raw,pos).unwrap()+if mode==0{0.001}else{1e-8};raw[pos..pos+8].copy_from_slice(&v.to_le_bytes());}return true;
                }}if let Some(c)=&mut n.children{if edit(c,modern,mode){return true;}}
            }false}assert!(edit(&mut file.nodes,version==21,mode));
            fn pack(f:&mut NativeRecords)->Vec<u8>{if f.version==2100{repack21(f).unwrap()}else{f.members.insert("content/riffData.cdr".into(),write(&f.nodes).unwrap());zip(&f.members.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>())}}
            let actual=crate::cdr::decode(&pack(&mut file),"changed").unwrap().0;file.members.remove("metadata/metadata.xml");file.members.remove("META-INF/metadata.xml");let reference=crate::cdr::decode(&pack(&mut file),"reference").unwrap().0;
            let a=actual.object("content").unwrap();let b=reference.object("content").unwrap();assert_eq!([a.width_mm,a.height_mm],[b.width_mm,b.height_mm]);let am=actual.world_transform(a).unwrap();let bm=reference.world_transform(b).unwrap();
            for p in [(0.,0.),(a.width_mm,0.),(a.width_mm,a.height_mm),(0.,a.height_mm)]{let x=am*Point::from(p);let y=bm*Point::from(p);assert!((x.x-y.x).abs()<1e-11&&(x.y-y.y).abs()<1e-11,"version{version} mode{mode}");}
        }}
    }

}
