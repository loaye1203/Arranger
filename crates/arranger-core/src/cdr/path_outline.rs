//! A single physical budget for live glyph controls and their scaled pen.
//! Notes may restore units only after this comparison with actual native data.
use super::Result;
use crate::{Document,Object};
use kurbo::{PathEl,Point};

pub(super) fn needs_check(doc:&Document,o:&Object)->Result<bool>{
    if !matches!(o.geometry,crate::Geometry::Text{text_path:Some(_),..}){return Ok(false);}
    if super::path_text::own_turn(o).is_some(){return Ok(true);}
    if o.style.stroke=="none"{return Ok(false);}
    let own=super::path_text::own_scale(o)?;
    let width=o.style.stroke_width_mm*super::path_text::parent_scale(doc,o)?*own;
    Ok(own!=1.||o.style.stroke_width_mm>0.&&super::binary::coord(width)? as f64/10000.!=width)
}

pub(super) fn scene(doc:&Document,id:&str)->Result<Document>{
    let crate::Geometry::Text{text_path:Some(binding),..}=&doc.object(id)?.geometry else{return Err(super::invalid("fitted outline target missing"))};
    let mut ids=std::collections::BTreeSet::new();
    for root in[id,binding.path_id.as_str()]{let mut current=Some(root);let mut depth=0;
        while let Some(id)=current{depth+=1;if depth>17{return Err(super::unsupported("fitted outline parent chain exceeds native depth"));}ids.insert(id.to_string());current=doc.object(id)?.parent_id.as_deref();}
    }
    // Images are irrelevant to glyph layout. Do not copy the entire resource
    // catalogue twice for every outlined story in a large mixed document.
    let value=Document{id:doc.id.clone(),unit:doc.unit.clone(),pages:doc.pages.clone(),objects:doc.objects.iter().filter(|o|ids.contains(&o.id)).cloned().collect(),resources:Default::default()};value.validate()?;Ok(value)
}

pub(super) fn preserves(native:&Document,actual:&Object,proposal:&Document,expected:&Object)->Result<bool>{
    if !actual.style.stroke_scale_with_object||!expected.style.stroke_scale_with_object||actual.style.stroke_fixed_nib.is_some()||expected.style.stroke_fixed_nib.is_some(){return Ok(false);}
    if [actual,expected].into_iter().any(|o|o.style.stroke!="none"&&o.style.stroke_width_mm<=0.){return Ok(false);}
    let left=crate::text::layout(native,actual)?;let right=crate::text::layout(proposal,expected)?;
    if !left.outline_supported||!right.outline_supported||left.outline.elements().is_empty()||left.outline.elements().len()!=right.outline.elements().len(){return Ok(false);}
    let a=native.world_transform(actual)?;let b=proposal.world_transform(expected)?;
    let (Ok(sa),Ok(sb))=(super::path_text::similarity_scale(a),super::path_text::similarity_scale(b))else{return Ok(false)};
    if actual.style.stroke_linecap!=expected.style.stroke_linecap||actual.style.stroke_linejoin!=expected.style.stroke_linejoin{return Ok(false);}
    let pad=|o:&Object|if o.style.stroke=="none"{0.}else{o.style.stroke_width_mm*o.style.stroke_miterlimit.max(1.)};let(pa,pb)=(pad(actual)*sa,pad(expected)*sb);
    // Each curve stays in its control hull; the conservative miter/cap box is
    // scaled by the same positive similarity as its circular pen. Compare
    // world box vertices so changing the local unit rotation cannot fail a
    // physically identical round pen. Never
    // spend a separate geometry tolerance and another independent pen budget.
    let close=|p:Point,q:Point|[[-1.,-1.],[1.,-1.],[1.,1.],[-1.,1.]].into_iter().all(|v|{
        let p=a*p+kurbo::Vec2::new(pa*v[0],pa*v[1]);let q=b*q+kurbo::Vec2::new(pb*v[0],pb*v[1]);
        (p.x-q.x).abs()<=0.00021&&(p.y-q.y).abs()<=0.00021
    });
    Ok(left.outline.elements().iter().zip(right.outline.elements()).all(|(a,b)|match(*a,*b){
        (PathEl::MoveTo(p),PathEl::MoveTo(q))|(PathEl::LineTo(p),PathEl::LineTo(q))=>close(p,q),
        (PathEl::QuadTo(p,q),PathEl::QuadTo(r,s))=>close(p,r)&&close(q,s),
        (PathEl::CurveTo(p,q,r),PathEl::CurveTo(s,t,u))=>close(p,s)&&close(q,t)&&close(r,u),
        (PathEl::ClosePath,PathEl::ClosePath)=>true,_=>false,
    }))
}

pub(super) fn check_encoding(source:&Document,bytes:&[u8])->Result<()>{
    let mut ids=Vec::new();
    for o in &source.objects{if needs_check(source,o)?{ids.push(&o.id);}}
    if ids.is_empty(){return Ok(());}
    // Keep the native resource catalogue so unrelated lossless CMYK/images
    // remain readable, while disabling source coordinate/font restoration.
    let actual=super::read::decode_geometry(bytes,"native-pen-check")?.0;
    for id in ids{if !preserves(&actual,actual.object(id)?,source,source.object(id)?)?{return Err(super::unsupported(format!("{id}: native fitted glyph and outline exceed the joint 0.00021 mm bound")));}}
    Ok(())
}
