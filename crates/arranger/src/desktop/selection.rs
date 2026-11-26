use super::*;
use arranger_core::Document;
use kurbo::{Affine, Point};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Mode { #[default] Scale, Rotate }

#[derive(Default)]
pub(super) struct SelectionBounds {
    scene: String,
    pub outlines: Vec<kurbo::Rect>,
    pub union: Option<kurbo::Rect>,
}

impl SelectionBounds {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    // set_selection clears this cache when membership or selection order changes.
    // The scene key also changes for every distinct in-progress geometry preview.
    pub fn get(&mut self, doc: &Document, ids: &[String], scene: &str) -> &Self {
        if self.scene == scene {
            return self;
        }
        self.outlines.clear();
        let bounds = |o: &arranger_core::Object| {
            let [x, y, width, height] = doc.bounds(o).ok()?;
            Some(kurbo::Rect::new(x, y, x + width, y + height))
        };
        if ids.len() <= 4 {
            self.outlines.extend(ids.iter().filter_map(|id| doc.object(id).ok().and_then(bounds)));
        } else {
            let mut selected: std::collections::HashMap<_, _> =
                ids.iter().map(|id| (id.as_str(), None)).collect();
            for object in &doc.objects {
                if let Some(slot) = selected.get_mut(object.id.as_str()) {
                    *slot = bounds(object);
                }
            }
            self.outlines.extend(ids.iter().filter_map(|id| selected[id.as_str()]));
        }
        self.union = self.outlines.iter().copied().reduce(|a, b| a.union(b));
        self.scene.clear();
        self.scene.push_str(scene);
        self
    }
}

#[test]
fn bounds_follow_selection_order_and_preview_revisions() {
    let mut doc = Document::new("bounds".into(), 210., 297.).unwrap();
    for number in 0..6 {
        let object = serde_json::from_value(json!({
            "id":format!("r{number}"), "page_id":"page-1", "x_mm":number*10, "y_mm":2,
            "width_mm":3, "height_mm":4, "geometry":{"kind":"rect"}
        })).unwrap();
        doc.objects.push(object);
    }
    let mut bounds = SelectionBounds::default();
    let ids: Vec<_> = ["r4", "r1", "gone", "r3", "r1", "r5"].into_iter().map(str::to_owned).collect();
    let view = bounds.get(&doc, &ids, "source");
    assert_eq!(view.outlines.iter().map(|r| r.x0).collect::<Vec<_>>(), vec![40.,10.,30.,10.,50.]);
    assert_eq!(view.union.unwrap(), kurbo::Rect::new(10.,2.,53.,6.));
    doc.objects[5].x_mm = 60.;
    assert_eq!(bounds.get(&doc, &ids, "preview").union.unwrap().x1, 63.);
    bounds.clear();
    assert_eq!(bounds.get(&doc, &ids[..1], "preview").outlines.len(), 1);
    bounds.clear();
    assert!(bounds.get(&doc, &[], "preview").union.is_none());
}

// Corners retain their existing clockwise indices; sides start at the top.
pub(super) fn handles(r:kurbo::Rect)->[Point;8] {
    let c=r.center();
    [Point::new(r.x0,r.y0),Point::new(r.x1,r.y0),Point::new(r.x1,r.y1),Point::new(r.x0,r.y1),
     Point::new(c.x,r.y0),Point::new(r.x1,c.y),Point::new(c.x,r.y1),Point::new(r.x0,c.y)]
}
pub(super) fn resize(r:kurbo::Rect,index:usize,delta:kurbo::Vec2)->Affine {
    let points=handles(r);let opposite=if index<4{(index+2)%4}else{4+(index-4+2)%4};
    let fixed=points[opposite];let diagonal=points[index]-fixed;
    let ratio=|distance:f64,change:f64|if distance.abs()>1e-10{((distance+change)/distance).max(0.01)}else{1.};
    let (sx,sy)=if index<4 {
        // Project onto the diagonal, so even an off-diagonal drag is uniform.
        let s=if diagonal.hypot2()>1e-20{(1.+delta.dot(diagonal)/diagonal.hypot2()).max(0.01)}else{1.};(s,s)
    }else if index==4||index==6{(1.,ratio(diagonal.y,delta.y))}else{(ratio(diagonal.x,delta.x),1.)};
    Affine::translate(fixed.to_vec2())*Affine::scale_non_uniform(sx,sy)*Affine::translate(-fixed.to_vec2())
}

// Use the same operations as the final batch, but keep the preview private.
pub(super) fn projected(doc:&Document,ops:&[Operation])->arranger_core::Result<Vec<arranger_core::Object>> {
    let mut objects=Vec::with_capacity(ops.len());
    for op in ops {
        let Operation::ObjectUpdate{object_id,patch}=op else{unreachable!("selection transform only updates objects")};
        let mut o=doc.object(object_id)?.clone();
        if let Some(x)=patch.x_mm{o.x_mm=x;}if let Some(y)=patch.y_mm{o.y_mm=y;}
        if let Some(matrix)=patch.transform{o.transform=matrix;}objects.push(o);
    }
    Ok(objects)
}

pub(super) fn rotate_arrow(painter:&egui::Painter,center:Pos2,index:usize) {
    let start=std::f32::consts::PI*(1.+index as f32/2.);
    let points=(0..=12).map(|i|{let a=start+i as f32/12.*std::f32::consts::FRAC_PI_2;center+Vec2::angled(a)*7.}).collect::<Vec<_>>();
    painter.add(egui::Shape::line(points.clone(),egui::Stroke::new(3.5,Color32::WHITE)));
    painter.add(egui::Shape::line(points.clone(),egui::Stroke::new(1.8,ACCENT)));
    for (tip,other) in [(points[0],points[1]),(points[12],points[11])] {
        let direction=(tip-other).normalized();let base=tip-direction*5.;let side=Vec2::new(-direction.y,direction.x)*3.;
        painter.add(egui::Shape::convex_polygon(vec![tip,base+side,base-side],ACCENT,egui::Stroke::new(0.7,Color32::WHITE)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn near(a:Point,b:Point){assert!(a.distance(b)<1e-9,"{a:?} != {b:?}");}
    #[test]
    fn corners_preserve_ratio_and_opposite_anchor_even_off_diagonal() {
        let r=kurbo::Rect::new(20.,30.,60.,50.);let p=handles(r);
        for i in 0..4{let a=resize(r,i,kurbo::Vec2::new(9.,-7.));let [sx,_,_,sy,..]=a.as_coeffs();assert_eq!(sx,sy);assert!(sx>0.);near(a*p[(i+2)%4],p[(i+2)%4]);}
        let a=resize(r,2,kurbo::Vec2::new(20.,10.));near(a*p[2],Point::new(80.,60.));
    }
    #[test]
    fn all_four_side_handles_only_stretch_their_axis_and_fix_opposite_side() {
        let r=kurbo::Rect::new(20.,30.,60.,50.);let p=handles(r);
        for i in 4..8{let a=resize(r,i,kurbo::Vec2::new(9.,7.));near(a*p[4+(i-4+2)%4],p[4+(i-4+2)%4]);let [sx,_,_,sy,..]=a.as_coeffs();if i==4||i==6{assert_eq!(sx,1.);}else{assert_eq!(sy,1.);}}
        assert!(resize(r,2,kurbo::Vec2::new(-1000.,-1000.)).determinant()>0.);
        assert!(resize(kurbo::Rect::new(5.,5.,5.,20.),2,kurbo::Vec2::new(0.,5.)).as_coeffs().iter().all(|v|v.is_finite()));
    }
}
