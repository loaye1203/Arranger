//! Version21 model writer. Share geometry/scene lowering with the14 encoder,
//! then encode the distinct native21 styles, rectangle/story formats and real
//! external container. This is experimental interchange, not complete Save.
use super::{binary::*,invalid,Result};
use crate::{Document,Geometry,command::Notice};
use std::collections::BTreeMap;
use serde_json::json;
const PROFILE:&[u8]=include_bytes!("../../../../reports/windows/WIN-CDR-SAVE-20261022-01/focused/native-preservation/identity-v21.cdr");
fn collect(nodes:&[Node])->Vec<Node> {nodes.iter().flat_map(|n|if n.kind()==Some(b"cmpr") {collect(n.children.as_deref().unwrap_or_default())}else{vec![n.clone()]}).collect()}
#[cfg(test)]
#[test]
fn changed124_rectangle_profiles_include_parent_world_frames_and_leave36_inputs_unchanged(){
    let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");let p:serde_json::Value=serde_json::from_slice(&std::fs::read(root.join("examples/assets/cdr/local-circle21-20261101/manifest.json")).unwrap()).unwrap();let(mut changed,mut unchanged)=(0,0);
    for case in p["cases"].as_array().unwrap(){let d:Document=serde_json::from_value(case["document"].clone()).unwrap();let raw=encode(&d).unwrap().0;let old=std::fs::read(root.join(case["file"].as_str().unwrap())).unwrap();
        let failed=case["name"].as_str().unwrap().starts_with("rect-")&&!case["name"].as_str().unwrap().contains("equal-columns");
        if failed{assert_ne!(raw,old);let native=read_native(&raw).unwrap();let mut found=0;for node in all(&native.nodes).into_iter().filter(|n|n.kind()==Some(b"obj ")){
            let(kind,args)=super::write::arguments(&super::read::own_record(node,b"loda").unwrap().data).unwrap();if args.iter().any(|(k,v)|*k==1000&&read_wide(v).unwrap()=="content"){
                assert_eq!(kind,1);assert_eq!(args[0].0,40001);assert_eq!(args[0].1,[0x86,0x2f,0xe5,0x52,0xb6,0x4f,0xa3,0x43,0x97,0xf5,0x61,0x6a,0x9f,0x43,0x33,0xb8]);found+=1;
            }}assert_eq!(found,1);changed+=1;
        }else{assert_eq!(raw,old,"{} must keep its accepted native representation",case["name"]);unchanged+=1;}
    }assert_eq!((changed,unchanged),(28,36));
}
pub(super) fn encode(doc:&Document)->Result<(Vec<u8>,Vec<Notice>)> {
    // Geometry lowering is shared, but21 carries its fill rule in its typed
    // native style as well as the actual path-mode argument. No intermediate14
    // document is delivered to the caller.
    doc.validate()?;
    let mut base=doc.clone();for o in &mut base.objects {if matches!(o.geometry,Geometry::Group){o.style.opacity=1.;}}
    let (raw,mut warnings,mut clip_rules)=super::write::lower21(&base)?;let mut common=collect(&read(&raw)?);let mut model=super::read::decode(&raw,"native21-lowering")?.0;for o in &mut model.objects {if let Ok(source)=base.object(&o.id){if matches!(source.geometry,Geometry::Text{text_path:Some(_),..}){*o=super::path_text::native_object(&base,source)?;}}super::paint_params::lower(o)?;}let fonts=super::fonts::Fonts::new(&model)?;
    let paragraph_styles=allocate_paragraph_styles(&mut common,&model)?;
    let native_outlines=all(&common).into_iter().filter(|n|n.tag==*b"outl").map(|n|u32_at(&n.data,0).map(|key|(key,n.data.clone()))).collect::<Result<BTreeMap<_,_>>>()?;
    let group_mask=if doc.objects.iter().any(|o|matches!(o.geometry,Geometry::Group)&&o.style.opacity!=1.) {
        let key=all(&common).into_iter().filter(|n|n.tag==*b"fild").map(|n|u32_at(&n.data,0)).collect::<Result<Vec<_>>>()?.into_iter().max().unwrap_or(0).checked_add(1).ok_or_else(||invalid("group transparency mask key overflow"))?;
        let native=common.iter_mut().find(|n|n.kind()==Some(b"doc ")).ok_or_else(||invalid("lowered document absent"))?;
        native.list_mut(b"filt")?.children.as_mut().ok_or_else(||invalid("lowered fill table absent"))?.push(super::opacity::mask(key,false)?);Some(key)
    }else{None};
    fn convert(nodes:&mut [Node],model:&Document,original:&Document,fonts:&super::fonts::Fonts,clip_rules:&mut BTreeMap<u32,crate::FillRule>,group_mask:Option<u32>,paragraph_styles:&BTreeMap<String,Vec<u32>>,native_outlines:&BTreeMap<u32,Vec<u8>>)->Result<()> {
        for n in nodes {
            if matches!(n.kind(),Some(b"obj "|b"grp ")) {
                let (kind,mut args)=super::write::arguments(&super::read::own_record(n,b"loda")?.data)?;let name=args.iter().find(|(k,_)|*k==1000).map(|(_,v)|read_wide(v)).transpose()?.unwrap_or_default();
                let value=if n.kind()==Some(b"grp "){
                    let opacity=original.object(&name).map_or(1.,|o|o.style.opacity);
                    if opacity!=1. {args.push((8000,super::opacity::encode(opacity,super::clipping::key(n)?,group_mask.ok_or_else(||invalid("group mask absent"))?)?.ok_or_else(||invalid("group transparency absent"))?));json!({"transparency":super::opacity::json(opacity)?})}else{json!({})}
                }else if let Ok(o)=model.object(&name) {
                    let mut value=super::modern::style_json(&o.style)?;
                    if let Ok(source)=original.object(&name){if source.style.stroke!="none"{if let Some(units)=super::path_precision::plan(original,source)?{value["outline"]["scaleWithObject"]=json!(if source.style.stroke_scale_with_object{"1"}else{"0"});value["outline"]["width"]=json!(coord(source.style.stroke_width_mm*if source.style.stroke_scale_with_object{f64::from(units)}else{1.})?.to_string());}}}
                    if let Ok(source)=original.object(&name) {if let Geometry::Path{fill_rule,..}=&source.geometry {value["fill"]["winding"]=json!(if *fill_rule==crate::FillRule::EvenOdd {"0"}else{"1"});}}
                    if matches!(o.geometry,Geometry::Rect{..}|Geometry::Ellipse|Geometry::Path{..}){let tr=&super::read::own_record(n,b"trfd")?.data;let at=u32_at(tr,u32_at(tr,8)? as usize)? as usize+16;let mut matrix=[0.;6];for(i,v)in matrix.iter_mut().enumerate(){*v=f64_at(tr,at+i*8)?;}super::primitive_frames::world_style(&mut value,matrix)?;}
                    if let Some(line)=args.iter().find(|(k,_)|*k==10).map(|(_,v)|u32_at(v,0)).transpose()?.and_then(|key|native_outlines.get(&key)) {
                        if let Some(nib)=super::pen::read(line)? {let at=super::read::outline_body(line)?;value["outline"]["width"]=json!(i32_at(line,at+6)?.to_string());value["outline"]["angle"]=json!((i32_at(line,at+12)? as f64/1_000_000.).to_string());value["outline"]["aspect"]=json!(u16_at(line,at+10)?.to_string());value["outline"]["scaleWithObject"]=json!(if nib.scale_with_object{"1"}else{"0"});let tr=&super::read::own_record(n,b"trfd")?.data;let pos=u32_at(tr,u32_at(tr,8)? as usize)? as usize+16;let mut matrix=[0.;6];for(i,v)in matrix.iter_mut().enumerate(){*v=f64_at(tr,pos+i*8)?;}super::pen::rotated_preset(&mut value,matrix,line)?;}
                    }
                    if let Geometry::Text{content,font_family,font_size_mm,layout,..}=&o.geometry {
                        value=super::modern::text_json(fonts.native_face(font_family,*font_size_mm)?,&o.style,layout,*font_size_mm,o.width_mm,super::typography::tracking(model,o)?)?;
                        if let Some((_,effect))=args.iter().find(|(k,_)|*k==8000){value["transparency"]=super::opacity::json_scoped(o.style.opacity,u32_at(effect,89)?)?;}
                        let template=&super::read::own_record(n,b"txsm")?.data;let ids=paragraph_styles.get(&name).ok_or_else(||invalid("native21 paragraph style allocation missing"))?;let data=super::modern::encoded_text(template,kind==6,content,&value,fonts.choice(font_family)?.language(),o.width_mm,o.height_mm,ids)?;n.find_mut(b"txsm")?.data=data;
                    }
                    if matches!(o.geometry,Geometry::Rect{..}) {let tr=&super::read::own_record(n,b"trfd")?.data;let at=u32_at(tr,u32_at(tr,8)? as usize)? as usize+16;let mut matrix=[0.;6];for(i,v)in matrix.iter_mut().enumerate(){*v=f64_at(tr,at+i*8)?;}let old=&args.iter().find(|(k,_)|*k==30).ok_or_else(||invalid("native21 rectangle argument absent"))?.1;let g=super::modern::encoded_rectangle_in_frame(i32_at(old,0)? as f64/10000.,i32_at(old,4)? as f64/10000.,i32_at(old,8)? as f64/10000.,matrix)?;args.iter_mut().find(|(k,_)|*k==30).unwrap().1=g;}
                    if let Some((_,effect))=args.iter().find(|(k,_)|*k==8000){value["transparency"]=super::opacity::json_scoped(o.style.opacity,u32_at(effect,89)?)?;}
                    value
                }else{
                    // Match a generated frame by its native stream key, never
                    // by a synthetic name that could collide with an object.
                    let rule=clip_rules.remove(&super::clipping::key(n)?).ok_or_else(||invalid("native21 generated frame rule missing"))?;
                    if kind==1 {let old=&args.iter().find(|(k,_)|*k==30).ok_or_else(||invalid("native clip geometry absent"))?.1;let g=super::modern::encoded_rectangle(i32_at(old,0)? as f64/10000.,i32_at(old,4)? as f64/10000.,i32_at(old,8)? as f64/10000.)?;args.iter_mut().find(|(k,_)|*k==30).unwrap().1=g;}
                    json!({"fill":{"type":"0","winding":if rule==crate::FillRule::EvenOdd {"0"}else{"1"}},"outline":{"width":"0"},"transparency":{}})
                };
                // Actual Corel-created21 affine rectangles and the four
                // type-preserving report124 returns carry this exact opaque
                // record. It is shared by distinct native rectangles, not a
                // per-object identity to randomize or duplicate from a UUID.
                // Restrict the changed profile to the unequal-column scaled
                // rectangles that report124 converted onOpen. Native testing
                // must still establish whether this construction keeps type1.
                if kind==1{if let Ok(o)=model.object(&name){
                    let mut world=o.clone();world.transform=model.world_transform(o)?.as_coeffs();
                    if matches!(world.geometry,Geometry::Rect{radius_mm:0.})&&super::primitive_frames::local_circle21(&world)?&&world.transform[0].hypot(world.transform[1])!=world.transform[2].hypot(world.transform[3]){
                        args.insert(0,(40001,vec![0x86,0x2f,0xe5,0x52,0xb6,0x4f,0xa3,0x43,0x97,0xf5,0x61,0x6a,0x9f,0x43,0x33,0xb8]));
                    }
                }
                }
                args.push((201,super::modern::encoded_style(&value)?));n.find_mut(b"loda")?.data=super::write::loda(kind,&args);
            }
            if let Some(c)=&mut n.children {convert(c,model,original,fonts,clip_rules,group_mask,paragraph_styles,native_outlines)?;}
        }Ok(())
    }
    convert(&mut common,&model,doc,&fonts,&mut clip_rules,group_mask,&paragraph_styles,&native_outlines)?;
    super::opacity::upgrade_masks21(&mut common)?;
    if !clip_rules.is_empty(){return Err(invalid("native21 generated frame rule was not consumed"));}
    let native=read_native(PROFILE)?;let mut root=native.nodes;
    let config=all(&root).into_iter().find(|n|n.tag==*b"mcfg").ok_or_else(||invalid("native21 coordinate configuration absent"))?.data.clone();
    super::origin::upgrade(&mut common,&config,[model.pages[0].width_mm,model.pages[0].height_mm])?;
    let old_doc=common.iter().find(|n|n.kind()==Some(b"doc ")).ok_or_else(||invalid("lowered document absent"))?;
    let target=root.iter_mut().find(|n|n.kind()==Some(b"doc ")).ok_or_else(||invalid("native21 document profile absent"))?;
    for kind in [b"fntt",b"filt",b"otlt"] {let source=all(std::slice::from_ref(old_doc)).into_iter().find(|n|n.kind()==Some(kind)).ok_or_else(||invalid("lowered native table absent"))?;*target.list_mut(kind)?=source.clone();}
    let stlt=all(std::slice::from_ref(old_doc)).into_iter().find(|n|n.kind()==Some(b"stlt")).ok_or_else(||invalid("lowered styles absent"))?;
    let children=target.children.as_mut().ok_or_else(||invalid("native21 document children absent"))?;children.retain(|n|n.tag!=*b"stlt"&&n.kind()!=Some(b"stlt"));children.push(Node::leaf(b"stlt",stlt.data[4..].to_vec()));for n in children.iter_mut().filter(|n|n.kind()==Some(b"udat")){n.children=Some(vec![]);}
    children.retain(|n|n.kind()!=Some(b"clpt"));children.extend(old_doc.children.as_deref().unwrap_or_default().iter().filter(|n|n.kind()==Some(b"clpt")).cloned());
    children.retain(|n|n.tag!=*b"lnkt");children.extend(old_doc.children.as_deref().unwrap_or_default().iter().filter(|n|n.tag==*b"lnkt").cloned());
    super::local_data::transfer21(old_doc,target)?;
    let config=&mut target.find_mut(b"mcfg")?.data;set_coord(config,12,model.pages[0].width_mm)?;set_coord(config,16,model.pages[0].height_mm)?;
    root.retain(|n|!matches!(n.kind(),Some(b"page"|b"bmpt")));let at=root.iter().position(|n|n.kind()==Some(b"doc ")).unwrap();root.splice(at..at,common.iter().filter(|n|n.kind()==Some(b"bmpt")).cloned());root.extend(common.into_iter().filter(|n|n.kind()==Some(b"page")));
    let mut members=BTreeMap::new();members.insert("font/fontTable.dat".into(),unzip(&raw)?["font/fontTable.dat"].clone());
    if let Some(data)=super::metadata::document(doc,true)?{members.insert("META-INF/metadata.xml".into(),data);}
    members.insert("META-INF/container.xml".into(),b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\" xmlns:crl=\"urn:corel:namespaces:zcf:xmlns:container\"><rootfiles><rootfile crl:file-kind=\"main\" full-path=\"content/root.dat\" media-type=\"application/x-vnd.corel.cdr.rootdata\"/></rootfiles></container>".to_vec());
    if let Some(style)=native.members.get("styles/document.cdss") {members.insert("styles/document.cdss".into(),style.clone());}
    let data=write21(&root,members)?;
    for w in &mut warnings {if w.code=="CDR_EXPERIMENTAL" {w.message="Experimental genuine CDR21 model interchange: external native records, object styles, floating rectangles and editable stories encoded. New Corel open/edit/SaveAs21/reopen validation is pending; this is not full-fidelity Save. Keep the complete .arranger source.".into();}}
    Ok((data,warnings))
}

