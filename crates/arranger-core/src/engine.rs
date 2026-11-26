use crate::{Command, Document, Error, Operation, Request, Response, Result, command::Notice, native::{self, FileBinding}, render::Renderer};
use serde::Serialize;
use serde_json::{Value, json};
use std::{collections::{BTreeMap, BTreeSet}, path::Path, sync::{Arc, Mutex, RwLock, atomic::{AtomicU64, Ordering}}, time::{Duration, Instant}};

#[derive(Clone)]
struct State { document: Arc<Document>, hash: String }
impl State { fn new(document: Document) -> Self { Self { hash: document.hash(), document: Arc::new(document) } } }
struct Transaction { id: String, staged: Document, deadline: Instant, count: usize, changed: BTreeSet<String>, warnings: Vec<Notice> }
struct Session {
    current: State,
    revision: u64,
    saved_hash: Option<String>,
    binding: Option<FileBinding>,
    undo: Vec<State>,
    redo: Vec<State>,
    transaction: Option<Transaction>,
    warnings: Vec<Notice>,
    saving: bool,
    closed: bool,
}
impl Session {
    fn new(document: Document, revision: u64) -> Self {
        Self { current: State::new(document), revision, saved_hash: None, binding: None, undo: vec![], redo: vec![], transaction: None, warnings: vec![], saving: false, closed: false }
    }
    fn expire(&mut self) {
        if self.transaction.as_ref().is_some_and(|t| Instant::now() >= t.deadline) {
            self.transaction = None;
            self.notice(Notice::new("TRANSACTION_EXPIRED", "The write lease expired; all staged operations were rolled back"));
        }
    }
    fn notice(&mut self, notice: Notice) {
        if self.warnings.len() == 32 { self.warnings.remove(0); }
        self.warnings.push(notice);
    }
    fn readable(&self) -> Result<()> {
        if self.closed { Err(Error::new("DOCUMENT_NOT_FOUND", "This document session was closed")) } else { Ok(()) }
    }
    fn writable(&self, expected_revision: u64) -> Result<()> {
        self.readable()?;
        if self.transaction.is_some() || self.saving { return Err(Error::new("DOCUMENT_BUSY", "The document has an active transaction or save")); }
        if expected_revision != self.revision { return Err(Error::new("REVISION_CONFLICT", format!("Expected {expected_revision}, current revision is {}", self.revision))); }
        Ok(())
    }
    fn owned(&self, token: &str) -> Result<()> {
        self.readable()?;
        if self.transaction.as_ref().is_some_and(|t| t.id == token) { Ok(()) }
        else { Err(Error::new("TRANSACTION_NOT_FOUND", "Transaction is absent, expired, or not owned by this token")) }
    }
    fn summary(&self, changed: Vec<String>) -> Value {
        json!({"revision":self.revision,"document_hash":self.current.hash,"changed_ids":changed,"undo_depth":self.undo.len(),"redo_depth":self.redo.len(),"busy":self.transaction.is_some() || self.saving})
    }
    fn publish(&mut self, document: Document) {
        let state = State::new(document);
        if state.hash != self.current.hash {
            self.undo.push(std::mem::replace(&mut self.current, state));
            self.redo.clear(); self.revision += 1;
        }
    }
}

#[derive(Clone, Serialize)]
pub struct Snapshot {
    pub document_id: String,
    pub revision: u64,
    pub document_hash: String,
    pub document: Arc<Document>,
    pub dirty: bool,
    pub busy: bool,
    pub undo_depth: usize,
    pub redo_depth: usize,
    pub saved_path: Option<String>,
    pub warnings: Vec<Notice>,
}

#[derive(Serialize)]
pub struct DocumentSummary {
    pub document_id:String,
    pub revision:u64,
    pub dirty:bool,
    pub busy:bool,
    pub saved_path:Option<String>,
    pub object_count:usize,
}

