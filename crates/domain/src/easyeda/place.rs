//! Where an EasyEDA mesh sits on the footprint.

use super::convert::ModelSpec;

#[derive(Debug, Clone, PartialEq)]
pub struct ModelPlacement {
    pub offset: [f64; 3],
    pub rotation: [f64; 3],
    /// False when the OBJ mesh was missing, so the offset is only the EasyEDA anchor.
    pub bounds_known: bool,
}

pub fn place_model(spec: &ModelSpec, obj: Option<&str>) -> ModelPlacement {
    let rotation = spec.rotation;
    let Some(center) = obj.and_then(mesh_anchor) else {
        return ModelPlacement {
            offset: spec.translation,
            rotation,
            bounds_known: false,
        };
    };
    let turned = rotate(rotation, center);
    ModelPlacement {
        offset: [
            spec.translation[0] - turned[0],
            spec.translation[1] - turned[1],
            spec.translation[2] - turned[2],
        ],
        rotation,
        bounds_known: true,
    }
}

/// XY centre of the mesh, and its lowest Z. EasyEDA drops the model onto that point.
fn mesh_anchor(obj: &str) -> Option<[f64; 3]> {
    let mut min = [f64::MAX; 3];
    let mut max = [f64::MIN; 3];
    let mut any = false;
    for line in obj.lines() {
        let mut parts = line.split_whitespace();
        if parts.next() != Some("v") {
            continue;
        }
        let x: f64 = parts.next()?.parse().ok()?;
        let y: f64 = parts.next()?.parse().ok()?;
        let z: f64 = parts.next()?.parse().ok()?;
        if ![x, y, z].iter().all(|value| value.is_finite()) {
            continue;
        }
        min[0] = min[0].min(x);
        min[1] = min[1].min(y);
        min[2] = min[2].min(z);
        max[0] = max[0].max(x);
        max[1] = max[1].max(y);
        max[2] = max[2].max(z);
        any = true;
    }
    if !any {
        return None;
    }
    Some([(min[0] + max[0]) / 2.0, (min[1] + max[1]) / 2.0, min[2]])
}

/// KiCad turns the model about X, then Y, then Z, before the offset.
fn rotate(degrees: [f64; 3], vector: [f64; 3]) -> [f64; 3] {
    let rx = degrees[0].to_radians();
    let ry = degrees[1].to_radians();
    let rz = degrees[2].to_radians();

    let x1 = vector[0];
    let y1 = vector[1] * rx.cos() - vector[2] * rx.sin();
    let z1 = vector[1] * rx.sin() + vector[2] * rx.cos();

    let x2 = x1 * ry.cos() + z1 * ry.sin();
    let y2 = y1;
    let z2 = -x1 * ry.sin() + z1 * ry.cos();

    let x3 = x2 * rz.cos() - y2 * rz.sin();
    let y3 = x2 * rz.sin() + y2 * rz.cos();
    [x3, y3, z2]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(translation: [f64; 3], rotation: [f64; 3]) -> ModelSpec {
        ModelSpec {
            uuid: "abc".into(),
            translation,
            rotation,
        }
    }

    #[test]
    fn a_mesh_is_centred_and_sat_on_its_lowest_point() {
        let obj = "v 0 0 1\nv 2 0 3\nv 2 2 3\nv 0 2 1\n";
        let placed = place_model(&spec([0.0, 0.0, 0.0], [0.0, 0.0, 0.0]), Some(obj));
        assert!(placed.bounds_known);
        assert!((placed.offset[0] + 1.0).abs() < 1e-9);
        assert!((placed.offset[1] + 1.0).abs() < 1e-9);
        assert!((placed.offset[2] + 1.0).abs() < 1e-9);
    }

    #[test]
    fn the_easyeda_anchor_is_kept_when_the_mesh_is_missing() {
        let placed = place_model(&spec([1.0, 2.0, 3.0], [0.0, 0.0, 90.0]), None);
        assert!(!placed.bounds_known);
        assert_eq!(placed.offset, [1.0, 2.0, 3.0]);
        assert_eq!(placed.rotation, [0.0, 0.0, 90.0]);
    }

    #[test]
    fn rotation_about_z_moves_the_anchor_before_the_offset() {
        let obj = "v 0 0 0\nv 2 2 0\n";
        let placed = place_model(&spec([0.0, 0.0, 0.0], [0.0, 0.0, 90.0]), Some(obj));
        // Centre (1, 1, 0) turned 90° about Z becomes (-1, 1, 0).
        assert!((placed.offset[0] - 1.0).abs() < 1e-9, "{:?}", placed.offset);
        assert!((placed.offset[1] + 1.0).abs() < 1e-9, "{:?}", placed.offset);
        assert!(placed.offset[2].abs() < 1e-9);
    }
}
