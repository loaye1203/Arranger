//! One usvg/harfrust layout supplies measurement, outlines and SVG text display.
//! Fonts are read from the OS once per process; no font files are embedded.
use crate::{Document, Error, FillRule, Geometry, Object, Result, command::Notice, model::sha256};
use kurbo::{Affine, BezPath, ParamCurveArclen, Point, Rect, Shape};
use resvg::{tiny_skia::PathSegment, usvg::{self, fontdb}};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::{BTreeMap, HashMap, VecDeque}, sync::{Arc, Mutex, OnceLock}};
use unicode_segmentation::UnicodeSegmentation;

mod caret_stops;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextMode { #[default] Point, Area }
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign { #[default] Left, Center, Right }
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextLayoutOptions {
    #[serde(default)] pub mode: TextMode,
    #[serde(default)] pub align: TextAlign,
    #[serde(default)] pub letter_spacing_mm: f64,
    /// Baseline-to-baseline distance. None means 1.2 * font_size_mm.
    #[serde(default)] pub line_height_mm: Option<f64>,
}
impl TextLayoutOptions {
    pub fn is_default(&self) -> bool { *self == Self::default() }
}

#[derive(Clone, Debug, Serialize)]
pub struct LineInfo {
    pub index: usize,
    /// UTF-8 byte range in the original content, excluding hard separators.
    pub source_range_bytes: [usize; 2],
    pub text: String,
    pub break_after: &'static str,
    /// None for curved text, which has no single straight baseline origin.
    pub baseline_origin_mm: Option<[f64; 2]>,
    pub advance_mm: f64,
    pub glyph_range: [usize; 2],
    pub ink_bounds_local_mm: Option<[f64; 4]>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAnchor { #[default] Start, Middle, End }
impl TextAnchor {
    fn svg(self) -> &'static str { match self { Self::Start => "start", Self::Middle => "middle", Self::End => "end" } }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextOnPath {
    pub path_id: String,
    #[serde(default)] pub start_offset_mm: f64,
    #[serde(default)] pub baseline_offset_mm: f64,
    #[serde(default)] pub anchor: TextAnchor,
}

#[derive(Clone, Debug, Serialize)]
pub struct FontInfo {
    pub key: String,
    pub families: Vec<String>,
    pub post_script_name: String,
    pub face_index: u32,
    pub sha256: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct GlyphInfo {
    pub glyph_id: u32,
    pub cluster_text: String,
    pub font_key: String,
    /// Font outline units (y up) to object-local millimetres (y down).
    pub outline_transform: [f64; 6],
    pub origin_mm: [f64; 2],
    pub angle_degrees: f64,
}
pub struct TextLayout {
    pub outline: BezPath,
    pub outline_supported: bool,
    pub ink_bounds: Option<Rect>,
    pub advance_mm: f64,
    pub glyphs: Vec<GlyphInfo>,
    pub shaped_glyph_count: usize,
    pub fonts: Vec<FontInfo>,
    pub warnings: Vec<Notice>,
    pub overflow: bool,
    pub missing_glyphs: bool,
    pub path_length_mm: Option<f64>,
    pub lines: Vec<LineInfo>,
    pub line_height_mm: f64,
    pub layout_bounds: Rect,
    pub area_overflow: bool,
}

pub fn font_database() -> Arc<fontdb::Database> {
    static FONTS: OnceLock<Arc<fontdb::Database>> = OnceLock::new();
    FONTS.get_or_init(|| { let mut db = fontdb::Database::new(); db.load_system_fonts(); Arc::new(db) }).clone()
}
fn options() -> &'static usvg::Options<'static> {
    static OPTIONS: OnceLock<usvg::Options> = OnceLock::new();
    OPTIONS.get_or_init(|| {
        // These options always use the same process font database and the
        // stock selector. The order of excluded faces is significant.
        thread_local! {
            static FALLBACK: RefCell<HashMap<(char, Vec<fontdb::ID>), fontdb::ID>> = RefCell::default();
        }
        let mut options = usvg::Options { fontdb: font_database(), ..Default::default() };
        let select = options.font_resolver.select_fallback;
        let system_db = options.fontdb.clone();
        options.font_resolver.select_fallback = Box::new(move |ch, excluded, db| {
            if excluded.len() > 16 || !Arc::ptr_eq(db, &system_db) { return select(ch, excluded, db); }
            let key = (ch, excluded.to_vec());
            FALLBACK.with(|cell| {
                if let Some(id) = cell.borrow().get(&key) { return Some(*id); }
                let id = select(ch, excluded, db)?;
                let mut cache = cell.borrow_mut();
                if cache.len() >= 1024 { cache.clear(); }
                cache.insert(key, id);
                Some(id)
            })
        });
        options
    })
}
fn xml(s: &str) -> String { s.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;").replace('"',"&quot;").replace('\'',"&apos;") }
fn attributes(font: &str, size: f64, spacing: f64) -> String {
    format!("font-family=\"{}\" font-size=\"{size}\" letter-spacing=\"{spacing}\" xml:space=\"preserve\"",xml(font))
}
fn svg(body: &str) -> String {
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1000\" height=\"1000\">{body}</svg>")
}
fn parse(body: &str) -> Result<usvg::Tree> {
    usvg::Tree::from_str(&svg(body),options()).map_err(|e| Error::new("TEXT_LAYOUT_FAILED",e.to_string()))
}
fn node(tree: &usvg::Tree) -> Option<&usvg::Text> {
    match tree.node_by_id("layout-text") { Some(usvg::Node::Text(text)) => Some(text), _ => None }
}
fn affine(t: usvg::Transform) -> Affine { Affine::new([t.sx as f64,t.ky as f64,t.kx as f64,t.sy as f64,t.tx as f64,t.ty as f64]) }

pub(crate) fn validate_text(content: &str, family: &str, size: f64) -> Result<()> {
    if family.trim().is_empty() || !size.is_finite() || size <= 0. {
        return Err(Error::new("INVALID_TEXT","Font family and positive finite font size are required"));
    }
    if !content.chars().all(|c| matches!(c,'\t'|'\n'|'\r') || c >= '\u{20}' && c != '\u{fffe}' && c != '\u{ffff}') {
        return Err(Error::new("INVALID_TEXT","Text contains a character not representable in XML 1.0"));
    }
    if !(size as f32).is_finite() || size as f32 == 0. { return Err(Error::new("TEXT_RANGE","Font size exceeds the text backend's range")); }
    Ok(())
}

pub(crate) fn validate_options(object: &Object) -> Result<()> {
    let Geometry::Text {content, font_size_mm, text_path, layout, ..} = &object.geometry else {return Ok(());};
    let height=layout.line_height_mm.unwrap_or(font_size_mm*1.2);
    if !layout.letter_spacing_mm.is_finite() || !height.is_finite() || height <= 0. {
        return Err(Error::new("INVALID_TEXT_LAYOUT","Letter spacing must be finite and line height must be positive and finite"));
    }
    if ![layout.letter_spacing_mm,height].iter().all(|v|(*v as f32).is_finite()) || height as f32 == 0. {
        return Err(Error::new("TEXT_RANGE","Text spacing exceeds the text backend's range"));
    }
    if layout.mode==TextMode::Area && (![object.width_mm,object.height_mm].iter().all(|v|(*v as f32).is_finite() && *v as f32>0.)) {
        return Err(Error::new("TEXT_RANGE","The text area exceeds the backend's range"));
    }
    if text_path.is_some() && (layout.mode!=TextMode::Point || layout.align!=TextAlign::Left || content.contains(['\r','\n','\u{2028}','\u{2029}'])) {
        return Err(Error::new("INVALID_TEXT_LAYOUT","Text on a path requires one point-text line; use its path anchor for alignment"));
    }
    Ok(())
}

/// A guide is projected into the text parent's coordinates; the text object's
/// own matrix/x/y then acts as a normal, editable offset from that guide.
pub(crate) fn guide(doc: &Document, text: &Object, binding: &TextOnPath) -> Result<(BezPath, f64)> {
    if !binding.start_offset_mm.is_finite() || !binding.baseline_offset_mm.is_finite() {
        return Err(Error::new("INVALID_TEXT_PATH","Text path offsets must be finite millimetres"));
    }
    if ![binding.start_offset_mm,binding.baseline_offset_mm].iter().all(|v|(*v as f32).is_finite()) {
        return Err(Error::new("TEXT_RANGE","Text path offsets exceed the backend's coordinate range"));
    }
    let guide = doc.object(&binding.path_id)?;
    if !matches!(guide.geometry,Geometry::Path { .. }) { return Err(Error::new("INVALID_TEXT_PATH","The guide must be a path object; convert shapes with object_to_path first")); }
    if guide.page_id != text.page_id { return Err(Error::new("CROSS_PAGE","Text and its guide must be on the same page")); }
    let parent = match &text.parent_id { Some(id) => doc.world_transform(doc.object(id)?)?, None => Affine::IDENTITY };
    if !parent.determinant().is_finite() || parent.determinant().abs() < f64::EPSILON {
        return Err(Error::new("INVALID_TEXT_PATH","Text parent transform must be invertible to map the guide"));
    }
    let path = parent.inverse() * doc.world_transform(guide)? * guide.local_path()?;
    if !path.is_finite() || path.elements().iter().filter(|e| matches!(e,kurbo::PathEl::MoveTo(_))).count()!=1 {
        return Err(Error::new("INVALID_TEXT_PATH","The guide must contain one finite continuous subpath"));
    }
    let fits=|p:Point|(p.x as f32).is_finite() && (p.y as f32).is_finite();
    if !path.elements().iter().all(|el|match *el {
        kurbo::PathEl::MoveTo(p)|kurbo::PathEl::LineTo(p)=>fits(p),kurbo::PathEl::QuadTo(a,b)=>fits(a)&&fits(b),
        kurbo::PathEl::CurveTo(a,b,c)=>fits(a)&&fits(b)&&fits(c),kurbo::PathEl::ClosePath=>true,
    }) {return Err(Error::new("TEXT_RANGE","The projected guide exceeds the text backend's coordinate range"));}
    let length:f64 = path.segments().map(|s|s.arclen(1e-6)).sum();
    if !length.is_finite() || length<=0. { return Err(Error::new("INVALID_TEXT_PATH","The guide must have positive finite length")); }
    Ok((path,length))
}

pub(crate) fn markup(doc: &Document, object: &Object, id: &str) -> Result<String> {
    let Geometry::Text {content,font_family,font_size_mm,text_path,layout}=&object.geometry else {return Err(Error::new("NOT_TEXT","Expected a text object"));};
    let attrs=attributes(font_family,*font_size_mm,layout.letter_spacing_mm);
    if let Some(binding)=text_path {
        let (path,_)=guide(doc,object,binding)?;
        Ok(format!("<defs><path id=\"guide-{id}\" d=\"{}\"/></defs><text id=\"{id}\" {attrs} dy=\"{}\" text-anchor=\"{}\"><textPath href=\"#guide-{id}\" startOffset=\"{}\">{}</textPath></text>",path.to_svg(),binding.baseline_offset_mm,binding.anchor.svg(),binding.start_offset_mm,xml(content)))
    } else {
        Ok(format!("<text id=\"{id}\" y=\"{font_size_mm}\" {attrs}>{}</text>",xml(content)))
    }
}

pub(crate) fn projection(doc: &Document, object: &Object, id: &str) -> Result<String> {
    if let Geometry::Text {content,text_path:Some(_),..}=&object.geometry {
        let positioned=layout(doc,object)?;require_complete(&positioned)?;
        if !positioned.outline_supported {return Err(Error::new("UNSUPPORTED_GLYPH_OUTLINE","Path text currently requires monochrome outline glyphs"));}
        // usvg adjusts text-path arclength accuracy with the viewport scale.
        // Freeze that derived layout once, so GUI/PNG/SVG/converted paths do
        // not move glyphs when parsed under different page or export scales.
        return Ok(format!("<title>{}</title><path id=\"{id}\" data-arranger-text-path=\"true\" fill-rule=\"nonzero\" d=\"{}\"/>",xml(content),positioned.outline.to_svg()));
    }
    if let Geometry::Text {content,font_family,font_size_mm,layout:settings,..}=&object.geometry {
        if !settings.is_default() || content.contains(['\r','\n','\u{2028}','\u{2029}']) {
            let positioned=layout(doc,object)?;
            // Freeze the paragraph's placed monochrome contours for every
            // viewport and export DPI, retaining editable text in native files.
            if positioned.outline_supported {
                return Ok(format!("<title>{}</title><path id=\"{id}\" data-arranger-text-layout=\"true\" fill-rule=\"nonzero\" d=\"{}\"/>",xml(content),positioned.outline.to_svg()));
            }
            // Preserve colored glyph paint through the same per-line shaper.
            let attrs=attributes(font_family,*font_size_mm,settings.letter_spacing_mm);
            let lines=positioned.lines.iter().map(|line| {
                let [x,y]=line.baseline_origin_mm.expect("paragraph baseline");
                format!("<text x=\"{x}\" y=\"{y}\" {attrs}>{}</text>",xml(&line.text))
            }).collect::<String>();
            return Ok(format!("<g id=\"{id}\" data-arranger-text-layout=\"true\">{lines}</g>"));
        }
    }
    markup(doc,object,id)
}

fn font_info(db: &fontdb::Database, id: fontdb::ID) -> Result<FontInfo> {
    static INFO:OnceLock<Mutex<HashMap<fontdb::ID,FontInfo>>>=OnceLock::new();
    let mut cache=INFO.get_or_init(Default::default).lock().expect("font metadata cache");
    if let Some(info)=cache.get(&id) {return Ok(info.clone());}
    let face=db.face(id).ok_or_else(|| Error::new("TEXT_LAYOUT_FAILED","A resolved font face disappeared"))?;
    let hash=db.with_face_data(id,|bytes,_|sha256(bytes)).ok_or_else(|| Error::new("TEXT_LAYOUT_FAILED","Cannot read a resolved font"))?;
    let info=FontInfo {key:format!("{hash}:{}",face.index),families:face.families.iter().map(|(f,_)|f.clone()).collect(),
        post_script_name:face.post_script_name.clone(),face_index:face.index,sha256:hash};
    cache.insert(id,info.clone());Ok(info)
}

fn glyphs(tree: &usvg::Tree, text: &usvg::Text, omit_probe: bool) -> Result<(Vec<GlyphInfo>,BTreeMap<String,FontInfo>)> {
    let mut glyphs=vec![];
    let mut fonts = BTreeMap::new();
    let mut last_face = None;
    for span in text.layouted() {
        if omit_probe && span.fill.as_ref().is_some_and(|f|matches!(f.paint(),usvg::Paint::Color(c) if c.red==1 && c.green==2 && c.blue==3)) {continue;}
        for glyph in &span.positioned_glyphs {
            if last_face.as_ref().map(|(id, _)| *id) != Some(glyph.font) {
                let font = font_info(tree.fontdb(), glyph.font)?;
                last_face = Some((glyph.font, font.key.clone()));
                fonts.insert(font.key.clone(), font);
            }
            let (_, key) = last_face.as_ref().unwrap();
            let matrix=affine(glyph.outline_transform()).as_coeffs();
            let t=glyph.transform();
            glyphs.push(GlyphInfo {glyph_id:glyph.id.0,cluster_text:glyph.text.clone(),font_key:key.clone(),outline_transform:matrix,
                origin_mm:[t.tx as f64,t.ty as f64],angle_degrees:(t.ky as f64).atan2(t.sx as f64).to_degrees()});
        }
    }
    Ok((glyphs,fonts))
}

fn collect(group: &usvg::Group, parent: Affine, path: &mut BezPath) -> bool {
    if group.should_isolate() {return false;}
    let transform=parent*affine(group.transform());
    let mut supported=true;
    for node in group.children() {
        match node {
            usvg::Node::Group(g) => supported &= collect(g,transform,path),
            usvg::Node::Path(p) => {
                let black=p.fill().is_some_and(|f| f.opacity().get()==1. && matches!(f.paint(),usvg::Paint::Color(c) if c.red==0 && c.green==0 && c.blue==0));
                if !black || p.stroke().is_some() {supported=false;continue;}
                let point=|p:resvg::tiny_skia::Point|transform * Point::new(p.x as f64,p.y as f64);
                for segment in p.data().segments() { match segment {
                    PathSegment::MoveTo(p)=>path.move_to(point(p)),PathSegment::LineTo(p)=>path.line_to(point(p)),
                    PathSegment::QuadTo(c,p)=>path.quad_to(point(c),point(p)),
                    PathSegment::CubicTo(a,b,p)=>path.curve_to(point(a),point(b),point(p)),PathSegment::Close=>path.close_path(),
                }}
            }
            _=>supported=false,
        }
    }
    supported
}

fn requested_font(family: &str) -> Option<fontdb::ID> {
    let named=match family {"serif"=>fontdb::Family::Serif,"sans-serif"=>fontdb::Family::SansSerif,"monospace"=>fontdb::Family::Monospace,_=>fontdb::Family::Name(family)};
    font_database().query(&fontdb::Query {families:&[named],..Default::default()})
}

fn natural_run(content: &str, font_family: &str, font_size_mm: f64, spacing: f64) -> Result<(usvg::Tree, bool, f64)> {
    let linear=format!("<text id=\"layout-text\" y=\"{font_size_mm}\" {}>{}</text>",attributes(font_family,font_size_mm,spacing),xml(content));
    let mut natural=parse(&linear)?;
    let mut probe=false;
    // usvg drops nodes containing no ink. A separate-font marker keeps a
    // whitespace-only run observable; its positioned origin is that run's
    // actual shaped advance. The marker is excluded from all returned glyphs.
    if node(&natural).is_none() && !content.is_empty() && content.chars().all(|c|matches!(c,' '|'\t'|'\r'|'\n')) {
        let base=requested_font(font_family).or_else(||requested_font("serif"));
        let db=font_database();
        let marker=db.faces().flat_map(|f|f.families.iter().map(|(name,_)|name)).find(|name| {
            db.query(&fontdb::Query{families:&[fontdb::Family::Name(name)],..Default::default()})!=base
        }).cloned().ok_or_else(||Error::new("TEXT_LAYOUT_FAILED","Cannot resolve a measurement marker font"))?;
        natural=parse(&format!("<text id=\"layout-text\" y=\"{font_size_mm}\" {}>{}<tspan fill=\"#010203\" font-family=\"{}\">M</tspan></text>",attributes(font_family,font_size_mm,spacing),xml(content),xml(&marker)))?;
        probe=true;
    }
    let advance = match node(&natural) {
        Some(text) if probe => text.layouted().last().and_then(|s|s.positioned_glyphs.first())
            .map(|g| f64::from(g.transform().tx) - spacing)
            .ok_or_else(||Error::new("TEXT_LAYOUT_FAILED","Whitespace measurement marker was not positioned"))?,
        Some(text) => f64::from(text.bounding_box().width()),
        None if content.is_empty() => 0.,
        None => return Err(Error::new("TEXT_LAYOUT_FAILED","No text layout was produced; check font availability and content")),
    };
    Ok((natural, probe, advance))
}

/// Exact point-text advance of one visible prefix for a caret. Preserve the
/// full layout's error checks for negative spacing and multi-line input.
fn caret_advance(doc: &Document, object: &Object, prefix: &str) -> Result<f64> {
    let Geometry::Text{font_family,font_size_mm,layout:settings,..}=&object.geometry else {
        return Err(Error::new("NOT_TEXT","Expected a text object"));
    };
    if settings.letter_spacing_mm < 0. || prefix.contains(['\r','\n','\u{2028}','\u{2029}']) {
        let mut line = object.clone();
        if let Geometry::Text{content,layout,text_path,..} = &mut line.geometry {
            *content=prefix.into(); layout.mode=TextMode::Point; layout.align=TextAlign::Left; *text_path=None;
        }
        return Ok(layout(doc,&line)?.advance_mm);
    }
    validate_text(prefix,font_family,*font_size_mm)?;
    let (_, _, advance) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(||
        natural_run(prefix,font_family,*font_size_mm,settings.letter_spacing_mm)))
        .map_err(|_|Error::new("TEXT_LAYOUT_FAILED","The text backend failed while measuring a caret"))??;
    if !advance.is_finite() {return Err(Error::new("TEXT_RANGE","Text layout produced nonfinite geometry"));}
    Ok(advance.max(0.))
}

/// Widths from the start of a visible line through each new Unicode scalar.
/// Reuse positioned glyphs at stable shaping boundaries on long LTR lines.
/// Measure remaining prefixes with their own kerning, fallback and bidi context.
pub fn caret_advances(doc: &Document, object: &Object, line: &str, from: usize) -> Result<Vec<f64>> {
    let Geometry::Text{font_family,font_size_mm,layout:settings,..}=&object.geometry else {
        return Err(Error::new("NOT_TEXT","Expected a text object"));
    };
    validate_text(line,font_family,*font_size_mm)?;
    let rest=line.get(from..).ok_or_else(||Error::new("INVALID_TEXT","Invalid caret byte boundary"))?;
    let ends: Vec<_> = rest.char_indices().map(|(at, ch)| from + at + ch.len_utf8()).collect();
    if ends.is_empty() || settings.letter_spacing_mm < 0. || line.contains(['\r','\n','\u{2028}','\u{2029}']) {
        return ends.iter().map(|&end| caret_advance(doc, object, &line[..end])).collect();
    }
    // Even a short edited tail can contain many expensive prefixes on a long
    // line. A single appended scalar still needs only one independent width.
    if ends.len() > 1 && settings.letter_spacing_mm == 0. && line.chars().nth(191).is_some() {
        let observed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(||
            caret_stops::widths(line, font_family, *font_size_mm, &ends)));
        if let Ok(Some(widths)) = observed {
            let unresolved: Vec<_> = ends.iter().zip(&widths)
                .filter_map(|(&end, width)| width.is_none().then_some(end)).collect();
            let mut measured = measure_prefixes(doc, object, line, &unresolved)?.into_iter();
            return Ok(widths.into_iter().map(|width| width.unwrap_or_else(|| measured.next().unwrap())).collect());
        }
    }
    measure_prefixes(doc, object, line, &ends)
}

fn measure_prefixes(doc: &Document, object: &Object, line: &str, ends: &[usize]) -> Result<Vec<f64>> {
    let Geometry::Text{font_family,font_size_mm,layout:settings,..}=&object.geometry else {
        return Err(Error::new("NOT_TEXT","Expected a text object"));
    };
    let attrs = attributes(font_family, *font_size_mm, settings.letter_spacing_mm);
    let measure = |ends: &[usize], batch_size| -> Result<Vec<f64>> {
        let mut widths = Vec::with_capacity(ends.len());
        for batch in ends.chunks(batch_size) {
            let tree = (batch.len() > 1).then(|| {
                prefix_tree(&attrs, *font_size_mm, batch.iter().map(|&end| &line[..end]))
            }).flatten();
            for (i, &end) in batch.iter().enumerate() {
                let width = match tree.as_ref().and_then(|t| prefix_node(t, i)) {
                    Some(text) => f64::from(text.bounding_box().width()),
                    None => caret_advance(doc, object, &line[..end])?,
                };
                widths.push(width);
            }
        }
        Ok(widths)
    };
    let batch_size = if line.len() <= 256 { 32 } else { 16 };
    // Typing one character and reflowing short rows stay on the UI thread.
    // Bound the number of simultaneous SVG trees, even on large workstations.
    if ends.len() < 192 {
        return measure(ends, batch_size);
    }
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get().min(4));
    if workers == 1 { return measure(ends, batch_size); }
    // Later prefixes cost more to shape. Split by total prefix bytes so the
    // last worker does not inherit most of the work on a fresh long line.
    // For very long prefixes keep the total number of live SVG text
    // elements within one serial batch's budget.
    let per_worker = if line.len() <= 4096 { 8 } else { batch_size / workers };
    let target = ends.iter().map(|&end| end as u128).sum::<u128>().div_ceil(workers as u128);
    let mut bytes = 0;
    let mut ranges = ends.split_inclusive(|&end| {
        bytes += end as u128;
        if bytes < target { return false; }
        bytes = 0;
        true
    });
    std::thread::scope(|scope| {
        let first = ranges.next().unwrap();
        let pending: Vec<_> = ranges.map(|range| {
            let measure = &measure;
            let handle = std::thread::Builder::new().spawn_scoped(scope, move || measure(range, per_worker));
            (range, handle)
        }).collect();
        let mut widths = measure(first, per_worker)?;
        for (range, handle) in pending {
            let part = match handle {
                Ok(thread) => thread.join().map_err(|_| Error::new("TEXT_LAYOUT_FAILED", "A caret measurement worker failed"))??,
                Err(_) => measure(range, per_worker)?,
            };
            widths.extend(part);
        }
        Ok(widths)
    })
}

