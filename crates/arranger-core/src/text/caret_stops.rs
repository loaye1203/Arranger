//! Recover exact prefix widths at the shaper's safe-to-break boundaries.
//! Re-shape a short ending at a safe join for cuts inside kerning pairs or clusters.
use super::{fontdb, natural_run, node, options, usvg};
use harfrust::{BufferFlags, Direction, Feature, FontRef, Script, ShapeOptions, Shaper, ShaperData, ShaperInstance, Tag, UnicodeBuffer, Variation};

struct Glyph {
    byte: usize,
    id: u32,
    safe: bool,
    join: bool,
    x_offset: i32,
    advance: i32,
}

fn with_shaper<T>(db: &fontdb::Database, face: fontdb::ID, span: &usvg::TextSpan,
    use_shaper: impl FnOnce(&Shaper<'_>) -> Option<T>) -> Option<T> {
    db.with_face_data(face, |bytes, index| {
        let font = FontRef::from_index(bytes, index).ok()?;
        let size = span.font_size().get();
        let mut axes: Vec<_> = span.font().variations().iter().map(|v| Variation {
            tag: Tag::from_be_bytes(v.tag), value: v.value,
        }).collect();
        if span.font_optical_sizing() == usvg::FontOpticalSizing::Auto && !axes.iter().any(|v| v.tag == *b"opsz") {
            axes.push(Variation { tag: Tag::new(b"opsz"), value: size });
        }
        let instance = ShaperInstance::from_variations(&font, &axes);
        let data = ShaperData::new(&font);
        let shaper = data.shaper(&font).instance(Some(&instance)).build();
        use_shaper(&shaper)
    })?
}

// A fragment uses the whole line's script, including when it begins with
// punctuation or Latin characters in a Han-script run.
struct Shaping<'a> {
    script: Script,
    size: f32,
    features: &'a [Feature],
}

impl Shaping<'_> {
    fn glyphs(&self, shaper: &Shaper<'_>, text: &str) -> Vec<Glyph> {
        let mut buffer = UnicodeBuffer::new();
        buffer.set_direction(Direction::LeftToRight);
        buffer.set_script(self.script);
        buffer.set_flags(BufferFlags::PRODUCE_UNSAFE_TO_CONCAT);
        buffer.push_str(text);
        buffer.guess_segment_properties();
        let run = shaper.shape(buffer, ShapeOptions::new().features(self.features));
        run.glyph_infos().iter().zip(run.glyph_positions()).map(|(info, pos)| Glyph {
            byte: info.cluster as usize, id: info.glyph_id, safe: !info.unsafe_to_break(),
            join: !info.unsafe_to_concat(), x_offset: pos.x_offset, advance: pos.x_advance,
        }).collect()
    }

    fn ending_advances(&self, font: &Shaper<'_>, base: Option<&Shaper<'_>>, part: &str, scale: f32)
        -> Option<Vec<f32>> {
        if let Some(base) = base {
            let original = self.glyphs(base, part);
            if original.first()?.byte != 0 || original.iter().take_while(|g| g.byte == 0).any(|g| !g.join) {
                return None;
            }
        }
        let glyphs = self.glyphs(font, part);
        if !glyphs.first().is_some_and(|g| g.byte == 0 && g.join)
            || !glyphs.windows(2).all(|pair| pair[0].byte < pair[1].byte) {
            return None;
        }
        glyphs.iter().map(|g| {
            let advance = g.advance as f32 * scale;
            (g.id != 0 && advance > 0. && advance.is_finite()).then_some(advance)
        }).collect()
    }

    fn fill_endings(&self, font: &Shaper<'_>, base_font: Option<&Shaper<'_>>, line: &str,
        joins: &[(usize, f32)], ends: &[usize], widths: &mut [Option<f64>]) {
        let scale = self.size / font.units_per_em() as f32;
        let mut cache = std::collections::HashMap::new();
        for (&end, width) in ends.iter().zip(widths) {
            if width.is_some() { continue; }
            let before = joins.partition_point(|&(start, _)| start < end);
            // Bound work and memory even if this font has no viable nearby join.
            for &(start, mut x) in joins[..before].iter().rev().take(8) {
                if end - start > 128 { break; }
                let part = &line[start..end];
                if cache.len() >= 256 { cache.clear(); }
                let steps = cache.entry(part).or_insert_with(||
                    self.ending_advances(font, base_font, part, scale));
                if let Some(steps) = steps {
                    // usvg accumulates each scaled cluster into the running f32
                    // position. Adding a pre-summed width rounds differently.
                    for dx in steps.iter() { x += dx; }
                    if x.is_finite() { *width = Some(f64::from(x)); }
                    break;
                }
            }
        }
    }
}

