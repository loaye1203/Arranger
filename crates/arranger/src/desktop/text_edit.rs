//! Canvas text is painted by the document renderer, including during editing.
//! egui owns keyboard/clipboard/IME state, but its hidden text widget never
//! paints a second font layout over the document. Carets use the core shaper.
use super::*;
use arranger_core::{Document, Object, TextMode};
use egui::text::{CCursor, CCursorRange};
use kurbo::{Affine, ParamCurve, ParamCurveArclen, Point};

pub(super) struct InlineText {
    id:egui::Id,
    initial:Option<Pos2>,select_all:bool,
    drag_anchor:Option<CCursor>,
    range:CCursorRange,
    ime_active:bool,
    composition_state:Option<egui::text_edit::TextEditState>,
    cache:LayoutCache,
}
impl InlineText { pub(super) fn widget_id(&self)->egui::Id{self.id} pub(super) fn composing(&self)->bool{self.ime_active} }
#[derive(Default)]
struct LayoutCache {
    key:String, typography:String,
    widths:Vec<Arc<RowAdvances>>,
    layout:Option<Arc<EditLayout>>,
    galley: Option<(f32, Arc<egui::Galley>)>,
}
// A line stores one copy of its visible text and one width per character.
#[derive(Clone, Default)]
struct RowAdvances { text: String, widths: Vec<f64> }

impl RowAdvances {
    fn update(&mut self, doc: &Document, object: &Object, text: &str) -> arranger_core::Result<()> {
        let mut common_bytes = 0;
        let mut common_chars = 0;
        for (old, new) in self.text.chars().zip(text.chars()) {
            if old != new { break; }
            common_bytes += new.len_utf8();
            common_chars += 1;
        }
        let tail = arranger_core::text::caret_advances(doc, object, text, common_bytes)?;
        self.widths.truncate(common_chars);
        self.text.truncate(common_bytes);
        self.text.push_str(&text[common_bytes..]);
        self.widths.extend(tail);
        Ok(())
    }
}
struct EditRow {start:usize,chars:Vec<char>,x:Vec<f64>,ordered:bool,top:f64,hard:bool}
struct EditLayout {rows:Vec<EditRow>,size:f64,height:f64,path:Option<(kurbo::BezPath,f64,f64)>}

pub(super) fn focus_text_once(ctx:&egui::Context,id:egui::Id){
    // egui::Memory::request_focus interrupts the platform IME even when the
    // requested widget already has focus. Do not repeat it every frame.
    ctx.memory_mut(|memory|{if !memory.has_focus(id){memory.request_focus(id);}});
}
pub(super) fn editing_hint(zh:bool)->&'static str{
    tr(zh,"文字编辑 · 完成文字 / Cmd或Ctrl+Enter 结束 · Esc 取消","Editing text · Finish text / Cmd or Ctrl+Enter to finish · Esc to cancel")
}

fn rebase_editor(old:&Editor,doc:&Document,object:&Object,revision:u64)->Editor{
    let mut rebased=Editor::new(doc,object,revision);
    for(index,(before,after))in[(&old.x,&mut rebased.x),(&old.y,&mut rebased.y),(&old.width,&mut rebased.width),(&old.height,&mut rebased.height),(&old.rotation,&mut rebased.rotation)].into_iter().enumerate(){
        if before!=&editing::display_number(old.original_numbers[index]){*after=before.clone();rebased.dirty=true;}
    }
    rebased.dirty|=old.style!=object.style;rebased.style=old.style.clone();rebased
}

pub(super) fn hit(doc: &Document, page: &str, p: Point, tolerance: f64) -> Option<String> {
    let id = doc.hit_test_with_text_region(page, [p.x, p.y], tolerance, |object, shaped, local| {
        let Geometry::Text { content, layout, text_path, .. } = &object.geometry else { return false };
        if layout.mode == TextMode::Area || content.is_empty() {
            let frame = kurbo::Rect::new(0., 0., object.width_mm.max(8.), object.height_mm.max(5.));
            if frame.inset(tolerance).contains(local) { return true; }
        }
        // Pick the whitespace between glyphs on each straight row. A curved
        // story's bounding rectangle can enclose unrelated objects.
        text_path.is_none() && shaped.lines.iter().filter_map(|line| line.ink_bounds_local_mm)
            .any(|[x, y, w, h]| kurbo::Rect::new(x, y, x+w, y+h).contains(local))
    })?;
    matches!(doc.object(&id).ok()?.geometry, Geometry::Text { .. }).then_some(id)
}

