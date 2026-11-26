//! Editing actions exposed by the contextual workstation chrome.
use super::*;
use arranger_core::{Document, Result, Error};
use kurbo::{BezPath, PathEl, PathSeg, ParamCurve};

pub(super) fn node_path(path:&BezPath,index:usize,action:&str)->Result<BezPath>{
    if index>=path.elements().len(){return Err(Error::new("INVALID_INDEX","Select a path node"));}
    let mut normalized=path.clone();
    if matches!(action,"smooth"|"symmetric"|"line"|"curve"){
        let end=path.elements()[index+1..].iter().position(|e|matches!(e,PathEl::MoveTo(_))).map_or(path.elements().len(),|n|index+1+n);
        if let Some((i,PathSeg::Line(l)))=nodes::segments(path).into_iter().find(|(i,_)|*i==end-1&&matches!(path.elements()[*i],PathEl::ClosePath)){
            let mut els=path.elements().to_vec();els.insert(i,PathEl::LineTo(l.p1));normalized=BezPath::from_vec(els);
        }
    }
    let path=&normalized;
    let segment=nodes::segments(path).into_iter().find(|(i,_)|*i>index).or_else(||nodes::segments(path).into_iter().find(|(i,_)|*i==index));
    match action{
        "add"=>{let(i,_)=segment.ok_or_else(||Error::new("INVALID_INDEX","Select a segment"))?;nodes::inserted(path,i,0.5)},
        "delete"=>nodes::deleted(path,index),
        "reverse"=>Ok(path.reverse_subpaths()),
        "open"=>{
            let mut out=path.elements().to_vec();let start=out[..=index].iter().rposition(|p|matches!(p,PathEl::MoveTo(_))).unwrap_or(0);let end=out[index+1..].iter().position(|p|matches!(p,PathEl::MoveTo(_))).map_or(out.len(),|n|index+1+n);
            if matches!(out[end-1],PathEl::ClosePath){
                let p=*nodes::points(out[index]).last().ok_or_else(||Error::new("INVALID_INDEX","Select an anchor"))?;
                let edges=nodes::segments(path).into_iter().filter(|(i,_)|*i>start&&*i<end).collect::<Vec<_>>();let rotate=edges.iter().position(|(i,_)|*i>index).unwrap_or(0);let mut part=vec![PathEl::MoveTo(p)];
                for(_,edge)in edges[rotate..].iter().chain(&edges[..rotate]){part.push(edge.as_path_el());}
                out.splice(start..end,part);
            }else{if index==start||index+1==end{return Err(Error::new("INVALID_INDEX","Select an interior anchor to split"));}let p=*nodes::points(out[index]).last().ok_or_else(||Error::new("INVALID_INDEX","Select an anchor"))?;out.insert(index+1,PathEl::MoveTo(p));}
            Ok(BezPath::from_vec(out))
        },
        "close"=>{let mut out=path.elements().to_vec();let end=out[index+1..].iter().position(|p|matches!(p,PathEl::MoveTo(_))).map_or(out.len(),|n|index+1+n);if !matches!(out[end-1],PathEl::ClosePath){out.insert(end,PathEl::ClosePath);}Ok(BezPath::from_vec(out))},
        "line"|"curve"=>{let(i,seg)=segment.ok_or_else(||Error::new("INVALID_INDEX","Select a segment"))?;let mut out=path.elements().to_vec();out[i]=if action=="line"{PathEl::LineTo(seg.end())}else{match seg{PathSeg::Line(l)=>PathEl::CurveTo(l.p0+(l.p1-l.p0)/3.,l.p0+(l.p1-l.p0)*(2./3.),l.p1),PathSeg::Quad(q)=>{let c=q.raise();PathEl::CurveTo(c.p1,c.p2,c.p3)},PathSeg::Cubic(c)=>PathEl::CurveTo(c.p1,c.p2,c.p3)}};Ok(BezPath::from_vec(out))},
        "cusp"=>Ok(path.clone()),
        "smooth"|"symmetric"=>{
            let mut out=path.clone();let mut arms=nodes::control_arms(&out,index);
            // Straight neighbours gain collinear cubic handles, with no change
            // to the segment's shape, before choosing a tangent direction.
            if arms.len()<2{let mut els=out.elements().to_vec();for(i,seg)in nodes::segments(&out){if let PathSeg::Line(l)=seg{if (l.p0-*nodes::points(path.elements()[index]).last().unwrap()).hypot()<1e-9||(l.p1-*nodes::points(path.elements()[index]).last().unwrap()).hypot()<1e-9{if !matches!(els[i],PathEl::ClosePath){els[i]=PathEl::CurveTo(l.p0+(l.p1-l.p0)/3.,l.p0+(l.p1-l.p0)*(2./3.),l.p1);}}}}out=BezPath::from_vec(els);arms=nodes::control_arms(&out,index);}
            if arms.len()!=2{return Err(Error::new("INVALID_NODE","This node needs two adjacent curve handles"));}
            let a=&arms[0];let b=&arms[1];let va=a.control-a.anchor;let vb=b.control-b.anchor;let tangent=va-vb;let length=tangent.hypot();if length<1e-12{return Ok(out);}
            let direction=tangent/length;let la=if action=="symmetric"{(va.hypot()+vb.hypot())/2.}else{va.hypot()};let lb=if action=="symmetric"{la}else{vb.hypot()};
            out=nodes::moved(&out,a.index,a.point,a.anchor+direction*la)?;nodes::moved(&out,b.index,b.point,b.anchor-direction*lb)
        },_=>Err(Error::new("INVALID_PARAMETER","Unknown node action"))
    }
}
/// Page-space alignment is lowered through each object's parent transform.
/// One batch for all roots; a locked member rejects before any mutation.
pub(super) fn arrange(doc:&Document,ids:&[String],action:&str)->Result<Vec<Operation>>{
    let roots=editing::roots(doc,ids);if roots.len()<2{return Ok(vec![]);}
    let mut items=roots.iter().map(|id|{doc.require_editable(id)?;Ok((id.clone(),doc.bounds(doc.object(id)?)?))}).collect::<Result<Vec<_>>>()?;
    let horizontal=action.ends_with('x');let axis=if horizontal{0}else{1};
    let lo=items.iter().map(|(_,b)|b[axis]).fold(f64::INFINITY,f64::min);let hi=items.iter().map(|(_,b)|b[axis]+b[axis+2]).fold(f64::NEG_INFINITY,f64::max);
    if action.starts_with("distribute")&&items.len()<3{return Ok(vec![]);}
    if action.starts_with("distribute"){items.sort_by(|a,b|(a.1[axis]+a.1[axis+2]/2.).total_cmp(&(b.1[axis]+b.1[axis+2]/2.)));}
    let first=items.first().unwrap().1;let last=items.last().unwrap().1;let center0=first[axis]+first[axis+2]/2.;let center1=last[axis]+last[axis+2]/2.;let n=items.len();let mut ops=vec![];
    for(i,(id,b))in items.into_iter().enumerate(){let delta=if action.starts_with("distribute"){center0+(center1-center0)*i as f64/(n-1)as f64-b[axis]-b[axis+2]/2.}else if action.starts_with("start"){lo-b[axis]}else if action.starts_with("end"){hi-b[axis]-b[axis+2]}else{(lo+hi-b[axis+2])/2.-b[axis]};if delta.abs()>1e-12{ops.extend(editing::translate(doc,&[id],if horizontal{delta}else{0.},if horizontal{0.}else{delta})?);}}
    Ok(ops)
}
impl App{
    pub(super) fn set_node_focus(&mut self,index:usize,hash:String){if self.node_link_target.as_ref().is_none_or(|(id,i)|Some(id)!=self.selected.as_ref()||*i!=index){self.node_kind="cusp".into();self.node_link_target=None;}self.node_focus=Some((index,hash));}
    pub(super) fn node_target(&self,s:&Snapshot)->Option<(String,BezPath,usize)>{let id=self.selected.as_ref()?;let o=s.document.object(id).ok()?;if !nodes::supported(o){return None;}let path=o.local_path().ok()?;let index=self.node_focus.as_ref().filter(|(_,hash)|hash==&s.document_hash).map_or(0,|(i,_)|*i);Some((id.clone(),path,index))}
    pub(super) fn node_action(&mut self,s:&Snapshot,action:&str){if let Some((id,path,index))=self.node_target(s){if matches!(action,"cusp"|"smooth"|"symmetric"){self.node_kind=action.into();self.node_link_target=Some((id.clone(),index));}let result=node_path(&path,index,action).and_then(|p|nodes::operations(&s.document,&id,&p));if result.as_ref().is_ok_and(Vec::is_empty){self.set_node_focus(index,s.document_hash.clone());return;}self.operations(s,result);if let Ok(current)=self.service.snapshot(&self.active){self.set_node_focus(if action=="delete"{0}else{index},current.document_hash);}}}
    pub(super) fn virtual_objects(&mut self,ui:&mut egui::Ui,s:&Snapshot,w:&mut BTreeMap<String,Value>,compact:bool){
        let source=Arc::downgrade(&s.document);
        let rows=&mut self.object_rows;
        if !rows.0.ptr_eq(&source)||rows.1!=self.page {
            rows.0=source;rows.1.clone_from(&self.page);
            rows.2=s.document.objects.iter().enumerate().rev().filter_map(|(i,o)|o.on_canvas(&self.page).then_some(i)).collect();
        }
        let objects=rows.2.clone();
        let height=ui.spacing().interact_size.y.max(ui.text_style_height(&egui::TextStyle::Body)+2.*ui.spacing().button_padding.y).max(17.);
        let output=egui::ScrollArea::vertical().id_salt(if compact{"inline-object-tree"}else{"large-object-list"}).auto_shrink([false,false]).show_rows(ui,height,objects.len(),|ui,range|{
            #[cfg(feature="test-hooks")] {self.performance["object_rows"]=json!({"first":range.start,"end":range.end,"total":objects.len(),"stride":height+ui.spacing().item_spacing.y});}
            for &index in &objects[range]{let o=&s.document.objects[index];
                let mut depth=0;let mut parent=o.parent_id.as_deref();while let Some(id)=parent{depth+=1;parent=s.document.object(id).ok().and_then(|o|o.parent_id.as_deref());}
                let chosen=ui.push_id(&o.id,|ui|{ui.style_mut().wrap_mode=Some(egui::TextWrapMode::Truncate);ui.horizontal(|ui|{
                    ui.add_space((depth as f32*10.).min(ui.available_width()/2.));
                    if compact{let image=match o.geometry{Geometry::Text{..}=>"type",Geometry::Ellipse=>"circle",Geometry::Image{..}=>"image",_=>"square"};let(r,_)=ui.allocate_exact_size(Vec2::splat(17.),egui::Sense::hover());shell::icon(ui,image,r,INK);}
                    let label=if compact{match &o.geometry{Geometry::Text{content,..}=>content.chars().take(14).collect::<String>(),Geometry::Ellipse=>format!("{} · {}",tr(self.chinese,"圆孔","Circle"),o.id),_=>o.id.clone()}}else{format!("{}{}{} {}",if o.page_id==arranger_core::WORKSPACE_PAGE{tr(self.chinese,"共享 · ","Shared · ")}else{""},if o.locked{"▣ "}else{""},if o.visible{""}else{"· "},o.id)};
                    let r=ui.selectable_label(self.selection.contains(&o.id),label).on_hover_text(&o.id);record_widget!(w,format!("object-{}",o.id),rect(r.rect));r.clicked()
                }).inner}).inner;
                if chosen&&self.prepare_navigation(s).is_some(){let mut ids=if ui.input(|i|i.modifiers.shift||i.modifiers.command){self.selection.clone()}else{vec![]};if ids.contains(&o.id){ids.retain(|id|id!=&o.id);}else{ids.push(o.id.clone());}self.set_selection(ids);}
            }
        });
        #[cfg(feature="test-hooks")] {self.performance["object_rows"]["offset"]=json!(output.state.clamped_offset().y);self.performance["object_rows"]["rect"]=rect(output.inner_rect);}
        #[cfg(not(feature="test-hooks"))]let _=output;
    }