fn build_run(doc: &Document, object: &Object) -> Result<TextLayout> {
    let Geometry::Text{content,font_family,font_size_mm,layout:settings,..}=&object.geometry else {return Err(Error::new("NOT_TEXT","text_measure requires a text object"));};
    validate_text(content,font_family,*font_size_mm)?;
    let (natural, probe, advance) = natural_run(content,font_family,*font_size_mm,settings.letter_spacing_mm)?;
    resolved_run(doc, object, &natural, node(&natural), probe, advance)
}

fn resolved_run(doc: &Document, object: &Object, natural: &usvg::Tree, natural_text: Option<&usvg::Text>,
                probe: bool, advance: f64) -> Result<TextLayout> {
    let Geometry::Text { content,font_family,font_size_mm,text_path,layout:settings } = &object.geometry else { unreachable!() };
    let (natural_glyphs,mut fonts) = match natural_text {
        Some(text) => glyphs(natural,text,probe)?,
        None => (vec![],BTreeMap::new()),
    };
    // Public usvg layout uses 0.5 / scale as its arclength tolerance. A fixed
    // high scale requests ~0.000122 mm without changing local glyph geometry.
    let curved=if text_path.is_some() {
        let body=markup(doc,object,"layout-text")?;
        Some(parse(&format!("<g transform=\"scale(4096)\">{body}</g>"))?)
    } else {None};
    let actual_text = match &curved { Some(tree) => node(tree), None => natural_text };
    let shaped_glyph_count = natural_glyphs.len();
    let natural_missing = natural_glyphs.iter().any(|g|g.glyph_id == 0);
    let placed = if let (Some(tree), Some(text)) = (&curved, actual_text) {
        let (glyphs, resolved) = glyphs(tree, text, false)?;
        fonts.extend(resolved);
        glyphs
    } else if curved.is_some() {
        Vec::new()
    } else {
        natural_glyphs
    };
    let mut outline=BezPath::new();
    let (outline_supported,ink_bounds)=if probe && text_path.is_none() {(true,None)}
        else if let Some(text)=actual_text {
            let supported=collect(text.flattened(),Affine::IDENTITY,&mut outline);
            let r=text.flattened().bounding_box();
            let bounds=if supported {(!outline.elements().is_empty()).then(||outline.bounding_box())}
                else {Some(Rect::new(r.x() as f64,r.y() as f64,r.right() as f64,r.bottom() as f64))};
            (supported,bounds)
        } else {(true,None)};
    let path_length=text_path.as_ref().map(|binding|guide(doc,object,binding).map(|(_,l)|l)).transpose()?;
    let overflow=if let (Some(binding),Some(length))=(text_path,path_length) {
        let start=binding.start_offset_mm-match binding.anchor {TextAnchor::Start=>0.,TextAnchor::Middle=>advance/2.,TextAnchor::End=>advance};
        start < -1e-5 || start+advance > length+1e-5 || (placed.len()!=shaped_glyph_count && !content.chars().all(|c|matches!(c,' '|'\r'|'\n'|'\t')))
    } else {false};
    let missing=natural_missing || (curved.is_some() && placed.iter().any(|g|g.glyph_id==0));
    let mut warnings=vec![];
    let resolved=fonts.values().filter_map(|f|f.families.first()).cloned().collect::<Vec<_>>().join(", ");
    if requested_font(font_family).is_none() {warnings.push(Notice::new("FONT_SUBSTITUTED",&format!("Font '{font_family}' is unavailable. Using: {resolved}.")));}
    else if fonts.values().any(|f|!f.families.iter().any(|name|name.eq_ignore_ascii_case(font_family))) && !matches!(font_family.as_str(),"serif"|"sans-serif"|"monospace") {
        warnings.push(Notice::new("FONT_FALLBACK",&format!("Some text uses fallback fonts: {resolved}. Requested: {font_family}.")));
    }
    if content.contains('\t') {warnings.push(Notice::new("TEXT_WHITESPACE_NORMALIZED","Tabs are displayed as one space; source content remains unchanged"));}
    if missing {warnings.push(Notice::new("MISSING_GLYPH","Some characters are missing from the available fonts. Complete outlining requires fonts for these characters."));}
    if overflow {warnings.push(Notice::new("TEXT_PATH_OVERFLOW","The complete shaped text does not fit on the guide; adjust offsets, font size or guide length"));}
    if !outline_supported {warnings.push(Notice::new("UNSUPPORTED_GLYPH_OUTLINE","The run contains color, SVG or bitmap glyph paint that cannot be represented as one monochrome path"));}
    if !outline.is_finite() || !advance.is_finite() {return Err(Error::new("TEXT_RANGE","Text layout produced nonfinite geometry"));}
    if settings.letter_spacing_mm < 0. {
        let mut unspaced=object.clone();
        if let Geometry::Text {layout,..}=&mut unspaced.geometry {layout.letter_spacing_mm=0.;}
        let base=build_run(doc,&unspaced)?;
        if base.shaped_glyph_count!=shaped_glyph_count || advance<0. {
            return Err(Error::new("INVALID_TEXT_LAYOUT","Negative letter spacing would discard shaped glyphs; increase the spacing"));
        }
    }
    Ok(TextLayout {outline,outline_supported,ink_bounds,advance_mm:advance,glyphs:placed,shaped_glyph_count,fonts:fonts.into_values().collect(),warnings,overflow,missing_glyphs:missing,path_length_mm:path_length,
        lines:vec![],line_height_mm:settings.line_height_mm.unwrap_or(font_size_mm*1.2),
        layout_bounds:Rect::new(0.,0.,advance,natural_text.map_or(*font_size_mm,|t|(t.bounding_box().bottom() as f64).max(*font_size_mm))),area_overflow:false})
}

