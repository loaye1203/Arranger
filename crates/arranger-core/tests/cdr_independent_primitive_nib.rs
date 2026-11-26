use arranger_core::{cdr,Document,Geometry};
use serde_json::json;
const ROOT:&str=concat!(env!("CARGO_MANIFEST_DIR"),"/../../examples/assets/cdr/corel2019/canonical-nibs");
#[test]
fn all_six_real_corel_controls_keep_the_live_rectangle_and_both_pen_axes(){
    for version in [14,21]{for profile in ["horizontal","vertical","circular"]{
        let raw=std::fs::read(format!("{ROOT}/{profile}-v{version}.cdr")).unwrap();let doc=cdr::decode(&raw,"native").unwrap().0;let o=doc.object("outline-scaling-control").unwrap();assert!(matches!(o.geometry,Geometry::Rect{radius_mm:0.}));
        //21 uses its measured pixel-grid paper origin; do not assume the
        // two file versions have identical paper-relative translations.
        let bounds=doc.bounds(o).unwrap();assert!((bounds[2]-40.).abs()<1e-10&&(bounds[3]-10.).abs()<1e-10);
        let m=doc.world_transform(o).unwrap().as_coeffs();let w=o.style.stroke_width_mm;let x=(m[0].hypot(m[2])*w,y(m,w));
        let wanted=match profile{"horizontal"=>(2.,1.),"vertical"=>(1.,2.),_=>(1.,1.)};assert!((x.0-wanted.0).abs()<1e-10&&(x.1-wanted.1).abs()<1e-10,"{profile} native{version}: {x:?}");
        for encode in [cdr::encode_document,cdr::encode_document21]{let copy=cdr::decode(&encode(&doc).unwrap().0,"native").unwrap().0;assert_eq!(copy,doc);}
    }}
}
fn y(m:[f64;6],w:f64)->f64{m[1].hypot(m[3])*w}
fn source(vertical:bool)->Document {serde_json::from_value(json!({"id":"independent","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"rect","page_id":"page-1","x_mm":20.,"y_mm":30.,"width_mm":20.,"height_mm":10.,"transform":if vertical{[1.,0.,0.,2.,0.,0.]}else{[2.,0.,0.,1.,0.,0.]},"geometry":{"kind":"rect","radius_mm":0.},"style":{"fill":"#123456","stroke":"#000000","stroke_width_mm":0.25,"opacity":1.}}]})).unwrap()}
#[test]
fn horizontal_and_vertical_model_charts_keep_source_dimensions_and_stroke_units(){
    for vertical in [false,true]{let source=source(vertical);for encode in [cdr::encode_document,cdr::encode_document21]{let bytes=encode(&source).unwrap().0;let actual=cdr::decode(&bytes,&source.id).unwrap().0;assert_eq!(actual,source);assert_eq!(cdr::decode(&encode(&actual).unwrap().0,&source.id).unwrap().0,source);}}
}
#[test]
fn integer_percentage_axis_charts_preserve_primitive_pen_and_two_exchanges(){
 for aspect in [1u16,25,40,60,75,99]{for vertical in [false,true]{
  let mut doc=source(false);let r=f64::from(aspect)/100.;doc.objects[0].transform=if vertical{[r,0.,0.,1.,0.,0.]}else{[1.,0.,0.,r,0.,0.]};
  for encode in [cdr::encode_document,cdr::encode_document21]{let raw=encode(&doc).unwrap().0;let copy=cdr::decode(&raw,&doc.id).unwrap().0;assert_eq!(copy,doc,"{aspect}/{vertical}");assert!(matches!(copy.objects[0].geometry,Geometry::Rect{radius_mm:0.}));assert_eq!(cdr::decode(&encode(&copy).unwrap().0,&doc.id).unwrap().0,doc);}
 }}
}

#[test]
fn signed_axis_permutations_keep_dimensions_units_and_two_native_exchanges(){
 for q in [0.01,0.25,0.4,0.5,0.6,0.75,0.99]{for swap in [false,true]{for xsign in [-1.,1.]{for ysign in [-1.,1.]{
  let mut doc=source(false);doc.objects[0].transform=if swap{[0.,ysign*q,xsign,0.,100.,80.]}else{[xsign,0.,0.,ysign*q,100.,80.]};
  for encode in [cdr::encode_document,cdr::encode_document21]{let raw=encode(&doc).unwrap().0;let copy=cdr::decode(&raw,&doc.id).unwrap().0;assert_eq!(copy,doc,"{q}/{swap}/{xsign}/{ysign}");assert_eq!(cdr::decode(&encode(&copy).unwrap().0,&doc.id).unwrap().0,doc);}
 }}}}
}

#[test]
fn axis_ellipses_keep_live_type_and_exact_source_charts_through_two_exchanges(){
 for q in [0.01,0.25,0.4,0.5,0.6,0.75,0.99]{for swap in [false,true]{for xsign in [-1.,1.]{for ysign in [-1.,1.]{
  let mut doc=source(false);doc.objects[0].geometry=Geometry::Ellipse;doc.objects[0].transform=if swap{[0.,ysign*q,xsign,0.,100.,80.]}else{[xsign,0.,0.,ysign*q,100.,80.]};
  for encode in [cdr::encode_document,cdr::encode_document21]{let raw=encode(&doc).unwrap().0;let copy=cdr::decode(&raw,&doc.id).unwrap().0;assert_eq!(copy,doc,"{q}/{swap}/{xsign}/{ysign}");assert!(matches!(copy.objects[0].geometry,Geometry::Ellipse));assert_eq!(cdr::decode(&encode(&copy).unwrap().0,&doc.id).unwrap().0,doc);}
 }}}}
}

#[test]
fn nested_signed_parent_and_child_charts_recover_both_primitive_families(){
 for geometry in [Geometry::Rect{radius_mm:0.},Geometry::Ellipse]{
  let mut doc=source(false);let mut parent=doc.objects[0].clone();parent.id="parent".into();parent.x_mm=0.;parent.y_mm=0.;parent.geometry=Geometry::Group;parent.style=Default::default();parent.transform=[0.,-2.,1.,0.,100.,100.];
  doc.objects[0].geometry=geometry;doc.objects[0].parent_id=Some(parent.id.clone());doc.objects[0].transform=[0.,0.5,-1.,0.,10.,5.];doc.objects.insert(0,parent);
  for encode in [cdr::encode_document,cdr::encode_document21]{let copy=cdr::decode(&encode(&doc).unwrap().0,&doc.id).unwrap().0;assert_eq!(copy,doc);assert_eq!(cdr::decode(&encode(&copy).unwrap().0,&doc.id).unwrap().0,doc);}
 }
}

#[test]
fn rotated_orthogonal_primitive_charts_stay_live_and_source_exact(){
 for geometry in [Geometry::Rect{radius_mm:0.},Geometry::Ellipse]{for reflect in [-1.,1.]{for q in [0.5,0.6]{
  let mut doc=source(false);doc.objects[0].geometry=geometry.clone();doc.objects[0].transform=[0.8,0.6,-0.6*q*reflect,0.8*q*reflect,60.,60.];
  for encode in [cdr::encode_document,cdr::encode_document21]{let raw=encode(&doc).unwrap().0;let copy=cdr::decode(&raw,&doc.id).unwrap().0;assert_eq!(copy,doc,"{geometry:?}/{reflect}/{q}");assert_eq!(cdr::decode(&encode(&copy).unwrap().0,&doc.id).unwrap().0,doc);}
 }}}
}
