use arranger_core::{cdr,native,paint,Document,Geometry,Object,Style,render::Renderer,command::ExportFormat};
use std::{fs,path::PathBuf,sync::atomic::{AtomicU64,Ordering}};
fn root()->PathBuf{PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")}
fn dir()->PathBuf{static N:AtomicU64=AtomicU64::new(0);let p=root().join(format!("artifacts/color-tests/{}-{}",std::process::id(),N.fetch_add(1,Ordering::Relaxed)));fs::create_dir_all(&p).unwrap();fs::canonicalize(p).unwrap()}
fn scene()->Document {
 let mut d=Document::new("color".into(),210.,297.).unwrap();
 for (i,color) in ["cmyk(12,34,56,78)","cmyk255(1,127,128,255)","#123456"].into_iter().enumerate(){d.objects.push(Object{id:format!("paint-{i}"),page_id:"page-1".into(),parent_id:None,x_mm:20.+i as f64*40.,y_mm:30.,width_mm:25.,height_mm:15.,transform:[1.,0.,0.,1.,0.,0.],style:Style{fill:color.into(),stroke:"cmyk(0,0,0,100)".into(),stroke_width_mm:0.4,..Default::default()},visible:true,locked:false,clip:None,geometry:Geometry::Rect{radius_mm:0.}})}d
}
#[test]
fn native_cdr_paints_preserve_channel_scale_and_all_four_components() {
 let d=scene();let (raw,w)=cdr::encode_document(&d).unwrap();assert!(w.iter().any(|n|n.code=="CDR_CMYK_CHANNELS"));let (out,w)=cdr::decode(&raw,"copy").unwrap();assert!(w.iter().any(|n|n.code=="CMYK_RGB_APPROXIMATION"));
 for (a,b) in d.objects.iter().zip(&out.objects){assert_eq!(a.style,b.style);}
}
#[test]
fn native_v3_saves_exact_channels_and_old_rgb_files_keep_their_versions() {
 let p=dir().join("color.arranger");let mut d=scene();native::save(&d,0,&p,None,false).unwrap();let header:serde_json::Value=serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();assert_eq!(header["format_version"],3);assert_eq!(native::open(&p).unwrap().document,d);
 for o in &mut d.objects{o.style.fill="#123456".into();o.style.stroke="none".into();}native::save(&d,0,&p,None,true).unwrap();let header:serde_json::Value=serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();assert_eq!(header["format_version"],1);
 d.objects[0].page_id=arranger_core::WORKSPACE_PAGE.into();native::save(&d,0,&p,None,true).unwrap();let header:serde_json::Value=serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();assert_eq!(header["format_version"],2);
}
#[test]
fn down_versioned_cmyk_and_unknown_versions_never_load_an_rgb_backup() {
 let p=dir().join("keep.arranger");let d=scene();native::save(&d,0,&p,None,false).unwrap();let raw=fs::read(&p).unwrap();let mut header:serde_json::Value=serde_json::from_slice(&raw).unwrap();let backup=p.with_file_name("keep.arranger.bak");fs::write(&backup,&raw).unwrap();
 for version in [1,2,6] {header["format_version"]=version.into();fs::write(&p,serde_json::to_vec(&header).unwrap()).unwrap();assert_eq!(native::open(&p).err().unwrap().code,"UNSUPPORTED_FILE_VERSION");assert_eq!(fs::read(&backup).unwrap(),raw);}
}
#[test]
fn actual_corel_cmyk_workspace_and_identity_keep_states_and_live_text() {
 let base=root().join("reports/windows/WIN-CDR-SAVE-20261022-01/focused/native-preservation");
 for name in ["identity-v14.cdr","identity-v14-edited.cdr","workspace-v14.cdr","workspace-v14-edited.cdr","images-v14.cdr","images-v14-edited.cdr"] {
  let (d,_)=cdr::decode(&fs::read(base.join(name)).unwrap(),"actual").unwrap();assert!(paint::has_cmyk(&d),"{name}");
  let p=dir().join("actual.arranger");native::save(&d,0,&p,None,false).unwrap();assert_eq!(native::open(&p).unwrap().document,d);
  let out=cdr::decode(&cdr::encode_document(&d).unwrap().0,"actual").unwrap().0;
  for a in &d.objects {let b=out.object(&a.id).unwrap();assert_eq!(a.style,b.style);assert_eq!((a.visible,a.locked,&a.page_id),(b.visible,b.locked,&b.page_id));}
 }
}
#[test]
fn screen_svg_png_pdf_use_explicit_unmanaged_rgb_approximation() {
 let mut d=scene();d.objects.truncate(1);d.objects[0].style.fill="cmyk(100,0,0,0)".into();d.objects[0].style.stroke="none".into();let renderer=Renderer::new();
 let image=renderer.render(&d,"page-1",25.4,None).unwrap();let p=image.pixels.pixel(25,35).unwrap();assert_eq!([p.red(),p.green(),p.blue(),p.alpha()],[0,255,255,255]);assert!(image.warnings.iter().any(|n|n.code=="CMYK_RGB_APPROXIMATION"));
 let original=d.hash();let directory=dir();for (format,ext) in [(ExportFormat::Svg,"svg"),(ExportFormat::Png,"png"),(ExportFormat::Pdf,"pdf")] {let (_,w)=renderer.export(&d,"page-1",format,72.,&directory.join(format!("preview.{ext}")),false).unwrap();assert!(w.iter().any(|n|n.code=="CMYK_RGB_APPROXIMATION"));}
 assert_eq!(d.hash(),original);let svg=fs::read_to_string(directory.join("preview.svg")).unwrap();assert!(svg.contains("#00ffff")&&!svg.contains("cmyk("));
}
