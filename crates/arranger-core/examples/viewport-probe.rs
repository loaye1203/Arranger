//! Test support: independently render a captured viewport in a fresh process.
//! This is not shipped as part of the desktop/headless product executables.
use arranger_core::{Document,render::Renderer};
use serde::Deserialize;
use std::{env,fs,path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Projection {document:Document,page_id:String,pixels_per_mm:f64,region_mm:[f64;4]}

fn run()->Result<(),Box<dyn std::error::Error>> {
    let args:Vec<_>=env::args_os().skip(1).collect();
    if args.len()!=2{return Err("Usage: viewport-probe INPUT.json OUTPUT.png".into());}
    let input:Projection=serde_json::from_slice(&fs::read(&args[0])?)?;input.document.validate()?;
    let rendered=Renderer::new().render_viewport(&input.document,&input.page_id,input.pixels_per_mm,input.region_mm)?;
    rendered.pixels.save_png(Path::new(&args[1]))?;
    println!("{}",serde_json::json!({"width":rendered.pixels.width(),"height":rendered.pixels.height(),"document_hash":input.document.hash()}));
    Ok(())
}
fn main(){if let Err(error)=run(){eprintln!("Viewport probe: {error}");std::process::exit(1);}}