/// Shape path text in one em-based chart. Parent scale and page translation
/// must not change f32 shaping/curve subdivision, then amplify those changes
/// when the result returns to page millimetres. Glyphs remain derived data.
fn build_path_run(doc:&Document,object:&Object)->Result<TextLayout>{
    let Geometry::Text{font_size_mm,text_path:Some(binding),..}=&object.geometry else{unreachable!()};
    let (path,length)=guide(doc,object,binding)?;
    let origin=path.elements().iter().find_map(|el|if let kurbo::PathEl::MoveTo(p)=el{Some(*p)}else{None}).ok_or_else(||Error::new("INVALID_TEXT_PATH","The guide has no origin"))?;
    let direction=path.elements().iter().flat_map(|el|match *el{kurbo::PathEl::MoveTo(p)|kurbo::PathEl::LineTo(p)=>vec![p],kurbo::PathEl::QuadTo(c,p)=>vec![c,p],kurbo::PathEl::CurveTo(a,b,p)=>vec![a,b,p],kurbo::PathEl::ClosePath=>vec![]}).find(|p|*p!=origin).ok_or_else(||Error::new("INVALID_TEXT_PATH","The guide has no direction"))?-origin;
    let unit=*font_size_mm/128.;let angle=direction.y.atan2(direction.x);
    let chart=Affine::translate(origin.to_vec2())*Affine::rotate(angle)*Affine::scale(unit);
    let mut text=object.clone();text.parent_id=None;text.x_mm=0.;text.y_mm=0.;text.transform=Affine::IDENTITY.as_coeffs();
    if let Geometry::Text{font_size_mm,layout,text_path:Some(path),..}=&mut text.geometry{
        *font_size_mm=128.;layout.letter_spacing_mm/=unit;layout.line_height_mm=layout.line_height_mm.map(|v|v/unit);path.start_offset_mm/=unit;path.baseline_offset_mm/=unit;
    }
    let mut guide_object=doc.object(&binding.path_id)?.clone();guide_object.parent_id=None;guide_object.x_mm=0.;guide_object.y_mm=0.;guide_object.transform=Affine::IDENTITY.as_coeffs();guide_object.geometry=Geometry::Path{d:(chart.inverse()*path).to_svg(),fill_rule:Default::default()};
    let canonical=Document{id:doc.id.clone(),unit:doc.unit.clone(),pages:doc.pages.clone(),objects:vec![guide_object,text],resources:BTreeMap::new()};
    let text=canonical.object(&object.id)?;let mut run=build_run(&canonical,text)?;
    run.outline=chart*run.outline;run.ink_bounds=if run.outline_supported{(!run.outline.elements().is_empty()).then(||run.outline.bounding_box())}else{run.ink_bounds.map(|r|chart.transform_rect_bbox(r))};
    for glyph in &mut run.glyphs{glyph.outline_transform=(chart*Affine::new(glyph.outline_transform)).as_coeffs();let p=chart*Point::new(glyph.origin_mm[0],glyph.origin_mm[1]);glyph.origin_mm=[p.x,p.y];glyph.angle_degrees+=angle.to_degrees();}
    run.advance_mm*=unit;run.line_height_mm*=unit;run.path_length_mm=Some(length);run.layout_bounds=Affine::scale(unit).transform_rect_bbox(run.layout_bounds);
    Ok(run)
}

