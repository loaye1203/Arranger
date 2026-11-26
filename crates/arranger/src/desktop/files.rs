use super::*;
pub(super) fn modal_frame(ctx:&egui::Context)->egui::Frame{egui::Frame::popup(&ctx.style_of(egui::Theme::Light)).fill(Color32::WHITE).inner_margin(20).corner_radius(10)}
pub(super) fn modal_heading(ui:&mut egui::Ui,title:&str){ui.horizontal(|ui|{let(_,r)=ui.allocate_space(Vec2::new(4.,22.));ui.painter().rect_filled(r,1.,ACCENT);ui.label(egui::RichText::new(title).size(21.).strong().color(INK));});ui.add_space(6.);}
pub(super) fn primary_enter(ctx:&egui::Context)->bool{ctx.input(|i|i.events.iter().any(|e|matches!(e,egui::Event::Key{key:egui::Key::Enter,pressed:true,repeat:false,modifiers,..}if modifiers.is_none())))}
pub(super) fn button_activated(ctx:&egui::Context,response:&egui::Response)->bool{
    let(keyboard,fresh)=ctx.input(|i|(i.key_pressed(egui::Key::Enter)||i.key_pressed(egui::Key::Space),i.events.iter().any(|e|matches!(e,egui::Event::Key{key:egui::Key::Enter|egui::Key::Space,pressed:true,repeat:false,modifiers,..}if modifiers.is_none()))));
    response.clicked()&&(!keyboard||fresh)
}
pub(super) fn is_source_backup(path:&std::path::Path)->bool{path.file_name().is_some_and(|n|n.to_string_lossy().to_ascii_lowercase().ends_with(".arranger.bak"))}
pub(super) fn recovery_copy_path(path:&std::path::Path)->PathBuf{let source=if is_source_backup(path){path.with_extension("")}else{path.to_owned()};source.with_file_name(format!("{}-恢复.arranger",source.file_stem().unwrap_or_default().to_string_lossy()))}


#[derive(Clone,Debug)]
pub(super) enum FileKind { Open, Save, Export(String), Image }
impl FileKind {
    pub(super) fn extension(&self)->Option<&str>{match self{Self::Save=>Some("arranger"),Self::Export(f)=>Some(match f.as_str(){"cdr14"|"cdr21"=>"cdr","print"=>"pdf",_=>f}),_=>None}}
}
struct FileResume {document:String,revision:u64,selection:Vec<String>,inline:Option<text_edit::InlineText>,text_field:Option<(egui::Id,Rect)>,tool:Tool}
impl FileResume {
    fn take(app:&mut App,revision:u64)->Self{
        let this=Self{document:app.active.clone(),revision,selection:app.selection.clone(),inline:app.inline_text.take(),text_field:app.text_field.take(),tool:app.tool};
        if this.inline.is_some(){app.tool=Tool::Select;}this
    }
    fn restore(self,app:&mut App,ctx:Option<&egui::Context>){
        if app.active!=self.document||app.selection!=self.selection||!app.service.snapshot(&app.active).is_ok_and(|s|s.revision==self.revision){return;}
        if self.inline.is_some(){app.inline_text=self.inline;app.tool=self.tool;}
        app.text_field=self.text_field;
        if let Some(ctx)=ctx{app.restore_text_focus(ctx);}
    }
}
pub(super) struct FileDialog {
    pub kind:FileKind,pub directory:String,pub name:String,pub error:String,pub replace:bool,pub all_pages:bool,pub recover_backup:bool,pub recover_source:bool,
    pub choose_format:bool,pub details:bool,
    pub problems:Vec<feedback::FileProblem>,
    composing:bool,ime_focus:Option<egui::Id>,focus_name:bool,
    pub cdr_mode:arranger_core::cdr::ExportMode,pub cdr_preflight:Option<Value>,cdr_preflight_key:String,
    native_ready:bool,resume:Option<FileResume>,pub pdf:Option<super::printing::PdfPlan>,
}
impl FileDialog {
    fn new(kind:FileKind,path:&str)->Self {
        let path=std::path::absolute(path).unwrap_or_else(|_|PathBuf::from(path));
        let native_ready=!matches!(&kind,FileKind::Export(f)if matches!(f.as_str(),"cdr14"|"cdr21"|"pdf"|"print"));
        Self{kind,directory:path.parent().unwrap_or(std::path::Path::new(".")).to_string_lossy().into_owned(),
            name:path.file_name().unwrap_or_default().to_string_lossy().into_owned(),error:String::new(),replace:false,all_pages:true,recover_backup:false,recover_source:false,choose_format:false,details:false,problems:vec![],composing:false,ime_focus:None,focus_name:true,cdr_mode:Default::default(),cdr_preflight:None,cdr_preflight_key:String::new(),native_ready,resume:None,pdf:None}
    }
    fn path(&self)->PathBuf {PathBuf::from(&self.directory).join(&self.name)}
    fn set_format(&mut self,format:&str){
        self.kind=if format=="arranger"{FileKind::Save}else{FileKind::Export(format.into())};
        self.name=PathBuf::from(&self.name).with_extension(self.kind.extension().unwrap_or("arranger")).to_string_lossy().into_owned();
        self.cdr_preflight=None;self.cdr_preflight_key.clear();self.clear_failure();self.native_ready=false;
        if format=="arranger"{self.cdr_mode=Default::default();}
    }
    fn clear_failure(&mut self){self.error.clear();self.problems.clear();}
    fn failed(problem:feedback::FileProblem)->Self{let mut d=Self::new(problem.kind.clone(),&problem.path.to_string_lossy());d.error=problem.raw.clone();d.problems.push(problem);d.native_ready=false;d}
    fn copy_requested(&self)->bool{self.problems.len()==1&&self.problems[0].can_copy()}
    fn prepare_copy(&mut self){let stem=PathBuf::from(&self.name).file_stem().unwrap_or_default().to_string_lossy().into_owned();self.name=format!("{stem}-副本.{}",self.kind.extension().unwrap_or("arranger"));self.clear_failure();self.focus_name=true;}
    fn format_controls(&mut self,ui:&mut egui::Ui,w:&mut BTreeMap<String,Value>,zh:bool){
        let cdr=matches!(&self.kind,FileKind::Export(f)if matches!(f.as_str(),"cdr14"|"cdr21"));
        if !self.choose_format&&!cdr{return;}
        ui.horizontal(|ui|{
            if self.choose_format{
                ui.label(tr(zh,"格式","Format"));let mut use_cdr=cdr;
                let r=egui::ComboBox::from_id_salt("file-format").selected_text(if use_cdr{"CorelDRAW (.cdr)"}else{"Arranger (.arranger)"}).show_ui(ui,|ui|{
                    for(value,key,label)in[(false,"arranger","Arranger (.arranger)"),(true,"cdr","CorelDRAW (.cdr)")]{let r=ui.selectable_value(&mut use_cdr,value,label);record_widget!(w,format!("dialog_format_{key}"),rect(r.rect));}
                });record_widget!(w,"dialog_format".into(),rect(r.response.rect));if use_cdr!=cdr{self.set_format(if use_cdr{"cdr14"}else{"arranger"});}
            }
            if let FileKind::Export(f)=&self.kind{if matches!(f.as_str(),"cdr14"|"cdr21"){
                ui.label(tr(zh,"版本","Version"));let mut version=f.clone();
                let r=egui::ComboBox::from_id_salt("file-version").selected_text(if version=="cdr21"{"21.0 · 2019"}else{"14.0"}).show_ui(ui,|ui|{
                    for(value,label)in[("cdr14","14.0"),("cdr21","21.0 · 2019")]{let r=ui.selectable_value(&mut version,value.to_string(),label);record_widget!(w,format!("dialog_version_{value}"),rect(r.rect));}
                });record_widget!(w,"dialog_version".into(),rect(r.response.rect));if version!=*f{self.set_format(&version);}
            }}
        });
    }
    fn matches(&self,path:&std::path::Path)->bool {
        let ext=path.extension().and_then(|e|e.to_str()).unwrap_or("").to_lowercase();
        if self.recover_source{return ext=="arranger"||is_source_backup(path);}
        if self.recover_backup{return ext=="cdr"||path.file_name().is_some_and(|n|n.to_string_lossy().to_ascii_lowercase().ends_with(".cdr.bak"));}
        match &self.kind {FileKind::Image=>matches!(ext.as_str(),"png"|"jpg"|"jpeg"),FileKind::Open=>matches!(ext.as_str(),"arranger"|"cdr"),FileKind::Save=>ext=="arranger",FileKind::Export(format)=>ext==if matches!(format.as_str(),"cdr14"|"cdr21") {"cdr"}else if format=="print"{"pdf"}else{format}}
    }
    fn original_path(&self,path:PathBuf)->PathBuf{
        if self.recover_backup&&path.file_name().is_some_and(|n|n.to_string_lossy().to_ascii_lowercase().ends_with(".cdr.bak")){path.with_extension("")}else{path}
    }
}