type SharedSession = Arc<Mutex<Session>>;
pub struct Service {
    sessions: RwLock<BTreeMap<String, SharedSession>>,
    next_transaction: AtomicU64,
    renderer: Mutex<Option<Renderer>>,
}
impl Default for Service { fn default() -> Self { Self::new() } }
impl Service {
    pub fn new() -> Self { Self { sessions: RwLock::new(BTreeMap::new()), next_transaction: AtomicU64::new(1), renderer: Mutex::new(None) } }
    fn session(&self, id: &str) -> Result<SharedSession> {
        self.sessions.read().expect("session map lock").get(id).cloned().ok_or_else(|| Error::new("DOCUMENT_NOT_FOUND", format!("Unknown document session {id}")))
    }
    fn insert(&self, id: &str, session: Session) -> Result<Value> {
        if !crate::model::valid_id(id) { return Err(Error::new("INVALID_DOCUMENT", "Invalid document session ID")); }
        let summary = session.summary(vec![]);
        let mut map = self.sessions.write().expect("session map lock");
        if map.contains_key(id) { return Err(Error::new("DOCUMENT_EXISTS", format!("Session {id} is already open"))); }
        map.insert(id.into(), Arc::new(Mutex::new(session)));
        Ok(summary)
    }
    pub fn document_summaries(&self)->Vec<DocumentSummary>{
        let sessions:Vec<_>=self.sessions.read().expect("session map lock").iter().map(|(id,session)|(id.clone(),session.clone())).collect();
        sessions.into_iter().filter_map(|(document_id,session)|{
            let mut s=session.lock().expect("document lock");s.expire();s.readable().ok()?;
            Some(DocumentSummary{document_id,revision:s.revision,dirty:s.saved_hash.as_ref()!=Some(&s.current.hash),
                busy:s.transaction.is_some()||s.saving,saved_path:s.binding.as_ref().map(|b|b.path.to_string_lossy().into_owned()),object_count:s.current.document.objects.len()})
        }).collect()
    }
    pub fn snapshot(&self, id: &str) -> Result<Snapshot> {
        let session = self.session(id)?;
        let mut s = session.lock().expect("document lock"); s.expire(); s.readable()?;
        Ok(Snapshot { document_id: id.into(), revision: s.revision, document_hash: s.current.hash.clone(), document: s.current.document.clone(),
            dirty: s.saved_hash.as_ref() != Some(&s.current.hash), busy: s.transaction.is_some() || s.saving,
            undo_depth: s.undo.len(), redo_depth: s.redo.len(), saved_path: s.binding.as_ref().map(|b| b.path.to_string_lossy().into_owned()), warnings: s.warnings.clone() })
    }
    pub fn execute_json(&self, input: &str) -> Response {
        let value = match serde_json::from_str::<Value>(input) {
            Ok(v) => v,
            Err(e) => return Response::from_result(Value::Null, Err(Error::new("INVALID_JSON", e.to_string()))),
        };
        let request_id = value.get("request_id").cloned().unwrap_or(Value::Null);
        let invalid_execute = if value["protocol_version"] == 1 && value["command"]["type"] == "execute" {
            value["command"]["document_id"].as_str().zip(value["command"]["transaction_id"].as_str()).map(|(d,t)|(d.to_owned(),t.to_owned()))
        } else { None };
        match serde_json::from_value::<Request>(value) {
            Ok(request) => self.execute(request),
            Err(e) => {
                let mut error = Error::new("INVALID_REQUEST", e.to_string());
                // A well-formed execute envelope with an invalid operation still
                // fails its owned transaction, just like a geometry validation error.
                if let Some((id, token)) = invalid_execute { if let Ok(shared)=self.session(&id) {
                    let mut s=shared.lock().expect("document lock");s.expire();
                    if s.owned(&token).is_ok() {let count=s.transaction.as_ref().expect("owned transaction").count+1;s.transaction=None;error=error.rollback(count);}
                }}
                Response::from_result(request_id, Err(error))
            }
        }
    }
    pub fn execute(&self, request: Request) -> Response {
        let result = if request.protocol_version != 1 { Err(Error::new("PROTOCOL_VERSION", "Supported protocol_version is 1")) }
            else { self.command(request.command) };
        Response::from_result(request.request_id, result)
    }
    pub fn command(&self, command: Command) -> Result<Value> {
        match command {
            Command::Describe => Ok(crate::schema::describe()),
            Command::Documents => Ok(json!({"documents":self.document_summaries()})),
            Command::DocumentNew { document_id, width_mm, height_mm, unit } => {
                require_mm(&unit)?;
                self.insert(&document_id, Session::new(Document::new(document_id.clone(), width_mm, height_mm)?, 0))
            }
            Command::DocumentOpen { document_id, path } => {
                let loaded = native::open(Path::new(&path))?;
                let mut session = Session::new(loaded.document, loaded.revision);
                session.saved_hash = Some(session.current.hash.clone()); session.binding = Some(loaded.binding); session.warnings = loaded.warnings;
                let warnings = session.warnings.clone();
                let mut value = self.insert(&document_id, session)?; value["warnings"] = json!(warnings); Ok(value)
            }
            Command::DocumentRecoverBackup {document_id,path} => {
                let (document,revision,warnings)=native::recover_source_backup(Path::new(&path))?;
                let mut session=Session::new(document,revision);session.warnings=warnings.clone();
                let mut value=self.insert(&document_id,session)?;value["warnings"]=json!(warnings);Ok(value)
            }
            Command::CdrImport { document_id, path } => {
                let (document, warnings) = crate::cdr::import(Path::new(&path), &document_id)?;
                let mut session = Session::new(document, 0);
                // Interchange creates an unsaved document. The source CDR is
                // never a native save binding and cannot be overwritten by Save.
                session.warnings = warnings.clone();
                let mut value = self.insert(&document_id, session)?;
                value["warnings"] = json!(warnings); Ok(value)
            }
            Command::CdrRecoverBackup { document_id, path } => {
                let (document, warnings) = native::recover_cdr_backup(Path::new(&path), &document_id)?;
                let mut session = Session::new(document, 0);
                session.warnings = warnings.clone();
                let mut value = self.insert(&document_id, session)?;
                value["warnings"] = json!(warnings); Ok(value)
            }
            Command::CdrPreflight { document_id,format,mode,page_id } => {
                let s=self.snapshot(&document_id)?;let mut value=crate::cdr::check_delivery(&s.document,&format,mode,page_id.as_deref())?;value["revision"]=json!(s.revision);value["busy"]=json!(s.busy);Ok(value)
            }
            Command::CdrExport { document_id, path, format, overwrite,mode,page_id } => {
                self.cdr_exchange(&document_id, |document| {
                    if let Some(mode)=mode{return crate::cdr::export_delivery(document,&format,mode,page_id.as_deref(),Path::new(&path),overwrite);}
                    if page_id.is_some(){return Err(Error::new("INVALID_PARAMETER","Selected-page cdr_export requires an explicit delivery mode; legacy selected-page exchange uses export"));}
                    let (path, warnings) = match format.as_str() {"cdr14"=>crate::cdr::export_document(document,Path::new(&path),overwrite),"cdr21"=>crate::cdr::export_document21(document,Path::new(&path),overwrite),_=>Err(Error::new("INVALID_PARAMETER","cdr_export format must be cdr14 or cdr21"))}?;
                    Ok(json!({"path":path,"page_count":document.pages.len(),"format":format,"warnings":warnings}))
                })
            }
            Command::DocumentClose { document_id, expected_revision, discard_changes } => {
                let mut map = self.sessions.write().expect("session map lock");
                let shared = map.get(&document_id).cloned().ok_or_else(|| Error::new("DOCUMENT_NOT_FOUND", &document_id))?;
                let mut s = shared.lock().expect("document lock"); s.expire(); s.writable(expected_revision)?;
                if !discard_changes && s.saved_hash.as_ref() != Some(&s.current.hash) { return Err(Error::new("UNSAVED_CHANGES", "Save the document or explicitly discard changes")); }
                s.closed = true; map.remove(&document_id); Ok(json!({"closed":document_id}))
            }
            Command::Snapshot { document_id } => Ok(serde_json::to_value(self.snapshot(&document_id)?).expect("snapshot serializes")),
            Command::ObjectGet { document_id, object_id } => {
                let s = self.snapshot(&document_id)?; let o = s.document.object(&object_id)?;
                Ok(json!({"revision":s.revision,"object":o,"bounds_mm":s.document.bounds(o)?,"active_clips":s.document.active_clips(o)?}))
            }
            Command::ResourceGet {document_id,resource_id} => {
                let s=self.snapshot(&document_id)?;
                let mut value=crate::resource::query(&s.document,&resource_id)?;
                value["revision"]=json!(s.revision);value["document_hash"]=json!(s.document_hash);Ok(value)
            }
            Command::PathGet { document_id, object_id } => {
                let s = self.snapshot(&document_id)?;
                let mut value = crate::path::query(&s.document, &object_id)?;
                value["revision"] = json!(s.revision);
                value["document_hash"] = json!(s.document_hash);
                Ok(value)
            }
            Command::TextMeasure { document_id, object_id } => {
                let s = self.snapshot(&document_id)?;
                let mut value = crate::text::measure(&s.document, &object_id)?;
                value["revision"] = json!(s.revision);
                value["document_hash"] = json!(s.document_hash);
                Ok(value)
            }
            Command::HitTest { document_id, page_id, point_mm } => {
                if !point_mm.iter().all(|x| x.is_finite()) { return Err(Error::new("INVALID_POINT", "Hit-test coordinates must be finite")); }
                let s = self.snapshot(&document_id)?; s.document.page(&page_id)?;
                Ok(json!({"revision":s.revision,"object_id":s.document.hit_test(&page_id,point_mm,0.5)}))
            }
            Command::Begin { document_id, expected_revision, unit, lease_ms } => self.begin(&document_id, expected_revision, &unit, lease_ms),
            Command::Execute { document_id, transaction_id, operation } => self.step(&document_id, &transaction_id, &operation),
            Command::Commit { document_id, transaction_id } => self.commit(&document_id, &transaction_id),
            Command::Rollback { document_id, transaction_id } => {
                let shared = self.session(&document_id)?; let mut s = shared.lock().expect("document lock"); s.expire(); s.owned(&transaction_id)?;
                s.transaction = None; Ok(s.summary(vec![]))
            }
            Command::Batch { document_id, expected_revision, unit, operations } => {
                if operations.is_empty() { return Err(Error::new("EMPTY_BATCH", "Batch needs at least one operation")); }
                let token = self.begin(&document_id, expected_revision, &unit, 60_000)?["transaction_id"].as_str().expect("token").to_owned();
                for operation in &operations {
                    self.step(&document_id, &token, operation)?;
                    // Yield between operations; readers keep seeing the committed state.
                    std::thread::yield_now();
                }
                self.commit(&document_id, &token)
            }
            Command::Undo { document_id, expected_revision } => self.history(&document_id, expected_revision, false),
            Command::Redo { document_id, expected_revision } => self.history(&document_id, expected_revision, true),
            Command::SaveCheck { document_id, format } => {
                let s = self.snapshot(&document_id)?;
                let mut result = crate::save_check::analyze(&s.document, format)?;
                result["revision"] = json!(s.revision);
                result["busy"] = json!(s.busy);
                Ok(result)
            }
            Command::Save { document_id, expected_revision, path, overwrite } => {
                let shared = self.session(&document_id)?;
                let (state, revision, binding) = {
                    let mut s = shared.lock().expect("document lock"); s.expire(); s.writable(expected_revision)?; s.saving = true;
                    (s.current.clone(), s.revision, s.binding.clone())
                };
                let saved = native::save(&state.document, revision, Path::new(&path), binding.as_ref(), overwrite);
                let mut s = shared.lock().expect("document lock"); s.saving = false;
                let binding = saved?; let path = binding.path.to_string_lossy().into_owned();
                s.binding = Some(binding); s.saved_hash = Some(state.hash.clone());
                Ok(json!({"path":path,"revision":revision,"document_hash":state.hash,"dirty":false}))
            }
            Command::PdfExport { document_id,path,page_ids,expected_revision,overwrite,dpi } => {
                let s=self.snapshot(&document_id)?;
                if s.busy{return Err(Error::new("DOCUMENT_BUSY","Finish the current edit before exporting PDF"));}
                if expected_revision.is_some_and(|r|r!=s.revision){return Err(Error::new("REVISION_CONFLICT","The document changed; review the PDF page selection again"));}
                let pages=page_ids.unwrap_or_else(||s.document.pages.iter().map(|p|p.id.clone()).collect());
                if Path::new(&path).extension().and_then(|s|s.to_str()).is_none_or(|s|!s.eq_ignore_ascii_case("pdf")){return Err(Error::new("INVALID_PATH","PDF export requires a .pdf path"));}
                let mut renderer=self.renderer.lock().expect("renderer lock");let renderer=renderer.get_or_insert_with(Renderer::new);
                let (data,warnings)=crate::pdf::export_pages(renderer,&s.document,&pages,dpi)?;
                let path=native::write_export(Path::new(&path),&data,overwrite)?;
                Ok(json!({"path":path,"revision":s.revision,"document_hash":s.document_hash,"page_ids":pages,"page_count":pages.len(),"warnings":warnings}))
            }
            Command::Export { document_id, page_id, format, path, overwrite, dpi } => {
                if matches!(format, crate::command::ExportFormat::Cdr14 | crate::command::ExportFormat::Cdr21) {
                    return self.cdr_exchange(&document_id, |document| {
                        let mut renderer = self.renderer.lock().expect("renderer lock");
                        let renderer = renderer.get_or_insert_with(Renderer::new);
                        let (path, warnings) = renderer.export(document, &page_id, format, dpi, Path::new(&path), overwrite)?;
                        Ok(json!({"path":path,"warnings":warnings}))
                    });
                }
                let s = self.snapshot(&document_id)?;
                let mut renderer = self.renderer.lock().expect("renderer lock");
                let renderer = renderer.get_or_insert_with(Renderer::new);
                let (path, warnings) = renderer.export(&s.document, &page_id, format, dpi, Path::new(&path), overwrite)?;
                Ok(json!({"path":path,"revision":s.revision,"document_hash":s.document_hash,"warnings":warnings}))
            }
        }
    }
    fn cdr_exchange(&self, id: &str, write: impl FnOnce(&Document) -> Result<Value>) -> Result<Value> {
        let shared = self.session(id)?;
        let (state, revision) = {
            let mut s = shared.lock().expect("document lock"); s.expire();
            let revision = s.revision; s.writable(revision)?; s.saving = true;
            (s.current.clone(), revision)
        };
        let result = write(&state.document);
        shared.lock().expect("document lock").saving = false;
        let mut value = result?;
        value["revision"] = json!(revision); value["document_hash"] = json!(state.hash);
        value["atomic_exchange"] = json!(true); value["complete_save"] = json!(false);
        Ok(value)
    }
    fn begin(&self, id: &str, revision: u64, unit: &str, lease_ms: u64) -> Result<Value> {
        require_mm(unit)?;
        if !(10..=60_000).contains(&lease_ms) { return Err(Error::new("INVALID_LEASE", "lease_ms must be between 10 and 60000")); }
        let shared = self.session(id)?; let mut s = shared.lock().expect("document lock"); s.expire(); s.writable(revision)?;
        let token = format!("tx-{}-{}",std::process::id(),self.next_transaction.fetch_add(1,Ordering::Relaxed));
        s.transaction = Some(Transaction { id: token.clone(), staged: (*s.current.document).clone(), deadline: Instant::now() + Duration::from_millis(lease_ms), count: 0, changed: BTreeSet::new(), warnings: vec![] });
        Ok(json!({"transaction_id":token,"revision":s.revision,"lease_ms":lease_ms}))
    }
    fn step(&self, id: &str, token: &str, operation: &Operation) -> Result<Value> {
        let shared = self.session(id)?; let mut s = shared.lock().expect("document lock"); s.expire(); s.owned(token)?;
        let t = s.transaction.as_mut().expect("owned transaction"); t.count += 1;
        match operation.apply(&mut t.staged) {
            Ok(applied) => {
                t.changed.extend(applied.changed.clone());
                for notice in &applied.warnings {
                    if !t.warnings.iter().any(|w|w.code==notice.code && w.message==notice.message) {t.warnings.push(notice.clone());}
                }
                Ok(json!({"staged":true,"operation_count":t.count,"changed_ids":applied.changed,"warnings":applied.warnings,"revision":s.revision}))
            }
            Err(error) => {
                let index = t.count; s.transaction = None;
                s.notice(Notice::new("TRANSACTION_ROLLED_BACK", "A failed operation rolled back the entire transaction"));
                Err(error.rollback(index))
            }
        }
    }
    fn commit(&self, id: &str, token: &str) -> Result<Value> {
        let shared = self.session(id)?; let mut s = shared.lock().expect("document lock"); s.expire(); s.owned(token)?;
        let t = s.transaction.take().expect("owned transaction"); t.staged.validate()?;
        let changed = t.changed.into_iter().collect(); s.publish(t.staged);
        for notice in &t.warnings { s.notice(notice.clone()); }
        let mut summary = s.summary(changed); summary["warnings"] = json!(t.warnings); Ok(summary)
    }
    fn history(&self, id: &str, revision: u64, redo: bool) -> Result<Value> {
        let shared = self.session(id)?; let mut s = shared.lock().expect("document lock"); s.expire(); s.writable(revision)?;
        let previous = if redo { s.redo.pop() } else { s.undo.pop() }.ok_or_else(|| Error::new(if redo {"NOTHING_TO_REDO"} else {"NOTHING_TO_UNDO"}, "No history entry available"))?;
        let current = std::mem::replace(&mut s.current, previous);
        if redo { s.undo.push(current); } else { s.redo.push(current); }
        s.revision += 1; Ok(s.summary(vec![]))
    }
}

fn require_mm(unit: &str) -> Result<()> {
    if unit == "mm" { Ok(()) } else { Err(Error::new("UNSUPPORTED_UNIT", "Use explicit mm units; other input units are not currently supported")) }
}
