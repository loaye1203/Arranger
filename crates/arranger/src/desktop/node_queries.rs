use super::nodes::{self, Snap};
use arranger_core::Document;
use kurbo::{ParamCurve, ParamCurveNearest, PathEl, PathSeg, Point, Rect};
use std::sync::{Arc, Weak};

struct Contour {
    index: usize,
    bounds: Rect,
    anchors: Vec<Point>,
    edges: Vec<(Rect, PathSeg)>,
}

fn near(bounds: Rect, point: Point, tolerance: f64) -> bool {
    point.x >= bounds.x0 - tolerance && point.x <= bounds.x1 + tolerance
        && point.y >= bounds.y0 - tolerance && point.y <= bounds.y1 + tolerance
}

// The control polygon encloses the entire Bezier segment, including its extrema.
fn edge_bounds(edge: PathSeg) -> Rect {
    match edge {
        PathSeg::Line(line) => Rect::from_points(line.p0, line.p1),
        PathSeg::Quad(quad) => Rect::from_points(quad.p0, quad.p1).union_pt(quad.p2),
        PathSeg::Cubic(cubic) => Rect::from_points(cubic.p0, cubic.p1)
            .union_pt(cubic.p2)
            .union_pt(cubic.p3),
    }
}

impl Contour {
    fn nearest(&self, point: Point, tolerance: f64) -> Option<(Point, f64)> {
        self.edges.iter()
            .filter(|(bounds, _)| near(*bounds, point, tolerance))
            .filter_map(|(_, edge)| {
                let result = edge.nearest(point, 1e-6);
                (result.distance_sq <= tolerance * tolerance)
                    .then(|| (edge.eval(result.t), result.distance_sq))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }
}

#[derive(Default)]
pub(super) struct NodeQueries {
    source: Weak<Document>,
    page: String,
    contours: Vec<Contour>,
}

impl NodeQueries {
    pub fn clear(&mut self) {
        if !self.page.is_empty() {
            *self = Self::default();
        }
    }

    pub fn matches(&self, source: &Arc<Document>, page: &str) -> bool {
        self.page == page && std::ptr::eq(self.source.as_ptr(), Arc::as_ptr(source))
    }

    pub fn prepare(&mut self, source: &Arc<Document>, page: &str) {
        if self.matches(source, page) {
            return;
        }
        self.contours = source.objects.iter().enumerate().filter_map(|(index, object)| {
            if !object.on_canvas(page) || !nodes::supported(object) || !nodes::visible(source, object) {
                return None;
            }
            let path = object.local_path().ok()?;
            let world = source.world_transform(object).ok()?;
            let anchors: Vec<_> = path.iter().filter_map(|el| match el {
                PathEl::MoveTo(p) | PathEl::LineTo(p) | PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => Some(world * p),
                PathEl::ClosePath => None,
            }).collect();
            let edges: Vec<_> = nodes::segments(&path).into_iter().map(|(_, edge)| {
                let edge = world * edge;
                (edge_bounds(edge), edge)
            }).collect();
            let first = *anchors.first()?;
            let anchors_box = anchors.iter().fold(Rect::from_points(first, first), |r, &p| r.union_pt(p));
            let bounds = edges.iter().fold(anchors_box, |r, (b, _)| r.union(*b));
            Some(Contour { index, bounds, anchors, edges })
        }).collect();
        self.page = page.into();
        self.source = Arc::downgrade(source);
    }

    pub fn hit(&mut self, doc: &Arc<Document>, page: &str, point: Point, tolerance: f64) -> Option<String> {
        self.prepare(doc, page);
        self.contours.iter().rev()
            .filter(|c| near(c.bounds, point, tolerance))
            .filter_map(|contour| {
                let (at, distance) = contour.nearest(point, tolerance)?;
                let object = &doc.objects[contour.index];
                doc.inside_clips(object, at).unwrap_or(false).then_some((&object.id, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id.clone())
            .or_else(|| doc.hit_test(page, [point.x, point.y], tolerance))
    }

    pub fn snap(&mut self, doc: &Arc<Document>, page: &str, point: Point, tolerance: f64) -> Option<Snap> {
        self.prepare(doc, page);
        let candidates = || self.contours.iter().filter(|c| near(c.bounds, point, tolerance));
        let mut anchor = None;
        for contour in candidates() {
            for &at in &contour.anchors {
                let distance = point.distance(at);
                if distance <= tolerance && anchor.is_none_or(|(_, best)| distance < best)
                    && doc.inside_clips(&doc.objects[contour.index], at).unwrap_or(false) {
                    anchor = Some((at, distance));
                }
            }
        }
        if let Some((point, _)) = anchor {
            return Some(Snap { point, anchor: true });
        }
        candidates().filter_map(|contour| {
            let (at, distance) = contour.nearest(point, tolerance)?;
            doc.inside_clips(&doc.objects[contour.index], at).unwrap_or(false).then_some((at, distance))
        })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(point, _)| Snap { point, anchor: false })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arranger_core::{Object, WORKSPACE_PAGE};
    use serde_json::json;
    use std::time::Instant;

    fn scene(count:usize)->Arc<Document> {
        let mut doc=Document::new("node-query".into(),210.,297.).unwrap();
        let base:Object=serde_json::from_value(json!({"id":"rect","page_id":"page-1","width_mm":8.,"height_mm":8.,"geometry":{"kind":"rect"}})).unwrap();
        for index in 0..count {
            let mut object=base.clone();object.id=format!("r{index}");object.x_mm=(index%60) as f64*10.;object.y_mm=(index/60) as f64*10.;
            doc.objects.push(object);
        }
        Arc::new(doc)
    }
    fn same(actual:Option<Snap>,expected:Option<Snap>){
        assert_eq!(actual.map(|s|(s.point,s.anchor)),expected.map(|s|(s.point,s.anchor)));
    }

    #[test]
    fn cached_queries_match_original_after_page_changes_edits_and_undo(){
        let mut source=scene(8);
        let doc=Arc::make_mut(&mut source);
        let mut group=doc.objects[0].clone();group.id="group".into();group.geometry=arranger_core::Geometry::Group;
        group.transform=[1.2,0.15,0.3,0.8,2.,4.];group.clip=Some(arranger_core::Clip::Rect{rect_mm:[0.,0.,9.,6.]});
        doc.objects.push(group);doc.objects[0].parent_id=Some("group".into());doc.objects[0].geometry=serde_json::from_value(json!({"kind":"path","d":"M0 0 C-4 15 20 -5 10 2 Q12 9 0 8 Z M12 1 L17 6"})).unwrap();
        doc.objects[1].visible=false;doc.objects[2].page_id=WORKSPACE_PAGE.into();doc.objects[3].style.opacity=0.;
        let mut shared=doc.objects[2].clone();shared.id="tied".into();doc.objects.push(shared);
        let mut next=doc.clone();next.objects[2].x_mm+=0.25;
        let next=Arc::new(next);let mut queries=NodeQueries::default();
        for (doc,page) in [(&source,"page-1"),(&source,"other"),(&next,"page-1"),(&source,"page-1")] {
            for x in -2..35 {for y in -1..13{for tolerance in [0.,0.5,2.] {
                let point=Point::new(x as f64,y as f64);
                same(queries.snap(doc,page,point,tolerance),nodes::snap(doc,page,point,tolerance));
                assert_eq!(queries.hit(doc,page,point,tolerance),nodes::hit(doc,page,point,tolerance));
            }}}
        }
    }

    #[test]
    fn repeated_queries_on_3000_objects_keep_anchor_and_edge_results(){
        let doc=scene(3000);let mut cache=NodeQueries::default();
        let cold=Instant::now();cache.prepare(&doc,"page-1");let cold=cold.elapsed();
        let points:Vec<_>=(0..128).map(|i|Point::new((i*37%600)as f64+0.125,(i*71%500)as f64+0.25)).collect();
        let start=Instant::now();let expected:Vec<_>=points.iter().map(|&p|nodes::snap(&doc,"page-1",p,0.8)).collect();let old=start.elapsed();
        let start=Instant::now();let actual:Vec<_>=points.iter().map(|&p|cache.snap(&doc,"page-1",p,0.8)).collect();let new=start.elapsed();
        assert!(actual.iter().flatten().any(|s|s.anchor)&&actual.iter().flatten().any(|s|!s.anchor)&&actual.iter().any(Option::is_none));
        for (a,b) in actual.into_iter().zip(expected){same(a,b);}
        println!("3000 objects / 128 snap queries: build {cold:?}, original {old:?}, cached {new:?}");
    }
}
