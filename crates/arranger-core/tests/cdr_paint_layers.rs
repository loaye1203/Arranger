use arranger_core::{cdr,Document};
use serde_json::json;
#[cfg(target_os="macos")]
const FONT:&str="PingFang SC";
#[cfg(not(target_os="macos"))]
const FONT:&str="FZHei-B01S";
fn source(kind:&str,fill:&str,stroke:&str,opacity:f64,clipped:bool)->Document{
 let geometry=match kind{"rect"=>json!({"kind":"rect","radius_mm":0.75}),"ellipse"=>json!({"kind":"ellipse"}),_=>json!({"kind":"path","d":"M0.1234567 0 C10 12 32 -8 60 0 L60 40 H0 Z","fill_rule":"nonzero"})};
 let mut obj=json!({"id":"content","page_id":"page-1","x_mm":40.,"y_mm":60.,"width_mm":60.,"height_mm":40.,"geometry":geometry,"style":{"fill":fill,"stroke":stroke,"stroke_width_mm":1.234567,"stroke_linecap":"round","stroke_linejoin":"bevel","stroke_miterlimit":3.,"opacity":opacity}});
 if clipped{obj["clip"]=json!({"kind":"rect","rect_mm":[2.,3.,45.,30.]});}
 serde_json::from_value(json!({"id":"layers","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"objects":[obj],"resources":{}})).unwrap()
}
#[test]
fn independent_alpha_layers_keep_complete_vector_models_and_two_exchanges(){
 for kind in ["rect","ellipse","path"]{for clipped in [false,true]{for(fill,stroke)in[("#1E5ab480","#Dc502840"),("#1e5ab400","#dc5028fe"),("#1e5ab4ff","#dc502880")]{let doc=source(kind,fill,stroke,1.,clipped);
  for encode in [cdr::encode_document,cdr::encode_document21]{let(bytes,_)=encode(&doc).unwrap();let imported=cdr::decode(&bytes,&doc.id).unwrap().0;assert_eq!(doc,imported,"{kind}/{clipped}/{fill}/{stroke}");let again=encode(&imported).unwrap().0;assert_eq!(cdr::decode(&again,&doc.id).unwrap().0,doc);}
 }}}
}
#[test]
fn isolated_outer_opacity_uses_native21_group_and_legacy14_remains_explicitly_unsupported(){
 for fill in ["#1e5ab480","#1e5ab4ff"]{let doc=source("rect",fill,"#dc502840",0.37654321,true);assert_eq!(cdr::encode_document(&doc).unwrap_err().code,"CDR_UNSUPPORTED");let(bytes,_)=cdr::encode_document21(&doc).unwrap();assert_eq!(cdr::decode(&bytes,&doc.id).unwrap().0,doc);}
}
#[test]
fn selection_export_and_preflight_use_the_same_paint_layer_encoder(){
 let doc=source("rect","#1e5ab480","#dc502840",1.,false);
 assert_eq!(cdr::check_document(&doc)["can_export"],true);assert_eq!(cdr::check(&doc,"page-1")["can_export"],true);assert_eq!(cdr::check_document21(&doc)["can_export"],true);
 for encode in [cdr::encode,cdr::encode21]{let(bytes,_)=encode(&doc,"page-1").unwrap();assert_eq!(cdr::decode(&bytes,&doc.id).unwrap().0,doc);}
}
#[test]
fn editable_multiline_text_keeps_its_story_and_exact_style_in_both_formats(){
 for mode in ["point","area"]{let mut doc=source("rect","#1e5ab480","#dc502840",1.,false);doc.objects[0].geometry=serde_json::from_value(json!({"kind":"text","content":"Arranger\n中文","font_family":FONT,"font_size_mm":5.,"layout":{"mode":mode,"align":"left","line_height_mm":7.}})).unwrap();
  for encode in [cdr::encode_document,cdr::encode_document21]{let(raw,_)=encode(&doc).unwrap();assert_eq!(cdr::decode(&raw,&doc.id).unwrap().0,doc);}
 }
}
#[test]
fn hierarchy_shared_workspace_and_generated_name_collisions_keep_original_serial_order(){
 let mut doc=source("path","#1e5ab480","#dc502840",1.,true);let content=doc.objects.remove(0);
 let mut group=content.clone();group.id="parent".into();group.geometry=arranger_core::Geometry::Group;group.x_mm=2.;group.y_mm=3.;group.transform=[0.8,0.2,-0.2,0.8,5.,6.];group.style=Default::default();group.clip=None;
 let mut shared=content.clone();shared.id="content-paint-fill".into();shared.page_id=arranger_core::WORKSPACE_PAGE.into();shared.parent_id=None;shared.style.stroke="none".into();
 let mut content=content;content.parent_id=Some(group.id.clone());doc.objects=vec![group,shared,content];
 let mut page=doc.pages[0].clone();page.id="blank".into();doc.pages.push(page);
 for encode in [cdr::encode_document,cdr::encode_document21]{let raw=encode(&doc).unwrap().0;assert_eq!(cdr::decode(&raw,&doc.id).unwrap().0,doc);}
}
