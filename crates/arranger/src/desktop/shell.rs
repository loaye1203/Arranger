//! Native workstation chrome. All controls invoke the existing command engine.
use super::*;
use kurbo::{Affine, BezPath, PathEl};
use std::sync::OnceLock;

const HEADER:Color32=Color32::from_rgb(15,15,16);
pub(super) fn icon(ui:&egui::Ui,name:&str,r:Rect,color:Color32){
    static ICONS:OnceLock<BTreeMap<String,Vec<Vec<kurbo::Point>>>>=OnceLock::new();
    let icons=ICONS.get_or_init(||{
        let raw:BTreeMap<String,Vec<String>>=serde_json::from_str(include_str!("../../assets/icons/paths.json")).expect("vendored icon paths");
        raw.into_iter().map(|(key,paths)|{
            let mut contours=vec![];
            for path in paths {
                let mut contour=vec![];let mut first=kurbo::Point::ORIGIN;
                kurbo::flatten(BezPath::from_svg(&path).expect("upstream icon geometry").iter(),0.15,|element|match element {
                    PathEl::MoveTo(p)=>{if contour.len()>1{contours.push(std::mem::take(&mut contour));}first=p;contour.push(p);}
                    PathEl::LineTo(p)=>contour.push(p),
                    PathEl::ClosePath=>contour.push(first),_=>{}
                });
                if contour.len()>1{contours.push(contour);}
            }
            (key,contours)
        }).collect()
    });
    if let Some(contours)=icons.get(name){
        let matrix=Affine::translate((r.min.x as f64,r.min.y as f64))*Affine::scale(r.width() as f64/24.);
        for contour in contours {
            let points=contour.iter().map(|p|{let p=matrix* *p;Pos2::new(p.x as f32,p.y as f32)}).collect();
            ui.painter().add(egui::Shape::line(points,egui::Stroke::new(1.6,color)));
        }
    }
}
pub(super) fn icon_button(ui:&mut egui::Ui,widgets:&mut BTreeMap<String,Value>,key:&str,image:&str,label:&str,enabled:bool,selected:bool)->egui::Response{
    let size=Vec2::new(if label.is_empty(){26.}else{ui.fonts_mut(|f|f.layout_no_wrap(label.into(),egui::FontId::proportional(14.),INK).size().x)+34.},26.);
    let r=ui.add_enabled(enabled,egui::Button::new("").min_size(size).frame(selected).fill(if selected{Color32::from_rgb(255,235,225)}else{Color32::TRANSPARENT}).stroke(if selected{egui::Stroke::new(1.,ACCENT)}else{egui::Stroke::NONE}));
    if r.hovered()&&!selected{ui.painter().rect_filled(r.rect,3.,Color32::from_black_alpha(12));}
    let color=if !enabled{MUTED.gamma_multiply(0.5)}else if selected{ACCENT}else{ui.visuals().text_color()};
    icon(ui,image,Rect::from_center_size(Pos2::new(if key=="ribbon_toggle"{r.rect.max.x-13.}else{r.rect.min.x+13.},r.rect.center().y),Vec2::splat(17.)),color);
    if !label.is_empty(){ui.painter().text(Pos2::new(r.rect.min.x+if key=="ribbon_toggle"{7.}else{26.},r.rect.center().y),egui::Align2::LEFT_CENTER,label,egui::FontId::proportional(14.),color);}
    // The header registers an open menu before the toolbar is laid out.
    // Keep its foreground hit target when both controls invoke the same action.
    if cfg!(feature = "test-hooks") {
        widgets.entry(key.into()).or_insert_with(|| rect(r.rect));
    }
    if label.is_empty() { r } else { r.on_hover_text(label) }
}
impl App {
    pub(super) fn status_text(&self)->&str{
        if self.error{self.ui_message()}
        else if self.inline_text.is_some(){text_edit::editing_hint(self.chinese)}
        else if self.tool==Tool::Nodes{tr(self.chinese,"双击边缘添加节点 · 双击节点删除","Double-click edge to add · node to delete")}
        else{message_text(&self.message,self.chinese)}
    }
    fn exports_menu(&mut self,ui:&mut egui::Ui,s:&Snapshot,w:&mut BTreeMap<String,Value>){
        let zh=self.chinese;for format in ["svg","png","pdf","cdr14","cdr21"]{if button(ui,w,format,&format.to_uppercase(),!s.busy){self.file_action(s,format);ui.close();}}
        ui.separator();for (key,z,e) in [("cdr14_appearance","CDR14 保外观副本…","CDR14 appearance copy…"),("cdr21_appearance","CDR21 保外观副本…","CDR21 appearance copy…")]{if button(ui,w,key,tr(zh,z,e),!s.busy){self.file_action(s,key);ui.close();}}
    }
    pub(super) fn header(&mut self,ui:&mut egui::Ui,s:&Snapshot,w:&mut BTreeMap<String,Value>)->Rect{
        let area=egui::Panel::top("app-header").frame(frame(HEADER,10,3)).show(ui,|ui|{
            ui.visuals_mut().override_text_color=Some(Color32::WHITE);
            ui.visuals_mut().widgets.inactive.weak_bg_fill=HEADER;ui.visuals_mut().widgets.inactive.bg_stroke=egui::Stroke::NONE;
            ui.visuals_mut().widgets.hovered.weak_bg_fill=Color32::from_gray(48);
            ui.spacing_mut().item_spacing.x=5.;ui.spacing_mut().button_padding=Vec2::new(7.,2.);ui.spacing_mut().interact_size.y=26.;
            ui.style_mut().text_styles.insert(egui::TextStyle::Button,egui::FontId::proportional(14.));
            ui.horizontal(|ui|{
                ui.set_height(26.);
                if !cfg!(target_os="macos") { ui.label(egui::RichText::new("Arranger").family(if self.chinese{egui::FontFamily::Name("arranger-bold".into())}else{egui::FontFamily::Proportional}).size(20.).strong().color(Color32::WHITE));
                let (_,r)=ui.allocate_space(Vec2::splat(6.));ui.painter().rect_filled(r,0.,ACCENT);ui.separator(); }
                // Stay beside the app name, never in macOS's right status area.
                for(key,image,label,enabled)in[("undo","undo-2",tr(self.chinese,"撤销","Undo"),s.undo_depth>0||self.text_command_target(ui.ctx()).is_some()),("redo","redo-2",tr(self.chinese,"重做","Redo"),s.redo_depth>0||self.text_command_target(ui.ctx()).is_some())]{if icon_button(ui,w,key,image,"",enabled&&!s.busy,false).on_hover_text(label).clicked(){self.edit_action(ui.ctx(),s,key);}}
                ui.separator();
                if !cfg!(target_os="macos") { for(key,z,e)in[("file","文件","File"),("edit","编辑","Edit"),("view","视图","View"),("layout","布局","Layout"),("object","对象","Object"),("path","路径","Path"),("export","导出","Export")]{
                    let r=ui.menu_button(tr(self.chinese,z,e),|ui|self.menu(ui,s,w,key));record_widget!(w,format!("menu-{key}"),rect(r.response.rect));
                }
                let narrow=ui.max_rect().width()<1160.;
                if narrow{let r=ui.menu_button(tr(self.chinese,"更多","More"),|ui|{for(key,z,e)in[("text","文本","Text"),("tools","工具","Tools"),("window","窗口","Window"),("help","帮助","Help")]{ui.menu_button(tr(self.chinese,z,e),|ui|self.menu(ui,s,w,key));}});record_widget!(w,"menu-more".into(),rect(r.response.rect));}
                else{for(key,z,e)in[("text","文本","Text"),("tools","工具","Tools"),("window","窗口","Window"),("help","帮助","Help")]{let r=ui.menu_button(tr(self.chinese,z,e),|ui|self.menu(ui,s,w,key));record_widget!(w,format!("menu-{key}"),rect(r.response.rect));}}
                }
                ui.separator();self.document_tabs(ui,w);
            });
        }).response.rect;self.command_bar=area;area
    }
    fn document_tabs(&mut self,ui:&mut egui::Ui,w:&mut BTreeMap<String,Value>){
        let docs:BTreeMap<_,_>=self.service.document_summaries().into_iter().map(|d|(d.document_id.clone(),d)).collect();
        self.document_order.retain(|id|docs.contains_key(id));
        if self.document_order.len()!=docs.len(){
            let known:std::collections::BTreeSet<_>=self.document_order.iter().cloned().collect();
            self.document_order.extend(docs.keys().filter(|id|!known.contains(*id)).cloned());
        }
        let active=self.active.clone();let memory=ui.scope_id().with("active-document-tab");
        let reveal=ui.ctx().data_mut(|d|{let changed=d.get_temp::<String>(memory).as_ref()!=Some(&active);d.insert_temp(memory,active.clone());changed});
        let width=ui.available_width().max(40.);let mut action=None;
        ui.allocate_ui(Vec2::new(width,26.),|ui|{
            ui.set_min_width(width);
            let tabs=egui::ScrollArea::horizontal().id_salt("document-tabs").auto_shrink([false,true]).max_height(26.).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui,|ui|{
                ui.spacing_mut().item_spacing.x=2.;
                ui.horizontal_top(|ui|{ui.set_height(26.);for id in &self.document_order{let d=&docs[id];let id=id.as_str();
                    let path=d.saved_path.as_deref().or_else(||self.known_document_path(id));
                    let name=format!("{}{}",title(path,id),if d.dirty{" *"}else{""});
                    let selected=active==id;
                    let desired=ui.fonts_mut(|f|f.layout_no_wrap(name.clone(),egui::FontId::proportional(14.),Color32::WHITE).size().x)+24.;
                    let tab=ui.scope(|ui|{
                        ui.spacing_mut().item_spacing.x=0.;let fill=if selected{Color32::from_gray(48)}else{HEADER};
                        let close_button=|ui:&mut egui::Ui|ui.add_sized([24.,26.],egui::Button::new("×").fill(fill).stroke(egui::Stroke::NONE).corner_radius(0)).on_hover_text(format!("{} {name}",tr(self.chinese,"关闭","Close")));
                        let leading_close=if cfg!(target_os="macos"){Some(close_button(ui))}else{None};
                        let r=ui.add_sized([desired.clamp(100.,220.),26.],egui::Button::new(egui::RichText::new(&name).size(14.).color(if selected{Color32::WHITE}else{Color32::from_gray(185)})).fill(fill).stroke(egui::Stroke::NONE).corner_radius(0).truncate());
                        record_widget!(w,format!("document-{id}"),rect(r.rect));
                        if r.on_hover_text(path.unwrap_or(&name)).clicked(){action=Some((id.to_owned(),false));}
                        let close=leading_close.unwrap_or_else(||close_button(ui));
                        record_widget!(w,format!("document-close-{id}"),rect(close.rect));if selected{record_widget!(w,"document_close".into(),rect(close.rect));}
                        if close.clicked(){action=Some((id.to_owned(),true));}
                    }).response.rect;
                    if selected{ui.painter().line_segment([tab.left_bottom(),tab.right_bottom()],egui::Stroke::new(2.,ACCENT));if reveal{ui.scroll_to_rect(tab,Some(egui::Align::Center));}}
                }});
            });record_widget!(w,"document_tabs".into(),rect(tabs.inner_rect));
        });
        if let Some((id,close))=action{if close{self.close_document_tab(ui.ctx(),&id);}else{self.activate_document(&id);}}
    }
    pub(super) fn activate_document(&mut self,id:&str)->bool{
        if id==self.active{return true;}
        if self.service.snapshot(id).is_err(){return false;}
        let Ok(s)=self.service.snapshot(&self.active)else{return false;};
        if self.prepare_navigation(&s).is_none(){return false;}
        if self.inline_text.is_some(){self.tool=Tool::Select;}
        self.remember_document_path();
        self.load_document(id);true
    }
    pub(super) fn load_document(&mut self,id:&str){
        self.viewport_cache=None;
        self.active=id.into();self.set_selection(vec![]);self.page="page-1".into();self.fit=true;self.pan=Vec2::ZERO;self.page_draft_dirty=false;self.page_draft_key.clear();self.pen.clear();self.gesture=None;if let Ok(s)=self.service.snapshot(id){self.path=self.suggested_document_path(&s);}
    }
    fn menu(&mut self,ui:&mut egui::Ui,s:&Snapshot,w:&mut BTreeMap<String,Value>,key:&str){let zh=self.chinese;match key{
        "file"=>{for(key,z,e)in[("new","新建","New"),("open","打开…","Open…"),("save","保存","Save"),("save_as","另存为…","Save as…"),("print","打印／预览…","Print / Preview…")]{if button(ui,w,key,tr(zh,z,e),!s.busy){if key=="print"{self.print_preview(s);}else{self.file_action(s,key);}ui.close();}}let recent=ui.menu_button(tr(zh,"最近文档","Recent documents"),|ui|self.recent_menu(ui,w));record_widget!(w,"menu-recent".into(),rect(recent.response.rect));if button(ui,w,"recover_source",tr(zh,"从原稿备份恢复…","Recover source backup…"),!s.busy){self.file_action(s,"recover_source");ui.close();}ui.separator();if button(ui,w,"close_document",tr(zh,"关闭当前文档","Close current document"),true){self.request_close(ui.ctx(),false);ui.close();}if button(ui,w,"quit",tr(zh,"退出 Arranger","Quit Arranger"),true){self.request_close(ui.ctx(),true);ui.close();}ui.separator();ui.menu_button(tr(zh,"导出","Export"),|ui|self.exports_menu(ui,s,w));},
        "edit"|"object"=>self.context_menu(ui,s,w),
        "export"=>self.exports_menu(ui,s,w),
        "path"=>{if button(ui,w,"to_curves",editing::curves_label(zh),!s.busy&&!self.selection.is_empty()){self.edit_action(ui.ctx(),s,"to_curves");ui.close();}for(key,z,e)in[("to_path","形状转路径","Convert to path"),("union","并集","Union"),("intersection","交集","Intersection"),("difference","差集","Difference"),("xor","异或","Xor")]{if button(ui,w,key,tr(zh,z,e),!s.busy&&if key=="to_path"{!self.selection.is_empty()}else{self.selection.len()==2}){self.edit_action(ui.ctx(),s,key);ui.close();}}},
        "view"=>{for(key,z,e)in[("fit","适合页面","Fit page"),("actual_size","实际大小","Actual size"),("zoom_in","放大","Zoom in"),("zoom_out","缩小","Zoom out")]{if button(ui,w,key,tr(zh,z,e),true){match key{"fit"=>{self.fit=true;self.pan=Vec2::ZERO;},"actual_size"=>{self.fit=false;self.zoom=1.;self.pan=Vec2::ZERO;},"zoom_in"=>self.zoom_by(1.25,None,&s.document),_=>self.zoom_by(0.8,None,&s.document)}ui.close();}}},
        "layout"=>{if button(ui,w,"page_settings",tr(zh,"页面设置","Page setup"),true){self.inspector_tab=1;ui.close();}for(key,z,e)in[("page_add","添加A4页面","Add A4 page"),("page_delete","删除当前页","Delete current page")]{if button(ui,w,key,tr(zh,z,e),!s.busy&&(key!="page_delete"||s.document.pages.len()>1)){self.edit_action(ui.ctx(),s,key);ui.close();}}},
        "text"=>{if button(ui,w,"menu-text-tool",tr(zh,"文字工具","Text tool"),true){self.tool=Tool::Text;ui.close();}if button(ui,w,"menu-text-curves",editing::curves_label(zh),!self.selection.is_empty()&&!s.busy){self.edit_action(ui.ctx(),s,"to_curves");ui.close();}},
        "tools"=>{for(tool,key,z,e)in Tool::items(){if button(ui,w,&format!("menu-tool-{key}"),tr(zh,z,e),true)&&self.finish_text(ui.ctx(),s){self.tool=tool;self.inspector_tab=0;ui.close();}}},
        "window"=>{let docs=self.service.command(Command::Documents).unwrap_or_default();if let Some(docs)=docs["documents"].as_array(){for d in docs{if let Some(id)=d["document_id"].as_str(){if ui.button(title(d["saved_path"].as_str().or_else(||self.known_document_path(id)),id)).clicked(){self.activate_document(id);ui.close();}}}}},
        "help"=>{ui.label("Arranger 0.1.0");ui.label(tr(zh,"V 选择 · A 节点 · T 文字","V Select · A Nodes · T Text"));ui.label(tr(zh,"双击边缘加点，双击节点删点","Double-click edge to add, node to delete"));ui.label(if cfg!(target_os="macos"){tr(zh,"⌘⇧O 转曲 · ⌘S 保存原稿","⌘⇧O Curves · ⌘S Save original")}else{tr(zh,"Ctrl+Q 转曲 · Ctrl+S 保存原稿","Ctrl+Q Curves · Ctrl+S Save original")});},_=>{}
    }}
    pub(super) fn toolbar(&mut self,ui:&mut egui::Ui,s:&Snapshot,w:&mut BTreeMap<String,Value>)->Rect{
        let area=egui::Panel::top("view-toolbar").frame(frame(Color32::from_rgb(250,250,251),10,3)).show(ui,|ui|{
            ui.spacing_mut().button_padding=Vec2::new(7.,2.);ui.spacing_mut().interact_size.y=26.;
            for style in [egui::TextStyle::Body,egui::TextStyle::Button]{ui.style_mut().text_styles.insert(style,egui::FontId::proportional(14.));}
            let zh=self.chinese;let tool=self.tool;let expanded=self.ribbon_expanded[tool as usize];
            ui.horizontal(|ui|{
                ui.set_height(26.);let heading=ui.label(egui::RichText::new(if tool==Tool::Nodes{tr(zh,"节点编辑","Nodes")}else{tool.label(zh)}).strong().size(14.));ui.painter().line_segment([heading.rect.left_bottom()+Vec2::new(0.,3.),heading.rect.right_bottom()+Vec2::new(0.,3.)],egui::Stroke::new(2.,ACCENT));ui.separator();
                // Reserve the right edge for the disclosure. Narrow windows
                // scroll the controls, never push the disclosure off-screen.
                let remaining=(ui.available_width()-82.).max(120.);
                ui.allocate_ui(Vec2::new(remaining,26.),|ui|{ui.set_min_width(remaining);ui.style_mut().wrap_mode=Some(egui::TextWrapMode::Extend);egui::ScrollArea::horizontal().id_salt("ribbon-scroll").max_height(30.).show(ui,|ui|{ui.horizontal(|ui|self.common_tools(ui,s,w));});});
                let label=if expanded{tr(zh,"收起","Less")}else{tr(zh,"更多","More")};
                if icon_button(ui,w,"ribbon_toggle",if expanded{"chevron-up"}else{"chevron-down"},label,true,false).clicked(){self.ribbon_expanded[tool as usize]=!expanded;}
            });
            if expanded{ui.separator();ui.horizontal_wrapped(|ui|self.advanced_tools(ui,s,w));}
        }).response.rect;
        // Disclosure and typography belong to the current edit session.
        // text_boundary runs before layout and uses this previous-frame area.
        self.text_actions=if self.tool==Tool::Text{area}else{Rect::NOTHING};area
    }
    fn common_tools(&mut self,ui:&mut egui::Ui,s:&Snapshot,w:&mut BTreeMap<String,Value>){let zh=self.chinese;let enabled=!s.busy;
        match self.tool{
            Tool::Select=>{for(key,image,z,e)in[("new","file-plus","新建","New"),("open","folder-open","打开","Open"),("save","save","保存","Save"),("save_as","copy","另存为","Save as"),("print","printer","打印／预览","Print / Preview")]{if icon_button(ui,w,key,image,tr(zh,z,e),enabled,false).clicked(){if key=="print"{self.print_preview(s);}else{self.file_action(s,key);}}}ui.separator();for(key,image,z,e)in[("cut","scissors","剪切","Cut"),("copy","copy","复制","Copy"),("paste","copy","粘贴","Paste"),("group","link","编组","Group")]{if icon_button(ui,w,&format!("ribbon-{key}"),image,tr(zh,z,e),enabled&&(key=="paste"||!self.selection.is_empty()),false).clicked(){self.edit_action(ui.ctx(),s,key);}}},
            Tool::Nodes=>{for(key,image,z,e)in[("add","plus","添加节点","Add node"),("delete","trash-2","删除节点","Delete node"),("cusp","corner-up-right","尖角","Cusp"),("smooth","spline","平滑","Smooth"),("symmetric","spline","对称","Symmetric"),("open","unlink","断开路径","Break path"),("close","link","连接端点","Join ends")]{if icon_button(ui,w,&format!("ribbon-node-{key}"),image,tr(zh,z,e),enabled&&self.node_target(s).is_some(),key==self.node_kind).clicked(){self.node_action(s,key);}}ui.separator();let r=ui.checkbox(&mut self.node_snapping,tr(zh,"吸附","Snap"));record_widget!(w,"ribbon-node-snapping".into(),rect(r.rect));},
            Tool::Text=>{self.text_toolbar(ui,s,w);},
            Tool::Pen=>{for(key,z,e)in[("pen_finish","完成路径","Finish path"),("pen_close","闭合路径","Close path")]{if button(ui,w,key,tr(zh,z,e),enabled&&self.pen.len()>if key=="pen_close"{2}else{1}){self.finish_pen(s,key=="pen_close");}}},
            Tool::Pan=>{if button(ui,w,"fit",tr(zh,"适合页面","Fit page"),true){self.fit=true;self.pan=Vec2::ZERO;}ui.label(tr(zh,"双指移动 · 捏合缩放","Two fingers to pan · pinch to zoom"));},
            _=>{ui.label(tr(zh,"拖动绘制 · 右侧设置外观","Drag to draw · set appearance on the right"));if button(ui,w,"ribbon-to-curves",editing::curves_label(zh),!self.selection.is_empty()&&enabled){self.edit_action(ui.ctx(),s,"to_curves");}}
        }
    }
    fn advanced_tools(&mut self,ui:&mut egui::Ui,s:&Snapshot,w:&mut BTreeMap<String,Value>){let zh=self.chinese;
        match self.tool{
            Tool::Nodes=>{for(key,z,e)in[("line","转为直线","To line"),("curve","转为曲线","To curve"),("reverse","反转方向","Reverse"),("open","断开闭合路径","Open subpath"),("close","闭合路径","Close subpath")]{if button(ui,w,&format!("ribbon-node-{key}"),tr(zh,z,e),!s.busy&&self.node_target(s).is_some()){self.node_action(s,key);}}let r=ui.checkbox(&mut self.show_node_handles,tr(zh,"显示控制柄","Show handles"));record_widget!(w,"ribbon-node-show-handles".into(),rect(r.rect));},
            Tool::Select=>{for(key,z,e)in[("duplicate","再制一份","Duplicate"),("delete","删除","Delete"),("ungroup","解组","Ungroup"),("front","置于顶层","Bring to front"),("back","置于底层","Send to back"),("lock","锁定","Lock")]{if button(ui,w,&format!("ribbon-{key}"),tr(zh,z,e),!s.busy&&!self.selection.is_empty()){self.edit_action(ui.ctx(),s,key);}}if button(ui,w,"page_settings",tr(zh,"页面设置","Page setup"),true){self.inspector_tab=1;}},
            Tool::Text=>{self.text_advanced(ui,s,w);},_=>{if button(ui,w,"page_settings",tr(zh,"页面设置","Page setup"),true){self.inspector_tab=1;}}
        }
    }
    fn text_toolbar(&mut self,ui:&mut egui::Ui,s:&Snapshot,w:&mut BTreeMap<String,Value>){let zh=self.chinese;
        if self.inline_text.is_some(){ui.horizontal(|ui|{if button(ui,w,"text_finish",tr(zh,"完成文字","Finish text"),!s.busy){self.finish_text(ui.ctx(),s);}if button(ui,w,"text_cancel",tr(zh,"取消文字","Cancel text"),true){self.cancel_text(ui.ctx());}});ui.separator();}
        if let Some(id)=self.selected.clone(){if let Ok(o)=s.document.object(&id){if self.editor.as_ref().is_none_or(|e|e.id!=id||e.revision!=s.revision&&!e.dirty){self.editor=Some(Editor::new(&s.document,o,s.revision));}}}
        let text_editor=self.editor.as_mut().filter(|e|matches!(e.geometry,Geometry::Text{..}));
        if let Some(editor)=text_editor{if let Geometry::Text{font_family,font_size_mm,layout,..}=&mut editor.geometry{let changed=font_controls(ui,w,zh,font_family,font_size_mm,layout);editor.dirty|=changed;editor.text_dirty|=changed&&self.inline_text.is_some();}}
        else{font_controls(ui,w,zh,&mut self.ui_font,&mut self.new_text_size,&mut self.new_text_layout);}
        if button(ui,w,"ribbon-text-apply",tr(zh,"应用排版","Apply typography"),!s.busy&&self.editor.as_ref().is_some_and(|e|e.dirty&&matches!(e.geometry,Geometry::Text{..}))){self.apply_editor(s);if !self.error{self.end_text_input(ui.ctx());}}
    }
    fn text_advanced(&mut self,ui:&mut egui::Ui,s:&Snapshot,w:&mut BTreeMap<String,Value>){let zh=self.chinese;
        let text_editor=self.editor.as_mut().filter(|e|matches!(e.geometry,Geometry::Text{..}));
        if let Some(e)=text_editor{if let Geometry::Text{layout,..}=&mut e.geometry{let changed=spacing_controls(ui,w,zh,layout);e.dirty|=changed;e.text_dirty|=changed&&self.inline_text.is_some();}}
        else{spacing_controls(ui,w,zh,&mut self.new_text_layout);}
        if button(ui,w,"ribbon-text-curves",editing::curves_label(zh),!s.busy&&!self.selection.is_empty()){self.edit_action(ui.ctx(),s,"to_curves");}
    }
    pub(super) fn footer(&mut self,ui:&mut egui::Ui,s:&Snapshot,w:&mut BTreeMap<String,Value>)->Rect{let zh=self.chinese;
        egui::Panel::bottom("status-bar").frame(frame(Color32::WHITE,12,5)).show(ui,|ui|{ui.horizontal(|ui|{
            let doc=&s.document;let index=doc.pages.iter().position(|p|p.id==self.page).unwrap_or(0);ui.label(tr(zh,"页面","Page"));
            let r=egui::ComboBox::from_id_salt("pages").width(65.).selected_text(format!("{} / {}",index+1,doc.pages.len())).show_ui(ui,|ui|{for(index,page)in doc.pages.iter().enumerate(){let r=ui.selectable_label(self.page==page.id,format!("{} · {}×{} mm",index+1,editing::display_number(page.width_mm),editing::display_number(page.height_mm)));record_widget!(w,format!("page-{}",page.id),rect(r.rect));if r.clicked(){self.activate_page(ui.ctx(),&page.id);}}});record_widget!(w,"pages".into(),rect(r.response.rect));
            if let Ok(p)=doc.page(&self.page){ui.label(egui::RichText::new(format!("{} × {} mm",editing::display_number(p.width_mm),editing::display_number(p.height_mm))).small());}ui.separator();
            for color in [Color32::BLACK,Color32::WHITE,ACCENT]{let(r,_)=ui.allocate_exact_size(Vec2::splat(17.),egui::Sense::hover());ui.painter().rect_filled(r,2.,color);ui.painter().rect_stroke(r,2.,egui::Stroke::new(1.,LINE),egui::StrokeKind::Inside);}
            ui.separator();ui.allocate_ui(Vec2::new((ui.available_width()-260.).max(40.),24.),|ui|{let text=self.status_text();let r=ui.add(egui::Label::new(egui::RichText::new(text).small().color(if self.error{Color32::from_rgb(182,50,57)}else{MUTED})).truncate());record_widget!(w,"status_hint".into(),rect(r.rect));if let Some(p)=self.file_problem.as_ref().filter(|p|self.error&&p.raw==self.message){r.on_hover_ui(|ui|{ui.set_max_width(480.);ui.label(&p.hint);ui.separator();egui::ScrollArea::vertical().max_height(220.).show(ui,|ui|{ui.label(egui::RichText::new(&p.raw).small());});});}else{r.on_hover_text(text);}});
            if self.notice_hash==s.document_hash&&!self.render_notices.is_empty(){if button(ui,w,"document_notices",tr(zh,"提示","Notices"),true){self.show_notices=true;}}
            if button(ui,w,"zoom_out","−",true){self.zoom_by(0.8,None,doc);}ui.label(format!("{:.0}%",self.zoom*100.));if button(ui,w,"zoom_in","+",true){self.zoom_by(1.25,None,doc);}if button(ui,w,"fit",tr(zh,"适合","Fit"),true){self.fit=true;self.pan=Vec2::ZERO;}
        });}).response.rect
    }
    pub(super) fn rail(&mut self,ui:&mut egui::Ui,s:&Snapshot,w:&mut BTreeMap<String,Value>)->Rect{
        egui::Panel::left("tool-rail").exact_size(64.).resizable(false).frame(frame(Color32::from_rgb(248,248,249),5,8)).show(ui,|ui|{
            for(tool,key,z,e)in Tool::items(){let image=match tool{Tool::Select=>"mouse-pointer-2",Tool::Nodes=>"spline",Tool::Pan=>"hand",Tool::Rect=>"square",Tool::Ellipse=>"circle",Tool::Line=>"minus",Tool::Pen=>"pen-tool",Tool::Text=>"type"};let r=ui.add_sized([52.,46.],egui::Button::new("").selected(self.tool==tool).frame(self.tool==tool));icon(ui,image,Rect::from_center_size(r.rect.center(),Vec2::splat(26.)),if self.tool==tool{Color32::WHITE}else{INK});record_widget!(w,format!("tool-{key}"),rect(r.rect));if r.clicked()&&self.finish_text(ui.ctx(),s){self.tool=tool;self.selection_mode=Default::default();self.rotation_center=None;self.inspector_tab=0;self.gesture=None;ui.ctx().memory_mut(|m|m.stop_text_input());}r.on_hover_text(tr(self.chinese,z,e));}ui.separator();if icon_button(ui,w,"image","image","",!s.busy,false).on_hover_text(tr(self.chinese,"导入图片","Import image")).clicked(){self.file_action(s,"image");}
        }).response.rect
    }
    fn print_preview(&mut self,s:&Snapshot){
        // Reuse the validated vector PDF route. Printing remains explicit in
        // the OS preview, never a background physical printer operation.
        self.file_action(s,"print");self.message=tr(self.chinese,"打印使用矢量PDF；导出后用系统预览打印","Print via vector PDF; open the exported PDF in the system preview").into();
    }
}

