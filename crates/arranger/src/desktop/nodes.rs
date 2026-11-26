//! Derived node geometry. A drag previews this projection; release publishes one batch.
use arranger_core::{Document, Error, Geometry, Object, PathElement, Result, command::Operation};
use kurbo::{Affine, BezPath, CubicBez, Line, ParamCurve, ParamCurveNearest, PathEl, PathSeg, Point, Shape};

pub(super) fn supported(object: &Object) -> bool {
    matches!(object.geometry, Geometry::Path { .. } | Geometry::Rect { .. } | Geometry::Ellipse)
}

pub(super) struct ElementPoints([Point;3],usize);
impl std::ops::Deref for ElementPoints {
    type Target=[Point];
    fn deref(&self)->&Self::Target {&self.0[..self.1]}
}
pub(super) fn points(el: PathEl) -> ElementPoints {
    let zero=Point::ORIGIN;
    match el {
        PathEl::MoveTo(p) | PathEl::LineTo(p) => ElementPoints([p,zero,zero],1),
        PathEl::QuadTo(c,p) => ElementPoints([c,p,zero],2),
        PathEl::CurveTo(a,b,p) => ElementPoints([a,b,p],3),
        PathEl::ClosePath => ElementPoints([zero;3],0),
    }
}

fn range(path: &BezPath, index: usize) -> (usize, usize) {
    let start = path.elements()[..=index].iter().rposition(|e| matches!(e, PathEl::MoveTo(_))).unwrap_or(0);
    let end = path.elements()[index + 1..].iter().position(|e| matches!(e, PathEl::MoveTo(_)))
        .map_or(path.elements().len(), |i| index + 1 + i);
    (start, end)
}

/// Closed curves often encode the first anchor twice. Keep both endpoints in
/// the path but expose only one handle, and update both when that handle moves.
pub(super) fn duplicate_anchor(path: &BezPath, index: usize) -> bool {
    if index == 0 || !matches!(path.elements().get(index + 1), Some(PathEl::ClosePath)) { return false; }
    let (start, _) = range(path, index);
    points(path.elements()[index]).last().zip(points(path.elements()[start]).last())
        .is_some_and(|(a, b)| a.distance(*b) < 1e-9)
}

pub(super) struct ControlArm {
    pub index: usize,
    pub point: usize,
    pub anchor: Point,
    pub control: Point,
}

/// A cubic has two separate tangent arms, never a control-to-control edge.
/// The incoming arm at an explicit closed endpoint belongs to the first node.
pub(super) fn control_arms(path: &BezPath, selected: usize) -> Vec<ControlArm> {
    let mut arms = vec![];
    let mut previous = Point::ORIGIN;
    let mut previous_index = 0;
    for (index, el) in path.iter().enumerate() {
        let canonical = if duplicate_anchor(path, index) { range(path, index).0 } else { index };
        match el {
            PathEl::QuadTo(control, to) => {
                if previous_index == selected { arms.push(ControlArm { index, point: 0, anchor: previous, control }); }
                if canonical == selected { arms.push(ControlArm { index, point: 0, anchor: to, control }); }
            }
            PathEl::CurveTo(a, b, to) => {
                if previous_index == selected { arms.push(ControlArm { index, point: 0, anchor: previous, control: a }); }
                if canonical == selected { arms.push(ControlArm { index, point: 1, anchor: to, control: b }); }
            }
            _ => {}
        }
        if let Some(to) = points(el).last() { previous = *to; previous_index = canonical; }
    }
    arms
}

fn set_point(el: &mut PathEl, point: usize, to: Point) {
    match el {
        PathEl::MoveTo(p) | PathEl::LineTo(p) => *p = to,
        PathEl::QuadTo(c, p) => if point == 0 { *c = to; } else { *p = to; },
        PathEl::CurveTo(a, b, p) => match point { 0 => *a = to, 1 => *b = to, _ => *p = to },
        PathEl::ClosePath => {}
    }
}

