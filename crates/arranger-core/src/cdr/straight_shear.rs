//! Measured native font-frame shear, isolated from the ordinary model codec.
//! Corel's curved-guide cache refits origins rather than applying a whole-story
//! affine. On a horizontal guide, a baseline-fixed x shear is a bounded test of
//! that distinction. Actual Corel glyphs/resaves, including the zero controls,
//! must establish it; native fields and source roundtrips alone cannot.
use super::{binary::*,invalid,unsupported,Result};
use crate::{Document,Geometry,TextAnchor,TextLayoutOptions,command::Notice};
use kurbo::{Affine,PathEl,Rect};

fn carrier(source:&Document,id:&str)->Result<(Document,f64)>{
    source.validate()?;
    let text=source.object(id)?;
    let Geometry::Text{content,text_path:Some(binding),layout,..}=&text.geometry else{return Err(unsupported("straight shear research needs a live fitted story"))};
    if source.pages.len()!=1||!source.resources.is_empty()||source.objects.len()!=3||source.objects.iter().any(|o|o.parent_id.is_some()||o.clip.is_some()){
        return Err(unsupported("straight shear research is limited to one page and three independent resource-free objects"));
    }
    if text.x_mm!=0.||text.y_mm!=0.||text.style.stroke!="none"||text.style.opacity!=1.||*layout!=TextLayoutOptions::default()||!content.is_ascii()||content.is_empty()||binding.anchor!=TextAnchor::Start{
        return Err(unsupported("straight shear research needs nonempty ASCII start-anchored plain fill text with zero local origin"));
    }
    let guide=source.object(&binding.path_id)?;
    if guide.transform!=Affine::IDENTITY.as_coeffs()||guide.x_mm!=0.||guide.y_mm!=0.||guide.style.fill!="none"||guide.style.stroke!="none"{
        return Err(unsupported("straight shear research needs an unchanged unpainted page-space guide"));
    }
    let Geometry::Path{d,..}=&guide.geometry else{return Err(unsupported("straight shear guide must be a curve"))};
    let path=crate::path::parse(d)?;
    let [PathEl::MoveTo(p),PathEl::LineTo(q)]=path.elements()else{return Err(unsupported("straight shear research needs exactly one line segment"))};
    if p.y!=q.y||q.x<=p.x{return Err(unsupported("straight shear research needs a left-to-right horizontal guide"));}
    let baseline=p.y+binding.baseline_offset_mm;
    let [a,b,k,c,e,f]=text.transform;
    if a!=1.||b!=0.||c!=1.||!k.is_finite()||k.abs()>1.||e!= -k*baseline||f!=0.{
        return Err(unsupported("straight shear research requires an exact unit-diagonal x shear fixed at the fitted baseline"));
    }
    let remaining=source.objects.iter().filter(|o|o.id!=text.id&&o.id!=guide.id).collect::<Vec<_>>();
    if remaining.len()!=1||!matches!(remaining[0].geometry,Geometry::Rect{radius_mm:0.})||remaining[0].transform!=Affine::IDENTITY.as_coeffs(){return Err(unsupported("straight shear research needs one independent rectangular witness"));}
    let mut result=source.clone();result.objects.iter_mut().find(|o|o.id==id).unwrap().transform=Affine::IDENTITY.as_coeffs();
    let neutral=result.object(id)?;let run=crate::text::layout(&result,neutral)?;
    if run.overflow||binding.start_offset_mm<0.||binding.start_offset_mm+run.advance_mm>q.x-p.x{return Err(unsupported("straight shear research cannot cross a live endpoint"));}
    Ok((result,k))
}

pub(super) fn encode(source:&Document,id:&str,modern:bool)->Result<(Vec<u8>,Vec<Notice>)>{
    let (neutral,k)=carrier(source,id)?;
    let (bytes,mut warnings)=if modern{super::encode_document21(&neutral)?}else{super::encode_document(&neutral)?};
    let mut native=read_native(&bytes)?;let mut changed=0;
    let target=source.object(id)?;let [x,y,w,h]=source.bounds(target)?;let bounds=Rect::new(x,y,x+w,y+h);
    fn patch(nodes:&mut[Node],id:&str,k:f64,modern:bool,bounds:Rect,size:[f64;2],changed:&mut usize)->Result<()>{
        for node in nodes{
            if node.kind()==Some(b"obj "){
                let (kind,args)=super::write::arguments(&node.find(b"loda")?.data)?;
                let name=args.iter().find(|(key,_)|*key==1000).map(|(_,v)|read_wide(v)).transpose()?;
                if name.as_deref()==Some(id){
                    if kind!=4{return Err(invalid("straight shear native target is not artistic text"));}
                    let current=super::read::matrix(node)?;
                    let at=if modern{49}else{44};let story=node.find(b"txsm")?.data.clone();
                    if u32_at(&story,at+48)?!=1{return Err(invalid("straight shear lost native fit options"));}
                    let mut matrix=[0.;6];for(i,v)in matrix.iter_mut().enumerate(){*v=f64_at(&story,at+i*8)?;}
                    super::path_text::check_frame(matrix,current)?;
                    // Raw native order [a,c,x,b,d,y], with a y-up axis.
                    // The two matching linear fields are the measured Corel
                    // shear profile. Their derived translations stay untouched.
                    let trfd=&mut node.find_mut(b"trfd")?.data;let start=u32_at(trfd,u32_at(trfd,8)?as usize)?as usize+16;
                    trfd[start+8..start+16].copy_from_slice(&(-k).to_le_bytes());
                    node.find_mut(b"txsm")?.data[at+8..at+16].copy_from_slice(&(-k).to_le_bytes());
                    super::write::painted_bounds(node,bounds,bounds,size)?;*changed+=1;
                }
                // Do not recurse into this object's leaf records.
            }else if let Some(children)=&mut node.children{patch(children,id,k,modern,bounds,size,changed)?;}
        }Ok(())
    }
    patch(&mut native.nodes,id,k,modern,bounds,[source.pages[0].width_mm,source.pages[0].height_mm],&mut changed)?;
    if changed!=1{return Err(invalid("straight shear native target missing or ambiguous"));}
    // No stale source chart may make this experiment look like a supported
    // full-fidelity codec. Complete requested sources are separate .arranger
    // files. In particular, no Notes supply hidden document/glyph coordinates.
    native.members.remove("metadata/metadata.xml");native.members.remove("META-INF/metadata.xml");
    let bytes=if modern{super::binary::write21(&native.nodes,native.members)?}else{
        native.members.insert("content/riffData.cdr".into(),write(&native.nodes)?);
        let mut keys=native.members.keys().filter(|k|k.as_str()!="mimetype").cloned().collect::<Vec<_>>();keys.insert(0,"mimetype".into());
        zip(&keys.iter().map(|key|(key.as_str(),native.members[key].as_slice())).collect::<Vec<_>>())
    };
    warnings.push(Notice::new("CDR_STRAIGHT_SHEAR_RESEARCH","Controlled real native font-frame experiment only; Corel glyph placement and resave are unverified. Ordinary export and nonneutral import remain guarded. Keep the separate complete source."));
    Ok((bytes,warnings))
}

