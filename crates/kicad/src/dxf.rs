//! KiCad DXF in millimetres, drill origin.

use anyhow::{bail, Result};
use pcbcraft_domain::drawing::{BoardGeometry, PathSeg, Point};

pub fn parse_outline(text: &str) -> Result<BoardGeometry> {
    let mut geometry = BoardGeometry::default();
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    let mut pairs = Vec::new();
    while let Some(code) = lines.next() {
        let Some(value) = lines.next() else {
            bail!("DXF ends at group code {code}");
        };
        pairs.push((code.to_string(), value.to_string()));
    }
    let mut index = 0;
    while index < pairs.len() {
        if pairs[index].0 == "0" {
            let kind = pairs[index].1.as_str();
            let start = index + 1;
            index += 1;
            while index < pairs.len() && pairs[index].0 != "0" {
                index += 1;
            }
            if matches!(kind, "LINE" | "CIRCLE" | "ARC" | "LWPOLYLINE") {
                push_entity(&mut geometry, kind, &pairs[start..index])?;
            }
        } else {
            index += 1;
        }
    }
    Ok(geometry)
}

fn push_entity(geometry: &mut BoardGeometry, kind: &str, pairs: &[(String, String)]) -> Result<()> {
    match kind {
        "LINE" => {
            let from = Point {
                x: number(pairs, "10")?,
                y: number(pairs, "20")?,
            };
            let to = Point {
                x: number(pairs, "11")?,
                y: number(pairs, "21")?,
            };
            geometry.paths.push(PathSeg::Line { from, to });
        }
        "CIRCLE" => geometry.paths.push(PathSeg::Circle {
            center: Point {
                x: number(pairs, "10")?,
                y: number(pairs, "20")?,
            },
            radius: number(pairs, "40")?,
        }),
        "ARC" => geometry.paths.push(PathSeg::Arc {
            center: Point {
                x: number(pairs, "10")?,
                y: number(pairs, "20")?,
            },
            radius: number(pairs, "40")?,
            start_deg: number(pairs, "50")?,
            end_deg: number(pairs, "51")?,
        }),
        "LWPOLYLINE" => {
            let mut points = Vec::new();
            let mut index = 0;
            while index < pairs.len() {
                if pairs[index].0 == "10" {
                    let x = pairs[index].1.parse::<f64>().unwrap_or(0.0);
                    let y = pairs
                        .get(index + 1)
                        .filter(|pair| pair.0 == "20")
                        .and_then(|pair| pair.1.parse().ok())
                        .unwrap_or(0.0);
                    points.push(Point { x, y });
                }
                index += 1;
            }
            for window in points.windows(2) {
                geometry.paths.push(PathSeg::Line {
                    from: window[0],
                    to: window[1],
                });
            }
            let closed = pairs
                .iter()
                .find(|pair| pair.0 == "70")
                .and_then(|pair| pair.1.parse::<i32>().ok())
                .unwrap_or(0)
                & 1
                == 1;
            if closed && points.len() >= 2 {
                geometry.paths.push(PathSeg::Line {
                    from: *points.last().unwrap(),
                    to: points[0],
                });
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_line_and_a_closed_polyline() {
        let text = "\
0
LINE
10
0
20
0
11
10
21
4
0
LWPOLYLINE
70
1
10
0
20
0
10
2
20
0
10
2
20
2
0
ENDSEC
";
        let geometry = parse_outline(text).unwrap();
        assert!(matches!(geometry.paths[0], PathSeg::Line { .. }));
        assert!(geometry.paths.len() >= 3);
    }
}

fn number(pairs: &[(String, String)], code: &str) -> Result<f64> {
    pairs
        .iter()
        .find(|pair| pair.0 == code)
        .and_then(|pair| pair.1.parse().ok())
        .ok_or_else(|| anyhow::anyhow!("DXF is missing group {code}"))
}
