use super::*;
use arranger_core::Style;

pub(super) fn button(ui:&mut egui::Ui,widgets:&mut BTreeMap<String,Value>,key:&str,label:&str,enabled:bool)->bool{
    let response=ui.add_enabled(enabled,egui::Button::new(label));record_widget!(widgets,key.into(),rect(response.rect));response.clicked()
}
fn color_value(value:&str)->Color32 {
    let [r,g,b,a]=arranger_core::paint::parse(value).map(|p|p.rgba()).unwrap_or([59,130,246,255]);Color32::from_rgba_unmultiplied(r,g,b,a)
}
fn style_controls(ui:&mut egui::Ui,style:&mut Style,zh:bool,widgets:&mut BTreeMap<String,Value>)->bool {
    let mut changed=false;
    for (key,label,value) in [("fill",tr(zh,"填充","Fill"),&mut style.fill),("stroke",tr(zh,"描边","Stroke"),&mut style.stroke)] {
        ui.horizontal(|ui|{
            ui.label(label);let mut color=color_value(value);let r=ui.color_edit_button_srgba(&mut color);record_widget!(widgets,format!("{key}_color"),rect(r.rect));
            if r.changed(){let [r,g,b,a]=color.to_srgba_unmultiplied();*value=if a==255{format!("#{r:02x}{g:02x}{b:02x}")}else{format!("#{r:02x}{g:02x}{b:02x}{a:02x}")};changed=true;}
            let r=ui.add(egui::TextEdit::singleline(value).desired_width(86.));record_widget!(widgets,key.into(),rect(r.rect));changed|=r.changed();
            let mut none=value=="none";let r=ui.checkbox(&mut none,tr(zh,"无","None"));record_widget!(widgets,format!("{key}_none"),rect(r.rect));if r.changed(){*value=if none{"none".into()}else{"#344054".into()};changed=true;}
        });
    }
    let more=ui.collapsing(tr(zh,"更多外观","More appearance"),|ui|{
    ui.horizontal(|ui|{ui.label(tr(zh,"线宽 / mm","Stroke / mm"));let r=ui.add(egui::DragValue::new(&mut style.stroke_width_mm).speed(0.1).range(0. ..=100.));record_widget!(widgets,"stroke_width".into(),rect(r.rect));changed|=r.changed();});
    ui.horizontal(|ui|{ui.label(tr(zh,"不透明度","Opacity"));let r=ui.add(egui::Slider::new(&mut style.opacity,0. ..=1.));record_widget!(widgets,"opacity".into(),rect(r.rect));changed|=r.changed();});
    });record_widget!(widgets,"appearance-more".into(),rect(more.header_response.rect));
    ui.horizontal(|ui|{for color in ["#1d2939","#ffffff","#3976e8","#ee635b","#f6bc43","#2b9c7c"] {
        let r=ui.add(egui::Button::new("").fill(color_value(color)).min_size(Vec2::splat(23.)));record_widget!(widgets,format!("swatch-{}",color.trim_start_matches('#')),rect(r.rect));if r.clicked(){style.fill=color.into();changed=true;}
    }});changed
}
impl App {
    fn inspector_edit_actions(&mut self,ui:&mut egui::Ui,snapshot:&Snapshot,widgets:&mut BTreeMap<String,Value>)->(bool,bool){
        let Some(id)=self.selected.clone()else{return(false,false)};
        let Ok(object)=snapshot.document.object(&id)else{return(false,false)};
        if self.editor.as_ref().is_none_or(|e|e.id!=id||e.revision!=snapshot.revision&&!e.dirty){self.editor=Some(Editor::new(&snapshot.document,object,snapshot.revision));}
        let zh=self.chinese;
        let hint=if self.error{self.ui_message()}else if self.inline_text.is_some(){tr(zh,"文字编辑中 · 完成文字或切换工具结束","Editing text · Finish text or switch tools to finish")}else if self.editor.as_ref().is_some_and(|e|e.text_dirty){tr(zh,"文字实时预览 · 离开输入时保存修改","Live text preview · finish by clicking outside")}else if self.editor.as_ref().is_some_and(|e|e.dirty){tr(zh,"属性更改尚未应用","Changes not applied")}else{tr(zh,"属性与文档一致","Properties match the document")}.to_owned();
        let mut apply=false;let mut reload=false;
        let panel=egui::Panel::bottom("inspector-edit-actions").exact_size(66.).resizable(false).frame(frame(Color32::WHITE,0,0)).show(ui,|ui|{
            ui.spacing_mut().item_spacing.y=3.;ui.add_space(3.);ui.separator();
            self.editor_actions=ui.add_enabled_ui(!object.locked&&!snapshot.busy,|ui|ui.horizontal(|ui|{
                apply=button(ui,widgets,"apply",tr(zh,"应用更改","Apply"),true);reload=button(ui,widgets,"reload",tr(zh,"恢复数值","Reload"),true);
            })).inner.response.rect;
            ui.add(egui::Label::new(egui::RichText::new(&hint).small().color(if self.error{Color32::from_rgb(182,50,57)}else{MUTED})).truncate()).on_hover_text(&hint);
        });record_widget!(widgets,"inspector-actions".into(),rect(panel.response.rect));(apply,reload)
    }
    pub(super) fn inspector(&mut self,ui:&mut egui::Ui,snapshot:&Snapshot,widgets:&mut BTreeMap<String,Value>){
        self.text_properties=Rect::NOTHING;
        let zh=self.chinese;let doc=&snapshot.document;
        ui.horizontal(|ui|{for (tab,label) in [tr(zh,"属性","Properties"),tr(zh,"页面","Page"),tr(zh,"列表","Objects")].into_iter().enumerate(){let r=ui.add(egui::Button::new(egui::RichText::new(label).size(15.).strong()).frame(false));if self.inspector_tab==tab{ui.painter().line_segment([r.rect.left_bottom(),r.rect.right_bottom()],egui::Stroke::new(2.,ACCENT));}record_widget!(widgets,format!("inspector-{tab}"),rect(r.rect));if r.clicked(){self.inspector_tab=tab;}}});ui.separator();
        let tree_rows = if self.inspector_tab == 0 {
            doc.objects.iter().filter(|o| o.on_canvas(&self.page)).take(4).count()
        } else { 0 };
        if tree_rows > 0 {
            let row=ui.spacing().interact_size.y.max(ui.text_style_height(&egui::TextStyle::Body)+2.*ui.spacing().button_padding.y);
            let height=(42.+tree_rows as f32*(row+ui.spacing().item_spacing.y)).min(185.).min(ui.available_height()*0.32);
            let tree=egui::Panel::bottom("inspector-object-tree").exact_size(height).resizable(false).frame(frame(Color32::WHITE,0,0)).show(ui,|ui|{if doc.objects.len()>200{self.inline_objects(ui,snapshot,widgets);}else{egui::ScrollArea::vertical().id_salt("inline-object-tree").auto_shrink([false,false]).show(ui,|ui|self.inline_objects(ui,snapshot,widgets));}});record_widget!(widgets,"inspector-tree".into(),rect(tree.response.rect));
        }
        self.editor_actions=Rect::NOTHING;
        let(apply,reload)=if self.inspector_tab==0{self.inspector_edit_actions(ui,snapshot,widgets)}else{(false,false)};
        record_widget!(widgets,"inspector-properties".into(),rect(ui.available_rect_before_wrap()));
        if self.inspector_tab==2&&doc.objects.len()>200{
            ui.heading(tr(zh,"对象列表","Objects"));ui.label(tr(zh,"Shift 点选可多选","Shift-click to select more"));
            if self.selection.len()>1&&button(ui,widgets,"list_group",tr(zh,"编组所选对象","Group selection"),!snapshot.busy){self.edit_action(ui.ctx(),snapshot,"group");}
            if button(ui,widgets,"list_unlock",tr(zh,"解锁所选对象","Unlock selection"),!self.selection.is_empty()&&!snapshot.busy){self.edit_action(ui.ctx(),snapshot,"unlock");}
            if button(ui,widgets,"list_show",tr(zh,"显示所选对象","Show selection"),!self.selection.is_empty()&&!snapshot.busy){self.edit_action(ui.ctx(),snapshot,"show");}
            self.virtual_objects(ui,snapshot,widgets,false);return;
        }
        egui::ScrollArea::vertical().id_salt("inspector-scroll").auto_shrink([false,false]).show(ui,|ui|{
            match self.inspector_tab {
                1=>{
                    ui.heading(tr(zh,"页面设置","Page settings"));ui.label(tr(zh,"新建默认为 A4 · 单位 mm","New pages use A4 · millimetres"));ui.add_space(10.);
                    let key=format!("{}:{}:{}",self.active,self.page,snapshot.revision);
                    if self.page_draft_key!=key&&!self.page_draft_dirty{if let Ok(page)=doc.page(&self.page){self.page_width=editing::display_number(page.width_mm);self.page_height=editing::display_number(page.height_mm);self.page_draft_revision=snapshot.revision;self.page_draft_key=key;}}
                    ui.horizontal(|ui|{if button(ui,widgets,"a4_portrait",tr(zh,"A4 竖向","A4 portrait"),true){self.page_width="210".into();self.page_height="297".into();self.page_draft_dirty=true;}
                        if button(ui,widgets,"a4_landscape",tr(zh,"A4 横向","A4 landscape"),true){self.page_width="297".into();self.page_height="210".into();self.page_draft_dirty=true;}});
                    for (key,label,value) in [("page_width",tr(zh,"宽度 / mm","Width / mm"),&mut self.page_width),("page_height",tr(zh,"高度 / mm","Height / mm"),&mut self.page_height)]{ui.label(label);let raw=doc.page(&self.page).map(|p|if key=="page_width"{p.width_mm}else{p.height_mm}).unwrap_or(0.);let r=ui.add(egui::TextEdit::singleline(value).desired_width(ui.available_width())).on_hover_text(if zh{format!("纸张尺寸原始值：{raw} mm，可输入完整精度。")}else{format!("Original page size: {raw} mm; full precision input is supported.")});record_widget!(widgets,key.into(),rect(r.rect));self.page_draft_dirty|=r.changed();}
                    ui.add_space(8.);
                    if button(ui,widgets,"page_apply",tr(zh,"应用页面尺寸","Apply page size"),!snapshot.busy){
                        let dimensions=doc.page(&self.page).and_then(|p|Ok((editing::parse_displayed_number(&self.page_width,p.width_mm)?,editing::parse_displayed_number(&self.page_height,p.height_mm)?)));
                        match dimensions{
                        Ok((w,h)) if w>0.&&h>0.=>{if self.batch(self.page_draft_revision,vec![Operation::PageResize{page_id:self.page.clone(),width_mm:w,height_mm:h}]){self.page_draft_dirty=false;self.fit=true;self.pan=Vec2::ZERO;}},
                        _=>{self.error=true;self.message=tr(zh,"页面宽高必须是大于零的有效毫米数值","Page dimensions must be positive finite millimetres").into();}
                    }}
                    if button(ui,widgets,"page_reload",tr(zh,"恢复页面尺寸","Reload page size"),true){self.page_draft_dirty=false;self.page_draft_key.clear();}
                    ui.separator();if button(ui,widgets,"page_add",tr(zh,"添加 A4 页面","Add A4 page"),!snapshot.busy){self.edit_action(ui.ctx(),snapshot,"page_add");}
                    if button(ui,widgets,"page_delete",tr(zh,"删除当前页","Delete current page"),!snapshot.busy&&doc.pages.len()>1){self.edit_action(ui.ctx(),snapshot,"page_delete");}
                    ui.label(egui::RichText::new(tr(zh,"删除页面会删除该页对象，可一次撤销。","Deleting a page removes its objects. Undo restores it.")).small().color(MUTED));
                },
                2=>{
                    ui.heading(tr(zh,"对象列表","Objects"));ui.label(tr(zh,"Shift 点选可多选","Shift-click to select more"));ui.add_space(8.);
                    let current_page=self.page.clone();
                    for o in doc.objects.iter().rev().filter(|o|o.on_canvas(&current_page)){
                        let mut depth=0;let mut parent=o.parent_id.as_deref();while let Some(id)=parent{depth+=1;parent=doc.object(id).ok().and_then(|o|o.parent_id.as_deref());}
                        ui.horizontal(|ui|{ui.add_space((depth*12) as f32);let label=format!("{}{}{} {}",if o.page_id==arranger_core::WORKSPACE_PAGE{tr(zh,"共享 · ","Shared · ")}else{""},if o.locked{"▣ "}else{""},if o.visible{""}else{"· "},o.id);
                            let r=ui.selectable_label(self.selection.contains(&o.id),label);record_widget!(widgets,format!("object-{}",o.id),rect(r.rect));
                            if r.clicked()&&self.prepare_navigation(snapshot).is_some(){let mut ids=if ui.input(|i|i.modifiers.shift||i.modifiers.command){self.selection.clone()}else{vec![]};if ids.contains(&o.id){ids.retain(|id|id!=&o.id);}else{ids.push(o.id.clone());}self.set_selection(ids);}
                        });
                    }
                    if self.selection.len()>1&&button(ui,widgets,"list_group",tr(zh,"编组所选对象","Group selection"),!snapshot.busy){self.edit_action(ui.ctx(),snapshot,"group");}
                    if button(ui,widgets,"list_unlock",tr(zh,"解锁所选对象","Unlock selection"),!self.selection.is_empty()&&!snapshot.busy){self.edit_action(ui.ctx(),snapshot,"unlock");}
                    if button(ui,widgets,"list_show",tr(zh,"显示所选对象","Show selection"),!self.selection.is_empty()&&!snapshot.busy){self.edit_action(ui.ctx(),snapshot,"show");}
                },
                _=>{
                    if self.selection.len()>1{ui.label(egui::RichText::new(if zh{format!("已选择 {} 个对象",self.selection.len())}else{format!("{} objects selected",self.selection.len())}).strong());
                        ui.horizontal(|ui|{if button(ui,widgets,"selection_group",tr(zh,"编组","Group"),!snapshot.busy){self.edit_action(ui.ctx(),snapshot,"group");}if button(ui,widgets,"selection_delete",tr(zh,"删除","Delete"),!snapshot.busy){self.edit_action(ui.ctx(),snapshot,"delete");}});ui.separator();}
                    if let Some(id)=self.selected.clone(){if let Ok(object)=doc.object(&id){
                        let font_info=if let Geometry::Text{font_family,..}=&object.geometry{
                            let key=format!("{}:{id}",snapshot.document_hash);
                            if self.font_feedback.as_ref().is_none_or(|(old,_)|old!=&key){
                                let info=match self.service.command(Command::TextMeasure{document_id:self.active.clone(),object_id:id.clone()}){
                                    Ok(value)=>feedback::FontFeedback::from_measurement(font_family,&value),
                                    Err(error)=>feedback::FontFeedback{requested:font_family.clone(),actual_fonts:vec![],warnings:vec![arranger_core::command::Notice{code:error.code.into(),message:error.message}]},
                                };self.font_feedback=Some((key,info));
                            }
                            self.font_feedback.as_ref().map(|(_,info)|info.clone())
                        }else{self.font_feedback=None;None};
                        let kind=match object.geometry{Geometry::Rect{..}=>tr(zh,"矩形","Rectangle"),Geometry::Ellipse=>tr(zh,"圆孔 / 椭圆","Circle / ellipse"),Geometry::Path{..}=>tr(zh,"路径","Path"),Geometry::Text{..}=>tr(zh,"文字","Text"),Geometry::Image{..}=>tr(zh,"图片","Image"),Geometry::Group=>tr(zh,"群组","Group")};
                        ui.heading(kind);ui.add(egui::Label::new(egui::RichText::new(&id).small().color(MUTED)).truncate()).on_hover_text(&id);if object.locked{ui.label(tr(zh,"已锁定 · 可在列表中解锁","Locked · unlock in Objects"));}
                        let mut arrange_action=None;
                        if let Some(editor)=self.editor.as_mut(){
                            ui.add_enabled_ui(!object.locked&&!snapshot.busy,|ui|{
                                let original=editor.original_numbers;
                                ui.columns(2,|columns|{for (i,(key,label,value)) in [("x",tr(zh,"X / mm","X / mm"),&mut editor.x),("y",tr(zh,"Y / mm","Y / mm"),&mut editor.y),("width",tr(zh,"宽度 / mm","Width / mm"),&mut editor.width),("height",tr(zh,"高度 / mm","Height / mm"),&mut editor.height)].into_iter().enumerate(){let col=&mut columns[i%2];col.label(egui::RichText::new(label).small().color(MUTED));let r=col.add(egui::TextEdit::singleline(value).desired_width(col.available_width())).on_hover_text(if zh{format!("原始值：{} mm\n显示通常保留三位小数，可输入完整精度。",original[i])}else{format!("Original: {} mm\nUsually shown to 3 decimals; full precision input is supported.",original[i])});record_widget!(widgets,key.into(),rect(r.rect));editor.dirty|=r.changed();}});
                                ui.horizontal(|ui|{ui.label(tr(zh,"旋转 / °","Rotate / °"));let r=ui.add(egui::TextEdit::singleline(&mut editor.rotation).desired_width(80.)).on_hover_text(format!("{}°",original[4]));record_widget!(widgets,"rotation".into(),rect(r.rect));editor.dirty|=r.changed();});
                                ui.add_space(4.);ui.separator();ui.label(egui::RichText::new(tr(zh,"外观","Appearance")).strong());editor.dirty|=style_controls(ui,&mut editor.style,zh,widgets);
                                arrange_action=workstation::alignment_controls(ui,snapshot,self.selection.len(),!editor.dirty,widgets,zh);
                                if let Geometry::Text{content,font_family,font_size_mm,layout,..}=&mut editor.geometry{
                                    let properties=ui.scope(|ui|{
                                    ui.separator();ui.label(egui::RichText::new(tr(zh,"文字","Text")).strong());
                                    // The dirty-state hint above adds a widget after the first
                                    // keystroke. Keep text/IME focus independent of that layout.
                                    ui.label(egui::RichText::new(tr(zh,"也可双击画布文字直接输入","Double-click canvas text to type directly")).small().color(MUTED));
                                    let r=ui.add(egui::TextEdit::multiline(content).id_salt(("text-content",&id)).desired_rows(3).desired_width(ui.available_width()));record_widget!(widgets,"text_content".into(),rect(r.rect));editor.dirty|=r.changed();editor.text_dirty|=r.changed();if r.has_focus(){self.text_field=Some((r.id,r.rect));}if self.focus_text{r.request_focus();self.focus_text=false;}
                                    ui.label(tr(zh,"字体","Font family"));let r=ui.add(egui::TextEdit::singleline(font_family).id_salt(("font-family",&id)).desired_width(ui.available_width()));record_widget!(widgets,"font_family".into(),rect(r.rect));editor.dirty|=r.changed();editor.text_dirty|=r.changed();
                                    if let Some(info)=&font_info{
                                        let label=if zh{format!("当前文档实际字体：{}",info.actual_fonts.join("、"))}else{format!("Actual document fonts: {}",info.actual_fonts.join(", "))};
                                        let r=ui.label(egui::RichText::new(label).small().color(MUTED));record_widget!(widgets,"font_feedback".into(),rect(r.rect));
                                        for notice in &info.warnings{ui.label(egui::RichText::new(feedback::notice_text(notice,zh)).small().color(Color32::from_rgb(145,91,19))).on_hover_text(&notice.message);}
                                    }
                                    let mut points=*font_size_mm*72./25.4;ui.horizontal(|ui|{ui.label(tr(zh,"字号 / pt","Size / pt"));let r=ui.add(egui::DragValue::new(&mut points).speed(0.5).range(0.1..=3000.));record_widget!(widgets,"font_size".into(),rect(r.rect));if r.changed(){*font_size_mm=points*25.4/72.;editor.dirty=true;editor.text_dirty=true;}});
                                    let old=layout.align;let combo=egui::ComboBox::from_id_salt("text-align").selected_text(match old{arranger_core::TextAlign::Left=>tr(zh,"左对齐","Left"),arranger_core::TextAlign::Center=>tr(zh,"居中","Center"),arranger_core::TextAlign::Right=>tr(zh,"右对齐","Right")}).show_ui(ui,|ui|{for (align,key,label) in [(arranger_core::TextAlign::Left,"left",tr(zh,"左对齐","Left")),(arranger_core::TextAlign::Center,"center",tr(zh,"居中","Center")),(arranger_core::TextAlign::Right,"right",tr(zh,"右对齐","Right"))]{let r=ui.selectable_value(&mut layout.align,align,label);record_widget!(widgets,format!("text_align_{key}"),rect(r.rect));}});record_widget!(widgets,"text_align".into(),rect(combo.response.rect));editor.dirty|=layout.align!=old;editor.text_dirty|=layout.align!=old;
                                    });self.text_properties=properties.response.rect.intersect(ui.clip_rect());
                                }
                            });
                        }
                        if let Some(action)=arrange_action{self.operations(snapshot,workstation::arrange(doc,&self.selection,action));}
                        if self.error{ui.colored_label(Color32::from_rgb(182,50,57),self.ui_message());}
                        if self.tool==Tool::Nodes&&matches!(object.geometry,Geometry::Rect{..}|Geometry::Ellipse){ui.label(egui::RichText::new(tr(zh,"首次修改节点时将转为路径；只选取不会修改。","First node edit converts this shape to a path.")).small().color(MUTED));if button(ui,widgets,"to_path",tr(zh,"立即转为路径","Convert to path now"),!snapshot.busy&&!object.locked){self.edit_action(ui.ctx(),snapshot,"to_path");}}
                        if matches!(object.geometry,Geometry::Group)&&button(ui,widgets,"inspector_ungroup",tr(zh,"解组","Ungroup"),!snapshot.busy&&!object.locked){self.edit_action(ui.ctx(),snapshot,"ungroup");}
                    }}else{
                        ui.heading(tr(zh,"开始绘制","Start drawing"));ui.label(tr(zh,"从左侧选择工具，在页面上拖动。","Choose a tool on the left and drag on the page."));ui.add_space(12.);ui.label(tr(zh,"新对象外观","New object style"));style_controls(ui,&mut self.default_style,zh,widgets);
                        ui.add_space(14.);ui.label(egui::RichText::new(tr(zh,"右键：复制、粘贴、编组\n空格拖动：平移画布\n滚轮：缩放 · Esc：返回选择","Right-click: copy, paste, group\nSpace-drag: pan · Wheel: zoom\nEsc: return to selection")).small().color(MUTED));
                    }
                }
            }
        });
        if apply{self.apply_editor(snapshot);if !self.error{self.end_text_input(ui.ctx());}}if reload{self.editor=None;self.end_text_input(ui.ctx());self.error=false;self.message="Ready".into();}
    }
    pub(super) fn context_menu(&mut self,ui:&mut egui::Ui,snapshot:&Snapshot,widgets:&mut BTreeMap<String,Value>){
        let zh=self.chinese;let selected=!self.selection.is_empty();let text=self.text_command_target(ui.ctx()).is_some();let editable=selected&&!snapshot.busy&&!text;
        let group=self.selection.iter().any(|id|snapshot.document.object(id).is_ok_and(|o|matches!(o.geometry,Geometry::Group)));
        for (key,zh_label,en_label,enabled) in [
            ("cut","剪切","Cut",!snapshot.busy&&(text||selected)),("copy","复制","Copy",!snapshot.busy&&(text||selected)),("paste","粘贴","Paste",!snapshot.busy),("duplicate","再制一份","Duplicate",editable),("delete","删除","Delete",editable),
            ("group","编组","Group",editable&&self.selection.len()>1),("ungroup","解组","Ungroup",editable&&group),
            ("front","置于顶层","Bring to front",editable),("back","置于底层","Send to back",editable),
            ("lock","锁定","Lock",editable),("unlock","解锁","Unlock",editable),("hide","隐藏","Hide",editable),("show","显示","Show",editable),("all","全选","Select all",true)] {
            if matches!(key,"group"|"front"|"all"){ui.separator();}
            if button(ui,widgets,&format!("ctx-{key}"),tr(zh,zh_label,en_label),enabled){self.edit_action(ui.ctx(),snapshot,key);ui.close();}
        }
        ui.separator();
        if button(ui,widgets,"ctx-to_curves",editing::curves_label(zh),editable){self.edit_action(ui.ctx(),snapshot,"to_curves");ui.close();}
    }
    pub(super) fn keyboard(&mut self,ctx:&egui::Context,snapshot:&Snapshot){
        if self.dialog.is_some()||self.quit_prompt{return;}
        use egui::{Key,Modifiers};
        let shortcut=|key,shift|ctx.input_mut(|i|i.consume_key(Modifiers{shift,command:true,..Default::default()},key));
        #[cfg(target_os="macos")]
        if consume_mac_curves(ctx){if self.inline_text.is_none()&&!ctx.text_edit_focused(){self.edit_action(ctx,snapshot,"to_curves");}return;}
        for (key,shift,action) in [(Key::S,true,"save_as"),(Key::S,false,"save"),(Key::O,false,"open"),(Key::N,false,"new")] {if shortcut(key,shift){self.file_action(snapshot,action);return;}}
        #[cfg(not(target_os="macos"))]
        if shortcut(Key::W,false){self.request_close(ctx,false);return;}
        if self.text_escape(ctx){return;}
        if shortcut(Key::P,false){self.file_action(snapshot,"print");return;}
        if shortcut(Key::Enter,false){if self.inline_text.is_some(){self.finish_text(ctx,snapshot);}else{self.apply_editor(snapshot);if !self.error{self.command_focus=None;ctx.memory_mut(|m|m.stop_text_input());}}return;}
        if self.inline_text.is_none()&&self.editor.as_ref().is_some_and(|e|e.text_dirty)&&ctx.input(|i|i.key_pressed(Key::Tab)){self.commit_text(snapshot);self.text_field=None;}
        // The edit session owns keys even while the platform temporarily
        // changes IME/focus ownership. Never delete the text object on a
        // Backspace intended for its content.
        if self.inline_text.is_some()||ctx.text_edit_focused(){return;}
        // Do not consume macOS Cmd+Q (Quit), or keys owned by a text editor.
        #[cfg(not(target_os="macos"))]
        if shortcut(Key::Q,false){self.edit_action(ctx,snapshot,"to_curves");return;}
        let events=ctx.input(|i|i.events.clone());for event in events {match event {
            egui::Event::Copy=>{self.edit_action(ctx,snapshot,"copy");return;},egui::Event::Cut=>{self.edit_action(ctx,snapshot,"cut");return;},egui::Event::Paste(text)=>{self.paste_text(snapshot,&text);return;},_=>{}
        }}
        for (key,shift,action) in [(Key::Z,true,"redo"),(Key::Z,false,"undo"),(Key::Y,false,"redo"),(Key::G,true,"ungroup"),(Key::G,false,"group"),(Key::D,false,"duplicate"),(Key::A,false,"all")] {if shortcut(key,shift){self.edit_action(ctx,snapshot,action);return;}}
        if ctx.input_mut(|i|i.consume_key(Modifiers::NONE,Key::Delete)||i.consume_key(Modifiers::NONE,Key::Backspace)){self.edit_action(ctx,snapshot,"delete");return;}
        if ctx.input_mut(|i|i.consume_key(Modifiers::NONE,Key::Escape)){if self.gesture.take().is_some(){return;}if !self.pen.is_empty(){self.pen.clear();}else if self.tool!=Tool::Select{self.tool=Tool::Select;}else if self.selection_mode==selection::Mode::Rotate{self.selection_mode=selection::Mode::Scale;}else{self.set_selection(vec![]);}return;}
        if self.tool==Tool::Pen&&ctx.input_mut(|i|i.consume_key(Modifiers::NONE,Key::Enter)){self.finish_pen(snapshot,false);return;}
        for (key,tool) in [(Key::V,Tool::Select),(Key::A,Tool::Nodes),(Key::H,Tool::Pan),(Key::R,Tool::Rect),(Key::E,Tool::Ellipse),(Key::L,Tool::Line),(Key::P,Tool::Pen),(Key::T,Tool::Text)] {if ctx.input_mut(|i|i.consume_key(Modifiers::NONE,key)){self.tool=tool;self.gesture=None;self.selection_mode=Default::default();self.rotation_center=None;}}
        if !snapshot.busy&&!self.selection.is_empty(){for (key,dx,dy) in [(Key::ArrowLeft,-1.,0.),(Key::ArrowRight,1.,0.),(Key::ArrowUp,0.,-1.),(Key::ArrowDown,0.,1.)] {
            let step=if ctx.input(|i|i.modifiers.shift){10.}else{1.};if ctx.input(|i|i.key_pressed(key)){self.operations(snapshot,editing::translate(&snapshot.document,&self.selection,dx*step,dy*step));break;}
        }}
    }
}

