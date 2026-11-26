//! Experimental cyclic-start source charts. Native geometry is authoritative.
//! Notes carry identities, a segment shift, an Own matrix, coordinate-free
//! notation and digests; never a document or a guide/glyph coordinate backup.
use super::{binary::*,invalid,unsupported,Result};
use crate::{Document,Geometry,command::Notice};
use kurbo::BezPath;
use serde::{Deserialize,Serialize};
use std::collections::{BTreeMap,BTreeSet};

const PREFIX:&str="ArrangerCyclicSourceV1|sha256:";
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Entry{
    version:u32,text_id:String,guide_id:String,turn:u8,shift:usize,
    own:[f64;6],notation:super::path_notation::Notation,
    native_sha256:String,carrier_sha256:String,
}

// Keep the exact legacy single-story serialization. Batch v2 is deliberately
// rejected by old readers rather than silently losing additional Own charts.
#[derive(Serialize,Deserialize)]
#[serde(untagged)]
pub(super) enum Contract{Single(Entry),Batch(Batch)}
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Batch{version:u32,stories:BTreeMap<String,Entry>}
impl Contract{
    fn entries(&self)->Box<dyn Iterator<Item=&Entry>+'_>{match self{Self::Single(e)=>Box::new(std::iter::once(e)),Self::Batch(b)=>Box::new(b.stories.values())}}
    fn entries_mut(&mut self)->Box<dyn Iterator<Item=&mut Entry>+'_>{match self{Self::Single(e)=>Box::new(std::iter::once(e)),Self::Batch(b)=>Box::new(b.stories.values_mut())}}
    fn validate(&self)->Result<()>{
        match self{Self::Single(e)if e.version==1=>{},Self::Batch(b)if b.version==2&&b.stories.len()>=2&&b.stories.len()<=4096&&b.stories.iter().all(|(id,e)|id==&e.text_id&&e.version==2)=>{},_=>return Err(invalid("unknown cyclic source version/count/identity"))}
        let mut guides=BTreeSet::new();
        for e in self.entries(){if !guides.insert(&e.guide_id)||e.text_id==e.guide_id{return Err(invalid("cyclic source bindings share an endpoint"));}}
        Ok(())
    }
}

