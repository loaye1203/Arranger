//! Bounded CMYK8 and opaque grayscale16/RGB16 native pixels, checked against Corel RI.
//! Source containers remain in the resource catalogue; pixels stay native.
use super::{invalid,unsupported,Result};
use zune_jpeg::{JpegDecoder,zune_core::{colorspace::ColorSpace,options::DecoderOptions}};
use std::io::Cursor;

#[derive(Clone,Debug,PartialEq,Eq)]
pub(super) struct Cmyk {pub width:u32,pub height:u32,pub pixels:Vec<u8>}

#[derive(Clone,Debug,PartialEq,Eq)]
pub(super) struct Gray16 {pub width:u32,pub height:u32,pub pixels:Vec<u16>}

#[derive(Clone,Debug,PartialEq,Eq)]
pub(super) struct Rgb16 {pub width:u32,pub height:u32,pub pixels:Vec<u16>}

/// Native TIFF witnesses use RI model12 with little-endian B,G,R16 samples.
pub(super) fn rgb16(raw:&[u8],mime:&str)->Result<Option<Rgb16>> {
    let meta=crate::resource::metadata(raw,mime)?;
    if mime!="image/png"||meta.png_bit_depth!=Some(16)||meta.png_color_type!=Some(2){return Ok(None);}
    let decoded=image::load_from_memory(raw).map_err(|e|invalid(e.to_string()))?;
    let image::DynamicImage::ImageRgb16(image)=decoded else{return Err(unsupported("native RGB16 with transparency has no measured lossless profile"));};
    let(width,height)=image.dimensions();if width==0||height==0||u64::from(width)*u64::from(height)>crate::resource::MAX_PIXELS{return Err(unsupported("RGB16 image dimensions exceed the resource limit"));}
    Ok(Some(Rgb16{width,height,pixels:image.into_raw()}))
}
impl Rgb16 {
    pub fn image(&self)->image::ImageBuffer<image::Rgb<u16>,Vec<u16>>{image::ImageBuffer::from_raw(self.width,self.height,self.pixels.clone()).expect("validated native RGB16 dimensions")}
}

/// Corel2019's TIFF importer writes RI model14, not model5/bpp16. Both
/// SaveAs14 and21 preserve those little-endian samples, including low bits.
/// Product resources remain PNG/JPEG; TIFF is only the independent witness.
pub(super) fn gray16(raw:&[u8],mime:&str)->Result<Option<Gray16>> {
    let meta=crate::resource::metadata(raw,mime)?;
    if mime!="image/png"||meta.png_bit_depth!=Some(16)||meta.png_color_type!=Some(0){return Ok(None);}
    let decoded=image::load_from_memory(raw).map_err(|e|invalid(e.to_string()))?;
    let image::DynamicImage::ImageLuma16(image)=decoded else{return Err(unsupported("native grayscale16 with transparency has no measured lossless profile"));};
    let (width,height)=image.dimensions();
    if width==0||height==0||u64::from(width)*u64::from(height)>crate::resource::MAX_PIXELS{return Err(unsupported("grayscale16 image dimensions exceed the resource limit"));}
    Ok(Some(Gray16{width,height,pixels:image.into_raw()}))
}

impl Gray16 {
    pub fn image(&self)->image::ImageBuffer<image::Luma<u16>,Vec<u16>> {image::ImageBuffer::from_raw(self.width,self.height,self.pixels.clone()).expect("validated native grayscale16 dimensions")}
}

