use arranger_core::{Document,Resource,cdr,save_check::{self,SaveFormat}};
use serde_json::json;

fn source()->Document {
    serde_json::from_value(json!({"id":"query-scope","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[{"id":"rect","page_id":"page-1","x_mm":20.,"y_mm":30.,"width_mm":12.,"height_mm":8.,"geometry":{"kind":"rect","radius_mm":0.}}]})).unwrap()
}
fn unsupported_high_depth()->Resource {
    // Opaque grayscale16/RGB16 now have measured native profiles. RGBA16 remains
    // unsupported, so it still exercises selected-page vs desktop rejection.
    let rgba=image::ImageBuffer::from_pixel(1,1,image::Rgba([1u16,2,3,32768]));let mut encoded=std::io::Cursor::new(Vec::new());image::DynamicImage::ImageRgba16(rgba).write_to(&mut encoded,image::ImageFormat::Png).unwrap();Resource::image(&encoded.into_inner()).unwrap()
}
fn check(source:&Document) {
    let original=source.clone();
    for format in [SaveFormat::Cdr14,SaveFormat::Cdr21] {
        let report=save_check::analyze(source,format).unwrap();
        let(full,first)=if format==SaveFormat::Cdr14{(cdr::check_document(source),cdr::check(source,"page-1"))}else{(cdr::check_document21(source),cdr::check21(source,"page-1"))};
        assert_eq!(report["document_interchange"],full);assert_eq!(report["interchange"],first);assert_eq!(report["can_save"],false);
    }
    assert_eq!(source,&original);
}
#[test]
fn single_page_preflight_keeps_unreferenced_resources_and_the_selected_scope_contract() {
    let mut source=source();source.resources.insert("unused".into(),Resource::image(include_bytes!("../../../examples/assets/cdr/native-cmyk/gray16.png")).unwrap());check(&source);
    let report=save_check::analyze(&source,SaveFormat::Cdr21).unwrap();assert_eq!(report["interchange_fidelity"]["roundtrip_source_fields"]["exact"],true);
}
#[test]
fn one_page_shared_workspace_is_checked_separately_from_the_page() {
    let mut source=source();let mut image=source.objects[0].clone();image.id="desktop-unsupported".into();image.page_id=arranger_core::WORKSPACE_PAGE.into();image.geometry=serde_json::from_value(json!({"kind":"image","resource_id":"gray16","fit":"stretch"})).unwrap();source.objects.push(image);
    source.resources.insert("gray16".into(),unsupported_high_depth());check(&source);
    for format in [SaveFormat::Cdr14,SaveFormat::Cdr21] {let report=save_check::analyze(&source,format).unwrap();assert_eq!(report["document_interchange"]["can_export"],false);assert_eq!(report["interchange"]["can_export"],true);assert_eq!(report["interchange_fidelity"]["encoding_status"],"rejected");}
}
#[test]
fn a_rejected_second_page_never_marks_the_first_page_unexportable() {
    let mut source=source();source.pages.push(serde_json::from_value(json!({"id":"page-2","width_mm":210.,"height_mm":297.})).unwrap());
    let mut group=source.objects[0].clone();group.id="unsupported-group".into();group.page_id="page-2".into();group.geometry=arranger_core::Geometry::Group;group.style.opacity=0.5;source.objects.push(group);
    let mut child=source.objects[0].clone();child.id="child".into();child.page_id="page-2".into();child.parent_id=Some("unsupported-group".into());source.objects.push(child);check(&source);
    let report=save_check::analyze(&source,SaveFormat::Cdr14).unwrap();assert_eq!(report["document_interchange"]["can_export"],false);assert_eq!(report["interchange"]["can_export"],true);
}
