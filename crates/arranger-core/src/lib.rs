//! Arranger's document, command, persistence and rendering kernel.
//! Frontends share `Service`; no window dependency is required here.
pub mod command;
pub mod cdr;
pub mod clip;
pub mod engine;
pub mod error;
pub mod model;
pub mod native;
pub mod path;
pub mod paint;
mod pdf;
pub mod render;
pub mod resource;
pub mod schema;
pub mod save_check;
pub mod text;

pub use command::{Command, Operation, Request, Response};
pub use engine::{Service, Snapshot};
pub use error::{Error, Result};
pub use model::{Document, Geometry, Object, Page, Resource, Style, LineCap, LineJoin, ImageFit, FixedStrokeNib, WORKSPACE_PAGE};
pub use clip::Clip;
pub use path::{BooleanOp, FillRule, PathElement};
pub use text::{TextAnchor, TextOnPath, TextLayoutOptions, TextMode, TextAlign};