/// Basic Chinese/English line policy on UAX #29 word/grapheme boundaries.
/// This is deliberately not a claim of complete UAX #14 or locale kinsoku.
fn safe_break(text: &str, at: usize) -> bool {
    if at==text.len() {return true;}
    let before=text[..at].chars().next_back().unwrap();
    let after=text[at..].chars().next().unwrap();
    !"\u{a0}\u{202f}\u{2060}\u{feff}".contains(before)
        && !"\u{a0}\u{202f}\u{2060}\u{feff}".contains(after)
        && !"（([【《〈「『〔［｛{“‘".contains(before)
        && !"，。！？、；：,.!?;:）)]】》〉」』〕］｝}”’".contains(after)
}
fn paragraphs(content: &str) -> Vec<(usize,usize,bool)> {
    let mut result=vec![];let mut start=0;let mut chars=content.char_indices().peekable();
    while let Some((at,c))=chars.next() {
        if matches!(c,'\r'|'\n'|'\u{2028}'|'\u{2029}') {
            result.push((start,at,true));start=at+c.len_utf8();
            if c=='\r' && chars.peek().is_some_and(|(_,c)|*c=='\n') {let (i,c)=chars.next().unwrap();start=i+c.len_utf8();}
        }
    }
    result.push((start,content.len(),false));result
}
struct ResolvedLine { start:usize, end:usize, text:String, break_after:&'static str, run:Arc<TextLayout> }

struct WrappedLine {
    end: usize,
    run: Arc<TextLayout>,
}

// Count geometry retained by derived caches. Dense outlines can be much larger
// than the same number of stories containing a few labels.
impl TextLayout {
    pub(crate) fn local_path(&self) -> BezPath {
        if self.outline_supported { return self.outline.clone(); }
        self.ink_bounds.map_or_else(BezPath::new, |r| r.to_path(1e-4))
    }

