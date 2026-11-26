use super::*;
use std::collections::BTreeSet;

impl App {
    pub(super) fn prepare_navigation(&mut self,snapshot:&Snapshot)->Option<Snapshot>{
        if let Some(reason)=self.file_open_blocker(snapshot){self.error=true;self.message=reason.into();return None;}
        if !self.commit_text(snapshot){return None;}
        let current=self.service.snapshot(&self.active).ok()?;
        if let Some(reason)=self.file_open_blocker(&current){self.error=true;self.message=reason.into();return None;}
        Some(current)
    }
    pub(super) fn activate_page(&mut self,ctx:&egui::Context,id:&str)->bool{
        if id==self.page{return true;}
        let Ok(snapshot)=self.service.snapshot(&self.active)else{return false;};
        if snapshot.document.page(id).is_err(){return false;}
        let Some(current)=self.prepare_navigation(&snapshot)else{return false;};
        let Ok(page)=current.document.page(id)else{return false;};
        self.switch_page(page.id.clone(),page.width_mm,page.height_mm);
        ctx.memory_mut(|m|m.stop_text_input());true
    }
    pub(super) fn switch_page(&mut self,id:String,width:f64,height:f64){
        // Page navigation must not move the shared desktop under the pointer.
        if self.view.canvas.is_positive(){
            let scale=self.view.scale;self.zoom=scale/(96./25.4);self.fit=false;
            self.pan=self.view.origin-(self.view.canvas.center()-Vec2::new(width as f32,height as f32)*scale/2.);
        }
        if self.inline_text.is_some(){self.tool=Tool::Select;}
        self.page=id;self.set_selection(vec![]);self.gesture=None;self.page_draft_dirty=false;self.page_draft_key.clear();
    }
}

/// Geometry and page ownership are published by one core transaction. The
/// precise API may also append object_place explicitly after its own edits.
pub(super) fn placed(mut operations:Vec<Operation>,page:&str)->Vec<Operation> {
    let mut ids=BTreeSet::new();let mut removed=BTreeSet::new();
    for op in &operations {match op {
        Operation::ObjectCreate{object}|Operation::ImageImport{object,..}=>{ids.insert(object.id.clone());},
        Operation::ObjectDuplicate{new_id,..}=>{ids.insert(new_id.clone());},
        Operation::ObjectUpdate{object_id,patch}=>{if patch.x_mm.is_some()||patch.y_mm.is_some()||patch.width_mm.is_some()||patch.height_mm.is_some()||patch.transform.is_some()||patch.geometry.is_some(){ids.insert(object_id.clone());}},
        Operation::ObjectReparent{object_id,..}|Operation::ObjectToPath{object_id}|Operation::TextToPath{object_id}|
        Operation::PathSet{object_id,..}|Operation::PathNodeUpdate{object_id,..}|Operation::PathNodeInsert{object_id,..}|Operation::PathNodeDelete{object_id,..}|
        Operation::PathSubpathSetClosed{object_id,..}|Operation::PathSplitSegment{object_id,..}=>{ids.insert(object_id.clone());},
        Operation::PathBoolean{left_id,right_id,result_id,remove_inputs,..}=>{ids.insert(result_id.clone());if *remove_inputs{removed.insert(left_id.clone());removed.insert(right_id.clone());}},
        Operation::ObjectDelete{object_id}=>{removed.insert(object_id.clone());},_=>{}
    }}
    operations.extend(ids.difference(&removed).map(|id|Operation::ObjectPlace{object_id:id.clone(),page_id:page.into()}));operations
}