    pub(super) fn inline_objects(&mut self,ui:&mut egui::Ui,s:&Snapshot,w:&mut BTreeMap<String,Value>){
        ui.add_space(8.);ui.separator();ui.label(egui::RichText::new(tr(self.chinese,"对象","Objects")).strong());
        if s.document.objects.len()>200{self.virtual_objects(ui,s,w,true);return;}
        let page=self.page.clone();
        for o in s.document.objects.iter().rev().filter(|o|o.on_canvas(&page)){
            let mut depth=0;let mut parent=o.parent_id.as_deref();while let Some(id)=parent{depth+=1;parent=s.document.object(id).ok().and_then(|o|o.parent_id.as_deref());}
            let mut chosen=false;ui.horizontal(|ui|{ui.add_space(depth as f32*10.);let image=match o.geometry{Geometry::Text{..}=>"type",Geometry::Ellipse=>"circle",Geometry::Image{..}=>"image",_=>"square"};let(r,_)=ui.allocate_exact_size(Vec2::splat(17.),egui::Sense::hover());shell::icon(ui,image,r,INK);let label=match &o.geometry{Geometry::Text{content,..}=>content.chars().take(14).collect::<String>(),Geometry::Ellipse=>format!("{} · {}",tr(self.chinese,"圆孔","Circle"),o.id),_=>o.id.clone()};let r=ui.selectable_label(self.selection.contains(&o.id),label);record_widget!(w,format!("object-{}",o.id),rect(r.rect));chosen=r.clicked();});if chosen&&self.prepare_navigation(s).is_some(){let mut ids=if ui.input(|i|i.modifiers.shift||i.modifiers.command){self.selection.clone()}else{vec![]};if ids.contains(&o.id){ids.retain(|id|id!=&o.id);}else{ids.push(o.id.clone());}self.set_selection(ids);}
        }
    }

}

