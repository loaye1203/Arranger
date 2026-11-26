//! Async AppKit file sheets. Callbacks only collect a result and wake egui;
//! document commands still run through App::finish_file on the next UI pass.
use super::*;
use super::files::{FileDialog,FileKind};
use std::{cell::RefCell,rc::Rc};
use block2::RcBlock;
use objc2::{class,msg_send};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSApplication,NSSavePanel,NSOpenPanel,NSModalResponse,NSModalResponseOK};
use objc2_foundation::{MainThreadMarker,NSString,NSURL,NSArray};

// UTType is available on the supported macOS 11+ baseline. Reuse AppKit's
// system framework with the public Objective-C API; no new binding version.
#[link(name="UniformTypeIdentifiers",kind="framework")]
unsafe extern "C" {}

#[derive(Clone)]
enum Panel {Open(Retained<NSOpenPanel>),Save(Retained<NSSavePanel>)}
impl Panel {
    fn save(&self)->&NSSavePanel{match self{Self::Open(p)=>p,Self::Save(p)=>p}}
    fn paths(&self)->Vec<PathBuf>{
        let urls=match self{Self::Open(p)=>p.URLs(),Self::Save(p)=>NSArray::from_retained_slice(&p.URL().into_iter().collect::<Vec<_>>())};
        (0..urls.count()).filter_map(|i|{let url=urls.objectAtIndex(i);if url.isFileURL(){url.path().map(|p|PathBuf::from(p.to_string()))}else{None}}).collect()
    }
}
pub(super) struct Reply {pub paths:Vec<PathBuf>,pub accepted:bool,pub aborted:bool}
#[derive(Default)]
struct State {panel:Option<Panel>,reply:Option<Reply>,#[cfg(feature="test-hooks")] replay:Option<Reply>}
pub(super) struct NativeFiles {state:Rc<RefCell<State>>,context:egui::Context}
impl NativeFiles {
    pub(super) fn new(context:egui::Context)->Self{Self{state:Rc::new(RefCell::new(State::default())),context}}
    pub(super) fn active(&self)->bool{self.state.borrow().panel.is_some()}
    pub(super) fn take_reply(&self)->Option<Reply>{
        let mut state=self.state.borrow_mut();let reply=state.reply.take()?;state.panel=None;Some(reply)
    }
    pub(super) fn cancel(&self){let panel=self.state.borrow().panel.clone();if let Some(panel)=panel{unsafe{panel.save().cancel(None);}}}
    pub(super) fn show(&self,dialog:&FileDialog,zh:bool)->Result<(),String>{
        if self.active(){return Err("A file sheet is already open".into());}
        let mtm=MainThreadMarker::new().expect("File sheets require the main thread");
        let app=NSApplication::sharedApplication(mtm);
        let parent=app.mainWindow().or_else(||{
            let windows=app.windows();(0..windows.count()).map(|i|windows.objectAtIndex(i)).find(|w|w.canBecomeMainWindow())
        }).ok_or_else(||"No document window is available for the file sheet".to_string())?;
        let panel=if matches!(dialog.kind,FileKind::Open|FileKind::Image){
            let p=NSOpenPanel::openPanel(mtm);p.setCanChooseFiles(true);p.setCanChooseDirectories(false);p.setAllowsMultipleSelection(matches!(dialog.kind,FileKind::Open)&&!dialog.recover_backup&&!dialog.recover_source);Panel::Open(p)
        }else{Panel::Save(NSSavePanel::savePanel(mtm))};
        let save=panel.save();save.setCanCreateDirectories(true);save.setAllowsOtherFileTypes(false);save.setExtensionHidden(false);save.setCanSelectHiddenExtension(false);save.setShowsTagField(false);
        let title=if dialog.choose_format{tr(zh,"另存为","Save as")}else{match &dialog.kind{
            FileKind::Open if dialog.recover_source=>tr(zh,"从原稿备份恢复","Recover source backup"),
            FileKind::Open if dialog.recover_backup=>tr(zh,"从 CDR 备份恢复","Recover CDR backup"),
            FileKind::Open=>tr(zh,"打开文件","Open file"),FileKind::Image=>tr(zh,"导入图片","Import image"),
            FileKind::Save=>if dialog.choose_format{tr(zh,"另存为","Save as")}else{tr(zh,"保存文件","Save file")},FileKind::Export(f)if f=="print"=>tr(zh,"打印 / PDF","Print / PDF"),FileKind::Export(_)=>tr(zh,"导出文件","Export file")}};
        save.setTitle(Some(&NSString::from_str(title)));
        save.setPrompt(Some(&NSString::from_str(if dialog.choose_format{tr(zh,"保存","Save")}else{match dialog.kind{FileKind::Open=>tr(zh,"打开","Open"),FileKind::Image=>tr(zh,"导入","Import"),FileKind::Save=>tr(zh,"保存","Save"),_=>tr(zh,"导出","Export")}})));
        save.setNameFieldLabel(Some(&NSString::from_str(tr(zh,"名称：","Name:"))));
        let extensions:Vec<&str>=match &dialog.kind{
            FileKind::Open if dialog.recover_source=>vec!["arranger","bak"],
            FileKind::Open if dialog.recover_backup=>vec!["cdr","bak"],FileKind::Open=>vec!["arranger","cdr"],FileKind::Image=>vec!["png","jpg","jpeg"],
            _=>vec![dialog.kind.extension().unwrap_or("arranger")],
        };
        if extensions.contains(&"cdr"){
            // macOS resolves .cdr to its optical-disc type, whose preferred
            // extension is .dvdr. Keep the literal drawing extension through
            // the supported extension-based API rather than suggesting a
            // different suffix. Document commands recheck extensions too.
            let extensions:Vec<_>=extensions.into_iter().map(NSString::from_str).collect();
            #[allow(deprecated)] save.setAllowedFileTypes(Some(&NSArray::from_retained_slice(&extensions)));
        }else{
            let types:Vec<Retained<AnyObject>>=extensions.into_iter().filter_map(|e|unsafe{msg_send![class!(UTType),typeWithFilenameExtension:&*NSString::from_str(e)]}).collect();
            if types.is_empty(){return Err("The system could not resolve the selected file type".into());}
            // SAFETY: Public selector takes NSArray<UTType*> from UTType's
            // documented factory method.
            unsafe{let _:()=msg_send![save,setAllowedContentTypes:&*NSArray::from_retained_slice(&types)];}
        }
        let directory=PathBuf::from(&dialog.directory);
        if directory.is_dir(){save.setDirectoryURL(Some(&NSURL::fileURLWithPath_isDirectory(&NSString::from_str(&dialog.directory),true)));}
        if matches!(dialog.kind,FileKind::Save|FileKind::Export(_)){save.setNameFieldStringValue(&NSString::from_str(&dialog.name));}
        save.setMessage(None);
        if dialog.recover_source{save.setMessage(Some(&NSString::from_str(tr(zh,"请选择 .arranger 或 .arranger.bak；原件保留，恢复后另存副本。","Choose .arranger or .arranger.bak; originals stay unchanged. Save a recovered copy."))));}
        if dialog.recover_backup{save.setMessage(Some(&NSString::from_str(tr(zh,"请选择原 .cdr 或 .cdr.bak 备份；原件不改动。","Choose the original .cdr or its .cdr.bak backup; files stay unchanged."))));}
        let weak=Rc::downgrade(&self.state);let context=self.context.clone();
        let completion=RcBlock::new(move|response:NSModalResponse|{
            let Some(state)=weak.upgrade()else{return;};
            let panel=state.borrow().panel.clone();
            let paths=if response==NSModalResponseOK{panel.as_ref().map_or_else(Vec::new,Panel::paths)}else{vec![]};
            if let Some(panel)=panel{panel.save().orderOut(None);}
            let reply=Reply{paths,accepted:response==NSModalResponseOK,aborted:response!=NSModalResponseOK&&response!=objc2_app_kit::NSModalResponseCancel};
            #[cfg(feature="test-hooks")] let reply=state.borrow_mut().replay.take().unwrap_or(reply);
            state.borrow_mut().reply=Some(reply);
            context.request_repaint();
        });
        self.state.borrow_mut().panel=Some(panel.clone());
        save.beginSheetModalForWindow_completionHandler(&parent,&completion);
        Ok(())
    }
    #[cfg(feature="test-hooks")]
    pub(super) fn status(&self)->Value{
        let state=self.state.borrow();let Some(panel)=&state.panel else{return json!(null);};let save=panel.save();
        let types:Retained<NSArray<AnyObject>>=unsafe{msg_send![save,allowedContentTypes]};
        #[allow(deprecated)] let file_types=save.allowedFileTypes().map(|a|(0..a.count()).map(|i|a.objectAtIndex(i).to_string()).collect::<Vec<_>>());
        json!({"kind":if matches!(panel,Panel::Open(_)){"open"}else{"save"},"title":save.title().to_string(),"prompt":save.prompt().to_string(),"shows_tags":save.showsTagField(),"message":save.message().to_string(),"visible":save.isVisible(),"is_sheet":save.sheetParent().is_some(),"window_number":save.windowNumber(),"key":save.isKeyWindow(),"name":save.nameFieldStringValue().to_string(),"directory":save.directoryURL().and_then(|u|u.path()).map(|p|p.to_string()),"file_types":file_types,"extensions":(0..types.count()).map(|i|{let t=types.objectAtIndex(i);let e:Option<Retained<NSString>>=unsafe{msg_send![&*t,preferredFilenameExtension]};e.map(|e|e.to_string())}).collect::<Vec<_>>(),"multiple":match panel{Panel::Open(p)=>p.allowsMultipleSelection(),_=>false},"choose_directories":match panel{Panel::Open(p)=>p.canChooseDirectories(),_=>false},"paths":panel.paths()})
    }
    #[cfg(feature="test-hooks")]
    pub(super) fn test_action(&self,event:&Value){
        let panel=self.state.borrow().panel.clone();let Some(panel)=panel else{return;};let save=panel.save();
        match event["action"].as_str(){
            // Exercise application result handling after a real cancellation;
            // this is an explicit replay, never evidence of an AppKit OK click.
            Some("replay_result")=>{self.state.borrow_mut().replay=Some(Reply{paths:event["paths"].as_array().into_iter().flatten().filter_map(Value::as_str).map(PathBuf::from).collect(),accepted:event["accepted"]==true,aborted:event["aborted"]==true});unsafe{save.cancel(None);}},
            Some("cancel")=>unsafe{save.cancel(None);},
            Some("name")=>{if let Some(name)=event["text"].as_str(){save.setNameFieldStringValue(&NSString::from_str(name));}},
            Some("activate")=>{let app=NSApplication::sharedApplication(MainThreadMarker::new().unwrap());#[allow(deprecated)] app.activateIgnoringOtherApps(true);save.makeKeyAndOrderFront(None);},
            Some("text") if save.isKeyWindow()=>{
                if let Some(responder)=save.firstResponder(){
                    use objc2::sel;
                    let accepts:bool=unsafe{msg_send![&*responder,respondsToSelector:sel!(insertText:replacementRange:)]};
                    if accepts{unsafe{let _:()=msg_send![&*responder,insertText:&*NSString::from_str(event["text"].as_str().unwrap_or("")),replacementRange:objc2_foundation::NSRange::new(objc2_foundation::NSNotFound as usize,0)];}}
                }
            },
            Some("ok")|Some("key")=>{
                if !save.isKeyWindow(){return;}
                use objc2_app_kit::{NSEvent,NSEventType,NSEventModifierFlags};
                let text=event["text"].as_str().unwrap_or("\r");let key=event["code"].as_u64().unwrap_or(36) as u16;
                let flags=if event["command"]==true{NSEventModifierFlags::Command}else{NSEventModifierFlags::empty()}|if event["shift"]==true{NSEventModifierFlags::Shift}else{NSEventModifierFlags::empty()};
                let app=NSApplication::sharedApplication(MainThreadMarker::new().unwrap());
                for kind in [NSEventType::KeyDown,NSEventType::KeyUp]{
                    if let Some(key_event)=NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(kind,objc2_foundation::NSPoint::new(0.,0.),flags,0.,save.windowNumber(),None,&NSString::from_str(text),&NSString::from_str(text),false,key){app.sendEvent(&key_event);}
                }
            },
            _=>{},
        }
    }
}
impl Drop for NativeFiles {fn drop(&mut self){self.cancel();}}
