use super::*;
use editing::{Tool,translate,transform};
use arranger_core::Document;
use kurbo::{Affine,Point};

#[derive(Clone)]
pub(super) enum GestureKind { Move,Resize(usize),Rotate{center:Point},Pivot{center:Point},Box(bool),Draw(Tool),Node{object:String,index:usize,point:usize} }
pub(super) struct Gesture {pub kind:GestureKind,pub start:Pos2,pub revision:u64,pub ids:Vec<String>,pub bounds:kurbo::Rect}
#[derive(Clone)]
pub(super) struct PenPoint {pub point:Point,pub handle:kurbo::Vec2}
#[derive(Clone,Copy)]
pub(super) struct View {pub canvas:Rect,pub origin:Pos2,pub scale:f32}
impl Default for View {fn default()->Self{Self{canvas:Rect::NOTHING,origin:Pos2::ZERO,scale:1.}}}
impl View {
    pub(super) fn screen(self,p:Point)->Pos2 {self.origin+Vec2::new(p.x as f32,p.y as f32)*self.scale}
    pub(super) fn local(self,p:Pos2)->Point {let v=(p-self.origin)/self.scale;Point::new(f64::from(v.x),f64::from(v.y))}
}
fn node_outline(painter:&egui::Painter,path:&kurbo::BezPath,world:Affine,view:View) {
    let screen=|p|view.screen(world*p);
    let segments=nodes::segments(path);
    for stroke in [egui::Stroke::new(2.5,Color32::WHITE),egui::Stroke::new(1.2,ACCENT)] {
        for (_,segment) in &segments {
            match segment {
                kurbo::PathSeg::Line(line)=>{painter.line_segment([screen(line.p0),screen(line.p1)],stroke);}
                kurbo::PathSeg::Quad(q)=>{painter.add(egui::epaint::QuadraticBezierShape::from_points_stroke([screen(q.p0),screen(q.p1),screen(q.p2)],false,Color32::TRANSPARENT,stroke));}
                kurbo::PathSeg::Cubic(c)=>{painter.add(egui::epaint::CubicBezierShape::from_points_stroke([screen(c.p0),screen(c.p1),screen(c.p2),screen(c.p3)],false,Color32::TRANSPARENT,stroke));}
            }
        }
    }
}
impl App {
    pub(super) fn zoom_by(&mut self,factor:f32,anchor:Option<Pos2>,doc:&Document) {
        if !factor.is_finite()||factor<=0.{return;}
        let page=match doc.page(&self.page){Ok(p)=>p,Err(_)=>return,};let old=self.view;
        self.zoom=(old.scale/(96./25.4)*factor).clamp(0.02,64.);self.fit=false;
        if let Some(anchor)=anchor {let local=(anchor-old.origin)/old.scale;let scale=(96./25.4)*self.zoom;
            self.pan=anchor-local*scale-(old.canvas.center()-Vec2::new(page.width_mm as f32,page.height_mm as f32)*scale/2.);
        }
    }
    pub(super) fn finish_pen(&mut self,snapshot:&Snapshot,closed:bool) {
        if self.pen.len()<2{return;}
        let mut path=kurbo::BezPath::new();path.move_to(self.pen[0].point);
        for pair in self.pen.windows(2){let (a,b)=(&pair[0],&pair[1]);
            if a.handle.hypot()<1e-8&&b.handle.hypot()<1e-8{path.line_to(b.point);}else{path.curve_to(a.point+a.handle,b.point-b.handle,b.point);}
        }
        if closed {let a=self.pen.last().unwrap();let b=&self.pen[0];if a.handle.hypot()>1e-8||b.handle.hypot()>1e-8{path.curve_to(a.point+a.handle,b.point-b.handle,b.point);}path.close_path();}
        use kurbo::Shape;let b=path.bounding_box();
        let mut object=self.object(&snapshot.document,Geometry::Path{d:path.to_svg(),fill_rule:Default::default()},0.,0.,b.width(),b.height());
        if !closed{object.style.fill="none".into();if object.style.stroke=="none"{object.style.stroke="#344054".into();object.style.stroke_width_mm=0.4;}}
        let id=object.id.clone();if self.batch(snapshot.revision,vec![Operation::ObjectCreate{object}]){self.pen.clear();self.set_selection(vec![id]);}
    }
    fn node_drag(&self,snapshot:&Snapshot,gesture:&Gesture,end:Pos2,view:View,snapping:bool)->arranger_core::Result<(kurbo::BezPath,Option<nodes::Snap>)> {
        let GestureKind::Node{object,index,point}=&gesture.kind else{unreachable!()};
        let doc=&snapshot.document;doc.require_editable(object)?;
        let o=doc.object(object)?;let world=doc.world_transform(o)?;
        if world.determinant().abs()<1e-12{return Err(arranger_core::Error::new("SINGULAR_TRANSFORM","Cannot edit singular geometry"));}
        let original_path=o.local_path()?;
        let original=nodes::points(original_path.elements()[*index])[*point];
        let position=world*original+(view.local(end)-view.local(gesture.start));
        let snap=if snapping{self.node_queries.borrow_mut().snap(doc,&self.page,position,f64::from(7./view.scale))}else{None};
        let mut path=nodes::moved(&original_path,*index,*point,world.inverse()*snap.as_ref().map_or(position,|s|s.point))?;
        if self.node_link_target.as_ref().is_some_and(|(id,node)|id==object&&self.node_focus.as_ref().is_some_and(|(focus,_)|node==focus))&&self.node_kind!="cusp"&&*point+1<nodes::points(path.elements()[*index]).len(){if let Some((selected,_))=&self.node_focus{
            let arms=nodes::control_arms(&path,*selected);if arms.len()==2{if let Some(changed)=arms.iter().find(|a|a.index==*index&&a.point==*point){
                let other=arms.iter().find(|a|a.index!=*index||a.point!=*point).unwrap();let delta=changed.control-changed.anchor;let length=delta.hypot();if length>1e-12{let reach=if self.node_kind=="symmetric"{length}else{(other.control-other.anchor).hypot()};path=nodes::moved(&path,other.index,other.point,other.anchor-delta*(reach/length))?;}
            }}
        }}
        Ok((path,snap))
    }
    fn gesture_operations(&self,snapshot:&Snapshot,gesture:&Gesture,end:Pos2,view:View,snapping:bool)->arranger_core::Result<Vec<Operation>> {
        let doc=&snapshot.document;let delta=(end-gesture.start)/view.scale;
        match &gesture.kind {
            GestureKind::Move=>translate(doc,&gesture.ids,f64::from(delta.x),f64::from(delta.y)),
            GestureKind::Resize(index)=>{
                transform(doc,&gesture.ids,selection::resize(gesture.bounds,*index,view.local(end)-view.local(gesture.start)))
            },
            GestureKind::Rotate{center}=>{
                let a=view.local(gesture.start)-*center;let b=view.local(end)-*center;
                transform(doc,&gesture.ids,Affine::translate(center.to_vec2())*Affine::rotate(b.y.atan2(b.x)-a.y.atan2(a.x))*Affine::translate(-center.to_vec2()))
            },
            GestureKind::Node{object,..}=>{
                let (path,_)=self.node_drag(snapshot,gesture,end,view,snapping)?;
                nodes::operations(doc,object,&path)
            },_=>Ok(vec![])
        }
    }
    fn create_drag(&mut self,snapshot:&Snapshot,tool:Tool,start:Pos2,end:Pos2,view:View,revision:u64) {
        let (a,b)=(view.local(start),view.local(end));
        if tool==Tool::Pen {
            if self.pen.len()>2 && view.screen(self.pen[0].point).distance(start)<8. {self.finish_pen(snapshot,true);}
            else {self.pen.push(PenPoint{point:a,handle:b-a});}return;
        }
        if start.distance(end)<3.{return;}
        let r=kurbo::Rect::from_points(a,b);
        let geometry=match tool {Tool::Rect=>Geometry::Rect{radius_mm:0.},Tool::Ellipse=>Geometry::Ellipse,
            Tool::Line=>Geometry::Path{d:format!("M {} {} L {} {}",a.x-r.x0,a.y-r.y0,b.x-r.x0,b.y-r.y0),fill_rule:Default::default()},_=>return};
        let mut object=self.object(&snapshot.document,geometry,r.x0,r.y0,r.width(),r.height());
        if tool==Tool::Line {object.style.fill="none".into();if object.style.stroke=="none"{object.style.stroke="#344054".into();object.style.stroke_width_mm=0.4;}}
        let id=object.id.clone();if self.batch(revision,vec![Operation::ObjectCreate{object}]){self.set_selection(vec![id]);}
    }
    pub(super) fn canvas(&mut self,ui:&mut egui::Ui,snapshot:&Snapshot,widgets:&mut BTreeMap<String,Value>)->View {
        let ctx=ui.ctx().clone();let source_doc=&snapshot.document;
        if self.tool!=Tool::Select{self.selection_mode=Default::default();self.rotation_center=None;}
        if self.tool!=Tool::Nodes || !self.node_queries.get_mut().matches(source_doc,&self.page) {self.node_queries.get_mut().clear();}
        if self.tool!=Tool::Nodes||self.node_focus.as_ref().is_some_and(|(_,hash)|hash!=&snapshot.document_hash){self.node_focus=None;self.node_link_target=None;self.node_kind="cusp".into();}
        if self.gesture.as_ref().is_some_and(|g|g.revision!=snapshot.revision||snapshot.busy) {
            self.gesture=None;self.error=true;self.message=tr(self.chinese,"文档已被更新，已取消拖动","Document changed; drag cancelled").into();
        }
        let doc=source_doc;
        let (canvas,response)=ui.allocate_exact_size(ui.available_size(),egui::Sense::click_and_drag());
        let page=doc.page(&self.page).expect("current page");
        let fit_scale=((canvas.width()-72.).max(1.)/page.width_mm as f32).min((canvas.height()-56.).max(1.)/page.height_mm as f32);
        let mut scale=if self.fit{self.zoom=fit_scale/(96./25.4);fit_scale}else{(96./25.4)*self.zoom}.max(1e-8);
        let page_mm=Vec2::new(page.width_mm as f32,page.height_mm as f32);
        // Revealing tool options takes space from the top of the canvas.
        // In manual zoom keep the artwork under the same screen coordinate.
        if !self.fit && self.view.canvas.is_positive() && canvas.bottom() == self.view.canvas.bottom() {
            self.pan.y += (self.view.canvas.top() - canvas.top()) * 0.5;
        }
        // Apply navigation before projecting the document, text caret and selection handles.
        let space=ctx.input(|i|i.key_down(egui::Key::Space))&&!ctx.text_edit_focused()&&self.inline_text.is_none();
        if (self.tool==Tool::Pan||space)&&response.hovered(){ctx.set_cursor_icon(egui::CursorIcon::Grab);}
        else if matches!(self.tool,Tool::Rect|Tool::Ellipse|Tool::Line|Tool::Pen)&&response.hovered(){ctx.set_cursor_icon(egui::CursorIcon::Crosshair);}
        else if self.tool==Tool::Text&&response.hovered(){ctx.set_cursor_icon(egui::CursorIcon::Text);}
        if (response.dragged_by(egui::PointerButton::Middle)||(self.tool==Tool::Pan||space)&&response.dragged_by(egui::PointerButton::Primary))&&self.gesture.is_none(){self.pan+=ctx.input(|i|i.pointer.delta());}
        if response.hovered()&&!response.dragged()&&self.dialog.is_none(){
            let (zoom,scroll,source,anchor)=ctx.input(|i|(i.zoom_delta(),i.smooth_scroll_delta,i.scroll_source(),i.pointer.hover_pos()));
            // egui-winit translates native pinch gestures into Zoom, separately
            // from two-finger scrolling. Keep precise scrolling as canvas pan.
            let factor = if zoom != 1. { Some(zoom) }
            else if matches!(source,Some(egui::MouseWheelSource::Trackpad|egui::MouseWheelSource::Momentum)) {
                self.pan += scroll;
                None
            }
            else if scroll.y.abs()>0.1 { Some((scroll.y*0.002).exp()) } else { None };
            if let Some(factor) = factor {
                self.view = View { canvas, origin: canvas.center() - page_mm*scale/2. + self.pan, scale };
                self.zoom_by(factor,anchor,doc);
                scale = (96./25.4)*self.zoom;
            }
        }
        let page_size=page_mm*scale;
        let origin=canvas.center()-page_size/2.+self.pan;let view=View{canvas,origin,scale};self.view=view;
        self.inline_input(ui,snapshot,view,widgets);
        let snapping=self.node_snapping&&!ctx.input(|i|i.modifiers.alt);
        let mut snap=None;
        let preview=self.gesture.as_ref().and_then(|gesture|{
            let end=ctx.input(|i|i.pointer.latest_pos())?;
            match &gesture.kind {
                GestureKind::Node{object,..}=>{let (path,target)=self.node_drag(snapshot,gesture,end,view,snapping).ok()?;snap=target;nodes::projected(source_doc,object,&path).ok().map(|o|vec![o])},
                GestureKind::Move|GestureKind::Resize(_)|GestureKind::Rotate{..}=>{
                    let ops=self.gesture_operations(snapshot,gesture,end,view,snapping).ok()?;selection::projected(source_doc,&ops).ok()
                },_=>None
            }
        });
        let preview=preview.or_else(||self.text_preview(snapshot).map(|o|vec![o]));
        let mut cache=self.preview_cache.take();
        let active_preview=preview.and_then(|objects|{
            if !cache.as_ref().is_some_and(|(p,_)|p.matches(source_doc,&objects)){
                cache=Some((arranger_core::model::DocumentPreview::new(snapshot,&objects).ok()?,snapshot.document_hash.clone()));
            }
            let (scene,key)=cache.as_mut()?;
            if scene.update(objects).ok()?{
                #[cfg(feature="test-hooks")] { *key=scene.document().hash(); }
                #[cfg(not(feature="test-hooks"))] { *key=if scene.at_source(){snapshot.document_hash.clone()}else{format!("preview-{}",self.frames)}; }
            }
            Some(())
        }).is_some();
        if !active_preview{cache=None;}
        let (doc,rendered_hash)=cache.as_ref().map_or((source_doc.as_ref(),snapshot.document_hash.clone()),|(scene,key)|(scene.document(),key.clone()));
        let page_rect=Rect::from_min_size(origin,page_size);let painter=ui.painter().with_clip_rect(canvas);
        painter.rect_filled(canvas,0.,Color32::from_rgb(237,239,243));
        painter.rect_filled(page_rect.translate(Vec2::new(0.,3.)).expand(1.),1.,Color32::from_rgb(214,218,225));
        painter.rect_filled(page_rect,0.,Color32::WHITE);
        let visible=canvas;
        if visible.is_positive(){
            let ppp=ctx.pixels_per_point();
            let min=Pos2::new((visible.min.x*ppp).floor()/ppp,(visible.min.y*ppp).floor()/ppp);
            let max=Pos2::new((visible.max.x*ppp).ceil()/ppp,(visible.max.y*ppp).ceil()/ppp);
            let rendered_rect=Rect::from_min_max(min,max);
            let region=[f64::from((min.x-origin.x)/scale),f64::from((min.y-origin.y)/scale),f64::from(rendered_rect.width()/scale),f64::from(rendered_rect.height()/scale)];
            let key=format!("{}:{}:{:?}:{:?}:{}",rendered_hash,self.page,region,scale,ppp);
            if self.raster_key!=key {
                #[cfg(feature="test-hooks")]let render_started=std::time::Instant::now();
                let projection_key=format!("{}:{}",rendered_hash,self.page);
                let render=(||{
                    if self.viewport_cache.as_ref().is_none_or(|(key,_)|key!=&projection_key){
                        self.viewport_cache=None;
                        self.viewport_cache=Some((projection_key,self.renderer.prepare_viewport(doc,&self.page)?));
                        #[cfg(feature="test-hooks")] {self.performance["projection_builds"]=json!(self.performance["projection_builds"].as_u64().unwrap_or(0)+1);}
                    }
                    self.renderer.render_projection(&self.viewport_cache.as_ref().expect("prepared viewport").1,f64::from(scale*ppp),region)
                })();
                #[cfg(feature="test-hooks")] {self.performance["projection_input_bytes"]=json!(self.viewport_cache.as_ref().map_or(0,|(_,p)|p.input_bytes()));}
                #[cfg(feature="test-hooks")] {self.performance["render_ms"]=json!(render_started.elapsed().as_secs_f64()*1000.);self.performance["renders"]=json!(self.performance["renders"].as_u64().unwrap_or(0)+1);}
                match render {
                    Ok(rendered)=>{self.render_notices=snapshot.warnings.iter().filter(|n|n.code.starts_with("CDR_")).cloned().collect();self.render_notices.extend(rendered.warnings);self.render_notices.dedup_by(|a,b|a.code==b.code&&a.message==b.message);self.notice_hash=rendered_hash.clone();let pixels=&rendered.pixels;self.raster_size=[pixels.width(),pixels.height()];
                        self.raster_region_mm=region;self.raster_density=f64::from(scale*ppp);
                        let image=egui::ColorImage::from_rgba_premultiplied([pixels.width() as usize,pixels.height() as usize],pixels.data());
                        if let Some(texture)=&mut self.texture {texture.set(image,egui::TextureOptions::LINEAR);}else{self.texture=Some(ctx.load_texture("document",image,egui::TextureOptions::LINEAR));}
                        self.texture_key=format!("{}:{}",rendered_hash,self.page);self.raster_key=key;
                        #[cfg(feature="test-hooks")] if let Some(out)=&self.test_output{if std::env::var_os("ARRANGER_TEST_NO_CANVAS_DUMP").is_none(){let _=pixels.save_png(out.join("canvas-latest.png"));}}
                    },Err(error)=>{self.error=true;self.message=error.to_string();self.texture=None;self.raster_key.clear();}
                }
            }
            if let Some(texture)=&self.texture{painter.image(texture.id(),rendered_rect,Rect::from_min_max(Pos2::ZERO,Pos2::new(1.,1.)),Color32::WHITE);}
        }
        painter.rect_stroke(page_rect,0.,egui::Stroke::new(1.,Color32::from_rgb(212,217,225)),egui::StrokeKind::Outside);
        let mut handles=vec![];
        if self.tool==Tool::Select && self.inline_text.is_none() {if let Some(b)=self.selection_bounds.get(doc,&self.selection,&rendered_hash).union {
            if self.selection_mode==selection::Mode::Scale {
                for (index,p) in selection::handles(b).into_iter().enumerate(){let center=view.screen(p);let rect=Rect::from_center_size(center,Vec2::splat(8.));
                    painter.rect_filled(rect,0.,Color32::WHITE);painter.rect_stroke(rect,0.,egui::Stroke::new(1.3,ACCENT),egui::StrokeKind::Outside);
                    record_widget!(widgets,format!("handle-{index}"),super::rect(rect));handles.push((center,GestureKind::Resize(index)));
                }
            }else {
                let pivot=match self.gesture.as_ref().map(|g|&g.kind){
                    Some(GestureKind::Rotate{center}|GestureKind::Pivot{center})=>*center,
                    _=>self.rotation_center.unwrap_or(b.center()),
                };
                for (index,p) in selection::handles(b)[..4].iter().enumerate(){
                    let center=view.screen(*p)+Vec2::new(if index==0||index==3{-10.}else{10.},if index<2{-10.}else{10.});
                    selection::rotate_arrow(&painter,center,index);record_widget!(widgets,format!("handle-rotate-{index}"),super::rect(Rect::from_center_size(center,Vec2::splat(18.))));
                    handles.push((center,GestureKind::Rotate{center:pivot}));
                }
                let pivot=if let Some(Gesture{kind:GestureKind::Pivot{..}|GestureKind::Move,start,..})=&self.gesture{
                    if self.rotation_center.is_some()||matches!(self.gesture.as_ref().unwrap().kind,GestureKind::Pivot{..}){ctx.input(|i|i.pointer.latest_pos()).map_or(pivot,|p|pivot+(view.local(p)-view.local(*start)))}else{pivot}
                }else{pivot};
                let center=view.screen(pivot);painter.circle_filled(center,5.,Color32::WHITE);painter.circle_stroke(center,5.,egui::Stroke::new(1.3,ACCENT));
                for axis in [Vec2::X,Vec2::Y]{painter.line_segment([center-axis*8.,center+axis*8.],egui::Stroke::new(1.3,ACCENT));}
                record_widget!(widgets,"rotation-center".into(),super::rect(Rect::from_center_size(center,Vec2::splat(16.))));handles.push((center,GestureKind::Pivot{center:pivot}));
            }
        }}
        if self.tool==Tool::Select&&!space&&self.inline_text.is_none(){
            if let Some(pos)=ctx.input(|i|i.pointer.hover_pos()){
                let kind=self.gesture.as_ref().map(|g|&g.kind).or_else(||handles.iter().filter(|(p,_)|p.distance(pos)<10.).min_by(|a,b|a.0.distance(pos).total_cmp(&b.0.distance(pos))).map(|(_,k)|k));
                if let Some(kind)=kind{ctx.set_cursor_icon(match kind{
                    GestureKind::Resize(0|2)=>egui::CursorIcon::ResizeNwSe,GestureKind::Resize(1|3)=>egui::CursorIcon::ResizeNeSw,
                    GestureKind::Resize(4|6)=>egui::CursorIcon::ResizeVertical,GestureKind::Resize(_)=>egui::CursorIcon::ResizeHorizontal,
                    GestureKind::Rotate{..}=>egui::CursorIcon::Crosshair,_=>egui::CursorIcon::Move,
                });}
            }
        }
        if self.tool==Tool::Nodes {if let Some(id)=self.selected.as_deref(){if let Ok(o)=doc.object(id){if nodes::supported(o){if let (Ok(path),Ok(world))=(o.local_path(),doc.world_transform(o)) {
            node_outline(&painter,&path,world,view);
            if let Some((selected,_))=&self.node_focus {
              if self.show_node_handles {
                for arm in nodes::control_arms(&path,*selected) {
                    let start=view.screen(world*arm.anchor);let end=view.screen(world*arm.control);
                    let direction=(end-start).normalized();
                    let direction=if direction.is_finite(){direction}else{Vec2::X};
                    let tip=if start.distance(end)<9.{start+direction*14.}else{end};
                    painter.line_segment([start,tip],egui::Stroke::new(2.5,Color32::WHITE));
                    painter.line_segment([start,tip],egui::Stroke::new(1.,ACCENT));
                    let base=tip-direction*7.;let side=Vec2::new(-direction.y,direction.x)*3.5;
                    painter.add(egui::Shape::convex_polygon(vec![tip,base+side,base-side],ACCENT,egui::Stroke::new(0.8,Color32::WHITE)));
                    let rect=Rect::from_center_size(tip,Vec2::splat(9.));
                    record_widget!(widgets,format!("node-{}-{}",arm.index,arm.point),super::rect(rect));
                    handles.push((tip,GestureKind::Node{object:id.into(),index:arm.index,point:arm.point}));
                }
            }
              }
            for (index,el) in path.iter().enumerate(){
                let points=nodes::points(el);
                if let Some(p)=points.last(){let point=points.len()-1;let center=view.screen(world*(*p));let rect=Rect::from_center_size(center,Vec2::splat(8.));
                    if nodes::duplicate_anchor(&path,index){continue;}
                    let selected=self.node_focus.as_ref().is_some_and(|(i,_)|*i==index);
                    painter.rect_filled(rect,0.,if selected{ACCENT}else{Color32::WHITE});painter.rect_stroke(rect,0.,egui::Stroke::new(1.4,ACCENT),egui::StrokeKind::Outside);
                    record_widget!(widgets,format!("node-{index}-{point}"),super::rect(rect));handles.push((center,GestureKind::Node{object:id.into(),index,point}));
                }
            }
        }}}}}
        if let Some(snap)=snap {
            let center=view.screen(snap.point);let color=Color32::from_rgb(185,72,150);
            painter.circle_stroke(center,7.,egui::Stroke::new(1.5,color));
            painter.line_segment([center-Vec2::new(10.,0.),center+Vec2::new(10.,0.)],egui::Stroke::new(1.,color));
            painter.line_segment([center-Vec2::new(0.,10.),center+Vec2::new(0.,10.)],egui::Stroke::new(1.,color));
            painter.text(center+Vec2::new(12.,-16.),egui::Align2::LEFT_BOTTOM,if snap.anchor{tr(self.chinese,"节点吸附","Snap to node")}else{tr(self.chinese,"边缘吸附","Snap to edge")},egui::FontId::proportional(12.),color);
        }
        if active_preview{if let Some(Gesture{kind:GestureKind::Node{object,index,point},..})=&self.gesture{
            if let Ok(object)=doc.object(object){if let (Ok(path),Ok(world))=(object.local_path(),doc.world_transform(object)){
                let p=world*nodes::points(path.elements()[*index])[*point];
                painter.text(view.screen(p)+Vec2::new(12.,14.),egui::Align2::LEFT_TOP,format!("{}, {} mm",editing::display_number(p.x),editing::display_number(p.y)),egui::FontId::proportional(12.),INK);
            }}
        }}
        let editable=self.dialog.is_none()&&!snapshot.busy&&self.inline_text.is_none();
        // A text session owns ordinary canvas clicks, but clicking a different
        // text is an explicit target change. Process this after native IME and
        // text input for the frame, then acquire the new source revision.
        if self.tool==Tool::Text&&self.inline_text.is_some()&&!snapshot.busy&&self.dialog.is_none()&&!self.quit_prompt&&response.clicked_by(egui::PointerButton::Primary){
            if let Some(pos)=response.interact_pointer_pos(){
                if let Some(id)=text_edit::hit(doc,&self.page,view.local(pos),f64::from(5./scale)){
                    if self.selected.as_deref()!=Some(id.as_str())&&doc.require_editable(&id).is_ok()&&self.finish_text(&ctx,snapshot){
                        if let Ok(current)=self.service.snapshot(&self.active){self.begin_text(&ctx,&current,id,Some(pos),false);}
                    }
                }
            }
        }
        if editable&&response.drag_started_by(egui::PointerButton::Primary)&&self.tool!=Tool::Pan&&!space {
            if let Some(start)=ctx.input(|i|i.pointer.press_origin()) {
                ctx.memory_mut(|m|m.stop_text_input());
                let handle=handles.iter().filter(|(p,_)|p.distance(start)<9.).min_by(|a,b|a.0.distance(start).total_cmp(&b.0.distance(start))).map(|(_,k)|k.clone());
                let kind=if let Some(kind)=handle{kind}
                    else if matches!(self.tool,Tool::Rect|Tool::Ellipse|Tool::Line|Tool::Pen){GestureKind::Draw(self.tool)}
                    else {let p=view.local(start);let hit=if self.tool==Tool::Nodes{self.node_queries.borrow_mut().hit(source_doc,&self.page,p,f64::from(5./scale))}else{doc.hit_test(&self.page,[p.x,p.y],f64::from(5./scale))};
                        if let Some(id)=hit {let id=if self.tool==Tool::Nodes{id}else{editing::top_object(doc,id)};
                            if !self.selection.contains(&id){self.set_selection(vec![id]);}GestureKind::Move
                        }else{GestureKind::Box(ctx.input(|i|i.modifiers.shift))}};
                if let GestureKind::Node{object,index,point}=&kind {
                    if source_doc.object(object).and_then(|o|o.local_path()).is_ok_and(|p|*point+1==nodes::points(p.elements()[*index]).len()){
                        self.set_node_focus(*index,snapshot.document_hash.clone());
                    }
                }
                let bounds=self.selection_bounds.get(doc,&self.selection,&rendered_hash).union.unwrap_or(kurbo::Rect::ZERO);
                self.gesture=Some(Gesture{kind,start,revision:snapshot.revision,ids:self.selection.clone(),bounds});
                // The preview pass precedes input handling. Paint this first drag
                // position even if the pointer stays still until button release.
                ctx.request_repaint();
            }
        }
        if editable&&response.clicked_by(egui::PointerButton::Primary)&&self.tool!=Tool::Pan&&!space {
            if let Some(pos)=response.interact_pointer_pos(){ctx.memory_mut(|m|m.stop_text_input());let p=view.local(pos);
                match self.tool {
                    Tool::Select=>{
                        let hit=doc.hit_test(&self.page,[p.x,p.y],f64::from(5./scale));
                        if let Some(id)=response.double_clicked().then(||text_edit::hit(doc,&self.page,p,f64::from(5./scale))).flatten(){self.begin_text(&ctx,snapshot,id,Some(pos),false);}
                        else if !handles.iter().any(|(center,_)|center.distance(pos)<10.) {
                            let add=ctx.input(|i|i.modifiers.shift||i.modifiers.command);
                            let id=hit.map(|id|editing::top_object(doc,id));
                            if !add&&id.as_ref().is_some_and(|id|self.selection.contains(id)){
                                self.selection_mode=if self.selection_mode==selection::Mode::Scale{selection::Mode::Rotate}else{selection::Mode::Scale};
                            }else{self.select_hit(doc,id,add);}
                        }
                    },
                    Tool::Nodes=>{
                        if response.double_clicked(){
                            if let Some(id)=self.selected.clone().filter(|id|source_doc.object(id).is_ok_and(nodes::supported)) {
                                let result=(||{
                                    source_doc.require_editable(&id)?;
                                    let object=source_doc.object(&id)?;let path=object.local_path()?;
                                    let anchor=handles.iter().filter_map(|(center,kind)|{
                                        if let GestureKind::Node{index,point,..}=kind {
                                            (center.distance(pos)<9.&&*point+1==nodes::points(path.elements()[*index]).len()).then_some((center.distance(pos),*index))
                                        }else{None}
                                    }).min_by(|a,b|a.0.total_cmp(&b.0));
                                    let path=if let Some((_,index))=anchor{nodes::deleted(&path,index)?}
                                        else if let Some((index,t,_,_))=nodes::nearest(&path,source_doc.world_transform(object)?,p,f64::from(7./scale)){nodes::inserted(&path,index,t)?}
                                        else{return Ok(vec![]);};
                                    nodes::operations(source_doc,&id,&path)
                                })();self.operations(snapshot,result);
                            }
                        }else if let Some((_,GestureKind::Node{index,point,..}))=handles.iter().filter(|(p,_)|p.distance(pos)<9.).min_by(|a,b|a.0.distance(pos).total_cmp(&b.0.distance(pos))){
                            if self.selected.as_deref().and_then(|id|source_doc.object(id).ok()).and_then(|o|o.local_path().ok()).is_some_and(|path|*point+1==nodes::points(path.elements()[*index]).len()){
                                self.set_node_focus(*index,snapshot.document_hash.clone());
                            }
                        }else{
                            let id=self.node_queries.borrow_mut().hit(source_doc,&self.page,p,f64::from(5./scale));self.set_selection(id.into_iter().collect());
                        }
                    },
                    Tool::Text=>{
                        if let Some(id)=text_edit::hit(doc,&self.page,p,f64::from(5./scale)) {self.begin_text(&ctx,snapshot,id,Some(pos),false);}
                        else{let mut object=self.object(doc,Geometry::Text{content:tr(self.chinese,"输入文字","Type here").into(),font_family:self.ui_font.clone(),font_size_mm:self.new_text_size,text_path:None,layout:self.new_text_layout.clone()},p.x,p.y,100.,15.);
                            object.style.fill="#25334b".into();let id=object.id.clone();if self.batch(snapshot.revision,vec![Operation::ObjectCreate{object}]){if let Ok(current)=self.service.snapshot(&self.active){self.begin_text(&ctx,&current,id,None,true);}}}
                    },
                    Tool::Pen=>{
                        if self.pen.len()>2&&view.screen(self.pen[0].point).distance(pos)<8.{self.finish_pen(snapshot,true);}
                        else if response.double_clicked(){self.finish_pen(snapshot,false);}
                        else{self.pen.push(PenPoint{point:p,handle:kurbo::Vec2::ZERO});}
                    },_=>{}
                }
            }
        }
        if let Some(gesture)=&self.gesture {if let Some(end)=ctx.input(|i|i.pointer.latest_pos()) {
            match gesture.kind {
                GestureKind::Box(_)=>{let r=Rect::from_two_pos(gesture.start,end);painter.rect_filled(r,0.,Color32::from_rgba_unmultiplied(255,87,24,18));painter.rect_stroke(r,0.,egui::Stroke::new(1.,ACCENT),egui::StrokeKind::Inside);},
                GestureKind::Draw(Tool::Line)=>{painter.line_segment([gesture.start,end],egui::Stroke::new(1.5,ACCENT));},
                GestureKind::Draw(tool)=>{let r=Rect::from_two_pos(gesture.start,end);if tool==Tool::Ellipse{painter.add(egui::epaint::EllipseShape{center:r.center(),radius:r.size()/2.,angle:0.,fill:Color32::from_rgba_unmultiplied(50,105,220,30),stroke:egui::Stroke::new(1.,ACCENT)});}else{painter.rect_stroke(r,0.,egui::Stroke::new(1.,ACCENT),egui::StrokeKind::Inside);}},
                _=>{}
            }
        }}
        if editable&&response.drag_stopped_by(egui::PointerButton::Primary) {if let (Some(gesture),Some(end))=(self.gesture.take(),ctx.input(|i|i.pointer.latest_pos())) {
            match gesture.kind {
                GestureKind::Draw(tool)=>self.create_drag(snapshot,tool,gesture.start,end,view,gesture.revision),
                GestureKind::Pivot{center}=>{self.rotation_center=Some(center+(view.local(end)-view.local(gesture.start)));},
                GestureKind::Box(add)=>{let r=kurbo::Rect::from_points(view.local(gesture.start),view.local(end));let mut ids=if add{self.selection.clone()}else{vec![]};
                    let already_selected:std::collections::HashSet<_>=self.selection.iter().take(ids.len()).map(String::as_str).collect();
                    ids.extend(doc.objects.iter().filter(|o|o.on_canvas(&self.page)&&o.parent_id.is_none()&&o.visible&&!already_selected.contains(o.id.as_str()))
                        .filter_map(|o|{let [x,y,w,h]=doc.bounds(o).ok()?;(r.intersect(kurbo::Rect::new(x,y,x+w,y+h)).area()>0.).then(||o.id.clone())}));
                    self.set_selection(ids);
                },_=>{let ops=self.gesture_operations(snapshot,&gesture,end,view,snapping);match ops{Ok(ops)=>{if !ops.is_empty()&&self.batch(gesture.revision,ops){
                    if matches!(gesture.kind,GestureKind::Node{..}){if let (Some((_,hash)),Ok(current))=(&mut self.node_focus,self.service.snapshot(&self.active)){*hash=current.document_hash;}}
                    if matches!(gesture.kind,GestureKind::Move){if let Some(center)=&mut self.rotation_center{*center+=view.local(end)-view.local(gesture.start);}}
                }},Err(e)=>{self.error=true;self.message=e.to_string();}}}
            }
        }}
        if self.tool!=Tool::Nodes && self.inline_text.is_none() {
            for b in &self.selection_bounds.get(doc,&self.selection,&rendered_hash).outlines {
                painter.rect_stroke(Rect::from_min_max(view.screen(b.origin()),view.screen(Point::new(b.x1,b.y1))),
                    0.,egui::Stroke::new(1.25,ACCENT),egui::StrokeKind::Outside);
            }
        }
        self.paint_text_edit(&painter,&snapshot.document_hash,doc,view,widgets);
        for pair in self.pen.windows(2){painter.line_segment([view.screen(pair[0].point),view.screen(pair[1].point)],egui::Stroke::new(1.5,ACCENT));}
        for point in &self.pen{let p=view.screen(point.point);painter.circle_filled(p,3.,ACCENT);if point.handle.hypot()>0.{painter.line_segment([view.screen(point.point-point.handle),view.screen(point.point+point.handle)],egui::Stroke::new(1.,ACCENT));}}
        if self.inline_text.is_none(){
            if response.secondary_clicked(){if let Some(pos)=response.interact_pointer_pos(){let p=view.local(pos);if let Some(id)=doc.hit_test(&self.page,[p.x,p.y],f64::from(5./scale)){let id=editing::top_object(doc,id);if !self.selection.contains(&id){self.set_selection(vec![id]);}}ctx.memory_mut(|m|m.stop_text_input());}}
            response.context_menu(|ui|{ui.set_min_width(158.);self.context_menu(ui,snapshot,widgets);});
        }
        self.preview_cache=cache;
        view
    }
}