impl App {
    pub(super) fn record_file_error(&mut self,kind:&FileKind,path:&std::path::Path,error:arranger_core::Error){let p=feedback::FileProblem::new(kind,path,error,self.chinese);self.error=true;self.message=p.raw.clone();self.file_problem=Some(p);}
    pub(super) fn file_act(&mut self,command:Command,kind:&FileKind,path:&std::path::Path,success:&str)->Option<Value>{
        self.file_problem=None;match self.service.command(command){Ok(value)=>{self.error=false;self.message=success.into();Some(value)},Err(error)=>{self.record_file_error(kind,path,error);None}}
    }
    fn file_failure(&self,kind:&FileKind,path:&std::path::Path)->feedback::FileProblem{self.file_problem.as_ref().filter(|p|p.raw==self.message).cloned().unwrap_or_else(||{let mut p=feedback::FileProblem::new(kind,path,arranger_core::Error::new("FILE_OPERATION",&self.message),self.chinese);p.raw=self.message.clone();p})}
    pub(super) fn resume_file_dialog(&mut self,dialog:FileDialog,ctx:&egui::Context){self.complete_file_dialog(dialog,ctx,false);}
    fn complete_file_dialog(&mut self,mut dialog:FileDialog,ctx:&egui::Context,success:bool){
        let saved=success&&matches!(dialog.kind,FileKind::Save);
        if let Some(resume)=dialog.resume.take(){resume.restore(self,Some(ctx));}
        self.finish_close_save(ctx,saved);
    }
    #[cfg(target_os="macos")]
    pub(super) fn use_native_files(&self)->bool{
        #[cfg(feature="test-hooks")]
        if self.test_output.is_some()&&std::env::args().any(|a|a=="--test-portable-files"){return false;}
        true
    }
    pub(super) fn file_open_blocker(&self,s:&Snapshot)->Option<&'static str>{
        let zh=self.chinese;
        if self.dialog.is_some()||self.quit_prompt{return Some(tr(zh,"请先关闭当前对话框","Close the current dialog first"));}
        #[cfg(target_os="macos")] if self.native_files.active(){return Some(tr(zh,"请等待系统文件面板关闭","Wait for the system file sheet to close"));}
        if s.busy{return Some(tr(zh,"等待当前编辑操作完成","Waiting for the current edit"));}
        if self.inline_text.as_ref().is_some_and(|t|t.composing()){return Some(tr(zh,"请先完成输入法选词","Finish composing text first"));}
        if self.gesture.is_some(){return Some(tr(zh,"请先完成拖动","Finish dragging first"));}
        if !self.pen.is_empty(){return Some(tr(zh,"请先完成或取消当前路径","Finish or cancel the current path"));}
        if self.page_draft_dirty{return Some(tr(zh,"请先应用或恢复页面尺寸","Apply or reload the page size first"));}
        if self.editor.as_ref().is_some_and(|e|e.dirty&&!e.text_dirty){return Some(tr(zh,"请先应用或恢复属性修改","Apply or reload the property changes first"));}
        None
    }
    pub(super) fn process_pending_files(&mut self,ctx:&egui::Context){
        if self.pending_files.is_empty()||self.allow_quit{return;}
        let Ok(s)=self.service.snapshot(&self.active)else{return;};
        if self.file_open_blocker(&s).is_some(){return;}
        if !self.finish_text(ctx,&s){return;}
        let Ok(s)=self.service.snapshot(&self.active)else{return;};
        if self.file_open_blocker(&s).is_some(){return;}
        self.file_open_errors.clear();let mut problems=vec![];
        let mut count=0;
        while let Some(path)=self.pending_files.pop_front(){
            let ext=path.extension().and_then(|v|v.to_str()).unwrap_or("").to_ascii_lowercase();
            let source_backup=is_source_backup(&path);
            if !source_backup&&!matches!(ext.as_str(),"arranger"|"cdr"){
                self.file_open_errors.push(format!("{}: {}",path.display(),tr(self.chinese,"请打开 .arranger 或 .cdr 文档；图片请使用导入图片","Open an .arranger or .cdr document; use Import image for pictures")));problems.push(feedback::FileProblem::new(&FileKind::Open,&path,arranger_core::Error::new("FILE_TYPE","Open an .arranger or .cdr document"),self.chinese));continue;
            }
            if self.finish_file(&FileKind::Open,path.clone(),false,true,false,source_backup,Default::default()){count+=1;}
            else{self.file_open_errors.push(format!("{}: {}",path.display(),self.message));problems.push(self.file_failure(&FileKind::Open,&path));}
        }
        self.error=!self.file_open_errors.is_empty();
        if self.error{self.message=format!("{} {count} · {} {}\n{}",tr(self.chinese,"已打开","Opened"),tr(self.chinese,"未打开","Not opened"),self.file_open_errors.len(),self.file_open_errors.join("\n"));self.file_problem=Some(feedback::FileProblem::batch(&problems,count,self.message.clone(),self.chinese));}
        ctx.request_repaint();
    }
    pub(super) fn file_action(&mut self,snapshot:&Snapshot,action:&str) {
        if self.dialog.is_some()||self.quit_prompt{return;}
        #[cfg(target_os="macos")] if self.native_files.active(){return;}
        if self.inline_text.as_ref().is_some_and(|t|t.composing()){
            self.error=true;self.message=tr(self.chinese,"请先完成输入法选词，再进行文件操作","Finish composing text before a file operation").into();return;
        }
        let mut resume=FileResume::take(self,snapshot.revision);
        self.file_action_inner(snapshot,action);
        if self.active==resume.document{if let Ok(s)=self.service.snapshot(&self.active){resume.revision=s.revision;}}
        if let Some(dialog)=&mut self.dialog{dialog.resume=Some(resume);}else{resume.restore(self,None);}
    }
    fn file_action_inner(&mut self,snapshot:&Snapshot,action:&str) {
        let mut snapshot=snapshot.clone();
        if matches!(action,"new"|"open"|"recover_cdr"|"recover_source"){
            if let Some(reason)=self.file_open_blocker(&snapshot){self.error=true;self.message=reason.into();return;}
        }
        if self.inline_text.is_some()||self.editor.as_ref().is_some_and(|e|e.text_dirty){
            if !self.commit_text(&snapshot){return;}
            if self.inline_text.is_some(){self.tool=Tool::Select;}
            self.inline_text=None;self.text_field=None;
            if let Ok(current)=self.service.snapshot(&self.active){snapshot=current;}else{return;}
        }
        if matches!(action,"new"|"open"|"recover_cdr"|"recover_source"){
            if let Some(reason)=self.file_open_blocker(&snapshot){self.error=true;self.message=reason.into();return;}
        }
        if matches!(action,"print"|"save"|"save_as"|"svg"|"png"|"pdf"|"cdr14"|"cdr21"|"cdr14_appearance"|"cdr21_appearance") {
            if self.page_draft_dirty {self.error=true;self.message=tr(self.chinese,"请先应用或恢复页面尺寸，再保存或导出","Apply or reload the page size before saving").into();return;}
            if self.editor.as_ref().is_some_and(|e|e.dirty) {
                self.apply_editor(&snapshot);
                if self.error{return;}
                if let Ok(current)=self.service.snapshot(&self.active){snapshot=current;}else{return;}
            }
            if !self.pen.is_empty(){self.finish_pen(&snapshot,false);if self.error{return;}self.pen.clear();if let Ok(current)=self.service.snapshot(&self.active){snapshot=current;}}
        }
        match action {
            "new"=>{let id=format!("untitled-{}",self.next_document);self.next_document+=1;
                if self.act(Command::DocumentNew{document_id:id.clone(),width_mm:210.,height_mm:297.,unit:"mm".into()},"New document").is_some(){
                    self.remember_document_path();self.active=id;self.path=format!("未命名-{}.arranger",self.next_document-1);self.page="page-1".into();self.set_selection(vec![]);self.fit=true;self.zoom=1.;self.pan=Vec2::ZERO;self.pen.clear();self.gesture=None;self.page_draft_dirty=false;self.page_draft_key.clear();
                }
            },
            "open"=>self.dialog=Some(FileDialog::new(FileKind::Open,&self.path)),
            "recover_source"=>{let mut d=FileDialog::new(FileKind::Open,&PathBuf::from(&self.path).with_extension("arranger").to_string_lossy());d.recover_source=true;self.dialog=Some(d);},
            "recover_cdr"=>{let mut d=FileDialog::new(FileKind::Open,&PathBuf::from(&self.path).with_extension("cdr").to_string_lossy());d.recover_backup=true;self.dialog=Some(d);},
            "save_as"=>{
                let path=PathBuf::from(&self.path);let stem=path.file_stem().unwrap_or_default().to_string_lossy();
                let copy=path.with_file_name(format!("{stem}-副本.arranger"));let mut d=FileDialog::new(FileKind::Save,&copy.to_string_lossy());d.choose_format=true;d.native_ready=false;self.dialog=Some(d);
            },
            "save"=>{
                if let Some(path)=&snapshot.saved_path {if !self.finish_file(&FileKind::Save,PathBuf::from(path),false,true,false,false,Default::default())&&self.close_flow.is_none(){if let Some(p)=self.file_problem.clone(){self.dialog=Some(FileDialog::failed(p));}}}
                else{self.dialog=Some(FileDialog::new(FileKind::Save,&self.path));}
            },
            "print"|"pdf"=>{let mut d=FileDialog::new(FileKind::Export(action.into()),&PathBuf::from(&self.path).with_extension("pdf").to_string_lossy());d.pdf=Some(super::printing::PdfPlan::new(self.active.clone(),&snapshot,&self.page));self.dialog=Some(d);},
            "svg"|"png"=>self.dialog=Some(FileDialog::new(FileKind::Export(action.into()),&PathBuf::from(&self.path).with_extension(action).to_string_lossy())),
            "cdr14"|"cdr21"=>self.dialog=Some(FileDialog::new(FileKind::Export(action.into()),&PathBuf::from(&self.path).with_extension("cdr").to_string_lossy())),
            "cdr14_appearance"|"cdr21_appearance"=>{
                let format=action.trim_end_matches("_appearance");let path=PathBuf::from(&self.path);
                let name=format!("{}-保外观.cdr",path.file_stem().unwrap_or_default().to_string_lossy());
                let mut dialog=FileDialog::new(FileKind::Export(format.into()),&path.with_file_name(name).to_string_lossy());
                dialog.cdr_mode=arranger_core::cdr::ExportMode::Appearance;self.dialog=Some(dialog);
            },
            "image"=>{let mut dialog=FileDialog::new(FileKind::Image,&self.path);dialog.name.clear();self.dialog=Some(dialog);},
            _=>{}
        }
    }
    fn finish_dialog_file(&mut self,dialog:&FileDialog,path:PathBuf,overwrite:bool)->bool{
        if matches!(dialog.kind,FileKind::Save|FileKind::Export(_))&&path.is_dir(){self.record_file_error(&dialog.kind,&path,arranger_core::Error::new("FILE_DIRECTORY","The output path points to a directory"));return false;}
        if let Some(plan)=&dialog.pdf{return self.finish_pdf(plan,path,overwrite,matches!(&dialog.kind,FileKind::Export(f)if f=="print"));}
        self.finish_file(&dialog.kind,path,overwrite,dialog.all_pages,dialog.recover_backup,dialog.recover_source,dialog.cdr_mode)
    }
    fn finish_file(&mut self,kind:&FileKind,path:PathBuf,overwrite:bool,all_pages:bool,recover_backup:bool,recover_source:bool,cdr_mode:arranger_core::cdr::ExportMode)->bool {
        let snapshot=match self.service.snapshot(&self.active){Ok(s)=>s,Err(e)=>{self.error=true;self.message=e.to_string();return false;}};
        let path=match std::path::absolute(path){Ok(p)=>p,Err(e)=>{self.error=true;self.message=e.to_string();return false;}};
        let path_string=path.to_string_lossy().into_owned();
        match kind {
            FileKind::Open=>{
                // Resolve aliases/symlinks, but keep the user's spelling for
                // display. Reopening a file activates its existing session,
                // preserving its current edits and undo stack.
                let key=path.canonicalize().unwrap_or_else(|_|path.clone());
                let cdr=path.extension().and_then(|s|s.to_str()).is_some_and(|s|s.eq_ignore_ascii_case("cdr"));
                if !recover_backup&&!recover_source{
                    let docs=self.service.command(Command::Documents).unwrap_or_default();
                    let existing=docs["documents"].as_array().and_then(|ds|ds.iter().find(|d|d["saved_path"].as_str().is_some_and(|p|PathBuf::from(p).canonicalize().is_ok_and(|p|p==key))).and_then(|d|d["document_id"].as_str()).map(str::to_owned));
                    let existing=existing.or_else(||self.opened_cdrs.get(&key).filter(|id|self.service.snapshot(id).is_ok()).cloned());
                    if let Some(id)=existing{if !self.activate_document(&id){return false;}self.file_problem=None;self.error=false;self.recents.remember(&path);self.message=tr(self.chinese,"已切换到打开的文档；保留当前修改","Switched to the open document; current edits preserved").into();return true;}
                }
                let id=format!("opened-{}",self.next_document);self.next_document+=1;
                let command=if recover_source{Command::DocumentRecoverBackup{document_id:id.clone(),path:path_string.clone()}}else if cdr&&recover_backup {Command::CdrRecoverBackup{document_id:id.clone(),path:path_string.clone()}}else if cdr {Command::CdrImport{document_id:id.clone(),path:path_string.clone()}}else{Command::DocumentOpen{document_id:id.clone(),path:path_string.clone()}};
                if self.file_act(command,kind,&path,"Document opened").is_some(){
                    if cdr&&!recover_backup{self.opened_cdrs.insert(key,id.clone());}
                    self.remember_document_path();self.active=id;self.path=if recover_source{recovery_copy_path(&path).to_string_lossy().into_owned()}else if cdr {path.with_extension("arranger").to_string_lossy().into_owned()}else{path_string};self.page="page-1".into();self.set_selection(vec![]);self.fit=true;self.zoom=1.;self.pan=Vec2::ZERO;self.pen.clear();self.gesture=None;self.page_draft_dirty=false;self.page_draft_key.clear();if !recover_source&&!recover_backup{self.recents.remember(&path);}if recover_source{self.message=tr(self.chinese,"已从原稿备份恢复；原件未改动，请另存恢复副本","Source backup recovered; originals unchanged. Save a recovered copy").into();}else if cdr {self.message=if recover_backup{tr(self.chinese,"已从 CDR 备份恢复；原文件未改动，请另存 .arranger 原稿","CDR backup recovered; files unchanged. Save an .arranger source")}else{tr(self.chinese,"CDR 已实验导入；请另存 .arranger 完整原稿","CDR imported experimentally; save an .arranger source")}.into();}true
                }else{false}
            },
            FileKind::Save=>{if self.file_act(Command::Save{document_id:self.active.clone(),expected_revision:snapshot.revision,path:path_string.clone(),overwrite},kind,&path,"Document saved").is_some(){self.path=path_string;self.recents.remember(&path);true}else{false}},
            FileKind::Export(format)=>{
                let print=format=="print";
                let format=match format.as_str(){"svg"=>ExportFormat::Svg,"pdf"|"print"=>ExportFormat::Pdf,"cdr14"=>ExportFormat::Cdr14,"cdr21"=>ExportFormat::Cdr21,_=>ExportFormat::Png};
                let cdr=matches!(format,ExportFormat::Cdr14|ExportFormat::Cdr21);
                let command=if cdr {Command::CdrExport{document_id:self.active.clone(),path:path_string,format:if matches!(format,ExportFormat::Cdr21) {"cdr21".into()}else{"cdr14".into()},overwrite,mode:Some(cdr_mode),page_id:if all_pages{None}else{Some(self.page.clone())}}}else{Command::Export{document_id:self.active.clone(),page_id:self.page.clone(),format,path:path_string,overwrite,dpi:96.}};
                let result=self.file_act(command,kind,&path,"Export complete");if let Some(result)=&result{if cdr {let mode=if cdr_mode==arranger_core::cdr::ExportMode::Editable{tr(self.chinese,"保编辑","editable")}else{tr(self.chinese,"保外观","appearance")};let count=result["converted_text_ids"].as_array().map_or(0,Vec::len);self.message=format!("{} · {mode} · {} ({count})",if matches!(format,ExportFormat::Cdr21){"CDR21"}else{"CDR14"},tr(self.chinese,"已导出；完整原稿请保存为 .arranger；转曲文字","exported; keep the complete .arranger source; outlined text"));}}
                if print&&result.is_some(){
                    #[cfg(target_os="macos")] {
                        #[cfg(feature="test-hooks")] let preview=self.test_output.is_none();
                        #[cfg(not(feature="test-hooks"))] let preview=true;
                        if preview{if let Err(e)=std::process::Command::new("/usr/bin/open").args(["-a","Preview"]).arg(&path).spawn(){self.error=true;self.message=format!("PDF已生成；预览未打开：{e}");return true;}}
                    }
                    self.message=tr(self.chinese,"打印PDF已生成；可在系统预览中选择纸张与打印机","Print PDF created; choose paper and printer in the system preview").into();
                }result.is_some()
            },
            FileKind::Image=>{
                let result=arranger_core::resource::import(&path).and_then(|r|{let info=arranger_core::resource::validate(&r)?;Ok((info.width_px,info.height_px))});
                match result {Ok((w,h))=>{
                    let scale=(80./f64::from(w)).min(80./f64::from(h));
                    let resource_id=self.unique(&snapshot.document,"image");
                    let mut object=self.object(&snapshot.document,Geometry::Image{resource_id,clip_mm:None,fit:Default::default()},20.,20.,f64::from(w)*scale,f64::from(h)*scale);
                    object.style=arranger_core::Style{fill:"none".into(),..Default::default()};let id=object.id.clone();
                    let operations=workspace::placed(vec![Operation::ImageImport{path:path_string,object}],&self.page);
                    if self.file_act(Command::Batch{document_id:self.active.clone(),expected_revision:snapshot.revision,unit:"mm".into(),operations},kind,&path,"Image imported").is_some(){self.editor=None;self.set_selection(vec![id]);true}else{false}
                },Err(e)=>{self.record_file_error(kind,&path,e);false}}
            }
        }
    }
    pub(super) fn show_file_dialog(&mut self,ctx:&egui::Context,widgets:&mut BTreeMap<String,Value>) {
        let Some(mut dialog)=self.dialog.take() else{return;};let zh=self.chinese;
        #[cfg(target_os="macos")]
        let native=self.use_native_files();
        #[cfg(not(target_os="macos"))]
        let native=false;
        #[cfg(target_os="macos")]
        if native&&dialog.native_ready{
            if let Some(reply)=self.native_files.take_reply(){
                if !reply.accepted{
                    if reply.aborted{self.record_file_error(&dialog.kind,&dialog.path(),arranger_core::Error::new("NATIVE_FILE_PANEL","The system file sheet did not complete"));dialog.problems=vec![self.file_failure(&dialog.kind,&dialog.path())];dialog.error=self.message.clone();dialog.native_ready=false;dialog.focus_name=true;self.dialog=Some(dialog);ctx.request_repaint();return;}
                    self.resume_file_dialog(dialog,ctx);return;
                }
                if matches!(dialog.kind,FileKind::Open){
                    self.file_open_errors.clear();let mut opened=0;let mut problems=vec![];let empty_paths=reply.paths.is_empty();
                    if reply.paths.is_empty(){self.record_file_error(&dialog.kind,&dialog.path(),arranger_core::Error::new("FILE_PANEL_RESULT","The system did not return a file path"));self.file_open_errors.push(self.message.clone());problems.push(self.file_failure(&dialog.kind,&dialog.path()));}
                    for path in reply.paths{let path=dialog.original_path(path);
                        if !dialog.matches(&path){self.record_file_error(&dialog.kind,&path,arranger_core::Error::new("FILE_TYPE","Unexpected file type"));self.file_open_errors.push(format!("{}: {}",path.display(),self.message));problems.push(self.file_failure(&dialog.kind,&path));continue;}
                        if self.finish_dialog_file(&dialog,path.clone(),false){opened+=1;}else{self.file_open_errors.push(format!("{}: {}",path.display(),self.message));problems.push(self.file_failure(&dialog.kind,&path));}
                    }
                    self.error=!self.file_open_errors.is_empty();if self.error{if empty_paths{self.message=problems[0].raw.clone();self.file_problem=Some(problems[0].clone());}else{self.message=format!("{} {opened} · {}\n{}",tr(zh,"已打开","Opened"),tr(zh,"部分文件未打开","Some files did not open"),self.file_open_errors.join("\n"));self.file_problem=Some(feedback::FileProblem::batch(&problems,opened,self.message.clone(),zh));}let first=&problems[0].path;dialog.directory=first.parent().unwrap_or(std::path::Path::new(".")).to_string_lossy().into_owned();dialog.name=first.file_name().unwrap_or_default().to_string_lossy().into_owned();dialog.problems=problems;dialog.error=self.message.clone();dialog.native_ready=false;dialog.focus_name=true;self.dialog=Some(dialog);ctx.request_repaint();return;}
                    self.resume_file_dialog(dialog,ctx);ctx.request_repaint();return;
                }
                if let Some(mut path)=reply.paths.into_iter().next(){
                    if path.extension().is_none(){if let Some(ext)=dialog.kind.extension(){path.set_extension(ext);}}
                    if dialog.matches(&path){
                        // AppKit's OK response includes its replacement confirmation.
                        let overwrite=path.exists();
                        if self.finish_dialog_file(&dialog,path.clone(),overwrite){self.complete_file_dialog(dialog,ctx,true);ctx.request_repaint();return;}
                    }else{self.record_file_error(&dialog.kind,&path,arranger_core::Error::new("FILE_TYPE","Unexpected file type"));}
                    dialog.directory=path.parent().unwrap_or(std::path::Path::new(".")).to_string_lossy().into_owned();dialog.name=path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                }else{self.record_file_error(&dialog.kind,&dialog.path(),arranger_core::Error::new("FILE_PANEL_RESULT","The system did not return a file path"));}
                dialog.native_ready=false;dialog.error=self.message.clone();dialog.problems=vec![self.file_failure(&dialog.kind,&dialog.path())];dialog.focus_name=true;
            }else{
                if !self.native_files.active(){if let Err(error)=self.native_files.show(&dialog,zh){self.record_file_error(&dialog.kind,&dialog.path(),arranger_core::Error::new("NATIVE_FILE_PANEL",error));dialog.native_ready=false;dialog.error=self.message.clone();dialog.problems=vec![self.file_failure(&dialog.kind,&dialog.path())];}}
                if dialog.native_ready{self.dialog=Some(dialog);return;}
            }
        }
        // An IME confirmation/dismissal key belongs to the filename editor,
        // including the frame containing Commit or the final empty Preedit.
        let mut ime_frame=dialog.composing;
        for event in ctx.input(|i|i.events.clone()){if let egui::Event::Ime(event)=event{
            #[allow(deprecated)]
            match event{egui::ImeEvent::Preedit{text,..}=>{if !dialog.composing{dialog.ime_focus=ctx.memory(|m|m.focused());}ime_frame=true;dialog.composing=!text.is_empty();},egui::ImeEvent::Commit(_)|egui::ImeEvent::Disabled=>{ime_frame=true;dialog.composing=false;},_=>{}}
        }}
        if ime_frame{if let Some(id)=dialog.ime_focus{ctx.memory_mut(|m|m.request_focus(id));}}
        let fresh_enter=!ime_frame&&primary_enter(ctx);
        let title=if dialog.choose_format{tr(zh,"另存为","Save as")}else{match &dialog.kind{FileKind::Open if dialog.recover_source=>tr(zh,"恢复原稿","Recover source"),FileKind::Open if dialog.recover_backup=>tr(zh,"恢复 CDR","Recover CDR"),FileKind::Open=>tr(zh,"打开文件","Open file"),FileKind::Save=>tr(zh,"保存文件","Save file"),FileKind::Export(f)if f=="print"=>tr(zh,"打印 / PDF","Print / PDF"),FileKind::Export(_)=>tr(zh,"导出文件","Export file"),FileKind::Image=>tr(zh,"导入图片","Import image")}};
        let mut cancel=false;let mut confirm=false;let mut replace=false;let mut pdf_allowed=true;let mut file_allowed=false;let mut directory_enter=false;let mut copy=false;let mut recover=false;let mut clear_feedback=false;
        let modal=egui::Modal::new(egui::Id::unique("file-dialog")).frame(modal_frame(ctx)).show(ctx,|ui|{
            ui.set_width((ctx.content_rect().width()-72.).clamp(280.,520.));ui.spacing_mut().item_spacing=Vec2::new(8.,10.);
            modal_heading(ui,title);
            egui::ScrollArea::vertical().id_salt("file-dialog-body").max_height((ctx.content_rect().height()-190.).max(160.)).show(ui,|ui|{
                if dialog.replace{
                    ui.label(tr(zh,"同名文件已存在，要替换吗？","A file with this name exists. Replace it?"));ui.label(egui::RichText::new(&dialog.name).strong());
                    ui.label(egui::RichText::new(&dialog.directory).small().color(MUTED));return;
                }
                let previous=format!("{:?}",dialog.kind);dialog.format_controls(ui,widgets,zh);if previous!=format!("{:?}",dialog.kind){clear_feedback=true;}
                if let Some(plan)=&mut dialog.pdf{pdf_allowed=self.pdf_options(ui,plan,widgets);}
                if let FileKind::Export(format)=dialog.kind.clone(){if matches!(format.as_str(),"cdr14"|"cdr21"){
                    let r=ui.checkbox(&mut dialog.all_pages,tr(zh,"全部页面","All pages"));record_widget!(widgets,"dialog_all_pages".into(),rect(r.rect));r.on_hover_text(tr(zh,"关闭后仅输出当前页；开启时保留共享区对象。","Turn off for the current page; all pages also preserves workspace objects."));
                    if let Ok(snapshot)=self.service.snapshot(&self.active){
                        let key=format!("{}:{}:{}:{:?}:{}:{}:{}",snapshot.document_hash,snapshot.revision,format,dialog.cdr_mode,dialog.all_pages,self.page,snapshot.busy);
                        if dialog.cdr_preflight_key!=key{
                            let had_error=!dialog.error.is_empty();let clearable=!dialog.problems.iter().any(|p|p.is_io());if clearable{dialog.clear_failure();}let page_id=if dialog.all_pages{None}else{Some(self.page.clone())};
                            let mut report=match self.service.command(Command::CdrPreflight{document_id:self.active.clone(),format:format.clone(),mode:dialog.cdr_mode,page_id:page_id.clone()}){Ok(v)=>v,Err(e)=>json!({"can_export":false,"error":e})};
                            if dialog.cdr_mode==arranger_core::cdr::ExportMode::Editable&&report["can_export"]!=true{report["appearance_available"]=json!(self.service.command(Command::CdrPreflight{document_id:self.active.clone(),format:format.clone(),mode:arranger_core::cdr::ExportMode::Appearance,page_id}).is_ok_and(|r|r["can_export"]==true));}
                            dialog.cdr_preflight=Some(report);dialog.cdr_preflight_key=key;
                            if had_error&&clearable&&dialog.cdr_preflight.as_ref().is_some_and(|r|r["can_export"]==true){self.file_problem=None;self.error=false;self.message=tr(zh,"预检通过；请选择文件导出","Preflight passed; choose an export file").into();}
                        }
                    }
                    if let Some(report)=&dialog.cdr_preflight{
                        if report["can_export"]==true{
                            let text=if dialog.cdr_mode==arranger_core::cdr::ExportMode::Appearance{tr(zh,"保外观副本 · 文字转曲","Appearance copy · text as curves")}else{tr(zh,"可编辑文字 · Corel 中排版可能变化","Editable text · layout may differ in Corel")};ui.label(egui::RichText::new(text).small().color(MUTED));
                        }else if report["suggested_format"]=="cdr21"{
                            ui.colored_label(Color32::from_rgb(182,50,57),tr(zh,"此透明效果需要 21.0。","This transparency requires 21.0."));
                            let r=ui.button(tr(zh,"改用 21.0","Use 21.0"));record_widget!(widgets,"dialog_cdr_use21".into(),rect(r.rect));if r.clicked(){dialog.set_format("cdr21");}
                        }else{
                            ui.colored_label(Color32::from_rgb(182,50,57),tr(zh,"部分内容无法按当前方式保存。","Some content cannot be saved with these settings."));
                            if report["appearance_available"]==true{let r=ui.button(tr(zh,"改存保外观副本","Save appearance copy"));record_widget!(widgets,"dialog_cdr_appearance".into(),rect(r.rect));if r.clicked(){dialog.cdr_mode=arranger_core::cdr::ExportMode::Appearance;dialog.cdr_preflight_key.clear();}}
                        }
                    }
                    if dialog.cdr_mode==arranger_core::cdr::ExportMode::Appearance{let r=ui.button(tr(zh,"保留可编辑文字","Keep editable text"));record_widget!(widgets,"dialog_cdr_editable".into(),rect(r.rect));if r.clicked(){dialog.cdr_mode=arranger_core::cdr::ExportMode::Editable;dialog.cdr_preflight_key.clear();}}
                    if format=="cdr21"{
                        let r=ui.add_enabled(false,egui::Checkbox::new(&mut false,tr(zh,"嵌入字体 · 尚未支持","Embed fonts · not yet supported")));record_widget!(widgets,"dialog_font_embedding_unavailable".into(),rect(r.rect));r.on_hover_text(tr(zh,"当前仅写入字体引用，不嵌入字体文件。需跨机保外观时使用保外观副本；该副本的文字会转曲。","Font references only; font files are not embedded. Use an appearance copy for matching shapes across computers; its text becomes curves."));
                    }
                }}
                if !native{
                    ui.horizontal(|ui|{
                        if shell::icon_button(ui,widgets,"dialog_up","chevron-up","",true,false).on_hover_text(tr(zh,"上一级文件夹","Parent folder")).clicked(){if let Some(parent)=PathBuf::from(&dialog.directory).parent(){dialog.directory=parent.to_string_lossy().into_owned();dialog.clear_failure();clear_feedback=true;}}
                        let r=ui.add(egui::TextEdit::singleline(&mut dialog.directory).id_salt("file-directory").return_key(None).desired_width(ui.available_width()));record_widget!(widgets,"dialog_directory".into(),rect(r.rect));
                        if r.changed(){dialog.clear_failure();clear_feedback=true;}
                        if fresh_enter&&r.has_focus(){directory_enter=true;ctx.input_mut(|i|i.consume_key(egui::Modifiers::NONE,egui::Key::Enter));
                            if PathBuf::from(&dialog.directory).is_dir(){dialog.error.clear();dialog.focus_name=true;}else{dialog.error=tr(zh,"找不到此文件夹，请检查位置。","Folder not found; check the location.").into();}
                        }
                    });
                    egui::Frame::new().fill(Color32::from_rgb(248,248,249)).corner_radius(5).inner_margin(8).show(ui,|ui|{
                        ui.set_min_width(ui.available_width());
                        egui::ScrollArea::vertical().id_salt("file-entries").max_height(if matches!(dialog.kind,FileKind::Open|FileKind::Image){190.}else{110.}).min_scrolled_height(70.).show(ui,|ui|{
                            match std::fs::read_dir(&dialog.directory){
                                Ok(entries)=>{
                                    let mut entries:Vec<_>=entries.filter_map(Result::ok).filter(|e|e.path().is_dir()||dialog.matches(&e.path())).collect();entries.sort_by_key(|e|(!e.path().is_dir(),e.file_name().to_string_lossy().to_lowercase()));
                                    if entries.is_empty(){ui.label(egui::RichText::new(tr(zh,"没有匹配的文件","No matching files")).small().color(MUTED));}
                                    for entry in entries{let name=entry.file_name().to_string_lossy().into_owned();let is_dir=entry.path().is_dir();
                                        let r=ui.selectable_label(!is_dir&&dialog.name==name,format!("{}  {name}",if is_dir{"▸"}else{"·"}));record_widget!(widgets,format!("dialog_entry_{name}"),rect(r.rect));
                                        if r.double_clicked(){dialog.clear_failure();clear_feedback=true;if is_dir{dialog.directory=entry.path().to_string_lossy().into_owned();}else{dialog.name=name;confirm=true;}}
                                        else if r.clicked(){dialog.clear_failure();clear_feedback=true;if is_dir{dialog.directory=entry.path().to_string_lossy().into_owned();}else{dialog.name=name;}}
                                    }
                                },Err(e)=>{ui.colored_label(Color32::from_rgb(182,50,57),tr(zh,"无法访问此文件夹，请检查路径或切换位置。","This folder cannot be read; check the path or choose another location.")).on_hover_text(e.to_string());}
                            }
                        });
                    });
                    ui.horizontal(|ui|{ui.label(tr(zh,"名称","Name"));let r=ui.add(egui::TextEdit::singleline(&mut dialog.name).id_salt("file-name").return_key(None).desired_width(ui.available_width()));record_widget!(widgets,"dialog_name".into(),rect(r.rect));if r.changed(){dialog.clear_failure();clear_feedback=true;}
                        if dialog.focus_name{r.request_focus();dialog.focus_name=false;}
                    });
                }
                if !dialog.problems.is_empty(){
                    if let Some(p)=self.file_problem.as_ref().filter(|p|p.code=="OPEN_ERRORS"&&p.raw==dialog.error){ui.label(egui::RichText::new(&p.summary).small().color(MUTED));}
                    for(i,p)in dialog.problems.iter().take(3).enumerate(){
                        if !matches!(p.code.as_str(),"FILE_PANEL_RESULT"|"NATIVE_FILE_PANEL"){let r=ui.add(egui::Label::new(egui::RichText::new(p.path.file_name().unwrap_or_default().to_string_lossy()).strong()).truncate()).on_hover_text(p.path.to_string_lossy());record_widget!(widgets,format!("dialog_problem_name_{i}"),rect(r.rect));}
                        let summary=if dialog.recover_source||dialog.recover_backup{tr(zh,"备份未能恢复","The backup could not be recovered")}else{&p.summary};let r=ui.colored_label(Color32::from_rgb(182,50,57),summary);record_widget!(widgets,format!("dialog_problem_summary_{i}"),rect(r.rect));
                        if dialog.problems.len()==1{ui.label(egui::RichText::new(&p.hint).small().color(MUTED));if !dialog.recover_source&&!dialog.recover_backup&&p.backup().is_some(){let r=ui.button(tr(zh,"从备份恢复副本","Recover a copy from backup"));record_widget!(widgets,"dialog_recover_available".into(),rect(r.rect));recover=button_activated(ctx,&r);}}
                    }
                    if dialog.problems.len()>3{ui.label(format!("{} {}",tr(zh,"更多失败项见详细信息：","More failures in details:"),dialog.problems.len()-3));}
                }else if !dialog.error.is_empty(){ui.colored_label(Color32::from_rgb(182,50,57),&dialog.error);}
                if shell::icon_button(ui,widgets,"dialog_details",if dialog.details{"chevron-up"}else{"chevron-down"},tr(zh,"更多选项","More options"),true,false).clicked(){dialog.details=!dialog.details;}
                if dialog.details{
                    let area=ui.scope(|ui|{
                        ui.style_mut().wrap_mode=Some(egui::TextWrapMode::Wrap);
                        for p in &dialog.problems{ui.label(egui::RichText::new(p.path.to_string_lossy()).small().color(MUTED));ui.label(egui::RichText::new(&p.raw).small().color(MUTED));}if !dialog.problems.is_empty(){record_widget!(widgets,"dialog_problem_details".into(),rect(ui.min_rect()));ui.separator();}
                        if dialog.recover_source{ui.label(tr(zh,"恢复为新副本，原稿与备份均保留。","Recover into a new copy; original and backup stay unchanged."));}
                        else if matches!(dialog.kind,FileKind::Open){
                            ui.label(egui::RichText::new(tr(zh,"支持 .arranger 与 .cdr；CDR 导入后可另存完整原稿。","Open .arranger or .cdr; imported CDR can be saved as a complete source.")).small().color(MUTED));
                            let r=ui.checkbox(&mut dialog.recover_backup,tr(zh,"从 CDR 备份恢复","Recover CDR backup"));record_widget!(widgets,"dialog_recover_backup".into(),rect(r.rect));
                        }else if matches!(dialog.kind,FileKind::Save){ui.label(egui::RichText::new(tr(zh,"保留全部页面、共享区、资源与编辑结构。","Keep all pages, workspace, resources and editable objects.")).small().color(MUTED));}
                        if let FileKind::Export(f)=&dialog.kind{if f.starts_with("cdr"){
                            ui.label(egui::RichText::new(if f=="cdr14"{tr(zh,"14.0 兼容旧版；群组隔离透明需使用 21.0。","14.0 for older applications; isolated group transparency requires 21.0.")}else{tr(zh,"21.0 对应 CorelDRAW 2019，保留支持的群组透明。","21.0 targets CorelDRAW 2019 and preserves supported group transparency.")}).small().color(MUTED));
                            ui.label(egui::RichText::new(tr(zh,"CDR 是交付副本；完整可编辑原稿仍保存为 .arranger。","CDR is a delivery copy; keep the complete editable .arranger source.")).small().color(MUTED));
                            if let Some(r)=&dialog.cdr_preflight{if let Some(error)=r["error"]["message"].as_str(){ui.label(egui::RichText::new(error).small().color(MUTED));}}
                        }}
                        if let Some(plan)=&dialog.pdf{self.pdf_details(ui,plan);}
                    });record_widget!(widgets,"dialog_detail_content".into(),rect(area.response.rect));
                }
            });
            ui.add_space(8.);ui.separator();
            ui.allocate_ui_with_layout(Vec2::new(ui.available_width(),32.),egui::Layout::right_to_left(egui::Align::Center),|ui|{
                let allowed=pdf_allowed&&dialog.cdr_preflight.as_ref().is_none_or(|r|r["can_export"]==true)&&(native||(!dialog.name.trim().is_empty()&&PathBuf::from(&dialog.directory).is_dir()));file_allowed=allowed;record_widget!(widgets,"dialog_confirm_enabled".into(),json!(allowed));
                let label=if dialog.replace{tr(zh,"替换","Replace")}else if dialog.copy_requested(){tr(zh,"另存副本…","Save a copy…")}else if !dialog.problems.is_empty(){if native{tr(zh,"重新选择…","Choose again…")}else{tr(zh,"重试","Retry")}}else if native{tr(zh,"选择位置…","Choose location…")}else if dialog.choose_format{tr(zh,"保存","Save")}else{match dialog.kind{FileKind::Open=>tr(zh,"打开","Open"),FileKind::Save=>tr(zh,"保存","Save"),FileKind::Image=>tr(zh,"导入","Import"),_=>tr(zh,"导出","Export")}};
                let r=ui.add_enabled(allowed,egui::Button::new(egui::RichText::new(label).strong().color(Color32::WHITE)).fill(if allowed{ACCENT}else{Color32::from_gray(185)}).min_size(Vec2::new(96.,32.)));record_widget!(widgets,if dialog.replace{"dialog_replace"}else{"dialog_confirm"}.into(),rect(r.rect));record_widget!(widgets,"dialog_confirm_focused".into(),json!(r.has_focus()));if native&&dialog.focus_name{r.request_focus();dialog.focus_name=false;}let clicked=button_activated(ctx,&r);if dialog.replace{replace=clicked;}else if dialog.copy_requested(){copy|=clicked;}else{confirm|=clicked;}
                let r=ui.add_sized([76.,32.],egui::Button::new(tr(zh,"取消","Cancel")));record_widget!(widgets,"dialog_cancel".into(),rect(r.rect));cancel=r.clicked();
                if dialog.replace{let r=ui.button(tr(zh,"返回","Back"));record_widget!(widgets,"dialog_back".into(),rect(r.rect));if r.clicked(){dialog.replace=false;dialog.focus_name=true;}}
            });
        });record_widget!(widgets,"file-dialog".into(),rect(modal.response.rect));
        // Keep Enter on a focused button/checkbox or an open combo in that
        // control. Text editors and an unfocused options sheet use the primary
        // action; a held key cannot accept a later replacement warning.
        if fresh_enter&&!directory_enter&&!modal.any_popup_open&&!egui::Popup::is_any_open(ctx)&&(ctx.text_edit_focused()||ctx.memory(|m|m.focused().is_none())){
            if dialog.replace{replace=true;}else if dialog.copy_requested(){copy=true;}else{confirm=true;}
        }
        if cancel||(!ime_frame&&modal.should_close()){self.resume_file_dialog(dialog,ctx);return;}
        if clear_feedback{self.clear_file_problem();}
        if ime_frame{confirm=false;replace=false;copy=false;recover=false;}
        if !file_allowed{confirm=false;replace=false;copy=false;}
        if copy{dialog.prepare_copy();self.clear_file_problem();if native{dialog.native_ready=true;}self.dialog=Some(dialog);ctx.request_repaint();return;}
        if recover{if let Some(p)=dialog.problems.first().cloned(){if let Some(source)=p.backup(){dialog.recover_source=source;dialog.recover_backup=!source;if self.finish_dialog_file(&dialog,p.path.clone(),false){self.complete_file_dialog(dialog,ctx,true);return;}dialog.error=self.message.clone();dialog.problems=vec![self.file_failure(&dialog.kind,&p.path)];}}self.dialog=Some(dialog);ctx.request_repaint();return;}
        if native&&confirm{dialog.native_ready=true;self.dialog=Some(dialog);ctx.request_repaint();return;}
        if confirm||replace {
            let mut path=dialog.original_path(dialog.path());
            let extension=dialog.kind.extension();
            if let Some(extension)=extension {if path.extension().is_none(){path.set_extension(extension);dialog.name=path.file_name().unwrap_or_default().to_string_lossy().into_owned();}}
            if dialog.recover_backup&&path.extension().and_then(|s|s.to_str()).is_none_or(|s|!s.eq_ignore_ascii_case("cdr")) {dialog.error=tr(zh,"备份恢复仅用于 CDR；请选择原 .cdr 路径","CDR recovery requires the original .cdr path").into();}
            else if !dialog.matches(&path) {self.record_file_error(&dialog.kind,&path,arranger_core::Error::new("FILE_TYPE","Select a file with the expected extension"));dialog.error=self.message.clone();dialog.problems=vec![self.file_failure(&dialog.kind,&path)];}
            else if matches!(dialog.kind,FileKind::Save|FileKind::Export(_))&&path.is_dir(){self.record_file_error(&dialog.kind,&path,arranger_core::Error::new("FILE_DIRECTORY","The output path points to a directory"));dialog.error=self.message.clone();dialog.problems=vec![self.file_failure(&dialog.kind,&path)];dialog.replace=false;}
            else if matches!(dialog.kind,FileKind::Save|FileKind::Export(_))&&path.exists()&&!replace{dialog.replace=true;}
            else if self.finish_dialog_file(&dialog,path,replace){self.complete_file_dialog(dialog,ctx,true);return;}
            else{dialog.replace=false;dialog.error=self.message.clone();dialog.problems=vec![self.file_failure(&dialog.kind,&dialog.path())];}
        }
        self.dialog=Some(dialog);
    }
}
