//! Close one document or walk all documents on quit. Saving is asynchronous;
//! a successful file write alone never authorizes closing a changed revision.
use super::*;

#[derive(Clone)]
struct Checkpoint {document:String,revision:u64,hash:String}
impl Checkpoint {
    fn new(s:&Snapshot)->Self{Self{document:s.document_id.clone(),revision:s.revision,hash:s.document_hash.clone()}}
    fn matches(&self,s:&Snapshot)->bool{self.document==s.document_id&&self.revision==s.revision&&self.hash==s.document_hash}
}
pub(super) struct CloseFocus {id:egui::Id,document:String,editor:Option<String>}
pub(super) struct CloseFlow {quit:bool,current:Checkpoint,saving:Option<Checkpoint>,focus:Option<egui::Id>,focus_primary:bool}
impl CloseFlow {
    #[cfg(feature="test-hooks")]
    pub(super) fn status(&self)->Value{json!({"quit":self.quit,"document_id":self.current.document,"revision":self.current.revision,"saving":self.saving.is_some(),"focus_saved":self.focus.is_some()})}
}
impl App {
    pub(super) fn close_document_tab(&mut self,ctx:&egui::Context,id:&str){
        if self.allow_quit||self.dialog.is_some()||self.close_flow.is_some(){return;}
        if id!=self.active{
            let Ok(s)=self.service.snapshot(id)else{return;};
            // A clean background tab can close without disturbing the active
            // canvas, its selection, input focus or an uncommitted draft.
            if !s.dirty&&!s.busy{
                let focus=self.text_command_target(ctx).or_else(||ctx.memory(|m|m.focused()));
                if self.act(Command::DocumentClose{document_id:id.into(),expected_revision:s.revision,discard_changes:false},"Document closed").is_some(){self.forget_closed_document(id);self.close_focus=focus.map(|id|CloseFocus{id,document:self.active.clone(),editor:self.editor.as_ref().map(|e|e.id.clone())});}
                return;
            }
            // Saving a dirty tab uses the existing active-document save flow.
            // Navigation first protects the current IME and property drafts.
            if !self.activate_document(id){return;}
        }
        self.request_close(ctx,false);
    }
    fn close_has_draft(&self)->bool{self.editor.as_ref().is_some_and(|e|e.dirty||e.text_dirty)||self.page_draft_dirty||!self.pen.is_empty()||self.gesture.is_some()||self.inline_text.as_ref().is_some_and(|t|t.composing())}
    pub(super) fn request_close(&mut self,ctx:&egui::Context,quit:bool){
        if self.allow_quit||self.dialog.is_some()||self.close_flow.is_some(){return;}
        let Ok(s)=self.service.snapshot(&self.active)else{return;};
        self.close_flow=Some(CloseFlow{quit,current:Checkpoint::new(&s),saving:None,focus:self.text_command_target(ctx).or_else(||ctx.memory(|m|m.focused())),focus_primary:true});
        self.text_commands.clear();self.error=false;
        self.continue_closing(ctx);
    }
    fn continue_closing(&mut self,ctx:&egui::Context){
        loop {
            let Some(flow)=&self.close_flow else{return;};
            let Ok(s)=self.service.snapshot(&flow.current.document)else{if self.advance_closed_document(ctx){continue;}return;};
            if s.dirty||s.busy||self.close_has_draft(){
                self.quit_prompt=true;ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));return;
            }
            if !self.remove_close_document(&s,false){return;}
            if !self.advance_closed_document(ctx){return;}
        }
    }
    fn cancel_close(&mut self,ctx:&egui::Context){
        let flow=self.close_flow.take();self.quit_prompt=false;
        self.restore_text_focus(ctx);
        self.close_focus=flow.and_then(|f|f.focus).map(|id|CloseFocus{id,document:self.active.clone(),editor:self.editor.as_ref().map(|e|e.id.clone())});
        ctx.request_repaint();
    }
    pub(super) fn restore_close_focus(&mut self,ctx:&egui::Context){
        if self.close_focus.is_none(){return;}
        // egui retains the modal layer for the next pass. A focus request
        // while that layer still blocks the inspector is silently lost.
        if ctx.memory(|m|m.top_modal_layer().is_some()){ctx.request_repaint();return;}
        let focus=self.close_focus.take().unwrap();
        if !self.quit_prompt&&self.dialog.is_none()&&focus.document==self.active&&focus.editor==self.editor.as_ref().map(|e|e.id.clone())&&!ctx.input(|i|i.pointer.any_pressed()){
            text_edit::focus_text_once(ctx,focus.id);
        }
    }
    fn current_close_snapshot(&mut self)->Option<Snapshot>{
        let checkpoint=self.close_flow.as_ref()?.current.clone();
        let s=self.service.snapshot(&checkpoint.document).ok()?;
        if !checkpoint.matches(&s){
            if let Some(flow)=&mut self.close_flow{flow.current=Checkpoint::new(&s);}
            self.error=true;self.message=tr(self.chinese,"文档已更新，请核对后再次确认关闭","The document changed; review it and confirm closing again").into();return None;
        }
        Some(s)
    }
    fn remove_close_document(&mut self,s:&Snapshot,discard:bool)->bool{
        if self.act(Command::DocumentClose{document_id:s.document_id.clone(),expected_revision:s.revision,discard_changes:discard},"Document closed").is_none(){self.quit_prompt=true;return false;}
        true
    }
    // Returns true only when Quit has another document to process. Documents
    // already explicitly saved/discarded remain closed if a later one cancels.
    fn advance_closed_document(&mut self,ctx:&egui::Context)->bool{
        let Some(flow)=self.close_flow.take()else{return false;};
        self.select_after_close(ctx,flow.quit,&flow.current.document)
    }
    fn select_after_close(&mut self,ctx:&egui::Context,quit:bool,closed:&str)->bool{
        self.forget_closed_document(closed);
        self.quit_prompt=false;self.text_commands.clear();
        let docs=self.service.command(Command::Documents).unwrap_or_default();
        let next=docs["documents"].as_array().and_then(|ds|ds.first()).and_then(|d|d["document_id"].as_str()).map(str::to_owned);
        if let Some(id)=next{
            self.load_document(&id);self.tool=Tool::Select;
            if quit{if let Ok(s)=self.service.snapshot(&id){self.close_flow=Some(CloseFlow{quit:true,current:Checkpoint::new(&s),saving:None,focus:None,focus_primary:true});return true;}}
        }else if quit{
            self.allow_quit=true;self.pending_files.clear();ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }else{
            let id=loop{let id=format!("untitled-{}",self.next_document);self.next_document+=1;if self.service.snapshot(&id).is_err(){break id;}};
            if self.act(Command::DocumentNew{document_id:id.clone(),width_mm:210.,height_mm:297.,unit:"mm".into()},"New document").is_some(){self.load_document(&id);self.path=format!("未命名-{}.arranger",self.next_document-1);self.tool=Tool::Select;}
        }
        ctx.memory_mut(|m|m.stop_text_input());ctx.request_repaint();false
    }
    fn forget_closed_document(&mut self,id:&str){
        self.recents.forget_document(id);self.document_paths.remove(id);self.opened_cdrs.retain(|_,known|known!=id);self.document_order.retain(|known|known!=id);
    }
    pub(super) fn reconcile_closed_document(&mut self,ctx:&egui::Context){
        if self.allow_quit||self.service.snapshot(&self.active).is_ok(){return;}
        // The external API can close a session independently of the UI. Never
        // leave its old save sheet targeting whichever document survives.
        #[cfg(target_os="macos")] self.native_files.cancel();
        self.dialog=None;self.close_focus=None;
        let next=if self.close_flow.is_some(){self.advance_closed_document(ctx)}else{let closed=self.active.clone();self.select_after_close(ctx,false,&closed)};
        if next{self.continue_closing(ctx);}
    }
    fn discard_close(&mut self,ctx:&egui::Context){
        let Some(s)=self.current_close_snapshot()else{return;};
        if self.remove_close_document(&s,true)&&self.advance_closed_document(ctx){self.continue_closing(ctx);}
    }
    fn save_close(&mut self,ctx:&egui::Context){
        let Some(s)=self.current_close_snapshot()else{return;};
        if self.close_save_blocker(&s).is_some(){return;}
        self.quit_prompt=false;
        self.file_action(&s,"save");
        if let Some(flow)=&mut self.close_flow{if let Ok(s)=self.service.snapshot(&self.active){flow.current=Checkpoint::new(&s);flow.saving=Some(Checkpoint::new(&s));}}
        if self.dialog.is_none(){
            if self.error{self.quit_prompt=true;if let Some(flow)=&mut self.close_flow{flow.saving=None;}}
            else{self.finish_close_save(ctx,true);}
        }
    }
    pub(super) fn finish_close_save(&mut self,ctx:&egui::Context,saved:bool){
        let Some(expected)=self.close_flow.as_mut().and_then(|f|f.saving.take())else{return;};
        if !saved{self.cancel_close(ctx);return;}
        let Ok(s)=self.service.snapshot(&expected.document)else{self.cancel_close(ctx);return;};
        if !expected.matches(&s)||s.dirty||s.busy{
            self.quit_prompt=true;if let Some(flow)=&mut self.close_flow{flow.current=Checkpoint::new(&s);}
            self.error=true;self.message=tr(self.chinese,"保存期间文档有更新，已保留打开；请核对后再次关闭","Document changed while saving; it remains open for review").into();return;
        }
        if self.remove_close_document(&s,false)&&self.advance_closed_document(ctx){self.continue_closing(ctx);}
    }
    fn close_save_blocker(&self,s:&Snapshot)->Option<&'static str>{
        let zh=self.chinese;
        if s.busy{return Some(tr(zh,"外部任务进行中，请完成或取消任务后再关闭。","An external task is active; finish or cancel it before closing."));}
        if self.inline_text.as_ref().is_some_and(|t|t.composing()){return Some(tr(zh,"请先取消关闭并完成输入法选词；也可明确不保存。","Cancel closing to finish composing, or explicitly close without saving."));}
        if self.page_draft_dirty{return Some(tr(zh,"请先取消关闭，应用或恢复页面尺寸后再保存。","Cancel closing, then apply or reload the page size before saving."));}
        if self.gesture.is_some(){return Some(tr(zh,"请先取消关闭并完成拖动。","Cancel closing and finish dragging first."));}
        None
    }
    pub(super) fn show_close_dialog(&mut self,ctx:&egui::Context,widgets:&mut BTreeMap<String,Value>){
        if !self.quit_prompt{return;}
        let Some(flow)=&self.close_flow else{return;};let quit=flow.quit;let focus_primary=flow.focus_primary;
        let Ok(s)=self.service.snapshot(&flow.current.document)else{self.cancel_close(ctx);return;};
        let zh=self.chinese;let location=s.saved_path.as_deref().or_else(||self.known_document_path(&s.document_id)).map(str::to_owned);let name=title(location.as_deref(),&s.document_id);
        let reason=self.close_save_blocker(&s);let mut save=false;let mut discard=false;let mut cancel=false;
        let fresh_enter=files::primary_enter(ctx);
        // A held confirmation key must not close the next document during Quit.
        // Keep keyboard activation of a focused Cancel/Discard in that button.
        let primary_key=fresh_enter&&(ctx.text_edit_focused()||ctx.memory(|m|m.focused().is_none()));
        #[cfg(target_os="macos")]
        let discard_key=ctx.input(|i|i.events.iter().any(|e|matches!(e,egui::Event::Key{key:egui::Key::D,pressed:true,repeat:false,modifiers,..}if modifiers.command&&!modifiers.ctrl&&!modifiers.shift&&!modifiers.alt)));
        #[cfg(not(target_os="macos"))]
        let discard_key=false;
        let response=egui::Modal::new(egui::Id::unique("unsaved-close")).frame(files::modal_frame(ctx)).show(ctx,|ui|{
            ui.set_width((ctx.content_rect().width()-72.).clamp(280.,520.));ui.spacing_mut().item_spacing=Vec2::new(8.,10.);
            files::modal_heading(ui,if quit{tr(zh,"退出前保存？","Save before quitting?")}else{tr(zh,"关闭前保存？","Save before closing?")});
            egui::ScrollArea::vertical().id_salt("unsaved-close-body").max_height((ctx.content_rect().height()-190.).max(160.)).show(ui,|ui|{
                let r=ui.add(egui::Label::new(egui::RichText::new(&name).strong().color(INK)).truncate()).on_hover_text(location.as_deref().unwrap_or(&name));record_widget!(widgets,"quit_name".into(),rect(r.rect));
                if quit{ui.label(egui::RichText::new(tr(zh,"逐份处理；取消后保留尚未关闭的文档。","One document at a time; Cancel keeps those still open.")).small().color(MUTED));}
                if let Some(reason)=reason{ui.label(reason);}
                if self.error{ui.colored_label(Color32::from_rgb(182,50,57),self.ui_message());if self.file_problem.as_ref().is_some_and(|p|p.raw==self.message&&p.can_copy()){ui.label(tr(zh,"请取消关闭后另存副本，保留当前编辑。","Cancel closing, then save a copy to keep your edits."));}}
            });
            ui.add_space(8.);ui.separator();
            ui.allocate_ui_with_layout(Vec2::new(ui.available_width(),32.),egui::Layout::right_to_left(egui::Align::Center),|ui|{
                let enabled=reason.is_none();record_widget!(widgets,"quit_save_enabled".into(),json!(enabled));record_widget!(widgets,"quit_discard_enabled".into(),json!(!s.busy));
                let r=ui.add_enabled(enabled,egui::Button::new(egui::RichText::new(tr(zh,"保存","Save")).strong().color(Color32::WHITE)).fill(if enabled{ACCENT}else{Color32::from_gray(185)}).min_size(Vec2::new(96.,32.)));record_widget!(widgets,"quit_save".into(),rect(r.rect));if focus_primary{r.request_focus();}save=enabled&&(files::button_activated(ctx,&r)||primary_key);
                let r=ui.add_sized([76.,32.],egui::Button::new(tr(zh,"取消","Cancel")));record_widget!(widgets,"quit_continue".into(),rect(r.rect));if r.has_focus(){record_widget!(widgets,"quit_focused_action".into(),json!("cancel"));}cancel=r.clicked();
                let r=ui.add_enabled(!s.busy,egui::Button::new(tr(zh,"不保存","Don't save")).min_size(Vec2::new(88.,32.))).on_hover_text(if cfg!(target_os="macos"){tr(zh,"不保存并关闭（⌘D）","Close without saving (⌘D)")}else{tr(zh,"不保存并关闭","Close without saving")});record_widget!(widgets,"quit_discard".into(),rect(r.rect));discard=!s.busy&&(files::button_activated(ctx,&r)||discard_key);
            });
        });record_widget!(widgets,"quit_dialog".into(),rect(response.response.rect));if let Some(flow)=&mut self.close_flow{flow.focus_primary=false;}
        if cancel||response.should_close(){self.cancel_close(ctx);}else if discard{self.discard_close(ctx);}else if save{self.save_close(ctx);}
    }
}