fn path(doc:&Document,id:&str)->Result<BezPath>{
    let Geometry::Path{d,..}=&doc.object(id)?.geometry else{return Err(unsupported("cyclic source needs one editable path"))};
    crate::path::parse(d)
}
fn rephase(path:&BezPath,shift:usize)->Result<BezPath>{
    let segments=super::path_rotation::pieces(path).ok_or_else(||unsupported("cyclic source needs one closed contour"))?;
    if shift>=segments.len(){return Err(invalid("cyclic source shift exceeds segment count"));}
    let mut output=BezPath::new();let start=segments[shift][0];output.move_to(start);
    for (i,s) in segments[shift..].iter().chain(&segments[..shift]).enumerate(){
        match s.as_slice(){
            [_,p] if i==segments.len()-1&&*p==start=>{},
            [_,p]=>output.line_to(*p),[_,a,p]=>output.quad_to(*a,*p),
            [_,a,b,p]=>output.curve_to(*a,*b,*p),_=>return Err(invalid("cyclic segment arity differs")),
        }
    }output.close_path();Ok(output)
}
// Bind both endpoints and all their ancestor records. An unrelated marker may
// still be edited/reordered. Additional users of the guide invalidate recovery.
fn fingerprint(doc:&Document,text:&str,guide:&str)->Result<Option<String>>{fingerprint_scoped(doc,text,guide,false)}
fn fingerprint_scoped(doc:&Document,text:&str,guide:&str,page_scoped:bool)->Result<Option<String>>{
    fingerprint_with_cache(doc,text,guide,page_scoped,None)
}
fn fingerprint_with_cache(doc:&Document,text:&str,guide:&str,page_scoped:bool,cache:Option<[f64;2]>)->Result<Option<String>>{
    if doc.object(text).is_err()||doc.object(guide).is_err(){return Ok(None);}
    let mut selected=BTreeSet::new();
    for id in[text,guide]{let mut current=Some(id);let mut depth=0;while let Some(id)=current{
        depth+=1;if depth>17{return Err(invalid("cyclic source ancestor depth exceeded"));}
        selected.insert(id.to_owned());let object=doc.object(id)?;current=object.parent_id.as_deref();
    }}
    let replacement=cache.map(|[width,height]|{let mut object=doc.object(text)?.clone();object.width_mm=width;object.height_mm=height;Ok::<_,crate::Error>(object)}).transpose()?;
    let objects=selected.iter().map(|id|doc.object(id).map(|o|(id,if id==text{replacement.as_ref().unwrap_or(o)}else{o}))).collect::<Result<BTreeMap<_,_>>>()?;
    let users=doc.objects.iter().filter_map(|o|match &o.geometry{Geometry::Text{text_path:Some(p),..}if p.path_id==guide=>Some(o.id.as_str()),_=>None}).collect::<BTreeSet<_>>();
    let bytes=if page_scoped{
        let pages=objects.values().map(|o|o.page_id.as_str()).collect::<BTreeSet<_>>();
        let relevant=doc.pages.iter().filter(|p|pages.contains(p.id.as_str())).collect::<Vec<_>>();
        serde_json::to_vec(&(&doc.unit,relevant,objects,users))
    }else{serde_json::to_vec(&(&doc.unit,&doc.pages,objects,users))}.map_err(|e|invalid(e.to_string()))?;
    Ok(Some(crate::model::sha256(&bytes)))
}
fn plan_one(requested:&Document,carrier:&Document)->Result<Option<Entry>>{
    requested.validate()?;carrier.validate()?;
    let mut equal=carrier.clone();equal.id=requested.id.clone();if &equal==requested{return Ok(None);}
    let texts=requested.objects.iter().filter(|o|matches!(o.geometry,Geometry::Text{text_path:Some(_),..})&&carrier.object(&o.id).is_ok_and(|c|c.transform!=o.transform)).collect::<Vec<_>>();
    if texts.len()!=1{return Err(unsupported("cyclic research requires exactly one changed live story"));}
    let text=texts[0];let Geometry::Text{text_path:Some(binding),..}=&text.geometry else{unreachable!()};
    let turn=super::path_text::own_turn(text).ok_or_else(||unsupported("cyclic research requires an exact own quarter turn"))?;
    let delta=super::path_text::translation_world(requested,text)?.ok_or_else(||invalid("missing cyclic world placement"))?;
    let source=path(requested,&binding.path_id)?;let target=path(carrier,&binding.path_id)?;
    let guide=requested.object(&binding.path_id)?;
    let shift=super::path_rotation::shift(&source,requested.world_transform(guide)?,turn,delta).ok_or_else(||unsupported("cyclic research requires exact directed control symmetry"))?;
    let wanted=rephase(&source,shift)?;
    if super::path_rotation::pieces(&wanted)!=super::path_rotation::pieces(&target){return Err(unsupported("cyclic carrier changes guide segments or controls"));}
    let actual=carrier.object(&text.id)?;
    if actual.transform!=[1.,0.,0.,1.,-text.x_mm,-text.y_mm]{return Err(unsupported("cyclic carrier needs a neutral own placement"));}
    let users=requested.objects.iter().filter(|o|matches!(&o.geometry,Geometry::Text{text_path:Some(p),..}if p.path_id==binding.path_id)).count();
    if users!=1{return Err(unsupported("cyclic guide is shared by another story"));}
    // No hidden differences in content, style, hierarchy, resources or pages.
    let mut expected=requested.clone();expected.id=carrier.id.clone();
    expected.objects.iter_mut().find(|o|o.id==text.id).unwrap().transform=actual.transform;
    let Geometry::Path{d:target_d,..}=&carrier.object(&guide.id)?.geometry else{return Err(unsupported("cyclic carrier guide changed type"))};
    let Geometry::Path{d:expected_d,..}=&mut expected.objects.iter_mut().find(|o|o.id==guide.id).unwrap().geometry else{unreachable!()};*expected_d=target_d.clone();
    if &expected!=carrier{return Err(unsupported("cyclic carrier differs outside its start node and own matrix"));}
    let Geometry::Path{d,..}=&guide.geometry else{unreachable!()};
    let notation=super::path_notation::Notation::from_source(d)?.ok_or_else(||unsupported("cyclic source notation cannot be recovered from native coordinates"))?;
    let inverse=rephase(&target,(super::path_rotation::pieces(&target).unwrap().len()-shift)%super::path_rotation::pieces(&target).unwrap().len())?;
    if notation.restore(&inverse.to_svg())?.as_deref()!=Some(d.as_str()){return Err(unsupported("cyclic inverse cannot reproduce source notation"));}
    Ok(Some(Entry{version:1,text_id:text.id.clone(),guide_id:guide.id.clone(),turn,shift,own:text.transform,notation,native_sha256:String::new(),carrier_sha256:fingerprint(carrier,&text.id,&guide.id)?.unwrap()}))
}