/// The same controls edit an existing draft or defaults for the next story.
/// Default settings never write a document transaction.
fn font_controls(ui:&mut egui::Ui,w:&mut BTreeMap<String,Value>,zh:bool,font:&mut String,size:&mut f64,layout:&mut arranger_core::TextLayoutOptions)->bool{
    let mut changed=false;
    ui.label(tr(zh,"字体","Font"));let r=ui.add(egui::TextEdit::singleline(font).id_salt("ribbon-font").desired_width(140.));record_widget!(w,"ribbon-font".into(),rect(r.rect));changed|=r.changed();
    ui.label(tr(zh,"字号","Size"));let mut pt=*size*72./25.4;let r=ui.add(egui::DragValue::new(&mut pt).range(0.1..=3000.).suffix(" pt"));record_widget!(w,"ribbon-font-size".into(),rect(r.rect));if r.changed(){*size=pt*25.4/72.;changed=true;}
    ui.separator();for (align,key,label) in [(arranger_core::TextAlign::Left,"left",tr(zh,"左对齐","Left")),(arranger_core::TextAlign::Center,"center",tr(zh,"居中","Center")),(arranger_core::TextAlign::Right,"right",tr(zh,"右对齐","Right"))]{let r=ui.selectable_label(layout.align==align,label);record_widget!(w,format!("ribbon-text-align-{key}"),rect(r.rect));if r.clicked(){layout.align=align;changed=true;}}
    changed
}
fn spacing_controls(ui:&mut egui::Ui,w:&mut BTreeMap<String,Value>,zh:bool,layout:&mut arranger_core::TextLayoutOptions)->bool{
    ui.label(tr(zh,"字距","Letter spacing"));let r=ui.add(egui::DragValue::new(&mut layout.letter_spacing_mm).speed(0.01).suffix(" mm"));record_widget!(w,"ribbon-letter-spacing".into(),rect(r.rect));let mut changed=r.changed();
    ui.label(tr(zh,"行距","Line height"));let mut line=layout.line_height_mm.unwrap_or(0.);let r=ui.add(egui::DragValue::new(&mut line).range(0. ..=1000.).suffix(" mm"));record_widget!(w,"ribbon-line-height".into(),rect(r.rect));if r.changed(){layout.line_height_mm=(line>0.).then_some(line);changed=true;}
    changed
}