// egui logical matching permits extra Shift: consume the more specific
// macOS outline shortcut before the generic Cmd+O file action.
#[cfg(any(target_os="macos",test))]
fn consume_mac_curves(ctx:&egui::Context)->bool {
    ctx.input_mut(|i|i.consume_key(egui::Modifiers{command:true,shift:true,..Default::default()},egui::Key::O))
}

#[cfg(test)]
mod shortcut_tests {
    use super::*;
    #[test]
    fn mac_outline_consumes_shift_o_before_open_and_leaves_open_and_quit_alone(){
        for (key,shift,outline,open) in [(egui::Key::O,true,true,false),(egui::Key::O,false,false,true),(egui::Key::Q,false,false,false)] {
            let ctx=egui::Context::default();let mods=egui::Modifiers{mac_cmd:true,command:true,shift,..Default::default()};
            let input=egui::RawInput{events:vec![egui::Event::Key{key,physical_key:Some(key),pressed:true,repeat:false,modifiers:mods}],..Default::default()};
            ctx.begin_pass(input);
            assert_eq!(consume_mac_curves(&ctx),outline);
            assert_eq!(ctx.input_mut(|i|i.consume_key(egui::Modifiers::COMMAND,egui::Key::O)),open);
            if key==egui::Key::Q{assert!(ctx.input(|i|i.key_pressed(egui::Key::Q)));}
            let _=ctx.end_pass();
        }
    }
}
