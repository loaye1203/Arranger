use arranger_core::{cdr,Document,Geometry,Object,Resource,Style};
use std::{fs,io::Cursor,path::PathBuf};
fn root()->PathBuf{PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")}
fn image(id:&str,resource:&str,page:&str)->Object {
    Object{id:id.into(),page_id:page.into(),parent_id:None,x_mm:20.,y_mm:40.,width_mm:40.,height_mm:20.,transform:[1.,0.,0.,1.,0.,0.],style:Style{fill:"none".into(),stroke:"none".into(),stroke_width_mm:0.,..Default::default()},visible:true,locked:false,clip:None,geometry:Geometry::Image{resource_id:resource.into(),fit:arranger_core::ImageFit::Stretch,clip_mm:None}}
}
fn encode(d:&Document,version:u32)->Vec<u8>{if version==14{cdr::encode_document(d)}else{cdr::encode_document21(d)}.unwrap().0}
#[test]
fn exact_png_jpeg_containers_ids_and_shared_instances_survive_both_native_codecs() {
    let mut d=Document::new("containers".into(),210.,297.).unwrap();d.pages.push(arranger_core::Page{id:"second".into(),width_mm:148.,height_mm:210.});
    for (i,name) in ["examples/assets/images/quadrants.png","examples/assets/cdr/content14/source-assets/photo-63x41.jpg","examples/assets/cdr/content14/source-assets/gray-35x17.jpg"].into_iter().enumerate(){
        let id=format!("source-{i}");let r=Resource::image(&fs::read(root().join(name)).unwrap()).unwrap();d.resources.insert(id.clone(),r);
        d.objects.push(image(&format!("image-{i}"),&id,"page-1"));d.objects.push(image(&format!("shared-{i}"),&id,"second"));d.objects.push(image(&format!("desktop-{i}"),&id,"_workspace"));
    }
    for version in [14,21] {
        let before=d.hash();let bytes=encode(&d,version);let (copy,notices)=cdr::decode(&bytes,"copy").unwrap();assert_eq!(d.hash(),before);assert_eq!(copy.resources,d.resources);assert!(notices.iter().any(|n|n.code=="CDR_RESOURCE_ORIGINAL"));assert!(!notices.iter().any(|n|n.code=="CDR_IMAGE_PIXELS"));
        for o in &d.objects {assert_eq!(copy.object(&o.id).unwrap().geometry,o.geometry);}
        let again=cdr::decode(&encode(&copy,version),"copy").unwrap().0;assert_eq!(again,copy);
    }
}
#[test]
fn equal_rgba_with_different_source_containers_remain_distinct_resources() {
    use image::{ImageEncoder,codecs::png::{PngEncoder,CompressionType,FilterType}};
    let raw=fs::read(root().join("examples/assets/images/quadrants.png")).unwrap();let decoded=image::load_from_memory(&raw).unwrap().to_rgba8();let mut other=vec![];PngEncoder::new_with_quality(&mut other,CompressionType::Best,FilterType::NoFilter).write_image(&decoded,decoded.width(),decoded.height(),image::ExtendedColorType::Rgba8).unwrap();assert_ne!(raw,other);
    let mut d=Document::new("distinct".into(),210.,297.).unwrap();
    for (id,bytes) in [("original",raw),("reencoded",other)] {d.resources.insert(id.into(),Resource::png(&bytes).unwrap());d.objects.push(image(id,id,"page-1"));}
    for version in [14,21] {let copy=cdr::decode(&encode(&d,version),"copy").unwrap().0;assert_eq!(copy.resources,d.resources);for o in &d.objects {assert_eq!(copy.object(&o.id).unwrap().geometry,o.geometry);}}
}
#[test]
fn large_valid_png_uses_bounded_chunks_without_losing_original_bytes() {
    let mut seed=0x12345678u32;let pixels=(0..257*193*3).map(|_|{seed^=seed<<13;seed^=seed>>17;seed^=seed<<5;seed as u8}).collect::<Vec<_>>();
    let image=image::RgbImage::from_raw(257,193,pixels).unwrap();let mut out=Cursor::new(Vec::new());image::DynamicImage::ImageRgb8(image).write_to(&mut out,image::ImageFormat::Png).unwrap();let raw=out.into_inner();assert!(raw.len()>131072);
    let mut d=Document::new("large".into(),210.,297.).unwrap();d.resources.insert("large-png".into(),Resource::png(&raw).unwrap());d.objects.push(self::image("large","large-png","page-1"));
    for version in [14,21] {let copy=cdr::decode(&encode(&d,version),"copy").unwrap().0;assert_eq!(copy.resources,d.resources);}
}

#[test]
fn eight_bit_rgb_icc_and_exif_originals_survive_without_claiming_managed_appearance() {
    for name in ["png-profile.png","jpeg-profile.jpg"] {
        let raw=fs::read(root().join("examples/assets/cdr/rust14-images").join(name)).unwrap();let resource=Resource::image(&raw).unwrap();assert!(arranger_core::resource::validate(&resource).unwrap().metadata.needs_managed_interpretation());
        let mut d=Document::new("metadata".into(),210.,297.).unwrap();d.resources.insert("original".into(),resource);d.objects.push(image("image","original","page-1"));
        for version in [14,21] {let (bytes,warnings)=if version==14{cdr::encode_document(&d)}else{cdr::encode_document21(&d)}.unwrap();let copy=cdr::decode(&bytes,"copy").unwrap().0;assert_eq!(copy.resources,d.resources);assert!(warnings.iter().any(|n|n.code=="CDR_RESOURCE_METADATA_UNMANAGED"));}
    }
}

#[test]
fn opaque_gray16_originals_survive_both_measured_native_profiles() {
    let raw=fs::read(root().join("examples/assets/cdr/native-gray16/gray16.png")).unwrap();let expected=image::load_from_memory(&raw).unwrap().into_luma16();assert!(expected.as_raw().iter().any(|v|v%257!=0));
    let mut d=Document::new("precision".into(),210.,297.).unwrap();d.resources.insert("original".into(),Resource::image(&raw).unwrap());d.objects.push(image("image","original","page-1"));
    for version in[14,21]{let copy=cdr::decode(&encode(&d,version),"copy").unwrap().0;assert_eq!(copy.resources,d.resources);let copy=cdr::decode(&encode(&copy,version),"second").unwrap().0;assert_eq!(copy.resources,d.resources);}
}
