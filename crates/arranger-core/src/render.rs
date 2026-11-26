use crate::{Clip, Document, Error, Geometry, ImageFit, Object, Result, command::{ExportFormat, Notice}};
use std::path::Path;
pub use resvg::tiny_skia::Pixmap;

fn xml(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

#[cfg(test)]mod viewport_projection_tests{
    use super::*;
    #[test]fn prepared_scene_matches_original_pixels_across_cameras_and_pages(){
        let source=std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/assets/cdr/all-content.arranger");
        let doc=crate::native::open(&source.canonicalize().unwrap()).unwrap().document;let hash=doc.hash();let renderer=Renderer::new();
        for page in &doc.pages{
            let prepared=renderer.prepare_viewport(&doc,&page.id).unwrap();assert!(prepared.input_bytes()>0);
            for(region,density)in[([-10.,-5.,240.,320.],1.),([40.25,18.5,60.5,80.75],2.5),([-63.87,-18.005,321.742,383.91],1.73),([54.001,21.123,15.9673,12.334],13.37),([321.912,401.99,41.33,51.19],2.1)]{
                let legacy=workspace_svg(&doc,&page.id,region).unwrap();
                let tree=resvg::usvg::Tree::from_str(&legacy,&renderer.options).unwrap();let mut expected=Pixmap::new((region[2]*density).round()as u32,(region[3]*density).round()as u32).unwrap();
                let transform=resvg::tiny_skia::Transform::from_row((density*region[2]/f64::from(tree.size().width()))as f32,0.,0.,(density*region[3]/f64::from(tree.size().height()))as f32,0.,0.);resvg::render(&tree,transform,&mut expected.as_mut());
                let actual=renderer.render_projection(&prepared,density,region).unwrap();assert!(actual.pixels.data()==expected.data(),"page {} region {region:?} density {density}",page.id);
            }
        }assert_eq!(doc.hash(),hash);
    }
    #[test]fn prepared_projection_keeps_viewport_validation_and_size_limits(){
        let doc=Document::new("projection".into(),210.,297.).unwrap();let renderer=Renderer::new();let prepared=renderer.prepare_viewport(&doc,"page-1").unwrap();
        assert_eq!(renderer.render_projection(&prepared,0.,[0.,0.,210.,297.]).err().unwrap().code,"INVALID_VIEWPORT");
        assert_eq!(renderer.render_projection(&prepared,1.,[f64::NAN,0.,210.,297.]).err().unwrap().code,"INVALID_VIEWPORT");
        assert_eq!(renderer.render_projection(&prepared,100.,[0.,0.,210.,297.]).err().unwrap().code,"RENDER_SIZE_LIMIT");
        assert_eq!(renderer.prepare_viewport(&doc,"missing").err().unwrap().code,"PAGE_NOT_FOUND");
    }
}
type ImageSizes = std::collections::BTreeMap<String,(u32,u32)>;
fn scene_objects(sizes: &ImageSizes, doc: &Document, page: &str, workspace: bool) -> Result<String> {
    let children = doc.children_by_parent();
    let mut body = String::new();
    for root in doc.objects.iter().filter(|o| o.parent_id.is_none() && o.visible && (o.page_id==page || workspace && o.on_canvas(page))) {
        body.push_str(&object(sizes, doc, root, &children)?);
    }
    Ok(body)
}
fn paint(name:&str,color:&str)->Result<String> {
    let parsed=crate::paint::parse(color)?;
    if let crate::paint::Paint::RgbaFloat{rgb:[r,g,b],alpha}=parsed {return Ok(format!("{name}=\"#{r:02x}{g:02x}{b:02x}\" {name}-opacity=\"{alpha}\""));}
    if let value@crate::paint::Paint::Cmyk{..}=parsed {let [r,g,b,_]=value.rgba();return Ok(format!("{name}=\"#{r:02x}{g:02x}{b:02x}\" {name}-opacity=\"1\""));}
    if color.len()==9 && color.starts_with('#') {
        let alpha=u8::from_str_radix(&color[7..],16).map_err(|_|Error::new("INVALID_STYLE","Invalid paint alpha"))?;
        Ok(format!("{name}=\"{}\" {name}-opacity=\"{}\"",xml(&color[..7]),f64::from(alpha)/255.))
    } else {Ok(format!("{name}=\"{}\" {name}-opacity=\"1\"",xml(color)))}
}
fn clipped(id:&str,clip:&Clip,body:String)->Result<String> {
    let hidden=if clip.empty()? {" display=\"none\""} else {""};
    Ok(format!("<defs><clipPath id=\"{id}\" clipPathUnits=\"userSpaceOnUse\">{}</clipPath></defs><g clip-path=\"url(#{id})\"{hidden}>{body}</g>",clip.svg()))
}
fn object(sizes:&ImageSizes,doc: &Document, o: &Object, children: &crate::model::Children<'_>) -> Result<String> {
    let [a, b, c, d, e, f] = o.transform;
    let fixed= !o.style.stroke_scale_with_object&&matches!(o.geometry,Geometry::Rect{..}|Geometry::Ellipse|Geometry::Path{..});
    let out = format!("<g id=\"object-{}\" data-arranger-id=\"{}\" transform=\"matrix({a} {b} {c} {d} {e} {f}) translate({} {})\" {} {} stroke-width=\"{}\" opacity=\"{}\" stroke-linecap=\"{}\" stroke-linejoin=\"{}\" stroke-miterlimit=\"{}\">",
        xml(&o.id), xml(&o.id), o.x_mm, o.y_mm, paint("fill",&o.style.fill)?, paint("stroke",if fixed{"none"}else{&o.style.stroke})?, o.style.stroke_width_mm, o.style.opacity,
        o.style.stroke_linecap.svg(),o.style.stroke_linejoin.svg(),o.style.stroke_miterlimit);
    let mut body = match &o.geometry {
        Geometry::Rect { radius_mm } => format!("<rect width=\"{}\" height=\"{}\" rx=\"{radius_mm}\"/>", o.width_mm, o.height_mm),
        Geometry::Ellipse => format!("<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"/>", o.width_mm/2., o.height_mm/2., o.width_mm/2., o.height_mm/2.),
        Geometry::Path { d, fill_rule } => format!("<path d=\"{}\" fill-rule=\"{}\"/>", xml(d), fill_rule.svg_name()),
        Geometry::Text { .. } => crate::text::projection(doc, o, &format!("text-{}",o.id))?,
        Geometry::Image { resource_id, clip_mm, fit } => {
            let (width,height)=sizes[resource_id];
            let (px,py)=(f64::from(width),f64::from(height));
            let (mut sx,mut sy)=(o.width_mm/px,o.height_mm/py);
            if *fit!=ImageFit::Stretch {let uniform=if *fit==ImageFit::Cover {sx.max(sy)} else {sx.min(sy)};sx=uniform;sy=uniform;}
            let (x,y)=((o.width_mm-px*sx)/2.,(o.height_mm-py*sy)/2.);
            let mut image=format!("<use href=\"#resource-{}\" transform=\"matrix({sx} 0 0 {sy} {x} {y})\"/>",xml(resource_id));
            if *fit==ImageFit::Cover {image=clipped(&format!("cover-clip-{}",o.id),&Clip::Rect{rect_mm:[0.,0.,o.width_mm,o.height_mm]},image)?;}
            if let Some([x, y, w, h]) = clip_mm {
                image=clipped(&format!("legacy-clip-{}",o.id),&Clip::Rect{rect_mm:[*x,*y,*w,*h]},image)?;
            }
            image
        }
        Geometry::Group => {
            let mut body = String::new();
            for &child in children.get(o.id.as_str()).into_iter().flatten() {
                if child.visible && child.page_id==o.page_id { body.push_str(&object(sizes, doc, child, children)?); }
            }
            body
        },
    };
    if fixed&&o.style.stroke!="none"&&o.style.stroke_width_mm>0.{
        let pen=o.style.page_pen_basis();let inverse=(doc.world_transform(o)?.inverse()*pen).as_coeffs();let path=pen.inverse()*doc.page_vector_path(o)?;
        body+=&format!("<path d=\"{}\" transform=\"matrix({} {} {} {} {} {})\" fill=\"none\" {} stroke-width=\"{}\"/>",xml(&path.to_svg()),inverse[0],inverse[1],inverse[2],inverse[3],inverse[4],inverse[5],paint("stroke",&o.style.stroke)?,o.style.stroke_width_mm);
    }
    if let Some(clip)=&o.clip {body=clipped(&format!("object-clip-{}",o.id),clip,body)?;}
    Ok(out + &body + "</g>")
}

fn definitions(doc:&Document,page_id:&str,workspace:bool)->Result<(String,ImageSizes)> {
    let used:std::collections::BTreeSet<_>=doc.objects.iter().filter(|o|o.page_id==page_id||workspace&&o.page_id==crate::WORKSPACE_PAGE).filter_map(|o|match &o.geometry {Geometry::Image{resource_id,..}=>Some(resource_id),_=>None}).collect();
    let mut definitions=String::new();let mut sizes=ImageSizes::new();
    for id in used {
        let resource=doc.resources.get(id).ok_or_else(||Error::new("RESOURCE_NOT_FOUND",id))?;let info=crate::resource::validate(resource)?;
        sizes.insert(id.clone(),(info.width_px,info.height_px));
        definitions+=&format!("<image id=\"resource-{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\" href=\"data:{};base64,{}\"/>",xml(id),info.width_px,info.height_px,resource.mime,resource.data_base64);
    }
    Ok((definitions,sizes))
}
pub fn svg(doc: &Document, page_id: &str) -> Result<String> {
    let p = doc.page(page_id)?;let (definitions,sizes)=definitions(doc,page_id,false)?;
    Ok(format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}mm\" height=\"{}mm\" viewBox=\"0 0 {} {}\" overflow=\"hidden\"><defs>{definitions}</defs><rect width=\"100%\" height=\"100%\" fill=\"white\"/>{}</svg>",
        p.width_mm, p.height_mm, p.width_mm, p.height_mm, scene_objects(&sizes,doc,page_id,false)?))
}
#[cfg(test)]
fn workspace_svg(doc:&Document,page_id:&str,region:[f64;4])->Result<String> {
    let p=doc.page(page_id)?;let [x,y,w,h]=region;let (definitions,sizes)=definitions(doc,page_id,true)?;
    let body=scene_objects(&sizes,doc,page_id,true)?;
    Ok(format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}mm\" height=\"{h}mm\" viewBox=\"0 0 {w} {h}\"><defs>{definitions}</defs><rect width=\"100%\" height=\"100%\" fill=\"#edeff3\"/><g transform=\"translate({} {})\"><rect width=\"{}\" height=\"{}\" fill=\"white\"/>{body}</g></svg>",-x,-y,p.width_mm,p.height_mm))
}

