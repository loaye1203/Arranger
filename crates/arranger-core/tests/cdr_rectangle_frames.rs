use arranger_core::{cdr,Geometry};
use kurbo::Affine;

// These are existing, byte-verified Corel2019 controls from report84. Two
// failed native14 scaled controls remain negative inputs, not new successes.
#[test]
fn actual_scaled_rectangles_keep_type_geometry_and_effective_pen(){
    let cases:[(&str,&[u8],bool,f64,f64);10]=[
        ("fixed-id14",include_bytes!("../../../examples/assets/cdr/corel2019/rectangle-scales/outline-fixed-identity-v14.cdr"),false,1.,1.),
        ("fixed-id21",include_bytes!("../../../examples/assets/cdr/corel2019/rectangle-scales/outline-fixed-identity-v21.cdr"),false,1.,1.),
        ("fixed-u14",include_bytes!("../../../examples/assets/cdr/corel2019/rectangle-scales/outline-fixed-uniform2-v14.cdr"),false,2.,2.),
        ("fixed-u21",include_bytes!("../../../examples/assets/cdr/corel2019/rectangle-scales/outline-fixed-uniform2-v21.cdr"),false,2.,2.),
        ("fixed-xy14",include_bytes!("../../../examples/assets/cdr/corel2019/rectangle-scales/outline-fixed-nonuniform2x1-v14.cdr"),false,2.,1.),
        ("fixed-xy21",include_bytes!("../../../examples/assets/cdr/corel2019/rectangle-scales/outline-fixed-nonuniform2x1-v21.cdr"),false,2.,1.),
        ("scaled-id14",include_bytes!("../../../examples/assets/cdr/corel2019/rectangle-scales/outline-scaled-identity-v14.cdr"),true,1.,1.),
        ("scaled-id21",include_bytes!("../../../examples/assets/cdr/corel2019/rectangle-scales/outline-scaled-identity-v21.cdr"),true,1.,1.),
        ("scaled-u21",include_bytes!("../../../examples/assets/cdr/corel2019/rectangle-scales/outline-scaled-uniform2-v21.cdr"),true,2.,2.),
        ("scaled-xy21",include_bytes!("../../../examples/assets/cdr/corel2019/rectangle-scales/outline-scaled-nonuniform2x1-v21.cdr"),true,2.,1.),
    ];
    for(name,bytes,scaled,sx,sy)in cases{
        let source=cdr::decode(bytes,name).unwrap_or_else(|e|panic!("{name}: {e:?}")).0;assert_eq!(source.objects.len(),1);let o=source.object("outline-scaling-control").unwrap();
        if name.ends_with("21"){assert!(matches!(o.geometry,Geometry::Rect{radius_mm:0.}),"native21 primitive cannot be replaced by a curve");}
        let b=source.bounds(o).unwrap();assert!((b[2]-20.*sx).abs()<0.00021&&(b[3]-10.*sy).abs()<0.00021,"{name} dimensions");
        assert_eq!(o.style.stroke_scale_with_object,scaled,"{name} native scaling mode");let[a,b,c,d,_,_]=if o.style.stroke_scale_with_object{source.world_transform(o).unwrap().as_coeffs()}else{Affine::IDENTITY.as_coeffs()};let w=o.style.stroke_width_mm;let covariance=[(a*a+c*c)*w*w,(b*b+d*d)*w*w,(a*b+c*d)*w*w];let expected=if scaled{[sx*sx,sy*sy,0.]}else{[1.,1.,0.]};for(a,b)in covariance.into_iter().zip(expected){assert!((a-b).abs()<1e-12,"{name} actual pen");}
        for version in [14,21]{let bytes=if version==14{cdr::encode_document(&source)}else{cdr::encode_document21(&source)}.unwrap().0;assert_eq!(cdr::decode(&bytes,name).unwrap().0,source,"{name} exchange{version}");}
    }
}

#[test]
fn actual_native14_scaled_failures_still_reject_instead_of_losing_the_outline(){
    for bytes in [include_bytes!("../../../examples/assets/cdr/corel2019/rectangle-scales/outline-scaled-uniform2-v14.cdr").as_slice(),include_bytes!("../../../examples/assets/cdr/corel2019/rectangle-scales/outline-scaled-nonuniform2x1-v14.cdr").as_slice()]{assert!(cdr::decode(bytes,"failed-native-control").is_err());}
}

#[test]
fn uniform_scaled_round_rect_stays_a_real_circle_in_local_frame_after_exchange(){
    let mut source=cdr::decode(include_bytes!("../../../examples/assets/cdr/corel2019/rectangle-scales/outline-scaled-identity-v21.cdr"),"rounded").unwrap().0;let o=&mut source.objects[0];o.geometry=Geometry::Rect{radius_mm:0.8};o.transform=(Affine::translate((60.,80.))*Affine::scale(2.5)).as_coeffs();
    let bytes=cdr::encode_document21(&source).unwrap().0;assert_eq!(cdr::decode(&bytes,"rounded").unwrap().0,source);
}
