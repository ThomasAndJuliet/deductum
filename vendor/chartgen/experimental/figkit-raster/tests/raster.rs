//! The raster output: the right size, and group opacity compositing a group as one layer.
use figkit_model::{ElementMetadata, Figure, FigureMetadata, Limits, Paint, PathCommand, Point, Primitive, Resources, Size};
use figkit_raster::{RasterOptions, render_pixmap, render_png};

/// Two identical black bars, either in one group at 0.5 or each at paint opacity 0.5.
fn figure(grouped: bool) -> Figure {
    let bar = |opacity: f64| Primitive::Path {
        commands: vec![
            PathCommand::MoveTo { to: Point { x: 4., y: 0. } },
            PathCommand::LineTo { to: Point { x: 16., y: 0. } },
            PathCommand::LineTo { to: Point { x: 16., y: 10. } },
            PathCommand::LineTo { to: Point { x: 4., y: 10. } },
            PathCommand::Close,
        ],
        paint: Paint { fill: Some("#000000".into()), stroke: None, stroke_width: 0., opacity },
        metadata: ElementMetadata::default(),
    };
    let primitives = if grouped {
        vec![Primitive::Group {
            children: vec![bar(1.), bar(1.)],
            translate: Point { x: 0., y: 0. },
            clip: None,
            opacity: 0.5,
            metadata: ElementMetadata::default(),
        }]
    } else {
        vec![bar(0.5), bar(0.5)]
    };
    Figure {
        version: 2,
        size: Size { width: 20., height: 10. },
        metadata: FigureMetadata::default(),
        background: "#ffffff".into(),
        primitives,
        resources: Resources::default(),
    }
}

fn red_at_centre(grouped: bool) -> u8 {
    let scene = figkit_scene::prepare(&figure(grouped), &Limits::default()).unwrap();
    let pixmap = render_pixmap(&scene, &RasterOptions::default()).unwrap();
    pixmap.pixel(10, 5).unwrap().demultiply().red()
}

#[test]
fn a_group_at_half_opacity_is_one_layer() {
    // One layer of black at 0.5 over white: mid grey (≈128). Two layers at 0.5 each: 0.25 of
    // white left (≈64). The group draws its overlapping children once.
    let grouped = red_at_centre(true);
    let stacked = red_at_centre(false);
    assert!((120..=136).contains(&grouped), "grouped {grouped}");
    assert!((56..=72).contains(&stacked), "stacked {stacked}");
}

#[test]
fn the_png_is_scale_pixels_per_point() {
    let scene = figkit_scene::prepare(&figure(true), &Limits::default()).unwrap();
    let png = render_png(&scene, &RasterOptions { scale: 2.4, ..Default::default() }).unwrap();
    assert_eq!(&png[1..4], b"PNG");
    // The IHDR chunk: width and height, big-endian, at bytes 16..24.
    let (w, h) = (u32::from_be_bytes(png[16..20].try_into().unwrap()), u32::from_be_bytes(png[20..24].try_into().unwrap()));
    assert_eq!((w, h), (48, 24));
}