#[cfg(test)]
mod tests{
    use super::*;
    fn source(k:f64)->Document{
        serde_json::from_str(&format!(r##"{{"id":"shear","unit":"mm","pages":[{{"id":"page-1","width_mm":210,"height_mm":297}}],"resources":{{}},"objects":[{{"id":"guide","page_id":"page-1","parent_id":null,"x_mm":0,"y_mm":0,"width_mm":160,"height_mm":0.1,"transform":[1,0,0,1,0,0],"style":{{"fill":"none","stroke":"none","stroke_width_mm":0.1,"opacity":1}},"visible":true,"locked":false,"clip":null,"geometry":{{"kind":"path","d":"M20 80L180 80","fill_rule":"nonzero"}}}},{{"id":"content","page_id":"page-1","parent_id":null,"x_mm":0,"y_mm":0,"width_mm":70,"height_mm":8,"transform":[1,0,{k},1,{},0],"style":{{"fill":"#000000","stroke":"none","stroke_width_mm":0.1,"opacity":1}},"visible":true,"locked":false,"clip":null,"geometry":{{"kind":"text","content":"HAM 42","font_family":"Arial","font_size_mm":6,"layout":{{"mode":"point","align":"left","line_height_mm":null,"letter_spacing_mm":0}},"text_path":{{"path_id":"guide","start_offset_mm":30,"baseline_offset_mm":0,"anchor":"start"}}}}}},{{"id":"edit-target","page_id":"page-1","parent_id":null,"x_mm":20,"y_mm":20,"width_mm":4,"height_mm":4,"transform":[1,0,0,1,0,0],"style":{{"fill":"#000000","stroke":"none","stroke_width_mm":0.1,"opacity":1}},"visible":true,"locked":false,"clip":null,"geometry":{{"kind":"rect","radius_mm":0}}}}]}}"##,-k*80.)).unwrap()
    }
    #[test]fn writes_matching_real_frames_and_no_recovery_payload(){
        for modern in[false,true]{for k in[0.,0.3249196962329004,-0.3249196962329004]{
            let doc=source(k);let output=encode(&doc,"content",modern).unwrap().0;let native=read_native(&output).unwrap();
            assert!(native.members.keys().all(|k|!k.ends_with("metadata.xml")));
            let node=all(&native.nodes).into_iter().find(|n|n.kind()==Some(b"obj ")&&super::super::write::arguments(&n.find(b"loda").unwrap().data).unwrap().1.iter().find(|(key,_)|*key==1000).is_some_and(|(_,v)|read_wide(v).unwrap()=="content")).unwrap();
            let frame=super::super::read::matrix(node).unwrap();assert_eq!(frame[1],-k);
            let story=&node.find(b"txsm").unwrap().data;assert_eq!(f64_at(story,if modern{57}else{52}).unwrap(),-k);
            if k==0.{super::super::decode(&output,"neutral").unwrap();}else{
                assert!(super::super::decode(&output,"reject").unwrap_err().message.contains("linear frame deformation"));
                let ordinary=if modern{super::super::encode_document21(&doc)}else{super::super::encode_document(&doc)}.unwrap().0;assert_eq!(super::super::decode(&ordinary,&doc.id).unwrap().0,doc);
            }
        }}
    }
    #[test]fn rejects_unproved_guide_origin_layout_and_linear_profiles(){
        for mode in 0..8{let mut doc=source(0.25);match mode{
            0=>doc.objects[0].geometry=Geometry::Path{d:"M20 80L180 81".into(),fill_rule:Default::default()},
            1=>doc.objects[0].geometry=Geometry::Path{d:"M20 80C30 80 40 80 180 80".into(),fill_rule:Default::default()},
            2=>doc.objects[1].transform[4]+=1e-9,3=>doc.objects[1].transform[0]+=1e-9,
            4=>doc.objects[1].style.stroke="#000000".into(),5=>doc.objects[1].x_mm=1.,
            6=>if let Geometry::Text{text_path:Some(p),..}=&mut doc.objects[1].geometry{p.anchor=TextAnchor::Middle;},
            _=>if let Geometry::Text{text_path:Some(p),..}=&mut doc.objects[1].geometry{p.start_offset_mm=200.;},
        }assert!(carrier(&doc,"content").is_err(),"mode {mode}");}
    }
}
