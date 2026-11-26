//! Structured edits are adapters around kurbo; filled-set operations use linesweeper.
//! The document remains the only editable model, including empty boolean results.
use crate::{Document, Error, Geometry, Object, Result};
use kurbo::{BezPath, ParamCurve, PathEl, Point, Shape};
use linesweeper::{sweep::Correctness, topology::{BinaryWindingNumber, OneOfTwo, Topology}};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
mod svg_smooth;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FillRule { #[default] NonZero, EvenOdd }
impl FillRule {
    pub fn is_nonzero(&self) -> bool { *self == Self::NonZero }
    pub fn is_inside(self, winding: i32) -> bool {
        match self { Self::NonZero => winding != 0, Self::EvenOdd => winding % 2 != 0 }
    }
    pub fn svg_name(self) -> &'static str {
        match self { Self::NonZero => "nonzero", Self::EvenOdd => "evenodd" }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BooleanOp { Union, Intersection, Difference, Xor }
impl BooleanOp {
    fn includes(self, a: bool, b: bool) -> bool {
        match self { Self::Union => a || b, Self::Intersection => a && b, Self::Difference => a && !b, Self::Xor => a != b }
    }
}

/// Absolute coordinates in the object's local millimetres. An index addresses
/// an element (including move/close), not a persistent node ID.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PathElement {
    MoveTo { to: [f64; 2] },
    LineTo { to: [f64; 2] },
    QuadTo { control: [f64; 2], to: [f64; 2] },
    CubicTo { control1: [f64; 2], control2: [f64; 2], to: [f64; 2] },
    Close,
}
impl From<PathEl> for PathElement {
    fn from(el: PathEl) -> Self {
        let pair = |p: Point| [p.x, p.y];
        match el {
            PathEl::MoveTo(p) => Self::MoveTo { to: pair(p) },
            PathEl::LineTo(p) => Self::LineTo { to: pair(p) },
            PathEl::QuadTo(c, p) => Self::QuadTo { control: pair(c), to: pair(p) },
            PathEl::CurveTo(c1, c2, p) => Self::CubicTo { control1: pair(c1), control2: pair(c2), to: pair(p) },
            PathEl::ClosePath => Self::Close,
        }
    }
}
impl PathElement {
    fn kurbo(&self) -> PathEl {
        let p = |v: [f64; 2]| Point::new(v[0], v[1]);
        match *self {
            Self::MoveTo { to } => PathEl::MoveTo(p(to)),
            Self::LineTo { to } => PathEl::LineTo(p(to)),
            Self::QuadTo { control, to } => PathEl::QuadTo(p(control), p(to)),
            Self::CubicTo { control1, control2, to } => PathEl::CurveTo(p(control1), p(control2), p(to)),
            Self::Close => PathEl::ClosePath,
        }
    }
}

pub(crate) fn contains(path: &BezPath, rule: FillRule, point: Point) -> bool {
    // kurbo 0.13.1's cubic winding root solver can misclassify almost-quadratic
    // cubics emitted by the boolean backend. Flatten only this derived hit-test
    // projection; editable/native/exported geometry retains the original curves.
    if path.elements().iter().all(|el|!matches!(el,PathEl::QuadTo(..)|PathEl::CurveTo(..))) {
        return rule.is_inside(path.winding(point));
    }
    let mut outline = BezPath::new();
    kurbo::flatten(path.elements().iter().copied(), 1e-4, |el| outline.push(el));
    rule.is_inside(outline.winding(point))
}

pub(crate) fn parse(d: &str) -> Result<BezPath> {
    let original = BezPath::from_svg(d).map_err(|_| Error::new("INVALID_PATH", "Path is not valid SVG path data"))?;
    // kurbo0.13.1 shares one previous control between cubic and quadratic
    // smooth commands. SVG requires separate reflection state for each family.
    let path = if let Some(expanded)=svg_smooth::expand(d)? {
        BezPath::from_svg(&expanded).map_err(|_| Error::new("INVALID_PATH", "Path is not valid SVG path data"))?
    }else{original};
    if !path.is_finite() { return Err(Error::new("INVALID_PATH", "Path coordinates must be finite")); }
    // SVG permits a drawing command after Z; expose its implicit initial point
    // as an explicit M so all structured edits have the same subpath grammar.
    let mut normalized = BezPath::new();
    let mut start = None;
    let mut active = false;
    for &el in path.elements() {
        match el {
            PathEl::MoveTo(p) => { start = Some(p); active = true; }
            PathEl::ClosePath => {
                if !active { continue; }
                active = false;
            }
            _ if !active => {
                let p = start.ok_or_else(|| Error::new("INVALID_PATH", "A subpath must start with move_to"))?;
                normalized.move_to(p); active = true;
            }
            _ => {}
        }
        normalized.push(el);
    }
    Ok(normalized)
}

fn from_elements(elements: &[PathElement]) -> Result<BezPath> {
    let mut path = BezPath::new();
    let mut active = false;
    for element in elements {
        match element {
            PathElement::MoveTo { .. } => active = true,
            PathElement::Close if active => active = false,
            _ if !active => return Err(Error::new("INVALID_PATH", "Each subpath starts with move_to; close ends that subpath")),
            _ => {}
        }
        path.push(element.kurbo());
    }
    if !path.is_finite() { return Err(Error::new("INVALID_PATH", "Path coordinates must be finite")); }
    Ok(path)
}

fn editable_path(doc: &Document, id: &str) -> Result<(BezPath, FillRule)> {
    doc.require_editable(id)?;
    let object = doc.object(id)?;
    if !matches!(object.geometry, Geometry::Path { .. }) { return Err(Error::new("NOT_A_PATH", "Convert a rectangle or ellipse with object_to_path first")); }
    Ok((object.local_path()?, object.fill_rule()))
}

fn store(doc: &mut Document, id: &str, path: BezPath, fill_rule: FillRule) -> Result<()> {
    let bounds = path.bounding_box();
    if !path.is_finite() || ![bounds.x0, bounds.y0, bounds.x1, bounds.y1, bounds.width(), bounds.height()].iter().all(|v| v.is_finite()) {
        return Err(Error::new("INVALID_PATH", "Path or bounds exceed the finite coordinate range"));
    }
    let object = doc.objects.iter_mut().find(|o| o.id == id).expect("editable path exists");
    object.geometry = Geometry::Path { d: path.to_svg(), fill_rule };
    object.width_mm = bounds.width(); object.height_mm = bounds.height();
    Ok(())
}

pub(crate) fn query(doc: &Document, id: &str) -> Result<Value> {
    let object = doc.object(id)?;
    if !matches!(object.geometry, Geometry::Path { .. }) { return Err(Error::new("NOT_A_PATH", "path_get requires a path object")); }
    let path = object.local_path()?;
    let elements: Vec<PathElement> = path.elements().iter().copied().map(Into::into).collect();
    let subpaths: Vec<_> = subpath_ranges(&path).into_iter().enumerate().map(|(index, (start, end))|
        json!({"index":index,"start_index":start,"end_index_exclusive":end,"closed":matches!(path.elements()[end-1],PathEl::ClosePath)})).collect();
    let b = path.bounding_box();
    Ok(json!({"object_id":id,"elements":elements,"fill_rule":object.fill_rule(),"subpaths":subpaths,
        "local_bounds_mm":[b.x0,b.y0,b.width(),b.height()],"bounds_mm":doc.bounds(object)?,
        "world_transform":doc.world_transform(object)?.as_coeffs()}))
}

fn subpath_ranges(path: &BezPath) -> Vec<(usize, usize)> {
    let starts: Vec<_> = path.elements().iter().enumerate().filter_map(|(i, e)| matches!(e, PathEl::MoveTo(_)).then_some(i)).collect();
    starts.iter().enumerate().map(|(i, &start)| (start, starts.get(i+1).copied().unwrap_or(path.elements().len()))).collect()
}

pub(crate) fn set(doc: &mut Document, id: &str, elements: &[PathElement], fill_rule: Option<FillRule>) -> Result<()> {
    let (_, old_rule) = editable_path(doc, id)?;
    store(doc, id, from_elements(elements)?, fill_rule.unwrap_or(old_rule))
}

pub(crate) fn edit_node(doc: &mut Document, id: &str, index: usize, element: Option<&PathElement>, insert: bool) -> Result<()> {
    let (path, rule) = editable_path(doc, id)?;
    let mut elements: Vec<PathElement> = path.elements().iter().copied().map(Into::into).collect();
    if index > elements.len() || (!insert && index == elements.len()) {
        return Err(Error::new("INVALID_INDEX", "Path element index is out of range"));
    }
    match (insert, element) {
        (true, Some(el)) => elements.insert(index, el.clone()),
        (false, Some(el)) => elements[index] = el.clone(),
        (false, None) => { elements.remove(index); }
        (true, None) => unreachable!("insert always supplies an element"),
    }
    store(doc, id, from_elements(&elements)?, rule)
}

pub(crate) fn set_closed(doc: &mut Document, id: &str, subpath_index: usize, closed: bool) -> Result<()> {
    let (path, rule) = editable_path(doc, id)?;
    let ranges = subpath_ranges(&path);
    let (_, end) = *ranges.get(subpath_index).ok_or_else(|| Error::new("INVALID_INDEX", "Subpath index is out of range"))?;
    let mut elements: Vec<PathElement> = path.elements().iter().copied().map(Into::into).collect();
    let was_closed = matches!(elements[end-1], PathElement::Close);
    if closed && !was_closed { elements.insert(end, PathElement::Close); }
    if !closed && was_closed { elements.remove(end-1); }
    store(doc, id, from_elements(&elements)?, rule)
}

pub(crate) fn split(doc: &mut Document, id: &str, index: usize, t: f64) -> Result<()> {
    let (path, rule) = editable_path(doc, id)?;
    if !t.is_finite() || !(0. < t && t < 1.) { return Err(Error::new("INVALID_PARAMETER", "Split parameter t must be finite and strictly between 0 and 1")); }
    if !matches!(path.elements().get(index), Some(PathEl::LineTo(_) | PathEl::QuadTo(..) | PathEl::CurveTo(..))) {
        return Err(Error::new("INVALID_INDEX", "Split index must identify a line, quadratic or cubic segment"));
    }
    let segment = path.get_seg(index).ok_or_else(|| Error::new("INVALID_PATH", "Segment has no initial point"))?;
    let mut elements: Vec<PathElement> = path.elements().iter().copied().map(Into::into).collect();
    elements.splice(index..=index, [segment.subsegment(0. .. t).as_path_el().into(), segment.subsegment(t .. 1.).as_path_el().into()]);
    store(doc, id, from_elements(&elements)?, rule)
}

pub(crate) fn convert(doc: &mut Document, id: &str) -> Result<()> {
    doc.require_editable(id)?;
    let object = doc.object(id)?;
    match object.geometry {
        Geometry::Path { .. } => Ok(()),
        Geometry::Rect { .. } | Geometry::Ellipse => store(doc, id, object.local_path()?, FillRule::NonZero),
        _ => Err(Error::new("UNSUPPORTED_GEOMETRY", "Only rectangles and ellipses can currently be converted to editable paths")),
    }
}

fn filled_operand(doc: &Document, id: &str) -> Result<(BezPath, FillRule)> {
    doc.require_editable(id)?;
    let object = doc.object(id)?;
    if !matches!(object.geometry, Geometry::Rect { .. } | Geometry::Ellipse | Geometry::Path { .. }) {
        return Err(Error::new("UNSUPPORTED_GEOMETRY", "Boolean operands must be rectangles, ellipses or closed paths"));
    }
    if !doc.active_clips(object)?.is_empty() {return Err(Error::new("UNSUPPORTED_GEOMETRY","Filled booleans require unclipped inputs; remove local and ancestor clips first"));}
    let path = object.local_path()?;
    let mut filled = BezPath::new();
    for (start, end) in subpath_ranges(&path) {
        let subpath = &path.elements()[start..end];
        // A lone anchor has no filled area. Keep empty-set results composable.
        if !subpath.iter().any(|e| matches!(e, PathEl::LineTo(_) | PathEl::QuadTo(..) | PathEl::CurveTo(..))) { continue; }
        if !matches!(subpath.last(), Some(PathEl::ClosePath)) { return Err(Error::new("OPEN_PATH", "Close every nonempty subpath before a filled boolean operation")); }
        filled.extend(subpath.iter().copied());
    }
    let world = doc.world_transform(object)? * filled;
    if !world.is_finite() { return Err(Error::new("BOOLEAN_RANGE", "Operand transform exceeds finite world coordinates")); }
    Ok((world, object.fill_rule()))
}

/// Preserve both inputs unless removal is explicit. Compute and validate before
/// changing the staged document; Service supplies atomic commit and history.
pub(crate) fn boolean(doc: &mut Document, left_id: &str, right_id: &str, result_id: &str, operation: BooleanOp, remove_inputs: bool) -> Result<Vec<String>> {
    if !crate::model::valid_id(result_id) { return Err(Error::new("INVALID_OBJECT", "Invalid boolean result ID")); }
    if doc.objects.iter().any(|o| o.id == result_id) { return Err(Error::new("DUPLICATE_ID", "Boolean result ID must be new")); }
    let (a, rule_a) = filled_operand(doc, left_id)?;
    let (b, rule_b) = filled_operand(doc, right_id)?;
    let left = doc.object(left_id)?;
    if left.page_id != doc.object(right_id)?.page_id { return Err(Error::new("CROSS_PAGE", "Boolean operands must be on the same page")); }
    let bounds = a.bounding_box().union(b.bounding_box());
    let magnitude = [bounds.x0, bounds.y0, bounds.x1, bounds.y1].into_iter().map(f64::abs).fold(0., f64::max);
    // Same conservative rule as linesweeper's binary_op; guard arithmetic and
    // refuse ranges where its tolerance would exceed a micron (0.001 mm).
    let eps = (magnitude * (f64::EPSILON * 64.)).max(1e-6);
    if ![bounds.x0, bounds.y0, bounds.x1, bounds.y1, bounds.width(), bounds.height()].iter().all(|v| v.is_finite()) || eps > 0.001 {
        return Err(Error::new("BOOLEAN_RANGE", "Boolean coordinate range requires a tolerance above 0.001 mm; move geometry nearer the page origin"));
    }
    // Use the public topology API so each operand keeps its own fill rule.
    // ReallyCorrect is intentional: the default GoodEnough reconstruction
    // missed the curve area oracle by 0.1624 mm²; see the retained regression.
    // A backend panic must not poison the document mutex or publish half a result.
    let output = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let topology = Topology::from_paths_with_correctness([(&a, OneOfTwo::A), (&b, OneOfTwo::B)], eps, Correctness::ReallyCorrect)
            .map_err(|e| Error::new("BOOLEAN_FAILED", e.to_string()))?;
        let contours = topology.contours(|w: &BinaryWindingNumber| operation.includes(rule_a.is_inside(w.shape_a), rule_b.is_inside(w.shape_b)));
        let mut output = BezPath::new();
        for contour in contours.contours() { output.extend(contour.path.elements().iter().copied()); }
        Ok::<_, Error>(output)
    })).map_err(|_| Error::new("BOOLEAN_FAILED", "Boolean backend failed; no result was published"))??;
    if !output.is_finite() { return Err(Error::new("BOOLEAN_FAILED", "Boolean backend returned nonfinite coordinates")); }
    let result_bounds = output.bounding_box();
    if ![result_bounds.x0, result_bounds.y0, result_bounds.x1, result_bounds.y1, result_bounds.width(), result_bounds.height()].iter().all(|v| v.is_finite())
        || subpath_ranges(&output).iter().any(|&(_, end)| !matches!(output.elements()[end-1], PathEl::ClosePath)) {
        return Err(Error::new("BOOLEAN_FAILED", "Boolean backend returned an open contour or nonfinite bounds"));
    }
    let result = Object { id: result_id.into(), page_id: left.page_id.clone(), parent_id: None,
        x_mm: 0., y_mm: 0., width_mm: result_bounds.width(), height_mm: result_bounds.height(),
        transform: [1., 0., 0., 1., 0., 0.], style: left.style.clone(), visible: left.visible, locked: false,clip:None,
        geometry: Geometry::Path { d: output.to_svg(), fill_rule: FillRule::EvenOdd } };
    // Even-odd handles all contour orientations, including nested holes.
    // Inputs' ancestor opacity/visibility and strokes are not part of set geometry.
    let mut changed = vec![result_id.to_owned()];
    if remove_inputs {
        doc.objects.retain(|o| o.id != left_id && o.id != right_id);
        changed.push(left_id.into());
        if right_id != left_id { changed.push(right_id.into()); }
    }
    doc.objects.push(result);
    Ok(changed)
}
