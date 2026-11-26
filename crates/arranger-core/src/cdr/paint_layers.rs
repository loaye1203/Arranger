//! Real fill/outline drawing layers for paint combinations that one native
//! transparency effect cannot express. Notes bind identities and native geometry digests; the two
//! editable native geometries, paints, hierarchy and order own every edit.
use super::{invalid,unsupported,Result};
use crate::{Document,Geometry,Object,Style,command::Notice};
use kurbo::Affine;
use serde::{Serialize,Deserialize};
use std::collections::{BTreeMap,BTreeSet};
const PREFIX:&str="ArrangerPaintLayersV1|sha256:";
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Layers{version:u32,objects:BTreeMap<String,Pair>}
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct Pair{fill:String,outline:String,fill_geometry_sha256:String,outline_geometry_sha256:String}
fn alpha(paint:&str)->Result<bool>{Ok(crate::paint::parse(paint)?.alpha()<1.)}
fn eligible(g:&Geometry)->bool{matches!(g,Geometry::Rect{..}|Geometry::Ellipse|Geometry::Path{..}|Geometry::Text{text_path:None,..})}
fn needed(o:&Object)->Result<bool>{
    if o.style.fill=="none"||o.style.stroke=="none"||!eligible(&o.geometry){return Ok(false);}
    let(f,s)=(alpha(&o.style.fill)?,alpha(&o.style.stroke)?);
    Ok((f||s)&&(f&&s||o.style.opacity!=1.||o.clip.is_some()))
}
fn generated(ids:&mut BTreeSet<String>,owner:&str,role:&str)->String{
    let base=format!("{}-paint-{role}",&owner[..owner.len().min(96)]);let mut id=base.clone();let mut n=0;
    while !ids.insert(id.clone()){n+=1;id=format!("{base}-{n}");}id
}
pub(super) fn expand(doc:&Document)->Result<(Document,Option<Layers>)>{
    doc.validate()?;let mut result=doc.clone();result.objects.clear();let mut ids=doc.objects.iter().map(|o|o.id.clone()).collect::<BTreeSet<_>>();let mut pairs=BTreeMap::new();
    for o in &doc.objects{
        if !needed(o)?{result.objects.push(o.clone());continue;}
        let fill=generated(&mut ids,&o.id,"fill");let outline=generated(&mut ids,&o.id,"outline");
        let mut group=o.clone();group.geometry=Geometry::Group;group.style=Style{opacity:o.style.opacity,..Default::default()};
        let layer=|id:String,paint_fill:bool|{let mut layer=o.clone();layer.id=id;layer.parent_id=Some(o.id.clone());layer.x_mm=0.;layer.y_mm=0.;layer.transform=[1.,0.,0.,1.,0.,0.];layer.clip=None;layer.visible=true;layer.locked=false;layer.style.opacity=1.;if paint_fill{layer.style.stroke="none".into();}else{layer.style.fill="none".into();}layer};
        result.objects.extend([group,layer(fill.clone(),true),layer(outline.clone(),false)]);pairs.insert(o.id.clone(),Pair{fill,outline,fill_geometry_sha256:String::new(),outline_geometry_sha256:String::new()});
    }
    result.validate()?;Ok((result,(!pairs.is_empty()).then_some(Layers{version:1,objects:pairs})))
}
pub(super) fn encode(doc:&Document,modern:bool)->Result<(Vec<u8>,Vec<Notice>)>{
    let(expanded,layers)=expand(doc)?;let(raw,mut warnings)=if modern{super::write21::encode(&expanded)}else{super::write::encode(&expanded)}?;
    let Some(mut layers)=layers else{return Ok((raw,warnings));};
    let parsed=super::binary::read_native(&raw)?;let signatures=signatures(&parsed.nodes)?;
    for pair in layers.objects.values_mut(){pair.fill_geometry_sha256=crate::model::sha256(signatures.get(&pair.fill).and_then(Option::as_ref).ok_or_else(||invalid("encoded fill layer identity missing"))?);pair.outline_geometry_sha256=crate::model::sha256(signatures.get(&pair.outline).and_then(Option::as_ref).ok_or_else(||invalid("encoded outline layer identity missing"))?);}
    let mut members=parsed.members;let mut order=members.keys().cloned().collect::<Vec<_>>();if !modern{order.retain(|k|k!="mimetype");order.insert(0,"mimetype".into());}
    let extra=super::metadata::encode(PREFIX,&layers)?;
    let metadata=super::metadata::document_with_extra(&expanded,modern,Some(extra))?.ok_or_else(||invalid("paint layer Notes missing"))?;
    members.insert(if modern{"META-INF/metadata.xml"}else{"metadata/metadata.xml"}.into(),metadata);
    warnings.push(Notice::new("CDR_PAINT_LAYERS","Separate native fill and outline objects are grouped to preserve independent alpha and optional isolated group opacity. Both geometries remain editable. Matching layer pairs fold on import; separate native edits or changed stacking remain explicit editable layers. Full CDR save remains unavailable."));
    Ok((super::binary::zip(&order.iter().map(|k|(k.as_str(),members[k].as_slice())).collect::<Vec<_>>()),warnings))
}
pub(super) fn read(members:&BTreeMap<String,Vec<u8>>)->Result<Option<Layers>>{super::metadata::find(members,PREFIX)?.map(|s|super::metadata::decode(PREFIX,&s)).transpose()}
#[derive(Default)]
pub(super) struct NativeProof{can_fold:BTreeSet<String>,prefer_native:BTreeSet<String>}
fn semantics(doc:&Document)->Result<BTreeMap<String,String>>{
    use kurbo::{PathEl,Point};let mut result=BTreeMap::new();
    for o in doc.objects.iter().filter(|o|eligible(&o.geometry)){
        let m=doc.world_transform(o)?;let mut bytes=vec![];let mut numbers=vec![];
        match &o.geometry{
            Geometry::Text{content,font_family,font_size_mm,layout,..}=>{
                bytes.extend(serde_json::to_vec(&("text",content,font_family,layout.mode,layout.align)).map_err(|e|invalid(e.to_string()))?);
                let[a,b,c,d,x,y]=m.as_coeffs();let size=*font_size_mm;let leading=layout.line_height_mm.unwrap_or(size*1.2);numbers.extend([x,y,a*size,b*size,c*size,d*size,c*leading,d*leading,a*layout.letter_spacing_mm,b*layout.letter_spacing_mm]);
                if layout.mode==crate::TextMode::Area{for p in [[0.,0.],[o.width_mm,0.],[o.width_mm,o.height_mm],[0.,o.height_mm]]{let q=m*Point::new(p[0],p[1]);numbers.extend([q.x,q.y]);}}
            },
            _=>{
                bytes.push(match o.geometry{Geometry::Rect{..}=>1,Geometry::Ellipse=>2,_=>3});
                let path=m*doc.object_path(o)?;
                for e in path.elements(){match *e{PathEl::MoveTo(p)=>{bytes.push(0);numbers.extend([p.x,p.y]);},PathEl::LineTo(p)=>{bytes.push(1);numbers.extend([p.x,p.y]);},PathEl::QuadTo(a,b)=>{bytes.push(2);numbers.extend([a.x,a.y,b.x,b.y]);},PathEl::CurveTo(a,b,c)=>{bytes.push(3);numbers.extend([a.x,a.y,b.x,b.y,c.x,c.y]);},PathEl::ClosePath=>bytes.push(4)}}
            }
        }
        for n in numbers{if !n.is_finite(){return Err(invalid("nonfinite native paint semantic coordinate"));}bytes.extend((if n==0.{0.}else{n}).to_le_bytes());}result.insert(o.id.clone(),crate::model::sha256(&bytes));
    }Ok(result)
}
fn signatures(nodes:&[super::binary::Node])->Result<BTreeMap<String,Option<Vec<u8>>>>{
    let mut result=BTreeMap::new();
    for node in super::binary::all(nodes).into_iter().filter(|n|n.kind()==Some(b"obj ")){
        let(kind,args)=super::write::arguments(&super::read::own_record(node,b"loda")?.data)?;let Some((_,name))=args.iter().find(|(k,_)|*k==1000)else{continue};let name=super::binary::read_wide(name)?;
        let geometry=&args.iter().find(|(k,_)|*k==30).ok_or_else(||invalid("paint layer geometry record missing"))?.1;let matrix=super::read::matrix(node)?;let mut signature=kind.to_le_bytes().to_vec();signature.extend((geometry.len() as u64).to_le_bytes());signature.extend(geometry);for v in matrix{signature.extend((if v==0.{0.}else{v}).to_le_bytes());}
        use std::collections::btree_map::Entry;match result.entry(name){Entry::Vacant(e)=>{e.insert(Some(signature));},Entry::Occupied(mut e)=>{e.insert(None);}}
    }Ok(result)
}
pub(super) fn native_proof(nodes:&[super::binary::Node],layers:Option<&Layers>)->Result<NativeProof>{
    let Some(layers)=layers else{return Ok(NativeProof::default())};let signatures=signatures(nodes)?;let mut proof=NativeProof::default();
    for(id,pair)in &layers.objects{
        let valid=|s:&str|s.len()==64&&s.bytes().all(|b|b.is_ascii_hexdigit());if !valid(&pair.fill_geometry_sha256)||!valid(&pair.outline_geometry_sha256){return Err(invalid("invalid paint layer geometry digest"));}
        let(a,b)=(signatures.get(&pair.fill).and_then(Option::as_ref),signatures.get(&pair.outline).and_then(Option::as_ref));let unchanged=a.is_some_and(|a|crate::model::sha256(a)==pair.fill_geometry_sha256)&&b.is_some_and(|b|crate::model::sha256(b)==pair.outline_geometry_sha256);
        if unchanged||a.is_some()&&a==b{proof.can_fold.insert(id.clone());}
        // Compare before source frame restoration. Even a one-grid or smaller
        // native move must never disappear inside the general .00021 budget.
        if !unchanged{proof.prefer_native.extend([id.clone(),pair.fill.clone(),pair.outline.clone()]);}
    }Ok(proof)
}
pub(super) fn prefer_native_charts(doc:&Document,frames:&mut super::local_frames::Frames,proof:&NativeProof){super::local_frames::prefer_native(doc,frames,&proof.prefer_native);}
pub(super) fn reconcile_charts(doc:&Document,frames:&mut super::local_frames::Frames,proof:&mut NativeProof,layers:Option<&Layers>,widths:&BTreeMap<String,f64>,units:&BTreeMap<String,u32>)->Result<()>{
    if let Some(layers)=layers{
        let mut ids=BTreeSet::new();for(id,p)in &layers.objects{for key in [id,&p.fill,&p.outline]{let mut current=Some(key.as_str());while let Some(id)=current{ids.insert(id.to_owned());current=doc.object(id).ok().and_then(|o|o.parent_id.as_deref());}}}
        // Only vector/text members and ancestor groups are copied. Original
        // image payloads are irrelevant to this proof and are never cloned.
        let mut candidate=Document{id:doc.id.clone(),unit:doc.unit.clone(),pages:doc.pages.clone(),resources:Default::default(),objects:doc.objects.iter().filter(|o|ids.contains(&o.id)).cloned().collect()};
        let before=semantics(&candidate)?;let original=candidate.clone();
        super::local_frames::restore_with_units(&mut candidate,frames.clone(),widths,units)?;let after=semantics(&candidate)?;
        for(id,p)in &layers.objects{
            let unchanged=[&p.fill,&p.outline].into_iter().all(|key|before.get(key).is_some()&&before.get(key)==after.get(key));
            let equal=before.get(&p.fill).is_some()&&before.get(&p.fill)==before.get(&p.outline);
            if unchanged&&equal{proof.can_fold.insert(id.clone());for key in [id,&p.fill,&p.outline]{proof.prefer_native.remove(key);}}
        }
        // A genuine ancestor move must also survive the normal frame pass.
        let mut ancestors=BTreeSet::new();for id in &proof.prefer_native{let mut current=original.object(id).ok().and_then(|o|o.parent_id.as_deref());while let Some(id)=current{ancestors.insert(id.to_owned());current=original.object(id).ok().and_then(|o|o.parent_id.as_deref());}}proof.prefer_native.extend(ancestors);
    }
    prefer_native_charts(doc,frames,proof);Ok(())
}
#[cfg(test)]
pub(super) fn restore(doc:&mut Document,layers:Layers)->Result<(usize,usize)>{let proof=NativeProof{can_fold:layers.objects.keys().cloned().collect(),prefer_native:BTreeSet::new()};restore_with_proof(doc,layers,proof)}
pub(super) fn restore_with_proof(doc:&mut Document,layers:Layers,proof:NativeProof)->Result<(usize,usize)>{
    if layers.version!=1{return Err(unsupported("unknown paint-layer binding version"));}
    let mut owned=BTreeSet::new();
    for(id,pair)in &layers.objects{for key in [id,&pair.fill,&pair.outline]{if !crate::model::valid_id(key)||!owned.insert(key.clone()){return Err(invalid("invalid or aliased paint-layer owner"));}}}
    let(mut folded,mut retained)=(0,0);
    for(id,pair)in layers.objects{
        if !proof.can_fold.contains(&id){retained+=1;continue;}
        let(Some(group),Some(fill),Some(outline))=(doc.objects.iter().find(|o|o.id==id).cloned(),doc.objects.iter().find(|o|o.id==pair.fill).cloned(),doc.objects.iter().find(|o|o.id==pair.outline).cloned())else{retained+=1;continue};
        let children=doc.objects.iter().filter(|o|o.parent_id.as_deref()==Some(&id)).map(|o|o.id.as_str()).collect::<Vec<_>>();
        let neutral=Style{opacity:group.style.opacity,..Default::default()};
        if !matches!(group.geometry,Geometry::Group)||group.style!=neutral||children!=[pair.fill.as_str(),pair.outline.as_str()]||fill.page_id!=group.page_id||outline.page_id!=group.page_id
            ||!fill.visible||!outline.visible||fill.locked||outline.locked||fill.clip.is_some()||outline.clip.is_some()
            ||fill.geometry!=outline.geometry||!eligible(&fill.geometry)
            ||[fill.x_mm,fill.y_mm,fill.width_mm,fill.height_mm]!=[outline.x_mm,outline.y_mm,outline.width_mm,outline.height_mm]||fill.transform!=outline.transform
            ||fill.style.fill=="none"||fill.style.stroke!="none"||outline.style.fill!="none"||outline.style.stroke=="none"||fill.style.opacity!=1.||outline.style.opacity!=1.
            ||doc.objects.iter().any(|o|o.parent_id.as_deref()==Some(&pair.fill)||o.parent_id.as_deref()==Some(&pair.outline)) {retained+=1;continue;}
        let mut object=group.clone();object.geometry=fill.geometry;object.width_mm=fill.width_mm;object.height_mm=fill.height_mm;object.style=outline.style;object.style.fill=fill.style.fill;object.style.opacity=group.style.opacity;
        if fill.x_mm!=0.||fill.y_mm!=0.||fill.transform!=[1.,0.,0.,1.,0.,0.]{object.transform=(Affine::new(group.transform)*Affine::translate((group.x_mm,group.y_mm))*Affine::new(fill.transform)*Affine::translate((fill.x_mm,fill.y_mm))).as_coeffs();object.x_mm=0.;object.y_mm=0.;}
        *doc.objects.iter_mut().find(|o|o.id==id).unwrap()=object;doc.objects.retain(|o|o.id!=pair.fill&&o.id!=pair.outline);doc.validate()?;folded+=1;
    }Ok((folded,retained))
}

