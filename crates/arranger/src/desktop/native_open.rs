//! Add the document-open delegate method missing from the pinned winit.
//! Keep winit's delegate, launch/window lifecycle and all existing methods.
use super::*;
use std::{cell::RefCell,rc::Rc};
use objc2::{define_class,msg_send,sel,MainThreadOnly};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass,AnyObject,Imp,Sel};
use objc2_app_kit::{NSApplication,NSApplicationWillFinishLaunchingNotification};
use objc2_foundation::{MainThreadMarker,NSObject,NSObjectProtocol,NSNotification,NSNotificationCenter,NSArray,NSURL};

#[derive(Default)]
struct OpenState { paths:RefCell<Vec<PathBuf>>,context:RefCell<Option<egui::Context>> }
thread_local! {static OPEN:RefCell<Option<Rc<OpenState>>>=const{RefCell::new(None)};}

// SAFETY: The selector is NSApplicationDelegate's void application:openURLs:.
// AppKit passes live objects on the main thread. The delegate's ivars are
// never accessed or cast to our type. Only local file URLs enter our queue.
unsafe extern "C-unwind" fn open_urls(_receiver:*mut AnyObject,_selector:Sel,_app:&NSApplication,urls:&NSArray<NSURL>){
    OPEN.with_borrow(|state|{if let Some(state)=state{
        for index in 0..urls.count(){let url=urls.objectAtIndex(index);if url.isFileURL(){if let Some(path)=url.path(){state.paths.borrow_mut().push(PathBuf::from(path.to_string()));}}}
        if let Some(context)=state.context.borrow().as_ref(){context.request_repaint();}
    }});
}
fn install(){
    let app=NSApplication::sharedApplication(MainThreadMarker::new().expect("Document events require the main thread"));
    let Some(delegate)=app.delegate()else{return;};let class:&AnyClass=unsafe{msg_send![&*delegate,class]};
    assert_eq!(class.name(),c"WinitApplicationDelegate","Review native document integration after changing the window backend");
    let selector=sel!(application:openURLs:);
    // SAFETY: Same ABI as the public NSApplicationDelegate method. Add only
    // this absent optional method; never replace an existing implementation.
    // The callback has process lifetime, and an empty queue after teardown.
    let implementation:Imp=unsafe{std::mem::transmute(open_urls as unsafe extern "C-unwind" fn(*mut AnyObject,Sel,&NSApplication,&NSArray<NSURL>))};
    if let Some(method)=class.instance_method(selector){assert_eq!(method.implementation() as usize,implementation as usize,"Backend already handles document events");}
    else{assert!(unsafe{objc2::ffi::class_addMethod((class as *const AnyClass).cast_mut(),selector,implementation,c"v@:@@".as_ptr())}.as_bool(),"Could not install document-open delegate method");}
}
define_class!(
    // SAFETY: NSObject without ivars. AppKit invokes on the main thread.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = ()]
    struct ArrangerOpenObserver;
    unsafe impl NSObjectProtocol for ArrangerOpenObserver {}
    impl ArrangerOpenObserver {
        #[unsafe(method(arrangerWillFinishLaunching:))]
        fn will_finish(&self,_notification:&NSNotification){install();}
    }
);
pub(super) struct NativeOpen {state:Rc<OpenState>,observer:Retained<ArrangerOpenObserver>}
impl NativeOpen {
    pub(super) fn new()->Self{
        let mtm=MainThreadMarker::new().expect("Document events require the main thread");
        let state=Rc::new(OpenState::default());OPEN.with_borrow_mut(|s|{assert!(s.is_none());*s=Some(state.clone());});
        let observer:Retained<ArrangerOpenObserver>=unsafe{msg_send![super(ArrangerOpenObserver::alloc(mtm).set_ivars(())),init]};
        // Open URLs can arrive before didFinishLaunching/AppCreator. Observe
        // willFinish, when winit's delegate exists but the cold event has not
        // yet been delivered. No document controller or AppleEvent override.
        unsafe{NSNotificationCenter::defaultCenter().addObserver_selector_name_object(&observer,sel!(arrangerWillFinishLaunching:),Some(NSApplicationWillFinishLaunchingNotification),None);}
        Self{state,observer}
    }
    pub(super) fn attach(&self,context:egui::Context){*self.state.context.borrow_mut()=Some(context);install();}
    pub(super) fn take(&self)->Vec<PathBuf>{std::mem::take(&mut *self.state.paths.borrow_mut())}
}
impl Drop for NativeOpen {
    fn drop(&mut self){
        unsafe{NSNotificationCenter::defaultCenter().removeObserver(&self.observer);}
        OPEN.with_borrow_mut(|s|*s=None);
    }
}
