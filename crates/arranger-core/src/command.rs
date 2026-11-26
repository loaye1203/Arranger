fn cdr14_format()->String {"cdr14".into()}
use crate::{BooleanOp, Clip, Document, Error, FillRule, Geometry, Object, Page, PathElement, Resource, Result, Style, TextOnPath};
use serde::{Deserialize, Serialize};
use serde_json::Value;

fn default_lease() -> u64 { 30_000 }
fn default_dpi() -> f64 { 96. }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub protocol_version: u32,
    #[serde(default)]
    pub request_id: Value,
    pub command: Command,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Describe,
    Documents,
    DocumentNew { document_id: String, width_mm: f64, height_mm: f64, unit: String },
    DocumentOpen { document_id: String, path: String },
    DocumentRecoverBackup { document_id:String, path:String },
    CdrImport { document_id: String, path: String },
    CdrRecoverBackup { document_id: String, path: String },
    CdrExport { document_id: String, path: String, #[serde(default="cdr14_format")] format:String, #[serde(default)] overwrite: bool, #[serde(default,skip_serializing_if="Option::is_none")] mode:Option<crate::cdr::ExportMode>, #[serde(default,skip_serializing_if="Option::is_none")] page_id:Option<String> },
    CdrPreflight { document_id:String, #[serde(default="cdr14_format")] format:String, mode:crate::cdr::ExportMode, #[serde(default,skip_serializing_if="Option::is_none")] page_id:Option<String> },
    DocumentClose { document_id: String, expected_revision: u64, #[serde(default)] discard_changes: bool },
    Snapshot { document_id: String },
    ObjectGet { document_id: String, object_id: String },
    ResourceGet { document_id: String, resource_id: String },
    PathGet { document_id: String, object_id: String },
    TextMeasure { document_id: String, object_id: String },
    HitTest { document_id: String, page_id: String, point_mm: [f64; 2] },
    Batch { document_id: String, expected_revision: u64, unit: String, operations: Vec<Operation> },
    Begin { document_id: String, expected_revision: u64, unit: String, #[serde(default = "default_lease")] lease_ms: u64 },
    Execute { document_id: String, transaction_id: String, operation: Operation },
    Commit { document_id: String, transaction_id: String },
    Rollback { document_id: String, transaction_id: String },
    Undo { document_id: String, expected_revision: u64 },
    Redo { document_id: String, expected_revision: u64 },
    SaveCheck { document_id: String, format: crate::save_check::SaveFormat },
    Save { document_id: String, expected_revision: u64, path: String, #[serde(default)] overwrite: bool },
    Export { document_id: String, page_id: String, format: ExportFormat, path: String, #[serde(default)] overwrite: bool, #[serde(default = "default_dpi")] dpi: f64 },
    PdfExport { document_id:String, path:String, #[serde(default)] page_ids:Option<Vec<String>>, #[serde(default)] expected_revision:Option<u64>, #[serde(default)] overwrite:bool, #[serde(default="default_dpi")] dpi:f64 },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat { Svg, Png, Pdf, Cdr14, Cdr21 }

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectPatch {
    pub x_mm: Option<f64>,
    pub y_mm: Option<f64>,
    pub width_mm: Option<f64>,
    pub height_mm: Option<f64>,
    pub transform: Option<[f64; 6]>,
    pub style: Option<Style>,
    pub geometry: Option<Geometry>,
    pub visible: Option<bool>,
    #[serde(default,skip_serializing_if="Option::is_none",deserialize_with="clip_patch")]
    pub clip: Option<Option<Clip>>,
}
fn clip_patch<'de,D:serde::Deserializer<'de>>(d:D)->std::result::Result<Option<Option<Clip>>,D::Error> {Option::<Clip>::deserialize(d).map(Some)}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    ObjectCreate { object: Object },
    ObjectUpdate { object_id: String, patch: ObjectPatch },
    /// Reconcile the edited root/subtree with a real page after geometric edits.
    /// Touching its source bounds belongs to that page; otherwise to workspace.
    ObjectPlace { object_id:String, page_id:String },
    ObjectDuplicate { object_id: String, new_id: String, #[serde(default,skip_serializing_if="std::collections::BTreeMap::is_empty")] descendant_ids: std::collections::BTreeMap<String,String> },
    ObjectDelete { object_id: String },
    ObjectReparent { object_id: String, parent_id: Option<String> },
    ObjectReorder { object_id: String, index: usize },
    ObjectLock { object_id: String, locked: bool },
    ObjectToPath { object_id: String },
    PathSet { object_id: String, elements: Vec<PathElement>, fill_rule: Option<FillRule> },
    PathNodeUpdate { object_id: String, index: usize, element: PathElement },
    PathNodeInsert { object_id: String, index: usize, element: PathElement },
    PathNodeDelete { object_id: String, index: usize },
    PathSubpathSetClosed { object_id: String, subpath_index: usize, closed: bool },
    PathSplitSegment { object_id: String, index: usize, t: f64 },
    PathBoolean { left_id: String, right_id: String, result_id: String, operation: BooleanOp, #[serde(default)] remove_inputs: bool },
    TextSetPath { object_id: String, #[serde(deserialize_with = "Option::deserialize")] text_path: Option<TextOnPath> },
    TextToPath { object_id: String },
    PageAdd { page: Page },
    PageResize { page_id: String, width_mm: f64, height_mm: f64 },
    PageDelete { page_id: String },
    ResourcePut { resource_id: String, resource: Resource },
    ImageImport { path:String, object:Object },
    ResourceDelete { resource_id:String },
}

// Ownership includes group subtrees and connected text/guide roots.
fn placement_members(doc:&Document,id:&str)->Result<std::collections::BTreeSet<String>>{
    fn root_id(doc:&Document,id:&str)->Result<String>{
        let mut o=doc.object(id)?;let mut seen=std::collections::BTreeSet::new();
        while let Some(parent)=&o.parent_id{if !seen.insert(o.id.clone()){return Err(Error::new("CYCLIC_GROUP","Cannot place a cyclic group"));}o=doc.object(parent)?;}
        Ok(o.id.clone())
    }
    let mut roots=std::collections::BTreeSet::from([root_id(doc,id)?]);
    loop{
        let mut ids=roots.clone();loop{let n=ids.len();for o in &doc.objects{if o.parent_id.as_ref().is_some_and(|p|ids.contains(p)){ids.insert(o.id.clone());}}if ids.len()==n{break;}}
        let n=roots.len();
        for o in &doc.objects{if let Geometry::Text{text_path:Some(binding),..}=&o.geometry{if ids.contains(&o.id)||ids.contains(&binding.path_id){roots.insert(root_id(doc,&o.id)?);roots.insert(root_id(doc,&binding.path_id)?);}}}
        if roots.len()==n{return Ok(ids);}
    }
}
fn assign_owner(doc:&mut Document,ids:&std::collections::BTreeSet<String>,owner:&str)->Result<Vec<String>>{
    for id in ids{let o=doc.object(id)?;if o.parent_id.is_none()&&o.page_id!=owner{doc.require_editable(id)?;}}
    let mut changed=vec![];for o in &mut doc.objects{if ids.contains(&o.id)&&o.page_id!=owner{o.page_id=owner.into();changed.push(o.id.clone());}}Ok(changed)
}

pub(crate) struct Applied { pub changed: Vec<String>, pub warnings: Vec<Notice> }
impl Operation {
    pub(crate) fn apply(&self, doc: &mut Document) -> Result<Applied> {
        let mut warnings = vec![];
        let changed = match self {
            Self::ObjectCreate { object } => {
                if let Some(parent) = &object.parent_id { doc.require_editable(parent)?; }
                doc.objects.push(object.clone()); vec![object.id.clone()]
            }
            Self::ObjectUpdate { object_id, patch } => {
                doc.require_editable(object_id)?;
                let o = doc.objects.iter_mut().find(|o| o.id == *object_id).expect("object checked");
                if let Some(v) = patch.x_mm { o.x_mm = v; }
                if let Some(v) = patch.y_mm { o.y_mm = v; }
                if let Some(v) = patch.width_mm { o.width_mm = v; }
                if let Some(v) = patch.height_mm { o.height_mm = v; }
                if let Some(v) = patch.transform { o.transform = v; }
                if let Some(v) = &patch.style { o.style = v.clone(); }
                if let Some(v) = &patch.geometry { o.geometry = v.clone(); }
                if let Some(v) = patch.visible { o.visible = v; }
                if let Some(v) = &patch.clip { o.clip = v.clone(); }
                vec![object_id.clone()]
            }
            Self::ObjectPlace {object_id,page_id}=>{
                let page=doc.page(page_id)?;
                let (width,height)=(page.width_mm,page.height_mm);
                doc.require_editable(object_id)?;
                let ids=placement_members(doc,object_id)?;
                let roots=ids.iter().filter(|id|doc.object(id).is_ok_and(|o|o.parent_id.is_none())).cloned().collect::<Vec<_>>();
                let mut bounds:Option<kurbo::Rect>=None;
                for id in &roots{let b=doc.placement_bounds(doc.object(id)?)?;bounds=Some(bounds.map_or(b,|r|r.union(b)));}
                let bounds=bounds.expect("at least one root");let (x,y,w,h)=(bounds.x0,bounds.y0,bounds.width(),bounds.height());
                if ![x,y,w,h,x+w,y+h].iter().all(|v|v.is_finite()){return Err(Error::new("INVALID_GEOMETRY","Placement needs finite world bounds"));}
                let owner=if x<=width&&y<=height&&x+w>=0.&&y+h>=0.{page_id.as_str()}else{crate::WORKSPACE_PAGE};
                assign_owner(doc,&ids,owner)?
            }
            Self::ObjectDuplicate { object_id, new_id, descendant_ids } => {
                doc.require_editable(object_id)?;
                let mut sources = std::collections::BTreeSet::from([object_id.clone()]);
                loop {
                    let before=sources.len();
                    for o in &doc.objects {
                        if o.parent_id.as_ref().is_some_and(|p|sources.contains(p)) {sources.insert(o.id.clone());}
                    }
                    if sources.len()==before {break;}
                }
                if descendant_ids.keys().any(|id|id==object_id || !sources.contains(id)) {
                    return Err(Error::new("INVALID_PARAMETER","descendant_ids must name descendants of the copied object"));
                }
                let mut mapping=std::collections::BTreeMap::from([(object_id.clone(),new_id.clone())]);
                let mut index=0;
                for o in &doc.objects {if sources.contains(&o.id) && o.id!=*object_id {
                    doc.require_editable(&o.id)?;
                    index+=1;
                    mapping.insert(o.id.clone(),descendant_ids.get(&o.id).cloned().unwrap_or_else(||format!("{new_id}.{index}")));
                }}
                let mut copies=vec![];
                for o in &doc.objects {if let Some(id)=mapping.get(&o.id) {
                    let mut copy=o.clone();copy.id=id.clone();
                    if let Some(parent)=copy.parent_id.as_ref().and_then(|p|mapping.get(p)) {copy.parent_id=Some(parent.clone());}
                    if let Geometry::Text{text_path:Some(binding),..}=&mut copy.geometry {
                        if let Some(guide)=mapping.get(&binding.path_id) {binding.path_id=guide.clone();}
                    }
                    copies.push(copy);
                }}
                let i = doc.objects.iter().position(|o| o.id == *object_id).expect("object checked");
                let changed=copies.iter().map(|o|o.id.clone()).collect();
                doc.objects.splice(i+1..i+1,copies);changed
            }
            Self::ObjectDelete { object_id } => {
                let mut ids = vec![object_id.clone()];
                let mut i = 0;
                while i < ids.len() {
                    ids.extend(doc.objects.iter().filter(|o| o.parent_id.as_ref() == Some(&ids[i])).map(|o| o.id.clone()).collect::<Vec<_>>());
                    i += 1;
                }
                for id in &ids { doc.require_editable(id)?; }
                doc.objects.retain(|o| !ids.contains(&o.id)); ids
            }
            Self::ObjectReparent { object_id, parent_id } => {
                doc.require_editable(object_id)?;
                let owner=if let Some(parent)=parent_id {
                    doc.require_editable(parent)?;let parent=doc.object(parent)?;let object=doc.object(object_id)?;
                    if parent.page_id!=object.page_id&&parent.page_id!=crate::WORKSPACE_PAGE&&object.page_id!=crate::WORKSPACE_PAGE{return Err(Error::new("INVALID_PARENT","Cannot group objects from different paper pages"));}
                    Some(parent.page_id.clone())
                }else{None};
                doc.objects.iter_mut().find(|o| o.id == *object_id).expect("object checked").parent_id = parent_id.clone();
                let mut changed=vec![object_id.clone()];
                if let (Some(parent),Some(owner))=(parent_id,owner){let members=placement_members(doc,parent)?;changed.extend(assign_owner(doc,&members,&owner)?);}
                changed
            }
            Self::ObjectReorder { object_id, index } => {
                doc.require_editable(object_id)?;
                if *index >= doc.objects.len() { return Err(Error::new("INVALID_INDEX", "Stacking index is out of range")); }
                let old = doc.objects.iter().position(|o| o.id == *object_id).expect("object checked");
                let object = doc.objects.remove(old); doc.objects.insert(*index, object); vec![object_id.clone()]
            }
            Self::ObjectLock { object_id, locked } => {
                if let Some(parent) = &doc.object(object_id)?.parent_id { doc.require_editable(parent)?; }
                doc.objects.iter_mut().find(|o| o.id == *object_id).expect("object checked").locked = *locked;
                vec![object_id.clone()]
            }
            Self::ObjectToPath { object_id } => {
                crate::path::convert(doc, object_id)?; vec![object_id.clone()]
            }
            Self::PathSet { object_id, elements, fill_rule } => {
                crate::path::set(doc, object_id, elements, *fill_rule)?; vec![object_id.clone()]
            }
            Self::PathNodeUpdate { object_id, index, element } => {
                crate::path::edit_node(doc, object_id, *index, Some(element), false)?; vec![object_id.clone()]
            }
            Self::PathNodeInsert { object_id, index, element } => {
                crate::path::edit_node(doc, object_id, *index, Some(element), true)?; vec![object_id.clone()]
            }
            Self::PathNodeDelete { object_id, index } => {
                crate::path::edit_node(doc, object_id, *index, None, false)?; vec![object_id.clone()]
            }
            Self::PathSubpathSetClosed { object_id, subpath_index, closed } => {
                crate::path::set_closed(doc, object_id, *subpath_index, *closed)?; vec![object_id.clone()]
            }
            Self::PathSplitSegment { object_id, index, t } => {
                crate::path::split(doc, object_id, *index, *t)?; vec![object_id.clone()]
            }
            Self::PathBoolean { left_id, right_id, result_id, operation, remove_inputs } => {
                crate::path::boolean(doc, left_id, right_id, result_id, *operation, *remove_inputs)?
            }
            Self::TextSetPath { object_id, text_path } => {
                doc.require_editable(object_id)?;
                let object = doc.objects.iter_mut().find(|o| o.id == *object_id).expect("editable object");
                let Geometry::Text { text_path: binding, .. } = &mut object.geometry else {
                    return Err(Error::new("NOT_TEXT", "text_set_path requires a text object"));
                };
                *binding = text_path.clone();
                doc.validate()?;
                let layout = crate::text::layout(doc, doc.object(object_id)?)?;
                crate::text::require_complete(&layout)?;
                warnings.extend(layout.warnings.clone());
                vec![object_id.clone()]
            }
            Self::TextToPath { object_id } => {
                warnings.extend(crate::text::convert(doc, object_id)?);
                vec![object_id.clone()]
            }
            Self::PageAdd { page } => { doc.pages.push(page.clone()); vec![page.id.clone()] }
            Self::PageResize { page_id, width_mm, height_mm } => {
                let p = doc.pages.iter_mut().find(|p| p.id == *page_id).ok_or_else(|| Error::new("PAGE_NOT_FOUND", page_id))?;
                p.width_mm = *width_mm; p.height_mm = *height_mm; vec![page_id.clone()]
            }
            Self::PageDelete { page_id } => {
                doc.page(page_id)?;
                if doc.pages.len() == 1 || doc.objects.iter().any(|o| o.page_id == *page_id) {
                    return Err(Error::new("PAGE_NOT_EMPTY", "Cannot delete the last page or a page containing objects"));
                }
                doc.pages.retain(|p| p.id != *page_id); vec![page_id.clone()]
            }
            Self::ResourcePut { resource_id, resource } => {
                crate::resource::validate(resource)?;
                if doc.resources.contains_key(resource_id) { return Err(Error::new("DUPLICATE_ID", "Resource ID is already in use")); }
                if let Some((id, _)) = doc.resources.iter().find(|(_, r)| r.sha256 == resource.sha256) {
                    return Err(Error::new("RESOURCE_ALREADY_EXISTS", format!("Identical image already stored as {id}")));
                }
                doc.resources.insert(resource_id.clone(), resource.clone()); vec![resource_id.clone()]
            }
            Self::ImageImport {path,object} => {
                if let Some(parent)=&object.parent_id {doc.require_editable(parent)?;}
                let Geometry::Image {resource_id,..}=&object.geometry else {return Err(Error::new("UNSUPPORTED_GEOMETRY","image_import requires an image object"));};
                if !crate::model::valid_id(resource_id) {return Err(Error::new("INVALID_RESOURCE","Invalid requested resource ID"));}
                let resource=crate::resource::import(std::path::Path::new(path))?;
                if doc.resources.get(resource_id).is_some_and(|r|r.sha256!=resource.sha256) {return Err(Error::new("DUPLICATE_ID","Requested resource ID already contains different image bytes"));}
                let mut imported=object.clone();let mut changed=vec![object.id.clone()];
                if let Some((existing,_))=doc.resources.iter().find(|(_,r)|r.sha256==resource.sha256) {
                    if let Geometry::Image {resource_id,..}=&mut imported.geometry {*resource_id=existing.clone();}
                    warnings.push(Notice::new("RESOURCE_REUSED",&format!("Image '{}' uses existing resource '{existing}'; no duplicate original was stored",object.id)));
                } else {doc.resources.insert(resource_id.clone(),resource);changed.push(resource_id.clone());}
                doc.objects.push(imported);changed
            }
            Self::ResourceDelete {resource_id} => {
                if !doc.resources.contains_key(resource_id) {return Err(Error::new("RESOURCE_NOT_FOUND",resource_id));}
                if doc.objects.iter().any(|o|matches!(&o.geometry,Geometry::Image{resource_id:id,..} if id==resource_id)) {return Err(Error::new("RESOURCE_IN_USE","Remove or rebind every image instance before deleting its resource"));}
                doc.resources.remove(resource_id);vec![resource_id.clone()]
            }
        };
        doc.validate()?;
        // A later guide/content edit must not silently clip an attached text run.
        warnings.extend(crate::text::validate_bound_layouts(doc)?);
        for id in &changed { if let Ok(object)=doc.object(id) {
            if matches!(object.geometry,Geometry::Text{text_path:None,..}) {warnings.extend(crate::text::layout(doc,object)?.warnings.clone());}
        }}
        let mut seen=std::collections::BTreeSet::new();
        warnings.retain(|w|seen.insert((w.code.clone(),w.message.clone())));
        Ok(Applied { changed, warnings })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Notice { pub code: String, pub message: String }
impl Notice {
    pub fn new(code: &str, message: &str) -> Self { Self { code: code.into(), message: message.into() } }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Response {
    pub protocol_version: u32,
    pub request_id: Value,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Error>,
}
impl Response {
    pub fn from_result(request_id: Value, result: Result<Value>) -> Self {
        match result {
            Ok(result) => Self { protocol_version: 1, request_id, ok: true, result: Some(result), error: None },
            Err(error) => Self { protocol_version: 1, request_id, ok: false, result: None, error: Some(error) },
        }
    }
}