    fn cache_weight(&self) -> usize {
        1 + self.outline.elements().len() + 4 * self.glyphs.len() + self.lines.len()
    }
}

struct LayoutCache<V> {
    entries: VecDeque<(String, V, usize)>,
    weight: usize,
    max_entries: usize,
    max_weight: usize,
}

impl<V> LayoutCache<V> {
    fn new(max_entries: usize, max_weight: usize) -> Self {
        Self { entries: VecDeque::new(), weight: 0, max_entries, max_weight }
    }

    fn get(&self, key: &str) -> Option<&V> {
        self.entries.iter().find(|(k,_,_)|k == key).map(|(_,v,_)|v)
    }

    fn insert(&mut self, key: String, value: V, weight: usize) {
        if self.get(&key).is_some() { return; }
        while !self.entries.is_empty() &&
            (self.entries.len() >= self.max_entries || self.weight + weight > self.max_weight) {
            self.weight -= self.entries.pop_front().unwrap().2;
        }
        self.entries.push_back((key, value, weight));
        self.weight += weight;
    }
}

fn visible_line(paragraph: &str, start: usize, end: usize) -> &str {
    let text = &paragraph[start..end];
    if end == paragraph.len() { text } else { text.trim_end_matches([' ', '\t']) }
}

// A single SVG parse shares usvg's font and glyph-outline caches across
// independent text elements. Each prefix still shapes with its own context.
fn prefix_tree<'a>(attrs: &str, size: f64, prefixes: impl Iterator<Item=&'a str>) -> Option<usvg::Tree> {
    use std::fmt::Write;
    let mut body = String::new();
    for (i, prefix) in prefixes.enumerate() {
        write!(body, "<text id=\"prefix-{i}\" y=\"{size}\" {attrs}>{}</text>", xml(prefix)).unwrap();
    }
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(||parse(&body))).ok()?.ok()
}

