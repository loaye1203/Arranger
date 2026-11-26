macro_rules! record_widget {
    ($widgets:expr, $key:expr, $value:expr) => {
        if cfg!(feature = "test-hooks") { $widgets.insert($key, $value); }
    };
}

mod editing;
mod files;
mod canvas;
mod nodes;
mod node_queries;
mod panels;
mod feedback;
mod text_edit;
mod workflow;
mod closing;
mod recent;
mod printing;
mod selection;
mod workspace;
mod shell;
mod workstation;
#[cfg(target_os="macos")] mod native_menu;
#[cfg(target_os="macos")] mod native_open;
#[cfg(target_os="macos")] mod native_files;
use crate::server::Server;
use arranger_core::{Command, Geometry, Service, Snapshot, command::{ExportFormat, ObjectPatch, Operation}, render::Renderer};
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, time::Duration};
use editing::{Editor,Tool};
use files::FileDialog;
use canvas::{View,Gesture,PenPoint};
use panels::button;

#[cfg(feature="test-hooks")]
#[derive(Debug)]
struct TestDroppedFile(PathBuf);
#[cfg(feature="test-hooks")]
impl egui::DroppedFile for TestDroppedFile {
    fn path(&self)->&std::path::Path{&self.0}
    fn bytes(&self)->Result<Vec<u8>,String>{std::fs::read(&self.0).map_err(|e|e.to_string())}
}

