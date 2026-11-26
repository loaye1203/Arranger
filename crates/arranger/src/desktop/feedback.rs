use arranger_core::command::Notice;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Clone)]
pub(super) struct FileProblem {
    pub kind:super::files::FileKind,pub path:std::path::PathBuf,pub code:String,pub raw:String,pub summary:String,pub hint:String,
}
impl FileProblem {
    pub(super) fn new(kind:&super::files::FileKind,path:&std::path::Path,error:arranger_core::Error,zh:bool)->Self{
        use super::{files::FileKind,tr};
        let read=matches!(kind,FileKind::Open|FileKind::Image);
        let(summary,hint)=match error.code.as_str(){
            "DISK_CONFLICT"=>(tr(zh,"文件已被其他程序修改","The file changed outside Arranger"),tr(zh,"请另存副本保留当前编辑，再核对原文件。","Save a copy to keep your edits, then review the original.")),
            "FILE_BUSY"=>(tr(zh,"文件正在使用","The file is in use"),tr(zh,"等待另一次写入结束后重试，或另选位置。","Wait for the other write to finish, or choose another location.")),
            "DOCUMENT_BUSY"=>(tr(zh,"文档仍在编辑操作中","An edit is still in progress"),tr(zh,"完成或取消当前操作后重试。","Finish or cancel the current operation, then retry.")),
            "OUTPUT_EXISTS"=>(tr(zh,"同名文件已存在","A file with this name exists"),tr(zh,"更换名称，或明确确认替换。","Choose another name, or explicitly confirm replacement.")),
            "UNSUPPORTED_FILE_VERSION"=>(tr(zh,"当前版本无法读取此文件","This file version cannot be read"),tr(zh,"请选择支持的文件，或使用创建它的软件版本打开。","Choose a supported file, or open it with the version that created it.")),
            "CORRUPT_FILE"|"CHECKSUM_MISMATCH"|"INVALID_CDR"|"INVALID_DOCUMENT"=>(tr(zh,"文件内容无法读取","The file contents cannot be read"),tr(zh,"请重新选择文件；有可用备份时可恢复为副本。","Choose the file again, or recover a copy from an available backup.")),
            "UNRECOVERABLE_FILE"=>(tr(zh,"原文件和备份均未能读取","Neither the file nor its backup could be read"),tr(zh,"请选择其他文件或另一份备份。","Choose another file or backup.")),
            "INVALID_RESOURCE"|"ENCODING_ERROR" if read=>(tr(zh,"图片无法读取","The image cannot be read"),tr(zh,"请选择有效的PNG或JPEG图片。","Choose a valid PNG or JPEG image.")),
            "CDR_REQUIRES_VERSION21"=>(tr(zh,"此内容需要CDR 21.0","This content requires CDR 21.0"),tr(zh,"请明确改用21.0后重新导出。","Select version 21.0 and export again.")),
            "CDR_UNSUPPORTED"|"CDR_SAVE_NOT_READY"|"CDR_IMPORT_NOT_READY"=>(tr(zh,"当前方式不支持部分内容","Some content is not supported in this mode"),tr(zh,"请检查版本和兼容说明，完整原稿可保存为Arranger。","Review the version and compatibility details; keep a complete Arranger source.")),
            "FILE_TYPE"=>(tr(zh,"文件类型不匹配","The file type does not match"),tr(zh,"请按当前格式选择文件或修改扩展名。","Choose a file or extension that matches the selected format.")),
            "FILE_DIRECTORY"=>(tr(zh,"该名称指向一个文件夹","This name points to a folder"),tr(zh,"请换一个文件名称，保留现有文件夹。","Choose another file name; keep the existing folder.")),
            "NATIVE_FILE_PANEL"|"FILE_PANEL_RESULT"=>(tr(zh,"文件位置未能确认","The file location could not be confirmed"),tr(zh,"请重新选择，或取消后再试。","Choose the location again, or cancel and retry.")),
            "IO_ERROR" if read=>(tr(zh,"无法读取此文件","The file could not be read"),tr(zh,"请检查文件位置和访问权限，再重新选择。","Check the file location and access, then choose it again.")),
            "IO_ERROR"=>(tr(zh,"无法写入此位置","The file could not be written here"),tr(zh,"请检查文件夹是否可写，或另选位置重试。","Check that the folder is writable, or choose another location.")),
            _=>(if read{tr(zh,"未能打开此文件","The file could not be opened")}else{tr(zh,"文件输出未完成","The file operation did not complete")},tr(zh,"请核对文件和选项后重试，详细原因可在下方查看。","Review the file and options, then retry. Details are available below.")),
        };
        Self{kind:kind.clone(),path:path.to_owned(),code:error.code.clone(),raw:error.to_string(),summary:summary.into(),hint:hint.into()}
    }
    pub(super) fn batch(problems:&[Self],opened:usize,raw:String,zh:bool)->Self{
        let mut p=problems[0].clone();p.code="OPEN_ERRORS".into();p.raw=raw;p.summary=if zh{format!("已打开{opened}份，{}份未打开",problems.len())}else{format!("Opened {opened}; {} files did not open",problems.len())};p.hint=super::tr(zh,"请重新选择未打开的文件；已打开的文档保留。","Choose the failed files again; opened documents stay open.").into();p
    }
    pub(super) fn can_copy(&self)->bool{matches!(self.kind,super::files::FileKind::Save|super::files::FileKind::Export(_))&&self.code=="DISK_CONFLICT"}
    pub(super) fn is_io(&self)->bool{matches!(self.code.as_str(),"IO_ERROR"|"DISK_CONFLICT"|"FILE_BUSY"|"OUTPUT_EXISTS"|"FILE_TYPE"|"FILE_DIRECTORY"|"NATIVE_FILE_PANEL"|"FILE_PANEL_RESULT")}
    pub(super) fn backup(&self)->Option<bool>{
        if !matches!(self.kind,super::files::FileKind::Open)||!matches!(self.code.as_str(),"IO_ERROR"|"CORRUPT_FILE"|"CHECKSUM_MISMATCH"|"INVALID_CDR"|"UNRECOVERABLE_FILE"){return None;}
        let ext=self.path.extension()?.to_str()?.to_ascii_lowercase();if !matches!(ext.as_str(),"arranger"|"cdr"){return None;}
        let mut path=self.path.as_os_str().to_owned();path.push(".bak");std::path::PathBuf::from(path).is_file().then_some(ext=="arranger")
    }
}

