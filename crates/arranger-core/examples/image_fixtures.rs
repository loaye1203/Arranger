//! Reproducible, original synthetic PNG/JPEG fixtures; no external media.
use image::{RgbImage,RgbaImage,ImageEncoder,codecs::{jpeg::JpegEncoder,png::PngEncoder},ExtendedColorType};
use std::{fs,path::PathBuf};
fn main() {
    let out=std::env::args_os().nth(1).map(PathBuf::from).expect("output directory");fs::create_dir_all(&out).unwrap();
    let colors=[[239,74,59],[54,164,101],[45,108,192],[246,196,71]];
    let mut rgb=RgbImage::new(120,80);let mut rgba=RgbaImage::new(120,80);
    for y in 0..80 {for x in 0..120 {let n=(if x>=60 {1} else {0})+(if y>=40 {2} else {0});let c=colors[n];rgb.put_pixel(x,y,image::Rgb(c));rgba.put_pixel(x,y,image::Rgba([c[0],c[1],c[2],match n {1=>128,3=>0,_=>255}]));}}
    let mut jpeg=vec![];JpegEncoder::new_with_quality(&mut jpeg,95).encode(rgb.as_raw(),120,80,ExtendedColorType::Rgb8).unwrap();fs::write(out.join("quadrants.jpg"),jpeg).unwrap();
    let mut png=vec![];PngEncoder::new(&mut png).write_image(rgba.as_raw(),120,80,ExtendedColorType::Rgba8).unwrap();fs::write(out.join("quadrants.png"),png).unwrap();
    let gray=image::GrayImage::from_fn(32,16,|x,_|image::Luma([match x/8 {0=>16,1=>80,2=>160,_=>240}]));
    let mut jpeg=vec![];JpegEncoder::new_with_quality(&mut jpeg,95).encode(gray.as_raw(),32,16,ExtendedColorType::L8).unwrap();fs::write(out.join("grayscale.jpg"),jpeg).unwrap();
    println!("Generated original 120x80 RGBA PNG, 120x80 RGB JPEG and 32x16 grayscale JPEG in {}",out.display());
}
