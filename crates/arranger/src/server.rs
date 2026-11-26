use arranger_core::{Error, Response, Service};
use serde_json::{Value, json};
use std::{fs::{self, OpenOptions}, io::{self, BufRead, BufReader, Read, Write}, net::{TcpListener, TcpStream}, path::{Path, PathBuf}, sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}}, thread, time::Duration};

/// Publish complete bytes without ever replacing another server's endpoint.
/// The temporary file is on the same filesystem; a hard link makes the final
/// name visible in one step only after the writer has closed it.
fn publish_endpoint(endpoint: &Path, bytes: &[u8]) -> io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Pending(PathBuf);
    impl Drop for Pending { fn drop(&mut self) { let _ = fs::remove_file(&self.0); } }
    let parent=endpoint.parent().filter(|p|!p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    for _ in 0..1024 {
        let path=parent.join(format!(".arranger-endpoint-{}-{}.pending",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));
        let mut file=match OpenOptions::new().create_new(true).write(true).open(&path) {
            Ok(file)=>file,Err(e) if e.kind()==io::ErrorKind::AlreadyExists=>continue,Err(e)=>return Err(e),
        };
        let pending=Pending(path);
        let written=file.write_all(bytes).and_then(|_|file.flush());drop(file);written?;
        // Unlike rename, hard_link refuses an existing destination on all
        // supported platforms. Never fall back to a truncate/replace write.
        fs::hard_link(&pending.0,endpoint)?;
        return Ok(());
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists,"Could not reserve an endpoint publication file"))
}

// Test input and screenshots are compiled only into explicitly requested test builds.
#[derive(Default)]
#[cfg_attr(not(feature="test-hooks"), allow(dead_code))]
pub struct TestBridge { pub input: Vec<Value>, pub capture: Vec<String>, pub status: Value,
    #[cfg(feature="test-hooks")] pub input_serial:u64,
}

/// The desktop registers a thread-safe repaint callback. Headless sessions
/// keep this empty; command workers never depend on a GUI runtime.
#[derive(Default)]
pub struct Wakeup(Mutex<Option<Arc<dyn Fn() + Send + Sync>>>);
impl Wakeup {
    #[cfg_attr(not(feature="desktop"), allow(dead_code))]
    pub fn set(&self, callback: impl Fn() + Send + Sync + 'static) {
        *self.0.lock().expect("GUI wakeup") = Some(Arc::new(callback));
    }
    pub fn clear(&self) { *self.0.lock().expect("GUI wakeup") = None; }
    fn request(&self) {
        // Invoke outside the lock: integrations can synchronously re-enter.
        let callback = self.0.lock().expect("GUI wakeup").clone();
        if let Some(callback) = callback { callback(); }
    }
}

