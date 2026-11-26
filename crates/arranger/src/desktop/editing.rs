use super::*;
use arranger_core::{Document, Object, Resource, Style, Page, TextLayoutOptions};
use kurbo::{Affine, Point};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub(super) const CLIPBOARD_PREFIX: &str = "ARRANGER-OBJECTS-1\n";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Tool { #[default] Select, Nodes, Pan, Rect, Ellipse, Line, Pen, Text }
impl Tool {
    pub fn items() -> [(Self,&'static str,&'static str,&'static str);8] {
        [(Self::Select,"select","选择","Select (V)"),(Self::Nodes,"nodes","节点","Nodes (A)"),
         (Self::Pan,"pan","手形","Pan (H / Space)"),(Self::Rect,"rect","矩形","Rectangle (R)"),
         (Self::Ellipse,"ellipse","椭圆","Ellipse (E)"),(Self::Line,"line","直线","Line (L)"),
         (Self::Pen,"pen","钢笔","Pen (P)"),(Self::Text,"text","文字","Text (T)")]
    }
    pub fn label(self,zh:bool)->&'static str { let item=Self::items().into_iter().find(|i|i.0==self).unwrap();if zh{item.2}else{item.3} }
}

#[derive(Clone)]
pub(super) struct Editor {
    pub id:String,pub revision:u64,pub x:String,pub y:String,pub width:String,pub height:String,
    pub rotation:String,pub style:Style,pub geometry:Geometry,pub dirty:bool,pub text_dirty:bool,
    pub original_numbers:[f64;5],
}

/// Presentation only. Unedited fields always resolve to their original f64.
pub(super) fn display_number(value:f64)->String {
    let rounded=format!("{value:.3}");
    let compact=rounded.trim_end_matches('0').trim_end_matches('.');
    if compact=="0"||compact=="-0" {
        if value==0. {"0".into()} else {value.to_string()}
    } else {compact.into()}
}
pub(super) fn parse_displayed_number(text:&str,original:f64)->arranger_core::Result<f64> {
    if text==display_number(original){return Ok(original);}
    text.parse::<f64>().ok().filter(|v|v.is_finite())
        .ok_or_else(||arranger_core::Error::new("INVALID_NUMBER","Enter numeric millimetre values"))
}
impl Editor {
    pub fn new(doc:&Document,o:&Object,revision:u64)->Self {
        let [x,y,w,h]=doc.bounds(o).unwrap_or([o.x_mm,o.y_mm,o.width_mm,o.height_mm]);
        let [a,b,..]=doc.world_transform(o).unwrap_or(Affine::IDENTITY).as_coeffs();
        let rotation=b.atan2(a).to_degrees();
        Self{id:o.id.clone(),revision,x:display_number(x),y:display_number(y),width:display_number(w),height:display_number(h),
            rotation:display_number(rotation),style:o.style.clone(),geometry:o.geometry.clone(),dirty:false,text_dirty:false,original_numbers:[x,y,w,h,rotation]}
    }
    fn numbers(&self)->arranger_core::Result<[f64;5]> {
        let mut values=self.original_numbers;
        for (i,text) in [&self.x,&self.y,&self.width,&self.height,&self.rotation].into_iter().enumerate(){
            values[i]=parse_displayed_number(text,values[i])?;
        }
        Ok(values)
    }
}

pub(super) fn roots(doc:&Document,ids:&[String])->Vec<String> {
    doc.objects.iter().filter(|o|ids.contains(&o.id)).filter(|o|{
        let mut parent=o.parent_id.as_deref();
        while let Some(id)=parent {if ids.iter().any(|s|s==id){return false;}parent=doc.object(id).ok().and_then(|p|p.parent_id.as_deref());}
        true
    }).map(|o|o.id.clone()).collect()
}
/// Convert selected text/shapes, including group descendants, in document order.
/// Existing curves and images are left alone; a locked target rejects the batch.
pub(super) fn curves_operations(doc:&Document,ids:&[String])->arranger_core::Result<Vec<Operation>> {
    let selected:BTreeSet<_>=ids.iter().map(String::as_str).collect();
    let mut ops=vec![];
    for object in &doc.objects {
        let mut current=Some(object.id.as_str());let mut included=false;
        while let Some(id)=current {if selected.contains(id){included=true;break;}current=doc.object(id)?.parent_id.as_deref();}
        if !included {continue;}
        let op=match object.geometry {
            Geometry::Text{..}=>Operation::TextToPath{object_id:object.id.clone()},
            Geometry::Rect{..}|Geometry::Ellipse=>Operation::ObjectToPath{object_id:object.id.clone()},
            _=>continue,
        };
        doc.require_editable(&object.id)?;ops.push(op);
    }
    Ok(ops)
}
pub(super) fn curves_label(zh:bool)->&'static str {
    if cfg!(target_os="macos"){tr(zh,"转为曲线  ⌘⇧O","Convert to curves  ⌘⇧O")}
    else{tr(zh,"转为曲线  Ctrl+Q","Convert to curves  Ctrl+Q")}
}
pub(super) fn top_object(doc:&Document,id:String)->String {
    let mut id=id;
    while let Ok(o)=doc.object(&id) {if let Some(parent)=&o.parent_id {id=parent.clone();}else{break;}}
    id
}
fn parent_transform(doc:&Document,o:&Object)->arranger_core::Result<Affine> {
    o.parent_id.as_deref().map_or(Ok(Affine::IDENTITY),|id|doc.world_transform(doc.object(id)?))
}
fn style_delta(old:&Style,new:&Style,target:&Style,opacity:bool)->Style {
    let mut result=target.clone();
    if old.fill!=new.fill{result.fill=new.fill.clone();}if old.stroke!=new.stroke{result.stroke=new.stroke.clone();}
    if old.stroke_width_mm!=new.stroke_width_mm{result.stroke_width_mm=new.stroke_width_mm;}
    if old.stroke_scale_with_object!=new.stroke_scale_with_object{result.stroke_scale_with_object=new.stroke_scale_with_object;}
    if old.stroke_fixed_nib!=new.stroke_fixed_nib{result.stroke_fixed_nib=new.stroke_fixed_nib;}
    if opacity&&old.opacity!=new.opacity{result.opacity=new.opacity;}
    result
}
pub(super) fn translate(doc:&Document,ids:&[String],dx:f64,dy:f64)->arranger_core::Result<Vec<Operation>> {
    roots(doc,ids).iter().map(|id|{
        let o=doc.object(id)?;doc.require_editable(id)?;
        let matrix=parent_transform(doc,o)?*Affine::new(o.transform);
        if matrix.determinant().abs()<1e-12 {return Err(arranger_core::Error::new("SINGULAR_TRANSFORM","Cannot move a singular transform"));}
        let delta=matrix.inverse()*Point::new(dx,dy)-matrix.inverse()*Point::ORIGIN;
        Ok(Operation::ObjectUpdate{object_id:id.clone(),patch:ObjectPatch{x_mm:Some(o.x_mm+delta.x),y_mm:Some(o.y_mm+delta.y),..Default::default()}})
    }).collect()
}
pub(super) fn transform(doc:&Document,ids:&[String],action:Affine)->arranger_core::Result<Vec<Operation>> {
    roots(doc,ids).iter().map(|id|{
        let o=doc.object(id)?;doc.require_editable(id)?;let parent=parent_transform(doc,o)?;
        if parent.determinant().abs()<1e-12{return Err(arranger_core::Error::new("SINGULAR_TRANSFORM","Cannot transform a singular parent"));}
        Ok(Operation::ObjectUpdate{object_id:id.clone(),patch:ObjectPatch{transform:Some((parent.inverse()*action*parent*Affine::new(o.transform)).as_coeffs()),..Default::default()}})
    }).collect()
}

#[derive(Clone,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Clipboard { pub objects:Vec<Object>,pub resources:BTreeMap<String,Resource> }
impl Clipboard {
    pub fn copy(doc:&Document,ids:&[String])->arranger_core::Result<Self> {
        let mut included:BTreeSet<String>=roots(doc,ids).into_iter().collect();
        if included.is_empty(){return Err(arranger_core::Error::new("EMPTY_SELECTION","Select objects to copy"));}
        loop {
            let before=included.len();
            for o in &doc.objects {
                if o.parent_id.as_ref().is_some_and(|p|included.contains(p)){included.insert(o.id.clone());}
                if included.contains(&o.id) {if let Geometry::Text{text_path:Some(binding),..}=&o.geometry {included.insert(binding.path_id.clone());}}
            }
            if before==included.len(){break;}
        }
        let mut objects=vec![];let mut resources=BTreeMap::new();
        for o in &doc.objects {if included.contains(&o.id) {
            let mut copy=o.clone();
            if o.parent_id.as_ref().is_some_and(|p|!included.contains(p)) {
                let mut parent=o.parent_id.as_deref();
                while let Some(id)=parent {let p=doc.object(id)?;
                    if p.clip.is_some() || p.style.opacity!=1. {return Err(arranger_core::Error::new("GROUP_APPEARANCE","Copy the parent group to preserve its clip and opacity"));}
                    parent=p.parent_id.as_deref();
                }
                copy.transform=doc.world_transform(o)?.as_coeffs();copy.x_mm=0.;copy.y_mm=0.;copy.parent_id=None;
            }
            if let Geometry::Image{resource_id,..}=&o.geometry {resources.insert(resource_id.clone(),doc.resources[resource_id].clone());}
            objects.push(copy);
        }}
        Ok(Self{objects,resources})
    }
    pub fn paste(&self,doc:&Document,page:&str,next:&mut u64,offset:[f64;2])->arranger_core::Result<(Vec<Operation>,Vec<String>)> {
        // Validate the received data with the same model before allocating IDs.
        let mut source=Document::new("clipboard".into(),210.,297.)?;
        source.objects=self.objects.clone();source.resources=self.resources.clone();
        for o in &mut source.objects{o.page_id="page-1".into();}source.validate()?;
        let mut used:BTreeSet<String>=doc.objects.iter().map(|o|o.id.clone()).chain(doc.resources.keys().cloned()).collect();
        let mut fresh=|kind:&str|loop{*next+=1;let id=format!("{kind}-{}",next);if used.insert(id.clone()){break id;}};
        let mapping:BTreeMap<_,_>=self.objects.iter().map(|o|(o.id.clone(),fresh("object"))).collect();
        let mut resource_map=BTreeMap::new();let mut operations=vec![];
        for (id,r) in &self.resources {
            let target=if let Some((existing,_))=doc.resources.iter().find(|(_,v)|v.sha256==r.sha256&&v.mime==r.mime){existing.clone()}
                else {let target=fresh("image");operations.push(Operation::ResourcePut{resource_id:target.clone(),resource:r.clone()});target};
            resource_map.insert(id.clone(),target);
        }
        // A valid document's stacking vector can put children before their
        // parent. ObjectCreate checks the parent immediately, so create a
        // stable topological order while retaining each sibling's order.
        let mut ordered=vec![];let mut created=BTreeSet::new();
        while ordered.len()<self.objects.len(){
            for o in &self.objects {if !created.contains(&o.id)&&o.parent_id.as_ref().is_none_or(|p|created.contains(p))
                && !matches!(&o.geometry,Geometry::Text{text_path:Some(binding),..} if !created.contains(&binding.path_id)) {
                ordered.push(o);created.insert(o.id.clone());
            }}
        }
        let mut selected=vec![];
        for o in &ordered {
            let mut copy=(**o).clone();copy.id=mapping[&o.id].clone();copy.page_id=page.into();
            copy.locked=false;
            copy.parent_id=o.parent_id.as_ref().map(|p|mapping[p].clone());
            if copy.parent_id.is_none(){copy.transform=(Affine::translate((offset[0],offset[1]))*Affine::new(copy.transform)).as_coeffs();selected.push(copy.id.clone());}
            match &mut copy.geometry {
                Geometry::Image{resource_id,..}=>*resource_id=resource_map[resource_id].clone(),
                Geometry::Text{text_path:Some(binding),..}=>binding.path_id=mapping[&binding.path_id].clone(),_=>{}
            }
            operations.push(Operation::ObjectCreate{object:copy});
        }
        // Restore the original painting order after dependency-ordered
        // creation; text may have needed a guide that was behind it in Vec.
        let last=doc.objects.len()+self.objects.len()-1;
        for o in &self.objects {operations.push(Operation::ObjectReorder{object_id:mapping[&o.id].clone(),index:last});}
        // Lock descendants before ancestors, after all required reorders.
        for o in ordered.into_iter().rev().filter(|o|o.locked){operations.push(Operation::ObjectLock{object_id:mapping[&o.id].clone(),locked:true});}
        Ok((operations,selected))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn curves_selection_includes_nested_children_once_and_rejects_locked_ancestors(){
        let mut doc=Document::new("curves".into(),210.,297.).unwrap();
        doc.objects=vec![object("outer",None,Geometry::Group),object("inner",Some("outer"),Geometry::Group),
            object("label",Some("inner"),Geometry::Text{content:"A".into(),font_family:"Arial".into(),font_size_mm:4.,text_path:None,layout:Default::default()}),
            object("shape",Some("outer"),Geometry::Ellipse),object("curve",Some("outer"),Geometry::Path{d:"M0 0L1 1".into(),fill_rule:Default::default()}),object("other",None,Geometry::Ellipse)];
        let ops=curves_operations(&doc,&["outer".into(),"label".into(),"outer".into()]).unwrap();
        assert_eq!(serde_json::to_value(ops).unwrap(),json!([{ "type":"text_to_path","object_id":"label"},{"type":"object_to_path","object_id":"shape"}]));
        assert!(curves_operations(&doc,&["curve".into()]).unwrap().is_empty());
        doc.objects[0].locked=true;assert_eq!(curves_operations(&doc,&["label".into()]).unwrap_err().code,"OBJECT_LOCKED");
    }
    fn object(id:&str,parent:Option<&str>,geometry:Geometry)->Object {
        Object{id:id.into(),page_id:"page-1".into(),parent_id:parent.map(Into::into),x_mm:2.,y_mm:3.,width_mm:20.,height_mm:10.,transform:Affine::IDENTITY.as_coeffs(),
            style:Style::default(),visible:true,locked:false,clip:None,geometry}
    }
    fn png(red:u8)->Resource {
        let mut p=arranger_core::render::Pixmap::new(2,2).unwrap();p.data_mut().fill(red);
        Resource::png(&p.encode_png().unwrap()).unwrap()
    }
    #[test]
    fn editing_an_ordinary_style_field_preserves_each_selected_fixed_nib(){
        let mut old=Style::default();old.stroke="#000000".into();old.stroke_width_mm=1.;old.stroke_scale_with_object=false;old.stroke_fixed_nib=Some(arranger_core::FixedStrokeNib{stretch_percent:50,angle_millionth_degrees:90_000_000});
        let mut other=old.clone();other.stroke_fixed_nib=Some(arranger_core::FixedStrokeNib{stretch_percent:70,angle_millionth_degrees:30_000_000});let mut edited=old.clone();edited.stroke_width_mm=2.;
        let delta=style_delta(&old,&edited,&other,true);assert_eq!(delta.stroke_width_mm,2.);assert_eq!(delta.stroke_fixed_nib,other.stroke_fixed_nib);assert!(!delta.stroke_scale_with_object);
        edited.stroke_fixed_nib=None;assert_eq!(style_delta(&old,&edited,&other,true).stroke_fixed_nib,None);
    }
    #[test]
    fn concise_fields_preserve_source_precision_and_accept_explicit_precision(){
        let mut doc=Document::new("numbers".into(),60.,14.).unwrap();
        let mut o=object("shape",None,Geometry::Rect{radius_mm:0.});
        o.x_mm=0.0123456789012345;o.width_mm=60.599999999999994;
        o.clip=Some(arranger_core::Clip::Rect{rect_mm:[0.,0.,60.,10.]});
        doc.objects.push(o);doc.validate().unwrap();
        let hash=doc.hash();let mut editor=Editor::new(&doc,&doc.objects[0],9);
        assert_eq!(editor.width,"60.6");assert_eq!(editor.x,"0.012");
        assert_eq!(editor.numbers().unwrap()[0].to_bits(),doc.objects[0].x_mm.to_bits());
        assert_eq!(editor.numbers().unwrap()[2].to_bits(),doc.objects[0].width_mm.to_bits());
        editor.style.fill="#123456".into();
        assert_eq!(editor.numbers().unwrap(),editor.original_numbers);
        editor.x="0.012345678901235".into();
        assert_eq!(editor.numbers().unwrap()[0],0.012345678901235);
        assert_eq!(doc.hash(),hash);
        editor.width="NaN".into();assert!(editor.numbers().is_err());
    }
    #[test]
    fn display_keeps_real_fractional_widths_and_tiny_nonzero_values(){
        for (value,shown) in [(60.,"60"),(60.599999999999994,"60.6"),(19.99999809265,"20"),(-0.,"0"),(-1.23456,"-1.235")]{
            assert_eq!(display_number(value),shown);
        }
        for value in [1e-12,-1e-12]{assert_ne!(display_number(value),"0");assert_eq!(parse_displayed_number(&display_number(value),value).unwrap(),value);}
        assert_eq!(parse_displayed_number("60.612345678901",60.6).unwrap(),60.612345678901);
        assert!(parse_displayed_number("inf",60.).is_err());
    }
    #[test]
    fn clipboard_preserves_group_order_locks_and_shared_image_resources(){
        let mut doc=Document::new("source".into(),210.,297.).unwrap();
        let resource=png(255);doc.resources.insert("shared".into(),resource.clone());
        let child=object("child",Some("group"),Geometry::Image{resource_id:"shared".into(),clip_mm:None,fit:Default::default()});
        let mut group=object("group",None,Geometry::Group);group.x_mm=12.;group.y_mm=20.;group.locked=true;
        doc.objects=vec![child,group];doc.validate().unwrap();
        let clipboard=Clipboard::copy(&doc,&["group".into()]).unwrap();
        let service=Service::new();service.command(Command::DocumentNew{document_id:"dest".into(),width_mm:210.,height_mm:297.,unit:"mm".into()}).unwrap();
        service.command(Command::Batch{document_id:"dest".into(),expected_revision:0,unit:"mm".into(),operations:vec![Operation::ResourcePut{resource_id:"shared".into(),resource:png(0)}]}).unwrap();
        let before=service.snapshot("dest").unwrap();let (ops,ids)=clipboard.paste(&before.document,"page-1",&mut 0,[5.,7.]).unwrap();
        service.command(Command::Batch{document_id:"dest".into(),expected_revision:before.revision,unit:"mm".into(),operations:ops}).unwrap();
        let after=service.snapshot("dest").unwrap();let pasted=&after.document;
        assert_eq!(pasted.objects.len(),2);assert_eq!(pasted.resources.len(),2);assert_eq!(ids.len(),1);
        assert!(matches!(pasted.objects[0].geometry,Geometry::Image{..}));assert!(pasted.objects[1].locked);
        let a=doc.bounds(&doc.objects[0]).unwrap();let b=pasted.bounds(&pasted.objects[0]).unwrap();
        assert!((b[0]-a[0]-5.).abs()<1e-10&&(b[1]-a[1]-7.).abs()<1e-10);
        assert!(pasted.resources.values().any(|r|r.sha256==resource.sha256));
        service.command(Command::Undo{document_id:"dest".into(),expected_revision:after.revision}).unwrap();
        assert_eq!(service.snapshot("dest").unwrap().document_hash,before.document_hash);
    }
    #[test]
    fn copying_bound_text_also_copies_guide_and_remaps_its_dependency(){
        let mut doc=Document::new("source".into(),210.,297.).unwrap();
        let mut guide=object("guide",None,Geometry::Path{d:"M0 0 L100 0".into(),fill_rule:Default::default()});guide.width_mm=100.;guide.height_mm=0.;
        let text=object("text",None,Geometry::Text{content:"Copy".into(),font_family:"sans-serif".into(),font_size_mm:2.,text_path:Some(arranger_core::TextOnPath{path_id:"guide".into(),start_offset_mm:0.,baseline_offset_mm:0.,anchor:Default::default()}),layout:Default::default()});
        doc.objects=vec![text,guide];doc.validate().unwrap();let clip=Clipboard::copy(&doc,&["text".into()]).unwrap();assert_eq!(clip.objects.len(),2);
        let service=Service::new();service.command(Command::DocumentNew{document_id:"dest".into(),width_mm:210.,height_mm:297.,unit:"mm".into()}).unwrap();
        let empty=service.snapshot("dest").unwrap();let (ops,_)=clip.paste(&empty.document,"page-1",&mut 0,[0.,0.]).unwrap();
        service.command(Command::Batch{document_id:"dest".into(),expected_revision:0,unit:"mm".into(),operations:ops}).unwrap();
        let after=service.snapshot("dest").unwrap();assert!(matches!(after.document.objects[0].geometry,Geometry::Text{..}));
        if let Geometry::Text{text_path:Some(binding),..}=&after.document.objects[0].geometry{assert_eq!(binding.path_id,after.document.objects[1].id);assert_ne!(binding.path_id,"guide");}else{panic!("editable text binding lost");}
    }
    #[test]
    fn translation_of_affine_child_uses_world_millimetres(){
        let mut doc=Document::new("affine".into(),210.,297.).unwrap();
        let mut group=object("group",None,Geometry::Group);group.transform=[2.,0.5,0.3,1.5,9.,7.];
        let mut child=object("child",Some("group"),Geometry::Ellipse);child.transform=[1.,0.2,0.1,1.,0.,0.];doc.objects=vec![group,child];doc.validate().unwrap();
        let before=doc.bounds(&doc.objects[1]).unwrap();let ops=translate(&doc,&["child".into()],8.,-4.).unwrap();
        if let Operation::ObjectUpdate{patch,..}=&ops[0]{doc.objects[1].x_mm=patch.x_mm.unwrap();doc.objects[1].y_mm=patch.y_mm.unwrap();}
        let after=doc.bounds(&doc.objects[1]).unwrap();assert!((after[0]-before[0]-8.).abs()<1e-10&&(after[1]-before[1]+4.).abs()<1e-10);assert_eq!(&before[2..],&after[2..]);
    }
    #[test]
    fn malformed_clipboard_is_rejected_without_modifying_destination(){
        let doc=Document::new("dest".into(),210.,297.).unwrap();let original=doc.hash();
        let o=object("same",None,Geometry::Ellipse);let clip=Clipboard{objects:vec![o.clone(),o],resources:BTreeMap::new()};
        assert!(clip.paste(&doc,"page-1",&mut 0,[0.,0.]).is_err());assert_eq!(doc.hash(),original);
    }
}

impl App {
    pub(super) fn set_selection(&mut self,ids:Vec<String>) {self.selection_bounds.clear();self.command_focus=None;self.selected=ids.first().cloned();self.selection=ids;self.selection_mode=Default::default();self.rotation_center=None;self.editor=None;self.inline_text=None;self.text_field=None;self.font_feedback=None;self.node_focus=None;}
    pub(super) fn select_hit(&mut self,doc:&Document,id:Option<String>,add:bool) {
        let id=id.map(|id|top_object(doc,id));
        if add {if let Some(id)=id {let mut ids=self.selection.clone();if ids.contains(&id){ids.retain(|v|v!=&id);}else{ids.push(id);}self.set_selection(ids);}}
        else{self.set_selection(id.into_iter().collect());}
    }
    pub(super) fn unique(&mut self,doc:&Document,kind:&str)->String {loop{self.next_object+=1;let id=format!("{kind}-{}",self.next_object);if doc.object(&id).is_err()&&doc.page(&id).is_err()&&!doc.resources.contains_key(&id){return id;}}}
    pub(super) fn object(&mut self,doc:&Document,geometry:Geometry,x:f64,y:f64,w:f64,h:f64)->Object {
        Object{id:self.unique(doc,"object"),page_id:self.page.clone(),parent_id:None,x_mm:x,y_mm:y,width_mm:w,height_mm:h,
            transform:Affine::IDENTITY.as_coeffs(),style:self.default_style.clone(),visible:true,locked:false,clip:None,geometry}
    }
    pub(super) fn operations(&mut self,snapshot:&Snapshot,result:arranger_core::Result<Vec<Operation>>) {
        match result {Ok(ops)=>{if !ops.is_empty(){self.batch(snapshot.revision,ops);}},Err(error)=>{self.error=true;self.message=error.to_string();}}
    }
    pub(super) fn apply_editor(&mut self,snapshot:&Snapshot) {
        if !self.text_commit_ready(){return;}
        let Some(editor)=self.editor.clone() else{return;};
        let revision=editor.revision;
        let result=(||{
            let [x,y,w,h,angle]=editor.numbers()?;
            if w<0. || h<0. {return Err(arranger_core::Error::new("INVALID_NUMBER","Width and height cannot be negative"));}
            let doc=&snapshot.document;let o=doc.object(&editor.id)?;let [bx,by,bw,bh]=doc.bounds(o)?;
            let mut ops=vec![];let new_style=editor.style.clone();let mut patch=ObjectPatch{style:Some(editor.style),geometry:Some(editor.geometry),..Default::default()};
            let [a,b,..]=doc.world_transform(o)?.as_coeffs();let old_angle=b.atan2(a).to_degrees();
            if w!=bw || h!=bh || angle!=old_angle {
                let sx=if bw>0. {w/bw}else{1.};let sy=if bh>0.{h/bh}else{1.};
                if sx<=0.||sy<=0. {return Err(arranger_core::Error::new("INVALID_NUMBER","Resized dimensions must be positive"));}
                let center=Point::new(bx+bw/2.,by+bh/2.);
                let action=Affine::translate(center.to_vec2())*Affine::rotate((angle-old_angle).to_radians())*Affine::scale_non_uniform(sx,sy)*Affine::translate(-center.to_vec2());
                let parent=parent_transform(doc,o)?;if parent.determinant().abs()<1e-12{return Err(arranger_core::Error::new("SINGULAR_TRANSFORM","Cannot edit a singular parent"));}
                patch.transform=Some((parent.inverse()*action*parent*Affine::new(o.transform)).as_coeffs());
                // Position fields refer to the resulting source geometry bounds.
                let mut preview=(**doc).clone();let target=preview.objects.iter_mut().find(|v|v.id==o.id).unwrap();target.transform=patch.transform.unwrap();
                let [px,py,..]=preview.bounds(preview.object(&o.id)?)?;
                let matrix=parent*Affine::new(patch.transform.unwrap());let delta=matrix.inverse()*Point::new(x-px,y-py)-matrix.inverse()*Point::ORIGIN;
                patch.x_mm=Some(o.x_mm+delta.x);patch.y_mm=Some(o.y_mm+delta.y);
            } else if x!=bx || y!=by {ops=translate(doc,std::slice::from_ref(&o.id),x-bx,y-by)?;
                if let Some(Operation::ObjectUpdate{patch:moved,..})=ops.pop(){patch.x_mm=moved.x_mm;patch.y_mm=moved.y_mm;}
            }
            let style_changed=o.style!=new_style;
            ops.push(Operation::ObjectUpdate{object_id:editor.id.clone(),patch});
            if style_changed {
                let selected=roots(doc,&self.selection);
                for target in &doc.objects {
                    if target.id==editor.id{continue;}
                    let mut ancestor=target.parent_id.as_deref();let mut in_selected_group=false;
                    while let Some(id)=ancestor {if selected.iter().any(|s|s==id){in_selected_group=true;break;}ancestor=doc.object(id).ok().and_then(|p|p.parent_id.as_deref());}
                    if selected.contains(&target.id)||in_selected_group {
                        let style=style_delta(&o.style,&new_style,&target.style,!in_selected_group);
                        if style!=target.style{ops.push(Operation::ObjectUpdate{object_id:target.id.clone(),patch:ObjectPatch{style:Some(style),..Default::default()}});}
                    }
                }
            }
            Ok(ops)
        })();
        match result {Ok(ops)=>{self.batch(revision,ops);},Err(error)=>{self.error=true;self.message=if error.code=="INVALID_NUMBER"{"Enter numeric millimetre values".into()}else{error.to_string()};}}
    }
    pub(super) fn edit_action(&mut self,ctx:&egui::Context,snapshot:&Snapshot,action:&str) {
        if self.text_command(ctx,action){return;}
        // Page commands can be dispatched from the native menu without an
        // egui pointer boundary. Confirm text before changing the page model,
        // and keep unfinished IME/numeric/page/path drafts on their source.
        let navigation=if action=="page_add"||action=="page_delete"&&snapshot.document.pages.len()>1{
            let Some(current)=self.prepare_navigation(snapshot)else{return;};Some(current)
        }else{None};
        let snapshot=navigation.as_ref().unwrap_or(snapshot);
        if action=="to_curves" {
            if snapshot.busy {self.error=true;self.message=tr(self.chinese,"外部任务进行中，请稍后操作","External transaction is active").into();return;}
            if !self.finish_text(ctx,snapshot){return;}
            let Ok(mut current)=self.service.snapshot(&self.active) else{return;};
            if self.editor.as_ref().is_some_and(|e|e.dirty){self.apply_editor(&current);if self.error{return;}let Ok(updated)=self.service.snapshot(&self.active)else{return;};current=updated;}
            match curves_operations(&current.document,&self.selection) {
                Ok(ops) if ops.is_empty()=>{self.error=false;self.message=tr(self.chinese,"所选内容已是曲线，或没有可转曲的文字和形状","No selected text or shapes need conversion").into();},
                Ok(ops)=>{if self.batch(current.revision,ops){self.editor=None;self.node_focus=None;self.message=tr(self.chinese,"已转为曲线；撤销可恢复文字和形状","Converted to curves; Undo restores text and shapes").into();}},
                Err(error)=>{self.error=true;self.message=error.to_string();},
            }
            return;
        }
        let doc=&snapshot.document;let ids=roots(doc,&self.selection);
        if snapshot.busy {self.message=tr(self.chinese,"外部任务进行中，请稍后操作","External transaction is active").into();return;}
        match action {
            "copy"|"cut"=>match Clipboard::copy(doc,&ids) {Ok(data)=>{
                if action=="cut"&&!self.batch(snapshot.revision,ids.iter().map(|id|Operation::ObjectDelete{object_id:id.clone()}).collect()){return;}
                ctx.copy_text(format!("{CLIPBOARD_PREFIX}{}",serde_json::to_string(&data).expect("clipboard serializes")));
                self.message=tr(self.chinese,"已复制对象","Objects copied").into();self.error=false;
            },Err(e)=>{self.error=true;self.message=e.to_string();}},
            "paste"=>{#[cfg(feature="test-hooks")] {self.test_paste_requested=true;}ctx.memory_mut(|m|m.stop_text_input());ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);},
            "duplicate"=>match Clipboard::copy(doc,&ids).and_then(|data|data.paste(doc,&self.page,&mut self.next_object,[5.,5.])) {
                Ok((ops,selected))=>{if self.batch(snapshot.revision,ops){self.set_selection(selected);}},Err(e)=>{self.error=true;self.message=e.to_string();}},
            "delete"=>{self.operations(snapshot,Ok(ids.iter().map(|id|Operation::ObjectDelete{object_id:id.clone()}).collect()));},
            "group"=>{let result=(||{
                if ids.len()<2{return Err(arranger_core::Error::new("SELECTION","Select at least two objects"));}
                let parent=doc.object(&ids[0])?.parent_id.clone();
                if ids.iter().any(|id|doc.object(id).is_ok_and(|o|o.parent_id!=parent)){return Err(arranger_core::Error::new("SELECTION","Group objects with the same parent"));}
                for id in &ids{doc.require_editable(id)?;}
                let mut group=self.object(doc,Geometry::Group,0.,0.,0.,0.);group.parent_id=parent;
                group.style=Style{fill:"none".into(),..Default::default()};let id=group.id.clone();
                let index=doc.objects.iter().rposition(|o|ids.contains(&o.id)).unwrap();
                let mut ops=vec![Operation::ObjectCreate{object:group}];
                ops.extend(ids.iter().map(|object_id|Operation::ObjectReparent{object_id:object_id.clone(),parent_id:Some(id.clone())}));
                ops.push(Operation::ObjectReorder{object_id:id.clone(),index});Ok((ops,id))
            })();match result {Ok((ops,id))=>{if self.batch(snapshot.revision,ops){self.set_selection(vec![id]);}},Err(e)=>{self.error=true;self.message=e.to_string();}}},
            "ungroup"=>{let mut ops=vec![];let mut children=vec![];let mut groups=BTreeSet::new();let mut unsupported=false;
                for id in &ids {if let Ok(group)=doc.object(id){if matches!(group.geometry,Geometry::Group){
                    if group.clip.is_some() || group.style.opacity!=1. {unsupported=true;break;}
                    groups.insert(id.clone());
                    let local=Affine::new(group.transform)*Affine::translate((group.x_mm,group.y_mm));
                    for child in doc.objects.iter().filter(|o|o.parent_id.as_deref()==Some(id)) {
                        children.push(child.id.clone());
                        ops.push(Operation::ObjectUpdate{object_id:child.id.clone(),patch:ObjectPatch{transform:Some((local*Affine::new(child.transform)).as_coeffs()),..Default::default()}});
                        ops.push(Operation::ObjectReparent{object_id:child.id.clone(),parent_id:group.parent_id.clone()});
                    }
                    ops.push(Operation::ObjectDelete{object_id:id.clone()});
                }}}
                if unsupported {self.error=true;self.message=tr(self.chinese,"带裁剪或整体透明度的群组暂不直接解组，以免改变外观","Ungroup would change the group clip or opacity").into();}
                else if !ops.is_empty(){
                    // Replace each group at its stacking slot. Only its own
                    // children are reordered, so locked unrelated objects do
                    // not need to be edited to preserve the visual order.
                    let child_set:BTreeSet<_>=children.iter().cloned().collect();
                    let target:Vec<_>=doc.objects.iter().filter(|o|!child_set.contains(&o.id)).flat_map(|o|{
                        if groups.contains(&o.id){doc.objects.iter().filter(|c|c.parent_id.as_deref()==Some(&o.id)).map(|c|c.id.clone()).collect()}
                        else{vec![o.id.clone()]}
                    }).collect();
                    let mut current:Vec<_>=doc.objects.iter().filter(|o|!groups.contains(&o.id)).map(|o|o.id.clone()).collect();let mut anchor=None;
                    for id in target.iter().rev(){
                        if child_set.contains(id){let old=current.iter().position(|v|v==id).unwrap();current.remove(old);
                            let index=anchor.as_ref().and_then(|next|current.iter().position(|v|v==next)).unwrap_or(current.len());current.insert(index,id.clone());
                            ops.push(Operation::ObjectReorder{object_id:id.clone(),index});
                        }
                        anchor=Some(id.clone());
                    }
                    if self.batch(snapshot.revision,ops){self.set_selection(children);}
                }
            },
            "front"|"back"=>{let ordered=if action=="front"{ids.clone()}else{ids.iter().rev().cloned().collect()};
                self.operations(snapshot,Ok(ordered.into_iter().map(|id|Operation::ObjectReorder{object_id:id,index:if action=="front"{doc.objects.len()-1}else{0}}).collect()));},
            "lock"|"unlock"=>self.operations(snapshot,Ok(ids.iter().map(|id|Operation::ObjectLock{object_id:id.clone(),locked:action=="lock"}).collect())),
            "hide"|"show"=>self.operations(snapshot,Ok(ids.iter().map(|id|Operation::ObjectUpdate{object_id:id.clone(),patch:ObjectPatch{visible:Some(action=="show"),..Default::default()}}).collect())),
            "all"=>self.set_selection(doc.objects.iter().filter(|o|o.on_canvas(&self.page)&&o.parent_id.is_none()&&o.visible).map(|o|o.id.clone()).collect()),
            "union"|"intersection"|"difference"|"xor" if ids.len()==2=>{let result_id=self.unique(doc,"boolean");
                let operation=match action{"union"=>arranger_core::BooleanOp::Union,"intersection"=>arranger_core::BooleanOp::Intersection,"difference"=>arranger_core::BooleanOp::Difference,_=>arranger_core::BooleanOp::Xor};
                if self.batch(snapshot.revision,vec![Operation::PathBoolean{left_id:ids[0].clone(),right_id:ids[1].clone(),result_id:result_id.clone(),operation,remove_inputs:true}]){self.set_selection(vec![result_id]);}},
            "to_path"=>self.operations(snapshot,Ok(ids.iter().map(|id|Operation::ObjectToPath{object_id:id.clone()}).collect())),
            "undo"|"redo"=>{self.act(if action=="undo"{Command::Undo{document_id:self.active.clone(),expected_revision:snapshot.revision}}else{Command::Redo{document_id:self.active.clone(),expected_revision:snapshot.revision}},if action=="undo"{"Undone"}else{"Redone"});self.editor=None;},
            "page_add"=>{let page=self.unique(doc,"page");if self.batch(snapshot.revision,vec![Operation::PageAdd{page:Page{id:page.clone(),width_mm:210.,height_mm:297.}}]){self.switch_page(page,210.,297.);ctx.memory_mut(|m|m.stop_text_input());}},
            "page_delete" if doc.pages.len()>1=>{
                let mut ops:Vec<_>=doc.objects.iter().filter(|o|o.page_id==self.page&&matches!(o.geometry,Geometry::Text{text_path:Some(_),..})).map(|o|Operation::TextSetPath{object_id:o.id.clone(),text_path:None}).collect();
                ops.extend(doc.objects.iter().filter(|o|o.page_id==self.page&&o.parent_id.is_none()).map(|o|Operation::ObjectDelete{object_id:o.id.clone()}));
                ops.push(Operation::PageDelete{page_id:self.page.clone()});
                if self.batch(snapshot.revision,ops){let next=doc.pages.iter().find(|p|p.id!=self.page).unwrap();self.switch_page(next.id.clone(),next.width_mm,next.height_mm);ctx.memory_mut(|m|m.stop_text_input());}
            },
            _=>{}
        }
    }
    pub(super) fn paste_text(&mut self,snapshot:&Snapshot,text:&str) {
        if snapshot.busy{return;}
        if let Some(json)=text.strip_prefix(CLIPBOARD_PREFIX) {
            let result=serde_json::from_str::<Clipboard>(json).map_err(|e|e.to_string()).and_then(|data|data.paste(&snapshot.document,&self.page,&mut self.next_object,[5.,5.]).map_err(|e|e.to_string()));
            match result {Ok((ops,ids))=>{if self.batch(snapshot.revision,ops){self.set_selection(ids);}},Err(e)=>{self.error=true;self.message=e;}}
        } else if !text.trim().is_empty() {
            let object=self.object(&snapshot.document,Geometry::Text{content:text.into(),font_family:self.ui_font.clone(),font_size_mm:5.,text_path:None,layout:TextLayoutOptions::default()},20.,20.,120.,20.);
            let id=object.id.clone();if self.batch(snapshot.revision,vec![Operation::ObjectCreate{object}]){self.set_selection(vec![id]);}
        }
    }
}