pub(super) fn moved(path: &BezPath, index: usize, point: usize, to: Point) -> Result<BezPath> {
    let old = points(*path.elements().get(index).ok_or_else(|| Error::new("INVALID_INDEX", "Node disappeared"))?);
    let from = *old.get(point).ok_or_else(|| Error::new("INVALID_INDEX", "Node disappeared"))?;
    let mut elements = path.elements().to_vec();
    set_point(&mut elements[index], point, to);
    if point == old.len() - 1 {
        let delta = to - from;
        match &mut elements[index] {
            PathEl::CurveTo(_, c, _) | PathEl::QuadTo(c, _) => *c += delta,
            _ => {}
        }
        if let Some(next) = elements.get_mut(index + 1) {
            match next { PathEl::CurveTo(c, _, _) | PathEl::QuadTo(c, _) => *c += delta, _ => {} }
        }
        let (start, end) = range(path, index);
        if index == start && end >= start + 3 && duplicate_anchor(path, end - 2) {
            let last = end - 2;
            let count = points(elements[last]).len();
            set_point(&mut elements[last], count - 1, to);
            match &mut elements[last] {
                PathEl::CurveTo(_, c, _) | PathEl::QuadTo(c, _) => *c += delta,
                _ => {}
            }
        }
    }
    Ok(BezPath::from_vec(elements))
}

pub(super) fn segments(path: &BezPath) -> Vec<(usize, PathSeg)> {
    let mut start = Point::ORIGIN;
    let mut previous = start;
    let mut result = vec![];
    for (index, el) in path.elements().iter().enumerate() {
        match *el {
            PathEl::MoveTo(p) => { start = p; previous = p; }
            PathEl::ClosePath => {
                if previous.distance(start) > 1e-9 { result.push((index, PathSeg::Line(Line::new(previous, start)))); }
                previous = start;
            }
            _ => { if let Some(segment) = path.get_seg(index) { result.push((index, segment)); }
                previous = *points(*el).last().unwrap(); }
        }
    }
    result
}

pub(super) fn nearest(path: &BezPath, world: Affine, point: Point, tolerance: f64) -> Option<(usize, f64, Point, f64)> {
    segments(path).into_iter().filter_map(|(index, segment)| {
        let segment = world * segment;
        let nearest = segment.nearest(point, 1e-6);
        (nearest.distance_sq <= tolerance * tolerance).then(|| (index, nearest.t, segment.eval(nearest.t), nearest.distance_sq))
    }).min_by(|a, b| a.3.total_cmp(&b.3))
}

pub(super) fn inserted(path: &BezPath, index: usize, t: f64) -> Result<BezPath> {
    if !(0. < t && t < 1.) { return Err(Error::new("INVALID_PARAMETER", "Double-click away from an existing node")); }
    let segment = segments(path).into_iter().find(|(i, _)| *i == index)
        .ok_or_else(|| Error::new("INVALID_INDEX", "Double-click a path edge"))?.1;
    let mut elements = path.elements().to_vec();
    if matches!(elements[index], PathEl::ClosePath) {
        elements.insert(index, PathEl::LineTo(segment.eval(t)));
    } else {
        elements.splice(index..=index, [segment.subsegment(0. ..t).as_path_el(), segment.subsegment(t..1.).as_path_el()]);
    }
    Ok(BezPath::from_vec(elements))
}

fn cubic(segment: PathSeg) -> CubicBez {
    match segment {
        PathSeg::Cubic(c) => c,
        PathSeg::Quad(q) => CubicBez::new(q.p0, q.p0 + (q.p1 - q.p0) * (2. / 3.), q.p2 + (q.p1 - q.p2) * (2. / 3.), q.p2),
        PathSeg::Line(l) => CubicBez::new(l.p0, l.eval(1. / 3.), l.eval(2. / 3.), l.p1),
    }
}

fn joined(a: PathSeg, b: PathSeg) -> PathSeg {
    if matches!((a, b), (PathSeg::Line(_), PathSeg::Line(_))) { return PathSeg::Line(Line::new(a.eval(0.), b.eval(1.))); }
    let (a, b) = (cubic(a), cubic(b));
    PathSeg::Cubic(CubicBez::new(a.p0, a.p1, b.p2, b.p3))
}

