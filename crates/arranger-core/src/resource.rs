//! Validated PNG/JPEG blobs. Native bytes are retained once per resource.
use crate::{Document,Error,Geometry,Resource,Result,model::sha256,command::Notice};
use base64::{Engine as _,engine::general_purpose::STANDARD};
use image::{ImageReader,ImageFormat,Limits};
use serde::Serialize;
use serde_json::{Value,json};
use std::{collections::VecDeque,io::{Cursor,Read},path::Path,sync::{Mutex,OnceLock}};

pub const MAX_PIXELS:u64=64_000_000;
pub const MAX_ENCODED_BYTES:u64=64*1024*1024;
#[derive(Clone,Serialize)]
pub struct ImageInfo {pub mime:String,pub width_px:u32,pub height_px:u32,pub encoded_bytes:usize,pub metadata:ImageMetadata}
#[derive(Clone,Serialize,Default)]
pub struct ImageMetadata {pub records:Vec<MetadataRecord>,pub jpeg_components:Option<u8>,pub jpeg_sample_precision:Option<u8>,pub png_bit_depth:Option<u8>,pub png_color_type:Option<u8>}
#[derive(Clone,Serialize)]
pub struct MetadataRecord {pub kind:String,pub offset:usize,pub encoded_bytes:usize,pub sha256:String}
impl ImageMetadata {
    pub fn needs_precision_conversion(&self)->bool {self.png_bit_depth.is_some_and(|v|v>8) || self.jpeg_sample_precision.is_some_and(|v|v>8)}
    pub fn needs_preservation_codec(&self)->bool {self.needs_managed_interpretation() || self.needs_precision_conversion()}
    pub fn needs_managed_interpretation(&self)->bool {self.jpeg_components==Some(4) || self.records.iter().any(|r|matches!(r.kind.as_str(),"png_icc"|"png_exif"|"jpeg_icc"|"jpeg_exif"|"jpeg_adobe_ycck"))}
}
/// Describe original container records without interpreting/converting them.
/// Record hashes include the exact payload bytes; the Resource hash covers the
/// whole original file, including order, headers and checksums.
pub fn metadata(bytes:&[u8],mime:&str)->Result<ImageMetadata> {
    let mut out=ImageMetadata::default();
    fn take(bytes:&[u8],at:usize,n:usize)->Result<&[u8]> {bytes.get(at..at.checked_add(n).ok_or_else(||invalid("metadata length overflow"))?).ok_or_else(||invalid("truncated image metadata"))}
    fn add(out:&mut ImageMetadata,kind:&str,offset:usize,data:&[u8]) {out.records.push(MetadataRecord{kind:kind.into(),offset,encoded_bytes:data.len(),sha256:sha256(data)});}
    if mime=="image/png" {
        if !bytes.starts_with(b"\x89PNG\r\n\x1a\n"){return Err(invalid("PNG signature missing"));}
        let mut at=8;
        while at<bytes.len() {
            let size=u32::from_be_bytes(take(bytes,at,4)?.try_into().unwrap()) as usize;let tag=take(bytes,at+4,4)?;let payload=take(bytes,at+8,size)?;take(bytes,at+8+size,4)?;
            if tag==b"IHDR" {if payload.len()!=13{return Err(invalid("invalid PNG header length"));}out.png_bit_depth=Some(payload[8]);out.png_color_type=Some(payload[9]);}
            let kind=match tag {b"iCCP"=>Some("png_icc"),b"eXIf"=>Some("png_exif"),b"tEXt"=>Some("png_text"),b"zTXt"=>Some("png_compressed_text"),b"iTXt"=>Some("png_international_text"),_=>None};
            if let Some(kind)=kind{add(&mut out,kind,at+8,payload);}
            at=at.checked_add(size).and_then(|v|v.checked_add(12)).ok_or_else(||invalid("PNG metadata length overflow"))?;
            if tag==b"IEND"{break;}
        }
    } else if mime=="image/jpeg" {
        if !bytes.starts_with(&[0xff,0xd8]){return Err(invalid("JPEG signature missing"));}
        let mut at=2;
        while at<bytes.len() {
            if take(bytes,at,1)?!=[0xff]{return Err(invalid("invalid JPEG metadata marker"));}
            while at<bytes.len() && bytes[at]==0xff{at+=1;}
            let marker=take(bytes,at,1)?[0];at+=1;if matches!(marker,0xda|0xd9){break;}if marker==1 || (0xd0..=0xd7).contains(&marker){continue;}
            let size=u16::from_be_bytes(take(bytes,at,2)?.try_into().unwrap()) as usize;if size<2{return Err(invalid("invalid JPEG metadata size"));}let payload=take(bytes,at+2,size-2)?;
            if [0xc0,0xc1,0xc2].contains(&marker){out.jpeg_components=payload.get(5).copied();out.jpeg_sample_precision=payload.first().copied();}
            let kind=if marker==0xe1 && payload.starts_with(b"Exif\0\0"){Some("jpeg_exif")}else if marker==0xe2 && payload.starts_with(b"ICC_PROFILE\0"){Some("jpeg_icc")}else if marker==0xee && payload.starts_with(b"Adobe") && payload.last()==Some(&2){Some("jpeg_adobe_ycck")}else{None};
            if let Some(kind)=kind{add(&mut out,kind,at+2,payload);}at+=size;
        }
    } else {return Err(invalid("Metadata inventory supports PNG/JPEG only"));}
    Ok(out)
}
static IMAGE_CACHE:OnceLock<Mutex<VecDeque<(String,ImageInfo)>>>=OnceLock::new();
fn cached(hash:&str)->Option<ImageInfo> {IMAGE_CACHE.get_or_init(Default::default).lock().expect("image validation cache").iter().find(|(key,_)|key==hash).map(|(_,info)|info.clone())}
fn invalid(e:impl std::fmt::Display)->Error {Error::new("INVALID_RESOURCE",e.to_string())}