#[cfg(test)]mod tests{
    use super::*;use kurbo::ParamCurveArclen;use arranger_core::Object;use serde_json::json;
    #[test]fn closed_split_preserves_geometry_and_all_subpaths(){let p=BezPath::from_svg("M0 0 L10 0 L10 10 L0 10 Z M20 20 L25 25").unwrap();let q=node_path(&p,2,"open").unwrap();assert_eq!(q.elements().iter().filter(|p|matches!(p,PathEl::MoveTo(_))).count(),2);assert!(!q.elements().iter().any(|p|matches!(p,PathEl::ClosePath)));assert_eq!(nodes::segments(&p).iter().map(|(_,s)|s.arclen(1e-8)).sum::<f64>(),nodes::segments(&q).iter().map(|(_,s)|s.arclen(1e-8)).sum::<f64>());}
    #[test]fn implicit_closing_edge_survives_curve_conversion_and_smoothing(){
        let p=BezPath::from_svg("M0 0 L10 0 L10 10 L0 10 Z").unwrap();
        for action in ["line","curve","smooth","symmetric"]{let q=node_path(&p,0,action).unwrap();assert!(matches!(q.elements().last(),Some(PathEl::ClosePath)));assert_eq!(q.elements().iter().filter(|e|matches!(e,PathEl::MoveTo(_))).count(),1);}
        let q=node_path(&p,0,"symmetric").unwrap();let a=nodes::control_arms(&q,0);assert_eq!(a.len(),2);let x=a[0].control-a[0].anchor;let y=a[1].control-a[1].anchor;assert!((x+y).hypot()<1e-9);
    }
    #[test]fn alignment_lowers_world_delta_and_prechecks_locked_roots(){let mut d=Document::new("layout".into(),210.,297.).unwrap();for(id,x,y)in[("a",10.,20.),("b",30.,40.),("c",80.,70.)]{let o:Object=serde_json::from_value(json!({"id":id,"page_id":"page-1","x_mm":x,"y_mm":y,"width_mm":10.,"height_mm":10.,"geometry":{"kind":"rect"}})).unwrap();d.objects.push(o);}let ids=vec!["a".into(),"b".into(),"c".into()];let p=selection::projected(&d,&arrange(&d,&ids,"center-x").unwrap()).unwrap();assert!(p.iter().all(|o|(d.bounds(o).unwrap()[0]-45.).abs()<1e-9));d.objects[2].locked=true;assert!(arrange(&d,&ids,"start-y").is_err());}
}