/// Delete an anchor and join its neighbours, keeping the surviving anchors'
/// adjacent tangents. Other subpaths and closure remain intact.
pub(super) fn deleted(path: &BezPath, index: usize) -> Result<BezPath> {
    let (start, end) = range(path, index);
    let closed = matches!(path.elements()[end - 1], PathEl::ClosePath);
    let duplicate = closed && end > start + 2 && duplicate_anchor(path, end - 2);
    let anchors: Vec<(usize, Point)> = (start..end).filter(|i| !duplicate || *i != end - 2)
        .filter_map(|i| points(path.elements()[i]).last().map(|p| (i, *p))).collect();
    let index = if duplicate && index == end - 2 { start } else { index };
    let removed = anchors.iter().position(|(i, _)| *i == index).ok_or_else(|| Error::new("INVALID_INDEX", "Select an anchor, not a control handle"))?;
    if anchors.len() <= if closed { 3 } else { 2 } {
        return Err(Error::new("MINIMUM_NODES", "Keep at least three anchors in a closed path or two in an open path"));
    }
    let edges: Vec<PathSeg> = segments(path).into_iter().filter(|(i, _)| *i > start && *i < end).map(|(_, s)| s).collect();
    let remaining: Vec<usize> = (0..anchors.len()).filter(|i| *i != removed).collect();
    let mut replacement = vec![PathEl::MoveTo(anchors[remaining[0]].1)];
    for n in 0..if closed { remaining.len() } else { remaining.len() - 1 } {
        let from = remaining[n];
        let to = remaining[(n + 1) % remaining.len()];
        let edge = if to == (from + 1) % anchors.len() { edges[from] }
            else { joined(edges[from], edges[(from + 1) % edges.len()]) };
        replacement.push(edge.as_path_el());
    }
    if closed { replacement.push(PathEl::ClosePath); }
    let mut elements = path.elements().to_vec();
    elements.splice(start..end, replacement);
    Ok(BezPath::from_vec(elements))
}

pub(super) fn operations(doc: &Document, id: &str, path: &BezPath) -> Result<Vec<Operation>> {
    doc.require_editable(id)?;
    let object = doc.object(id)?;
    if object.local_path()?.elements() == path.elements() { return Ok(vec![]); }
    let mut result = vec![];
    if !matches!(object.geometry, Geometry::Path { .. }) { result.push(Operation::ObjectToPath { object_id: id.into() }); }
    result.push(Operation::PathSet { object_id: id.into(), elements: path.iter().map(PathElement::from).collect(), fill_rule: None });
    Ok(result)
}

pub(super) fn projected(doc: &Document, id: &str, path: &BezPath) -> Result<Object> {
    doc.require_editable(id)?;
    let mut object = doc.object(id)?.clone();
    let fill_rule = object.fill_rule();
    object.geometry = Geometry::Path { d: path.to_svg(), fill_rule };
    let bounds = path.bounding_box();
    object.width_mm = bounds.width(); object.height_mm = bounds.height();
    Ok(object)
}

pub(super) fn visible(doc: &Document, object: &Object) -> bool {
    object.visible && object.style.opacity > 0. && object.parent_id.as_deref()
        .is_none_or(|id| doc.object(id).is_ok_and(|parent| visible(doc, parent)))
}

pub(super) struct Snap { pub point: Point, pub anchor: bool }

#[cfg(test)]
pub(super) fn hit(doc: &Document, page: &str, point: Point, tolerance: f64) -> Option<String> {
    // In the shape tool prefer the closest contour over a nearby filled object.
    // This makes a thin outer border selectable beside an inset black field.
    let mut closest: Option<(f64, String)> = None;
    for object in doc.objects.iter().rev().filter(|o| o.on_canvas(page) && supported(o) && visible(doc, o)) {
        let (Ok(path), Ok(world)) = (object.local_path(), doc.world_transform(object)) else { continue; };
        if let Some((_, _, p, distance)) = nearest(&path, world, point, tolerance) {
            if doc.inside_clips(object,p).unwrap_or(false) && closest.as_ref().is_none_or(|(d, _)| distance < *d) {
                closest = Some((distance, object.id.clone()));
            }
        }
    }
    closest.map(|(_, id)| id).or_else(|| doc.hit_test(page,[point.x,point.y],tolerance))
}

