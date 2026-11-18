//! Landscape A4 at scale 1. One face fills the page. Both faces split it.

use thiserror::Error;

use super::geometry::Bounds;
use super::side::Side;

pub const PAGE_WIDTH_MM: f64 = 297.0;
pub const PAGE_HEIGHT_MM: f64 = 210.0;
pub const MARGIN_MM: f64 = 10.0;
const GAP_MM: f64 = 6.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub width_mm: f64,
    pub height_mm: f64,
    pub views: Vec<Placement>,
}

/// Where one face sits on the page, still at scale 1.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub side: Side,
    pub band_x: f64,
    pub band_y: f64,
    pub band_w: f64,
    pub band_h: f64,
    /// Added to a board coordinate after optional mirroring.
    pub origin_x: f64,
    pub origin_y: f64,
}

#[derive(Debug, Error, PartialEq)]
#[error(
    "board {board_w:.2}×{board_h:.2} mm does not fit in the {slot_w:.2}×{slot_h:.2} mm area on A4"
)]
pub struct FitError {
    pub board_w: f64,
    pub board_h: f64,
    pub slot_w: f64,
    pub slot_h: f64,
}

/// `sides` empty means both faces, side above back.
pub fn layout(sides: &[Side], bounds: Bounds) -> Result<Page, FitError> {
    let faces: Vec<Side> = if sides.is_empty() {
        vec![Side::Side, Side::Back]
    } else {
        sides.to_vec()
    };
    let inner_w = PAGE_WIDTH_MM - 2.0 * MARGIN_MM;
    let inner_h = PAGE_HEIGHT_MM - 2.0 * MARGIN_MM;
    let (band_h, gap) = if faces.len() == 1 {
        (inner_h, 0.0)
    } else {
        let gap = GAP_MM;
        ((inner_h - gap) / faces.len() as f64, gap)
    };
    let board_w = bounds.width();
    let board_h = bounds.height();
    if board_w > inner_w + 1e-6 || board_h > band_h + 1e-6 {
        return Err(FitError {
            board_w,
            board_h,
            slot_w: inner_w,
            slot_h: band_h,
        });
    }
    let mut views = Vec::with_capacity(faces.len());
    for (index, side) in faces.iter().copied().enumerate() {
        let band_x = MARGIN_MM;
        let band_y = if faces.len() == 1 {
            MARGIN_MM
        } else {
            // Index 0 is the top band. PDF Y grows upward, so the top band has the larger Y.
            let from_top = index as f64 * (band_h + gap);
            MARGIN_MM + inner_h - band_h - from_top
        };
        let origin_x = band_x + (inner_w - board_w) / 2.0 - bounds.min_x;
        let origin_y = band_y + (band_h - board_h) / 2.0 - bounds.min_y;
        views.push(Placement {
            side,
            band_x,
            band_y,
            band_w: inner_w,
            band_h,
            origin_x,
            origin_y,
        });
    }
    Ok(Page {
        width_mm: PAGE_WIDTH_MM,
        height_mm: PAGE_HEIGHT_MM,
        views,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(w: f64, h: f64) -> Bounds {
        Bounds {
            min_x: 0.0,
            min_y: 0.0,
            max_x: w,
            max_y: h,
        }
    }

    #[test]
    fn both_faces_stack_side_above_back() {
        let page = layout(&[], bounds(20.0, 10.0)).unwrap();
        assert_eq!(page.views.len(), 2);
        assert_eq!(page.views[0].side, Side::Side);
        assert_eq!(page.views[1].side, Side::Back);
        assert!(page.views[0].band_y > page.views[1].band_y);
    }

    #[test]
    fn oversized_board_is_rejected() {
        let err = layout(&[Side::Side], bounds(400.0, 10.0)).unwrap_err();
        assert!(err.board_w > err.slot_w);
    }
}
