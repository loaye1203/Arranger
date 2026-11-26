//! Original font aliases and scalar layout precision; never story content.
//! Native edits take precedence whenever their measured projection changes.
use super::{binary::coord,invalid,unsupported,Result};
use crate::{Document,Geometry,TextAnchor,TextLayoutOptions};
use serde::{Deserialize,Serialize};
use std::collections::{BTreeMap,BTreeSet};

const PREFIX:&str="ArrangerTextParametersV1|sha256:";
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Parameters {version:u32,text:BTreeMap<String,TextParameters>}
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct TextParameters {
    font_family:String,font_size_mm:f64,layout:TextLayoutOptions,
    #[serde(default,skip_serializing_if="Vec::is_empty")]line_breaks:Vec<u8>,
    #[serde(default="one",skip_serializing_if="is_one")]path_scale:f64,
    #[serde(default,skip_serializing_if="Option::is_none")]native_family:Option<String>,
    #[serde(default,skip_serializing_if="Option::is_none")]native_tracking:Option<i32>,
    #[serde(default,skip_serializing_if="Option::is_none")]path_layout:Option<PathScalars>,
    #[serde(default,skip_serializing_if="Option::is_none")]path_translation_world_mm:Option<[f64;2]>,
    #[serde(default,skip_serializing_if="Option::is_none")]path_uniform_world:Option<[f64;3]>,
    #[serde(default,skip_serializing_if="Option::is_none")]path_quarter_world:Option<QuarterPlacement>,
    #[serde(default,skip_serializing_if="Option::is_none")]path_outline_width_mm:Option<f64>,
}
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct PathScalars {start_offset_mm:f64,baseline_offset_mm:f64,anchor:TextAnchor}
#[derive(Clone,Copy,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct QuarterPlacement {turns:u8,translation_mm:[f64;2]}
impl QuarterPlacement{fn apply(self,o:&mut crate::Object){o.x_mm=0.;o.y_mm=0.;o.transform=super::path_rotation::matrix(self.turns,self.translation_mm);}}
fn one()->f64{1.}fn is_one(value:&f64)->bool{*value==1.}

