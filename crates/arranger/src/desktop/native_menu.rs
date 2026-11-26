//! macOS owns the global menu bar, its notch layout and standard system items.
//! Targets only queue actions; App executes them with its current snapshot.
use super::*;
use std::cell::RefCell;
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadOnly};
use objc2::rc::Retained;
use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem};
use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol, NSString};

#[derive(Default)]
struct TargetIvars { actions: RefCell<Vec<isize>>, context:egui::Context }
define_class!(
    // SAFETY: NSObject has no subclass invariants. The target is main-thread
    // only, its ivars contain no borrowed pointers and are not manually freed.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = TargetIvars]
    struct ArrangerMenuTarget;
    unsafe impl NSObjectProtocol for ArrangerMenuTarget {}
    impl ArrangerMenuTarget {
        #[unsafe(method(arrangerMenuAction:))]
        fn action(&self, sender: &NSMenuItem) {
            self.ivars().actions.borrow_mut().push(sender.tag());
            self.ivars().context.request_repaint();
        }
    }
);

#[cfg_attr(not(feature="test-hooks"), allow(dead_code))]
struct Item { key: String, group: &'static str, item: Retained<NSMenuItem>, menu: Retained<NSMenu>, index: isize }
pub(super) struct NativeMenu {
    root: Retained<NSMenu>,
    original: Option<Retained<NSMenu>>,
    // NSMenuItem's target is weak. Keep it alive as long as every menu item.
    target: Retained<ArrangerMenuTarget>,
    items: Vec<Item>,
    groups: Vec<(&'static str, String)>,
    recent_menu:Option<Retained<NSMenu>>,static_len:usize,next_tag:isize,recent_generation:Option<u64>,
}
impl NativeMenu {
    pub(super) fn new(zh: bool, context:egui::Context) -> Self {
        let mtm = MainThreadMarker::new().expect("AppKit menus require the main thread");
        let target = unsafe { msg_send![super(ArrangerMenuTarget::alloc(mtm).set_ivars(TargetIvars{context,..Default::default()})), init] };
        let app = NSApplication::sharedApplication(mtm);
        let mut this = Self { root: NSMenu::new(mtm), original:app.mainMenu(), target, items: vec![], groups: vec![],recent_menu:None,static_len:0,next_tag:0,recent_generation:None };
        this.root.setAutoenablesItems(false);
        let app_menu = this.group("app", "Arranger", mtm);
        this.system(&app_menu, tr(zh,"关于 Arranger","About Arranger"), sel!(orderFrontStandardAboutPanel:), "", false, mtm);
        app_menu.addItem(&NSMenuItem::separatorItem(mtm));
        let services = NSMenu::new(mtm);
        let item = NSMenuItem::new(mtm); item.setTitle(&NSString::from_str(tr(zh,"服务","Services"))); item.setSubmenu(Some(&services)); app_menu.addItem(&item);
        // SAFETY: Services is a live NSMenu retained by its parent and AppKit.
        app.setServicesMenu(Some(&services));
        this.system(&app_menu,tr(zh,"隐藏 Arranger","Hide Arranger"),sel!(hide:),"h",false,mtm);
        this.system(&app_menu,tr(zh,"显示全部","Show All"),sel!(unhideAllApplications:),"",false,mtm);
        app_menu.addItem(&NSMenuItem::separatorItem(mtm));
        this.add(&app_menu,"app","quit",tr(zh,"退出 Arranger","Quit Arranger"),"q",false,mtm);
        for (group,z,en) in [("file","文件","File"),("edit","编辑","Edit"),("view","视图","View"),("layout","布局","Layout"),("object","对象","Object"),("path","路径","Path"),("export","导出","Export"),("text","文本","Text"),("tools","工具","Tools"),("window","窗口","Window"),("help","帮助","Help")] {
            let menu=this.group(group,tr(zh,z,en),mtm);
            let specs: &[(&str,&str,&str,&str,bool)] = match group {
                "file"=> &[("new","新建","New","n",false),("open","打开…","Open…","o",false),("recover_source","从原稿备份恢复…","Recover Source Backup…","",false),("recover_cdr","从 CDR 备份恢复…","Recover CDR Backup…","",false),("save","保存","Save","s",false),("save_as","另存为…","Save As…","s",true),("print","打印／预览…","Print / Preview…","p",false),("close_window","关闭当前文档","Close Document","w",false)],
                "edit"=> &[("undo","撤销","Undo","z",false),("redo","重做","Redo","z",true),("ctx-cut","剪切","Cut","x",false),("ctx-copy","复制","Copy","c",false),("ctx-paste","粘贴","Paste","v",false),("ctx-all","全选","Select All","a",false),("ctx-duplicate","再制一份","Duplicate","d",false),("ctx-delete","删除","Delete","",false)],
                "view"=> &[("fit","适合页面","Fit Page","",false),("actual_size","实际大小","Actual Size","",false),("zoom_in","放大","Zoom In","+",false),("zoom_out","缩小","Zoom Out","-",false)],
                "layout"=> &[("page_settings","页面设置","Page Setup","",false),("page_add","添加A4页面","Add A4 Page","",false),("page_delete","删除当前页","Delete Current Page","",false)],
                "object"=> &[("ctx-group","编组","Group","g",false),("ctx-ungroup","解组","Ungroup","g",true),("ctx-front","置于顶层","Bring to Front","",false),("ctx-back","置于底层","Send to Back","",false),("ctx-lock","锁定","Lock","",false),("ctx-unlock","解锁","Unlock","",false),("ctx-hide","隐藏对象","Hide Object","",false),("ctx-show","显示对象","Show Object","",false)],
                "path"=> &[("to_curves","转为曲线","Convert to Curves","o",true),("to_path","形状转路径","Convert to Path","",false),("union","并集","Union","",false),("intersection","交集","Intersection","",false),("difference","差集","Difference","",false),("xor","异或","Xor","",false)],
                "export"=> &[("svg","SVG…","SVG…","",false),("png","PNG…","PNG…","",false),("pdf","PDF…","PDF…","",false),("cdr14","CDR14…","CDR14…","",false),("cdr21","CDR21…","CDR21…","",false),("cdr14_appearance","CDR14 保外观副本…","CDR14 Appearance Copy…","",false),("cdr21_appearance","CDR21 保外观副本…","CDR21 Appearance Copy…","",false)],
                "text"=> &[("menu-text-tool","文字工具","Text Tool","",false)],
                "help"=> &[("help","Arranger 使用提示","Arranger Help","",false)],
                _=> &[],
            };
            for &(key,z,en,shortcut,shift) in specs { this.add(&menu,group,key,tr(zh,z,en),shortcut,shift,mtm);
                if group=="file"&&key=="open"{let recent=NSMenu::new(mtm);recent.setAutoenablesItems(false);let parent=NSMenuItem::new(mtm);parent.setTitle(&NSString::from_str(tr(zh,"最近文档","Recent Documents")));parent.setSubmenu(Some(&recent));menu.addItem(&parent);this.recent_menu=Some(recent);}
            }
            if group=="tools" {for (_,key,z,en) in Tool::items(){this.add(&menu,group,&format!("menu-tool-{key}"),tr(zh,z,en),"",false,mtm);}}
            if group=="help" { app.setHelpMenu(Some(&menu)); }
            if group=="window" {
                this.system(&menu,tr(zh,"最小化","Minimize"),sel!(performMiniaturize:),"m",false,mtm);
                this.system(&menu,tr(zh,"缩放窗口","Zoom Window"),sel!(performZoom:),"",false,mtm);
                this.system(&menu,tr(zh,"进入／退出全屏","Enter / Exit Full Screen"),sel!(toggleFullScreen:),"",false,mtm);
                app.setWindowsMenu(Some(&menu));
            }
        }
        this.static_len=this.items.len();app.setMainMenu(Some(&this.root));
        this
    }
    fn group(&mut self,key:&'static str,title:&str,mtm:MainThreadMarker)->Retained<NSMenu>{
        let menu=NSMenu::new(mtm); menu.setTitle(&NSString::from_str(title));menu.setAutoenablesItems(false);
        let item=NSMenuItem::new(mtm);item.setTitle(&NSString::from_str(title));item.setSubmenu(Some(&menu));self.root.addItem(&item);self.groups.push((key,title.into()));menu
    }
    fn add(&mut self,menu:&Retained<NSMenu>,group:&'static str,key:&str,title:&str,shortcut:&str,shift:bool,mtm:MainThreadMarker){
        // SAFETY: Selector signature is (id, SEL, NSMenuItem*) -> void and
        // matches ArrangerMenuTarget::action. Self retains its weak target.
        let item=unsafe{NSMenuItem::initWithTitle_action_keyEquivalent(mtm.alloc(),&NSString::from_str(title),Some(sel!(arrangerMenuAction:)),&NSString::from_str(shortcut))};
        item.setTag(self.next_tag);self.next_tag+=1;unsafe{item.setTarget(Some(&self.target));}
        item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command|if shift{NSEventModifierFlags::Shift}else{NSEventModifierFlags::empty()});
        let index=menu.numberOfItems();menu.addItem(&item);self.items.push(Item{key:key.into(),group,item,menu:menu.clone(),index});
    }
    fn system(&self,menu:&NSMenu,title:&str,action:objc2::runtime::Sel,key:&str,shift:bool,mtm:MainThreadMarker){
        // SAFETY: These selectors are the standard NSApplication/NSWindow
        // responder-chain actions. A nil target lets AppKit find the owner.
        let item=unsafe{NSMenuItem::initWithTitle_action_keyEquivalent(mtm.alloc(),&NSString::from_str(title),Some(action),&NSString::from_str(key))};
        item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command|if shift{NSEventModifierFlags::Shift}else{NSEventModifierFlags::empty()});menu.addItem(&item);
    }
    pub(super) fn take_actions(&self)->Vec<String>{
        std::mem::take(&mut *self.target.ivars().actions.borrow_mut()).into_iter().filter_map(|tag|self.items.iter().find(|v|v.item.tag()==tag).map(|v|v.key.clone())).collect()
    }
    pub(super) fn sync_recents(&mut self,recents:&recent::Recents){
        if self.recent_generation==Some(recents.generation){return;}self.recent_generation=Some(recents.generation);
        let Some(menu)=self.recent_menu.clone()else{return;};menu.removeAllItems();self.items.truncate(self.static_len);
        let mtm=self.root.mtm();let zh=self.groups.iter().any(|(key,title)|*key=="file"&&title=="文件");
        if recents.entries.is_empty(){let item=NSMenuItem::new(mtm);item.setTitle(&NSString::from_str(tr(zh,"暂无最近文档","No recent documents")));item.setEnabled(false);menu.addItem(&item);}
        for entry in &recents.entries{let label=format!("{}{}",entry.label,if entry.exists{""}else{tr(zh,"（找不到文件）"," (missing)")});self.add(&menu,"file",&entry.key,&label,"",false,mtm);self.items.last().unwrap().item.setToolTip(Some(&NSString::from_str(&entry.path.to_string_lossy())));}
        if recents.warning.is_some(){let item=NSMenuItem::new(mtm);item.setTitle(&NSString::from_str(tr(zh,"最近记录未能读写；文档操作不受影响","Recent history unavailable; documents unaffected")));item.setEnabled(false);menu.addItem(&item);}
        menu.addItem(&NSMenuItem::separatorItem(mtm));self.add(&menu,"file","recent_clear",tr(zh,"清空最近记录","Clear Recent Documents"),"",false,mtm);
    }
    pub(super) fn sync(&self,s:&Snapshot,selection:&[String],text_focus:bool,modal:bool,native_sheet:bool){
        let selected=selection.len();let group=selection.iter().any(|id|s.document.object(id).is_ok_and(|o|matches!(o.geometry,Geometry::Group)));
        for entry in &self.items {let key=entry.key.as_str();
            let responder_action=if native_sheet{match key{"ctx-cut"=>Some(sel!(cut:)),"ctx-copy"=>Some(sel!(copy:)),"ctx-paste"=>Some(sel!(paste:)),"ctx-all"=>Some(sel!(selectAll:)),"undo"=>Some(sel!(undo:)),"redo"=>Some(sel!(redo:)),_=>None}}else{None};
            // Sheet filename editing belongs to AppKit's responder chain,
            // never to the canvas selection or document undo stack.
            unsafe{entry.item.setTarget(if responder_action.is_some(){None}else{Some(&self.target)});entry.item.setAction(Some(responder_action.unwrap_or(sel!(arrangerMenuAction:))));}
            let enabled=match key{
            _ if responder_action.is_some()=>true,
            "quit" if native_sheet=>false,
            "quit"|"close_window"=>true,
            _ if modal=>false,
            "undo"=>!s.busy&&(text_focus||s.undo_depth>0),"redo"=>!s.busy&&(text_focus||s.redo_depth>0),
            "ctx-copy"|"ctx-cut"|"ctx-all"=>!s.busy&&(text_focus||key=="ctx-all"||selected>0),
            "ctx-group"=>!s.busy&&selected>1,
            "ctx-ungroup"=>!s.busy&&group&&!text_focus,
            "ctx-paste"=>!s.busy,
            "page_delete"=>!s.busy&&s.document.pages.len()>1,
            "union"|"intersection"|"difference"|"xor"=>!s.busy&&selected==2&&!text_focus,
            "to_curves"|"to_path"=>!s.busy&&selected>0&&!text_focus,
            _ if key.starts_with("ctx-")=>!s.busy&&selected>0&&!text_focus,
            _=>!s.busy,
        };if entry.item.isEnabled()!=enabled{entry.item.setEnabled(enabled);}}
    }
    #[cfg(feature="test-hooks")]
    pub(super) fn invoke(&self,key:&str)->bool{
        let Some(entry)=self.items.iter().find(|v|v.key==key)else{return false;};
        if !entry.item.isEnabled(){return false;}
        // This exercises the actual AppKit menu's target/action path, rather
        // than calling App's command dispatch directly from the test hook.
        entry.menu.performActionForItemAtIndex(entry.index);true
    }
    #[cfg(feature="test-hooks")]
    pub(super) fn status(&self)->Value{
        let app=NSApplication::sharedApplication(self.root.mtm());
        json!({"installed":app.mainMenu().is_some_and(|m|*m==*self.root),"groups":self.groups.iter().map(|(key,title)|json!({"key":key,"title":title})).collect::<Vec<_>>(),"items":self.items.iter().map(|v|json!({"key":v.key,"group":v.group,"title":v.item.title().to_string(),"enabled":v.item.isEnabled(),"shortcut":v.item.keyEquivalent().to_string(),"modifier_mask":v.item.keyEquivalentModifierMask().bits(),"action":v.item.action().map(|a|a.name().to_string_lossy().into_owned())})).collect::<Vec<_>>()})
    }
}