impl App{
    pub(super) fn work_canvas(&mut self,ui:&mut egui::Ui,s:&Snapshot,w:&mut BTreeMap<String,Value>)->View{
        let top=egui::Panel::top("ruler-top").exact_size(24.).resizable(false).frame(frame(Color32::from_rgb(248,248,249),0,0)).show(ui,|_|{}).response.rect;
        let left=egui::Panel::left("ruler-left").exact_size(24.).resizable(false).frame(frame(Color32::from_rgb(248,248,249),0,0)).show(ui,|_|{}).response.rect;
        let view=self.canvas(ui,s,w);
        let step_base=(40./view.scale).max(0.0001)as f64;let power=10f64.powf(step_base.log10().floor());let step=[1.,2.,5.,10.].into_iter().map(|v|v*power).find(|v|*v>=step_base).unwrap();
        #[cfg(feature="test-hooks")]let mut labels=vec![];
        for(horizontal,r)in[(true,top),(false,left)]{let clip=if horizontal{Rect::from_min_max(Pos2::new(view.canvas.min.x,r.min.y),r.max)}else{r};let painter=ui.painter().with_clip_rect(clip);let origin=if horizontal{view.origin.x}else{view.origin.y};let min=if horizontal{view.canvas.min.x}else{view.canvas.min.y};let max=if horizontal{view.canvas.max.x}else{view.canvas.max.y};let first=((min-origin)as f64/view.scale as f64/step).floor()as i64;let last=((max-origin)as f64/view.scale as f64/step).ceil()as i64;
            for n in first..=last{let value=n as f64*step;let at=origin+value as f32*view.scale;let(a,b,label)=if horizontal{(Pos2::new(at,r.max.y-8.),Pos2::new(at,r.max.y),Pos2::new(at+3.,r.min.y+2.))}else{(Pos2::new(r.max.x-6.,at),Pos2::new(r.max.x,at),Pos2::new(r.min.x+1.,at+2.))};painter.line_segment([a,b],egui::Stroke::new(1.,MUTED));
                let galley=painter.layout_no_wrap(editing::display_number(value),egui::FontId::proportional(10.),INK);let bounds=Rect::from_min_size(label,galley.size());
                // Keep ticks at the edge, but only draw a complete label.
                // Clipping half a number leaves an unrelated digit by "mm".
                let visible=clip.contains_rect(bounds);if visible{painter.galley(label,galley,INK);}
                #[cfg(feature="test-hooks")]labels.push(json!({"horizontal":horizontal,"value":value,"bounds":rect(bounds),"clip":rect(clip),"visible":visible}));
            }
            painter.line_segment(if horizontal{[r.left_bottom(),r.right_bottom()]}else{[r.right_top(),r.right_bottom()]},egui::Stroke::new(1.,LINE));
        }
        #[cfg(feature="test-hooks")] {self.performance["ruler_labels"]=json!(labels);}
        ui.painter().text(top.left_top()+Vec2::new(3.,2.),egui::Align2::LEFT_TOP,"mm",egui::FontId::proportional(10.),MUTED);view
    }
}