fn prefix_node(tree: &usvg::Tree, index: usize) -> Option<&usvg::Text> {
    match tree.node_by_id(&format!("prefix-{index}")) {
        Some(usvg::Node::Text(text)) => Some(text),
        _ => None,
    }
}

struct LineShaper<'a> {
    doc: &'a Document,
    template: Object,
    memo: HashMap<String, Arc<TextLayout>>,
    attrs: String,
    size: f64,
}

impl LineShaper<'_> {
    fn run(&mut self, text: &str, parsed: Option<(&usvg::Tree, &usvg::Text)>) -> Result<Arc<TextLayout>> {
        if let Some(run) = self.memo.get(text) { return Ok(run.clone()); }
        if let Geometry::Text {content,..} = &mut self.template.geometry { text.clone_into(content); }
        let run = Arc::new(match parsed {
            Some((tree, node)) => resolved_run(self.doc, &self.template, tree, Some(node), false, node.bounding_box().width().into())?,
            None => build_run(self.doc, &self.template)?,
        });
        self.memo.insert(text.to_owned(), run.clone());
        Ok(run)
    }

    fn fit(&mut self, paragraph: &str, start: usize, ends: &[usize], width: f64, split_word: bool) -> Result<Option<WrappedLine>> {
        let mut remaining = &ends[ends.partition_point(|&end| end <= start)..];
        let mut best = None;
        // Cap speculative work using the frame width in ems. Every break
        // decision below still uses the individually shaped prefix width.
        let limit = (width / self.size).clamp(1., 8.) as usize;
        while !remaining.is_empty() {
            let prefix = |end| visible_line(paragraph, start, end);
            let count = remaining.iter().take(limit)
                .take_while(|&&end| end - start <= 4096 && !self.memo.contains_key(prefix(end))).count().max(1);
            let (batch, rest) = remaining.split_at(count);
            let tree = if count > 1 {
                prefix_tree(&self.attrs, self.size, batch.iter().map(|&end|prefix(end)))
            } else { None };
            for (i, &end) in batch.iter().enumerate() {
                let parsed = tree.as_ref().and_then(|t|prefix_node(t, i).map(|n|(t,n)));
                let run = self.run(prefix(end), parsed)?;
                let fits = run.advance_mm <= width + 1e-5;
                if fits || (split_word && best.is_none()) { best = Some(WrappedLine { end, run }); }
                if !fits { return Ok(best); }
            }
            remaining = rest;
        }
        Ok(best)
    }
}

// Only chosen lines survive a wrap. Bound the retained geometry as well as
// the paragraph count; a large story can still be laid out without admission.
fn wrap_paragraph(
    paragraph: &str, width: f64, key: String, shaper: &mut LineShaper,
) -> Result<Arc<[WrappedLine]>> {
    const WEIGHT_LIMIT: usize = 131_072;
    static RECENT: OnceLock<Mutex<LayoutCache<Arc<[WrappedLine]>>>> = OnceLock::new();
    let cache = RECENT.get_or_init(|| Mutex::new(LayoutCache::new(64, WEIGHT_LIMIT)));
    if let Some(rows) = cache.lock().expect("wrap cache").get(&key) {
        return Ok(rows.clone());
    }
    let words: Vec<_> = paragraph.split_word_bound_indices().map(|(i,s)|i+s.len())
        .filter(|&i|safe_break(paragraph,i) && !paragraph[i..].starts_with([' ','\t'])).collect();
    let graphemes: Vec<_> = paragraph.grapheme_indices(true).map(|(i,s)|i+s.len())
        .filter(|&i|safe_break(paragraph,i)).collect();
    let mut rows = Vec::new();
    let mut start = 0;
    while start < paragraph.len() {
        let row = match shaper.fit(paragraph, start, &words, width, false)? {
            Some(row) => row,
            None => shaper.fit(paragraph, start, &graphemes, width, true)?
                .ok_or_else(||Error::new("TEXT_LAYOUT_FAILED","No safe wrapping boundary was found"))?,
        };
        start = row.end;
        rows.push(row);
    }
    let rows: Arc<[WrappedLine]> = rows.into();
    let weight = rows.iter().map(|row|row.run.cache_weight()).sum();
    if weight <= WEIGHT_LIMIT {
        cache.lock().expect("wrap cache").insert(key, rows.clone(), weight);
    }
    Ok(rows)
}