impl super::App {
    pub(super) fn ui_message(&self)->&str{self.file_problem.as_ref().filter(|p|self.error&&p.raw==self.message).map(|p|p.summary.as_str()).unwrap_or_else(||super::message_text(&self.message,self.chinese))}
    pub(super) fn clear_file_problem(&mut self){if self.file_problem.as_ref().is_some_and(|p|p.raw==self.message){self.error=false;self.message=super::tr(self.chinese,"Ready","Ready").into();}self.file_problem=None;}
}

#[derive(Clone,Serialize)]
pub(super) struct FontFeedback {
    pub requested:String,
    pub actual_fonts:Vec<String>,
    pub warnings:Vec<Notice>,
}
impl FontFeedback {
    pub fn from_measurement(requested:&str,measurement:&Value)->Self {
        let actual_fonts=measurement["fonts"].as_array().into_iter().flatten().filter_map(|font|
            font["families"].as_array().and_then(|f|f.first()).and_then(Value::as_str)
                .or_else(||font["post_script_name"].as_str()))
            .map(str::to_owned).collect::<BTreeSet<_>>().into_iter().collect();
        let warnings=measurement["warnings"].as_array().into_iter().flatten().filter_map(|n|
            Some(Notice{code:n["code"].as_str()?.into(),message:n["message"].as_str()?.into()})).collect();
        Self{requested:requested.into(),actual_fonts,warnings}
    }
}
pub(super) fn notice_text(notice:&Notice,chinese:bool)->&str {
    if !chinese{return &notice.message;}
    match notice.code.as_str() {
        "CDR_SOURCE_REQUIRED"=>"CDR 是交付副本；请保留完整可编辑的 .arranger 原稿。导出不改变原文档和撤销记录。",
        "CDR_EDITABLE_LAYOUT"=>"保编辑模式保留原生文字；Corel 的字体、字距和沿路径排版可能不同。需要当前排版时请选择保外观。",
        "CDR_TEXT_OUTLINED"=>"保外观副本的文字已转为矢量轮廓，在 Corel 中不能直接改字；原文字保留在 .arranger 原稿。",
        "IMAGE_PRECISION_REDUCED"=>"原高位深图片保留在 .arranger 及 SVG 原资源中；当前屏幕、PNG/PDF 使用 8 位预览，不能代表全部原始采样精度。",
        "IMAGE_METADATA_UNMANAGED"=>"原图片字节与元数据已保留；当前显示及 SVG/PNG/PDF 不执行 ICC 色彩管理或 EXIF 自动旋转，CMYK 位图也未校色，不代表印刷效果。",
        "CMYK_RGB_APPROXIMATION"=>"文档保留 CMYK 原通道；屏幕、SVG、PNG 和 PDF 使用未进行 ICC 管理的 RGB 近似色，不代表印刷分色或色彩校准。",
        "CDR_IMAGE_PLACEMENT_NORMALIZED" => "图片适配与裁剪已转换为可编辑位图和原生裁剪框；原框尺寸与适配参数已规范化，请保留 .arranger 原稿。",
        "CDR_TEXT_LAYOUT" => "保留可编辑的矩形区域文字、空行、左中右对齐和行距。原生百分比字距按本机字体空格宽度转换；Corel 会用其安装字体重新排版，溢出与新输出兼容性待验证，请保留 .arranger 原稿。",
        "CDR_CMYK_CHANNELS"=>"CDR 实验交换保留实色 CMYK 原通道及色彩模型；ICC、专色及新输出的 Corel 原生兼容性仍待验证。",
        "CDR_EXPERIMENTAL"=>"CDR 实验交换：请保留 .arranger 完整原稿。复杂内容仍未支持，新 Rust 输出待 CorelDRAW 验证；字体排版可能因设备变化。",
        "CDR_BACKUP_RECOVERED"=>"已将可验证的上一次 CDR 交换备份恢复为未保存文档；主文件和备份均未改动。请保存 .arranger 原稿或明确导出到新路径。",
        "CDR_IMAGE_PIXELS"=>"图片保留可编辑像素和实例共享；此交换过程不保留原 PNG/JPEG 容器字节及资源 ID。",
        "CDR_GROUP_NORMALIZED"=>"保留群组层级和层叠；组变换合入成员的页面坐标，原局部坐标/组框及组元数据不保留。新输出的原生兼容性待测。",
        "CDR_CLIP_NORMALIZED"=>"保留可编辑的 PowerClip 内容和裁剪路径；原生着色边框转成底色、裁剪内容组和顶层描边。边框与裁剪路径的联动需分别编辑，新输出待 Corel 验证。",
        "CDR_PAGE_ONLY"=>"已按明确选择仅导出当前页；其他页面和共享区不在此文件中。",
        "CDR_PAGES_ASSIGNED"=>"已恢复页面尺寸和对象归属；缺失、重复或不符合 ID 规则的页面名称已按文件顺序重新编号。",
        "CDR_WORKSPACE"=>"共享对象存放在原生非打印桌面层，跨页可见；只导出当前页面时不包含共享层。新输出待 Corel 验证。",
        "CDR_IDS_ASSIGNED"=>"部分原生对象名称不能直接作为 Arranger ID，已为导入对象分配新的稳定 ID。",
        "FONT_SUBSTITUTED"=>"请求的字体不可用，已替代显示。原字体名称仍保留，请查看文字属性中的实际字体。",
        "FONT_FALLBACK"=>"部分字符使用了回退字体，实际字体见文字属性。",
        "MISSING_GLYPH"=>"部分字符缺少字形。请换用包含这些字符的字体，再检查显示或转曲。",
        "TEXT_PATH_OVERFLOW"=>"文字超出导向路径，请调整字号、位置或路径长度。",
        "TEXT_AREA_OVERFLOW"=>"文字超出排版区域。内容仍保留，请扩大区域或调整文字。",
        "UNSUPPORTED_GLYPH_OUTLINE"=>"部分字形为彩色或位图，不能直接转为完整的单色路径。",
        "TEXT_WHITESPACE_NORMALIZED"=>"制表符按一个空格显示，原始文字仍保留。",
        _=>&notice.message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn feedback_keeps_requested_font_and_reports_actual_faces_and_notices(){
        let value=json!({"fonts":[{"families":["PingFang SC"]},{"families":["PingFang SC"]},{"families":[],"post_script_name":"FallbackFace"}],
            "warnings":[{"code":"FONT_SUBSTITUTED","message":"Missing requested font"}]});
        let info=FontFeedback::from_measurement("MissingFont",&value);
        assert_eq!(info.requested,"MissingFont");assert_eq!(info.actual_fonts,vec!["FallbackFace","PingFang SC"]);
        assert_eq!(info.warnings.len(),1);assert!(notice_text(&info.warnings[0],true).contains("替代"));
        assert_eq!(notice_text(&info.warnings[0],false),"Missing requested font");
    }
    #[test]
    fn unknown_notices_retain_their_actionable_message(){
        let notice=Notice{code:"NEW_NOTICE".into(),message:"Keep the full diagnostic".into()};
        assert_eq!(notice_text(&notice,true),notice.message);
    }
}
