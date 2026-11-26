mod server;
#[cfg(feature = "desktop")] mod desktop;
use arranger_core::{Command, Service};
use serde_json::json;
use std::{fs, io::{self, BufRead, Read, Write}, path::{Path, PathBuf}, sync::Arc};

fn absolute(path: &str) -> io::Result<PathBuf> { Ok(std::path::absolute(path)?) }
fn help() {
    println!("Arranger 0.1.0 — minimum editing loop\n\narranger describe\narranger run [REQUESTS.jsonl|-] [--keep-going]\narranger serve --endpoint FILE [--open FILE]\narranger gui [--open FILE] [--endpoint FILE] [--data-dir DIR]\narranger send ENDPOINT REQUEST.json\narranger stop ENDPOINT [--discard]\n\nAll mutations use protocol version 1. See docs/API.md.");
}
fn option(args: &[String], key: &str) -> Option<String> { args.windows(2).find(|v| v[0] == key).map(|v| v[1].clone()) }
fn initial(service: &Service, open: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(path) = open {
        let path=absolute(&path)?;
        let cdr=path.extension().and_then(|s|s.to_str()).is_some_and(|s|s.eq_ignore_ascii_case("cdr"));
        let backup=path.file_name().is_some_and(|n|n.to_string_lossy().to_ascii_lowercase().ends_with(".arranger.bak"));
        let path=path.to_string_lossy().into_owned();
        service.command(if backup{Command::DocumentRecoverBackup{document_id:"main".into(),path}}else if cdr {Command::CdrImport{document_id:"main".into(),path}}else{Command::DocumentOpen{document_id:"main".into(),path}})?;
    } else { service.command(Command::DocumentNew { document_id: "main".into(), width_mm: 210., height_mm: 297., unit: "mm".into() })?; }
    Ok(())
}
fn run() -> Result<i32, Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let Some(action) = args.first().map(String::as_str) else { help(); return Ok(0); };
    match action {
        "--help" | "help" | "-h" => { help(); Ok(0) }
        "--version" => { println!("Arranger {}", env!("CARGO_PKG_VERSION")); Ok(0) }
        "describe" => { println!("{}", Service::new().command(Command::Describe)?); Ok(0) }
        "run" => {
            let input = args.get(1).filter(|s| !s.starts_with("--")).map(String::as_str).unwrap_or("-");
            let source: Box<dyn BufRead> = if input == "-" { Box::new(io::BufReader::new(io::stdin())) } else { Box::new(io::BufReader::new(fs::File::open(input)?)) };
            let service = Service::new(); let mut failed = false;
            for line in source.lines() {
                let line = line?; if line.trim().is_empty() { continue; }
                let response = service.execute_json(&line); failed |= !response.ok;
                println!("{}", serde_json::to_string(&response)?); io::stdout().flush()?;
                if !response.ok && !args.iter().any(|v| v == "--keep-going") { break; }
            }
            Ok(if failed {1} else {0})
        }
        "send" => {
            if args.len() != 3 { return Err("Usage: arranger send ENDPOINT REQUEST.json".into()); }
            let input = if args[2] == "-" { let mut text=String::new(); io::stdin().read_to_string(&mut text)?; text } else { fs::read_to_string(&args[2])? };
            let answer = server::send(Path::new(&args[1]), &input)?;
            println!("{answer}"); Ok(if answer["ok"] == true {0} else {1})
        }
        "stop" => {
            let endpoint = args.get(1).ok_or("Usage: arranger stop ENDPOINT [--discard]")?;
            let answer = server::send(Path::new(endpoint), &json!({"control":"stop","discard_changes":args.iter().any(|s|s=="--discard")}).to_string())?;
            println!("{answer}"); Ok(if answer["ok"] == true {0} else {1})
        }
        "serve" | "gui" => {
            let service = Arc::new(Service::new()); initial(&service, option(&args, "--open"))?;
            let endpoint = if let Some(path) = option(&args, "--endpoint") { absolute(&path)? } else {
                let dir = absolute("artifacts/runtime")?; fs::create_dir_all(&dir)?; dir.join(format!("arranger-{}.json", std::process::id()))
            };
            let test_output = option(&args, "--test-output").map(|s| absolute(&s)).transpose()?;
            if test_output.is_some() && !cfg!(feature="test-hooks") { return Err("--test-output requires a test-hooks build".into()); }
            if let Some(out) = &test_output { fs::create_dir_all(out)?; }
            let mut server = server::Server::start(service.clone(), &endpoint, test_output.is_some())?;
            println!("{}", json!({"event":"ready","endpoint":endpoint,"document_id":"main","pid":std::process::id()})); io::stdout().flush()?;
            if action == "gui" {
                #[cfg(feature="desktop")]
                desktop::run(service, &server, test_output)?;
                #[cfg(not(feature="desktop"))]
                return Err("This binary was built without desktop support".into());
            } else { server.wait(); }
            Ok(0)
        }
        _ => Err(format!("Unknown command {action}; use arranger --help").into()),
    }
}
fn main() {
    match run() { Ok(code) => std::process::exit(code), Err(error) => { eprintln!("Arranger: {error}"); std::process::exit(2); } }
}