fn paragraph_lines(doc: &Document, object: &Object) -> Result<Vec<ResolvedLine>> {
    let Geometry::Text {content, font_family, font_size_mm, layout:settings,..} = &object.geometry else {unreachable!()};
    let mut line = object.clone();
    if let Geometry::Text{layout,..} = &mut line.geometry {
        layout.mode=TextMode::Point; layout.align=TextAlign::Left;
    }
    validate_text(content, font_family, *font_size_mm)?;
    let mut shaper = LineShaper { doc, template:line, memo:HashMap::new(), size:*font_size_mm,
        attrs:attributes(font_family, *font_size_mm, settings.letter_spacing_mm) };
    let mut lines = Vec::new();
    for (first, last, hard) in paragraphs(content) {
        let paragraph = &content[first..last];
        let terminal = if hard { "hard" } else { "end" };
        if settings.mode == TextMode::Point || paragraph.is_empty() {
            lines.push(ResolvedLine{start:first, end:last, text:paragraph.into(), break_after:terminal, run:shaper.run(paragraph, None)?});
            continue;
        }
        // Placement, frame height, alignment and the hard separator are
        // applied by the caller after the paragraph's local breaks are known.
        let key = sha256(&serde_json::to_vec(&(paragraph,font_family,font_size_mm,settings.letter_spacing_mm,
                                               settings.line_height_mm,object.width_mm)).expect("wrap key"));
        let rows = wrap_paragraph(paragraph, object.width_mm, key, &mut shaper)?;
        let mut start = 0;
        for row in rows.iter() {
            lines.push(ResolvedLine { start:first+start, end:first+row.end,
                text:visible_line(paragraph,start,row.end).into(), run:row.run.clone(),
                break_after:if row.end == paragraph.len() {terminal} else {"soft"} });
            start = row.end;
        }
    }
    Ok(lines)
}

fn rect_array(rect: Rect) -> [f64;4] { [rect.x0,rect.y0,rect.width(),rect.height()] }
fn build(doc: &Document, object: &Object) -> Result<TextLayout> {
    validate_options(object)?;
    let Geometry::Text {content,font_size_mm,text_path,layout:settings,..}=&object.geometry else {return Err(Error::new("NOT_TEXT","text_measure requires a text object"));};
    if text_path.is_some() {
        let mut run=build_path_run(doc,object)?;
        run.lines.push(LineInfo {index:0,source_range_bytes:[0,content.len()],text:content.clone(),break_after:"end",baseline_origin_mm:None,advance_mm:run.advance_mm,
            glyph_range:[0,run.glyphs.len()],ink_bounds_local_mm:run.ink_bounds.map(rect_array)});
        return Ok(run);
    }
    let source=paragraph_lines(doc,object)?;
    let advance=source.iter().map(|l|l.run.advance_mm).fold(0.,f64::max);
    let width=if settings.mode==TextMode::Area {object.width_mm} else {advance};
    let line_height=settings.line_height_mm.unwrap_or(font_size_mm*1.2);
    let mut result=TextLayout {outline:BezPath::new(),outline_supported:true,ink_bounds:None,advance_mm:advance,glyphs:vec![],shaped_glyph_count:0,fonts:vec![],warnings:vec![],
        overflow:false,missing_glyphs:false,path_length_mm:None,lines:vec![],line_height_mm:line_height,layout_bounds:Rect::ZERO,area_overflow:false};
    let mut fonts=BTreeMap::new();let mut height=source.len() as f64*line_height;
    for (index,line) in source.into_iter().enumerate() {
        let x=match settings.align {TextAlign::Left=>0.,TextAlign::Center=>(width-line.run.advance_mm)/2.,TextAlign::Right=>width-line.run.advance_mm};
        let y=index as f64*line_height;
        if ![x,y].iter().all(|v|v.is_finite() && (*v as f32).is_finite()) {return Err(Error::new("TEXT_RANGE","Paragraph positions exceed the backend's coordinate range"));}
        let transform=Affine::translate((x,y));
        let first=result.glyphs.len();
        for glyph in &line.run.glyphs {
            let mut placed=glyph.clone();placed.origin_mm[0]+=x;placed.origin_mm[1]+=y;
            placed.outline_transform=(transform*Affine::new(glyph.outline_transform)).as_coeffs();result.glyphs.push(placed);
        }
        result.outline.extend((transform*line.run.outline.clone()).elements().iter().copied());
        let ink=line.run.ink_bounds.map(|r|transform.transform_rect_bbox(r));
        if let Some(r)=ink {result.ink_bounds=Some(result.ink_bounds.map_or(r,|a|a.union(r)));}
        result.shaped_glyph_count+=line.run.shaped_glyph_count;
        result.outline_supported &= line.run.outline_supported;result.missing_glyphs |= line.run.missing_glyphs;
        for font in &line.run.fonts {fonts.insert(font.key.clone(),font.clone());}
        for warning in &line.run.warnings {if !result.warnings.iter().any(|w|w.code==warning.code && w.message==warning.message) {result.warnings.push(warning.clone());}}
        height=height.max(y+line.run.layout_bounds.height());
        result.lines.push(LineInfo {index,source_range_bytes:[line.start,line.end],text:line.text,break_after:line.break_after,baseline_origin_mm:Some([x,y+font_size_mm]),
            advance_mm:line.run.advance_mm,glyph_range:[first,result.glyphs.len()],ink_bounds_local_mm:ink.map(rect_array)});
    }
    if !height.is_finite() || !(height as f32).is_finite() || !result.outline.is_finite() {return Err(Error::new("TEXT_RANGE","Paragraph layout exceeds the backend's coordinate range"));}
    result.layout_bounds=Rect::new(0.,0.,width,height);result.fonts=fonts.into_values().collect();
    result.area_overflow=settings.mode==TextMode::Area && (advance>width+1e-5 || height>object.height_mm+1e-5);
    result.overflow=result.area_overflow;
    if result.area_overflow {result.warnings.push(Notice::new("TEXT_AREA_OVERFLOW","The complete text exceeds its area; all lines remain visible and editable. Enlarge the area or adjust typography before outlining."));}
    Ok(result)
}