fn pair_scene(doc:&Document,id:&str)->Result<Document>{
    let mut scene=super::path_outline::scene(doc,id)?;
    let pages=scene.objects.iter().map(|o|o.page_id.as_str()).collect::<BTreeSet<_>>();
    scene.pages.retain(|p|pages.contains(p.id.as_str()));scene.validate()?;Ok(scene)
}
fn plan(requested:&Document,carrier:&Document)->Result<Option<Contract>>{
    requested.validate()?;carrier.validate()?;
    let changed=requested.objects.iter().filter(|o|matches!(o.geometry,Geometry::Text{text_path:Some(_),..})&&carrier.object(&o.id).is_ok_and(|c|c.transform!=o.transform)).collect::<Vec<_>>();
    if changed.len()<2{return plan_one(requested,carrier).map(|v|v.map(Contract::Single));}
    if changed.len()>4096{return Err(unsupported("cyclic batch exceeds4096 independent stories"));}
    let mut expected=requested.clone();expected.id=carrier.id.clone();let mut stories=BTreeMap::new();let mut guides=BTreeSet::new();
    let mut users=BTreeMap::<&str,usize>::new();for o in &requested.objects{if let Geometry::Text{text_path:Some(p),..}=&o.geometry{*users.entry(&p.path_id).or_default()+=1;}}
    for text in changed{
        let Geometry::Text{text_path:Some(binding),..}=&text.geometry else{unreachable!()};
        if users.get(binding.path_id.as_str())!=Some(&1)||!guides.insert(&binding.path_id){return Err(unsupported("cyclic batch needs exclusive guide ownership"));}
        let mut entry=plan_one(&pair_scene(requested,&text.id)?,&pair_scene(carrier,&text.id)?)?.ok_or_else(||invalid("cyclic batch contains a neutral binding"))?;
        entry.version=2;entry.carrier_sha256=fingerprint_scoped(carrier,&entry.text_id,&entry.guide_id,true)?.unwrap();
        expected.objects.iter_mut().find(|o|o.id==text.id).unwrap().transform=carrier.object(&text.id)?.transform;
        let Geometry::Path{d:new,..}=&carrier.object(&binding.path_id)?.geometry else{return Err(unsupported("cyclic batch guide type changed"))};
        let Geometry::Path{d:old,..}=&mut expected.objects.iter_mut().find(|o|o.id==binding.path_id).ok_or_else(||invalid("cyclic guide missing"))?.geometry else{unreachable!()};*old=new.clone();
        stories.insert(text.id.clone(),entry);
    }
    if &expected!=carrier{return Err(unsupported("cyclic batch differs outside its independent Own/start charts"));}
    Ok(Some(Contract::Batch(Batch{version:2,stories})))
}