#[cfg(test)]
mod tests{
 use super::*;
 fn source()->Document{serde_json::from_value(serde_json::json!({"id":"layers","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"shape","page_id":"page-1","x_mm":40.,"y_mm":60.,"width_mm":60.,"height_mm":40.,"geometry":{"kind":"rect","radius_mm":0.75},"style":{"fill":"#1e5ab480","stroke":"#dc502840","stroke_width_mm":12.,"opacity":1.}}]})).unwrap()}
 #[test]
 fn binding_contains_only_distinct_generated_identities_and_handles_name_collisions(){
  let mut source=source();let mut collision=source.objects[0].clone();collision.id="shape-paint-fill".into();collision.style.stroke="none".into();source.objects.push(collision);
  let(expanded,layers)=expand(&source).unwrap();let value=serde_json::to_value(layers.unwrap()).unwrap();assert_eq!(value["objects"]["shape"]["fill"],"shape-paint-fill-1");assert_eq!(value["objects"]["shape"].as_object().unwrap().len(),4);assert_eq!(expanded.objects.len(),4);assert!(expanded.objects.iter().filter(|o|o.parent_id.as_deref()==Some("shape")).all(|o|!matches!(o.geometry,Geometry::Group)));
 }
 #[test]
 fn changed_native_geometry_order_ownership_and_layer_modes_never_restore_old_content(){
  for mode in 0..12{let(mut native,layers)=expand(&source()).unwrap();let layers=layers.unwrap();let pair=&layers.objects["shape"];let fill=pair.fill.clone();let outline=pair.outline.clone();
   match mode{
    0=>native.objects[1].x_mm+=1.,1=>native.objects[1].width_mm+=2.,2=>native.objects.swap(1,2),3=>native.objects[1].visible=false,4=>native.objects[2].locked=true,
    5=>native.objects[2].geometry=Geometry::Ellipse,6=>native.objects[2].style.opacity=0.5,7=>native.objects[1].parent_id=None,
    8=>{let mut extra=native.objects[1].clone();extra.id="new-native".into();native.objects.push(extra);},9=>native.objects.retain(|o|o.id!=fill),10=>native.objects[0].style.fill="#abcdef".into(),_=>native.objects[1].style.stroke="#000000".into(),
   }
   native.validate().unwrap();let expected=native.clone();assert_eq!(restore(&mut native,layers).unwrap(),(0,1),"mode{mode}");assert_eq!(native,expected);assert!(native.objects.iter().any(|o|o.id==outline));
  }
 }
 #[test]
 fn shared_native_moves_recolors_and_group_opacity_survive_the_fold(){
  let(mut doc,layers)=expand(&source()).unwrap();for child in &mut doc.objects[1..]{child.x_mm=3.;child.y_mm=4.;}doc.objects[0].style.opacity=0.3;doc.objects[1].style.fill="#abcdefff".into();doc.objects[2].style.stroke="#12345680".into();
  let before=doc.world_transform(&doc.objects[1]).unwrap();assert_eq!(restore(&mut doc,layers.unwrap()).unwrap(),(1,0));let o=&doc.objects[0];assert_eq!(doc.world_transform(o).unwrap(),before);assert_eq!(o.style.fill,"#abcdefff");assert_eq!(o.style.stroke,"#12345680");assert_eq!(o.style.opacity,0.3);
 }
 #[test]
 fn seven_actual_corel_returns_recover_source_charts_without_touching_their_bytes(){
  let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");let rows:serde_json::Value=serde_json::from_slice(&std::fs::read(root.join("examples/assets/cdr/corel2019/paint-layer-returns-20261030/manifest.json")).unwrap()).unwrap();
  for row in rows.as_array().unwrap(){let original=std::fs::read(root.join(row["original"].as_str().unwrap())).unwrap();let returned=std::fs::read(root.join(row["file"].as_str().unwrap())).unwrap();let expected=crate::cdr::decode(&original,"return").unwrap().0;let actual=crate::cdr::decode(&returned,"return").unwrap().0;assert_eq!(actual.object("content").unwrap(),expected.object("content").unwrap(),"{}",row["name"]);}
 }
}
#[cfg(test)]
mod rendering_tests{
 use super::*;
 #[test]
 fn real_expanded_layers_render_the_same_intersections_without_folding_notes(){
  let renderer=crate::render::Renderer::new();
  for opacity in [1.,0.5]{let doc:Document=serde_json::from_value(serde_json::json!({"id":"layers","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"shape","page_id":"page-1","x_mm":40.,"y_mm":60.,"width_mm":60.,"height_mm":40.,"geometry":{"kind":"rect","radius_mm":0.},"style":{"fill":"#1e5ab480","stroke":"#dc502840","stroke_width_mm":12.,"opacity":opacity}}]})).unwrap();
   let(expanded,_)=expand(&doc).unwrap();let a=renderer.render(&doc,"page-1",127.,None).unwrap();let b=renderer.render(&expanded,"page-1",127.,None).unwrap();
   assert_eq!(a.pixels.data(),b.pixels.data(),"actual split-layer composition differs at opacity{opacity}");
  }
 }
}
#[cfg(test)]
mod native_edit_tests{
 use super::*;use super::super::binary::*;
 #[test]
 fn one_grid_and_subgrid_native_layer_moves_survive_before_any_source_chart_recovery(){
  let source:Document=serde_json::from_value(serde_json::json!({"id":"tiny-native-edit","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"shape","page_id":"page-1","x_mm":40.,"y_mm":60.,"width_mm":60.,"height_mm":40.,"geometry":{"kind":"rect","radius_mm":0.75},"style":{"fill":"#1e5ab480","stroke":"#dc502840","stroke_width_mm":12.,"opacity":1.}}]})).unwrap();
  for version in [14,21]{for both in [false,true]{for delta in [0.0001,0.00000001]{let(raw,_)=encode(&source,version==21).unwrap();let mut native=read_native(&raw).unwrap();
   fn move_nodes(ns:&mut[Node],both:bool,delta:f64){for n in ns{if n.kind()==Some(b"obj "){let(_,a)=super::super::write::arguments(&super::super::read::own_record(n,b"loda").unwrap().data).unwrap();let name=read_wide(&a.iter().find(|(k,_)|*k==1000).unwrap().1).unwrap();if name=="shape-paint-fill"||both&&name=="shape-paint-outline"{let tr=&mut n.find_mut(b"trfd").unwrap().data;let at=u32_at(tr,u32_at(tr,8).unwrap()as usize).unwrap()as usize+16+16;let v=f64_at(tr,at).unwrap()+delta*10000.;tr[at..at+8].copy_from_slice(&v.to_le_bytes());for key in [b"bbox",b"obbx"]{let data=&mut n.find_mut(key).unwrap().data;for at in (0..data.len()).step_by(8){let v=i32_at(data,at).unwrap()+(delta*10000.).round_ties_even()as i32;data[at..at+4].copy_from_slice(&v.to_le_bytes());}}}}if let Some(c)=&mut n.children{move_nodes(c,both,delta);}}}
   move_nodes(&mut native.nodes,both,delta);let bytes=if version==21{repack21(&native).unwrap()}else{native.members.insert("content/riffData.cdr".into(),write(&native.nodes).unwrap());zip(&native.members.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>())};let doc=crate::cdr::decode(&bytes,&source.id).unwrap().0;assert_eq!(doc.objects.len(),if both{1}else{3});let target=doc.object(if both{"shape"}else{"shape-paint-fill"}).unwrap();let x=(doc.world_transform(target).unwrap()*kurbo::Point::ZERO).x;assert!((x-40.-delta).abs()<1e-10,"native{version}/{both}/{delta}: {x}");
  }}}
 }
}
