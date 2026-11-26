use arranger_core::{cdr,Geometry};
#[test]
fn genuine_corel_ascii_area_stories_keep_complete_content_and_style_in_both_exchanges(){
 for raw in [include_bytes!("../../../examples/assets/cdr/corel2019/ascii-stories-20261030/corel-area-ascii-v14.cdr").as_slice(),include_bytes!("../../../examples/assets/cdr/corel2019/ascii-stories-20261030/corel-area-ascii-v21.cdr").as_slice()]{
  let doc=cdr::decode(raw,"ascii-original").unwrap().0;let text=doc.object("content").unwrap();let Geometry::Text{content,..}=&text.geometry else{panic!()};assert_eq!(content,"One editable\nstory");
  for encode in [cdr::encode_document,cdr::encode_document21]{let bytes=encode(&doc).unwrap().0;let copy=cdr::decode(&bytes,&doc.id).unwrap().0;assert_eq!(copy,doc);assert_eq!(cdr::decode(&encode(&copy).unwrap().0,&doc.id).unwrap().0,doc);}
 }
}