impl LayoutCache {
    fn get(&mut self,source_key:&str,doc:&Document,o:&Object)->arranger_core::Result<Arc<EditLayout>> {
        let key=format!("{}:{}:{}",source_key,o.id,serde_json::to_string(&o.geometry).unwrap());
        if key==self.key{if let Some(layout)=&self.layout{return Ok(layout.clone());}}
        let Geometry::Text{content,font_family,font_size_mm,layout:settings,text_path}=&o.geometry else{unreachable!()};
        let typography=serde_json::to_string(&(font_family,font_size_mm,settings.letter_spacing_mm)).unwrap();
        if self.typography!=typography{self.widths.clear();self.typography=typography;}
        let core=arranger_core::text::layout(doc,o)?;
        let mut rows=vec![];
        let mut start_char = 0;
        let old_rows = std::mem::take(&mut self.widths);
        let mut measured: std::collections::HashMap<&str, Arc<RowAdvances>> = old_rows.iter()
            .map(|row| (row.text.as_str(), row.clone())).collect();
        self.widths.reserve(core.lines.len());
        for (index,line) in core.lines.iter().enumerate(){
            let [start,end]=line.source_range_bytes;
            let next=core.lines.get(index+1).map_or(content.len(),|l|l.source_range_bytes[0]);
            let gap=&content[end..next];let hard=!gap.is_empty();
            // CRLF is two source characters but one row separator. Preserve
            // both indices, with a zero-width CR before the final separator.
            let mut chars:Vec<_>=content[start..end].chars().collect();
            let mut separator:Vec<_>=gap.chars().collect();if hard{separator.pop();}chars.extend(separator);
            let origin=line.baseline_origin_mm.unwrap_or([0.,*font_size_mm]);
            let widths = if let Some(row) = measured.get(line.text.as_str()) {
                row.clone()
            } else {
                let mut row = old_rows.get(index).cloned().unwrap_or_default();
                Arc::make_mut(&mut row).update(doc, o, &line.text)?;
                measured.insert(line.text.as_str(), row.clone());
                row
            };
            let last = widths.widths.last().copied().unwrap_or(0.);
            let mut x = vec![origin[0]];
            x.extend((0..chars.len()).map(|i| origin[0]+widths.widths.get(i).copied().unwrap_or(last)));
            self.widths.push(widths);
            let row_len = chars.len() + usize::from(hard);
            let ordered = x.is_sorted();
            rows.push(EditRow{start:start_char,chars,x,ordered,top:origin[1]-font_size_mm,hard});
            start_char += row_len;
        }
        let path=if let Some(binding)=text_path {
            let guide=doc.object(&binding.path_id)?;
            let parent=if let Some(id)=&o.parent_id{doc.world_transform(doc.object(id)?)?}else{Affine::IDENTITY};
            let path=parent.inverse()*doc.world_transform(guide)?*guide.local_path()?;
            let offset=binding.start_offset_mm-match binding.anchor{arranger_core::TextAnchor::Start=>0.,arranger_core::TextAnchor::Middle=>core.advance_mm/2.,arranger_core::TextAnchor::End=>core.advance_mm};
            Some((path,offset,binding.baseline_offset_mm))
        }else{None};
        let layout=Arc::new(EditLayout{rows,size:*font_size_mm,height:core.line_height_mm,path});
        self.key=key;self.layout=Some(layout.clone());self.galley=None;Ok(layout)
    }

