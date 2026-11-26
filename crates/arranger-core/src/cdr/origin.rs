//! Corel21's paper origin is snapped to the document pixel grid. The four
//! millionths-of-pixels/mm fields below are bound to 18 real14/21 controls
//! (254/300/600 DPI, three paper sizes); legacy14 uses the geometric centre.
use super::{binary::*,invalid,unsupported,Result};

pub(super) fn paper(config:&[u8],size:[f64;2],modern:bool)->Result<[f64;2]> {
    if !modern {return Ok([size[0]/2.,size[1]/2.]);}
    let scale=[u32_at(config,716)?,u32_at(config,720)?];
    if scale[0]==0||scale[0]!=scale[1]||scale!=[u32_at(config,736)?,u32_at(config,740)?]
        ||[712,748].into_iter().any(|at|u32_at(config,at).ok()!=Some(0))
        ||[724,728,732,744].into_iter().any(|at|u32_at(config,at).ok()!=Some(1)) {
        return Err(unsupported("native21 document pixel-grid configuration is outside measured isotropic profiles"));
    }
    let mut origin=[0.;2];
    for i in 0..2 {
        let units=coord(size[i])?;if units<=0{return Err(invalid("nonpositive native paper dimension"));}
        // Integer ceiling avoids a spurious extra pixel at exact boundaries.
        let pixels=(units as u128*scale[i] as u128).div_ceil(20_000_000_000);
        let numerator=pixels*10_000_000_000;let denominator=scale[i] as u128;
        let q=numerator/denominator;let r=numerator%denominator;
        let rounded=q+u128::from(r*2>denominator||r*2==denominator&&q%2==1);
        if rounded>i32::MAX as u128{return Err(unsupported("native21 pixel-grid origin exceeds coordinate range"));}
        let snapped=rounded as f64/10000.;origin[i]=if i==0 {snapped}else{size[i]-snapped};
    }Ok(origin)
}

/// Rebase the common14 geometry after21 story conversion. This changes the
/// native coordinate reference, never the model's page-relative positions.
pub(super) fn upgrade(nodes:&mut [Node],config:&[u8],default:[f64;2])->Result<()> {
    let mut keys=std::collections::BTreeMap::new();
    let (_,scenes,_)=super::pages::decode(nodes,default)?;let mut links=super::clipping::Links::new(nodes)?;
    for scene in scenes {
        let origin=paper(config,scene.size,true)?;let delta=[coord(origin[0])? as f64-scene.size[0]*5000.,coord(origin[1])? as f64-scene.size[1]*5000.];let mut objects=vec![];
        links.scene(scene.node.children.as_deref().unwrap_or_default(),None,&mut objects,0)?;
        for (object,_) in objects {if keys.insert(super::clipping::key(object)?,delta).is_some(){return Err(invalid("ambiguous native coordinate owner"));}}
    }links.finish()?;
    for wrapper in all(nodes).into_iter().filter(|n|n.kind()==Some(b"grp ")) {
        if let Some(frame)=super::clipping::args(wrapper)?.get(&3500) {let delta=*keys.get(&u32_at(frame,0)?).ok_or_else(||invalid("native clip coordinate owner absent"))?;keys.insert(super::clipping::key(wrapper)?,delta);}
    }
    fn shift(node:&mut Node,delta:[f64;2])->Result<()> {
        if node.kind()==Some(b"obj ") {
            let tr=&mut node.find_mut(b"trfd")?.data;
            let at=u32_at(tr,u32_at(tr,8)? as usize)? as usize+16;
            for (i,v) in [(2,-delta[0]),(5,delta[1])] {
                let value=f64_at(tr,at+i*8)?+v;tr[at+i*8..at+(i+1)*8].copy_from_slice(&value.to_le_bytes());
            }
            if let Some(tx)=super::read::optional_own_record(node,b"txsm")? {
                let mut data=tx.data.clone();
                for (i,v) in [(2,-delta[0]),(5,delta[1])] {
                    let at=49+i*8;let value=f64_at(&data,at)?+v;data[at..at+8].copy_from_slice(&value.to_le_bytes());
                }node.find_mut(b"txsm")?.data=data;
            }
        }
        if matches!(&node.tag,b"bbox"|b"obbx") {
            if ![16,32].contains(&node.data.len()){return Err(invalid("native bounds length differs"));}
            for at in (0..node.data.len()).step_by(8) {
                let x=coord((i32_at(&node.data,at)? as f64-delta[0])/10000.)?;
                let y=coord((i32_at(&node.data,at+4)? as f64+delta[1])/10000.)?;
                set32(&mut node.data,at,x as u32)?;set32(&mut node.data,at+4,y as u32)?;
            }
        }
        Ok(())
    }
    fn visit(nodes:&mut [Node],config:&[u8],default:[f64;2],keys:&std::collections::BTreeMap<u32,[f64;2]>,inherited:Option<[f64;2]>)->Result<()> {
        for node in nodes {
            let delta=if node.kind()==Some(b"page") {
                let size=if super::read::flags(node)?&0x10000!=0 {default}else{super::pages::props(node,default)?.1};
                let origin=paper(config,size,true)?;Some([coord(origin[0])? as f64-size[0]*5000.,coord(origin[1])? as f64-size[1]*5000.])
            }else if matches!(node.kind(),Some(b"obj "|b"grp ")) {Some(*keys.get(&super::clipping::key(node)?).ok_or_else(||invalid("native coordinate owner absent"))?)}else{inherited};
            if let Some(d)=delta {shift(node,d)?;}
            if let Some(children)=&mut node.children {visit(children,config,default,keys,delta)?;}
        }Ok(())
    }visit(nodes,config,default,&keys,None)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_dpi_fields_bind_all_eighteen_controls_without_constant_offsets() {
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports/windows/WIN-53-20261025-01/page-coordinate-controls");
        for version in [14,21] {for size in [[210.,297.],[297.,210.],[60.,14.]] {for dpi in [254u32,300,600] {
            let path=root.join(format!("page-{}x{}-dpi{dpi}-v{version}.cdr",size[0],size[1]));
            let native=read_native(&std::fs::read(path).unwrap()).unwrap();
            let config=&all(&native.nodes).into_iter().find(|n|n.tag==*b"mcfg").unwrap().data;
            assert_eq!(u32_at(config,716).unwrap(),(dpi as f64/25.4*1e6).round_ties_even() as u32);
            let origin=paper(config,size,version==21).unwrap();
            for i in 0..2 {let expected=if version==14 {size[i]/2.}else{((size[i]/2.*dpi as f64/25.4-1e-10).ceil()*25.4/dpi as f64*10000.).round_ties_even()/10000.};assert!((origin[i]-if version==21&&i==1 {size[i]-expected}else{expected}).abs()<1e-10);}
            let mut bad=config.clone();set32(&mut bad,720,0).unwrap();assert!(paper(&bad,size,true).is_err());
        }}}
    }
}