pub(super) fn widths(line: &str, family: &str, size: f64, ends: &[usize]) -> Option<Vec<Option<f64>>> {
    // These characters form one LTR run. After the first Latin or Han letter,
    // every prefix has the same first script as the complete line. Earlier
    // cuts (including a whitespace-only prefix) are measured independently.
    let han = |c: char| ('\u{4e00}'..='\u{9fff}').contains(&c);
    let is_letter = |c: char| c.is_ascii_alphabetic() || han(c);
    let punctuation = "，。；：、！？（）《》〈〉「」『』【】〔〕“”‘’—…";
    let first_letter = line.char_indices().find(|&(_, c)| is_letter(c))?.0;
    if !line.chars().all(|c| is_letter(c) || c.is_ascii_graphic() || c == ' '
        || punctuation.contains(c) || ('\u{300}'..='\u{36f}').contains(&c)) {
        return None;
    }
    let (tree, _, width) = natural_run(line, family, size, 0.).ok()?;
    let text = node(&tree)?;
    let [chunk] = text.chunks() else { return None };
    let [span] = chunk.spans() else { return None };
    let [placed] = text.layouted() else { return None };
    if chunk.text() != line || span.small_caps() { return None; }
    let glyphs = &placed.positioned_glyphs;
    let face = glyphs.first()?.font;
    let mut db = tree.fontdb().clone();
    let resolver = &options().font_resolver;
    let base = (resolver.select_font)(span.font(), &mut db)?;
    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(line);
    buffer.guess_segment_properties();
    let no_kern = [Feature::new(Tag::new(b"kern"), 0, ..)];
    let shaping = Shaping {
        script: buffer.script(),
        size: span.font_size().get(),
        features: if span.apply_kerning() { &[] } else { &no_kern },
    };
    let run = with_shaper(&db, face, span, |font| Some(shaping.glyphs(font, line)))?;
    // A positive advance per cluster makes the next cluster's unshifted
    // origin equal to the prefix's typographic bounding-box width.
    if run.len() != glyphs.len() || run.first()?.byte != 0 ||
        !run.windows(2).all(|pair| pair[0].byte < pair[1].byte) ||
        !run.iter().zip(glyphs).all(|(g, p)| g.advance > 0 && g.id != 0 && p.font == face && p.id.0 == g.id) {
        return None;
    }
    // usvg replaces the entire base run when its first fallback covers it.
    // A prefix must retain both that first missing glyph and a safe break
    // in the base run before the fallback's positions apply to it.
    let mut base_breaks = None;
    if base != face {
        let original = with_shaper(&db, base, span, |font| Some(shaping.glyphs(font, line)))?;
        if !original.windows(2).all(|pair| pair[0].byte < pair[1].byte) { return None; }
        let missing = original.iter().find(|g| g.id == 0)?.byte;
        let ch = line.get(missing..)?.chars().next()?;
        if (resolver.select_fallback)(ch, &[base], &mut db)? != face { return None; }
        let mut safe = vec![(false, false); line.len()];
        for g in &original {
            if g.byte > missing { safe[g.byte] = (g.safe, g.join); }
        }
        base_breaks = Some(safe);
    }
    let mut widths = vec![None; ends.len()];
    let mut witness = None;
    for (g, placed) in run.iter().zip(glyphs).skip(1) {
        if g.byte > first_letter && g.safe && g.x_offset == 0 && base_breaks.as_ref().is_none_or(|safe| safe[g.byte].0) {
            let x = f64::from(placed.transform().tx);
            witness.get_or_insert((g.byte, x));
            if let Ok(index) = ends.binary_search(&g.byte) { widths[index] = Some(x); }
        }
    }
    // Check the earliest stable prefix for visible ink, even when the caller
    // only needs the last few stops. usvg drops nodes with no ink.
    let (witness_end, expected) = witness?;
    let (prefix, marker, measured) = natural_run(&line[..witness_end], family, size, 0.).ok()?;
    if marker || node(&prefix).is_none() || expected != measured { return None; }
    *widths.last_mut()? = Some(width);
    if widths.iter().all(Option::is_some) { return Some(widths); }
    // HarfRust's incremental line-break algorithm requires BOTH sides
    // of a splice to be clear of UNSAFE_TO_CONCAT. When using fallback,
    // keep the base font's original first missing glyph before the splice.
    let joins: Vec<_> = run.iter().zip(glyphs).filter(|(g, _)| {
        g.join && g.x_offset == 0 && g.byte >= witness_end
            && base_breaks.as_ref().is_none_or(|safe| safe[g.byte].1)
    }).map(|(g, placed)| (g.byte, placed.transform().tx)).collect();
    if !joins.is_empty() {
        with_shaper(&db, face, span, |font| {
            let mut finish = |base_font: Option<&Shaper<'_>>| {
                shaping.fill_endings(font, base_font, line, &joins, ends, &mut widths);
                Some(())
            };
            if base == face { finish(None) }
            else { with_shaper(&db, base, span, |original| finish(Some(original))) }
        })?;
    }
    Some(widths)
}