pub fn inspect(bytes:&[u8],mime:&str)->Result<ImageInfo> {
    inspect_with_digest(bytes,mime,None)
}
fn inspect_with_digest(bytes:&[u8],mime:&str,verified_digest:Option<&str>)->Result<ImageInfo> {
    if bytes.len() as u64>MAX_ENCODED_BYTES {return Err(invalid("Image exceeds the 64 MiB encoded size limit"));}
    let format=image::guess_format(bytes).map_err(invalid)?;
    let actual=match format {ImageFormat::Png=>"image/png",ImageFormat::Jpeg=>"image/jpeg",_=>return Err(invalid("Only PNG and JPEG images are supported"))};
    if actual!=mime {return Err(invalid("Image MIME does not match its encoded format"));}
    let computed;
    let key=match verified_digest {Some(digest)=>digest,None=>{computed=sha256(bytes);&computed}};
    let cache=IMAGE_CACHE.get_or_init(Default::default);
    if let Some(info)=cached(key) {return Ok(info);}
    let mut reader=ImageReader::with_format(Cursor::new(bytes),format);
    let mut limits=Limits::default();limits.max_alloc=Some(MAX_PIXELS*8);reader.limits(limits.clone());
    let (width,height)=reader.into_dimensions().map_err(invalid)?;
    if u64::from(width)*u64::from(height)>MAX_PIXELS {return Err(invalid("Image exceeds the 64 million pixel limit"));}
    // Use the same codecs/options as resvg. image's JPEG adapter explicitly
    // disables strict mode, so its successful decode alone cannot establish
    // that the actual resvg decoder will accept and display the blob.
    if format==ImageFormat::Png {resvg::tiny_skia::Pixmap::decode_png(bytes).map_err(invalid)?;}
    else {
        use zune_jpeg::zune_core::{colorspace::ColorSpace,options::DecoderOptions};
        let options=DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA);
        let mut decoder=zune_jpeg::JpegDecoder::new_with_options(Cursor::new(bytes),options);
        decoder.decode_headers().map_err(invalid)?;
        if decoder.output_colorspace()!=Some(ColorSpace::RGBA) {return Err(invalid("JPEG could not be converted to the renderer's RGBA format"));}
        decoder.decode().map_err(invalid)?;
    }
    let info=ImageInfo{mime:actual.into(),width_px:width,height_px:height,encoded_bytes:bytes.len(),metadata:metadata(bytes,mime)?};
    let mut cache=cache.lock().expect("image validation cache");if cache.len()==128 {cache.pop_front();}cache.push_back((key.to_owned(),info.clone()));Ok(info)
}
pub fn validate(resource:&Resource)->Result<ImageInfo> {
    let bytes=STANDARD.decode(&resource.data_base64).map_err(invalid)?;
    if sha256(&bytes)!=resource.sha256 {return Err(invalid("Image SHA-256 does not match its original bytes"));}
    inspect_with_digest(&bytes,&resource.mime,Some(&resource.sha256))
}
pub fn from_bytes(bytes:&[u8])->Result<Resource> {
    let mime=match image::guess_format(bytes).map_err(invalid)? {ImageFormat::Png=>"image/png",ImageFormat::Jpeg=>"image/jpeg",_=>return Err(invalid("Only PNG and JPEG images are supported"))};
    inspect(bytes,mime)?;
    Ok(Resource{mime:mime.into(),sha256:sha256(bytes),data_base64:STANDARD.encode(bytes)})
}
pub fn import(path:&Path)->Result<Resource> {
    if !path.is_absolute() {return Err(Error::new("INVALID_PATH","Image import requires an absolute file path"));}
    let file=std::fs::File::open(path).map_err(|e|Error::new("IO_ERROR",e.to_string()))?;
    let mut bytes=vec![];file.take(MAX_ENCODED_BYTES+1).read_to_end(&mut bytes).map_err(|e|Error::new("IO_ERROR",e.to_string()))?;
    from_bytes(&bytes)
}
pub fn query(doc:&Document,id:&str)->Result<Value> {
    let resource=doc.resources.get(id).ok_or_else(||Error::new("RESOURCE_NOT_FOUND",id))?;
    let info=validate(resource)?;
    let objects:Vec<_>=doc.objects.iter().filter_map(|o|match &o.geometry {Geometry::Image{resource_id,..} if resource_id==id=>Some(&o.id),_=>None}).collect();
    Ok(json!({"resource_id":id,"resource":resource,"image":info,"object_ids":objects,"reference_count":objects.len()}))
}

/// Diagnostics do not mutate/convert original resource bytes or orientation.
pub fn warnings(doc:&Document)->Vec<Notice> {
    let mut out=vec![];
    for (id,r) in &doc.resources {
        if !doc.objects.iter().any(|o|matches!(&o.geometry,Geometry::Image{resource_id,..} if resource_id==id)){continue;}
        if let Ok(info)=cached(&r.sha256).map(Ok).unwrap_or_else(||validate(r)) {
            if info.metadata.needs_precision_conversion(){out.push(Notice::new("IMAGE_PRECISION_REDUCED","Original high-bit-depth image bytes remain intact in .arranger and embedded SVG resources. Arranger screen/PNG/PDF previews use 8-bit RGBA and can lose low-order sample precision; this is not full-depth interchange."));}
            if info.metadata.needs_managed_interpretation() {out.push(Notice::new("IMAGE_METADATA_UNMANAGED",&format!("Image {id} retains its exact original container and metadata in .arranger. Arranger does not apply ICC color management or EXIF auto-orientation; CMYK raster conversion is unmanaged. SVG embeds original resources; external viewers may interpret their metadata differently. Original-byte preservation is not a managed appearance/print guarantee.")));}
        }
    }
    out
}