pub fn layout(doc: &Document, object: &Object) -> Result<Arc<TextLayout>> {
    static CACHE: OnceLock<Mutex<LayoutCache<Arc<TextLayout>>>> = OnceLock::new();
    let body = match &object.geometry {
        Geometry::Text{text_path:Some(_),..} => Some(markup(doc,object,"layout-text")?),
        Geometry::Text{..} => None,
        _ => return Err(Error::new("NOT_TEXT","Expected a text object")),
    };
    // Include area dimensions and typography even when the generated source
    // SVG is unchanged; resizing an area must invalidate line wrapping.
    let key=sha256(&serde_json::to_vec(&(&object.geometry,object.width_mm,object.height_mm,&body)).expect("text cache key"));
    let shared=CACHE.get_or_init(|| Mutex::new(LayoutCache::new(128, 262_144)));
    if let Some(layout)=shared.lock().expect("derived text layout cache").get(&key) {return Ok(layout.clone());}
    // Parsing/shaping runs outside the cache lock so different documents do
    // not serialize their full text work behind this derived-data cache.
    let built=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||build(doc,object)))
        .map_err(|_|Error::new("TEXT_LAYOUT_FAILED","The text backend failed; no layout or staged edit was published"))??;
    let layout=Arc::new(built);
    // Retain the current story even when its outline alone exceeds the budget:
    // successive paint, bounds and caret requests all need that same layout.
    shared.lock().expect("derived text layout cache").insert(key, layout.clone(), layout.cache_weight());
    Ok(layout)
}

pub(crate) fn require_complete(layout: &TextLayout) -> Result<()> {
    if layout.missing_glyphs {return Err(Error::new("MISSING_GLYPH","A complete layout requires fonts for all characters; no text was converted"));}
    if layout.area_overflow {return Err(Error::new("TEXT_AREA_OVERFLOW","The full text must fit in its area before conversion; no partial edit was committed"));}
    if layout.overflow {return Err(Error::new("TEXT_PATH_OVERFLOW","The full shaped run must fit on its guide; no partial edit was committed"));}
    Ok(())
}
pub(crate) fn validate_bound_layouts(doc: &Document) -> Result<Vec<Notice>> {
    let mut warnings=vec![];
    for object in &doc.objects {
        if matches!(&object.geometry,Geometry::Text{text_path:Some(_),..}) {
            let positioned=layout(doc,object)?;require_complete(&positioned)?;
            if !positioned.outline_supported {return Err(Error::new("UNSUPPORTED_GLYPH_OUTLINE","Path text currently requires monochrome outline glyphs"));}
            warnings.extend(positioned.warnings.clone());
        }
    }
    Ok(warnings)
}
pub(crate) fn convert(doc: &mut Document, id: &str) -> Result<Vec<Notice>> {
    doc.require_editable(id)?;
    let object=doc.object(id)?;
    let layout=layout(doc,object)?;require_complete(&layout)?;
    if !layout.outline_supported {return Err(Error::new("UNSUPPORTED_GLYPH_OUTLINE","Color, SVG or bitmap glyphs cannot be silently converted to a monochrome path"));}
    let bounds=layout.outline.bounding_box();
    let object=doc.objects.iter_mut().find(|o|o.id==id).expect("editable text");
    object.geometry=Geometry::Path {d:layout.outline.to_svg(),fill_rule:FillRule::NonZero};
    object.width_mm=bounds.width();object.height_mm=bounds.height();
    Ok(layout.warnings.clone())
}
pub(crate) fn measure(doc: &Document, id: &str) -> Result<Value> {
    let object=doc.object(id)?;let layout=layout(doc,object)?;
    let local=layout.ink_bounds.map(|r|[r.x0,r.y0,r.width(),r.height()]);
    let bounds=if layout.ink_bounds.is_some() {Some(doc.bounds(object)?)} else {None};
    Ok(json!({"object_id":id,"unit":"mm","advance_mm":layout.advance_mm,"ink_bounds_local_mm":local,"bounds_mm":bounds,
        "glyphs":layout.glyphs,"shaped_glyph_count":layout.shaped_glyph_count,"placed_glyph_count":layout.glyphs.len(),
        "fonts":layout.fonts,"path_length_mm":layout.path_length_mm,"overflow":layout.overflow,"outline_supported":layout.outline_supported,
        "lines":layout.lines,"line_count":layout.lines.len(),"line_height_mm":layout.line_height_mm,"layout_bounds_local_mm":if layout.path_length_mm.is_none() {Some(rect_array(layout.layout_bounds))} else {None},
        "area_mm":if matches!(&object.geometry,Geometry::Text{layout:TextLayoutOptions{mode:TextMode::Area,..},..}) {Some([object.width_mm,object.height_mm])} else {None},
        "warnings":layout.warnings,"world_transform":doc.world_transform(object)?.as_coeffs()}))
}
pub(crate) fn warnings(doc: &Document) -> Vec<Notice> {
    let mut warnings=vec![];
    for object in &doc.objects {if matches!(object.geometry,Geometry::Text{..}) {
        match layout(doc,object) {Ok(l)=>warnings.extend(l.warnings.clone()),Err(e)=>warnings.push(Notice::new(&e.code,&e.message))}
    }}
    warnings
}

#[cfg(test)]
mod layout_cache_tests {
    use super::*;
    #[test]
    fn batched_prefixes_match_independent_glyphs_outlines_and_warnings() {
        let doc = Document::new("prefixes".into(), 210., 297.).unwrap();
        let texts = ["A", "AV fi", "曲线（中文）", "e\u{301}\t42", "مرحبا ABC", "👩‍👩‍👧‍👦", "  "];
        for spacing in [0., 0.5, -0.02] {
            let tree = prefix_tree(&attributes("sans-serif", 4., spacing), 4., texts.into_iter()).unwrap();
            for (i, text) in texts.iter().enumerate() {
                let object: Object = serde_json::from_value(json!({
                    "id":"sample","page_id":"page-1","width_mm":50,"height_mm":20,
                    "geometry":{"kind":"text","content":text,"font_family":"sans-serif","font_size_mm":4,
                                "layout":{"letter_spacing_mm":spacing}}
                })).unwrap();
                let original = build_run(&doc, &object).unwrap();
                let Some(node) = prefix_node(&tree, i) else {
                    // Whitespace-only nodes use the independent marker.
                    continue;
                };
                let run = resolved_run(&doc, &object, &tree, Some(node), false, node.bounding_box().width().into()).unwrap();
                assert_eq!(run.advance_mm, original.advance_mm);
                assert_eq!(run.outline.elements(), original.outline.elements());
                assert_eq!(json!([run.glyphs,run.fonts,run.warnings,run.outline_supported]),
                           json!([original.glyphs,original.fonts,original.warnings,original.outline_supported]));
            }
        }
    }

    #[test]
    fn heavy_layout_eviction_releases_history_and_keeps_the_current_story() {
        let mut cache = LayoutCache::new(3, 10);
        for id in ["a","b","c","d"] {cache.insert(id.into(), id, 1);}
        assert!(cache.get("a").is_none());
        cache.insert("d".into(), "duplicate", 6);
        assert_eq!(cache.weight, 3);
        cache.insert("big".into(), "big", 9);
        assert_eq!(cache.weight, 10);
        assert!(cache.get("d").is_some());
        cache.insert("oversize".into(), "current", 25);
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(cache.get("oversize"), Some(&"current"));
        cache.insert("next".into(), "next", 1);
        assert_eq!(cache.weight, 1);
    }
}