fn viewport_dimensions(pixels_per_mm:f64,region_mm:[f64;4])->Result<(u32,u32)>{
        let [x, y, w, h] = region_mm;
        if ![x,y,w,h,pixels_per_mm].iter().all(|v|v.is_finite()) || w<=0. || h<=0. || pixels_per_mm<=0. {
            return Err(Error::new("INVALID_VIEWPORT", "Viewport and pixel density must be finite and positive"));
        }
        let width=(w*pixels_per_mm).round().max(1.);
        let height=(h*pixels_per_mm).round().max(1.);
        if width*height>64_000_000. || width>u32::MAX as f64 || height>u32::MAX as f64 {
            return Err(Error::new("RENDER_SIZE_LIMIT", "Visible viewport exceeds 64 million pixels"));
        }
        Ok((width as u32,height as u32))
}
pub struct Rendered {
    pub pixels: resvg::tiny_skia::Pixmap,
    pub warnings: Vec<Notice>,
}
/// Immutable vector projection for one document revision and active page.
/// Page and pasteboard content share one parsed scene. Camera coordinates stay outside it.
pub struct ViewportProjection {tree:resvg::usvg::Tree,input_bytes:usize,warnings:Vec<Notice>}
impl ViewportProjection {
    pub fn input_bytes(&self)->usize{self.input_bytes}
}
pub struct Renderer { options: resvg::usvg::Options<'static> }
impl Default for Renderer { fn default() -> Self { Self::new() } }
impl Renderer {
    pub fn new() -> Self {
        let mut options = resvg::usvg::Options::default();
        options.fontdb = crate::text::font_database();
        Self { options }
    }
    pub fn warnings(&self, doc: &Document) -> Vec<Notice> {
        let mut notices=crate::text::warnings(doc);notices.extend(crate::paint::warnings(doc));notices.extend(crate::resource::warnings(doc));notices
    }
    /// Render the visible workspace: selected page plus the shared pasteboard,
    /// without clipping editable content at the paper. Exports remain page-only.
    pub fn render_viewport(&self, doc: &Document, page_id: &str, pixels_per_mm: f64, region_mm: [f64; 4]) -> Result<Rendered> {
        viewport_dimensions(pixels_per_mm,region_mm)?;
        self.render_projection(&self.prepare_viewport(doc,page_id)?,pixels_per_mm,region_mm)
    }
    pub fn prepare_viewport(&self,doc:&Document,page_id:&str)->Result<ViewportProjection>{
        let p=doc.page(page_id)?;let (definitions,sizes)=definitions(doc,page_id,true)?;
        let body=scene_objects(&sizes,doc,page_id,true)?;
        let scene=format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"><defs>{definitions}</defs><rect width="{}" height="{}" fill="white"/>{body}</svg>"#,p.width_mm,p.height_mm);
        let tree=resvg::usvg::Tree::from_str(&scene,&self.options).map_err(|e|Error::new("RENDER_ERROR",e.to_string()))?;
        Ok(ViewportProjection{tree,input_bytes:scene.len(),warnings:self.warnings(doc)})
    }
    pub fn render_projection(&self,projection:&ViewportProjection,pixels_per_mm:f64,region_mm:[f64;4])->Result<Rendered>{
        let (width,height)=viewport_dimensions(pixels_per_mm,region_mm)?;
        let [x,y,w,h]=region_mm;
        let camera=format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}mm" height="{h}mm" viewBox="0 0 {w} {h}"><rect id="background" width="100%" height="100%" fill="#edeff3"/></svg>"##);
        let tree=resvg::usvg::Tree::from_str(&camera,&self.options).map_err(|e|Error::new("RENDER_ERROR",e.to_string()))?;
        let mut pixels=Pixmap::new(width as u32,height as u32).ok_or_else(||Error::new("RENDER_ERROR","Pixel allocation failed"))?;
        let transform=resvg::tiny_skia::Transform::from_row(
            (pixels_per_mm*w/f64::from(tree.size().width())) as f32,0.,0.,
            (pixels_per_mm*h/f64::from(tree.size().height())) as f32,0.,0.);
        let background=tree.node_by_id("background").ok_or_else(||Error::new("RENDER_ERROR","Invalid viewport background"))?;
        let root=background.abs_transform();
        // A solid pasteboard covering the entire buffer needs no path rasterization.
        // Retain SVG antialiasing where a fractional camera leaves an edge in the buffer.
        let bounds=background.bounding_box().transform(transform.pre_concat(root));
        if bounds.is_some_and(|b| b.left()<=0. && b.top()<=0. && b.right()>=width as f32 && b.bottom()>=height as f32) {
            pixels.fill(resvg::tiny_skia::Color::from_rgba8(237,239,243,255));
        } else {
            resvg::render(&tree,transform,&mut pixels.as_mut());
        }
        resvg::render(&projection.tree,transform.pre_concat(root).pre_translate(-x as f32,-y as f32),&mut pixels.as_mut());
        Ok(Rendered{pixels,warnings:projection.warnings.clone()})
    }
    pub fn render(&self, doc: &Document, page_id: &str, dpi: f64, max_side: Option<u32>) -> Result<Rendered> {
        if !dpi.is_finite() || !(1. ..=2400.).contains(&dpi) { return Err(Error::new("INVALID_DPI", "DPI must be finite and between 1 and 2400")); }
        let p = doc.page(page_id)?;
        let mut scale = dpi / 25.4;
        if let Some(max) = max_side { scale = scale.min(f64::from(max) / p.width_mm.max(p.height_mm)); }
        let width = (p.width_mm * scale).round().max(1.);
        let height = (p.height_mm * scale).round().max(1.);
        if !width.is_finite() || !height.is_finite() || width * height > 64_000_000. {
            return Err(Error::new("RENDER_SIZE_LIMIT", "PNG is larger than the current 64 million pixel limit; lower DPI"));
        }
        let tree = resvg::usvg::Tree::from_str(&svg(doc, page_id)?, &self.options).map_err(|e| Error::new("RENDER_ERROR", e.to_string()))?;
        let mut pixels = resvg::tiny_skia::Pixmap::new(width as u32, height as u32).ok_or_else(|| Error::new("RENDER_ERROR", "Pixel allocation failed"))?;
        let transform = resvg::tiny_skia::Transform::from_scale(width as f32 / tree.size().width(), height as f32 / tree.size().height());
        resvg::render(&tree, transform, &mut pixels.as_mut());
        Ok(Rendered { pixels, warnings: self.warnings(doc) })
    }
    pub fn export(&self, doc: &Document, page_id: &str, format: ExportFormat, dpi: f64, path: &Path, overwrite: bool) -> Result<(std::path::PathBuf, Vec<Notice>)> {
        if matches!(format, ExportFormat::Cdr14) {return crate::cdr::export(doc,page_id,path,overwrite);}
        if matches!(format, ExportFormat::Cdr21) {return crate::cdr::export21(doc,page_id,path,overwrite);}
        let extension = match format { ExportFormat::Svg => "svg", ExportFormat::Png => "png", ExportFormat::Pdf => "pdf", ExportFormat::Cdr14|ExportFormat::Cdr21 => unreachable!() };
        if path.extension().and_then(|s| s.to_str()).is_none_or(|s| !s.eq_ignore_ascii_case(extension)) {
            return Err(Error::new("INVALID_PATH", format!("{extension} export requires a .{extension} path")));
        }
        let (data, warnings) = match format {
            ExportFormat::Svg => (svg(doc, page_id)?.into_bytes(), self.warnings(doc)),
            ExportFormat::Png => {
                let rendered = self.render(doc, page_id, dpi, None)?;
                (rendered.pixels.encode_png().map_err(|e| Error::new("ENCODING_ERROR", e.to_string()))?, rendered.warnings)
            }
            ExportFormat::Pdf => crate::pdf::export(self,doc,page_id,dpi)?,
            ExportFormat::Cdr14|ExportFormat::Cdr21 => unreachable!(),
        };
        Ok((crate::native::write_export(path, &data, overwrite)?, warnings))
    }
}