pub(super) fn encode(requested:&Document,carrier:&Document,modern:bool)->Result<(Vec<u8>,Vec<Notice>)>{
    let contract=plan(requested,carrier)?;
    let (bytes,mut warnings)=if modern{super::encode_document21(carrier)?}else{super::encode_document(carrier)?};
    let Some(mut contract)=contract else{return Ok((bytes,warnings))};contract.validate()?;
    let bare=super::read::decode_geometry(&bytes,"cyclic-native")?.0;
    for entry in contract.entries_mut(){entry.native_sha256=fingerprint_scoped(&bare,&entry.text_id,&entry.guide_id,entry.version==2)?.ok_or_else(||invalid("cyclic native endpoints missing"))?;}
    // Verify the current normal reader produces the carrier before attaching
    // a different source chart; do not make a second restoration hide a failure.
    let decoded=super::read::decode(&bytes,&carrier.id)?.0;
    if decoded!=*carrier{return Err(unsupported("cyclic native carrier does not round-trip completely"));}
    let notes=super::metadata::encode(PREFIX,&contract)?;
    let mut native=read_native(&bytes)?;
    // Preserve every other Notes section, including paint-layer bindings on
    // unrelated objects. Rebuilding generic XMP would silently discard them.
    let old=super::metadata::find(&native.members,"ArrangerLocalFramesV1|sha256:")?.ok_or_else(||invalid("cyclic source requires native local frames"))?;
    let frames=super::local_frames::cyclic_notes(&old,&notes)?;
    let key=if modern{"META-INF/metadata.xml"}else{"metadata/metadata.xml"};
    let xml=std::str::from_utf8(native.members.get(key).ok_or_else(||invalid("cyclic XMP missing"))?).map_err(|_|invalid("cyclic XMP is not UTF8"))?;
    if xml.matches(&old).count()!=1{return Err(invalid("cyclic frame section is not unique"));}
    let xml=xml.replacen(&old,&format!("{frames};{notes}"),1);native.members.insert(key.into(),xml.into_bytes());
    let mut order=native.members.keys().cloned().collect::<Vec<_>>();
    if !modern{order.retain(|k|k!="mimetype");order.insert(0,"mimetype".into());}
    let bytes=zip(&order.iter().map(|k|(k.as_str(),native.members[k].as_slice())).collect::<Vec<_>>());
    if super::read::decode(&bytes,&requested.id)?.0!=*requested{return Err(unsupported("cyclic source contract failed complete final recovery"));}
    warnings.push(Notice::new("CDR_CYCLIC_RESEARCH","Research reproducer only: Windows report148 found that Corel does not apply the requested rotation and changed-start source recovery fails after resave. Keep those actual failures; ordinary CDR export is unchanged."));
    Ok((bytes,warnings))
}
pub(super) fn read(members:&BTreeMap<String,Vec<u8>>,frames:&mut Option<super::local_frames::Frames>)->Result<Option<Contract>>{
    let notes=super::metadata::find(members,PREFIX)?;
    super::local_frames::unseal_cyclic(frames,notes.as_deref())?;
    let contract:Option<Contract>=notes.map(|n|super::metadata::decode(PREFIX,&n)).transpose()?;
    if let Some(c)=&contract{c.validate()?;}Ok(contract)
}
fn native_matches(bare:&Document,carrier:&Document,c:&Entry,modern:bool,witness:&mut Option<Option<Document>>)->Result<bool>{
    let scoped=c.version==2;
    if fingerprint_scoped(bare,&c.text_id,&c.guide_id,scoped)?.as_deref()==Some(c.native_sha256.as_str()){return Ok(true);}
    let actual=bare.object(&c.text_id)?;
    if !matches!(&actual.geometry,Geometry::Text{text_path:Some(_),layout,..}if layout.mode==crate::TextMode::Point){return Ok(false);}
    // A Corel resave recalculates fitted-text width/height ink caches. They
    // do not control point-text layout. Rebuild their OLD exact witness from
    // the current, already hash-verified carrier; never discard the native
    // digest or substitute Notes for active font/fit/guide/ancestor records.
    // An unencodable/changed carrier or another host's different font metrics
    // simply keeps the native values. One witness is shared by the batch.
    let expected=witness.get_or_insert_with(||{
        let bytes=if modern{super::encode_document21(carrier)}else{super::encode_document(carrier)}.ok()?.0;
        super::read::decode_geometry(&bytes,"cyclic-cache-witness").ok().map(|v|v.0)
    });
    let Some(expected)=expected else{return Ok(false)};
    if fingerprint_scoped(expected,&c.text_id,&c.guide_id,scoped)?.as_deref()!=Some(c.native_sha256.as_str()){return Ok(false);}
    let old=expected.object(&c.text_id)?;
    Ok(fingerprint_with_cache(bare,&c.text_id,&c.guide_id,scoped,Some([old.width_mm,old.height_mm]))?.as_deref()==Some(c.native_sha256.as_str()))
}
fn restore_entry(doc:&mut Document,bare:&Document,c:Entry,modern:bool,carrier:&Document,witness:&mut Option<Option<Document>>)->Result<bool>{
    if ![1,2].contains(&c.version)||!(1..=3).contains(&c.turn)||c.own.iter().any(|v|!v.is_finite())||c.own!=super::path_rotation::matrix(c.turn,[c.own[4],c.own[5]]){return Err(invalid("invalid cyclic source contract version/own matrix"));}
    if fingerprint_scoped(doc,&c.text_id,&c.guide_id,c.version==2)?.as_deref()!=Some(c.carrier_sha256.as_str())||!native_matches(bare,carrier,&c,modern,witness)?{return Ok(false);}
    let actual=path(doc,&c.guide_id)?;let segments=super::path_rotation::pieces(&actual).ok_or_else(||invalid("bound cyclic guide is no longer closed"))?;
    if c.shift==0||c.shift>=segments.len(){return Err(invalid("cyclic inverse shift invalid"));}
    let inverse=rephase(&actual,segments.len()-c.shift)?;
    let Some(d)=c.notation.restore(&inverse.to_svg())?else{return Ok(false)};
    let carrier=if c.version==2{pair_scene(doc,&c.text_id)?}else{doc.clone()};
    let mut candidate=carrier.clone();candidate.objects.iter_mut().find(|o|o.id==c.text_id).unwrap().transform=c.own;
    let Geometry::Path{d:target,..}=&mut candidate.objects.iter_mut().find(|o|o.id==c.guide_id).unwrap().geometry else{unreachable!()};*target=d;
    // Reprove the exact chart relation after native decoding, atomically.
    let Some(proof)=plan_one(&candidate,&carrier)?else{return Err(invalid("cyclic inverse unexpectedly neutral"))};
    if proof.turn!=c.turn||proof.shift!=c.shift{return Err(invalid("cyclic inverse symmetry differs"));}
    if c.version==1{*doc=candidate;}else{
        for id in[&c.text_id,&c.guide_id]{*doc.objects.iter_mut().find(|o|&o.id==id).unwrap()=candidate.object(id)?.clone();}
    }Ok(true)
}

