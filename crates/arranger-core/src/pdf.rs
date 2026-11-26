//! Page PDF output uses Skia's codec. Geometry and glyph outlines come from
//! the same model/kurbo/usvg layout used by measurement and SVG/PNG.
use crate::{Clip,Document,Error,FillRule,Geometry,ImageFit,LineCap,LineJoin,Result,Style,command::Notice};
use base64::{Engine,engine::general_purpose::STANDARD};
use kurbo::{BezPath,PathEl};
use skia_safe::{self as sk,Canvas,Matrix,Paint,PathBuilder,Rect};

fn matrix([a,b,c,d,e,f]:[f64;6])->Matrix {
    Matrix::new_all(a as f32,c as f32,e as f32,b as f32,d as f32,f as f32,0.,0.,1.)
}
fn path(source:&BezPath,rule:FillRule)->sk::Path {
    let mut b=PathBuilder::new();
    b.set_fill_type(if rule==FillRule::EvenOdd {sk::PathFillType::EvenOdd}else{sk::PathFillType::Winding});
    for el in source.elements() {match el {
        PathEl::MoveTo(p)=>{b.move_to((p.x as f32,p.y as f32));},
        PathEl::LineTo(p)=>{b.line_to((p.x as f32,p.y as f32));},
        PathEl::QuadTo(a,bp)=>{b.quad_to((a.x as f32,a.y as f32),(bp.x as f32,bp.y as f32));},
        PathEl::CurveTo(a,bp,c)=>{b.cubic_to((a.x as f32,a.y as f32),(bp.x as f32,bp.y as f32),(c.x as f32,c.y as f32));},
        PathEl::ClosePath=>{b.close();},
    }}
    b.detach()
}
fn color(value:&str)->sk::Color4f {
    let paint=crate::paint::parse(value).expect("validated paint");let [r,g,b,_]=paint.rgba();sk::Color4f::new(f32::from(r)/255.,f32::from(g)/255.,f32::from(b)/255.,paint.alpha() as f32)
}
fn draw(canvas:&Canvas,geometry:&BezPath,rule:FillRule,style:&Style) {
    let geometry=path(geometry,rule);
    let mut paint=Paint::default();paint.set_anti_alias(true);
    if style.fill!="none" {paint.set_color4f(color(&style.fill),None);canvas.draw_path(&geometry,&paint);}
    if style.stroke!="none" && style.stroke_width_mm>0. {
        paint.set_style(sk::paint::Style::Stroke).set_color4f(color(&style.stroke),None).set_stroke_width(style.stroke_width_mm as f32)
            .set_stroke_miter(style.stroke_miterlimit as f32)
            .set_stroke_cap(match style.stroke_linecap {LineCap::Butt=>sk::paint::Cap::Butt,LineCap::Round=>sk::paint::Cap::Round,LineCap::Square=>sk::paint::Cap::Square})
            .set_stroke_join(match style.stroke_linejoin {LineJoin::Miter=>sk::paint::Join::Miter,LineJoin::Round=>sk::paint::Join::Round,LineJoin::Bevel=>sk::paint::Join::Bevel});
        canvas.draw_path(&geometry,&paint);
    }
}
fn draw_vector(canvas:&Canvas,doc:&Document,object:&crate::Object,page_matrix:&Matrix)->Result<()>{
    let geometry=object.local_path()?;
    if object.style.stroke_scale_with_object{return Ok(draw(canvas,&geometry,object.fill_rule(),&object.style));}
    let mut fill=object.style.clone();fill.stroke="none".into();draw(canvas,&geometry,object.fill_rule(),&fill);
    if object.style.stroke!="none"&&object.style.stroke_width_mm>0.{let mut stroke=object.style.clone();stroke.fill="none".into();canvas.save();canvas.reset_matrix();canvas.concat(page_matrix);let pen=object.style.page_pen_basis();canvas.concat(&matrix(pen.as_coeffs()));draw(canvas,&(pen.inverse()*doc.page_vector_path(object)?),object.fill_rule(),&stroke);canvas.restore();}Ok(())
}
fn clip(canvas:&Canvas,clip:&Clip)->Result<()> {
    canvas.clip_path(&path(&clip.path()?,clip.rule()),sk::ClipOp::Intersect,true);Ok(())
}
fn children(canvas:&Canvas,doc:&Document,page:&str,parent:Option<&str>,images:&mut std::collections::BTreeMap<String,sk::Image>,page_matrix:&Matrix)->Result<()> {
    for object in doc.objects.iter().filter(|o|o.page_id==page && o.parent_id.as_deref()==parent && o.visible && o.style.opacity>0.) {
        canvas.save();
        canvas.concat(&matrix((kurbo::Affine::new(object.transform)*kurbo::Affine::translate((object.x_mm,object.y_mm))).as_coeffs()));
        if let Some(region)=&object.clip {clip(canvas,region)?;}
        let layered=object.style.opacity<1.;
        if layered {canvas.save_layer_alpha_f(None,object.style.opacity as f32);}
        match &object.geometry {
            Geometry::Group=>children(canvas,doc,page,Some(&object.id),images,page_matrix)?,
            Geometry::Text{..}=>{
                let layout=crate::text::layout(doc,object)?;
                if !layout.outline_supported {return Err(Error::new("PDF_RASTER_REQUIRED","Color glyphs require raster page fallback"));}
                draw(canvas,&layout.outline,FillRule::NonZero,&object.style);
            },
            Geometry::Image{resource_id,clip_mm,fit}=>{
                if *fit==ImageFit::Cover {clip(canvas,&Clip::Rect{rect_mm:[0.,0.,object.width_mm,object.height_mm]})?;}
                if let Some(r)=clip_mm {clip(canvas,&Clip::Rect{rect_mm:*r})?;}
                if !images.contains_key(resource_id) {
                    let data=STANDARD.decode(&doc.resources[resource_id].data_base64).map_err(|e|Error::new("INVALID_RESOURCE",e.to_string()))?;
                    let image=sk::Image::from_encoded(sk::Data::new_copy(&data)).ok_or_else(||Error::new("ENCODING_ERROR","Skia could not decode the embedded image for PDF"))?;
                    images.insert(resource_id.clone(),image);
                }
                let image=&images[resource_id];let (w,h)=(image.width() as f64,image.height() as f64);
                let (mut sx,mut sy)=(object.width_mm/w,object.height_mm/h);
                if *fit!=ImageFit::Stretch {let s=if *fit==ImageFit::Cover {sx.max(sy)} else {sx.min(sy)};sx=s;sy=s;}
                let (dw,dh)=(w*sx,h*sy);let (x,y)=((object.width_mm-dw)/2.,(object.height_mm-dh)/2.);
                canvas.draw_image_rect(image,None,Rect::from_xywh(x as f32,y as f32,dw as f32,dh as f32),&Paint::default());
            },
            _=>draw_vector(canvas,doc,object,page_matrix)?,
        }
        if layered {canvas.restore();}canvas.restore();
    }
    Ok(())
}
fn needs_raster(doc:&Document,page:&str,parent:Option<&str>)->Result<bool>{
    for o in doc.objects.iter().filter(|o|o.page_id==page&&o.parent_id.as_deref()==parent&&o.visible&&o.style.opacity>0.){
        match &o.geometry{
            Geometry::Group=>if needs_raster(doc,page,Some(&o.id))?{return Ok(true);},
            Geometry::Text{..}=>if !crate::text::layout(doc,o)?.outline_supported{return Ok(true);},
            _=>{},
        }
    }Ok(false)
}
pub(crate) fn export(renderer:&crate::render::Renderer,doc:&Document,page_id:&str,dpi:f64)->Result<(Vec<u8>,Vec<Notice>)>{
    export_pages(renderer,doc,&[page_id.to_owned()],dpi)
}
pub(crate) fn export_pages(renderer:&crate::render::Renderer,doc:&Document,page_ids:&[String],dpi:f64)->Result<(Vec<u8>,Vec<Notice>)>{
    if !dpi.is_finite()||!(1. ..=2400.).contains(&dpi){return Err(Error::new("INVALID_DPI","DPI must be between 1 and 2400"));}
    if page_ids.is_empty(){return Err(Error::new("INVALID_PAGES","Select at least one PDF page"));}
    let mut seen=std::collections::BTreeSet::new();let mut pages=vec![];
    for id in page_ids{
        if !seen.insert(id){return Err(Error::new("INVALID_PAGES","Duplicate PDF page IDs are not allowed"));}
        let page=doc.page(id)?;
        if page.width_mm>5080.||page.height_mm>5080.{return Err(Error::new("PDF_PAGE_RANGE","PDF pages are limited to 5080 mm per side (14400 pt); use SVG or a smaller page"));}
        pages.push((page,needs_raster(doc,id,None)?));
    }
    let mut bytes=vec![];let mut warnings=renderer.warnings(doc);let mut images=Default::default();
    // Fixed 0.001 mm device grid preserves exact page sizes. Requested DPI
    // affects only the explicit color-glyph raster fallback, page by page.
    let metadata=sk::pdf::Metadata{creator:"Arranger".into(),raster_dpi:Some(25400.),..Default::default()};
    let mut pdf=sk::pdf::new_document(&mut bytes,Some(&metadata));
    for (page,raster) in pages{
        let mut sheet=pdf.begin_page(((page.width_mm*72./25.4)as f32,(page.height_mm*72./25.4)as f32),None);
        let canvas=sheet.canvas();canvas.scale((72./25.4,72./25.4));
        canvas.clip_rect(Rect::from_wh(page.width_mm as f32,page.height_mm as f32),sk::ClipOp::Intersect,false);canvas.clear(sk::Color::WHITE);
        if raster{
            let rendered=renderer.render(doc,&page.id,dpi,None)?;
            let png=rendered.pixels.encode_png().map_err(|e|Error::new("ENCODING_ERROR",e.to_string()))?;
            let image=sk::Image::from_encoded(sk::Data::new_copy(&png)).ok_or_else(||Error::new("ENCODING_ERROR","PDF raster fallback failed"))?;
            canvas.draw_image_rect(&image,None,Rect::from_wh(page.width_mm as f32,page.height_mm as f32),&Paint::default());
            warnings.push(Notice::new("PDF_RASTERIZED",&format!("Page {} contains color glyphs and uses the shared PNG projection at the requested DPI; other pages retain vector outlines.",page.id)));
        }else{
            let page_matrix=canvas.local_to_device_as_3x3();children(canvas,doc,&page.id,None,&mut images,&page_matrix)?;
        }
        pdf=sheet.end_page();
    }
    pdf.close();Ok((bytes,warnings))
}
#[cfg(test)]
mod tests{
    #[test]
    fn decimal_paint_uses_the_float_pdf_alpha_channel(){
        let color=super::color("rgba(30,90,180,0.5)");assert_eq!(color.a,0.5);assert_ne!(color.a,128f32/255.);
        let old=super::color("#1e5ab480");assert_eq!(old.a,128f32/255.);
    }
}
