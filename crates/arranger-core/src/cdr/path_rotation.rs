//! Exact cyclic symmetries of a live closed guide, never a copied guide/cache.
use super::{Result,unsupported};
use crate::{Document,Object,Geometry,TextAnchor};
use kurbo::{Affine,BezPath,PathEl,Point,ParamCurveArclen};

pub(super) fn rotate(p:Point,turns:u8)->Point{match turns{0=>p,1=>Point::new(-p.y,p.x),2=>Point::new(-p.x,-p.y),3=>Point::new(p.y,-p.x),_=>unreachable!()}}
pub(super) fn matrix(turns:u8,delta:[f64;2])->[f64;6]{let[a,b,c,d]=match turns{0=>[1.,0.,0.,1.],1=>[0.,1.,-1.,0.],2=>[-1.,0.,0.,-1.],3=>[0.,-1.,1.,0.],_=>unreachable!()};[a,b,c,d,delta[0],delta[1]]}

pub(super) fn pieces(path:&BezPath)->Option<Vec<Vec<Point>>>{
    let elements=path.elements();let PathEl::MoveTo(start)=*elements.first()?else{return None};
    if elements.last()!=Some(&PathEl::ClosePath){return None;}
    let mut current=start;let mut result=Vec::new();
    for el in &elements[1..elements.len()-1]{
        let points=match *el{PathEl::LineTo(p)=>vec![current,p],PathEl::CurveTo(a,b,p)=>vec![current,a,b,p],PathEl::QuadTo(a,p)=>vec![current,a,p],_=>return None};
        current=*points.last()?;result.push(points);
    }
    if current!=start{result.push(vec![current,start]);}
    (result.len()>1).then_some(result)
}
/// Compare the requested affine map with every active control, exactly. A
/// changed point, different contour order, or approximate symmetry is refused.
pub(super) fn shift(path:&BezPath,world:Affine,turns:u8,delta:[f64;2])->Option<usize>{
    let pieces=pieces(path)?;let world=pieces.iter().map(|s|s.iter().map(|p|world**p).collect::<Vec<_>>()).collect::<Vec<_>>();
    if world.iter().flatten().any(|p|!p.x.is_finite()||!p.y.is_finite()){return None;}
    let wanted=world.iter().map(|s|s.iter().map(|p|rotate(*p,turns)+kurbo::Vec2::new(delta[0],delta[1])).collect::<Vec<_>>()).collect::<Vec<_>>();
    // KMP over a doubled cyclic sequence. Repeated nodes/segments must not
    // make a failed symmetry search quadratic in a large editable guide.
    let n=world.len();let mut prefix=vec![0usize;n];
    for i in 1..n{let mut k=prefix[i-1];while k>0&&wanted[i]!=wanted[k]{k=prefix[k-1];}if wanted[i]==wanted[k]{k+=1;}prefix[i]=k;}
    let mut k=0;
    for i in 0..2*n-1{
        while k>0&&world[i%n]!=wanted[k]{k=prefix[k-1];}
        if world[i%n]==wanted[k]{k+=1;}
        if k==n{let offset=i+1-n;if offset>0&&offset<n{return Some(offset);}k=prefix[n-1];}
    }None
}
fn segment_path(points:&[Point])->BezPath{
    let mut p=BezPath::new();p.move_to(points[0]);match points{[_,b]=>p.line_to(*b),[_,a,b]=>p.quad_to(*a,*b),[_,a,b,c]=>p.curve_to(*a,*b,*c),_=>unreachable!()}p
}
pub(super) fn offsets(doc:&Document,text:&Object,raw:&BezPath,turns:u8,delta:[f64;2])->Result<[f64;2]>{
    let Geometry::Text{text_path:Some(binding),..}=&text.geometry else{return Err(super::invalid("rotated fit requires a live guide"))};let guide=doc.object(&binding.path_id)?;
    let world=doc.world_transform(guide)?;let source=guide.local_path()?;
    let original=shift(&source,world,turns,delta);let actual=shift(raw,world,turns,delta);
    let Some(offset)=actual.filter(|v|Some(*v)==original)else{return Err(unsupported("fitted text rotation requires an exact cyclic symmetry of the original closed guide"))};
    let segments=pieces(raw).unwrap();let lengths=segments.iter().map(|s|(world*segment_path(s)).segments().map(|v|v.arclen(1e-6)).sum::<f64>()).collect::<Vec<_>>();let length:f64=lengths.iter().sum();let mut along:f64=lengths[..offset].iter().sum();
    if !length.is_finite()||length<=0.||!along.is_finite(){return Err(unsupported("rotated fitted guide has no finite cyclic length"));}
    let run=crate::text::layout(doc,text)?;let parent=super::path_text::parent_scale(doc,text)?;let advance=run.advance_mm*parent;
    let start=binding.start_offset_mm*parent-match binding.anchor{TextAnchor::Start=>0.,TextAnchor::Middle=>advance/2.,TextAnchor::End=>advance};
    if start+along>=length{along-=length;}
    // A closed guide does not authorize the encoder to change the clipping
    // regime at its distinguished start/end. Never split or wrap a story.
    if run.overflow||start<0.||start+advance>length||start+along<0.||start+along+advance>length{return Err(unsupported("rotated fitted story would cross the live guide endpoint/overflow boundary"));}
    Ok([along,0.])
}