pub(super) fn source(raw:&[u8],mime:&str)->Result<Option<Cmyk>> {
    let meta=crate::resource::metadata(raw,mime)?;
    if meta.needs_precision_conversion()&&gray16(raw,mime)?.is_none()&&rgb16(raw,mime)?.is_none(){return Err(unsupported("this high-bit-depth active image has no measured lossless native profile; only opaque grayscale16/RGB16 PNG is supported"));}
    if meta.jpeg_components!=Some(4){return Ok(None);}
    if meta.jpeg_sample_precision!=Some(8)||meta.records.iter().any(|r|r.kind=="jpeg_adobe_ycck") {
        return Err(unsupported("only measured Adobe CMYK8 JPEG samples are supported; YCCK remains guarded"));
    }
    // The measured JPEG uses Adobe transform 0 and inverted four components.
    // Require that marker explicitly; do not guess the polarity of other JPEGs.
    let mut at=2;let mut adobe=0;
    while at<raw.len() {
        if raw.get(at)!=Some(&255){return Err(invalid("JPEG marker missing"));}
        while raw.get(at)==Some(&255){at+=1;}
        let marker=*raw.get(at).ok_or_else(||invalid("JPEG marker truncated"))?;at+=1;
        if matches!(marker,0xda|0xd9){break;}
        if marker==1||(0xd0..=0xd7).contains(&marker){continue;}
        let n=raw.get(at..at+2).ok_or_else(||invalid("JPEG marker length missing"))?;let n=u16::from_be_bytes(n.try_into().unwrap()) as usize;
        let body=raw.get(at+2..at+n).ok_or_else(||invalid("JPEG marker truncated"))?;
        if marker==0xee&&body.starts_with(b"Adobe") {if body.len()!=12||body[11]!=0{return Err(unsupported("unmeasured Adobe four-channel JPEG profile"));}adobe+=1;}
        at+=n;
    }
    if adobe!=1{return Err(unsupported("CMYK JPEG polarity requires one Adobe transform-0 marker"));}
    let mut decoder=JpegDecoder::new_with_options(Cursor::new(raw),DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::CMYK));
    decoder.decode_headers().map_err(|e|invalid(e.to_string()))?;
    if decoder.input_colorspace()!=Some(ColorSpace::CMYK)||decoder.output_colorspace()!=Some(ColorSpace::CMYK){return Err(unsupported("JPEG cannot retain its four native CMYK channels"));}
    let info=decoder.info().ok_or_else(||invalid("JPEG dimensions missing"))?;let (width,height)=(u32::from(info.width),u32::from(info.height));
    if width==0||height==0||u64::from(width)*u64::from(height)>crate::resource::MAX_PIXELS{return Err(unsupported("CMYK image dimensions exceed the resource limit"));}
    let mut pixels=decoder.decode().map_err(|e|invalid(e.to_string()))?;
    if pixels.len()!=width as usize*height as usize*4{return Err(invalid("JPEG four-channel pixel count differs"));}
    for p in &mut pixels{*p=255-*p;}
    Ok(Some(Cmyk{width,height,pixels}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::binary::{read_native,all,u32_at,bytes};
    #[test]
    fn four_real_corel_files_have_the_same_exact_native_cmyk_samples() {
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/assets/cdr/native-cmyk");
        let raw=std::fs::read(root.join("cmyk.jpg")).unwrap();let image=source(&raw,"image/jpeg").unwrap().unwrap();assert_eq!((image.width,image.height),(13,9));
        // JPEG inverse-DCT rounding differs between zune and Corel/libjpeg.
        // Pin each decoder result separately; never allow a tolerance when
        // checking actual encoded native samples against a resource binding.
        assert_eq!(crate::model::sha256(&image.pixels),"7791b122770f83abcc5de1eeb12121187f41e139d2e66fd4e4ed23d114e46f4e");
        for version in [14,21] {for suffix in ["","-edited"] {
            let data=std::fs::read(root.join(format!("cmyk-v{version}{suffix}.cdr"))).unwrap();let native=read_native(&data).unwrap();let n=all(&native.nodes).into_iter().find(|n|n.tag==*b"bmp ").unwrap();
            assert_eq!(u32_at(&n.data,54).unwrap(),3);let stride=u32_at(&n.data,78).unwrap() as usize;let raw=bytes(&n.data,118,stride*image.height as usize).unwrap();
            let top=raw.chunks_exact(stride).rev().flat_map(|r|r[..image.width as usize*4].iter().copied()).collect::<Vec<_>>();assert_eq!(crate::model::sha256(&top),"d206c6c4c12e944fa3193911734d8979ebf08b5222c33d8af83c5464c3983690");
        }}
    }
    #[test]
    fn unmeasured_precision_and_unknown_cmyk_polarity_stay_guarded() {
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/assets/cdr/native-cmyk");
        let gray=std::fs::read(root.join("gray16.png")).unwrap();assert!(source(&gray,"image/png").unwrap().is_none());assert!(gray16(&gray,"image/png").unwrap().is_some());
        let rgb=image::ImageBuffer::from_pixel(1,1,image::Rgb([1u16,2,3]));let mut encoded=Cursor::new(Vec::new());image::DynamicImage::ImageRgb16(rgb).write_to(&mut encoded,image::ImageFormat::Png).unwrap();let raw=encoded.into_inner();assert!(source(&raw,"image/png").unwrap().is_none());assert_eq!(rgb16(&raw,"image/png").unwrap().unwrap().pixels,vec![1,2,3]);
        for image in [image::DynamicImage::ImageRgba16(image::ImageBuffer::from_pixel(1,1,image::Rgba([1u16,2,3,32768]))),image::DynamicImage::ImageLumaA16(image::ImageBuffer::from_pixel(1,1,image::LumaA([1u16,32768])))]{let mut encoded=Cursor::new(Vec::new());image.write_to(&mut encoded,image::ImageFormat::Png).unwrap();assert!(source(&encoded.into_inner(),"image/png").is_err());}
        let gray_alpha=image::ImageBuffer::from_pixel(1,1,image::LumaA([1u16,65535]));let mut encoded=Cursor::new(Vec::new());image::DynamicImage::ImageLumaA16(gray_alpha).write_to(&mut encoded,image::ImageFormat::Png).unwrap();assert!(source(&encoded.into_inner(),"image/png").is_err());
        let raw=std::fs::read(root.join("cmyk.jpg")).unwrap();let at=raw.windows(5).position(|v|v==b"Adobe").unwrap();let mut missing=raw.clone();missing[at]=b'X';assert!(source(&missing,"image/jpeg").is_err());
        let mut ycck=raw;ycck[at+11]=2;assert!(source(&ycck,"image/jpeg").is_err());
    }
    fn scene()->crate::Document {
        let raw=include_bytes!("../../../../examples/assets/cdr/native-cmyk/cmyk.jpg");
        let mut doc=crate::Document::new("cmyk-source".into(),210.,297.).unwrap();doc.resources.insert("original-cmyk".into(),crate::Resource::image(raw).unwrap());
        for (id,x) in [("image-a",20.),("image-b",80.)] {
            doc.objects.push(serde_json::from_value(serde_json::json!({"id":id,"page_id":"page-1","x_mm":x,"y_mm":30.,"width_mm":26.,"height_mm":18.,"geometry":{"kind":"image","resource_id":"original-cmyk","fit":"stretch"}})).unwrap());
        }
        doc
    }
    #[test]
    fn both_writers_keep_native_four_channels_original_container_and_sharing() {
        let doc=scene();let expected=source(include_bytes!("../../../../examples/assets/cdr/native-cmyk/cmyk.jpg"),"image/jpeg").unwrap().unwrap();
        for version in [14,21] {
            let encode=if version==14{crate::cdr::encode_document}else{crate::cdr::encode_document21};let (bytes,_)=encode(&doc).unwrap();
            let native=read_native(&bytes).unwrap();let records=all(&native.nodes).into_iter().filter(|n|n.tag==*b"bmp ").collect::<Vec<_>>();assert_eq!(records.len(),1);
            let d=&records[0].data;assert_eq!(u32_at(d,54).unwrap(),3);assert_eq!(u32_at(d,74).unwrap(),32);assert_eq!(u32_at(d,24).unwrap(),0);
            let stride=u32_at(d,78).unwrap() as usize;let raw=super::super::binary::bytes(d,118,stride*expected.height as usize).unwrap();let actual=raw.chunks_exact(stride).rev().flat_map(|r|r[..expected.width as usize*4].iter().copied()).collect::<Vec<_>>();assert_eq!(actual,expected.pixels);
            let (restored,warnings)=crate::cdr::decode(&bytes,"cmyk-source").unwrap();assert_eq!(restored,doc);assert!(warnings.iter().any(|w|w.code=="CDR_CMYK_PIXELS"));
            let second=encode(&restored).unwrap().0;assert_eq!(crate::cdr::decode(&second,"cmyk-source").unwrap().0,doc);
        }
    }
    #[test]
    fn native_channel_edits_and_missing_originals_cannot_be_hidden_by_preview_or_notes() {
        fn mutate(nodes:&mut [super::super::binary::Node]) {
            for n in nodes {if n.tag==*b"bmp "{n.data[118]^=1;}if let Some(kids)=&mut n.children{mutate(kids);}}
        }
        for version in [14,21] {
            let bytes=if version==14{crate::cdr::encode_document(&scene())}else{crate::cdr::encode_document21(&scene())}.unwrap().0;let mut native=read_native(&bytes).unwrap();mutate(&mut native.nodes);
            let changed=if version==14{
                let riff=super::super::binary::write(&native.nodes).unwrap();native.members.insert("content/riffData.cdr".into(),riff);super::super::binary::zip(&native.members.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>())
            }else{super::super::binary::repack21(&native).unwrap()};let error=crate::cdr::decode(&changed,"stale").unwrap_err();assert!(error.message.contains("CMYK channels differ"),"{error:?}");
            let mut native=read_native(&bytes).unwrap();native.members.remove("metadata/metadata.xml");native.members.remove("META-INF/metadata.xml");let missing=if version==14{super::super::binary::zip(&native.members.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>())}else{super::super::binary::repack21(&native).unwrap()};let error=crate::cdr::decode(&missing,"missing").unwrap_err();assert!(error.message.contains("CMYK"),"{error:?}");
        }
        for data in [include_bytes!("../../../../examples/assets/cdr/native-cmyk/cmyk-v14.cdr").as_slice(),include_bytes!("../../../../examples/assets/cdr/native-cmyk/cmyk-v21.cdr").as_slice()] {assert!(crate::cdr::decode(data,"standalone").is_err());}
    }
    #[test]
    fn cmyk_edits_with_identical_rgb_preview_are_still_rejected() {
        let source=source(include_bytes!("../../../../examples/assets/cdr/native-cmyk/cmyk.jpg"),"image/jpeg").unwrap().unwrap();
        let preview=|c:u8,k:u8|((u32::from(255-c)*u32::from(255-k)+127)/255) as u8;
        let i=source.pixels.chunks_exact(4).position(|p|p[0]<255&&preview(p[0],p[3])==preview(p[0]+1,p[3])).unwrap();
        let bytes=crate::cdr::encode_document21(&scene()).unwrap().0;let mut native=read_native(&bytes).unwrap();
        fn change(nodes:&mut [super::super::binary::Node],i:usize,width:usize,height:usize) {for n in nodes {if n.tag==*b"bmp " {let stride=super::super::binary::u32_at(&n.data,78).unwrap() as usize;let at=118+(height-1-i/width)*stride+(i%width)*4;n.data[at]+=1;}if let Some(kids)=&mut n.children{change(kids,i,width,height);}}}
        change(&mut native.nodes,i,source.width as usize,source.height as usize);let changed=super::super::binary::repack21(&native).unwrap();assert!(crate::cdr::decode(&changed,"equal-preview").unwrap_err().message.contains("CMYK channels differ"));
    }

    fn gray_scene()->crate::Document {
        let raw=include_bytes!("../../../../examples/assets/cdr/native-gray16/gray16.png");
        let mut doc=crate::Document::new("gray-source".into(),210.,297.).unwrap();doc.resources.insert("original-gray".into(),crate::Resource::image(raw).unwrap());
        for (id,x)in[("gray-a",20.),("gray-b",80.)]{doc.objects.push(serde_json::from_value(serde_json::json!({"id":id,"page_id":"page-1","x_mm":x,"y_mm":30.,"width_mm":26.,"height_mm":18.,"geometry":{"kind":"image","resource_id":"original-gray","fit":"stretch"}})).unwrap());}doc
    }
    fn gray_samples(data:&[u8])->Gray16 {
        assert_eq!(u32_at(data,54).unwrap(),14);assert_eq!(u32_at(data,58).unwrap(),1);assert_eq!(u32_at(data,70).unwrap(),1);assert_eq!(u32_at(data,74).unwrap(),16);assert_eq!(u32_at(data,24).unwrap(),0);
        let(width,height,stride)=(u32_at(data,62).unwrap(),u32_at(data,66).unwrap(),u32_at(data,78).unwrap() as usize);
        let raw=bytes(data,118,stride*height as usize).unwrap();let pixels=raw.chunks_exact(stride).rev().flat_map(|row|row[..width as usize*2].chunks_exact(2).map(|p|u16::from_le_bytes([p[0],p[1]]))).collect();Gray16{width,height,pixels}
    }
    fn repack(mut native:super::super::binary::NativeRecords)->Vec<u8> {
        if native.version==2100{super::super::binary::repack21(&native).unwrap()}else{native.members.insert("content/riffData.cdr".into(),super::super::binary::write(&native.nodes).unwrap());super::super::binary::zip(&native.members.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>())}
    }
    #[test]
    fn actual_corel_tiff_imports_keep_all_gray16_samples_in_both_versions() {
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/assets/cdr/native-gray16");let raw=std::fs::read(root.join("gray16.png")).unwrap();let expected=gray16(&raw,"image/png").unwrap().unwrap();assert_eq!((expected.width,expected.height),(13,9));assert!(expected.pixels.iter().any(|v|v%257!=0));
        for version in[14,21]{let native=read_native(&std::fs::read(root.join(format!("tiff-import-v{version}.cdr"))).unwrap()).unwrap();let records=all(&native.nodes).into_iter().filter(|n|n.tag==*b"bmp ").collect::<Vec<_>>();assert_eq!(records.len(),1);assert_eq!(gray_samples(&records[0].data),expected);}
    }
    #[test]
    fn both_writers_keep_gray16_active_samples_original_container_and_sharing() {
        let doc=gray_scene();let expected=gray16(include_bytes!("../../../../examples/assets/cdr/native-gray16/gray16.png"),"image/png").unwrap().unwrap();
        for encode in[crate::cdr::encode_document,crate::cdr::encode_document21]{let encoded=encode(&doc).unwrap().0;let native=read_native(&encoded).unwrap();let records=all(&native.nodes).into_iter().filter(|n|n.tag==*b"bmp ").collect::<Vec<_>>();assert_eq!(records.len(),1);assert_eq!(gray_samples(&records[0].data),expected);let(restored,_)=crate::cdr::decode(&encoded,"gray-source").unwrap();assert_eq!(restored,doc);assert_eq!(crate::cdr::decode(&encode(&restored).unwrap().0,"gray-source").unwrap().0,doc);}
    }
    #[test]
    fn gray16_low_bits_cannot_be_hidden_by_an_identical_rgba8_preview() {
        fn edit(nodes:&mut[super::super::binary::Node]){for n in nodes{if n.tag==*b"bmp "{let h=u32_at(&n.data,66).unwrap() as usize;let stride=u32_at(&n.data,78).unwrap() as usize;let at=118+(h-1)*stride;assert_eq!(&n.data[at..at+2],&[0,0]);n.data[at]=1;}if let Some(kids)=&mut n.children{edit(kids);}}}
        let original=gray16(include_bytes!("../../../../examples/assets/cdr/native-gray16/gray16.png"),"image/png").unwrap().unwrap();let mut changed=original.clone();changed.pixels[0]=1;assert_eq!(image::DynamicImage::ImageLuma16(original.image()).to_rgba8(),image::DynamicImage::ImageLuma16(changed.image()).to_rgba8());
        for encode in[crate::cdr::encode_document,crate::cdr::encode_document21]{let mut native=read_native(&encode(&gray_scene()).unwrap().0).unwrap();edit(&mut native.nodes);assert!(crate::cdr::decode(&repack(native),"stale").unwrap_err().message.contains("grayscale16 samples"));}
    }
    #[test]
    fn standalone_gray16_without_original_notes_still_keeps_full_samples() {
        let expected=gray16(include_bytes!("../../../../examples/assets/cdr/native-gray16/gray16.png"),"image/png").unwrap().unwrap();
        for encode in[crate::cdr::encode_document,crate::cdr::encode_document21]{let mut native=read_native(&encode(&gray_scene()).unwrap().0).unwrap();native.members.remove("metadata/metadata.xml");native.members.remove("META-INF/metadata.xml");let(doc,_)=crate::cdr::decode(&repack(native),"standalone").unwrap();assert_eq!(doc.resources.len(),1);let raw=base64::Engine::decode(&base64::engine::general_purpose::STANDARD,&doc.resources.values().next().unwrap().data_base64).unwrap();assert_eq!(gray16(&raw,"image/png").unwrap().unwrap(),expected);assert_eq!(doc.objects.iter().filter(|o|matches!(o.geometry,crate::Geometry::Image{..})).count(),2);}
    }
    #[test]
    fn unmeasured_gray16_flags_depth_and_alpha_cannot_be_accepted_as_the_new_profile() {
        fn change(nodes:&mut[super::super::binary::Node],at:usize,value:u32){for n in nodes{if n.tag==*b"bmp "{super::super::binary::set32(&mut n.data,at,value).unwrap();}if let Some(kids)=&mut n.children{change(kids,at,value);}}}
        for encode in[crate::cdr::encode_document,crate::cdr::encode_document21]{let bytes=encode(&gray_scene()).unwrap().0;for(at,value)in[(54,5),(58,2),(70,2),(74,8),(24,1)]{let mut native=read_native(&bytes).unwrap();change(&mut native.nodes,at,value);assert!(crate::cdr::decode(&repack(native),"malformed").is_err(),"offset{at} value{value}");}}
    }

    fn rgb_scene()->crate::Document {
        let raw=include_bytes!("../../../../examples/assets/cdr/native-rgb16/rgb16.png");
        let mut doc=crate::Document::new("rgb-source".into(),210.,297.).unwrap();doc.resources.insert("original-rgb".into(),crate::Resource::image(raw).unwrap());
        for (id,x)in[("rgb-a",20.),("rgb-b",80.)]{doc.objects.push(serde_json::from_value(serde_json::json!({"id":id,"page_id":"page-1","x_mm":x,"y_mm":30.,"width_mm":26.,"height_mm":18.,"geometry":{"kind":"image","resource_id":"original-rgb","fit":"stretch"}})).unwrap());}doc
    }
    fn rgb_samples(data:&[u8])->Rgb16 {
        assert_eq!(u32_at(data,54).unwrap(),12);assert_eq!(u32_at(data,58).unwrap(),1);assert_eq!(u32_at(data,70).unwrap(),1);assert_eq!(u32_at(data,74).unwrap(),48);assert_eq!(u32_at(data,24).unwrap(),0);
        let(width,height,stride)=(u32_at(data,62).unwrap(),u32_at(data,66).unwrap(),u32_at(data,78).unwrap() as usize);
        let raw=bytes(data,118,stride*height as usize).unwrap();let pixels=raw.chunks_exact(stride).rev().flat_map(|row|row[..width as usize*6].chunks_exact(6).flat_map(|p|[u16::from_le_bytes([p[4],p[5]]),u16::from_le_bytes([p[2],p[3]]),u16::from_le_bytes([p[0],p[1]])])).collect();Rgb16{width,height,pixels}
    }
    #[test]
    fn actual_corel_tiff_imports_keep_all_rgb16_samples_in_both_versions() {
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/assets/cdr/native-rgb16");let raw=std::fs::read(root.join("rgb16.png")).unwrap();let expected=rgb16(&raw,"image/png").unwrap().unwrap();assert_eq!((expected.width,expected.height),(13,9));assert!(expected.pixels.iter().any(|v|v%257!=0));
        for version in[14,21]{let native=read_native(&std::fs::read(root.join(format!("tiff-import-v{version}.cdr"))).unwrap()).unwrap();let records=all(&native.nodes).into_iter().filter(|n|n.tag==*b"bmp ").collect::<Vec<_>>();assert_eq!(records.len(),1);assert_eq!(rgb_samples(&records[0].data),expected);}
    }
    #[test]
    fn both_writers_keep_rgb16_active_samples_original_container_and_sharing() {
        let doc=rgb_scene();let expected=rgb16(include_bytes!("../../../../examples/assets/cdr/native-rgb16/rgb16.png"),"image/png").unwrap().unwrap();
        for encode in[crate::cdr::encode_document,crate::cdr::encode_document21]{let encoded=encode(&doc).unwrap().0;let native=read_native(&encoded).unwrap();let records=all(&native.nodes).into_iter().filter(|n|n.tag==*b"bmp ").collect::<Vec<_>>();assert_eq!(records.len(),1);assert_eq!(rgb_samples(&records[0].data),expected);let(restored,_)=crate::cdr::decode(&encoded,"rgb-source").unwrap();assert_eq!(restored,doc);assert_eq!(crate::cdr::decode(&encode(&restored).unwrap().0,"rgb-source").unwrap().0,doc);}
    }
    #[test]
    fn rgb16_low_bits_cannot_be_hidden_by_an_identical_rgba8_preview() {
        fn edit(nodes:&mut[super::super::binary::Node]){for n in nodes{if n.tag==*b"bmp "{let h=u32_at(&n.data,66).unwrap() as usize;let stride=u32_at(&n.data,78).unwrap() as usize;let at=118+(h-1)*stride;assert_eq!(&n.data[at+4..at+6],&[17,0]);n.data[at+4]=18;}if let Some(kids)=&mut n.children{edit(kids);}}}
        let original=rgb16(include_bytes!("../../../../examples/assets/cdr/native-rgb16/rgb16.png"),"image/png").unwrap().unwrap();let mut changed=original.clone();changed.pixels[0]=18;assert_eq!(image::DynamicImage::ImageRgb16(original.image()).to_rgba8(),image::DynamicImage::ImageRgb16(changed.image()).to_rgba8());
        for encode in[crate::cdr::encode_document,crate::cdr::encode_document21]{let mut native=read_native(&encode(&rgb_scene()).unwrap().0).unwrap();edit(&mut native.nodes);assert!(crate::cdr::decode(&repack(native),"stale").unwrap_err().message.contains("RGB16 samples"));}
    }
    #[test]
    fn standalone_rgb16_without_original_notes_still_keeps_full_samples() {
        let expected=rgb16(include_bytes!("../../../../examples/assets/cdr/native-rgb16/rgb16.png"),"image/png").unwrap().unwrap();
        for encode in[crate::cdr::encode_document,crate::cdr::encode_document21]{let mut native=read_native(&encode(&rgb_scene()).unwrap().0).unwrap();native.members.remove("metadata/metadata.xml");native.members.remove("META-INF/metadata.xml");let(doc,_)=crate::cdr::decode(&repack(native),"standalone").unwrap();assert_eq!(doc.resources.len(),1);let raw=base64::Engine::decode(&base64::engine::general_purpose::STANDARD,&doc.resources.values().next().unwrap().data_base64).unwrap();assert_eq!(rgb16(&raw,"image/png").unwrap().unwrap(),expected);assert_eq!(doc.objects.iter().filter(|o|matches!(o.geometry,crate::Geometry::Image{..})).count(),2);}
    }
    #[test]
    fn unmeasured_rgb16_flags_depth_and_alpha_cannot_be_accepted_as_the_new_profile() {
        fn change(nodes:&mut[super::super::binary::Node],at:usize,value:u32){for n in nodes{if n.tag==*b"bmp "{super::super::binary::set32(&mut n.data,at,value).unwrap();}if let Some(kids)=&mut n.children{change(kids,at,value);}}}
        for encode in[crate::cdr::encode_document,crate::cdr::encode_document21]{let bytes=encode(&rgb_scene()).unwrap().0;for(at,value)in[(54,5),(58,2),(70,2),(74,8),(24,1)]{let mut native=read_native(&bytes).unwrap();change(&mut native.nodes,at,value);assert!(crate::cdr::decode(&repack(native),"malformed").is_err(),"offset{at} value{value}");}}
    }

    #[test]
    #[ignore = "controlled RI depth hypotheses, not product support; set ARRANGER_CDR_DEPTH_PROBES"]
    fn native_gray16_record_hypotheses_preserve_every_other_resolved_record() {
        use super::super::binary::{Node,set32,read_native,repack21,write,zip};
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/assets/cdr/native-cmyk");
        let directory=std::path::PathBuf::from(std::env::var_os("ARRANGER_CDR_DEPTH_PROBES").expect("set a fresh directory"));assert!(directory.is_absolute(),"probe directory must be absolute");std::fs::create_dir(&directory).unwrap();
        let png=std::fs::read(root.join("gray16.png")).unwrap();let image=image::load_from_memory(&png).unwrap().into_luma16();assert_eq!(image.dimensions(),(13,9));
        let samples=image.into_raw();assert_eq!(samples.len(),117);assert!(samples.iter().any(|v|v%257!=0));
        let sample_bytes=samples.iter().flat_map(|v|v.to_le_bytes()).collect::<Vec<_>>();
        let mut cases=vec![];
        fn replace(nodes:&mut [Node],replacement:&[u8])->usize {let mut count=0;for n in nodes {if n.tag==*b"bmp " {n.data=replacement.to_vec();count+=1;}if let Some(kids)=&mut n.children {count+=replace(kids,replacement);}}count}
        for version in [14,21] {
            let filename=format!("gray16-v{version}.cdr");let source=std::fs::read(root.join(&filename)).unwrap();let original=read_native(&source).unwrap();
            let bitmaps=all(&original.nodes).into_iter().filter(|n|n.tag==*b"bmp ").collect::<Vec<_>>();assert_eq!(bitmaps.len(),1);let template=&bitmaps[0].data;
            for (at,value) in [(54,5),(58,1),(62,13),(66,9),(70,1),(74,8),(78,16),(82,144)] {assert_eq!(u32_at(template,at).unwrap(),value);}
            for variant in ["repacked-gray8-control","gray16-le-hypothesis","gray16-be-hypothesis"] {
                let mut native=read_native(&source).unwrap();let mut changed=template.clone();
                if variant!="repacked-gray8-control" {
                    let stride=28;let mut pixels=vec![];
                    for row in samples.chunks_exact(13).rev() {for value in row {pixels.extend(if variant=="gray16-le-hypothesis" {value.to_le_bytes()}else{value.to_be_bytes()});}pixels.resize(pixels.len()+2,0);}
                    changed.truncate(118);changed.extend(pixels);
                    for (at,value) in [(12,changed.len() as u32-8),(42,78+stride*9),(74,16),(78,stride),(82,stride*9)] {set32(&mut changed,at,value).unwrap();}
                    assert_eq!(replace(&mut native.nodes,&changed),1);
                }
                let expected=native.nodes.clone();let bytes=if version==14 {native.members.insert("content/riffData.cdr".into(),write(&native.nodes).unwrap());zip(&native.members.iter().map(|(k,v)|(k.as_str(),v.as_slice())).collect::<Vec<_>>())}else{repack21(&native).unwrap()};
                let reread=read_native(&bytes).unwrap();assert_eq!(reread.nodes,expected);
                let mut restored=reread.nodes.clone();assert_eq!(replace(&mut restored,template),1);assert_eq!(restored,original.nodes);
                let file=format!("v{version}-{variant}.cdr");std::fs::write(directory.join(&file),&bytes).unwrap();
                cases.push(serde_json::json!({"file":file,"version":version,"variant":variant,"sha256":crate::model::sha256(&bytes),"source":format!("examples/assets/cdr/native-cmyk/{filename}"),"source_sha256":crate::model::sha256(&source),"target_samples_le_sha256":crate::model::sha256(&sample_bytes),"bit_depth":if variant=="repacked-gray8-control" {8}else{16},"byte_order":if variant.ends_with("be-hypothesis") {"big"}else{"little"}}));
            }
        }
        std::fs::write(directory.join("gray16.png"),png).unwrap();
        std::fs::write(directory.join("manifest.json"),serde_json::to_vec_pretty(&serde_json::json!({"model_encoder":false,"corel_validation":"pending","source_image":"gray16.png","cases":cases,"scope":"Controlled RI model5/bpp16 byte-order hypotheses. Exact other native records and source members preserved. No product support or full-save acceptance."})).unwrap()).unwrap();
    }
}