    fn keyboard_galley(&mut self, ui: &egui::Ui, source: &str, doc: &Document, object: &Object, scale: f32) -> arranger_core::Result<Arc<egui::Galley>> {
        let layout = self.get(source, doc, object)?;
        if let Some((old_scale, galley)) = &self.galley
            && *old_scale == scale && galley.pixels_per_point == ui.pixels_per_point() {
            return Ok(galley.clone());
        }
        let Geometry::Text { content, .. } = &object.geometry else { unreachable!() };
        let galley = layout.galley(ui, content, scale);
        self.galley = Some((scale, galley.clone()));
        Ok(galley)
    }
}
impl EditLayout {
    fn caret(&self,row:&EditRow,column:usize)->[Point;2] {
        let x=row.x[column];
        if let Some((path,offset,baseline))=&self.path {
            let mut distance=x+offset;
            for segment in path.segments(){
                let length=segment.arclen(1e-6);
                if distance<=length {
                    let t=segment.inv_arclen(distance.clamp(0.,length),1e-6);let p=segment.eval(t);
                    let d=segment.eval((t+1e-4).min(1.))-segment.eval((t-1e-4).max(0.));
                    let normal=if d.hypot()>1e-12{kurbo::Vec2::new(-d.y,d.x)/d.hypot()}else{kurbo::Vec2::new(0.,1.)};
                    return [p+normal*(baseline-self.size),p+normal*(baseline+self.size*0.2)];
                }distance-=length;
            }
        }
        [Point::new(x,row.top),Point::new(x,row.top+self.size*1.2)]
    }
    fn position(&self,cursor:CCursor)->[Point;2] {
        let mut found=None;
        for row in &self.rows{if (row.start..=row.start+row.chars.len()).contains(&cursor.index.0){
            found=Some(self.caret(row,cursor.index.0-row.start));if !cursor.prefer_next_row{break;}
        }}
        found.unwrap_or_else(||{let row=self.rows.last().expect("at least one text row");self.caret(row,row.chars.len())})
    }
    fn nearest(&self,pos:Pos2,world:Affine,view:View)->CCursor {
        let mut best=(f32::INFINITY,CCursor::new(0));
        let [sx, ky, kx, _, _, _] = world.as_coeffs();
        let horizontal = self.path.is_none() && sx > 0. && ky == 0. && kx == 0.;
        for row in &self.rows {
            let distance = |column| {
                let [a,b] = self.caret(row,column).map(|p| view.screen(world*p));
                let d = b-a;
                let t = if d.length_sq()>0. { ((pos-a).dot(d)/d.length_sq()).clamp(0.,1.) } else { 0. };
                pos.distance(a+d*t)
            };
            let columns = if horizontal && row.ordered && row.x.len() >= 32 {
                let split = row.x.partition_point(|&x| view.screen(world*Point::new(x,row.top)).x < pos.x);
                let left = split.saturating_sub(1);
                let right = split.min(row.x.len()-1);
                let nearest = distance(left).min(distance(right));
                if nearest.is_finite() {
                    // Repeated advances and f32 rounding can tie even distinct
                    // columns. The original screen-space scan chose the first.
                    let mut low = 0;
                    let mut high = right;
                    while low < high {
                        let mid = low + (high-low)/2;
                        if distance(mid) > nearest { low = mid+1; } else { high = mid; }
                    }
                    low..low+1
                } else { 0..row.x.len() }
            } else { 0..row.x.len() };
            for column in columns {
                let d = distance(column);
                if d < best.0 {
                    let mut cursor = CCursor::new(row.start+column);
                    cursor.prefer_next_row = column == 0;
                    best = (d, cursor);
                }
            }
        }
        best.1
    }
    fn contains(&self,pos:Pos2,world:Affine,view:View)->bool {
        if world.determinant().abs()<1e-12{return false;}
        if self.path.is_some(){return self.rows.iter().any(|r|(0..r.x.len()).any(|c|{
            let [a,b]=self.caret(r,c).map(|p|view.screen(world*p));Rect::from_two_pos(a,b).expand(self.size as f32*view.scale).contains(pos)
        }));}
        let p=world.inverse()*view.local(pos);let tolerance=f64::from(8./view.scale);
        self.rows.iter().any(|row|kurbo::Rect::new(row.x[0],row.top,row.x.last().copied().unwrap_or(0.).max(row.x[0]+8.),row.top+self.height.max(self.size)).inset(tolerance).contains(p))
    }
    fn galley(&self,ui:&egui::Ui,content:&str,scale:f32)->Arc<egui::Galley> {
        // Fixed-size template avoids growing the UI font atlas at large zoom.
        let template=ui.fonts_mut(|f|f.layout_no_wrap("M".into(),egui::FontId::proportional(14.),Color32::TRANSPARENT));
        let mut galley=(*template).clone();let mut job=(*galley.job).clone();job.text=content.into();for s in &mut job.sections{s.byte_range=egui::text::ByteIndex(0)..egui::text::ByteIndex(content.len());}galley.job=Arc::new(job);
        galley.rows.clear();galley.num_vertices=0;galley.num_indices=0;galley.mesh_bounds=Rect::NOTHING;
        let mut width=1.0_f32;
        for row in &self.rows {
            let mut placed=template.rows[0].clone();let mut logical=(*placed.row).clone();logical.glyphs.clear();logical.visuals=Default::default();
            for (index,chr) in row.chars.iter().enumerate(){let mut glyph=template.rows[0].glyphs[0];glyph.chr=*chr;
                glyph.pos=Pos2::new((row.x[index]-row.x[0]) as f32*scale,self.size as f32*scale);glyph.advance_width=(row.x[index+1]-row.x[index]) as f32*scale;
                glyph.line_height=self.height as f32*scale;glyph.font_ascent=self.size as f32*scale;glyph.font_height=self.size as f32*scale*1.2;glyph.font_face_ascent=glyph.font_ascent;glyph.font_face_height=glyph.font_height;logical.glyphs.push(glyph);
            }
            logical.size=Vec2::new((row.x.last().unwrap()-row.x[0]) as f32*scale,self.height as f32*scale);
            placed.pos=Pos2::new(row.x[0] as f32*scale,row.top as f32*scale);width=width.max(placed.pos.x+logical.size.x);
            placed.row=Arc::new(logical);placed.ends_with_newline=row.hard;galley.rows.push(placed);
        }
        galley.rect=Rect::from_min_size(Pos2::ZERO,Vec2::new(width,self.rows.len() as f32*self.height as f32*scale));Arc::new(galley)
    }
}

