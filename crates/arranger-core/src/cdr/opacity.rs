//! Measured whole-object and single-paint uniform transparency. A group's
//! isolated composition is different and is never distributed to its children.
use super::{binary::*,invalid,unsupported,Result};
use crate::{Geometry,Object,Style};
use serde_json::Value;
#[cfg(test)]
const ROOT:&str="../../../../reports/windows/WIN-SAVE-CLOSURE-20261024-01/controls/";
const CONTROL14:&[u8]=include_bytes!(concat!("../../../../reports/windows/WIN-SAVE-CLOSURE-20261024-01/controls/","opacity-rect-v14.cdr"));
const CONTROL21:&[u8]=include_bytes!(concat!("../../../../reports/windows/WIN-SAVE-CLOSURE-20261024-01/controls/","opacity-rect-v21.cdr"));
fn prototype()->Result<Vec<u8>> {
    static TEMPLATE:std::sync::OnceLock<Result<Vec<u8>>>=std::sync::OnceLock::new();
    TEMPLATE.get_or_init(||{
        let tree=super::binary::read(CONTROL14)?;
        for node in all(&tree).into_iter().filter(|n|n.kind()==Some(b"obj ")) {let (_,args)=super::write::arguments(&super::read::own_record(node,b"loda")?.data)?;if let Some((_,value))=args.into_iter().find(|(k,_)|*k==8000) {return Ok(value);}}
        Err(invalid("uniform transparency native profile missing"))
    }).clone()
}
pub(super) fn percent(opacity:f64)->f64 {
    let value=(1.-opacity)*100.;let integer=value.round_ties_even();
    // Preserve an exactly represented integer-percent opacity as that exact
    // percentage, rather than19.999999999999996 (Corel SDK reports19).
    // This is an equality test, never a tolerance that snaps arbitrary alpha.
    if opacity==(100.-integer)/100. {integer}else{value}
}
fn native_percent(opacity:f64)->Result<f64> {
    if !(0.0..=1.0).contains(&opacity) {return Err(unsupported("opacity outside native percentage range"));}Ok(percent(opacity))
}
pub(super) fn check(object:&Object)->Result<()> {
    if object.style.opacity==1. {return Ok(());}
    if matches!(object.geometry,Geometry::Group) {return Err(unsupported("isolated group opacity cannot be replaced with per-child transparency"));}
    if object.clip.is_some()&&object.style.fill!="none"&&object.style.stroke!="none"{return Err(unsupported("clipped combined-paint transparency requires a separate native composition profile"));}
    native_percent(object.style.opacity)?;Ok(())
}
pub(super) fn encode(opacity:f64,effect:u32,mask:u32)->Result<Option<Vec<u8>>> {
    if opacity==1. {return Ok(None);}
    let mut out=prototype()?;set32(&mut out,9,effect)?;set32(&mut out,113,mask)?;out[69..77].copy_from_slice(&native_percent(opacity)?.to_le_bytes());Ok(Some(out))
}
pub(super) fn encode_scoped(opacity:f64,effect:u32,mask:u32,scope:u32)->Result<Option<Vec<u8>>> {
    if scope>2{return Err(unsupported("unknown uniform transparency paint scope"));}
    let mut value=encode(opacity,effect,mask)?;if let Some(raw)=&mut value{set32(raw,89,scope)?;}Ok(value)
}
#[derive(Clone,Copy)]
pub(super) struct Effect{pub opacity:f64,pub scope:u32,pub percent:f64}
pub(super) fn read_effect(raw:&[u8],fills:&std::collections::BTreeMap<u32,&[u8]>,modern:bool)->Result<Effect>{
    let scope=u32_at(raw,89)?;if scope>2{return Err(unsupported("unknown uniform transparency paint scope"));}
    let mut normalized=raw.to_vec();set32(&mut normalized,89,2)?;
    // Actual SDK per-paint references use both complete mask-marker forms
    // in both file versions. The modern mask reader checks exactly those two
    // measured records; it does not approximate channels or accept more masks.
    Ok(Effect{opacity:read(&normalized,fills,modern||scope!=2)?,scope,percent:f64_at(raw,69)?})
}
pub(super) fn apply_part(style:&mut Style,effect:Effect)->Result<()> {
    if effect.scope==2{return Ok(());}
    if style.fill=="none"||style.stroke=="none"{return Err(unsupported("per-paint native transparency currently requires two live vector paints"));}
    let alpha=(effect.opacity*255.).round_ties_even() as u8;let value=f64::from(alpha)/255.;
    let paint=if effect.scope==0{&mut style.fill}else{&mut style.stroke};
    let crate::paint::Paint::Rgba([r,g,b,255])=crate::paint::parse(paint)?else{return Err(unsupported("native per-paint transparency needs an opaque RGB paint"));};
    *paint=if effect.opacity==value||effect.percent==native_percent(value)?{format!("#{r:02x}{g:02x}{b:02x}{alpha:02x}")}else{format!("rgba({r},{g},{b},{})",effect.opacity)};style.opacity=1.;Ok(())
}
pub(super) fn check_typed_scope(value:&Value,effect:Effect)->Result<()> {
    match value.get("transparency").filter(|v|!v.as_object().is_some_and(|m|m.is_empty())) {
        Some(v)=>if v.get("appliesTo").and_then(Value::as_str).and_then(|s|s.parse::<u32>().ok())!=Some(effect.scope){return Err(unsupported("native binary and typed transparency paint scopes differ"));},
        None if effect.scope!=2=>return Err(unsupported("native scoped transparency requires its typed paint scope")),
        _=>(),
    }Ok(())
}
pub(super) fn decode(raw:&[u8])->Result<f64> {
    let template=prototype()?;
    if raw.len()!=125 {return Err(unsupported("native transparency envelope is not the measured uniform profile"));}
    let percent=f64_at(raw,69)?;if !(0.0..=100.0).contains(&percent) {return Err(invalid("native uniform transparency outside percentage range"));}
    let mut normalized=raw.to_vec();normalized[69..77].copy_from_slice(&template[69..77]);
    // The first is an effect identity, stable even when Corel reorders fills.
    // The second refers to the native black transparency mask, resolved below.
    normalized[9..13].copy_from_slice(&template[9..13]);normalized[113..117].copy_from_slice(&template[113..117]);
    if normalized!=template {return Err(unsupported("native transparency mode/mask/effects require preservation support"));}
    Ok((100.-percent)/100.)
}
pub(super) fn mask(key:u32,modern:bool)->Result<Node> {
    static OLD:std::sync::OnceLock<Result<Vec<u8>>>=std::sync::OnceLock::new();
    static NEW:std::sync::OnceLock<Result<Vec<u8>>>=std::sync::OnceLock::new();
    let mut raw=(if modern{&NEW}else{&OLD}).get_or_init(||{
        let native=read_native(if modern{CONTROL21}else{CONTROL14})?;let id=u32_at(&prototype()?,113)?;
        all(&native.nodes).into_iter().find(|n|n.tag==*b"fild"&&u32_at(&n.data,0).ok()==Some(id)).map(|n|n.data.clone()).ok_or_else(||invalid("native black transparency mask missing"))
    }).clone()?;
    set32(&mut raw,0,key)?;Ok(Node::list(b"filc",vec![Node::leaf(b"fild",raw)]))
}
pub(super) fn upgrade_masks21(nodes:&mut [Node])->Result<()> {
    let mut keys=std::collections::BTreeSet::new();
    for n in all(nodes).into_iter().filter(|n|matches!(n.kind(),Some(b"obj "|b"grp "))) {let (_,args)=super::write::arguments(&super::read::own_record(n,b"loda")?.data)?;if let Some((_,raw))=args.iter().find(|(k,_)|*k==8000){keys.insert(u32_at(raw,113)?);}}
    fn replace(nodes:&mut [Node],keys:&mut std::collections::BTreeSet<u32>)->Result<()> {
        for n in nodes {if n.tag==*b"fild"&&keys.remove(&u32_at(&n.data,0)?) {let key=u32_at(&n.data,0)?;let replacement=mask(key,true)?;n.data=replacement.children.as_ref().unwrap()[0].data.clone();}if let Some(c)=&mut n.children{replace(c,keys)?;}}Ok(())
    }
    replace(nodes,&mut keys)?;if !keys.is_empty(){return Err(invalid("lowered transparency mask absent"));}Ok(())
}
pub(super) fn read(raw:&[u8],fills:&std::collections::BTreeMap<u32,&[u8]>,modern:bool)->Result<f64> {
    let value=decode(raw)?;
    if u32_at(raw,9)?==0 {return Err(invalid("native transparency effect identity is zero"));}
    let key=u32_at(raw,113)?;let found=fills.get(&key).ok_or_else(||invalid("native transparency mask reference is dangling"))?;
    let expected=mask(key,modern)?;let expected=&expected.children.as_ref().unwrap()[0].data;
    if *found!=expected {
        // Report100's actual SDK SaveAs21 keeps the measured14 mask bytes,
        // without the old21 profile's two extra marker bytes. Accept that
        // complete measured record, never an arbitrary nearly-black fill.
        let legacy=mask(key,false)?;
        if !modern||*found!=&legacy.children.as_ref().unwrap()[0].data{return Err(unsupported("native transparency mask is not the measured uniform black fill"));}
    }Ok(value)
}
fn json_profile()->Result<Value> {
    static TEMPLATE:std::sync::OnceLock<Result<Value>>=std::sync::OnceLock::new();
    TEMPLATE.get_or_init(||{
        let native=read_native(CONTROL21)?;
        for node in all(&native.nodes).into_iter().filter(|n|n.kind()==Some(b"obj ")) {let (_,args)=super::write::arguments(&super::read::own_record(node,b"loda")?.data)?;if let Some((_,value))=args.iter().find(|(k,_)|*k==201) {let json=super::modern::style_record(value)?;if let Some(value)=json.get("transparency"){return Ok(value.clone());}}}
        Err(invalid("native21 uniform transparency profile missing"))
    }).clone()
}
pub(super) fn json(opacity:f64)->Result<Value> {
    if opacity==1. {return Ok(serde_json::json!({}));}
    let ratio=native_percent(opacity)?/100.;let mut value=json_profile()?;
    for key in ["startTransparency","uniformTransparency"] {value[key]=Value::String(ratio.to_string());}Ok(value)
}
pub(super) fn json_scoped(opacity:f64,scope:u32)->Result<Value>{
    if scope>2{return Err(unsupported("unknown typed transparency paint scope"));}
    let mut value=json(opacity)?;if opacity!=1.{value["appliesTo"]=Value::String(scope.to_string());}Ok(value)
}
pub(super) fn modern(value:&Value,style:&mut Style)->Result<()> {
    let Some(value)=value.get("transparency") else{return Ok(())};
    if value.as_object().is_some_and(|v|v.is_empty()) {if style.opacity!=1. {return Err(unsupported("native21 style omitted the active native transparency"));}return Ok(());}
    let ratio=value.get("uniformTransparency").and_then(Value::as_str).and_then(|s|s.parse::<f64>().ok()).filter(|v|v.is_finite()&&(0.0..=1.0).contains(v)).ok_or_else(||unsupported("native21 nonuniform transparency is not represented"))?;
    let start=value.get("startTransparency").and_then(Value::as_str).and_then(|s|s.parse::<f64>().ok()).ok_or_else(||invalid("native21 transparency start value missing"))?;
    if start!=ratio {return Err(unsupported("native21 transparency start/uniform values differ"));}
    let scope=value.get("appliesTo").and_then(Value::as_str).and_then(|s|s.parse::<u32>().ok()).filter(|s|*s<=2).ok_or_else(||unsupported("unknown typed transparency paint scope"))?;
    let template=json_profile()?;let mut normalized=value.clone();for key in ["startTransparency","uniformTransparency"] {normalized[key]=template[key].clone();}if scope!=2{normalized["appliesTo"]=template["appliesTo"].clone();}
    if normalized!=template {return Err(unsupported("native21 transparency mask/mode/applies-to differs"));}
    let opacity=(100.-ratio*100.)/100.;
    if (style.opacity-opacity).abs()>1e-14 {return Err(unsupported("native21 transparency disagrees with native effect record"));}
    // Keep the native binary effect value; JSON decimal ratios can add another
    // float rounding. Local precision fields bind to the binary value.
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_fill_and_outline_effect_profiles_are_not_misread_as_whole_opacity(){
        for version in [14,21]{for part in ["fill","outline"]{let raw=std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../examples/assets/cdr/corel2019/scoped-alpha-controls/sdk-{part}50-v{version}.cdr"))).unwrap();let native=read_native(&raw).unwrap();let fills=all(&native.nodes).into_iter().filter(|n|n.tag==*b"fild").map(|n|(u32_at(&n.data,0).unwrap(),n.data.as_slice())).collect();
            let n=all(&native.nodes).into_iter().find(|n|n.kind()==Some(b"obj ")&&super::super::write::arguments(&n.find(b"loda").unwrap().data).is_ok_and(|(_,a)|a.iter().any(|(k,_)|*k==8000))).unwrap();let(_,args)=super::super::write::arguments(&n.find(b"loda").unwrap().data).unwrap();let effect=read_effect(&args.iter().find(|(k,_)|*k==8000).unwrap().1,&fills,version==21).unwrap();assert_eq!(effect.scope,if part=="fill"{0}else{1});assert_eq!(effect.opacity,0.5);
            let document=crate::cdr::decode(&raw,"native").unwrap().0;assert_eq!(document.objects.len(),1);let style=&document.objects[0].style;assert_eq!(style.opacity,1.);
            let (active,other)=if part=="fill"{(&style.fill,&style.stroke)}else{(&style.stroke,&style.fill)};
            assert_eq!(crate::paint::parse(active).unwrap().alpha(),0.5,"50percent must not be rounded to128/255");assert_eq!(crate::paint::parse(other).unwrap().alpha(),1.);
            let bytes=if version==14{crate::cdr::encode_document(&document)}else{crate::cdr::encode_document21(&document)}.unwrap().0;let copy=crate::cdr::decode(&bytes,"native").unwrap().0;assert_eq!(copy,document);
            let output=read_native(&bytes).unwrap();assert_eq!(all(&output.nodes).iter().filter(|n|n.kind()==Some(b"obj ")).count(),1,"a scoped effect must keep one real native primitive");
        }}
    }
    #[test]
    fn native_scope_and_alpha_edits_take_priority_over_old_source_scalars(){
        let source:crate::Document=serde_json::from_value(serde_json::json!({"id":"edit","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"paint","page_id":"page-1","x_mm":40.,"y_mm":60.,"width_mm":60.,"height_mm":40.,"geometry":{"kind":"rect","radius_mm":0.},"style":{"fill":"#1e5ab480","stroke":"#dc5028ff","stroke_width_mm":12.,"opacity":1.}}]})).unwrap();
        for version in [14,21]{for change_scope in [false,true]{let encoded=if version==14{crate::cdr::encode_document(&source)}else{crate::cdr::encode_document21(&source)}.unwrap().0;let mut native=read_native(&encoded).unwrap();
            fn change(ns:&mut [Node],scope:u32){for n in ns{if n.kind()==Some(b"obj "){let load=n.find_mut(b"loda").unwrap();let(k,mut a)=super::super::write::arguments(&load.data).unwrap();if let Some((_,effect))=a.iter_mut().find(|(k,_)|*k==8000){set32(effect,89,scope).unwrap();effect[69..77].copy_from_slice(&percent(64./255.).to_le_bytes());if let Some((_,typed))=a.iter_mut().find(|(k,_)|*k==201){let mut value=super::super::modern::style_record(typed).unwrap();value["transparency"]=json_scoped(64./255.,scope).unwrap();*typed=super::super::modern::encoded_style(&value).unwrap();}load.data=super::super::write::loda(k,&a);}}if let Some(c)=&mut n.children{change(c,scope);}}}
            change(&mut native.nodes,u32::from(change_scope));let bytes=if version==14{{native.members.insert("content/riffData.cdr".into(),super::super::binary::write(&native.nodes).unwrap());super::super::binary::zip(&native.members.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>())}}else{super::super::binary::repack21(&native).unwrap()};let doc=crate::cdr::decode(&bytes,"edit").unwrap().0;let style=&doc.object("paint").unwrap().style;assert_eq!(style.opacity,1.);assert_eq!(style.fill,if change_scope{"#1e5ab4ff"}else{"#1e5ab440"});assert_eq!(style.stroke,if change_scope{"#dc502840"}else{"#dc5028ff"});
        }}
    }
    #[test]
    fn continuous_native_percentage_edits_override_float_and_byte_source_notes(){
        for source_paint in ["#1e5ab480","rgba(30,90,180,0.5)"]{for version in [14,21]{for scope in [0,1,2]{for alpha in [0.25,0.12345678901234568]{
            let source:crate::Document=serde_json::from_value(serde_json::json!({"id":"edit-float","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"paint","page_id":"page-1","x_mm":40.,"y_mm":60.,"width_mm":60.,"height_mm":40.,"geometry":{"kind":"rect","radius_mm":0.},"style":{"fill":source_paint,"stroke":"#dc5028ff","stroke_width_mm":12.,"opacity":1.}}]})).unwrap();
            let encoded=if version==14{crate::cdr::encode_document(&source)}else{crate::cdr::encode_document21(&source)}.unwrap().0;let mut native=read_native(&encoded).unwrap();
            fn edit(ns:&mut[Node],scope:u32,alpha:f64){for n in ns{if n.kind()==Some(b"obj "){let load=n.find_mut(b"loda").unwrap();let(k,mut a)=super::super::write::arguments(&load.data).unwrap();if let Some((_,effect))=a.iter_mut().find(|(k,_)|*k==8000){set32(effect,89,scope).unwrap();effect[69..77].copy_from_slice(&percent(alpha).to_le_bytes());if let Some((_,typed))=a.iter_mut().find(|(k,_)|*k==201){let mut v=super::super::modern::style_record(typed).unwrap();v["transparency"]=json_scoped(alpha,scope).unwrap();*typed=super::super::modern::encoded_style(&v).unwrap();}load.data=super::super::write::loda(k,&a);}}if let Some(c)=&mut n.children{edit(c,scope,alpha);}}}
            edit(&mut native.nodes,scope,alpha);let bytes=if version==21{repack21(&native).unwrap()}else{write(&native.nodes).unwrap()};let doc=crate::cdr::decode(&bytes,"edit-float").unwrap().0;let s=&doc.objects[0].style;let effective=(100.-percent(alpha))/100.;
            assert_eq!(s.opacity,if scope==2{effective}else{1.});assert_eq!(crate::paint::parse(&s.fill).unwrap().alpha(),if scope==0{effective}else{1.});assert_eq!(crate::paint::parse(&s.stroke).unwrap().alpha(),if scope==1{effective}else{1.});
        }}}}
    }
    #[test]
    fn ordinary_text_keeps_one_actual_story_and_rejects_conflicting_story_scope(){
        let mut source=crate::Document::new("single-story".into(),210.,297.).unwrap();source.objects.push(serde_json::from_value(serde_json::json!({"id":"text","page_id":"page-1","x_mm":40.,"y_mm":60.,"width_mm":60.,"height_mm":40.,"geometry":{"kind":"text","content":"One story","font_family":"Arial","font_size_mm":8.},"style":{"fill":"rgba(30,90,180,0.5)","stroke":"#dc5028","stroke_width_mm":0.25,"opacity":1.}})).unwrap());
        for version in [14,21]{let raw=if version==14{crate::cdr::encode_document(&source)}else{crate::cdr::encode_document21(&source)}.unwrap().0;let mut native=read_native(&raw).unwrap();assert_eq!(all(&native.nodes).iter().filter(|n|n.tag==*b"txsm").count(),1);assert_eq!(all(&native.nodes).iter().filter(|n|n.kind()==Some(b"obj ")).count(),1);
            if version==21{fn edit(nodes:&mut[Node])->usize{let mut changed=0;for n in nodes{if n.tag==*b"txsm"{let needle=b"\"appliesTo\":\"0\"";let at=n.data.windows(needle.len()).position(|v|v==needle).unwrap();n.data[at+needle.len()-2]=b'2';changed+=1;}if let Some(c)=&mut n.children{changed+=edit(c);}}changed}assert_eq!(edit(&mut native.nodes),1);assert_eq!(crate::cdr::decode(&repack21(&native).unwrap(),"bad-scope").unwrap_err().code,"CDR_UNSUPPORTED");}
        }
    }
    #[test]
    #[ignore = "controlled native files; run explicitly with ARRANGER_CDR_GROUP_PROBE"]
    fn native_group_composition_probe_keeps_editable_members_and_measured_effect_payload() {
        let directory=std::env::var_os("ARRANGER_CDR_GROUP_PROBE").expect("set a fresh probe directory");
        let directory=std::path::PathBuf::from(directory);std::fs::create_dir(&directory).unwrap();
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports/windows/WIN-SAVE-CLOSURE-20261024-01/controls");
        let mut cases=vec![];
        for version in [14,21] {for variant in ["repacked-control","group-effect","group-style","group-effect-flags","group-effect-flags-id"] {
            if version==14&&variant=="group-style" {continue;}
            if version==21&&variant.starts_with("group-effect-flags") {continue;}
            let source=root.join(format!("opacity-group-overlap-v{version}.cdr"));let mut native=read_native(&std::fs::read(&source).unwrap()).unwrap();
            if variant!="repacked-control" {
                let mut effect=None;
                fn clear(nodes:&mut [Node],effect:&mut Option<Vec<u8>>)->Result<()> {
                    for n in nodes {
                        if n.kind()==Some(b"obj ") {
                            let load=n.find_mut(b"loda")?;let (kind,mut args)=super::super::write::arguments(&load.data)?;
                            if let Some((_,raw))=args.iter().find(|(k,_)|*k==8000){if effect.is_none(){*effect=Some(raw.clone());}}
                            args.retain(|(k,_)|*k!=8000);
                            if let Some((_,raw))=args.iter_mut().find(|(k,_)|*k==201){let mut style=super::super::modern::style_record(raw)?;style.as_object_mut().unwrap().remove("transparency");*raw=super::super::modern::encoded_style(&style)?;}
                            load.data=super::super::write::loda(kind,&args);
                            let flag=n.find_mut(b"flgs")?;let value=u32_at(&flag.data,0)?;flag.data=(value&!0x80000).to_le_bytes().to_vec();
                        }
                        if let Some(c)=&mut n.children{clear(c,effect)?;}
                    }Ok(())
                }
                clear(&mut native.nodes,&mut effect).unwrap();let effect=effect.unwrap();let mut groups=0;
                fn group(nodes:&mut [Node],effect:&[u8],variant:&str,modern:bool,groups:&mut usize)->Result<()> {
                    for n in nodes {
                        if n.kind()==Some(b"grp ") {
                            let key=super::super::clipping::key(n)?;
                            let load=n.find_mut(b"loda")?;let (kind,mut args)=super::super::write::arguments(&load.data)?;
                            if variant.starts_with("group-effect") {
                                let mut payload=effect.to_vec();
                                if variant=="group-effect-flags-id" {set32(&mut payload,9,key)?;}
                                args.push((8000,payload));
                            }
                            if modern {if let Some((_,raw))=args.iter_mut().find(|(k,_)|*k==201){let mut style=super::super::modern::style_record(raw)?;style["transparency"]=json(0.7)?;*raw=super::super::modern::encoded_style(&style)?;}else{args.push((201,super::super::modern::encoded_style(&serde_json::json!({"transparency":json(0.7)?}))?));}}
                            load.data=super::super::write::loda(kind,&args);
                            if variant.starts_with("group-effect-flags") {
                                let flags=n.find_mut(b"flgs")?;let value=u32_at(&flags.data,0)?;flags.data=(value|0x80000).to_le_bytes().to_vec();
                            }
                            *groups+=1;
                        }
                        if let Some(c)=&mut n.children{group(c,effect,variant,modern,groups)?;}
                    }Ok(())
                }
                group(&mut native.nodes,&effect,variant,version==21,&mut groups).unwrap();assert_eq!(groups,1);
            }
            assert_eq!(all(&native.nodes).into_iter().filter(|n|n.kind()==Some(b"obj ")).count(),2);
            let file=format!("composition-v{version}-{variant}.cdr");
            let bytes=if version==14{super::super::binary::write(&native.nodes).unwrap()}else{super::super::binary::repack21(&native).unwrap()};
            std::fs::write(directory.join(&file),&bytes).unwrap();
            cases.push(serde_json::json!({"file":file,"version":version,"variant":variant,"source":source,"sha256":crate::model::sha256(&bytes),"model_writer":false}));
        }}
        std::fs::write(directory.join("manifest.json"),serde_json::to_vec_pretty(&serde_json::json!({"cases":cases,"scope":"Measured ordinary transparency transplanted onto a live native group; controlled research, not a supported codec or compatibility result."})).unwrap()).unwrap();
    }
    #[test]
    fn all_exact_integer_percentages_stay_exact_without_snapping_non_grid_alpha() {
        for p in 1..=100 {let value=(100-p) as f64/100.;let raw=encode(value,1,2).unwrap().unwrap();assert_eq!(f64_at(&raw,69).unwrap(),p as f64);assert_eq!(decode(&raw).unwrap(),value);}
        let value=f64::from_bits(0.8f64.to_bits()+1);assert_eq!(percent(value),(1.-value)*100.);assert_ne!(percent(value),20.);
    }
    #[test]
    fn genuine_original_and_edited_uniform_controls_match_sdk_percentages() {
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/cdr").join(ROOT);
        for version in [14,21] {for kind in ["opacity-rect","opacity-group-overlap"] {for suffix in ["","-edited"] {
            let native=read_native(&std::fs::read(root.join(format!("{kind}-v{version}{suffix}.cdr"))).unwrap()).unwrap();let mut count=0;
            for node in all(&native.nodes).into_iter().filter(|n|n.kind()==Some(b"obj ")) {
                let (_,args)=super::super::write::arguments(&super::super::read::own_record(node,b"loda").unwrap().data).unwrap();let Some((_,raw))=args.iter().find(|(k,_)|*k==8000) else{continue};assert_eq!(decode(raw).unwrap(),0.7);count+=1;
                if version==21 {let style=super::super::modern::style_record(&args.iter().find(|(k,_)|*k==201).unwrap().1).unwrap();let mut model=Style{opacity:0.7,..Default::default()};modern(&style,&mut model).unwrap();assert_eq!(model.opacity,0.7);}
            }assert_eq!(count,if kind=="opacity-rect"{1}else{2});
        }}}
    }
    #[test]
    fn dangling_mask_and_changed_composition_reject_complete_native_documents() {
        for version in [14,21] {for mode in 0..4 {
            let mut native=read_native(if version==14{CONTROL14}else{CONTROL21}).unwrap();
            fn edit(nodes:&mut [Node],mode:u32,changed:&mut bool) {
                for n in nodes {
                    if n.kind()==Some(b"obj ")&&!*changed {
                        let load=n.find_mut(b"loda").unwrap();let (kind,mut args)=super::super::write::arguments(&load.data).unwrap();
                        if let Some((_,raw))=args.iter_mut().find(|(k,_)|*k==8000) {
                            if mode==0{set32(raw,113,999999).unwrap();}else if mode==1{set32(raw,89,0).unwrap();}else if mode==2{set32(raw,9,0).unwrap();}else{raw[69..77].copy_from_slice(&f64::NAN.to_le_bytes());}
                            load.data=super::super::write::loda(kind,&args);*changed=true;
                        }
                    }
                    if let Some(c)=&mut n.children{edit(c,mode,changed);}
                }
            }
            let mut changed=false;edit(&mut native.nodes,mode,&mut changed);assert!(changed);
            let raw=if version==14{super::super::binary::write(&native.nodes).unwrap()}else{repack21(&native).unwrap()};
            let result=std::panic::catch_unwind(||crate::cdr::decode(&raw,"damaged-opacity"));assert!(result.is_ok());assert!(result.unwrap().is_err(),"{version}/{mode}");
        }}
    }
    #[test]
    fn exact_uniform_values_and_unknown_modes_never_become_opaque_defaults() {
        for value in [0.,0.1,0.2,0.25,0.5,0.7,0.75,0.9] {assert_eq!(decode(&encode(value,1,2).unwrap().unwrap()).unwrap(),value);}
        assert!(encode(0.12345,1,2).is_ok());let original=encode(0.7,1,2).unwrap().unwrap();for at in [0,13,85,89,117] {let mut raw=original.clone();raw[at]^=1;assert!(decode(&raw).is_err(),"{at}");}
        let mut value=json(0.7).unwrap();value["mode"]=Value::String("1".into());assert!(modern(&serde_json::json!({"transparency":value}),&mut Style{opacity:0.7,..Default::default()}).is_err());
    }
}
