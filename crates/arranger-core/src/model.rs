use crate::{Error, FillRule, Result, TextOnPath, TextLayoutOptions};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use kurbo::{Affine, BezPath, Ellipse, ParamCurveNearest, Point, Rect, RoundedRect, Shape};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub fn sha256(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128
        && id.bytes().next().is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
}
pub(crate) fn default_true() -> bool { true }
pub(crate) fn is_true(value:&bool)->bool{*value}
fn identity() -> [f64; 6] { [1., 0., 0., 1., 0., 0.] }
fn default_miterlimit() -> f64 { 4. }
fn is_default_miterlimit(value: &f64) -> bool { *value==4. }
/// Document-wide pasteboard. This is an object owner, never a printable page.
pub const WORKSPACE_PAGE: &str = "_workspace";

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum LineCap { #[default] Butt, Round, Square }
impl LineCap {
    pub fn is_default(&self)->bool { *self==Self::Butt }
    pub fn svg(self)->&'static str {match self {Self::Butt=>"butt",Self::Round=>"round",Self::Square=>"square"}}
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum LineJoin { #[default] Miter, Round, Bevel }
impl LineJoin {
    pub fn is_default(&self)->bool { *self==Self::Miter }
    pub fn svg(self)->&'static str {match self {Self::Miter=>"miter",Self::Round=>"round",Self::Bevel=>"bevel"}}
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum ImageFit { #[default] Contain, Stretch, Cover }
impl ImageFit { pub fn is_default(&self)->bool { *self==Self::Contain } }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub id: String,
    pub width_mm: f64,
    pub height_mm: f64,
}

/// Native page-fixed elliptical pen; angle0 is vertical and90 horizontal.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FixedStrokeNib {pub stretch_percent:u16,pub angle_millionth_degrees:i32}
impl FixedStrokeNib {
    pub fn basis(self)->Affine {
        let(s,c)=match self.angle_millionth_degrees.rem_euclid(360_000_000){0=>(1.,0.),90_000_000=>(0.,1.),180_000_000=>(-1.,0.),270_000_000=>(0.,-1.),a=>(90.-f64::from(a)/1_000_000.).to_radians().sin_cos()};
        let ratio=f64::from(self.stretch_percent)/100.;Affine::new([c,s,-s*ratio,c*ratio,0.,0.])
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Style {
    pub fill: String,
    pub stroke: String,
    pub stroke_width_mm: f64,
    #[serde(default="default_true",skip_serializing_if="is_true")]
    pub stroke_scale_with_object: bool,
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub stroke_fixed_nib: Option<FixedStrokeNib>,
    pub opacity: f64,
    #[serde(default,skip_serializing_if="LineCap::is_default")]
    pub stroke_linecap: LineCap,
    #[serde(default,skip_serializing_if="LineJoin::is_default")]
    pub stroke_linejoin: LineJoin,
    #[serde(default="default_miterlimit",skip_serializing_if="is_default_miterlimit")]
    pub stroke_miterlimit: f64,
}
impl Style {pub(crate) fn page_pen_basis(&self)->Affine{self.stroke_fixed_nib.map_or(Affine::IDENTITY,FixedStrokeNib::basis)}}
impl Default for Style {
    fn default() -> Self {
        Self { fill: "#287ac7".into(), stroke: "none".into(), stroke_width_mm: 0.,stroke_scale_with_object:true,stroke_fixed_nib:None, opacity: 1.,stroke_linecap:Default::default(),stroke_linejoin:Default::default(),stroke_miterlimit:4. }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Geometry {
    Rect { #[serde(default)] radius_mm: f64 },
    Ellipse,
    Path {
        d: String,
        // Omitting the old default preserves canonical hashes of version 1 files.
        #[serde(default, skip_serializing_if = "FillRule::is_nonzero")]
        fill_rule: FillRule,
    },
    Text {
        content: String, font_family: String, font_size_mm: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text_path: Option<TextOnPath>,
        #[serde(default, skip_serializing_if = "TextLayoutOptions::is_default")]
        layout: TextLayoutOptions,
    },
    Image { resource_id: String, #[serde(default)] clip_mm: Option<[f64; 4]>,
        #[serde(default,skip_serializing_if="ImageFit::is_default")] fit: ImageFit },
    Group,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Object {
    pub id: String,
    pub page_id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub x_mm: f64,
    #[serde(default)]
    pub y_mm: f64,
    pub width_mm: f64,
    pub height_mm: f64,
    #[serde(default = "identity")]
    pub transform: [f64; 6],
    #[serde(default)]
    pub style: Style,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub clip: Option<crate::Clip>,
    pub geometry: Geometry,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    pub mime: String,
    pub sha256: String,
    pub data_base64: String,
}
impl Resource {
    pub fn png(data: &[u8]) -> Result<Self> {
        crate::resource::inspect(data,"image/png")?;
        Ok(Self { mime: "image/png".into(), sha256: sha256(data), data_base64: STANDARD.encode(data) })
    }
    pub fn image(data: &[u8]) -> Result<Self> { crate::resource::from_bytes(data) }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub id: String,
    pub unit: String,
    pub pages: Vec<Page>,
    // Vec order is stacking order within each parent. IDs never depend on indices.
    pub objects: Vec<Object>,
    pub resources: BTreeMap<String, Resource>,
}

impl Document {
    pub fn new(id: String, width_mm: f64, height_mm: f64) -> Result<Self> {
        let doc = Self { id, unit: "mm".into(), pages: vec![Page { id: "page-1".into(), width_mm, height_mm }], objects: vec![], resources: BTreeMap::new() };
        doc.validate()?;
        Ok(doc)
    }
    pub fn hash(&self) -> String { sha256(&serde_json::to_vec(self).expect("validated document serializes")) }
    pub fn object(&self, id: &str) -> Result<&Object> {
        self.objects.iter().find(|o| o.id == id).ok_or_else(|| Error::new("OBJECT_NOT_FOUND", format!("Unknown object {id}")))
    }
    pub fn page(&self, id: &str) -> Result<&Page> {
        self.pages.iter().find(|p| p.id == id).ok_or_else(|| Error::new("PAGE_NOT_FOUND", format!("Unknown page {id}")))
    }
    pub fn require_editable(&self, id: &str) -> Result<()> {
        let mut object = self.object(id)?;
        loop {
            if object.locked { return Err(Error::new("OBJECT_LOCKED", format!("{} is locked", object.id))); }
            match &object.parent_id { Some(parent) => object = self.object(parent)?, None => break }
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<()> {
        if !valid_id(&self.id) || self.unit != "mm" || self.pages.is_empty() {
            return Err(Error::new("INVALID_DOCUMENT", "Document needs a valid ID, mm units and at least one page"));
        }
        let pages: BTreeSet<_> = self.pages.iter().map(|p| p.id.as_str()).collect();
        if pages.len() != self.pages.len() { return Err(Error::new("DUPLICATE_ID", "Duplicate page ID")); }
        for p in &self.pages {
            if !valid_id(&p.id) || p.id==WORKSPACE_PAGE || ![p.width_mm, p.height_mm].iter().all(|x| x.is_finite() && *x > 0.) {
                return Err(Error::new("INVALID_PAGE", "Page dimensions must be finite and positive millimetres"));
            }
        }
        let objects: BTreeMap<_, _> = self.objects.iter().map(|o| (o.id.as_str(), o)).collect();
        if objects.len() != self.objects.len() { return Err(Error::new("DUPLICATE_ID", "Duplicate object ID")); }
        for (id, resource) in &self.resources {
            if !valid_id(id) { return Err(Error::new("INVALID_RESOURCE", "Invalid resource ID")); }
            crate::resource::validate(resource)?;
        }
        for o in &self.objects {
            self.validate_object_fields(o,pages.contains(o.page_id.as_str()) || o.page_id==WORKSPACE_PAGE)?;
            let mut visited = BTreeSet::from([o.id.as_str()]);
            let mut parent = &o.parent_id;
            while let Some(id) = parent {
                if !visited.insert(id) { return Err(Error::new("CYCLIC_GROUP", "Group parent cycle")); }
                let p = objects.get(id.as_str()).ok_or_else(|| Error::new("INVALID_PARENT", "Parent does not exist"))?;
                if !matches!(p.geometry, Geometry::Group) || p.page_id != o.page_id {
                    return Err(Error::new("INVALID_PARENT", "Parent must be a group on the same page"));
                }
                parent = &p.parent_id;
            }
            self.validate_object_geometry(o)?;
        }
        // Cross-object geometry is safe to inspect only after every parent
        // chain above has passed its cycle/reference checks.
        for o in &self.objects { if let Geometry::Text { text_path: Some(binding), .. } = &o.geometry {
            crate::text::guide(self, o, binding)?;
        }}
        Ok(())
    }
    fn validate_object_fields(&self,o:&Object,valid_page:bool)->Result<()> {
        if let Some(nib)=o.style.stroke_fixed_nib {if !(1..100).contains(&nib.stretch_percent)||o.style.stroke_scale_with_object||o.style.stroke!="none"&&!matches!(o.geometry,Geometry::Rect{..}|Geometry::Ellipse|Geometry::Path{..}|Geometry::Group){return Err(Error::new("INVALID_STYLE","An elliptical fixed nib requires1..99 percent, page-fixed mode and vector geometry"));}}
        if !o.style.stroke_scale_with_object&&o.style.stroke!="none"&&matches!(o.geometry,Geometry::Text{..}){return Err(Error::new("INVALID_STYLE","Fixed-world strokes currently require vector geometry"));}
        if !o.style.stroke_miterlimit.is_finite() || o.style.stroke_miterlimit<1. {return Err(Error::new("INVALID_STYLE","Stroke miter limit must be finite and at least 1"));}
        if let Some(clip)=&o.clip {clip.validate()?;}
        if !valid_id(&o.id) || !valid_page
            || ![o.x_mm, o.y_mm, o.width_mm, o.height_mm, o.style.stroke_width_mm, o.style.opacity].iter().all(|x| x.is_finite())
            || !o.transform.iter().all(|x| x.is_finite()) || o.width_mm < 0. || o.height_mm < 0.
            || o.style.stroke_width_mm < 0. || !(0. ..=1.).contains(&o.style.opacity)
            || !color(&o.style.fill) || !color(&o.style.stroke) {
            return Err(Error::new("INVALID_OBJECT", format!("Invalid geometry, page, transform or style on {}", o.id)));
        }
        Ok(())
    }
    fn validate_object_geometry(&self,o:&Object)->Result<()> {
        if !o.style.stroke_scale_with_object&&o.style.stroke!="none"&&self.world_transform(o)?.determinant().abs()<1e-12{return Err(Error::new("INVALID_STYLE","Fixed-world strokes require a nonsingular world frame"));}
        match &o.geometry {
            Geometry::Rect { radius_mm } if !radius_mm.is_finite() || *radius_mm < 0. || *radius_mm > o.width_mm.min(o.height_mm) / 2. => {
                return Err(Error::new("INVALID_GEOMETRY", "Rectangle radius is outside its dimensions"));
            }
            Geometry::Rect { .. } | Geometry::Ellipse | Geometry::Text { .. } | Geometry::Image { .. } if o.width_mm <= 0. || o.height_mm <= 0. => {
                return Err(Error::new("INVALID_GEOMETRY", "Shape, text and image dimensions must be positive"));
            }
            Geometry::Path { d, .. } => { crate::path::parse(d)?; }
            Geometry::Text { content, font_family, font_size_mm, .. } => {
                crate::text::validate_text(content, font_family, *font_size_mm)?;
                crate::text::validate_options(o)?;
            }
            Geometry::Image { resource_id, clip_mm, .. } => {
                if !self.resources.contains_key(resource_id) { return Err(Error::new("RESOURCE_NOT_FOUND", resource_id)); }
                if clip_mm.is_some_and(|c| !c.iter().all(|x| x.is_finite()) || c[2] <= 0. || c[3] <= 0.) { return Err(Error::new("INVALID_CLIP", "Clip dimensions must be positive and finite")); }
                if let Some(rect_mm)=clip_mm {crate::Clip::Rect{rect_mm:*rect_mm}.validate()?;}
            }
            _ => {}
        }
        Ok(())
    }
    pub fn draft_text(&self,id:&str,geometry:&Geometry)->Result<Object> {
        self.require_editable(id)?;
        let mut object=self.object(id)?.clone();
        if !matches!(object.geometry,Geometry::Text{..}) || !matches!(geometry,Geometry::Text{..}) {return Err(Error::new("NOT_TEXT","Expected a text object"));}
        object.geometry=geometry.clone();self.validate_object_geometry(&object)?;
        if let Geometry::Text{text_path:Some(binding),..}=&object.geometry {crate::text::guide(self,&object,binding)?;}
        Ok(object)
    }
    pub fn world_transform(&self, object: &Object) -> Result<Affine> {
        let local = Affine::new(object.transform) * Affine::translate((object.x_mm, object.y_mm));
        match &object.parent_id { Some(id) => Ok(self.world_transform(self.object(id)?)? * local), None => Ok(local) }
    }
    pub(crate) fn children_by_parent(&self) -> Children<'_> {
        let mut children = Children::new();
        for object in &self.objects {
            if let Some(parent) = object.parent_id.as_deref() {
                children.entry(parent).or_default().push(object);
            }
        }
        children
    }

    fn leaf_bounds(&self, object: &Object, world: Affine) -> Result<[f64; 4]> {
        if matches!(object.geometry, Geometry::Ellipse) {
            let center = world * Point::new(object.width_mm/2., object.height_mm/2.);
            let [a,b,c,d,_,_] = world.as_coeffs();
            let rx=(a*object.width_mm/2.).hypot(c*object.height_mm/2.);
            let ry=(b*object.width_mm/2.).hypot(d*object.height_mm/2.);
            return Ok([center.x-rx,center.y-ry,2.*rx,2.*ry]);
        }
        let r = (world * self.object_path(object)?).bounding_box();
        Ok([r.x0, r.y0, r.width(), r.height()])
    }

    fn leaf_rect(&self, object: &Object, world: Affine, placement: bool) -> Result<Rect> {
        if placement && matches!(&object.geometry, Geometry::Text{content,..} if content.is_empty()) {
            return Ok(world.transform_rect_bbox(Rect::new(0.,0.,object.width_mm,object.height_mm)));
        }
        let [x, y, w, h] = self.leaf_bounds(object, world)?;
        Ok(Rect::new(x, y, x+w, y+h))
    }

    fn group_bounds(&self, group: &Object, world: Affine, children: &Children<'_>, placement: bool) -> Result<Rect> {
        let mut bounds: Option<Rect> = None;
        for &child in children.get(group.id.as_str()).into_iter().flatten() {
            let local = Affine::new(child.transform) * Affine::translate((child.x_mm, child.y_mm));
            let child_world = world * local;
            let r = if matches!(child.geometry, Geometry::Group) {
                let r = self.group_bounds(child, child_world, children, placement)?;
                if placement { r } else { Rect::new(r.x0, r.y0, r.x0+r.width(), r.y0+r.height()) }
            } else {
                self.leaf_rect(child, child_world, placement)?
            };
            bounds = Some(bounds.map_or(r, |b| b.union(r)));
        }
        Ok(bounds.unwrap_or(Rect::ZERO))
    }

    pub fn bounds(&self, object: &Object) -> Result<[f64; 4]> {
        let world = self.world_transform(object)?;
        if matches!(object.geometry, Geometry::Group) {
            let r = self.group_bounds(object, world, &self.children_by_parent(), false)?;
            return Ok([r.x0, r.y0, r.width(), r.height()]);
        }
        self.leaf_bounds(object, world)
    }

    pub(crate) fn placement_bounds(&self, object: &Object) -> Result<Rect> {
        let world = self.world_transform(object)?;
        if matches!(object.geometry, Geometry::Group) {
            return self.group_bounds(object, world, &self.children_by_parent(), true);
        }
        self.leaf_rect(object, world, true)
    }
    pub fn hit_test(&self, page: &str, point: [f64; 2], tolerance_mm: f64) -> Option<String> {
        self.hit_test_with_text_region(page, point, tolerance_mm, |_, _, _| false)
    }

    /// Include a text-tool pick region in the same clipped, front-to-back
    /// traversal as painted geometry. The point is in object-local mm.
    pub fn hit_test_with_text_region(
        &self, page: &str, point: [f64; 2], tolerance_mm: f64,
        text_region: impl Fn(&Object, &crate::text::TextLayout, Point) -> bool,
    ) -> Option<String> {
        self.objects.iter().rev().find_map(|o| {
            if !o.on_canvas(page) || !self.effectively_visible(o) || matches!(o.geometry, Geometry::Group) { return None; }
            let transform = self.world_transform(o).ok()?;
            if transform.determinant().abs() < f64::EPSILON { return None; }
            let p = transform.inverse() * Point::from((point[0], point[1]));
            if matches!(o.geometry,Geometry::Image{..}) && !Rect::new(0.,0.,o.width_mm,o.height_mm).contains(p) {return None;}
            if matches!(o.geometry,Geometry::Rect{..}) && o.style.stroke_scale_with_object {
                let pad=(tolerance_mm+o.style.stroke_width_mm/2.).abs();
                if p.x < -pad || p.y < -pad || p.x > o.width_mm+pad || p.y > o.height_mm+pad {return None;}
            }
            if !self.inside_clips(o,Point::from((point[0],point[1]))).ok()? {return None;}
            let path = if matches!(o.geometry, Geometry::Text { .. }) {
                let layout = crate::text::layout(self, o).ok()?;
                if text_region(o, &layout, p) { return Some(o.id.clone()); }
                layout.local_path()
            } else {
                o.local_path().ok()?
            };
            let inside = crate::path::contains(&path, o.fill_rule(), p);
            if inside && o.style.stroke_scale_with_object {return Some(o.id.clone());}
            let stroke_hit = if o.style.stroke_scale_with_object{path.segments().any(|s| s.nearest(p, 1e-4).distance_sq <= (tolerance_mm + o.style.stroke_width_mm / 2.).powi(2))}else{let pen=o.style.page_pen_basis();if o.style.stroke_fixed_nib.is_some()&&o.style.stroke!="none"&&o.style.stroke_width_mm>0.{let outline=self.page_stroke_outline(o).ok()?;let query=Point::new(point[0],point[1]);crate::path::contains(&outline,FillRule::NonZero,query)||outline.segments().any(|s|s.nearest(query,1e-4).distance_sq<=tolerance_mm.powi(2))}else{let world=pen.inverse()*transform*path.clone();let query=pen.inverse()*Point::new(point[0],point[1]);world.segments().any(|s|s.nearest(query,1e-4).distance_sq<=(tolerance_mm+o.style.stroke_width_mm/2.).powi(2))}};
            (inside || stroke_hit).then(|| o.id.clone())
        })
    }
    fn page_stroke_outline(&self,o:&Object)->Result<BezPath>{
        let basis=o.style.page_pen_basis();let geometry=basis.inverse()*self.page_vector_path(o)?;
        let cap=match o.style.stroke_linecap{LineCap::Butt=>kurbo::Cap::Butt,LineCap::Round=>kurbo::Cap::Round,LineCap::Square=>kurbo::Cap::Square};
        let mut style=kurbo::Stroke::new(o.style.stroke_width_mm);style.start_cap=cap;style.end_cap=cap;style.join=match o.style.stroke_linejoin{LineJoin::Miter=>kurbo::Join::Miter,LineJoin::Round=>kurbo::Join::Round,LineJoin::Bevel=>kurbo::Join::Bevel};style.miter_limit=o.style.stroke_miterlimit;
        Ok(basis*kurbo::stroke(geometry.iter(),&style,&kurbo::StrokeOpts::default(),1e-5))
    }
    pub(crate) fn page_vector_path(&self,o:&Object)->Result<BezPath>{
        let m=self.world_transform(o)?;let[a,b,c,d,_,_]=m.as_coeffs();let norm=a.abs().max(b.abs()).max(c.abs()).max(d.abs()).max(1.);Ok(m*o.local_path_at_tolerance(1e-4/norm)?)
    }
    fn effectively_visible(&self, object: &Object) -> bool {
        if !object.visible || object.style.opacity==0. { return false; }
        object.parent_id.as_ref().is_none_or(|id| self.object(id).is_ok_and(|o| self.effectively_visible(o)))
    }
    pub fn object_path(&self, object: &Object) -> Result<BezPath> {
        if matches!(object.geometry, Geometry::Text { .. }) {
            return Ok(crate::text::layout(self, object)?.local_path());
        }
        object.local_path()
    }
}

impl Object {
    pub fn on_canvas(&self,page:&str)->bool {self.page_id==page||self.page_id==WORKSPACE_PAGE}
    pub fn fill_rule(&self) -> FillRule {
        match self.geometry { Geometry::Path { fill_rule, .. } => fill_rule, _ => FillRule::NonZero }
    }
    pub fn local_path(&self) -> Result<BezPath> {
        self.local_path_at_tolerance(1e-4)
    }
    pub(crate) fn local_path_at_tolerance(&self,tolerance:f64)->Result<BezPath>{
        let rect = Rect::new(0., 0., self.width_mm, self.height_mm);
        Ok(match &self.geometry {
            Geometry::Rect { radius_mm } => if *radius_mm == 0. { rect.to_path(tolerance) }
                else { RoundedRect::from_rect(rect, *radius_mm).to_path(tolerance) },
            Geometry::Ellipse => {
                // kurbo's ellipse iterator returns to its start without Z;
                // retain the shape's intrinsic closure in editable path form.
                let mut path = Ellipse::from_rect(rect).to_path(tolerance);
                path.close_path(); path
            }
            Geometry::Path { d, .. } => crate::path::parse(d)?,
            Geometry::Image { clip_mm: Some([x, y, w, h]), .. } => {
                let clipped=Rect::new(*x,*y,x+w,y+h).intersect(rect);
                if clipped.is_zero_area() {BezPath::new()} else {clipped.to_path(1e-4)}
            }
            _ => rect.to_path(1e-4),
        })
    }
}

fn color(c: &str) -> bool {
    crate::paint::parse(c).is_ok()
}

pub(crate) type Children<'a> = std::collections::HashMap<&'a str, Vec<&'a Object>>;

pub struct DocumentPreview {
    source: std::sync::Arc<Document>,
    document: Document,
    indices: Vec<usize>,
    dependencies: Vec<usize>,
}
impl DocumentPreview {
    pub fn matches(&self,source:&std::sync::Arc<Document>,objects:&[Object])->bool {
        std::sync::Arc::ptr_eq(&self.source,source) && self.indices.len()==objects.len() &&
            self.indices.iter().zip(objects).all(|(&index,object)|self.document.objects[index].id==object.id)
    }
    pub fn document(&self)->&Document { &self.document }
    pub fn at_source(&self)->bool { self.indices.iter().all(|&i|self.document.objects[i]==self.source.objects[i]) }
    pub fn new(snapshot:&crate::Snapshot,objects:&[Object])->Result<Self> {
        let source=&snapshot.document;
        let indices=objects.iter().map(|o|source.objects.iter().position(|item|item.id==o.id).ok_or_else(||Error::new("OBJECT_NOT_FOUND",&o.id))).collect::<Result<Vec<_>>>()?;
        let ids:BTreeSet<_>=objects.iter().map(|o|&o.id).collect();
        if ids.len()!=indices.len(){return Err(Error::new("DUPLICATE_ID","Duplicate preview target"));}
        let dependencies=source.objects.iter().enumerate().filter_map(|(index,obj)|match &obj.geometry {
            Geometry::Text{text_path:Some(binding),..} if ids.contains(&binding.path_id)=>Some(index),_=>None
        }).collect();
        Ok(Self{source:source.clone(),document:(**source).clone(),indices,dependencies})
    }
    pub fn update(&mut self,objects:Vec<Object>)->Result<bool> {
        if !self.matches(&self.source,&objects){return Err(Error::new("INVALID_OBJECT","Preview targets changed"));}
        if self.indices.iter().zip(&objects).all(|(&index,o)|self.document.objects[index]==*o){return Ok(false);}
        for (&index,o) in self.indices.iter().zip(&objects) {
            let base=&self.source.objects[index];
            if base.parent_id!=o.parent_id||base.page_id!=o.page_id||matches!(base.geometry,Geometry::Group)!=matches!(o.geometry,Geometry::Group){return Err(Error::new("INVALID_OBJECT","Preview cannot change hierarchy"));}
        }
        let old:Vec<_>=self.indices.iter().zip(objects).map(|(&index,o)|std::mem::replace(&mut self.document.objects[index],o)).collect();
        let result=(|| {
            if self.indices.iter().any(|&index|matches!(self.document.objects[index].geometry,Geometry::Group)){return self.document.validate();}
            for &index in &self.indices {
                let object=&self.document.objects[index];self.document.validate_object_fields(object,true)?;self.document.validate_object_geometry(object)?;
            }
            for &index in self.indices.iter().chain(&self.dependencies) {
                let object=&self.document.objects[index];
                if let Geometry::Text{text_path:Some(binding),..}=&object.geometry {crate::text::guide(&self.document,object,binding)?;}
            }
            Ok(())
        })();
        if let Err(error)=result {for (&index,object) in self.indices.iter().zip(old){self.document.objects[index]=object;}return Err(error);}
        Ok(true)
    }
}