impl App {
    pub(super) fn begin_text(&mut self,ctx:&egui::Context,snapshot:&Snapshot,id:String,initial:Option<Pos2>,select_all:bool) {
        let doc=&snapshot.document;
        if snapshot.busy||doc.require_editable(&id).is_err(){return;}
        let Ok(o)=doc.object(&id)else{return;};if !matches!(o.geometry,Geometry::Text{..}){return;}
        self.set_selection(vec![id.clone()]);self.editor=Some(Editor::new(doc,o,snapshot.revision));self.inspector_tab=0;self.gesture=None;self.tool=Tool::Text;
        let widget=egui::Id::unique(("canvas-text",&self.active,&id));
        let mut state=egui::text_edit::TextEditState::default();state.clear_undoer();state.store(ctx,widget);
        ctx.memory_mut(|m|m.request_focus(widget));
        self.inline_text=Some(InlineText{id:widget,initial,select_all,drag_anchor:None,range:CCursorRange::default(),ime_active:false,composition_state:None,cache:LayoutCache::default()});
        self.message=editing_hint(self.chinese).into();self.error=false;
    }
    pub(super) fn text_preview(&self,snapshot:&Snapshot)->Option<Object> {
        let editor=self.editor.as_ref().filter(|e|e.text_dirty&&e.revision==snapshot.revision&&!snapshot.busy)?;
        snapshot.document.draft_text(&editor.id,&editor.geometry).ok()
    }
    pub(super) fn text_commit_ready(&mut self)->bool {
        if self.inline_text.as_ref().is_some_and(InlineText::composing){
            self.error=true;self.message=tr(self.chinese,"请先完成输入法选词，文字草稿已保留","Finish composing text first; the draft is retained").into();false
        }else{true}
    }
    pub(super) fn commit_text(&mut self,snapshot:&Snapshot)->bool {
        if !self.text_commit_ready(){return false;}
        let Some(old)=self.editor.clone().filter(|e|e.text_dirty)else{return true;};
        if old.revision!=snapshot.revision||snapshot.busy {self.error=true;self.message=tr(self.chinese,"文档已更新或正被编辑，文字草稿保留在右侧；请核对后重新应用","Document changed or busy; text draft retained in the inspector. Review before applying.").into();return false;}
        if let Err(error)=snapshot.document.draft_text(&old.id,&old.geometry){self.error=true;self.message=error.to_string();return false;}
        if let Ok(o)=snapshot.document.object(&old.id){if o.geometry==old.geometry{self.editor=Some(rebase_editor(&old,&snapshot.document,o,snapshot.revision));return true;}}
        if !self.batch(old.revision,vec![Operation::ObjectUpdate{object_id:old.id.clone(),patch:ObjectPatch{geometry:Some(old.geometry.clone()),..Default::default()}}]){return false;}
        // Text can grow/shrink its bounds. Rebase untouched numeric fields so
        // a later color-only Apply cannot resize it to the pre-edit bounds.
        if let Ok(current)=self.service.snapshot(&self.active){if let Ok(object)=current.document.object(&old.id){
            self.editor=Some(rebase_editor(&old,&current.document,object,current.revision));
        }}true
    }
    pub(super) fn end_text_input(&mut self,ctx:&egui::Context){
        if self.inline_text.take().is_some(){self.tool=Tool::Select;}
        self.text_field=None;self.command_focus=None;self.focus_text=false;ctx.memory_mut(|m|m.stop_text_input());
    }
    pub(super) fn cancel_text(&mut self,ctx:&egui::Context){
        self.editor=None;self.end_text_input(ctx);self.error=false;
        self.message=tr(self.chinese,"已取消本次文字修改","Text edit cancelled").into();
    }
    pub(super) fn finish_text(&mut self,ctx:&egui::Context,snapshot:&Snapshot)->bool {
        let editing=self.inline_text.is_some()||self.text_field.is_some();
        if !self.commit_text(snapshot){return false;}
        self.end_text_input(ctx);
        if editing{self.error=false;self.message=tr(self.chinese,"文字编辑已完成","Text editing finished").into();}true
    }
    pub(super) fn text_escape(&mut self,ctx:&egui::Context)->bool {
        if self.inline_text.as_ref().is_some_and(|e|!e.ime_active)&&ctx.input_mut(|i|i.consume_key(egui::Modifiers::NONE,egui::Key::Escape)){
            self.cancel_text(ctx);return true;
        }false
    }
    pub(super) fn text_boundary(&mut self,ctx:&egui::Context,snapshot:&Snapshot) {
        self.observe_command_focus(ctx,snapshot);
        let commands=egui::Popup::is_any_open(ctx)||ctx.input(|i|i.pointer.interact_pos()).is_some_and(|p|self.command_bar.contains(p));
        // A close confirmation is not a completed edit. Keep the draft and
        // original checkpoint intact until the user continues or discards.
        if self.quit_prompt||self.dialog.is_some(){return;}
        if self.inline_text.is_some()&&self.editor.as_ref().is_none_or(|e|e.revision!=snapshot.revision||snapshot.busy){
            self.inline_text=None;self.command_focus=None;ctx.memory_mut(|m|m.stop_text_input());
            self.error=true;self.message=tr(self.chinese,"文档已更新，已停止画布输入；文字草稿保留在右侧","Document changed; canvas input stopped. Text draft retained in inspector.").into();return;
        }
        // Canvas editing is an explicit session. Pointer leave, idle time,
        // focus loss and clicks elsewhere on the canvas never end it.
        // Tool/file/Finish actions own the explicit transition instead.
        // Text properties share the session; other property drafts keep Apply.
        if self.inline_text.is_some(){
            if let Some(pos)=ctx.input(|i|i.pointer.interact_pos()).filter(|_|ctx.input(|i|i.pointer.primary_pressed())){
                if !commands&&!self.view.canvas.contains(pos)&&!self.editor_actions.contains(pos)&&!self.text_actions.contains(pos)&&!self.text_properties.contains(pos){self.finish_text(ctx,snapshot);}
            }
            return;
        }
        let pressed=ctx.input(|i|i.pointer.any_pressed());
        let pos=ctx.input(|i|i.pointer.interact_pos());
        if pressed {if let Some(pos)=pos {
            // Apply owns its batch; Reload explicitly discards the draft.
            // Neither button is an implicit click-out commit.
            if self.editor_actions.contains(pos){return;}
            let inside=self.text_properties.contains(pos)||self.text_field.as_ref().is_some_and(|(_,r)|r.contains(pos));
            if !commands&&!inside&&(self.inline_text.is_some()||self.editor.as_ref().is_some_and(|e|e.text_dirty)){
                self.finish_text(ctx,snapshot);
            }
        }}
    }
    pub(super) fn inline_input(&mut self,ui:&mut egui::Ui,snapshot:&Snapshot,view:View,widgets:&mut BTreeMap<String,Value>) {
        if self.quit_prompt||self.dialog.is_some(){return;}
        let keyboard_target=self.text_command_target(ui.ctx());
        let Some(mut inline)=self.inline_text.take()else{return;};let Some(editor)=self.editor.as_mut()else{return;};
        let ctx=ui.ctx().clone();
        let result=(||->arranger_core::Result<()> {
            // A toolbar TextEdit/DragValue may own keyboard input while the
            // canvas draft remains open. Never steal its focus or characters.
            let command_pointer=egui::Popup::is_any_open(&ctx)||!self.text_commands.is_empty()||ctx.input(|i|i.pointer.interact_pos()).is_some_and(|p|self.command_bar.contains(p)||self.text_actions.contains(p)||self.text_properties.contains(p));
            // A menu item may overlap the canvas without being a canvas click.
            let canvas_press=!command_pointer&&ctx.input(|i|i.pointer.primary_pressed()&&i.pointer.interact_pos().is_some_and(|p|view.canvas.contains(p)));
            let text_event=ctx.input(|i|i.events.iter().any(|e|matches!(e,egui::Event::Key{..}|egui::Event::Text(_)|egui::Event::Copy|egui::Event::Cut|egui::Event::Paste(_)|egui::Event::Ime(_))));
            // Enter may close a DragValue editor before this canvas pass.
            // The same key still belongs to that field, not a second newline.
            if !canvas_press&&(ctx.text_edit_focused()&&ctx.memory(|m|m.focused().is_some_and(|id|id!=inline.id))||(text_event||command_pointer)&&keyboard_target.is_some_and(|id|id!=inline.id)){
                record_widget!(widgets,"canvas_text".into(),json!(editor.id));return Ok(());
            }
            let doc=&snapshot.document;let object=doc.draft_text(&editor.id,&editor.geometry)?;let o=&object;let world=doc.world_transform(o)?;
            let layout=inline.cache.get(&snapshot.document_hash,doc,o)?;
            // Popup/toolbar pointer focus must not collapse the canvas range.
            // Actual key, text, clipboard and IME events still belong to TextEdit.
            let keep_command_range=command_pointer&&!text_event&&!inline.ime_active&&!inline.select_all;
            let command_range=inline.range;
            // A rejected toolbar/page click can briefly move egui focus and
            // collapse its marked-text range. Retain the actual TextEdit
            // composition state so the next OS commit replaces that range,
            // rather than appending Chinese after the visible pinyin.
            let mut state=inline.composition_state.clone().filter(|_|inline.ime_active).unwrap_or_else(||egui::text_edit::TextEditState::load(&ctx,inline.id).unwrap_or_default());
            if keep_command_range{state.cursor.set_char_range(Some(command_range));}
            if inline.select_all{if let Geometry::Text{content,..}=&editor.geometry{state.cursor.set_char_range(Some(CCursorRange::select_all_str(content)));}inline.select_all=false;}
            let press=ctx.input(|i|i.pointer.primary_pressed());let down=ctx.input(|i|i.pointer.primary_down());
            let pos=inline.initial.take().or_else(||ctx.input(|i|i.pointer.interact_pos()).filter(|pos|!inline.ime_active&&(down&&inline.drag_anchor.is_some()||press&&layout.contains(*pos,world,view))));
            if let Some(pos)=pos {
                let cursor=layout.nearest(pos,world,view);
                if press||inline.drag_anchor.is_none(){inline.drag_anchor=Some(if ctx.input(|i|i.modifiers.shift){state.cursor.char_range().map_or(cursor,|r|r.secondary)}else{cursor});}
                state.cursor.set_char_range(Some(CCursorRange::two(inline.drag_anchor.unwrap(),cursor)));
            }
            if !down{inline.drag_anchor=None;}
            state.store(&ctx,inline.id);focus_text_once(&ctx,inline.id);
            let ime_update=ctx.input(|i|i.events.iter().any(|e|matches!(e,egui::Event::Ime(egui::ImeEvent::Preedit{..}|egui::ImeEvent::Commit(_)))));
            for event in ctx.input(|i|i.events.clone()){match event {egui::Event::Ime(egui::ImeEvent::Preedit{text,..})=>inline.ime_active=!text.is_empty(),egui::Event::Ime(egui::ImeEvent::Commit(_))=>inline.ime_active=false,_=>{}}}
            let mut child=ui.new_child(egui::UiBuilder::new().id_salt("canvas-keyboard").max_rect(Rect::from_min_size(Pos2::new(-100000.,-100000.),Vec2::new(100000.,100000.))));child.set_clip_rect(Rect::NOTHING);
            let was_dirty=(editor.dirty,editor.text_dirty);
            let Geometry::Text{content,..}=&mut editor.geometry else{unreachable!()};
            let original_content=content.clone();let original_range=state_range(&ctx,inline.id);
            let mut object=o.clone();let mut layouter=|ui:&egui::Ui,text:&dyn egui::TextBuffer,_width:f32|{
                if let Geometry::Text{content,..}=&mut object.geometry{*content=text.as_str().into();}
                inline.cache.keyboard_galley(ui,&snapshot.document_hash,doc,&object,view.scale).unwrap_or_else(|_|ui.fonts_mut(|f|f.layout_no_wrap(text.as_str().into(),egui::FontId::proportional(14.),Color32::TRANSPARENT)))
            };
            let output=egui::TextEdit::multiline(content).id(inline.id).frame(egui::Frame::NONE).desired_width(99999.).layouter(&mut layouter).show(&mut child);
            if inline.ime_active&&!ime_update{
                *content=original_content.clone();
                if let Some(state)=&inline.composition_state{inline.range=state.cursor.char_range().unwrap_or_default();state.clone().store(&ctx,inline.id);}
            }else if keep_command_range&&!output.response.changed(){
                let mut kept=output.state.clone();kept.cursor.set_char_range(Some(command_range));kept.store(&ctx,inline.id);
                inline.range=command_range;
            }else{
                if output.response.changed(){editor.dirty=true;editor.text_dirty=true;}
                // A canvas click can briefly take focus from the hidden keyboard
                // widget on release. Its stored selection still belongs to this draft.
                inline.range = output.cursor_range.or_else(|| output.state.cursor.char_range())
                    .unwrap_or(inline.range);
                inline.composition_state=inline.ime_active.then(||output.state.clone());
            }
            if output.response.changed() && let Err(error)=snapshot.document.draft_text(&editor.id,&editor.geometry){
                if let Geometry::Text{content,..}=&mut editor.geometry{*content=original_content;}
                (editor.dirty,editor.text_dirty)=was_dirty;
                let mut state=egui::text_edit::TextEditState::load(&ctx,inline.id).unwrap_or_default();state.cursor.set_char_range(original_range);state.store(&ctx,inline.id);
                inline.range=original_range.unwrap_or_default();return Err(error);
            }
            record_widget!(widgets,"canvas_text".into(),json!(editor.id));
            Ok(())
        })();
        if let Err(error)=result{self.error=true;self.message=error.to_string();}
        self.inline_text=Some(inline);
    }
    pub(super) fn paint_text_edit(&mut self,painter:&egui::Painter,source_key:&str,doc:&Document,view:View,widgets:&mut BTreeMap<String,Value>) {
        let (Some(inline),Some(editor))=(&mut self.inline_text,&self.editor)else{return;};
        let Ok(o)=doc.object(&editor.id)else{return;};let (Ok(layout),Ok(world))=(inline.cache.get(source_key,doc,o),doc.world_transform(o))else{return;};
        let range=inline.range.as_sorted_char_range();
        for row in &layout.rows {
            let start = range.start.0.saturating_sub(row.start);
            let end = range.end.0.saturating_sub(row.start).min(row.chars.len());
            for column in start..end {
                let [a,b]=layout.caret(row,column).map(|p|view.screen(world*p));let [c,d]=layout.caret(row,column+1).map(|p|view.screen(world*p));
                painter.add(egui::Shape::convex_polygon(vec![a,c,d,b],Color32::from_rgba_unmultiplied(44,96,210,65),egui::Stroke::NONE));
            }
        }
        let [a,b]=layout.position(inline.range.primary).map(|p|view.screen(world*p));
        painter.line_segment([a,b],egui::Stroke::new(3.,Color32::WHITE));painter.line_segment([a,b],egui::Stroke::new(1.3,ACCENT));
        if inline.ime_active{painter.line_segment([b,b+Vec2::new(12.,0.)],egui::Stroke::new(1.5,ACCENT));}
        let caret=Rect::from_two_pos(a,b).expand(1.);record_widget!(widgets,"text_caret".into(),rect(caret));
        record_widget!(widgets,"text_range".into(),json!([inline.range.primary.index.0,inline.range.secondary.index.0]));
        #[cfg(feature="test-hooks")]
        record_widget!(widgets,"text_carets".into(),json!(layout.rows.iter().flat_map(|row|(0..row.x.len()).map(|column|{let [a,b]=layout.caret(row,column).map(|p|view.screen(world*p));json!({"index":row.start+column,"a":[a.x,a.y],"b":[b.x,b.y]})})).collect::<Vec<_>>()));
        let anchor=caret.intersect(view.canvas);if anchor.is_positive()&&!self.quit_prompt&&self.dialog.is_none()&&painter.ctx().memory(|m|m.has_focus(inline.id)){painter.ctx().output_mut(|o|o.ime=Some(egui::output::IMEOutput{purpose:egui::IMEPurpose::Normal,rect:view.canvas,cursor_rect:anchor,should_interrupt_composition:false}));}
        if view.canvas.contains(painter.ctx().input(|i|i.pointer.hover_pos()).unwrap_or(Pos2::new(-1.,-1.))){painter.ctx().set_cursor_icon(egui::CursorIcon::Text);}
    }
    pub(super) fn restore_text_focus(&mut self,ctx:&egui::Context){
        if self.inline_text.is_none()&&self.text_field.is_some(){self.focus_text=true;}
        if let Some(id)=self.inline_text.as_ref().map(|t|t.id).or_else(||self.text_field.as_ref().map(|(id,_)|*id)){focus_text_once(ctx,id);}
    }
}
fn state_range(ctx:&egui::Context,id:egui::Id)->Option<CCursorRange>{egui::text_edit::TextEditState::load(ctx,id).and_then(|s|s.cursor.char_range())}
#[cfg(test)]
fn project(source:&Document,editor:&Editor)->arranger_core::Result<Document> {
    let mut doc=source.clone();let object=doc.draft_text(&editor.id,&editor.geometry)?;
    *doc.objects.iter_mut().find(|o|o.id==editor.id).unwrap()=object;Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arranger_core::TextAlign;
    fn fixture(content:&str,area:bool)->Document {
        let mut doc=Document::new("text-edit".into(),210.,297.).unwrap();
        doc.objects.push(Object{id:"text".into(),page_id:"page-1".into(),parent_id:None,x_mm:25.123456789,y_mm:30.,width_mm:12.,height_mm:40.,transform:[1.,0.2,0.1,1.,0.,0.],style:Default::default(),visible:true,locked:false,clip:None,
            geometry:Geometry::Text{content:content.into(),font_family:"sans-serif".into(),font_size_mm:4.,text_path:None,layout:arranger_core::TextLayoutOptions{mode:if area{TextMode::Area}else{TextMode::Point},align:TextAlign::Center,..Default::default()}}});doc.validate().unwrap();doc
    }
    #[test]
    fn area_frame_whitespace_is_editable_and_still_respects_clips(){
        let mut doc=fixture("A",true);let o=&doc.objects[0];let p=doc.world_transform(o).unwrap()*Point::new(11.,30.);
        assert_eq!(hit(&doc,"page-1",p,0.0001),Some("text".into()));
        doc.objects[0].clip=Some(arranger_core::Clip::Rect{rect_mm:[0.,0.,4.,4.]});assert_eq!(hit(&doc,"page-1",p,0.0001),None);
    }
    #[test]
    fn row_whitespace_respects_stacking_clips_and_each_lines_width() {
        let mut doc = fixture("A       B\nI", false);
        let story = &doc.objects[0];
        let layout = arranger_core::text::layout(&doc, story).unwrap();
        let [x, y, w, h] = layout.lines[0].ink_bounds_local_mm.unwrap();
        let [_, y2, _, h2] = layout.lines[1].ink_bounds_local_mm.unwrap();
        let world = doc.world_transform(story).unwrap();
        let gap = world * Point::new(x+w/2., y+h/2.);
        let beside_short_row = world * Point::new(x+w*0.9, y2+h2/2.);
        let mut panel = story.clone();
        panel.id = "panel".into();
        panel.width_mm = 80.;
        panel.geometry = Geometry::Rect { radius_mm: 0. };
        doc.objects.insert(0, panel.clone());
        let pick = |doc: &Document, p: Point| hit(doc, "page-1", p, 0.001);
        assert_eq!(doc.hit_test("page-1", [gap.x,gap.y], 0.001).as_deref(), Some("panel"));
        assert_eq!(pick(&doc, gap).as_deref(), Some("text"));
        assert_eq!(pick(&doc, beside_short_row), None);
        doc.objects[1].visible = false;
        assert_eq!(pick(&doc, gap), None);
        doc.objects[1].visible = true;
        doc.objects[1].clip = Some(arranger_core::Clip::Rect { rect_mm: [0.,0.,1.,1.] });
        assert_eq!(pick(&doc, gap), None);
        doc.objects[1].clip = None;
        panel.id = "cover".into();
        doc.objects.push(panel);
        assert_eq!(pick(&doc, gap), None);
    }

    #[test]
    fn live_projection_preserves_original_model_ids_transforms_and_submillimetre_precision(){
        let doc=fixture("Original",false);let original=doc.hash();let mut draft=Editor::new(&doc,&doc.objects[0],7);
        if let Geometry::Text{content,..}=&mut draft.geometry{*content="Changed\n中文".into();}
        let projected=project(&doc,&draft).unwrap();assert_ne!(projected.hash(),original);assert_eq!(doc.hash(),original);
        let mut expected=doc.objects[0].clone();expected.geometry=draft.geometry;assert_eq!(projected.objects[0],expected);
        assert_eq!(projected.objects[0].x_mm.to_bits(),doc.objects[0].x_mm.to_bits());
    }
    #[test]
    fn source_character_indices_survive_unicode_crlf_empty_lines_and_soft_wrap(){
        for (content,area) in [("中文 A\r\n\nB",false),("中文 English 自动换行 末尾",true),("",false)] {
            let doc=fixture(content,area);let layout=LayoutCache::default().get(&doc.hash(),&doc,&doc.objects[0]).unwrap();let mut index=0;
            for row in &layout.rows{assert_eq!(row.start,index);assert_eq!(row.x.len(),row.chars.len()+1);index+=row.chars.len()+usize::from(row.hard);}
            assert_eq!(index,content.chars().count());
            let ctx=egui::Context::default();let _=ctx.run_ui(Default::default(),|ui|{
                let galley=layout.galley(ui,content,2.);assert_eq!(galley.end().index.0,content.chars().count());
                for index in 0..=content.chars().count(){let points=layout.position(CCursor::new(index));assert!(points.iter().all(|p|p.x.is_finite()&&p.y.is_finite()));}
            });
        }
    }
    #[test]
    fn long_row_hit_testing_matches_the_first_closest_screen_segment() {
        let make_row = |start, top| EditRow {
            start,
            chars: vec!['a'; 128],
            x: (0..=128).map(|i| (i / 2) as f64 * 0.125).collect(),
            ordered: true,
            top,
            hard: false,
        };
        let mut layout = EditLayout {
            rows: vec![make_row(0, 1.), make_row(128, 8.)],
            size: 4., height: 7., path: None,
        };
        let view = View { origin: Pos2::new(20., 42.), scale: 2., ..Default::default() };
        let positions: Vec<_> = [-100_000., 44., 55., 59., 100_000.].into_iter()
            .flat_map(|y| (-4..=260).map(move |i| Pos2::new(20.+i as f32/16., y)))
            .collect();
        for world in [Affine::IDENTITY, Affine::scale_non_uniform(0.055, -2.),
                      Affine::new([1., 0.25, 0.15, 1., -15., 2.])] {
            let hit = |layout: &EditLayout| positions.iter().map(|p| {
                let c = layout.nearest(*p, world, view);
                (c.index.0, c.prefer_next_row)
            }).collect::<Vec<_>>();
            for row in &mut layout.rows { row.ordered = false; }
            let expected = hit(&layout);
            for row in &mut layout.rows { row.ordered = true; }
            assert_eq!(hit(&layout), expected);
        }
    }

    #[test]
    fn canvas_keyboard_focus_never_interrupts_an_ongoing_platform_ime_composition(){
        let ctx=egui::Context::default();let id=egui::Id::unique("ime-regression");let mut content=String::new();
        for frame in 0..12 {
            let events=match frame{2=>vec![egui::Event::Ime(egui::ImeEvent::Preedit{text:"zhongwen".into(),active_range_chars:None})],8=>vec![egui::Event::Ime(egui::ImeEvent::Commit("中文".into()))],_=>vec![]};
            let output=ctx.run_ui(egui::RawInput{focused:true,events,..Default::default()},|ui|{
                focus_text_once(ui.ctx(),id);
                let mut child=ui.new_child(egui::UiBuilder::new().id_salt("canvas-keyboard").max_rect(Rect::from_min_size(Pos2::new(-100000.,-100000.),Vec2::splat(100000.))));child.set_clip_rect(Rect::NOTHING);
                egui::TextEdit::multiline(&mut content).id(id).show(&mut child);
                ui.output_mut(|o|o.ime=Some(egui::output::IMEOutput{purpose:egui::IMEPurpose::Normal,rect:Rect::from_min_size(Pos2::ZERO,Vec2::new(600.,600.)),cursor_rect:Rect::from_min_size(Pos2::new(40.,60.),Vec2::new(1.,20.)),should_interrupt_composition:false}));
            });
            if frame>0{assert!(!output.platform_output.ime.unwrap().should_interrupt_composition,"IME interrupted on frame {frame}");}
            if (2..8).contains(&frame){assert_eq!(content,"zhongwen");}if frame>=8{assert_eq!(content,"中文");}
        }
    }
}
