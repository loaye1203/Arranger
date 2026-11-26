//! Small per-user recent-file list. UI tests and portable checks use their own
//! data directory; document I/O never depends on metadata persistence succeeding.
use super::*;
use serde::{Deserialize,Serialize};
use std::{fs::{self,OpenOptions},io::Write,time::{Instant,Duration}};
const LIMIT:usize=12;
#[derive(Clone,PartialEq,Eq)]
pub(super) struct Entry {pub path:PathBuf,pub key:String,pub label:String,pub exists:bool}
impl Entry {
 fn new(path:PathBuf)->Self{let exists=path.is_file();let label=format!("{} · {}",title(path.to_str(),"Document"),path.parent().and_then(|p|p.file_name()).unwrap_or_default().to_string_lossy()).replace(['\n','\r','\t']," ");let key=format!("recent-{}",arranger_core::model::sha256(path.to_string_lossy().as_bytes()));Self{path,key,label,exists}}
}
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {version:u32,files:Vec<PathBuf>}
pub(super) struct Recents {pub file:Option<PathBuf>,pub entries:Vec<Entry>,pub warning:Option<String>,pub generation:u64,next_refresh:Instant,saved:BTreeMap<String,(String,String)>}
fn accepted(p:&std::path::Path)->bool{p.is_absolute()&&p.extension().and_then(|e|e.to_str()).is_some_and(|e|matches!(e.to_ascii_lowercase().as_str(),"arranger"|"cdr"))}
fn normalized(p:&std::path::Path)->PathBuf{p.canonicalize().or_else(|_|std::path::absolute(p)).unwrap_or_else(|_|p.to_owned())}
fn read(file:&std::path::Path)->Result<Vec<PathBuf>,String>{
 let metadata=match fs::metadata(file){Ok(m)=>m,Err(e)if e.kind()==std::io::ErrorKind::NotFound=>return Ok(vec![]),Err(e)=>return Err(e.to_string())};
 if metadata.len()>65536{return Err("Recent-file record is too large".into());}
 let r:Record=serde_json::from_slice(&fs::read(file).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;if r.version!=1{return Err("Unsupported recent-file record version".into());}
 let mut paths=vec![];for p in r.files{if accepted(&p){let p=normalized(&p);if !paths.contains(&p){paths.push(p);if paths.len()==LIMIT{break;}}}}Ok(paths)
}
impl Recents {
 pub(super) fn for_app(test:Option<&std::path::Path>)->Self{
  let args:Vec<_>=std::env::args().collect();let custom=args.windows(2).find(|a|a[0]=="--data-dir").map(|a|PathBuf::from(&a[1]));
  let dir=custom.or_else(||test.map(|p|p.join("settings"))).or_else(||std::env::var_os("ARRANGER_DATA_DIR").filter(|v|!v.is_empty()).map(PathBuf::from)).or_else(||{
   #[cfg(target_os="macos")] {std::env::var_os("HOME").map(|p|PathBuf::from(p).join("Library/Application Support/Arranger"))}
   #[cfg(target_os="windows")] {std::env::var_os("APPDATA").map(|p|PathBuf::from(p).join("Arranger"))}
   #[cfg(not(any(target_os="macos",target_os="windows")))] {std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(||std::env::var_os("HOME").map(|p|PathBuf::from(p).join(".local/share"))).map(|p|p.join("arranger"))}
  });
  let file=dir.and_then(|p|std::path::absolute(p).ok()).map(|p|p.join("recent-files.json"));Self::load(file)
 }
 fn load(file:Option<PathBuf>)->Self{let mut this=Self{file,entries:vec![],warning:None,generation:0,next_refresh:Instant::now(),saved:BTreeMap::new()};this.refresh();this}
 fn install(&mut self,paths:Vec<PathBuf>,warning:Option<String>){let entries=paths.into_iter().map(Entry::new).collect::<Vec<_>>();if entries!=self.entries||warning!=self.warning{self.entries=entries;self.warning=warning;self.generation+=1;}}
 pub(super) fn refresh_if_due(&mut self){if Instant::now()>=self.next_refresh{self.refresh();}}
 fn refresh(&mut self){self.next_refresh=Instant::now()+Duration::from_secs(1);if let Some(file)=&self.file{match read(file){Ok(paths)=>self.install(paths,None),Err(error)=>self.install(self.entries.iter().map(|e|e.path.clone()).collect(),Some(error))}}}
 fn write(&self,touch:Option<&std::path::Path>)->Result<Vec<PathBuf>,String>{
  let file=self.file.as_ref().ok_or("Recent files are available only for this session")?;fs::create_dir_all(file.parent().unwrap()).map_err(|e|e.to_string())?;
  let lock=OpenOptions::new().create(true).truncate(false).read(true).write(true).open(file.with_file_name("recent-files.lock")).map_err(|e|e.to_string())?;lock.try_lock().map_err(|e|e.to_string())?;
  let mut paths=if touch.is_some(){read(file)?}else{vec![]};let before=paths.clone();if let Some(path)=touch{paths.retain(|p|p!=path);paths.insert(0,path.to_owned());paths.truncate(LIMIT);if paths==before{return Ok(paths);}}
  let data=serde_json::to_vec_pretty(&Record{version:1,files:paths.clone()}).map_err(|e|e.to_string())?;let pending=file.with_file_name(format!("recent-files.{}.pending",std::process::id()));
  let result=(||{let mut f=fs::File::create(&pending)?;f.write_all(&data)?;f.sync_all()?;fs::rename(&pending,file)})();if result.is_err(){let _=fs::remove_file(&pending);}result.map_err(|e:std::io::Error|e.to_string())?;Ok(paths)
 }
 pub(super) fn remember(&mut self,path:&std::path::Path){let path=normalized(path);if !accepted(&path){return;}match self.write(Some(&path)){Ok(paths)=>self.install(paths,None),Err(error)=>{let mut paths=self.entries.iter().map(|e|e.path.clone()).filter(|p|p!=&path).collect::<Vec<_>>();paths.insert(0,path);paths.truncate(LIMIT);self.install(paths,Some(error));}}self.next_refresh=Instant::now()+Duration::from_secs(1);}
 pub(super) fn observe_saved(&mut self,s:&Snapshot){if s.dirty||s.busy{return;}if let Some(path)=&s.saved_path{let state=(path.clone(),s.document_hash.clone());if self.saved.get(&s.document_id)!=Some(&state){self.saved.insert(s.document_id.clone(),state);self.remember(std::path::Path::new(path));}}}
 pub(super) fn forget_document(&mut self,id:&str){self.saved.remove(id);}
 pub(super) fn clear(&mut self){match self.write(None){Ok(paths)=>self.install(paths,None),Err(error)=>self.install(self.entries.iter().map(|e|e.path.clone()).collect(),Some(error))}}
 #[cfg(feature="test-hooks")]
 pub(super) fn status(&self)->Value{json!({"file":self.file,"warning":self.warning,"entries":self.entries.iter().map(|e|json!({"path":e.path,"key":e.key,"exists":e.exists})).collect::<Vec<_>>()})}
}
impl App {
 pub(super) fn recent_action(&mut self,key:&str)->bool{
  if key=="recent_clear"{self.recents.clear();return true;}
  if !key.starts_with("recent-"){return false;}
  if let Some(entry)=self.recents.entries.iter().find(|e|e.key==key){
   if !entry.path.is_file(){self.error=true;self.message=format!("{}：{}",tr(self.chinese,"找不到最近文件，请使用打开重新选择","Recent file is missing; use Open to locate it"),entry.path.display());}
   else{self.pending_files.push_back(entry.path.clone());}
  }else{self.error=true;self.message=tr(self.chinese,"最近记录已更新，请重新选择","Recent documents changed; choose again").into();}true
 }
 pub(super) fn recent_menu(&mut self,ui:&mut egui::Ui,w:&mut BTreeMap<String,Value>){
  if self.recents.entries.is_empty(){ui.label(tr(self.chinese,"暂无最近文档","No recent documents"));}
  for entry in self.recents.entries.clone(){let label=format!("{}{}",entry.label,if entry.exists{""}else{tr(self.chinese,"（找不到文件）"," (missing)")});let r=ui.button(label).on_hover_text(entry.path.to_string_lossy());record_widget!(w,entry.key.clone(),rect(r.rect));if r.clicked(){self.recent_action(&entry.key);ui.close();}}
  if self.recents.warning.is_some(){ui.label(tr(self.chinese,"最近记录未能读写；文档操作不受影响","Recent history could not be read or saved; documents are unaffected"));}
  ui.separator();if button(ui,w,"recent_clear",tr(self.chinese,"清空最近记录","Clear recent documents"),true){self.recents.clear();ui.close();}
 }
}
