//! Raster picture of the same shapes `geometry dump` prints as JSON.

use anyhow::{bail, Result};
use pcbcraft_domain::geometry::{sample_outline, Curve, Outline, Point, Shape, Source, Window};
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};

const PIXELS_PER_MM: f64 = 20.0;
const MAX_SIDE: f64 = 4096.0;
const MARGIN_MM: f64 = 1.0;
const CURVE_TOLERANCE_MM: f64 = 0.05;

struct Frame {
    origin_x: f64,
    origin_y: f64,
    scale: f64,
    width: u32,
    height: u32,
}

pub fn geometry_png(
    shapes: &[&Shape],
    window: Window,
    detailed: bool,
    precision: f64,
) -> Result<Vec<u8>> {
    if detailed && precision <= 0.0 {
        bail!("detailed precision must be positive");
    }
    let frame = frame(window);
    let mut pixmap = Pixmap::new(frame.width, frame.height).ok_or_else(|| {
        anyhow::anyhow!("cannot allocate a {}x{} image", frame.width, frame.height)
    })?;
    pixmap.fill(Color::WHITE);

    let mut ordered: Vec<&Shape> = shapes.to_vec();
    ordered.sort_by_key(|shape| paint_rank(shape.source));
    for shape in &ordered {
        if shape.blocks && !stroke_only(shape.source) {
            paint_outline(
                &mut pixmap,
                &frame,
                &shape.clearance,
                layer_color(&shape.layer),
                70,
                true,
            );
        }
    }
    for shape in &ordered {
        let (red, green, blue) = layer_color(&shape.layer);
        if stroke_only(shape.source) {
            paint_outline(
                &mut pixmap,
                &frame,
                &shape.occupied,
                (red, green, blue),
                255,
                false,
            );
        } else {
            paint_outline(
                &mut pixmap,
                &frame,
                &shape.occupied,
                (red, green, blue),
                210,
                true,
            );
        }
    }
    for shape in &ordered {
        for point in vertices(&shape.occupied) {
            paint_dot(&mut pixmap, &frame, point);
        }
        if detailed {
            for point in sample_outline(&shape.occupied, precision) {
                paint_dot(&mut pixmap, &frame, point);
            }
        }
    }
    encode(&pixmap)
}

fn frame(window: Window) -> Frame {
    let span_x = (window.x_max - window.x_min).abs() + 2.0 * MARGIN_MM;
    let span_y = (window.y_max - window.y_min).abs() + 2.0 * MARGIN_MM;
    let span = span_x.max(span_y).max(0.1);
    let scale = PIXELS_PER_MM.min(MAX_SIDE / span);
    let width = ((span_x * scale).ceil() as u32).clamp(1, MAX_SIDE as u32);
    let height = ((span_y * scale).ceil() as u32).clamp(1, MAX_SIDE as u32);
    Frame {
        origin_x: window.x_min - MARGIN_MM,
        origin_y: window.y_max + MARGIN_MM,
        scale,
        width,
        height,
    }
}

fn paint_rank(source: Source) -> u8 {
    match source {
        Source::Zone => 0,
        Source::Track => 1,
        Source::Pad => 2,
        Source::Via => 3,
        Source::Keepout => 4,
        Source::Courtyard => 5,
        Source::Graphic => 6,
        Source::Hole => 7,
        Source::Edge => 8,
    }
}

fn stroke_only(source: Source) -> bool {
    matches!(source, Source::Edge | Source::Courtyard | Source::Keepout)
}

fn layer_color(layer: &str) -> (u8, u8, u8) {
    match layer {
        "F.Cu" => (180, 40, 40),
        "B.Cu" => (40, 140, 60),
        "drill" => (0, 0, 0),
        "Edge.Cuts" => (220, 180, 0),
        "F.SilkS" => (180, 150, 30),
        "B.SilkS" => (70, 70, 120),
        "F.Mask" => (0, 110, 70),
        "B.Mask" => (0, 80, 90),
        "F.Paste" | "B.Paste" => (150, 150, 150),
        "F.Fab" | "B.Fab" => (110, 110, 130),
        other if other.ends_with(".Cu") => (110, 60, 30),
        _ => (100, 100, 100),
    }
}

