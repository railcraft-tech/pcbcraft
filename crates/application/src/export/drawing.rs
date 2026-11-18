use anyhow::{bail, Result};
use pcbcraft_domain::drawing::{layout, BoardGeometry, Page, Side};

/// Scale-1 A4 placement. An empty `sides` stacks front above back.
pub fn mechanical_page(sides: &[Side], geometry: &BoardGeometry) -> Result<Page> {
    let Some(bounds) = geometry.bounds() else {
        bail!("board has no outline or holes");
    };
    layout(sides, bounds).map_err(|err| anyhow::anyhow!("{err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pcbcraft_domain::drawing::{BoardGeometry, Hole, PathSeg, Point, Side};

    #[test]
    fn a_small_board_gets_two_bands() {
        let geometry = BoardGeometry {
            paths: vec![PathSeg::Line {
                from: Point { x: 0.0, y: 0.0 },
                to: Point { x: 20.0, y: 10.0 },
            }],
            holes: vec![Hole {
                x: 1.0,
                y: 1.0,
                diameter: 0.5,
            }],
            slots: Vec::new(),
        };
        let page = mechanical_page(&[], &geometry).unwrap();
        assert_eq!(page.views.len(), 2);
        assert!(mechanical_page(&[Side::Side], &BoardGeometry::default()).is_err());
    }
}