impl App {
    pub(super) fn process_native_menu(&mut self,ctx:&egui::Context){
        for key in self.native_menu.take_actions(){
            if self.native_files.active(){if key=="close_window"{self.native_files.cancel();}continue;}
            if key=="close_window"{if let Some(dialog)=self.dialog.take(){self.resume_file_dialog(dialog,ctx);continue;}}
            let Ok(s)=self.service.snapshot(&self.active)else{continue;};
            if matches!(key.as_str(),"quit"|"close_window"){
                self.request_close(ctx,key=="quit");continue;
            }
            if self.dialog.is_some()||self.quit_prompt{continue;}
            if self.recent_action(&key){continue;}
            match key.as_str(){
                "new"|"open"|"recover_cdr"|"recover_source"|"save"|"save_as"|"print"|"svg"|"png"|"pdf"|"cdr14"|"cdr21"|"cdr14_appearance"|"cdr21_appearance"=>self.file_action(&s,&key),
                "page_settings"=>{if self.finish_text(ctx,&s){self.inspector_tab=1;}},
                "fit"=>{self.fit=true;self.pan=Vec2::ZERO;},"actual_size"=>{self.fit=false;self.zoom=1.;self.pan=Vec2::ZERO;},
                "zoom_in"=>self.zoom_by(1.25,None,&s.document),"zoom_out"=>self.zoom_by(0.8,None,&s.document),
                "menu-text-tool"=>{if self.finish_text(ctx,&s){self.tool=Tool::Text;}},
                "help"=>{self.message=tr(self.chinese,"V 选择 · A 节点 · T 文字 · 双击边缘加点／节点删点 · ⌘S 保存 · ⌘⇧O 转曲","V Select · A Nodes · T Text · double-click edge to add/node to delete · Cmd+S Save · Cmd+Shift+O Curves").into();self.error=false;},
                _ if key.starts_with("menu-tool-")=>{if let Some((tool,_,_,_))=Tool::items().into_iter().find(|(_,name,_,_)|key==format!("menu-tool-{name}")){if self.finish_text(ctx,&s){self.tool=tool;self.inspector_tab=0;self.gesture=None;}}},
                _=>{self.edit_action(ctx,&s,key.strip_prefix("ctx-").unwrap_or(&key));},
            }
        }
    }
}

impl Drop for NativeMenu {
    fn drop(&mut self){
        // Never leave an unretained target in an AppKit-owned menu during
        // teardown. Only restore the menu that this instance replaced.
        for entry in &self.items { unsafe { entry.item.setTarget(None); } }
        let app=NSApplication::sharedApplication(self.root.mtm());
        if app.mainMenu().is_some_and(|m|*m==*self.root){app.setMainMenu(self.original.as_deref());}
    }
}