#[cfg(test)]
pub(super) fn snap(doc: &Document, page: &str, point: Point, tolerance: f64) -> Option<Snap> {
    let mut anchor: Option<(f64, Point)> = None;
    let mut edge: Option<(f64, Point)> = None;
    for object in doc.objects.iter().filter(|o| o.on_canvas(page) && supported(o) && visible(doc, o)) {
        let (Ok(path), Ok(world)) = (object.local_path(), doc.world_transform(object)) else { continue; };
        for el in path.iter() {
            if let Some(p) = points(el).last() {
                let p = world * *p; let distance = point.distance(p);
                if distance <= tolerance && doc.inside_clips(object,p).unwrap_or(false) && anchor.is_none_or(|(d, _)| distance < d) { anchor = Some((distance, p)); }
            }
        }
        if let Some((_, _, p, distance)) = nearest(&path, world, point, tolerance) {
            if doc.inside_clips(object,p).unwrap_or(false) && edge.is_none_or(|(d, _)| distance < d) { edge = Some((distance, p)); }
        }
    }
    anchor.map(|(_, point)| Snap { point, anchor: true }).or_else(|| edge.map(|(_, point)| Snap { point, anchor: false }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tangent_arms_belong_to_the_selected_anchor_including_closed_and_compound_paths() {
        let path = BezPath::from_svg("M0 0 C10 0 20 10 20 20 C10 20 -10 0 0 0 Z M50 50 Q60 40 70 50").unwrap();
        let arms = control_arms(&path, 0);
        assert_eq!(arms.len(), 2);
        assert!(arms.iter().all(|a| a.anchor == Point::ORIGIN));
        assert_eq!((arms[0].index, arms[0].point, arms[0].control), (1, 0, Point::new(10., 0.)));
        assert_eq!((arms[1].index, arms[1].point, arms[1].control), (2, 1, Point::new(-10., 0.)));
        let arms = control_arms(&path, 1);
        assert_eq!(arms.len(), 2);
        assert!(arms.iter().all(|a| a.anchor == Point::new(20., 20.)));
        for (index, anchor) in [(4, Point::new(50., 50.)), (5, Point::new(70., 50.))] {
            let arms = control_arms(&path, index);
            assert_eq!(arms.len(), 1);
            assert_eq!((arms[0].index, arms[0].point, arms[0].anchor, arms[0].control), (5, 0, anchor, Point::new(60., 40.)));
        }
    }

    #[test]
    fn split_cubic_and_implicit_closing_edge_keep_the_original_geometry() {
        let path = BezPath::from_svg("M0 0 C10 -20 30 40 50 0 L60 30 Z").unwrap();
        let split = inserted(&path, 1, 0.37).unwrap();
        let original = path.get_seg(1).unwrap();
        for i in 0..=100 {
            let t = i as f64 / 100.;
            let actual = if t <= 0.37 { split.get_seg(1).unwrap().eval(t / 0.37) }
                else { split.get_seg(2).unwrap().eval((t - 0.37) / 0.63) };
            assert!(actual.distance(original.eval(t)) < 1e-10);
        }
        let closing = inserted(&path, 3, 0.5).unwrap();
        assert_eq!(closing.elements()[3], PathEl::LineTo(Point::new(30., 15.)));
        assert!(matches!(closing.elements().last(), Some(PathEl::ClosePath)));
    }

    #[test]
    fn moving_closed_start_updates_duplicate_endpoint_and_adjacent_handles() {
        let path = BezPath::from_svg("M0 0 C10 0 20 10 20 20 C10 20 -10 0 0 0 Z").unwrap();
        let moved = moved(&path, 0, 0, Point::new(3., 4.)).unwrap();
        assert_eq!(moved.elements()[0], PathEl::MoveTo(Point::new(3., 4.)));
        assert_eq!(moved.elements()[1], PathEl::CurveTo(Point::new(13., 4.), Point::new(20., 10.), Point::new(20., 20.)));
        assert_eq!(moved.elements()[2], PathEl::CurveTo(Point::new(10., 20.), Point::new(-7., 4.), Point::new(3., 4.)));
        assert!(duplicate_anchor(&moved, 2));
    }

    #[test]
    fn delete_start_or_interior_preserves_other_subpaths_and_minimum_nodes() {
        let path = BezPath::from_svg("M0 0 L20 0 L20 20 L0 20 Z M50 50 L60 60").unwrap();
        for index in [0, 1, 3] {
            let result = deleted(&path, index).unwrap();
            assert_eq!(&result.elements()[5..], &path.elements()[5..]);
            assert!(matches!(result.elements()[4], PathEl::ClosePath));
            assert!(deleted(&result, 0).is_err());
        }
    }

    #[test]
    fn nearest_uses_world_geometry_with_nonuniform_transform() {
        let path = BezPath::from_svg("M0 0 Q20 40 40 0").unwrap();
        let world = Affine::new([2., 0.3, 0.4, 0.7, 50., 70.]);
        let point = world * path.get_seg(1).unwrap().eval(0.43);
        let (_, t, found, _) = nearest(&path, world, point, 0.01).unwrap();
        assert!((t - 0.43).abs() < 1e-5 && found.distance(point) < 1e-5);
    }
}