struct App {
    #[cfg(target_os="macos")]
    native_menu:native_menu::NativeMenu,
    #[cfg(target_os="macos")]
    native_open:native_open::NativeOpen,
    #[cfg(target_os="macos")]
    native_files:native_files::NativeFiles,
    pending_files:std::collections::VecDeque<PathBuf>,
    opened_cdrs:BTreeMap<PathBuf,String>,
    document_paths:BTreeMap<String,workflow::DocumentPath>,
    document_order:Vec<String>,
    command_focus:Option<workflow::CommandFocus>,
    command_bar:Rect,
    text_commands:Vec<workflow::TextCommand>,
    file_open_errors:Vec<String>,
    file_problem:Option<feedback::FileProblem>,
    recents:recent::Recents,
    service: Arc<Service>,
    stop: Arc<AtomicBool>,
    #[cfg_attr(not(feature="test-hooks"), allow(dead_code))]
    bridge: Arc<Mutex<crate::server::TestBridge>>,
    #[cfg_attr(not(feature="test-hooks"), allow(dead_code))]
    test_output: Option<PathBuf>,
    #[cfg(feature="test-hooks")]
    test_background: bool,
    #[cfg(feature="test-hooks")]
    test_hidden: bool,
    #[cfg(feature="test-hooks")]
    test_paste_requested: bool,
    #[cfg(feature="test-hooks")]
    test_input_serial:u64,
    #[cfg(feature="test-hooks")]
    test_hold_text_commands:bool,
    renderer: Renderer,
    selection_bounds:selection::SelectionBounds,
    object_rows:(std::sync::Weak<arranger_core::Document>,String,Arc<[usize]>),
    node_queries:std::cell::RefCell<node_queries::NodeQueries>,
    preview_cache:Option<(arranger_core::model::DocumentPreview,String)>,
    viewport_cache:Option<(String,arranger_core::render::ViewportProjection)>,
    #[cfg(feature="test-hooks")] performance:Value,
    render_notices:Vec<arranger_core::command::Notice>,notice_hash:String,show_notices:bool,
    font_feedback:Option<(String,feedback::FontFeedback)>,
    texture: Option<egui::TextureHandle>,
    texture_key: String,
    raster_key:String,
    raster_size:[u32;2],
    raster_region_mm:[f64;4],raster_density:f64,
    active: String,
    page: String,
    path: String,
    zoom: f32,
    fit:bool,
    pan: Vec2,
    view:View,
    selected: Option<String>,
    selection:Vec<String>,
    selection_mode:selection::Mode,
    rotation_center:Option<kurbo::Point>,
    editor: Option<Editor>,
    inline_text: Option<text_edit::InlineText>,
    text_field: Option<(egui::Id,Rect)>,
    editor_actions: Rect,
    text_actions: Rect,
    text_properties: Rect,
    gesture:Option<Gesture>,
    node_focus:Option<(usize,String)>,
    tool:Tool,pen:Vec<PenPoint>,default_style:arranger_core::Style,next_object:u64,ui_font:String,new_text_size:f64,new_text_layout:arranger_core::TextLayoutOptions,focus_text:bool,
    inspector_tab:usize,page_width:String,page_height:String,page_draft_key:String,page_draft_revision:u64,page_draft_dirty:bool,
    dialog:Option<FileDialog>,
    message: String,
    error: bool,
    next_document: u64,
    frames: u64,
    quit_prompt: bool,
    close_flow:Option<closing::CloseFlow>,
    close_focus:Option<closing::CloseFocus>,
    allow_quit: bool,
    chinese: bool,
    ribbon_expanded:[bool;8],
    node_snapping:bool,
    show_node_handles:bool,
    node_kind:String,
    node_link_target:Option<(String,usize)>,
}
const INK:Color32=Color32::from_rgb(24,24,26);
const MUTED:Color32=Color32::from_rgb(114,115,120);
const LINE:Color32=Color32::from_rgb(220,220,224);
const ACCENT:Color32=Color32::from_rgb(255,87,24);
fn tr<'a>(chinese:bool,zh:&'a str,en:&'a str)->&'a str {if chinese {zh}else{en}}
fn frame(fill:Color32,x:i8,y:i8)->egui::Frame {
    egui::Frame::new().fill(fill).inner_margin(egui::Margin{left:x,right:x,top:y,bottom:y})
}
fn readable_path(path:&str)->String {
    if let Some(unc)=path.strip_prefix("\\\\?\\UNC\\"){format!("\\\\{unc}")}
    else {path.strip_prefix("\\\\?\\").unwrap_or(path).into()}
}
fn title(path:Option<&str>,fallback:&str)->String {
    path.and_then(|p|std::path::Path::new(p).file_name()).map(|n|n.to_string_lossy().into_owned()).unwrap_or_else(||fallback.into())
}
fn configure_ui(ctx:&egui::Context)->bool {
    ctx.set_theme(egui::Theme::Light);
    let db=arranger_core::text::font_database();
    let face=["Microsoft YaHei UI","PingFang SC","Microsoft YaHei","Noto Sans CJK SC","Heiti SC","Songti SC"].iter().find_map(|family| {
        db.faces().find(|f|f.weight.0==400 && f.families.iter().any(|(name,_)|name==family)).map(|f|f.id)
    });
    let chinese=if let Some(font)=face.and_then(|id|db.with_face_data(id,|data,index|egui::FontData::from_blob(Arc::new(data.to_vec()),index))) {
        let mut fonts=egui::FontDefinitions::default();fonts.font_data.insert("arranger-system-ui".into(),Arc::new(font));
        fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0,"arranger-system-ui".into());
        fonts.families.entry(egui::FontFamily::Monospace).or_default().push("arranger-system-ui".into());
        let bold=db.faces().find(|f|f.weight.0>=600&&f.families.iter().any(|(name,_)|name=="PingFang SC"||name=="Microsoft YaHei UI"||name=="Microsoft YaHei")).and_then(|f|db.with_face_data(f.id,|data,index|egui::FontData::from_blob(Arc::new(data.to_vec()),index)));
        if let Some(bold)=bold{fonts.font_data.insert("arranger-ui-bold".into(),Arc::new(bold));fonts.families.insert(egui::FontFamily::Name("arranger-bold".into()),vec!["arranger-ui-bold".into()]);}
        else{let fallback=fonts.families[&egui::FontFamily::Proportional].clone();fonts.families.insert(egui::FontFamily::Name("arranger-bold".into()),fallback);}
        ctx.set_fonts(fonts);true
    }else{false};
    let mut style=(*ctx.style_of(egui::Theme::Light)).clone();
    style.text_styles.insert(egui::TextStyle::Body,egui::FontId::proportional(16.));
    style.text_styles.insert(egui::TextStyle::Button,egui::FontId::proportional(16.));
    style.text_styles.insert(egui::TextStyle::Small,egui::FontId::proportional(12.));
    style.text_styles.insert(egui::TextStyle::Heading,egui::FontId::proportional(20.));
    style.spacing.item_spacing=Vec2::new(7.,7.);style.spacing.button_padding=Vec2::new(9.,5.);style.spacing.interact_size.y=29.;
    style.visuals=egui::Visuals::light();style.visuals.override_text_color=Some(INK);style.visuals.panel_fill=Color32::WHITE;
    style.visuals.extreme_bg_color=Color32::from_rgb(248,248,249);style.visuals.text_edit_bg_color=Some(Color32::from_rgb(250,250,251));
    style.visuals.selection.bg_fill=ACCENT;style.visuals.selection.stroke=egui::Stroke::new(1.,Color32::WHITE);
    for state in [&mut style.visuals.widgets.inactive,&mut style.visuals.widgets.hovered,&mut style.visuals.widgets.active] {
        state.corner_radius=egui::CornerRadius::same(3);state.fg_stroke=egui::Stroke::new(1.,INK);
    }
    style.visuals.widgets.inactive.bg_fill=Color32::from_rgb(248,248,249);style.visuals.widgets.inactive.weak_bg_fill=Color32::from_rgb(248,248,249);
    style.visuals.widgets.inactive.bg_stroke=egui::Stroke::new(1.,LINE);
    style.visuals.widgets.hovered.bg_fill=Color32::from_rgb(255,238,230);style.visuals.widgets.hovered.weak_bg_fill=Color32::from_rgb(255,238,230);
    style.visuals.widgets.hovered.bg_stroke=egui::Stroke::new(1.,ACCENT);
    style.visuals.widgets.active.bg_fill=Color32::from_rgb(255,225,210);style.visuals.widgets.active.weak_bg_fill=Color32::from_rgb(255,225,210);
    ctx.set_style_of(egui::Theme::Light,style);chinese
}
fn message_text(message:&str,chinese:bool)->&str {
    if !chinese{return message;}
    match message {
        "Ready"=>"画布已就绪", "Edit applied"=>"对象已更新", "Document opened"=>"文档已打开",
        "Document saved"=>"文档已保存", "Page exported"=>"页面已导出", "Export complete"=>"导出已完成", "Image imported"=>"图片已导入", "New document"=>"已新建文档",
        "Undone"=>"已撤销", "Redone"=>"已重做", "Enter numeric millimetre values"=>"请输入有效的毫米数值",
        value if value.contains("OUTPUT_EXISTS")=>"目标文件已存在，请更换位置或明确允许覆盖",
        value if value.contains("REVISION_CONFLICT")=>"文档已被更新，请重新载入属性后再修改",
        value if value.contains("MINIMUM_NODES")=>"闭合路径至少保留三个节点，开放路径至少保留两个节点",
        value if value.starts_with("Exported.")=>"页面已导出，请查看字体或输出提示",
        _=>message,
    }
}
#[cfg(feature="test-hooks")]
fn ctx_resize(ctx:&egui::Context,width:f64,height:f64){ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(width.clamp(980.,3000.) as f32,height.clamp(700.,2000.) as f32)));}
fn rect(r: Rect) -> Value { json!([r.min.x,r.min.y,r.max.x,r.max.y]) }
impl App {
    fn act(&mut self, command: Command, success: &str) -> Option<Value> {
        self.file_problem=None;
        match self.service.command(command) {
            Ok(value) => {self.error=false;self.message=success.into();Some(value)}
            Err(error) => {self.error=true;self.message=error.to_string();None}
        }
    }
    fn batch(&mut self, revision:u64, operations:Vec<Operation>)->bool {
        let operations=workspace::placed(operations,&self.page);
        if self.act(Command::Batch {document_id:self.active.clone(),expected_revision:revision,unit:"mm".into(),operations},"Edit applied").is_some(){self.editor=None;true}else{false}
    }
}
impl eframe::App for App {
    fn logic(&mut self,ctx:&egui::Context,_frame:&mut eframe::Frame){
        // eframe skips ui() for minimized/occluded windows. Keep lifecycle
        // handling alive without forcing background rendering.
        // Input and local commands wake immediately; the low-rate fallback
        // keeps transaction expiry/lifecycle checks alive during true idle.
        let idle=Duration::from_secs(1);
        // Forced hidden test painting retains its established frame cadence;
        // ordinary windows and lifecycle tests use the production idle path.
        #[cfg(feature="test-hooks")]
        let idle=if self.test_output.is_some()&&!self.test_background{Duration::from_millis(33)}else{idle};
        ctx.request_repaint_after(idle);
        if let Ok(s)=self.service.snapshot(&self.active){self.recents.observe_saved(&s);}
        #[cfg(target_os="macos")] self.process_native_menu(ctx);
        self.recents.refresh_if_due();
        #[cfg(target_os="macos")] self.native_menu.sync_recents(&self.recents);
        self.process_text_commands(ctx);
        self.restore_close_focus(ctx);
        #[cfg(target_os="macos")] {let paths=self.native_open.take();if !paths.is_empty(){
            self.pending_files.extend(paths);ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }}
        if self.stop.load(Ordering::SeqCst){
            #[cfg(target_os="macos")] self.native_files.cancel();
            self.allow_quit=true;ctx.send_viewport_cmd(egui::ViewportCommand::Close);return;
        }
        self.reconcile_closed_document(ctx);
        #[cfg(target_os="macos")] if self.dialog.is_none(){self.native_files.take_reply();}
        if ctx.input(|i|i.viewport().close_requested())&&!self.allow_quit{
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.request_close(ctx,true);
        }
    }
    #[cfg(feature="test-hooks")]
    fn raw_input_hook(&mut self,_ctx:&egui::Context,input:&mut egui::RawInput) {
        if self.test_output.is_none() {return;}
        if self.test_hidden {
            // Native hidden tests receive only their explicit test input.
            // A foreground Owner review must not type into a background
            // reliability fixture. Preserve only a requested OS paste.
            input.events.retain(|event|matches!(event,egui::Event::Paste(_))&&self.test_paste_requested);
            if !input.events.is_empty(){self.test_paste_requested=false;}
            input.focused=true;
        }
        for e in std::mem::take(&mut self.bridge.lock().expect("test bridge").input) {
            self.test_input_serial=self.test_input_serial.max(e["__input_serial"].as_u64().unwrap_or(0));
            let pos=Pos2::new(e["x"].as_f64().unwrap_or(0.) as f32,e["y"].as_f64().unwrap_or(0.) as f32);
            let mac_cmd=cfg!(target_os="macos")&&(e["command"]==true||e["mac_cmd"]==true);
            let ctrl=e["ctrl"]==true||!cfg!(target_os="macos")&&e["command"]==true;
            let modifiers=egui::Modifiers{ctrl,mac_cmd,command:if cfg!(target_os="macos"){mac_cmd}else{ctrl},shift:e["shift"]==true,alt:e["alt"]==true};
            match e["kind"].as_str().unwrap_or("") {
                #[cfg(target_os="macos")]
                "native_menu"=>{if let Some(key)=e["key"].as_str(){self.native_menu.invoke(key);}},
                #[cfg(target_os="macos")]
                "native_file_panel"=>self.native_files.test_action(&e),
                "hold_text_commands"=>{self.test_hold_text_commands=e["value"]==true;},
                "move"=>input.events.push(egui::Event::PointerMoved(pos)),
                "drop_file"=>{if let Some(path)=e["path"].as_str(){input.dropped_files.push(Arc::new(TestDroppedFile(path.into())));}},
                "pointer_gone"=>input.events.push(egui::Event::PointerGone),
                "window_focused"=>{input.focused=e["value"]==true;input.viewports.entry(egui::ViewportId::ROOT).or_default().focused=Some(e["value"]==true);},
                "button"=>{input.events.push(egui::Event::ModifiersChanged(modifiers));input.events.push(egui::Event::PointerMoved(pos));input.events.push(egui::Event::PointerButton {pos,button:if e["secondary"]==true {egui::PointerButton::Secondary}else if e["middle"]==true{egui::PointerButton::Middle}else{egui::PointerButton::Primary},pressed:e["pressed"]==true,modifiers});}
                "text"=>input.events.push(egui::Event::Text(e["text"].as_str().unwrap_or("").into())),
                "paste"=>input.events.push(egui::Event::Paste(e["text"].as_str().unwrap_or("").into())),
                "ime_preedit"=>input.events.push(egui::Event::Ime(egui::ImeEvent::Preedit{text:e["text"].as_str().unwrap_or("").into(),active_range_chars:None})),
                "ime_commit"=>input.events.push(egui::Event::Ime(egui::ImeEvent::Commit(e["text"].as_str().unwrap_or("").into()))),
                "zoom"=>{input.events.push(egui::Event::PointerMoved(pos));input.events.push(egui::Event::Zoom(e["factor"].as_f64().unwrap_or(1.) as f32));},
                "scroll"=>{
                    let unit=match e["unit"].as_str(){Some("line")=>egui::MouseWheelUnit::Line,Some("page")=>egui::MouseWheelUnit::Page,_=>egui::MouseWheelUnit::Point};
                    let source=match e["source"].as_str(){Some("trackpad")=>egui::MouseWheelSource::Trackpad,Some("momentum")=>egui::MouseWheelSource::Momentum,Some("wheel")=>egui::MouseWheelSource::Wheel,_=>egui::MouseWheelSource::Unknown};
                    let phase=match e["phase"].as_str(){Some("start")=>egui::TouchPhase::Start,Some("end")=>egui::TouchPhase::End,_=>egui::TouchPhase::Move};
                    input.events.push(egui::Event::PointerMoved(pos));input.events.push(egui::Event::MouseWheel{unit,delta:Vec2::new(e["dx"].as_f64().unwrap_or(0.) as f32,e["dy"].as_f64().unwrap_or(0.) as f32),phase,source,modifiers});
                },
                "copy"=>input.events.push(egui::Event::Copy),"cut"=>input.events.push(egui::Event::Cut),
                "key"=>{if let Some(key)=e["key"].as_str().and_then(egui::Key::from_name){input.events.push(egui::Event::ModifiersChanged(modifiers));let edges=e["pressed"].as_bool().map_or_else(||vec![true,false],|p|vec![p]);for pressed in edges{input.events.push(egui::Event::Key{key,physical_key:None,pressed,repeat:false,modifiers});}input.events.push(egui::Event::ModifiersChanged(Default::default()));}},
                "pixels_per_point"=>{if let Some(value)=e["value"].as_f64(){if value.is_finite()&&(0.5..=4.).contains(&value){_ctx.set_pixels_per_point(value as f32);}}},
                "resize"=>{if let (Some(w),Some(h))=(e["width"].as_f64(),e["height"].as_f64()){if w.is_finite()&&h.is_finite(){ctx_resize(_ctx,w,h);}}}
                "window_minimized"=>_ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(e["value"]==true)),
                "window_visible"=>_ctx.send_viewport_cmd(egui::ViewportCommand::Visible(e["value"]==true)),
                "close_request"=>input.viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close),
                "select_all"=>{
                    let modifiers=egui::Modifiers {ctrl:!cfg!(target_os="macos"),mac_cmd:cfg!(target_os="macos"),command:true,..Default::default()};
                    input.events.push(egui::Event::Key {key:egui::Key::A,physical_key:None,pressed:true,repeat:false,modifiers});
                    input.events.push(egui::Event::Key {key:egui::Key::A,physical_key:None,pressed:false,repeat:false,modifiers});
                }
                _=>{}
            }
        }
        let mut bridge=self.bridge.lock().expect("test bridge");
        if bridge.status["frame"].is_u64(){bridge.status["window"]=json!({"minimized":input.viewport().minimized,"occluded":input.viewport().occluded,"focused":input.viewport().focused});}
    }
    fn ui(&mut self,ui:&mut egui::Ui,_frame:&mut eframe::Frame){
        #[cfg(feature="test-hooks")]let frame_started=std::time::Instant::now();
        #[cfg(feature="test-hooks")]let prior_renders=self.performance["renders"].as_u64().unwrap_or(0);
        self.frames+=1;let ctx=ui.ctx().clone();
        #[cfg(feature="test-hooks")] if self.test_output.is_some()&&!self.test_background{ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaintWhileHidden);}
        if self.stop.load(Ordering::SeqCst){return;}
        self.pending_files.extend(ctx.input_mut(|i|std::mem::take(&mut i.raw.dropped_files)).into_iter().map(|f|f.path().to_path_buf()));
        let snapshot=match self.service.snapshot(&self.active){Ok(s)=>s,Err(error)=>{ui.label(error.to_string());return;}};
        if snapshot.document.page(&self.page).is_err(){self.page=snapshot.document.pages[0].id.clone();self.page_draft_dirty=false;}
        let retained=self.selection.iter().filter(|id|snapshot.document.object(id).is_ok_and(|o|o.on_canvas(&self.page))).cloned().collect::<Vec<_>>();if retained!=self.selection{self.set_selection(retained);}
        self.text_boundary(&ctx,&snapshot);
        let snapshot=match self.service.snapshot(&self.active){Ok(s)=>s,Err(_)=>return,};
        self.keyboard(&ctx,&snapshot);
        let snapshot=match self.service.snapshot(&self.active){Ok(s)=>s,Err(_)=>return,};let zh=self.chinese;let mut widgets=BTreeMap::new();
        let _header=self.header(ui,&snapshot,&mut widgets);
        let _toolbar=self.toolbar(ui,&snapshot,&mut widgets);
        if !self.pending_files.is_empty(){egui::Panel::top("pending-files").show(ui,|ui|{ui.horizontal(|ui|{
            ui.label(format!("{} {} · {}",tr(zh,"待打开","Waiting to open"),self.pending_files.len(),self.file_open_blocker(&snapshot).unwrap_or("")));
            if button(ui,&mut widgets,"cancel_pending_files",tr(zh,"取消打开","Cancel opening"),true){self.pending_files.clear();}
        });});}
        let _footer=self.footer(ui,&snapshot,&mut widgets);
        let _rail=self.rail(ui,&snapshot,&mut widgets);
        #[cfg(feature="test-hooks")]let inspector_started=std::time::Instant::now();
        let _inspector=egui::Panel::right("inspector").exact_size(336.).resizable(false).frame(frame(Color32::WHITE,12,12)).show(ui,|ui|{
            // Fractional-DPI text fields may slightly overflow their layout.
            // Keep that content from resizing the fixed panel and the canvas.
            let bounds=ui.available_rect_before_wrap();
            let mut content=ui.new_child(egui::UiBuilder::new().id_salt("inspector-content").max_rect(bounds));
            self.inspector(&mut content,&snapshot,&mut widgets);
            ui.allocate_rect(bounds,egui::Sense::hover());
        }).response.rect;
        #[cfg(feature="test-hooks")] {self.performance["inspector_ms"]=json!(inspector_started.elapsed().as_secs_f64()*1000.);}
        // An action above can add/remove a page or switch documents. The
        // canvas must use the resulting snapshot in this same frame.
        let snapshot=self.service.snapshot(&self.active).unwrap_or(snapshot);
        let doc=&snapshot.document;
        if doc.page(&self.page).is_err(){self.page=doc.pages[0].id.clone();}
        let _view=egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui,|ui|self.work_canvas(ui,&snapshot,&mut widgets)).inner;
        self.show_file_dialog(&ctx,&mut widgets);
        if self.show_notices {
            let mut open=true;let mut close=false;
            egui::Window::new(tr(zh,"文档提示","Document notices")).open(&mut open).default_width(480.).show(&ctx,|ui|{
                if self.notice_hash!=snapshot.document_hash||self.render_notices.is_empty(){ui.label(tr(zh,"当前文档没有字体或排版提示。","No current font or layout notices."));}
                else{egui::ScrollArea::vertical().max_height(400.).show(ui,|ui|{for notice in &self.render_notices{ui.label(feedback::notice_text(notice,zh)).on_hover_text(&notice.message);ui.add_space(8.);}});}
                close=button(ui,&mut widgets,"notices_close",tr(zh,"关闭","Close"),true);
            });self.show_notices=open&&!close;
        }
        self.show_close_dialog(&ctx,&mut widgets);
        // Apply document events after this frame's text/IME/pointer input.
        // A pending composition, modal or uncommitted numeric draft defers
        // the switch; the queued file never discards that work.
        self.process_pending_files(&ctx);
        #[cfg(target_os="macos")] if let Ok(s)=self.service.snapshot(&self.active){self.native_menu.sync(&s,&self.selection,self.text_command_target(&ctx).is_some(),self.dialog.is_some()||self.quit_prompt,self.native_files.active());}
        #[cfg(feature="test-hooks")]
        if let Some(out)=&self.test_output{
            self.performance["frame_ms"]=json!(frame_started.elapsed().as_secs_f64()*1000.);
            if self.performance["renders"].as_u64().unwrap_or(0)!=prior_renders{self.performance["raster_frame_ms"]=self.performance["frame_ms"].clone();self.performance["raster_inspector_ms"]=self.performance["inspector_ms"].clone();}
            let mut status=json!({"frame":self.frames,"input_serial":self.test_input_serial,"document_id":snapshot.document_id,"revision":snapshot.revision,"document_hash":snapshot.document_hash,"texture_key":self.texture_key,"raster_key":self.raster_key,"raster_size":self.raster_size,"raster_region_mm":self.raster_region_mm,"raster_density":self.raster_density,"pixels_per_point":ctx.pixels_per_point(),"objects":doc.objects.len(),"selected":self.selected,"selection":self.selection,"selection_mode":format!("{:?}",self.selection_mode),"rotation_center_mm":self.rotation_center.map(|p|[p.x,p.y]),"gesture":self.gesture.as_ref().map(|g|match g.kind{canvas::GestureKind::Move=>"move",canvas::GestureKind::Resize(_)=>"resize",canvas::GestureKind::Rotate{..}=>"rotate",canvas::GestureKind::Pivot{..}=>"pivot",_=>"other"}),"tool":format!("{:?}",self.tool),
                "file_path":self.path,"dialog":self.dialog.as_ref().map(|d|json!({"kind":format!("{:?}",d.kind),"choose_format":d.choose_format,"details":d.details,"directory":d.directory,"name":d.name,"replace":d.replace,"error":d.error,"cdr_mode":d.cdr_mode,"cdr_preflight":d.cdr_preflight,"pdf":d.pdf.as_ref().map(|p|p.status()),"recover_source":d.recover_source,"recover_backup":d.recover_backup})),"editor":self.editor.as_ref().map(|e|json!({"revision":e.revision,"x":e.x,"y":e.y,"width":e.width,"height":e.height,"rotation":e.rotation,"dirty":e.dirty,"text_content":if let Geometry::Text{content,..}=&e.geometry{Some(content)}else{None},"font_family":if let Geometry::Text{font_family,..}=&e.geometry{Some(font_family)}else{None}})),"font_feedback":self.font_feedback.as_ref().map(|(_,info)|info),"notices":if self.notice_hash==snapshot.document_hash{json!(self.render_notices)}else{json!([])},"notices_open":self.show_notices,"quit_prompt":self.quit_prompt,"ui_language":if zh{"zh-CN"}else{"en"},"layout":{"header":rect(_header),"toolbar":rect(_toolbar),"rail":rect(_rail),"inspector":rect(_inspector),"footer":rect(_footer)},"page_id":self.page,"zoom":self.zoom,"fit":self.fit,"pan":[self.pan.x,self.pan.y],"scale":_view.scale,"origin":[_view.origin.x,_view.origin.y],"canvas":rect(_view.canvas),"widgets":widgets,"busy":snapshot.busy,"dirty":snapshot.dirty,"message":self.message,"error":self.error});
            status["performance"]=self.performance.clone();
            status["status_text"]=json!(self.status_text());
            status["file_problem"]=self.file_problem.as_ref().map(|p|json!({"code":p.code,"summary":p.summary,"hint":p.hint,"path":p.path,"raw":p.raw})).unwrap_or(Value::Null);
            if let Some(d)=&self.dialog{status["dialog"]["problems"]=json!(d.problems.iter().map(|p|json!({"code":p.code,"summary":p.summary,"hint":p.hint,"path":p.path,"raw":p.raw})).collect::<Vec<_>>());status["dialog"]["copy_requested"]=json!(d.problems.len()==1&&d.problems[0].can_copy());status["dialog"]["all_pages"]=json!(d.all_pages);}
            status["ribbon_expanded"]=json!(self.ribbon_expanded[self.tool as usize]);status["node_snapping"]=json!(self.node_snapping);status["safe_native_titlebar"]=json!(true);status["native_window_rects"]=ctx.input(|i|json!({"inner":i.viewport().inner_rect.map(rect),"outer":i.viewport().outer_rect.map(rect),"fullscreen":i.viewport().fullscreen}));
            status["text_composing"]=json!(self.inline_text.as_ref().is_some_and(text_edit::InlineText::composing));status["page_draft"]=json!({"dirty":self.page_draft_dirty,"width":self.page_width,"height":self.page_height});
            status["recents"]=self.recents.status();status["close_request"]=self.close_flow.as_ref().map_or(Value::Null,closing::CloseFlow::status);status["pending_files"]=json!(self.pending_files);status["file_open_errors"]=json!(self.file_open_errors);
            #[cfg(target_os="macos")] { status["native_menu"]=self.native_menu.status(); }
            #[cfg(target_os="macos")] {status["native_file_panel"]=self.native_files.status();status["file_backend"]=json!(if self.use_native_files(){"AppKit"}else{"portable-test"});}
            let captures={let mut bridge=self.bridge.lock().expect("test bridge");bridge.status=status.clone();std::mem::take(&mut bridge.capture)};
            for label in captures{let out=out.clone();let status=status.clone();ctx.request_screenshot(move|image|{let Some(mut p)=arranger_core::render::Pixmap::new(image.size[0] as u32,image.size[1] as u32)else{return;};for (dest,source) in p.data_mut().chunks_exact_mut(4).zip(&image.pixels){dest.copy_from_slice(&source.to_array());}let _=p.save_png(out.join(format!("{label}.png")));let _=std::fs::write(out.join(format!("{label}.json")),serde_json::to_vec_pretty(&status).expect("status serializes"));});}
        }
    }
}
pub fn run(service:Arc<Service>,server:&Server,test_output:Option<PathBuf>)->Result<(),Box<dyn std::error::Error>>{
    let opened=std::env::args().collect::<Vec<_>>().windows(2).find(|a|a[0]=="--open").map(|a|PathBuf::from(&a[1]));
    let mut recents=recent::Recents::for_app(test_output.as_deref());if let Some(path)=&opened{recents.remember(path);}
    let path=readable_path(&service.snapshot("main")?.saved_path.unwrap_or_else(||opened.as_ref().map(|p|if files::is_source_backup(p){files::recovery_copy_path(p)}else{p.with_extension("arranger")}).map(|p|p.to_string_lossy().into_owned()).unwrap_or_else(||"未命名.arranger".into())));let visible=!std::env::args().any(|s|s=="--hidden");if !visible&&test_output.is_none(){return Err("--hidden is available only with --test-output".into());}
    let options=eframe::NativeOptions{viewport:egui::ViewportBuilder::default().with_inner_size([1440.,980.]).with_decorations(true).with_fullsize_content_view(false).with_titlebar_shown(true).with_titlebar_buttons_shown(true).with_min_inner_size([980.,700.]).with_visible(visible).with_active(false),renderer:eframe::Renderer::Glow,..Default::default()};let (stop,bridge)=(server.stop.clone(),server.bridge.clone());
    let wakeup=server.wakeup.clone();
    #[cfg(target_os="macos")] let native_open=native_open::NativeOpen::new();
    let result=eframe::run_native("Arranger",options,Box::new(move|cc|{let chinese=configure_ui(&cc.egui_ctx);let context=cc.egui_ctx.clone();wakeup.set(move||context.request_repaint());Ok(Box::new(App{
        #[cfg(target_os="macos")] native_menu:native_menu::NativeMenu::new(chinese,cc.egui_ctx.clone()),
        #[cfg(target_os="macos")] native_open:{native_open.attach(cc.egui_ctx.clone());native_open},
        #[cfg(target_os="macos")] native_files:native_files::NativeFiles::new(cc.egui_ctx.clone()),
        pending_files:Default::default(),opened_cdrs:{let args:Vec<_>=std::env::args().collect();args.windows(2).find(|a|a[0]=="--open").map(|a|PathBuf::from(&a[1])).filter(|p|p.extension().is_some_and(|s|s.eq_ignore_ascii_case("cdr"))).and_then(|p|p.canonicalize().ok()).map(|p|BTreeMap::from([(p,"main".into())])).unwrap_or_default()},file_open_errors:vec![],file_problem:None,recents,
        #[cfg(feature="test-hooks")]
        test_background:test_output.is_some()&&std::env::args().any(|a|a=="--test-background"),
        #[cfg(feature="test-hooks")]
        test_hidden:!visible,
        #[cfg(feature="test-hooks")]
        test_paste_requested:false,
        #[cfg(feature="test-hooks")]
        test_input_serial:0,
        #[cfg(feature="test-hooks")]
        test_hold_text_commands:false,
        service,stop,bridge,test_output,renderer:Renderer::new(),selection_bounds:Default::default(),object_rows:Default::default(),node_queries:Default::default(),preview_cache:None,viewport_cache:None,#[cfg(feature="test-hooks")] performance:json!({"renders":0,"render_ms":0.,"frame_ms":0.,"inspector_ms":0.}),render_notices:vec![],notice_hash:String::new(),show_notices:false,font_feedback:None,texture:None,texture_key:String::new(),raster_key:String::new(),raster_size:[0,0],raster_region_mm:[0.;4],raster_density:0.,document_paths:BTreeMap::new(),document_order:vec![],command_focus:None,command_bar:Rect::NOTHING,text_commands:vec![],active:"main".into(),page:"page-1".into(),path,zoom:1.,fit:true,pan:Vec2::ZERO,view:View::default(),selected:None,selection:vec![],selection_mode:Default::default(),rotation_center:None,editor:None,inline_text:None,text_field:None,editor_actions:Rect::NOTHING,text_actions:Rect::NOTHING,text_properties:Rect::NOTHING,gesture:None,node_focus:None,
        tool:Tool::Select,pen:vec![],default_style:arranger_core::Style{fill:"#3976e8".into(),..Default::default()},next_object:0,ui_font:if chinese{if cfg!(target_os="macos"){"PingFang SC"}else{"Microsoft YaHei"}}else{"sans-serif"}.into(),new_text_size:5.,new_text_layout:Default::default(),focus_text:false,inspector_tab:0,page_width:"210".into(),page_height:"297".into(),page_draft_key:String::new(),page_draft_revision:0,page_draft_dirty:false,dialog:None,message:"Ready".into(),error:false,next_document:1,frames:0,quit_prompt:false,close_flow:None,close_focus:None,allow_quit:false,chinese,ribbon_expanded:[false;8],node_snapping:true,show_node_handles:true,node_kind:"cusp".into(),node_link_target:None,
    }))}));server.wakeup.clear();result?;Ok(())
}