pub struct Server {
    pub stop: Arc<AtomicBool>,
    pub wakeup: Arc<Wakeup>,
    #[cfg_attr(not(feature="desktop"), allow(dead_code))]
    pub bridge: Arc<Mutex<TestBridge>>,
    pub endpoint: PathBuf,
    worker: Option<thread::JoinHandle<()>>,
}
impl Server {
    pub fn start(service: Arc<Service>, endpoint: &Path, test_ui: bool) -> Result<Self, Box<dyn std::error::Error>> {
        let listener = TcpListener::bind("127.0.0.1:0")?; listener.set_nonblocking(true)?;
        let address = listener.local_addr()?.to_string();
        let mut bytes=serde_json::to_vec(&json!({"protocol_version":1,"address":address,"pid":std::process::id()}))?;bytes.push(b'\n');
        publish_endpoint(endpoint,&bytes)?;
        let stop = Arc::new(AtomicBool::new(false)); let bridge = Arc::new(Mutex::new(TestBridge::default()));
        let wakeup = Arc::new(Wakeup::default());
        let (end, ui, wake) = (stop.clone(), bridge.clone(), wakeup.clone());
        let worker = thread::spawn(move || {
            while !end.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let (service, end, ui, wake) = (service.clone(), end.clone(), ui.clone(), wake.clone());
                        thread::spawn(move || handle(stream, service, end, ui, wake, test_ui));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(5)),
                    Err(e) => { eprintln!("Local command listener: {e}"); break; }
                }
            }
        });
        Ok(Self { stop, wakeup, bridge, endpoint: endpoint.into(), worker: Some(worker) })
    }
    pub fn wait(&mut self) { if let Some(worker) = self.worker.take() { let _ = worker.join(); } }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.wakeup.clear(); self.stop.store(true, Ordering::SeqCst); self.wait();
        let _ = fs::remove_file(&self.endpoint);
    }
}
fn handle(mut stream: TcpStream, service: Arc<Service>, stop: Arc<AtomicBool>, bridge: Arc<Mutex<TestBridge>>, wakeup: Arc<Wakeup>, test_ui: bool) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let mut line = String::new();
    let read = BufReader::new(&stream).take(16 * 1024 * 1024 + 1).read_line(&mut line);
    let mut wakes_ui=true;
    let response = if read.is_err() || line.len() > 16 * 1024 * 1024 {
        json!(Response::from_result(Value::Null, Err(Error::new("INVALID_REQUEST", "Request is unreadable or exceeds 16 MiB"))))
    } else if let Ok(value) = serde_json::from_str::<Value>(&line) {
        wakes_ui=value["control"]!="ui_status";
        if let Some(control) = value["control"].as_str() {
            control_request(control, &value, &service, &stop, &bridge, test_ui)
        } else { json!(service.execute_json(&line)) }
    } else { json!(service.execute_json(&line)) };
    if let Ok(mut bytes) = serde_json::to_vec(&response) {
        bytes.push(b'\n'); let _ = stream.write_all(&bytes); let _ = stream.shutdown(std::net::Shutdown::Write);
    }
    // Publish the reply before waking a stop: a fast minimized-window exit
    // must not race the worker's response write. Status reads remain passive.
    if wakes_ui { wakeup.request(); }
}
fn control_request(control: &str, value: &Value, service: &Service, stop: &AtomicBool, bridge: &Mutex<TestBridge>, test_ui: bool) -> Value {
    match control {
        "stop" => {
            let documents = service.command(arranger_core::Command::Documents).unwrap_or_default();
            let unsafe_to_close = documents["documents"].as_array().is_some_and(|docs| docs.iter().any(|d| d["dirty"] == true || d["busy"] == true));
            if unsafe_to_close && value["discard_changes"] != true {
                json!({"ok":false,"error":{"code":"UNSAVED_CHANGES","message":"Save/finish work or explicitly discard changes before stopping"}})
            } else { stop.store(true, Ordering::SeqCst); json!({"ok":true}) }
        }
        #[cfg(feature = "test-hooks")]
        "ui_status" if test_ui => json!({"ok":true,"result":bridge.lock().expect("test bridge").status}),
        #[cfg(feature = "test-hooks")]
        "ui_input" if test_ui => {
            let mut event=value["event"].clone();let Some(event)=event.as_object_mut()else{return json!({"ok":false});};
            let mut bridge=bridge.lock().expect("test bridge");bridge.input_serial+=1;let serial=bridge.input_serial;
            event.insert("__input_serial".into(),json!(serial));bridge.input.push(Value::Object(event.clone()));
            json!({"ok":true,"result":{"input_serial":serial}})
        }
        #[cfg(feature = "test-hooks")]
        "ui_capture" if test_ui => {
            let label = value["label"].as_str().unwrap_or("");
            if label.is_empty() || !label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') { return json!({"ok":false}); }
            bridge.lock().expect("test bridge").capture.push(label.into()); json!({"ok":true})
        }
        _ => { let _ = (bridge, test_ui); json!({"ok":false,"error":{"code":"UNKNOWN_CONTROL","message":"Unknown or disabled application control"}}) }
    }
}
pub fn send(endpoint: &Path, input: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let info: Value = serde_json::from_slice(&fs::read(endpoint)?)?;
    let address = info["address"].as_str().ok_or("Invalid endpoint address")?;
    // Only an explicit loopback endpoint is valid for this local adapter.
    let address: std::net::SocketAddr = address.parse()?;
    if !address.ip().is_loopback() { return Err("Endpoint must be on the local machine".into()); }
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(5))?;
    stream.set_read_timeout(Some(Duration::from_secs(120)))?;
    let parsed: Value = serde_json::from_str(input)?;
    let mut bytes = serde_json::to_vec(&parsed)?; bytes.push(b'\n'); stream.write_all(&bytes)?;
    let mut response = String::new(); BufReader::new(stream).read_line(&mut response)?;
    Ok(serde_json::from_str(&response)?)
}

#[cfg(test)]
mod endpoint_tests {
    use super::*;
    fn directory()->PathBuf {
        static NEXT:AtomicU64=AtomicU64::new(0);
        let dir=std::env::temp_dir().join(format!("arranger-endpoint-test-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));
        fs::create_dir(&dir).unwrap();dir
    }
    #[test]
    fn visible_endpoint_is_always_a_complete_record_under_concurrent_readers() {
        let dir=directory();let endpoint=dir.join("连接信息.json");
        let record=json!({"protocol_version":1,"address":"127.0.0.1:12345","pid":42,"padding":"x".repeat(1024*1024)});let bytes=serde_json::to_vec(&record).unwrap();
        thread::scope(|scope|{
            for _ in 0..8 {let endpoint=&endpoint;let record=&record;scope.spawn(move||{
                let deadline=std::time::Instant::now()+Duration::from_secs(5);
                loop {match fs::read(endpoint) {Ok(raw)=>{assert_eq!(serde_json::from_slice::<Value>(&raw).unwrap(),*record);break;},Err(e)if e.kind()==io::ErrorKind::NotFound=>{assert!(std::time::Instant::now()<deadline,"Endpoint was never published");thread::yield_now();},Err(e)=>panic!("{e}")}}
            });}
            publish_endpoint(&endpoint,&bytes).unwrap();
        });
        assert_eq!(fs::read_dir(&dir).unwrap().count(),1);fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn concurrent_starters_never_replace_the_winner_or_leave_publication_files() {
        let dir=directory();let endpoint=dir.join("endpoint.json");
        let wins=thread::scope(|scope|{
            let jobs=(0..16).map(|i|{let endpoint=&endpoint;scope.spawn(move||{
                let bytes=serde_json::to_vec(&json!({"winner":i})).unwrap();match publish_endpoint(endpoint,&bytes){Ok(())=>Some(bytes),Err(e)=>{assert_eq!(e.kind(),io::ErrorKind::AlreadyExists);None}}
            })}).collect::<Vec<_>>();jobs.into_iter().filter_map(|job|job.join().unwrap()).collect::<Vec<_>>()
        });
        assert_eq!(wins.len(),1);assert_eq!(fs::read(&endpoint).unwrap(),wins[0]);assert!(publish_endpoint(&endpoint,b"replacement").is_err());assert_eq!(fs::read(&endpoint).unwrap(),wins[0]);assert_eq!(fs::read_dir(&dir).unwrap().count(),1);fs::remove_dir_all(dir).unwrap();
    }
}