fn paint_outline(
    pixmap: &mut Pixmap,
    frame: &Frame,
    outline: &Outline,
    rgb: (u8, u8, u8),
    alpha: u8,
    fill: bool,
) {
    let points = sample_outline(outline, CURVE_TOLERANCE_MM);
    if points.len() < 2 {
        return;
    }
    let mut path = PathBuilder::new();
    let (x, y) = to_px(frame, points[0]);
    path.move_to(x, y);
    for point in points.iter().skip(1) {
        let (x, y) = to_px(frame, *point);
        path.line_to(x, y);
    }
    if fill {
        path.close();
    }
    let Some(path) = path.finish() else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(rgb.0, rgb.1, rgb.2, alpha));
    paint.anti_alias = true;
    if fill {
        pixmap.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    } else {
        let stroke = Stroke {
            width: (0.2 * frame.scale) as f32,
            ..Stroke::default()
        };
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

fn paint_dot(pixmap: &mut Pixmap, frame: &Frame, point: Point) {
    let (x, y) = to_px(frame, point);
    let radius = 1.5_f32;
    let mut path = PathBuilder::new();
    path.push_circle(x, y, radius);
    let Some(path) = path.finish() else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(20, 20, 20, 255));
    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

fn vertices(outline: &Outline) -> Vec<Point> {
    match outline {
        Outline::Circle { center, .. } => vec![*center],
        Outline::Path(path) => {
            let mut points = vec![path.start];
            for curve in &path.segments {
                match curve {
                    Curve::Line { to } => points.push(*to),
                    Curve::Arc { to, mid } => {
                        points.push(*mid);
                        points.push(*to);
                    }
                }
            }
            points
        }
    }
}

fn to_px(frame: &Frame, point: Point) -> (f32, f32) {
    let x = (point.x - frame.origin_x) * frame.scale;
    let y = (frame.origin_y - point.y) * frame.scale;
    (x as f32, y as f32)
}

fn encode(pixmap: &Pixmap) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, pixmap.width(), pixmap.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&straight_alpha(pixmap.data()))?;
    drop(writer);
    Ok(bytes)
}

fn straight_alpha(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    for pixel in data.chunks_exact(4) {
        let alpha = pixel[3];
        if alpha == 0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        let scale = 255.0 / f32::from(alpha);
        out.push(((f32::from(pixel[0]) * scale).round() as u16).min(255) as u8);
        out.push(((f32::from(pixel[1]) * scale).round() as u16).min(255) as u8);
        out.push(((f32::from(pixel[2]) * scale).round() as u16).min(255) as u8);
        out.push(alpha);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pcbcraft_domain::geometry::{ClearanceClass, Curve, PathOutline, Point, Shape, Source};

    fn rect(layer: &str, source: Source) -> Shape {
        Shape {
            source,
            layer: layer.into(),
            reference: None,
            pad: None,
            net: None,
            width: None,
            blocks: source != Source::Graphic,
            clearance_class: ClearanceClass::None,
            clearance_mm: 0.0,
            mask_expansion_mm: 0.0,
            drill_length_mm: None,
            pad_form: None,
            text_height_mm: None,
            inset: false,
            inward: false,
            occupied: Outline::Path(PathOutline {
                start: Point::new(0.0, 0.0),
                segments: vec![
                    Curve::Line {
                        to: Point::new(10.0, 0.0),
                    },
                    Curve::Line {
                        to: Point::new(10.0, 10.0),
                    },
                    Curve::Line {
                        to: Point::new(0.0, 10.0),
                    },
                    Curve::Line {
                        to: Point::new(0.0, 0.0),
                    },
                ],
            }),
            clearance: Outline::Path(PathOutline {
                start: Point::new(0.0, 0.0),
                segments: vec![
                    Curve::Line {
                        to: Point::new(10.0, 0.0),
                    },
                    Curve::Line {
                        to: Point::new(10.0, 10.0),
                    },
                    Curve::Line {
                        to: Point::new(0.0, 10.0),
                    },
                    Curve::Line {
                        to: Point::new(0.0, 0.0),
                    },
                ],
            }),
        }
    }

    fn circle() -> Shape {
        Shape {
            source: Source::Pad,
            layer: "F.Cu".into(),
            reference: None,
            pad: Some("1".into()),
            net: None,
            width: None,
            blocks: true,
            clearance_class: ClearanceClass::None,
            clearance_mm: 0.0,
            mask_expansion_mm: 0.0,
            drill_length_mm: None,
            pad_form: None,
            text_height_mm: None,
            inset: false,
            inward: false,
            occupied: Outline::Circle {
                center: Point::new(20.0, 5.0),
                radius: 2.0,
            },
            clearance: Outline::Circle {
                center: Point::new(20.0, 5.0),
                radius: 2.0,
            },
        }
    }

    fn decode(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut reader = decoder.read_info().unwrap();
        let mut buffer = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buffer).unwrap();
        buffer.truncate(info.buffer_size());
        (info.width, info.height, buffer)
    }

    #[test]
    fn a_rectangle_and_a_circle_land_on_the_picture() {
        let rectangle = rect("F.Cu", Source::Pad);
        let round = circle();
        let shapes = [&rectangle, &round];
        let window = Window {
            x_min: 0.0,
            x_max: 22.0,
            y_min: 0.0,
            y_max: 10.0,
        };
        let bytes = geometry_png(&shapes, window, false, 0.01).unwrap();
        let (width, height, pixels) = decode(&bytes);
        assert!(width > 10 && height > 10);
        assert!(width <= 4096 && height <= 4096);
        let frame = frame(window);
        let (cx, cy) = to_px(&frame, Point::new(5.0, 5.0));
        assert!(!is_white(&pixels, width, cx as u32, cy as u32));
        let (kx, ky) = to_px(&frame, Point::new(20.0, 5.0));
        assert!(!is_white(&pixels, width, kx as u32, ky as u32));
        assert!(is_white(&pixels, width, 0, 0));
    }

    fn is_white(pixels: &[u8], width: u32, x: u32, y: u32) -> bool {
        let index = ((y * width + x) * 4) as usize;
        pixels[index] > 250 && pixels[index + 1] > 250 && pixels[index + 2] > 250
    }
}
