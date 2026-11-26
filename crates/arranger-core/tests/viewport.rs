use arranger_core::{Document,Geometry,Object,Style,render::Renderer};

fn rectangle(id:&str,x:f64,y:f64,w:f64,h:f64)->Object {
    Object{id:id.into(),page_id:"page-1".into(),parent_id:None,x_mm:x,y_mm:y,width_mm:w,height_mm:h,
        transform:[1.,0.,0.,1.,0.,0.],style:Style{fill:"#e63724".into(),..Default::default()},visible:true,locked:false,clip:None,geometry:Geometry::Rect{radius_mm:0.}}
}
#[test]
fn viewport_is_a_geometry_correct_crop_at_the_requested_physical_density(){
    let mut doc=Document::new("crop".into(),25.4,25.4).unwrap();doc.objects.push(rectangle("rect",5.08,5.08,7.62,7.62));
    let renderer=Renderer::new();let whole=renderer.render(&doc,"page-1",100.,None).unwrap().pixels;
    let crop=renderer.render_viewport(&doc,"page-1",100./25.4,[2.54,2.54,12.7,12.7]).unwrap().pixels;
    assert_eq!((crop.width(),crop.height()),(50,50));
    for y in 0..50{for x in 0..50{let actual=crop.pixel(x,y).unwrap();let expected=whole.pixel(x+10,y+10).unwrap();
        assert_eq!(actual,expected,"wrong crop pixel at {x},{y}");
    }}
}
#[test]
fn high_zoom_large_page_allocates_only_the_visible_region(){
    let mut doc=Document::new("large".into(),5000.,5000.).unwrap();doc.objects.push(rectangle("rect",1005.,2005.,5.,5.));
    let pixels=Renderer::new().render_viewport(&doc,"page-1",50.,[1000.,2000.,20.,16.]).unwrap().pixels;
    assert_eq!((pixels.width(),pixels.height()),(1000,800));
    assert_eq!(pixels.pixel(375,375).unwrap().red(),230);assert_eq!(pixels.pixel(375,375).unwrap().green(),55);
    assert_eq!(pixels.pixel(100,100).unwrap().red(),255);
}
#[test]
fn oversized_visible_surface_returns_an_error_before_allocation(){
    let doc=Document::new("limit".into(),210.,297.).unwrap();
    assert!(matches!(Renderer::new().render_viewport(&doc,"page-1",100.,[0.,0.,100.,100.]),Err(e) if e.code=="RENDER_SIZE_LIMIT"));
}
