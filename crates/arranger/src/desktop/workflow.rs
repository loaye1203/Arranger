//! Context-aware edit commands and suggested paths for unsaved imports.
use super::*;

pub(super) struct CommandFocus {id:egui::Id,document:String,revision:u64,editor:Option<String>}
pub(super) struct TextCommand {id:egui::Id,document:String,revision:u64,editor:Option<String>,action:String}
pub(super) struct DocumentPath {path:String,revision:u64,hash:String}
impl App {
    pub(super) fn observe_command_focus(&mut self,ctx:&egui::Context,s:&Snapshot){
        if self.dialog.is_some()||self.quit_prompt{self.command_focus=None;return;}
        if ctx.text_edit_focused(){
            self.command_focus=ctx.memory(|m|m.focused()).map(|id|CommandFocus{id,document:self.active.clone(),revision:s.revision,editor:self.editor.as_ref().map(|e|e.id.clone())});
        }else if !egui::Popup::is_any_open(ctx)&&!ctx.input(|i|i.pointer.interact_pos()).is_some_and(|p|self.command_bar.contains(p)){
            self.command_focus=None;
        }
        if self.command_focus.as_ref().is_some_and(|f|f.document!=self.active||f.revision!=s.revision||f.editor!=self.editor.as_ref().map(|e|e.id.clone())){self.command_focus=None;}
    }
    pub(super) fn text_command_target(&self,ctx:&egui::Context)->Option<egui::Id>{
        if self.dialog.is_some()||self.quit_prompt{return None;}
        ctx.text_edit_focused().then(||ctx.memory(|m|m.focused())).flatten()
            // An in-window menu temporarily takes focus from a toolbar field.
            // Its remembered target must win over the still-open canvas draft.
            .or_else(||self.command_focus.as_ref().filter(|f|f.document==self.active&&f.editor==self.editor.as_ref().map(|e|e.id.clone())&&self.service.snapshot(&self.active).is_ok_and(|s|s.revision==f.revision)).map(|f|f.id))
            .or_else(||self.inline_text.as_ref().map(text_edit::InlineText::widget_id))
    }
    pub(super) fn text_command(&mut self,ctx:&egui::Context,action:&str)->bool{
        if !matches!(action,"copy"|"cut"|"paste"|"all"|"undo"|"redo"){return false;}
        let Some(id)=self.text_command_target(ctx)else{return false;};
        if action!="copy"&&!self.text_commit_ready(){return true;}
        text_edit::focus_text_once(ctx,id);
        if let Ok(s)=self.service.snapshot(&self.active){if s.busy{return true;}self.text_commands.push(TextCommand{id,document:self.active.clone(),revision:s.revision,editor:self.editor.as_ref().map(|e|e.id.clone()),action:action.into()});ctx.request_repaint();}
        true
    }
    pub(super) fn process_text_commands(&mut self,ctx:&egui::Context){
        #[cfg(feature="test-hooks")]
        if self.test_hold_text_commands{return;}
        for command in std::mem::take(&mut self.text_commands){
            if self.dialog.is_some()||self.quit_prompt||command.document!=self.active||command.editor!=self.editor.as_ref().map(|e|e.id.clone())||self.text_command_target(ctx)!=Some(command.id)||!self.service.snapshot(&self.active).is_ok_and(|s|!s.busy&&s.revision==command.revision){continue;}
            text_edit::focus_text_once(ctx,command.id);
            match command.action.as_str() {
            "copy"|"cut"=>ctx.input_mut(|i|i.events.push(if command.action=="copy"{egui::Event::Copy}else{egui::Event::Cut})),
            "paste"=>{#[cfg(feature="test-hooks")] {self.test_paste_requested=true;}ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);},
            _=>{
                let modifiers=egui::Modifiers{command:true,mac_cmd:cfg!(target_os="macos"),ctrl:!cfg!(target_os="macos"),shift:command.action=="redo",..Default::default()};
                ctx.input_mut(|i|for pressed in [true,false]{i.events.push(egui::Event::Key{key:if command.action=="all"{egui::Key::A}else{egui::Key::Z},physical_key:None,pressed,repeat:false,modifiers});});
            }
        }}
    }
    pub(super) fn remember_document_path(&mut self){
        self.viewport_cache=None;
        self.object_rows=Default::default();self.node_queries.get_mut().clear();
        if let Ok(s)=self.service.snapshot(&self.active){self.document_paths.insert(self.active.clone(),DocumentPath{path:self.path.clone(),revision:s.revision,hash:s.document_hash});}
        if let Ok(docs)=self.service.command(Command::Documents){
            if let Some(docs)=docs["documents"].as_array(){self.document_paths.retain(|id,_|docs.iter().any(|d|d["document_id"]==*id));}
        }
    }
    pub(super) fn known_document_path(&self,id:&str)->Option<&str>{
        if id==self.active{Some(&self.path)}else{self.document_paths.get(id).map(|p|p.path.as_str())}
    }
    pub(super) fn suggested_document_path(&self,s:&Snapshot)->String{
        if let Some(path)=&s.saved_path{return readable_path(path);}
        self.document_paths.get(&s.document_id).filter(|p|s.revision>p.revision||s.revision==p.revision&&s.document_hash==p.hash)
            .map(|p|p.path.clone()).unwrap_or_else(||format!("{}.arranger",s.document_id))
    }
}