pub(super) fn restore(doc:&mut Document,bare:&Document,c:Contract,modern:bool)->Result<(usize,usize)>{
    c.validate()?;let entries=match c{Contract::Single(e)=>vec![e],Contract::Batch(b)=>b.stories.into_values().collect()};
    let(mut restored,mut retained)=(0,0);let carrier=doc.clone();let mut witness=None;
    for entry in entries{if restore_entry(doc,bare,entry,modern,&carrier,&mut witness)?{restored+=1;}else{retained+=1;}}
    doc.validate()?;Ok((restored,retained))
}

#[cfg(test)]mod tests{
    use super::*;
    fn object_mut<'a>(d:&'a mut Document,id:&str)->&'a mut crate::Object{d.objects.iter_mut().find(|o|o.id==id).unwrap()}
    fn pair(curved:bool,parent:bool,turn:u8)->(Document,Document){
        let source=super::super::path_rotation::tests::source(curved,parent,turn,true);
        let text=source.object("text").unwrap();let delta=super::super::path_text::translation_world(&source,text).unwrap().unwrap();
        let guide=source.object("guide").unwrap();let path=guide.local_path().unwrap();
        let shift=super::super::path_rotation::shift(&path,source.world_transform(guide).unwrap(),turn,delta).unwrap();
        let mut carrier=source.clone();object_mut(&mut carrier,"guide").geometry=Geometry::Path{d:rephase(&path,shift).unwrap().to_svg(),fill_rule:crate::FillRule::NonZero};
        object_mut(&mut carrier,"text").transform=[1.,0.,0.,1.,-text.x_mm,-text.y_mm];(source,carrier)
    }
    fn pack(native:&mut NativeRecords)->Vec<u8>{
        if native.version==2100{repack21(native).unwrap()}else{native.members.insert("content/riffData.cdr".into(),write(&native.nodes).unwrap());zip(&native.members.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>())}
    }
    #[test]fn complete_source_restores_all_controls_and_own_without_changing_native_body(){
        for curved in[false,true]{for parent in[false,true]{for turn in[1,2,3]{for modern in[false,true]{
            let(source,carrier)=pair(curved,parent,turn);let bytes=encode(&source,&carrier,modern).unwrap().0;
            if !modern{assert_eq!(&bytes[30..38],b"mimetype");}
            let decoded=super::super::decode(&bytes,&source.id).unwrap();assert_eq!(decoded.0,source);assert!(decoded.1.iter().any(|n|n.code=="CDR_CYCLIC_SOURCE"));
            let ordinary=if modern{super::super::encode_document21(&carrier)}else{super::super::encode_document(&carrier)}.unwrap().0;
            let(mut a,mut b)=(read_native(&bytes).unwrap(),read_native(&ordinary).unwrap());
            for key in["metadata/metadata.xml","META-INF/metadata.xml"]{a.members.remove(key);b.members.remove(key);}assert_eq!(a.members,b.members);
        }}}}
    }
    #[test]fn unrelated_edits_survive_but_endpoint_ancestor_and_new_user_edits_disable_recovery(){
        let(source,carrier)=pair(true,true,1);let bytes=encode(&source,&carrier,false).unwrap().0;
        let native=read_native(&bytes).unwrap();let mut frames=super::super::local_frames::read(&native.members).unwrap();let contract=read(&native.members,&mut frames).unwrap().unwrap();let bare=super::super::read::decode_geometry(&bytes,"bare").unwrap().0;
        for mode in 0..9{
            let mut actual=carrier.clone();let mut observed=bare.clone();
            match mode{
                0=>actual.id="another-import-id".into(),
                1=>{actual.objects.reverse();},
                2=>{object_mut(&mut actual,"text").x_mm+=1e-7;},
                3=>{object_mut(&mut actual,"guide").transform[4]+=1e-7;},
                4=>{object_mut(&mut actual,"parent").transform[4]+=1e-7;},
                5=>{let mut copy=actual.object("text").unwrap().clone();copy.id="second-story".into();actual.objects.push(copy);},
                6=>{actual.objects.retain(|o|o.id!="text");},
                7=>{object_mut(&mut observed,"guide").transform[4]+=1e-12;},
                _=>{let Geometry::Text{content,..}=&mut object_mut(&mut actual,"text").geometry else{unreachable!()};*content="Changed".into();},
            }
            let before=actual.clone();let copy:Contract=super::super::metadata::decode(PREFIX,&super::super::metadata::encode(PREFIX,&contract).unwrap()).unwrap();let restored=restore(&mut actual,&observed,copy,false).unwrap();
            assert_eq!(restored,if mode<2{(1,0)}else{(0,1)},"mode{mode}");if mode>=2{assert_eq!(actual,before,"mode{mode}");}else{assert_eq!(actual.object("text").unwrap().transform,source.object("text").unwrap().transform);assert_eq!(actual.objects.iter().map(|o|&o.id).collect::<Vec<_>>(),before.objects.iter().map(|o|&o.id).collect::<Vec<_>>());}
        }
    }
    #[test]fn native_fit_node_and_subgrid_frame_edits_do_not_restore_old_rotation(){
        for modern in[false,true]{for mode in 0..3{
            let(source,carrier)=pair(true,true,1);let mut native=read_native(&encode(&source,&carrier,modern).unwrap().0).unwrap();
            fn edit(nodes:&mut[Node],modern:bool,mode:u8)->bool{for node in nodes{
                if mode==0&&node.tag==*b"txsm"{let at=if modern{117}else{112};let value=i32_at(&node.data,at).unwrap();set32(&mut node.data,at,(value+1)as u32).unwrap();return true;}
                if mode>0&&node.kind()==Some(b"obj "){
                    let(kind,mut args)=super::super::write::arguments(&node.find(b"loda").unwrap().data).unwrap();
                    if args.iter().any(|(k,v)|*k==1000&&read_wide(v).unwrap()=="guide"){
                        if mode==1{let data=&mut args.iter_mut().find(|(k,_)|*k==30).unwrap().1;let value=i32_at(data,12).unwrap();set32(data,12,(value+1)as u32).unwrap();node.find_mut(b"loda").unwrap().data=super::super::write::loda(kind,&args);}
                        else{let data=&mut node.find_mut(b"trfd").unwrap().data;let at=u32_at(data,u32_at(data,8).unwrap()as usize).unwrap()as usize+32;let value=f64_at(data,at).unwrap();data[at..at+8].copy_from_slice(&(value+0.001).to_le_bytes());}return true;
                    }
                }if let Some(children)=&mut node.children{if edit(children,modern,mode){return true;}}
            }false}
            assert!(edit(&mut native.nodes,modern,mode));let bytes=pack(&mut native);let actual=super::super::decode(&bytes,"edited").unwrap();
            assert!(actual.1.iter().any(|n|n.code=="CDR_CYCLIC_SOURCE_NATIVE"));assert_ne!(actual.0.object("text").unwrap().transform,source.object("text").unwrap().transform);
        }}
    }
    #[test]fn fitted_cache_recalculation_requires_both_original_digests_and_every_active_native_field(){
        for modern in[false,true]{let(source,carrier)=pair(true,true,1);let bytes=encode(&source,&carrier,modern).unwrap().0;let native=read_native(&bytes).unwrap();let notes=super::super::metadata::find(&native.members,PREFIX).unwrap().unwrap();let bare=super::super::read::decode_geometry(&bytes,"bare").unwrap().0;
            for mode in 0..7{let mut contract:Contract=super::super::metadata::decode(PREFIX,&notes).unwrap();let mut observed=bare.clone();let mut actual=carrier.clone();let text=object_mut(&mut observed,"text");text.width_mm+=1.;text.height_mm+=2.;
                match mode{
                    0=>{},
                    1=>if let Geometry::Text{font_size_mm,..}=&mut object_mut(&mut observed,"text").geometry{*font_size_mm+=0.0001;},
                    2=>object_mut(&mut observed,"guide").transform[4]+=1e-12,
                    3=>object_mut(&mut observed,"parent").transform[4]+=1e-12,
                    4=>contract.entries_mut().next().unwrap().native_sha256="a".repeat(64),
                    5=>object_mut(&mut actual,"text").width_mm+=1e-12,
                    _=>{let mut extra=observed.object("text").unwrap().clone();extra.id="new-live-user".into();observed.objects.push(extra);},
                }
                let before=actual.clone();let counts=restore(&mut actual,&observed,contract,modern).unwrap();assert_eq!(counts,if mode==0{(1,0)}else{(0,1)},"mode {mode}");if mode==0{assert_eq!(actual,source);}else{assert_eq!(actual,before);}
            }
        }
    }
    #[test]fn batch_cache_witness_is_reused_without_overwriting_another_native_edit(){
        for modern in[false,true]{let(source,carrier)=batch_pair(true,false);let bytes=encode(&source,&carrier,modern).unwrap().0;let native=read_native(&bytes).unwrap();let notes=super::super::metadata::find(&native.members,PREFIX).unwrap().unwrap();let bare=super::super::read::decode_geometry(&bytes,"bare").unwrap().0;
            for edited in[false,true]{let mut observed=bare.clone();for id in["text","text-two"]{object_mut(&mut observed,id).width_mm+=1.;object_mut(&mut observed,id).height_mm+=1.;}if edited{object_mut(&mut observed,"guide").transform[4]+=1e-12;}
                let contract=super::super::metadata::decode(PREFIX,&notes).unwrap();let mut actual=carrier.clone();assert_eq!(restore(&mut actual,&observed,contract,modern).unwrap(),if edited{(1,1)}else{(2,0)});if !edited{assert_eq!(actual,source);}else{assert_eq!(actual.object("text").unwrap(),carrier.object("text").unwrap());assert_eq!(actual.object("text-two").unwrap(),source.object("text-two").unwrap());}
            }
        }
    }
    #[test]fn missing_or_changed_bindings_and_legacy_frame_readers_refuse(){
        let(source,carrier)=pair(false,false,1);let bytes=encode(&source,&carrier,false).unwrap().0;
        let native=read_native(&bytes).unwrap();let raw=super::super::metadata::find(&native.members,"ArrangerLocalFramesV1|sha256:").unwrap().unwrap();
        let value:serde_json::Value=super::super::metadata::decode("ArrangerLocalFramesV1|sha256:",&raw).unwrap();assert_eq!(value["version"],3);
        assert!(super::super::local_frames::parent_chart(&carrier,super::super::local_frames::read(&native.members).unwrap().as_ref(),carrier.object("text").unwrap()).is_err());
        for mode in 0..3{let mut n=read_native(&bytes).unwrap();let raw=n.members.get_mut("metadata/metadata.xml").unwrap();let mut xml=String::from_utf8(raw.clone()).unwrap();let notes=super::super::metadata::find(&native.members,PREFIX).unwrap().unwrap();
            if mode==0{xml=xml.replace(&notes,"");}else if mode==1{let mut c:serde_json::Value=super::super::metadata::decode(PREFIX,&notes).unwrap();c["shift"]=2.into();xml=xml.replace(&notes,&super::super::metadata::encode(PREFIX,&c).unwrap());}else{xml=xml.replace(&super::super::metadata::find(&native.members,"ArrangerLocalFramesV1|sha256:").unwrap().unwrap(),"");}*raw=xml.into_bytes();
            assert!(super::super::decode(&pack(&mut n),"bad").is_err(),"mode{mode}");
        }
    }
    #[test]fn no_unrelated_carrier_changes_or_approximate_symmetry_are_accepted(){
        let(source,carrier)=pair(false,false,1);
        for mode in 0..4{let mut c=carrier.clone();match mode{
            0=>object_mut(&mut c,"text").style.fill="#345678".into(),
            1=>object_mut(&mut c,"text").transform[4]+=1e-9,
            2=>{if let Geometry::Path{d,..}=&mut object_mut(&mut c,"guide").geometry{*d=d.replace("100", "100.0000001");}},
            _=>{let mut t=c.object("text").unwrap().clone();t.id="another".into();c.objects.push(t);},
        }assert!(encode(&source,&c,false).is_err(),"mode{mode}");}
    }
    #[test]fn unrelated_paint_layer_bindings_survive_source_contract_insertion(){
        for modern in[false,true]{
            let(mut source,mut carrier)=pair(true,false,1);
            let marker:crate::Object=serde_json::from_value(serde_json::json!({"id":"marker","page_id":"page-1","x_mm":160.,"y_mm":200.,"width_mm":10.,"height_mm":12.,"geometry":{"kind":"rect","radius_mm":0.},"style":{"fill":"#12345680","stroke":"#234567c0","stroke_width_mm":0.25,"opacity":1.}})).unwrap();
            source.objects.push(marker.clone());carrier.objects.push(marker);
            let ordinary=if modern{super::super::encode_document21(&carrier)}else{super::super::encode_document(&carrier)}.unwrap().0;
            let before=read_native(&ordinary).unwrap();let layers=super::super::metadata::find(&before.members,"ArrangerPaintLayersV1|sha256:").unwrap();assert!(layers.is_some());
            let bytes=encode(&source,&carrier,modern).unwrap().0;let after=read_native(&bytes).unwrap();assert_eq!(layers,super::super::metadata::find(&after.members,"ArrangerPaintLayersV1|sha256:").unwrap());
            assert_eq!(super::super::decode(&bytes,&source.id).unwrap().0,source);
        }
    }
    fn batch_pair(pages:bool,shared_parent:bool)->(Document,Document){
        let(mut source,mut carrier)=pair(true,true,1);let(other,other_carrier)=pair(false,true,3);
        if pages{let mut page=source.pages[0].clone();page.id="page-2".into();source.pages.push(page.clone());carrier.pages.push(page);}
        for (doc,extra) in[(&mut source,other),(&mut carrier,other_carrier)]{
            for mut o in extra.objects{
                if shared_parent&&o.id=="parent"{continue;}
                o.id.push_str("-two");if pages{o.page_id="page-2".into();}
                if let Some(parent)=&mut o.parent_id{if !shared_parent{parent.push_str("-two");}}
                if let Geometry::Text{text_path:Some(binding),..}=&mut o.geometry{binding.path_id.push_str("-two");}
                doc.objects.push(o);
            }
        }source.validate().unwrap();carrier.validate().unwrap();(source,carrier)
    }
    #[test]fn batch_multiple_pages_parents_and_two_owns_restore_the_complete_document(){
        for (pages,shared)in[(false,false),(false,true),(true,false)]{for modern in[false,true]{
            let(source,carrier)=batch_pair(pages,shared);let bytes=encode(&source,&carrier,modern).unwrap().0;let actual=super::super::decode(&bytes,&source.id).unwrap();assert_eq!(actual.0,source);
            let native=read_native(&bytes).unwrap();let raw=super::super::metadata::find(&native.members,PREFIX).unwrap().unwrap();let value:serde_json::Value=super::super::metadata::decode(PREFIX,&raw).unwrap();assert_eq!(value["version"],2);assert_eq!(value["stories"].as_object().unwrap().len(),2);
            let contract:Contract=super::super::metadata::decode(PREFIX,&raw).unwrap();let bare=super::super::read::decode_geometry(&bytes,"native").unwrap().0;let mut doc=carrier.clone();assert_eq!(restore(&mut doc,&bare,contract,modern).unwrap(),(2,0));assert_eq!(doc,source);
        }}
    }
    #[test]fn batch_one_actual_node_edit_keeps_other_story_restorable_in_both_formats(){
        for modern in[false,true]{let(source,carrier)=batch_pair(true,false);let mut n=read_native(&encode(&source,&carrier,modern).unwrap().0).unwrap();
            fn edit(nodes:&mut[Node])->bool{for node in nodes{
                if node.kind()==Some(b"obj ") {let(kind,mut args)=super::super::write::arguments(&node.find(b"loda").unwrap().data).unwrap();if args.iter().any(|(k,v)|*k==1000&&read_wide(v).unwrap()=="guide"){
                    let data=&mut args.iter_mut().find(|(k,_)|*k==30).unwrap().1;let v=i32_at(data,12).unwrap();set32(data,12,(v+1)as u32).unwrap();node.find_mut(b"loda").unwrap().data=super::super::write::loda(kind,&args);return true;
                }}if let Some(children)=&mut node.children{if edit(children){return true;}}
            }false}
            assert!(edit(&mut n.nodes));let bytes=pack(&mut n);let(actual,notes)=super::super::decode(&bytes,"edited").unwrap();assert_ne!(actual.object("text").unwrap().transform,source.object("text").unwrap().transform);assert_eq!(actual.object("text-two").unwrap(),source.object("text-two").unwrap());assert_eq!(actual.object("guide-two").unwrap(),source.object("guide-two").unwrap());assert!(notes.iter().any(|n|n.code=="CDR_CYCLIC_SOURCE"));assert!(notes.iter().any(|n|n.code=="CDR_CYCLIC_SOURCE_NATIVE"));
        }
    }
    #[test]fn batch_unrelated_page_addition_reordering_and_reparented_story_are_scoped(){
        for mode in 0..4{let(source,carrier)=batch_pair(mode!=3,false);let bytes=encode(&source,&carrier,false).unwrap().0;let native=read_native(&bytes).unwrap();let raw=super::super::metadata::find(&native.members,PREFIX).unwrap().unwrap();let bare=super::super::read::decode_geometry(&bytes,"bare").unwrap().0;let mut doc=carrier.clone();let mut observed=bare.clone();
            if mode<2{for d in[&mut doc,&mut observed]{let mut page=d.pages[0].clone();page.id="unrelated-page".into();d.pages.insert(0,page);if mode==1{d.pages.reverse();}}}
            else if mode==2{object_mut(&mut doc,"parent").transform[4]+=1e-9;}
            else{let Geometry::Text{text_path:Some(p),..}=&mut object_mut(&mut doc,"text").geometry else{unreachable!()};p.path_id="guide-two".into();}
            let c=super::super::metadata::decode(PREFIX,&raw).unwrap();let counts=restore(&mut doc,&observed,c,false).unwrap();assert_eq!(counts,if mode<2{(2,0)}else if mode==2{(1,1)}else{(0,2)},"mode{mode}");
            if mode<2{assert_eq!(doc.objects,source.objects);assert_eq!(doc.pages,observed.pages);}
        }
    }
    #[test]fn batch_shared_guide_extra_carrier_edit_and_contract_aliasing_refuse(){
        let(source,carrier)=batch_pair(false,false);
        for mode in 0..2{let mut s=source.clone();let mut c=carrier.clone();
            if mode==0{for d in[&mut s,&mut c]{let Geometry::Text{text_path:Some(p),..}=&mut object_mut(d,"text-two").geometry else{unreachable!()};p.path_id="guide".into();}}
            else{object_mut(&mut c,"text-two").style.fill="#abcdef".into();}
            assert!(encode(&s,&c,false).is_err());
        }
        let Contract::Batch(mut b)=plan(&source,&carrier).unwrap().unwrap()else{panic!()};b.stories.get_mut("text-two").unwrap().guide_id="guide".into();assert!(Contract::Batch(b).validate().is_err());
    }

}