fn allocate_paragraph_styles(nodes:&mut[Node],model:&Document)->Result<BTreeMap<String,Vec<u32>>> {
    let table=all(nodes).into_iter().find(|n|n.kind()==Some(b"stlt")).ok_or_else(||invalid("lowered paragraph table missing"))?;
    let mut tables=super::typography::Tables::parse(&table.data)?;let source=tables.styles.clone();let mut styles=vec![];let mut allocated=BTreeMap::new();
    for n in all(nodes).into_iter().filter(|n|n.kind()==Some(b"obj ")) {
        let (kind,args)=super::write::arguments(&super::read::own_record(n,b"loda")?.data)?;if ![4,6].contains(&kind){continue;}
        let name=args.iter().find(|(k,_)|*k==1000).map(|(_,v)|read_wide(v)).transpose()?.ok_or_else(||invalid("text name missing"))?;
        let Geometry::Text{content,..}=&model.object(&name)?.geometry else{return Err(invalid("native text model differs"));};
        let story=&super::read::own_record(n,b"txsm")?.data;let on_path=u32_at(story,92)?;
        let key=u32_at(story,if kind==6{136}else if on_path==1{140}else{100})?;
        let row=source.iter().find(|s|u32_at(s,4).ok()==Some(key)).ok_or_else(||invalid("lowered paragraph style unresolved"))?;
        if u32_at(row,0)?!=if kind==6{3}else{2}{return Err(invalid("paragraph style has wrong text level"));}
        let normalized=content.replace("\r\n","\n").replace('\r',"\n");let mut ids=vec![];
        for _ in normalized.split('\n') {
            if styles.len()>=4096{return Err(super::unsupported("native paragraph style count exceeds bounded table"));}
            let id=styles.len() as u32+1;let mut row=row.clone();set32(&mut row,4,id)?;styles.push(row);ids.push(id);
        }
        if allocated.insert(name,ids).is_some(){return Err(invalid("duplicate native text style owner"));}
    }
    if !styles.is_empty(){tables.styles=styles;let data=tables.encode();
        fn replace(nodes:&mut[Node],data:&[u8])->bool{for n in nodes{if n.kind()==Some(b"stlt"){n.data=data.to_vec();return true;}if let Some(c)=&mut n.children{if replace(c,data){return true;}}}false}
        if !replace(nodes,&data){return Err(invalid("paragraph table replacement missing"));}
    }Ok(allocated)
}