pub(super) fn notes(doc:&Document)->Result<Option<String>> {
    let mut text=BTreeMap::new();
    for o in &doc.objects {if let Geometry::Text{content,font_family,font_size_mm,layout,text_path}=&o.geometry {
        let path_scale=if text_path.is_some(){super::path_text::parent_scale(doc,o)?}else{1.};let native=if text_path.is_some(){super::path_text::native_object(doc,o)?}else{o.clone()};
        let own=if text_path.is_some(){super::path_text::own_scale(o)?}else{1.};let delta=if text_path.is_some(){super::path_text::translation_world(doc,o)?}else{None};
        let turn=if text_path.is_some(){super::path_text::own_turn(o)}else{None};
        text.insert(o.id.clone(),TextParameters{path_quarter_world:turn.map(|turns|QuarterPlacement{turns,translation_mm:delta.unwrap()}),path_outline_width_mm:(o.style.stroke!="none"&&super::path_outline::needs_check(doc,o)?).then_some(o.style.stroke_width_mm),path_uniform_world:if own!=1.{let[x,y]=delta.unwrap();Some([own,x,y])}else{None},path_translation_world_mm:if own==1.&&turn.is_none(){delta}else{None},path_scale,line_breaks:breaks(content),font_family:font_family.clone(),font_size_mm:*font_size_mm,layout:layout.clone(),native_family:Some(super::fonts::native_family(font_family)?),native_tracking:Some(super::typography::tracking(doc,&native)?),path_layout:text_path.as_ref().map(|p|PathScalars{start_offset_mm:p.start_offset_mm,baseline_offset_mm:p.baseline_offset_mm,anchor:p.anchor})});
    }}
    if text.is_empty(){return Ok(None);}
    // Version2 extends outline precision to unscaled fitted stories. Earlier
    // readers must refuse that chart rather than silently lose fine pen units.
    let version=if text.values().any(|p|p.path_quarter_world.is_some()){3}else if text.values().any(|p|p.path_outline_width_mm.is_some()&&p.path_uniform_world.is_none()){2}else{1};
    super::metadata::encode(PREFIX,&Parameters{version,text}).map(Some)
}
pub(super) fn read(members:&BTreeMap<String,Vec<u8>>)->Result<Option<Parameters>> {
    super::metadata::find(members,PREFIX)?.map(|s|super::metadata::decode(PREFIX,&s)).transpose()
}
pub(super) fn quarter_names(params:&Parameters)->BTreeSet<String>{params.text.iter().filter(|(_,p)|p.path_quarter_world.is_some()).map(|(id,_)|id.clone()).collect()}
// Formatting only: 0=LF, 1=CR, 2=CRLF. No story characters are retained.
fn breaks(content:&str)->Vec<u8>{let mut result=vec![];let mut chars=content.chars().peekable();while let Some(c)=chars.next(){if c=='\r'{result.push(if chars.peek()==Some(&'\n'){chars.next();2}else{1});}else if c=='\n'{result.push(0);}}if result.iter().all(|v|*v==0){vec![]}else{result}}
fn restore_breaks(content:&mut String,formats:&[u8])->Result<()> {
    if formats.len()>65534||formats.iter().any(|v|*v>2){return Err(invalid("invalid source line-ending formats"));}
    if formats.is_empty()||content.bytes().filter(|v|*v==b'\n').count()!=formats.len(){return Ok(());}
    let mut result=String::new();let mut n=0;for c in content.chars(){if c=='\n'{result.push_str(match formats[n]{0=>"\n",1=>"\r",_=>"\r\n"});n+=1;}else{result.push(c);}}*content=result;Ok(())
}
fn quantized(value:f64)->Result<f64>{Ok(coord(value)? as f64/10000.)}
fn same_number(a:f64,b:f64)->bool{(a-b).abs()<=1e-10}
#[cfg(test)]
pub(super) fn restore(doc:&mut Document,params:Parameters)->Result<(usize,usize)> {
    restore_with_charts(doc,params,&BTreeMap::new())
}
#[cfg(test)]
pub(super) fn restore_with_charts(doc:&mut Document,params:Parameters,charts:&BTreeMap<String,f64>)->Result<(usize,usize)> {
    restore_with_placements(doc,params,charts,&mut BTreeSet::new())
}
pub(super) fn restore_with_placements(doc:&mut Document,params:Parameters,charts:&BTreeMap<String,f64>,discarded:&mut BTreeSet<String>)->Result<(usize,usize)> {
    if ![1,2,3].contains(&params.version){return Err(unsupported("unknown native text parameter version"));}
    let native=doc.clone();let mut exact=0;let mut edited=0;
    for (id,p) in params.text {
        if !p.path_scale.is_finite()||p.path_scale<=0.||p.path_layout.is_none()&&p.path_scale!=1.{return Err(invalid("invalid native path font/offset scale"));}
        if p.path_translation_world_mm.is_some_and(|v|p.path_layout.is_none()||v.into_iter().any(|v|!v.is_finite())){return Err(invalid("invalid native path translation scalar"));}
        if p.path_uniform_world.is_some_and(|v|p.path_layout.is_none()||p.path_translation_world_mm.is_some()||v.into_iter().any(|n|!n.is_finite())||v[0]<=0.||v[0]==1.){return Err(invalid("invalid native path uniform placement scalars"));}
        if p.path_quarter_world.is_some_and(|v|params.version!=3||p.path_layout.is_none()||p.path_uniform_world.is_some()||p.path_translation_world_mm.is_some()||!(1..=3).contains(&v.turns)||v.translation_mm.into_iter().any(|n|!n.is_finite())){return Err(invalid("invalid native path quarter-turn placement"));}
        if p.path_outline_width_mm.is_some_and(|v|p.path_layout.is_none()||params.version==1&&p.path_uniform_world.is_none()||!v.is_finite()||v<=0.){return Err(invalid("invalid native fitted outline width scalar"));}
        let own=p.path_uniform_world.map_or(1.,|v|v[0]);let placed=p.path_translation_world_mm.is_some()||p.path_uniform_world.is_some()||p.path_quarter_world.is_some();
        let chart=charts.get(&id).copied().unwrap_or(1.);let same_chart=p.path_scale==chart;
        if !crate::model::valid_id(&id){return Err(invalid("invalid native text parameter name"));}
        let Some(index)=doc.objects.iter().position(|o|o.id==id) else{continue};
        let observed=&native.objects[index];
        let Geometry::Text{content,font_family,font_size_mm,layout,text_path}=&observed.geometry else{continue};
        if layout.mode!=p.layout.mode||text_path.is_some()!=p.path_layout.is_some(){if placed{discarded.insert(id.clone());}edited+=1;continue;}
        let mut source=observed.clone();
        if let Geometry::Text{font_family,font_size_mm,layout,text_path,..}=&mut source.geometry {
            *font_family=p.font_family.clone();*font_size_mm=p.font_size_mm;*layout=p.layout.clone();
            if let (Some(actual),Some(old))=(text_path,&p.path_layout){actual.start_offset_mm=old.start_offset_mm;actual.baseline_offset_mm=old.baseline_offset_mm;actual.anchor=old.anchor;}
        }
        crate::text::validate_text(content,&p.font_family,p.font_size_mm)?;
        crate::text::validate_options(&source)?;
        let path_same=match &p.path_layout {
            Some(values)=>{
                if !values.start_offset_mm.is_finite()||!values.baseline_offset_mm.is_finite(){return Err(invalid("non-finite native text-path scalar"));}
                let mut expected=source.clone();super::path_text::scale_text(&mut expected,p.path_scale);let mut actual=observed.clone();super::path_text::scale_text(&mut actual,chart);
                if let Some([x,y])=p.path_translation_world_mm{expected.x_mm=x;expected.y_mm=y;expected.transform=kurbo::Affine::IDENTITY.as_coeffs();}
                if let Some([s,x,y])=p.path_uniform_world{expected.x_mm=0.;expected.y_mm=0.;expected.transform=[s,0.,0.,s,x,y];}
                if let Some(rotation)=p.path_quarter_world{rotation.apply(&mut expected);}
                let actual=super::path_text::encoded_options(&native,&actual)?;
                same_chart&&if placed{super::path_text::encoded_options(&native,&expected).is_ok_and(|v|v==actual)}else{super::path_text::encoded_options(&native,&expected)?==actual}
            },
            None=>true,
        };
        if !path_same{discarded.insert(id.clone());}
        // Resolve at export time. A missing old custom font must never stop
        // import after Corel has selected a valid native replacement.
        let family=p.native_family.or_else(||super::fonts::native_family(&p.font_family).ok());
        let family_same=family.is_some_and(|v|v.eq_ignore_ascii_case(font_family));
        let size_same=same_chart&&same_number(*font_size_mm,quantized(p.font_size_mm*p.path_scale*own)?/chart);
        if p.native_tracking.is_some_and(|v|!(-1_000_000..=20_000_000).contains(&v)){return Err(invalid("native tracking projection outside range"));}
        let tracking=p.native_tracking.or_else(||super::typography::tracking(&native,&source).ok());
        let tracking_same=family_same&&size_same&&tracking==Some(super::typography::tracking(&native,observed)?);
        let expected_leading=quantized(p.layout.line_height_mm.unwrap_or(p.font_size_mm*1.2)*p.path_scale*own)?/chart;
        let leading_same=family_same&&size_same&&layout.line_height_mm.is_some_and(|v|same_number(v,expected_leading));
        // Own scale, its font units and its fit placement form one chart.
        // On any native style/fit change keep all decoded units and suppress
        // the old source matrix, rather than applying its scale a second time.
        if (p.path_uniform_world.is_some()||p.path_quarter_world.is_some())&&!(path_same&&family_same&&size_same&&tracking_same&&leading_same&&(observed.style.stroke=="none"||p.path_outline_width_mm.is_some())){discarded.insert(id.clone());edited+=1;continue;}
        if let Some(rotation)=p.path_quarter_world{if p.path_outline_width_mm.is_none(){
            let mut actual=observed.clone();super::path_text::scale_text(&mut actual,chart);let mut expected=source.clone();super::path_text::scale_text(&mut expected,p.path_scale);rotation.apply(&mut expected);
            let mut actual_doc=super::path_outline::scene(&native,&id)?;*actual_doc.objects.iter_mut().find(|o|o.id==id).unwrap()=actual.clone();let mut expected_doc=actual_doc.clone();*expected_doc.objects.iter_mut().find(|o|o.id==id).unwrap()=expected.clone();
            if !super::path_outline::preserves(&actual_doc,&actual,&expected_doc,&expected)?{discarded.insert(id.clone());edited+=1;continue;}
        }}
        if let Some(old_width)=p.path_outline_width_mm{if observed.style.stroke!="none"&&path_same&&family_same&&size_same&&tracking_same&&leading_same{
            // A changed native width is retained in source units. An unchanged
            // quantized projection can propose the original fine scalar, but
            // only together with the actual glyph/pen control-hull comparison.
            let [s,x,y]=p.path_uniform_world.unwrap_or_else(||{let[x,y]=p.path_translation_world_mm.unwrap_or([0.,0.]);[1.,x,y]});
            let same_width=coord(observed.style.stroke_width_mm*chart)?==coord(old_width*(chart*s))?;
            let width=if same_width{old_width}else{observed.style.stroke_width_mm/s};
            let mut actual=observed.clone();super::path_text::scale_text(&mut actual,chart);
            let mut expected=source.clone();super::path_text::scale_text(&mut expected,p.path_scale);expected.x_mm=0.;expected.y_mm=0.;expected.transform=[s,0.,0.,s,x,y];expected.style.stroke_width_mm=width*chart;
            if let Some(rotation)=p.path_quarter_world{rotation.apply(&mut expected);}
            let mut actual_doc=super::path_outline::scene(&native,&id)?;*actual_doc.objects.iter_mut().find(|o|o.id==id).unwrap()=actual.clone();let mut expected_doc=actual_doc.clone();*expected_doc.objects.iter_mut().find(|o|o.id==id).unwrap()=expected.clone();
            if !super::path_outline::preserves(&actual_doc,&actual,&expected_doc,&expected)?{discarded.insert(id.clone());edited+=1;continue;}
            doc.objects[index].style.stroke_width_mm=width;
        }}
        let Geometry::Text{content,font_family,font_size_mm,layout:restored,text_path}=&mut doc.objects[index].geometry else{unreachable!()};
        restore_breaks(content,&p.line_breaks)?;
        if family_same{*font_family=p.font_family;}
        if family_same&&size_same{*font_size_mm=p.font_size_mm;}
        if tracking_same{restored.letter_spacing_mm=p.layout.letter_spacing_mm;}
        if leading_same{restored.line_height_mm=p.layout.line_height_mm;}
        if path_same{if let (Some(actual),Some(old))=(text_path,p.path_layout){actual.start_offset_mm=old.start_offset_mm;actual.baseline_offset_mm=old.baseline_offset_mm;actual.anchor=old.anchor;}}
        // Alignment, point/area mode, content and path bindings are native.
        if family_same&&size_same&&tracking_same&&leading_same&&path_same&&restored.mode==p.layout.mode&&restored.align==p.layout.align{exact+=1;}else{edited+=1;}
    }
    doc.validate()?;Ok((exact,edited))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn source()->Document {serde_json::from_value(json!({"id":"scalar-test","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"text","page_id":"page-1","x_mm":20.,"y_mm":30.,"width_mm":100.,"height_mm":20.,"geometry":{"kind":"text","content":"Live text 中文","font_family":"Microsoft YaHei","font_size_mm":4.1234567,"layout":{"letter_spacing_mm":0.12345678}}}]})).unwrap()}
    fn params(doc:&Document)->Parameters{super::super::metadata::decode(PREFIX,&notes(doc).unwrap().unwrap()).unwrap()}
    #[test]
    fn invalid_or_legacy_quarter_turn_hints_refuse_before_restoring_text(){
        let source=source();for mode in 0..5{let mut p=params(&source);p.version=3;let t=p.text.get_mut("text").unwrap();t.path_layout=Some(PathScalars{start_offset_mm:15.,baseline_offset_mm:0.125,anchor:TextAnchor::Start});t.path_quarter_world=Some(QuarterPlacement{turns:1,translation_mm:[210.,20.]});
            match mode{0=>p.version=2,1=>t.path_quarter_world.as_mut().unwrap().turns=0,2=>t.path_quarter_world.as_mut().unwrap().turns=4,3=>t.path_quarter_world.as_mut().unwrap().translation_mm[0]=f64::NAN,_=>t.path_translation_world_mm=Some([0.,0.])};
            let mut actual=source.clone();assert!(restore(&mut actual,p).is_err());assert_eq!(actual,source);
        }
    }
    #[test]
    fn invalid_fitted_outline_scalars_do_not_change_native_text(){
        let source=source();for width in[f64::NAN,f64::INFINITY,-0.1,0.]{let mut p=params(&source);let text=p.text.get_mut("text").unwrap();text.path_uniform_world=Some([1.5,0.,0.]);text.path_layout=Some(PathScalars{start_offset_mm:0.,baseline_offset_mm:0.,anchor:TextAnchor::Start});text.path_outline_width_mm=Some(width);let mut actual=source.clone();assert!(restore(&mut actual,p).is_err());assert_eq!(actual,source);}
        let mut p=params(&source);p.text.get_mut("text").unwrap().path_outline_width_mm=Some(0.125);let mut actual=source.clone();assert!(restore(&mut actual,p).is_err());assert_eq!(actual,source);
    }
    #[test]
    fn unscaled_fitted_pen_uses_an_explicit_new_parameter_version_and_legacy_guard(){
        let source=super::super::path_appearance::source(false,true,1.,2);let p=params(&source);assert_eq!(p.version,2);
        let raw=super::super::encode_document(&source).unwrap().0;let native=super::super::read::decode_geometry(&raw,"native").unwrap().0;
        let mut legacy=params(&source);legacy.version=1;let mut unchanged=native.clone();assert!(restore(&mut unchanged,legacy).is_err());assert_eq!(unchanged,native);
        let mut restored=native;restore(&mut restored,p).unwrap();assert_eq!(restored.object("text").unwrap().style.stroke_width_mm,source.object("text").unwrap().style.stroke_width_mm);
    }
    #[test]
    fn line_ending_formats_do_not_restore_story_characters_or_deleted_paragraphs() {
        assert_eq!(breaks("a\r\nb\rc\nd"),vec![2,1,0]);
        let mut native="Changed\nActual\nEdited\nLast".to_string();restore_breaks(&mut native,&[2,1,0]).unwrap();assert_eq!(native,"Changed\r\nActual\rEdited\nLast");
        let mut fewer="Changed\nLast".to_string();restore_breaks(&mut fewer,&[2,1,0]).unwrap();assert_eq!(fewer,"Changed\nLast");
        assert!(restore_breaks(&mut fewer,&[3]).is_err());
        assert!(restore_breaks(&mut fewer,&vec![0;65535]).is_err());
        assert!(breaks("a\nb").is_empty());
    }
    #[test]
    fn native_font_size_spacing_and_alignment_edits_are_not_overwritten() {
        let source=source();let mut native=source.clone();
        if let Geometry::Text{font_family,font_size_mm,layout,content,..}=&mut native.objects[0].geometry {
            *font_family="Calibri".into();*font_size_mm=6.25;layout.letter_spacing_mm=0.5;layout.line_height_mm=Some(10.);layout.align=crate::TextAlign::Right;*content="Actual edit".into();
        }
        let expected=native.clone();assert_eq!(restore(&mut native,params(&source)).unwrap(),(0,1));assert_eq!(native.objects,expected.objects);
    }
    #[test]
    fn native_tracking_edit_remains_with_unchanged_font_alias_and_size_precision() {
        let source=source();let mut native=source.clone();
        if let Geometry::Text{font_family,font_size_mm,layout,content,..}=&mut native.objects[0].geometry {
            *font_family="微软雅黑".into();*font_size_mm=4.1235;layout.letter_spacing_mm=0.5;layout.line_height_mm=Some(4.9481);*content="New content".into();
        }
        assert_eq!(restore(&mut native,params(&source)).unwrap(),(0,1));
        let Geometry::Text{font_family,font_size_mm,layout,content,..}=&native.objects[0].geometry else{unreachable!()};
        assert_eq!(font_family,"Microsoft YaHei");assert_eq!(*font_size_mm,4.1234567);assert_eq!(layout.letter_spacing_mm,0.5);assert_eq!(layout.line_height_mm,None);assert_eq!(content,"New content");
    }
    #[test]
    fn orphaned_or_replaced_text_parameters_never_create_a_story() {
        let source=source();let mut native=source.clone();native.objects.clear();assert_eq!(restore(&mut native,params(&source)).unwrap(),(0,0));assert!(native.objects.is_empty());
        let mut native=source.clone();native.objects[0].geometry=Geometry::Ellipse;let expected=native.clone();assert_eq!(restore(&mut native,params(&source)).unwrap(),(0,0));assert_eq!(native.objects,expected.objects);
    }
    #[test]
    fn missing_old_custom_font_does_not_block_a_valid_native_font_replacement() {
        let source=source();let mut p=params(&source);p.text.get_mut("text").unwrap().font_family="Absent custom font 2026".into();
        let mut native=source.clone();if let Geometry::Text{font_family,..}=&mut native.objects[0].geometry{*font_family="Calibri".into();}
        let expected=native.clone();assert_eq!(restore(&mut native,p).unwrap(),(0,1));assert_eq!(native.objects,expected.objects);
    }
    #[test]
    fn converting_an_area_story_to_native_point_path_does_not_mix_old_layout_and_new_binding() {
        let mut source=source();if let Geometry::Text{layout,..}=&mut source.objects[0].geometry{layout.mode=crate::TextMode::Area;}
        let mut native=source.clone();let mut guide=native.objects[0].clone();guide.id="guide".into();guide.geometry=Geometry::Path{d:"M0 0 L100 0".into(),fill_rule:Default::default()};native.objects.push(guide);
        native.objects[0].x_mm=0.;native.objects[0].y_mm=0.;
        if let Geometry::Text{layout,text_path,..}=&mut native.objects[0].geometry{layout.mode=crate::TextMode::Point;*text_path=Some(crate::TextOnPath{path_id:"guide".into(),start_offset_mm:0.,baseline_offset_mm:0.,anchor:TextAnchor::Start});}
        native.validate().unwrap();let expected=native.clone();assert_eq!(restore(&mut native,params(&source)).unwrap(),(0,1));assert_eq!(native.objects,expected.objects);
    }
}
