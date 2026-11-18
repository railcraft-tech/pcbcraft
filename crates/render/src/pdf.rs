use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use anyhow::{Context, Result};
use pcbcraft_domain::drawing::{BoardGeometry, Hole, Page, PathSeg, Placement, Point, Slot};
use printpdf::{Color, Line, Mm, PdfDocument, Point as PdfPoint, Rgb, Svg, SvgTransform};

use crate::svg_page::size_mm;

pub struct PdfRenderer;

impl PdfRenderer {
    pub fn mechanical(&self, page: &Page, geometry: &BoardGeometry, output: &Path) -> Result<()> {
        let (doc, page_id, layer_id) = PdfDocument::new(
            "pcbcraft",
            Mm(page.width_mm as f32),
            Mm(page.height_mm as f32),
            "rysunek",
        );
        let layer = doc.get_page(page_id).get_layer(layer_id);
        layer.set_outline_color(Color::Rgb(Rgb::new(0.0, 0.0, 0.0, None)));
        layer.set_outline_thickness(0.2_f32);
        for placement in &page.views {
            let drawn = if placement.side == pcbcraft_domain::drawing::Side::Back {
                geometry.mirrored_horizontally()
            } else {
                geometry.clone()
            };
            for path in &drawn.paths {
                stroke_path(&layer, placement, path);
            }
            for hole in &drawn.holes {
                stroke_circle(&layer, placement, hole);
            }
            for slot in &drawn.slots {
                stroke_slot(&layer, placement, slot);
            }
        }
        if let Some(parent) = output.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        doc.save(&mut BufWriter::new(File::create(output).with_context(
            || format!("cannot create {}", output.display()),
        )?))
        .context("writing PDF")?;
        Ok(())
    }

    pub fn color(
        &self,
        page: &Page,
        svgs: &[(pcbcraft_domain::drawing::Side, String)],
        output: &Path,
    ) -> Result<()> {
        let (doc, page_id, layer_id) = PdfDocument::new(
            "pcbcraft",
            Mm(page.width_mm as f32),
            Mm(page.height_mm as f32),
            "kolor",
        );
        let layer = doc.get_page(page_id).get_layer(layer_id);
        for (side, svg) in svgs {
            let Some(placement) = page.views.iter().find(|view| view.side == *side) else {
                continue;
            };
            let parsed = Svg::parse(svg).map_err(|err| anyhow::anyhow!("SVG: {err}"))?;
            let (width_mm, height_mm) = size_mm(svg).unwrap_or((
                parsed.width.0 as f64 * 25.4 / 72.0,
                parsed.height.0 as f64 * 25.4 / 72.0,
            ));
            let natural_w = parsed.width.0 as f32 / 72.0 * 25.4;
            let scale = if natural_w > 0.0 {
                width_mm as f32 / natural_w
            } else {
                1.0
            };
            let _ = height_mm;
            parsed.add_to_layer(
                &layer,
                SvgTransform {
                    translate_x: Some(Mm(placement.origin_x as f32).into()),
                    translate_y: Some(Mm(placement.origin_y as f32).into()),
                    scale_x: Some(scale),
                    scale_y: Some(scale),
                    dpi: Some(72.0),
                    ..SvgTransform::default()
                },
            );
        }
        if let Some(parent) = output.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        doc.save(&mut BufWriter::new(File::create(output).with_context(
            || format!("cannot create {}", output.display()),
        )?))
        .context("writing PDF")?;
        Ok(())
    }
}

fn stroke_path(layer: &printpdf::PdfLayerReference, placement: &Placement, path: &PathSeg) {
    match path {
        PathSeg::Line { from, to } => {
            layer.add_line(Line {
                points: vec![
                    (map_point(placement, *from), false),
                    (map_point(placement, *to), false),
                ],
                is_closed: false,
            });
        }
        PathSeg::Circle { center, radius } => {
            stroke_ring(layer, placement, center.x, center.y, *radius);
        }
        PathSeg::Arc {
            center,
            radius,
            start_deg,
            end_deg,
        } => {
            let mut points = Vec::new();
            let sweep = (*end_deg - *start_deg).rem_euclid(360.0);
            for step in 0..=16 {
                let deg = *start_deg + sweep * (step as f64) / 16.0;
                let rad = deg.to_radians();
                points.push((
                    map_point(
                        placement,
                        Point {
                            x: center.x + radius * rad.cos(),
                            y: center.y + radius * rad.sin(),
                        },
                    ),
                    false,
                ));
            }
            layer.add_line(Line {
                points,
                is_closed: false,
            });
        }
    }
}

fn stroke_circle(layer: &printpdf::PdfLayerReference, placement: &Placement, hole: &Hole) {
    stroke_ring(layer, placement, hole.x, hole.y, hole.diameter / 2.0);
}

fn stroke_slot(layer: &printpdf::PdfLayerReference, placement: &Placement, slot: &Slot) {
    stroke_ring(layer, placement, slot.x1, slot.y1, slot.diameter / 2.0);
    stroke_ring(layer, placement, slot.x2, slot.y2, slot.diameter / 2.0);
    layer.add_line(Line {
        points: vec![
            (
                map_point(
                    placement,
                    Point {
                        x: slot.x1,
                        y: slot.y1,
                    },
                ),
                false,
            ),
            (
                map_point(
                    placement,
                    Point {
                        x: slot.x2,
                        y: slot.y2,
                    },
                ),
                false,
            ),
        ],
        is_closed: false,
    });
}

fn stroke_ring(
    layer: &printpdf::PdfLayerReference,
    placement: &Placement,
    x: f64,
    y: f64,
    radius: f64,
) {
    let mut points = Vec::new();
    for step in 0..24 {
        let angle = std::f64::consts::TAU * (step as f64) / 24.0;
        points.push((
            map_point(
                placement,
                Point {
                    x: x + radius * angle.cos(),
                    y: y + radius * angle.sin(),
                },
            ),
            false,
        ));
    }
    layer.add_line(Line {
        points,
        is_closed: true,
    });
}

fn map_point(placement: &Placement, point: Point) -> PdfPoint {
    PdfPoint::new(
        Mm((placement.origin_x + point.x) as f32),
        Mm((placement.origin_y + point.y) as f32),
    )
}