pub(super) fn alignment_controls(ui:&mut egui::Ui,s:&Snapshot,count:usize,clean:bool,w:&mut BTreeMap<String,Value>,zh:bool)->Option<&'static str>{
    let mut action=None;ui.add_space(4.);ui.separator();ui.label(egui::RichText::new(tr(zh,"对齐与分布","Align & distribute")).strong());
    let disabled_help = if s.busy {
        tr(zh,"请先完成当前文档操作","Finish the current document operation")
    } else if !clean {
        tr(zh,"请先应用或恢复属性修改","Apply or reload property changes first")
    } else {
        tr(zh,"至少选择两个对象以对齐","Select at least two objects to align")
    };
    ui.horizontal(|ui| {
        for (key, image, chinese, english) in [
            ("start-x","align-start-vertical","左对齐","Align left edges"),
            ("center-x","align-center-vertical","水平居中","Align horizontal centers"),
            ("end-x","align-end-vertical","右对齐","Align right edges"),
            ("start-y","align-start-horizontal","顶端对齐","Align top edges"),
            ("center-y","align-center-horizontal","垂直居中","Align vertical centers"),
            ("end-y","align-end-horizontal","底端对齐","Align bottom edges"),
        ] {
            let response = shell::icon_button(ui,w,&format!("align-{key}"),image,"",!s.busy&&clean&&count>1,false);
            if response.on_hover_text(tr(zh,chinese,english)).on_disabled_hover_text(disabled_help).clicked() {
                action = Some(key);
            }
        }
    });
    ui.horizontal(|ui|{for(key,image,z,e)in[("distribute-x","align-horizontal-distribute-center","水平分布","Horizontal"),("distribute-y","align-vertical-distribute-center","垂直分布","Vertical")]{if shell::icon_button(ui,w,&format!("align-{key}"),image,tr(zh,z,e),!s.busy&&clean&&count>2,false).on_disabled_hover_text(if !s.busy&&clean {tr(zh,"至少选择三个对象以分布","Select at least three objects to distribute")} else {disabled_help}).clicked(){action=Some(key);}}});ui.separator();action
}