#[cfg(test)]pub(super) mod tests{
    use super::*;
    use super::super::binary::*;
    pub(in crate::cdr) fn source(curved:bool,parent:bool,turns:u8,outlined:bool)->Document{
        let guide=if curved{"M60 20C82 20 100 38 100 60C100 82 82 100 60 100C38 100 20 82 20 60C20 38 38 20 60 20Z"}else{"M20 20L100 20L100 100L20 100Z"};
        let mut objects=vec![serde_json::json!({"id":"guide","page_id":"page-1","x_mm":35.,"y_mm":55.,"width_mm":120.,"height_mm":120.,"geometry":{"kind":"path","d":guide},"style":{"fill":"none","stroke":"#888888","stroke_width_mm":0.2,"opacity":1.}})];
        if parent{objects.insert(0,serde_json::json!({"id":"parent","page_id":"page-1","x_mm":3.,"y_mm":4.,"width_mm":0.,"height_mm":0.,"transform":[0.,2.,-2.,0.,0.,0.],"geometry":{"kind":"group"}}));}
        let center=Point::new(95.,115.);let moved=rotate(center,turns);let delta=[center.x-moved.x,center.y-moved.y];let px=if parent{-8.}else{0.};let py=if parent{6.}else{0.};let r=rotate(Point::new(px,py),turns);let local=Point::new(delta[0]-px+r.x,delta[1]-py+r.y);let local=if parent{Point::new(local.y/2.,-local.x/2.)}else{local};
        let offset=rotate(Point::new(0.25,-0.5),turns);
        let mut text=serde_json::json!({"id":"text","page_id":"page-1","x_mm":0.25,"y_mm":-0.5,"width_mm":60.,"height_mm":12.,"transform":matrix(turns,[local.x-offset.x,local.y-offset.y]),"geometry":{"kind":"text","content":"Live 42","font_family":"Arial","font_size_mm":4.,"layout":{"letter_spacing_mm":0.125,"line_height_mm":5.25},"text_path":{"path_id":"guide","start_offset_mm":15.,"baseline_offset_mm":0.125,"anchor":"start"}}});
        if parent{text["parent_id"]="parent".into();}if outlined{text["style"]=serde_json::json!({"fill":"#123456","stroke":"#123456","stroke_width_mm":0.125,"opacity":1.});}objects.push(text);
        serde_json::from_value(serde_json::json!({"id":"closed-rotation","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":objects})).unwrap()
    }
    #[test]fn closed_symmetry_rotation_keeps_complete_source_and_bare_native_glyphs(){
        for curved in[false,true]{for parent in[false,true]{for turns in[1,2,3]{for outlined in[false,true]{for version in[14,21]{
            let source=source(curved,parent,turns,outlined);let encode=if version==14{crate::cdr::encode_document}else{crate::cdr::encode_document21};let bytes=encode(&source).unwrap_or_else(|e|panic!("curve{curved} parent{parent} turn{turns} outline{outlined} v{version}: {e:?}")).0;
            let actual=crate::cdr::decode(&bytes,&source.id).unwrap().0;assert_eq!(actual,source,"curve{curved} parent{parent} turn{turns} outline{outlined} v{version}");let native=super::super::read::decode_geometry(&bytes,"bare").unwrap().0;
            assert_eq!(native.objects.len(),source.objects.len());assert!(super::super::path_outline::preserves(&native,native.object("text").unwrap(),&source,source.object("text").unwrap()).unwrap());
            assert_eq!(crate::cdr::decode(&encode(&actual).unwrap().0,&source.id).unwrap().0,source);
            let g=native.object("guide").unwrap();let s=source.object("guide").unwrap();let Geometry::Path{d,..}=&s.geometry else{unreachable!()};let projected=super::super::path_precision::projected(d,1).unwrap();assert!(super::super::path_precision::same_world(&g.local_path().unwrap(),native.world_transform(g).unwrap(),&projected,source.world_transform(s).unwrap()),"curve{curved} parent{parent} turn{turns} outline{outlined} v{version}");
        }}}}}
    }
    #[test]fn exact_cyclic_controls_are_required_not_only_a_matching_box(){
        let p=crate::path::parse("M0 0L80 0L80 80L0 80Z").unwrap();assert_eq!(shift(&p,Affine::IDENTITY,1,[80.,0.]),Some(1));assert_eq!(shift(&p,Affine::IDENTITY,3,[0.,80.]),Some(3));
        assert_eq!(shift(&p,Affine::IDENTITY,1,[80.0000001,0.]),None);
        for d in["M0 0L80 0L80 80L0 80","M0 0L80 0L80 80.0001L0 80Z","M0 0L80 80L80 0L0 80Z"]{assert_eq!(shift(&crate::path::parse(d).unwrap(),Affine::IDENTITY,1,[80.,0.]),None);}
    }
    #[test]fn repeated_degenerate_prefixes_and_long_real_cycles_keep_exact_matching(){
        let mut p=BezPath::new();p.move_to((0.,0.));for _ in 0..20000{p.line_to((0.,0.));}p.line_to((80.,0.));p.line_to((0.,0.));p.close_path();assert_eq!(shift(&p,Affine::IDENTITY,2,[0.,0.]),None);
        let mut p=BezPath::new();p.move_to((0.,0.));for _ in 0..4000{p.line_to((80.,0.));p.line_to((80.,80.));p.line_to((0.,80.));p.line_to((0.,0.));}p.close_path();assert_eq!(shift(&p,Affine::IDENTITY,1,[80.,0.]),Some(1));
    }
    fn pack(n:&mut NativeRecords)->Vec<u8>{if n.version==2100{repack21(n).unwrap()}else{n.members.insert("content/riffData.cdr".into(),write(&n.nodes).unwrap());zip(&n.members.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>())}}
    #[test]fn native_fit_word_guide_node_and_subgrid_guide_moves_survive_old_rotation_notes(){
        for version in[14,21]{for mode in 0..3{
            let source=source(true,true,1,true);let encode=if version==14{crate::cdr::encode_document}else{crate::cdr::encode_document21};let mut n=read_native(&encode(&source).unwrap().0).unwrap();
            fn edit(nodes:&mut[Node],version:u16,mode:u8)->bool{for node in nodes{
                if mode==0&&node.tag==*b"txsm"{let at=if version==14{112}else{117};let old=i32_at(&node.data,at).unwrap();set32(&mut node.data,at,(old+1)as u32).unwrap();return true;}
                if mode!=0&&node.kind()==Some(b"obj "){let(kind,mut args)=super::super::write::arguments(&node.find(b"loda").unwrap().data).unwrap();if args.iter().any(|(k,v)|*k==1000&&read_wide(v).unwrap()=="guide"){
                    if mode==1{let raw=&mut args.iter_mut().find(|(k,_)|*k==30).unwrap().1;let old=i32_at(raw,12).unwrap();set32(raw,12,(old+1)as u32).unwrap();node.find_mut(b"loda").unwrap().data=super::super::write::loda(kind,&args);}
                    else{let raw=&mut node.find_mut(b"trfd").unwrap().data;let start=u32_at(raw,u32_at(raw,8).unwrap()as usize).unwrap()as usize+16;let old=f64_at(raw,start+16).unwrap();raw[start+16..start+24].copy_from_slice(&(old+0.001).to_le_bytes());}return true;
                }}if let Some(children)=&mut node.children{if edit(children,version,mode){return true;}}
            }false}
            assert!(edit(&mut n.nodes,version,mode));let raw=pack(&mut n);let actual=crate::cdr::decode(&raw,"actual").unwrap().0;let bare=super::super::read::decode_geometry(&raw,"bare").unwrap().0;
            assert_eq!(actual.object("text").unwrap().transform,Affine::IDENTITY.as_coeffs());assert!(super::super::path_outline::preserves(&actual,actual.object("text").unwrap(),&bare,bare.object("text").unwrap()).unwrap(),"v{version} mode{mode}");
            let(a,b)=(actual.object("guide").unwrap(),bare.object("guide").unwrap());let(pa,pb)=(actual.world_transform(a).unwrap()*a.local_path().unwrap(),bare.world_transform(b).unwrap()*b.local_path().unwrap());
            let controls=|path:&BezPath|path.elements().iter().flat_map(|e|match *e{PathEl::MoveTo(p)|PathEl::LineTo(p)=>vec![p],PathEl::QuadTo(a,p)=>vec![a,p],PathEl::CurveTo(a,b,p)=>vec![a,b,p],PathEl::ClosePath=>vec![]}).collect::<Vec<_>>();
            let(a,b)=(controls(&pa),controls(&pb));assert_eq!(a.len(),b.len());assert!(a.into_iter().zip(b).all(|(p,q)|(p.x-q.x).abs()<1e-11&&(p.y-q.y).abs()<1e-11),"every actual guide control must survive v{version} mode{mode}");
        }}
    }
    #[test]fn noncyclic_bends_extra_moves_nonquarter_angles_and_endpoint_crossings_refuse(){
        for mode in 0..5{let mut d=source(true,false,3,true);let text=d.objects.iter().position(|o|o.id=="text").unwrap();match mode{
            0=>{if let Geometry::Path{d,..}=&mut d.objects[0].geometry{*d=d.replace("82 20","82 20.0001");}},
            1=>d.objects[text].transform[4]+=0.0000001,
            2=>d.objects[text].transform=[0.6,0.8,-0.8,0.6,210.,20.],
            3=>{if let Geometry::Path{d,..}=&mut d.objects[0].geometry{*d=d.trim_end_matches('Z').into();}},
            _=>{let Geometry::Text{text_path:Some(p),..}=&mut d.objects[text].geometry else{unreachable!()};p.start_offset_mm=55.;},
        }for encode in[crate::cdr::encode_document,crate::cdr::encode_document21]{assert!(encode(&d).is_err(),"mode{mode}");}}
    }
    #[test]fn missing_scalar_rotation_contract_never_applies_the_old_frame_again(){
        for version in[14,21]{for parent in[false,true]{for mode in 0..2{
            let s=source(true,parent,1,true);let encode=if version==14{crate::cdr::encode_document}else{crate::cdr::encode_document21};let mut n=read_native(&encode(&s).unwrap().0).unwrap();
            for key in["metadata/metadata.xml","META-INF/metadata.xml"]{if let Some(raw)=n.members.get_mut(key){let text=String::from_utf8(raw.clone()).unwrap();let prefix="ArrangerTextParametersV1|sha256:";if let Some(start)=text.find(prefix){let end=start+text[start..].find([';','<']).unwrap();let mut value:serde_json::Value=super::super::metadata::decode(prefix,&text[start..end]).unwrap();
                if mode==0{let t=value["text"]["text"].as_object_mut().unwrap();t.remove("path_quarter_world");}
                let replacement=if mode==0{super::super::metadata::encode(prefix,&value).unwrap()}else{String::new()};*raw=text.replacen(&text[start..end],&replacement,1).into_bytes();
            }}}
            let raw=pack(&mut n);let actual=crate::cdr::decode(&raw,"actual").unwrap().0;let bare=super::super::read::decode_geometry(&raw,"bare").unwrap().0;assert_eq!(actual.object("text").unwrap().transform,Affine::IDENTITY.as_coeffs());assert!(super::super::path_outline::preserves(&actual,actual.object("text").unwrap(),&bare,bare.object("text").unwrap()).unwrap(),"v{version} parent{parent} mode{mode}");
        }}}
    }
}
