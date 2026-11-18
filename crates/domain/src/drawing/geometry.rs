/// Board geometry in millimetres, Y up, origin at the drill origin.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PathSeg {
    Line {
        from: Point,
        to: Point,
    },
    Arc {
        center: Point,
        radius: f64,
        start_deg: f64,
        end_deg: f64,
    },
    Circle {
        center: Point,
        radius: f64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hole {
    pub x: f64,
    pub y: f64,
    pub diameter: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub diameter: f64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct BoardGeometry {
    pub paths: Vec<PathSeg>,
    pub holes: Vec<Hole>,
    pub slots: Vec<Slot>,
}

#[derive(Debug, Clone, Copy)]
pub struct Bounds {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl Bounds {
    pub fn width(self) -> f64 {
        self.max_x - self.min_x
    }

    pub fn height(self) -> f64 {
        self.max_y - self.min_y
    }
}

impl BoardGeometry {
    pub fn bounds(&self) -> Option<Bounds> {
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        for path in &self.paths {
            match path {
                PathSeg::Line { from, to } => {
                    xs.extend([from.x, to.x]);
                    ys.extend([from.y, to.y]);
                }
                PathSeg::Arc { center, radius, .. } | PathSeg::Circle { center, radius } => {
                    xs.extend([center.x - radius, center.x + radius]);
                    ys.extend([center.y - radius, center.y + radius]);
                }
            }
        }
        for hole in &self.holes {
            let r = hole.diameter / 2.0;
            xs.extend([hole.x - r, hole.x + r]);
            ys.extend([hole.y - r, hole.y + r]);
        }
        for slot in &self.slots {
            let r = slot.diameter / 2.0;
            xs.extend([slot.x1 - r, slot.x1 + r, slot.x2 - r, slot.x2 + r]);
            ys.extend([slot.y1 - r, slot.y1 + r, slot.y2 - r, slot.y2 + r]);
        }
        let min_x = xs.iter().copied().reduce(f64::min)?;
        let max_x = xs.iter().copied().reduce(f64::max)?;
        let min_y = ys.iter().copied().reduce(f64::min)?;
        let max_y = ys.iter().copied().reduce(f64::max)?;
        Some(Bounds {
            min_x,
            min_y,
            max_x,
            max_y,
        })
    }

    /// Mirror X around the geometry centre so the bottom reads from below.
    pub fn mirrored_horizontally(&self) -> Self {
        let Some(bounds) = self.bounds() else {
            return self.clone();
        };
        let mid = (bounds.min_x + bounds.max_x) / 2.0;
        let flip = |x: f64| mid - (x - mid);
        let flip_pt = |p: Point| Point {
            x: flip(p.x),
            y: p.y,
        };
        Self {
            paths: self
                .paths
                .iter()
                .map(|path| match path {
                    PathSeg::Line { from, to } => PathSeg::Line {
                        from: flip_pt(*from),
                        to: flip_pt(*to),
                    },
                    PathSeg::Circle { center, radius } => PathSeg::Circle {
                        center: flip_pt(*center),
                        radius: *radius,
                    },
                    PathSeg::Arc {
                        center,
                        radius,
                        start_deg,
                        end_deg,
                    } => PathSeg::Arc {
                        center: flip_pt(*center),
                        radius: *radius,
                        start_deg: 180.0 - end_deg,
                        end_deg: 180.0 - start_deg,
                    },
                })
                .collect(),
            holes: self
                .holes
                .iter()
                .map(|hole| Hole {
                    x: flip(hole.x),
                    y: hole.y,
                    diameter: hole.diameter,
                })
                .collect(),
            slots: self
                .slots
                .iter()
                .map(|slot| Slot {
                    x1: flip(slot.x1),
                    y1: slot.y1,
                    x2: flip(slot.x2),
                    y2: slot.y2,
                    diameter: slot.diameter,
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_include_the_hole_radius() {
        let geometry = BoardGeometry {
            paths: vec![PathSeg::Line {
                from: Point { x: 0.0, y: 0.0 },
                to: Point { x: 10.0, y: 4.0 },
            }],
            holes: vec![Hole {
                x: 10.0,
                y: 4.0,
                diameter: 2.0,
            }],
            slots: Vec::new(),
        };
        let bounds = geometry.bounds().unwrap();
        assert!((bounds.width() - 11.0).abs() < 1e-9);
        assert!((bounds.height() - 5.0).abs() < 1e-9);
    }

    #[test]
    fn horizontal_mirror_keeps_the_same_span() {
        let geometry = BoardGeometry {
            paths: vec![PathSeg::Line {
                from: Point { x: 0.0, y: 1.0 },
                to: Point { x: 8.0, y: 1.0 },
            }],
            holes: Vec::new(),
            slots: Vec::new(),
        };
        let mirrored = geometry.mirrored_horizontally();
        let PathSeg::Line { from, to } = &mirrored.paths[0] else {
            panic!("line");
        };
        assert!((from.x - 8.0).abs() < 1e-9);
        assert!((to.x - 0.0).abs() < 1e-9);
        assert!((from.y - 1.0).abs() < 1e-9);
    }

    #[test]
    fn empty_geometry_has_no_bounds() {
        assert!(BoardGeometry::default().bounds().is_none());
        assert!(BoardGeometry::default()
            .mirrored_horizontally()
            .paths
            .is_empty());
    }
}
