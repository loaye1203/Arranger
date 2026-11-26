//! Measure the first layout, caret widths and page rendering independently
//! of window polling. Accepts one area-text Object as JSON on stdin.
use arranger_core::{Document, Geometry, Object, model::sha256, render::Renderer, text};
use std::{io, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let story: Object = serde_json::from_reader(io::stdin().lock())?;
    let mut doc = Document::new("probe".into(), 210., 297.)?;
    let mut original = story.clone();
    if let Geometry::Text { content, .. } = &mut original.geometry {
        "原稿".clone_into(content);
    } else {
        return Err("Expected an area-text object".into());
    }
    // The desktop has already displayed the original story before the paste.
    text::layout(&doc, &original)?;
    doc.objects.push(story);
    let story = &doc.objects[0];
    let start = Instant::now();
    let layout = text::layout(&doc, story)?;
    let shaped = Instant::now();
    let mut carets = Vec::new();
    for line in &layout.lines {
        carets.push(text::caret_advances(&doc, story, &line.text, 0)?);
    }
    let measured = Instant::now();
    let rendered = Renderer::new().render(&doc, &story.page_id, 96., None)?;
    let painted = Instant::now();
    println!("{}", serde_json::json!({
        "layout_ms": (shaped - start).as_secs_f64() * 1000.,
        "carets_ms": (measured - shaped).as_secs_f64() * 1000.,
        "render_ms": (painted - measured).as_secs_f64() * 1000.,
        "lines": layout.lines.len(),
        "layout_sha256": sha256(&serde_json::to_vec(&(&layout.lines, &layout.glyphs, &layout.fonts))?),
        "carets_sha256": sha256(&serde_json::to_vec(&carets)?),
        "page_sha256": sha256(rendered.pixels.data()),
    }));
    Ok(())
}
