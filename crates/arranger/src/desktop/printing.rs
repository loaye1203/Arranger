use super::*;

pub(super) struct PdfPlan{
    pub document:String,pub revision:u64,pub hash:String,
    pub pages:Vec<arranger_core::Page>,pub current:String,pub scope:u8,pub range:String,
}
impl PdfPlan{
    pub(super) fn new(document:String,s:&Snapshot,current:&str)->Self{
        let index=s.document.pages.iter().position(|p|p.id==current).unwrap_or(0)+1;
        Self{document,revision:s.revision,hash:s.document_hash.clone(),pages:s.document.pages.clone(),current:current.into(),scope:0,range:index.to_string()}
    }
    pub(super) fn selected(&self)->Result<Vec<String>,String>{
        match self.scope{
            0=>Ok(self.pages.iter().map(|p|p.id.clone()).collect()),
            1=>if self.pages.iter().any(|p|p.id==self.current){Ok(vec![self.current.clone()])}else{Err("当前页面已不存在 / Current page is unavailable".into())},
            _=>page_numbers(&self.range,self.pages.len()).map(|ns|ns.into_iter().map(|i|self.pages[i-1].id.clone()).collect()),
        }
    }
    pub(super) fn valid(&self,app:&App)->bool{
        app.active==self.document&&app.service.snapshot(&self.document).is_ok_and(|s|!s.busy&&s.revision==self.revision&&s.document_hash==self.hash)
    }
    #[cfg(feature="test-hooks")]
    pub(super) fn status(&self)->Value{json!({"document_id":self.document,"revision":self.revision,"scope":self.scope,"range":self.range,"selected":self.selected().ok()})}
}
fn page_numbers(text:&str,count:usize)->Result<Vec<usize>,String>{
    let error=||"页码无效，请使用 1,3-5，且不超出文档页数 / Use page numbers such as 1,3-5 within this document".to_string();
    let mut numbers=std::collections::BTreeSet::new();
    for part in text.split(','){
        let parts:Vec<_>=part.trim().split('-').collect();if parts.is_empty()||parts.len()>2{return Err(error());}
        let parse=|v:&str|->Result<usize,String>{let v=v.trim();if v.is_empty()||!v.bytes().all(|b|b.is_ascii_digit()){return Err(error());}v.parse().map_err(|_|error())};
        let start=parse(parts[0])?;let end=if parts.len()==2{parse(parts[1])?}else{start};
        if start==0||start>end||end>count{return Err(error());}numbers.extend(start..=end);
    }
    if numbers.is_empty(){return Err(error());}Ok(numbers.into_iter().collect())
}
impl App{
    pub(super) fn pdf_options(&self,ui:&mut egui::Ui,plan:&mut PdfPlan,widgets:&mut BTreeMap<String,Value>)->bool{
        let zh=self.chinese;
        ui.horizontal(|ui|{for (scope,z,en)in[(0,"全部页面","All pages"),(1,"当前页","Current page"),(2,"页码范围","Page range")]{let r=ui.radio_value(&mut plan.scope,scope,tr(zh,z,en));record_widget!(widgets,format!("pdf_scope_{scope}"),rect(r.rect));}});
        if plan.scope==2{let r=ui.add(egui::TextEdit::singleline(&mut plan.range).return_key(None).hint_text("1,3-5").desired_width(240.));record_widget!(widgets,"pdf_range".into(),rect(r.rect));}
        let valid=plan.valid(self);if !valid{ui.colored_label(Color32::RED,tr(zh,"文档已变化，请取消并重新选择页面","Document changed; cancel and select pages again"));return false;}
        match plan.selected(){Ok(ids)=>{
            ui.label(egui::RichText::new(format!("{} {} · 100%",ids.len(),tr(zh,"页","pages"))).small().color(MUTED));true
        },Err(e)=>{ui.colored_label(Color32::RED,e);false}}
    }
    pub(super) fn pdf_details(&self,ui:&mut egui::Ui,plan:&PdfPlan){
        let zh=self.chinese;let Ok(ids)=plan.selected()else{return;};
        for(i,p)in plan.pages.iter().enumerate().filter(|(_,p)|ids.contains(&p.id)){ui.label(egui::RichText::new(format!("{} {} · {} × {} mm",tr(zh,"页","Page"),i+1,p.width_mm,p.height_mm)).small().color(MUTED));}
        ui.label(egui::RichText::new(tr(zh,"仅输出纸张范围。文字转为轮廓；彩色字形页面会提示栅格化。","Paper bounds only. Text exports as outlines; color-glyph pages report rasterization.")).small().color(MUTED));
    }
    pub(super) fn finish_pdf(&mut self,plan:&PdfPlan,path:PathBuf,overwrite:bool,preview:bool)->bool{
        if !plan.valid(self){self.error=true;self.message=tr(self.chinese,"文档已变化，请重新选择输出页面","Document changed; select output pages again").into();return false;}
        let pages=match plan.selected(){Ok(p)=>p,Err(e)=>{self.error=true;self.message=e;return false;}};
        let path=match std::path::absolute(path){Ok(p)=>p,Err(e)=>{self.error=true;self.message=e.to_string();return false;}};
        let result=self.file_act(Command::PdfExport{document_id:plan.document.clone(),expected_revision:Some(plan.revision),page_ids:Some(pages),path:path.to_string_lossy().into_owned(),overwrite,dpi:300.},&files::FileKind::Export(if preview{"print"}else{"pdf"}.into()),&path,"PDF exported");
        let Some(result)=result else{return false;};
        self.message=format!("PDF · {} {} · 100%",result["page_count"],tr(self.chinese,"页","pages"));
        if result["warnings"].as_array().is_some_and(|w|w.iter().any(|n|n["code"]=="PDF_RASTERIZED")){self.message.push_str(tr(self.chinese," · 含彩色字形的页面已栅格化"," · Pages with color glyphs were rasterized"));}
        if preview{
            #[cfg(target_os="macos")]{
                #[cfg(feature="test-hooks")]let open=self.test_output.is_none();
                #[cfg(not(feature="test-hooks"))]let open=true;
                if open{if let Err(e)=std::process::Command::new("/usr/bin/open").args(["-a","Preview"]).arg(&path).spawn(){self.message.push_str(&format!(" · PDF已生成；系统预览未打开：{e}"));return true;}}
            }
            self.message.push_str(tr(self.chinese," · 请在PDF阅读器选择实际大小和打印机"," · Choose Actual Size and your printer in the PDF viewer"));
        }true
    }
}
#[cfg(test)]mod tests{
    #[test]fn ranges_validate_deduplicate_and_keep_document_order(){
        assert_eq!(super::page_numbers(" 3,1-2,2 ",4).unwrap(),vec![1,2,3]);
        for s in["","0","-1","2-1","1,","1-5","1--2","+1","1.5","999999999999999999999999"]{assert!(super::page_numbers(s,4).is_err(),"{s}");}
    }
}
