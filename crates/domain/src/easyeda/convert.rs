//! Classic EasyEDA footprint text (`PAD~…`, `TRACK~…`) turned into millimetres.

use serde_json::Value;
use thiserror::Error;

use super::arc;

const MM_PER_UNIT: f64 = 0.254;

#[derive(Debug, Clone, PartialEq)]
pub struct LibraryFootprint {
    pub name: String,
    pub description: String,
    pub lcsc: String,
    pub through_hole: bool,
    pub pads: Vec<Pad>,
    pub lines: Vec<Segment>,
    pub circles: Vec<Circle>,
    pub arcs: Vec<Arc>,
    pub polys: Vec<Poly>,
    pub texts: Vec<Label>,
    pub model: Option<ModelSpec>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pad {
    pub number: String,
    pub kind: PadKind,
    pub shape: PadShape,
    pub at: (f64, f64),
    pub size: (f64, f64),
    pub rotation: f64,
    pub drill: Option<Drill>,
    pub layers: Vec<Layer>,
    /// Polygon outline relative to the pad centre. Empty unless `shape` is custom.
    pub polygon: Vec<(f64, f64)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadKind {
    Smd,
    ThruHole,
    NpThruHole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadShape {
    Rect,
    Oval,
    Circle,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Drill {
    Round(f64),
    Oval(f64, f64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub start: (f64, f64),
    pub end: (f64, f64),
    pub width: f64,
    pub layer: Layer,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Circle {
    pub center: (f64, f64),
    pub radius: f64,
    pub width: f64,
    pub layer: Layer,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Arc {
    pub start: (f64, f64),
    pub mid: (f64, f64),
    pub end: (f64, f64),
    pub width: f64,
    pub layer: Layer,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Poly {
    pub points: Vec<(f64, f64)>,
    pub layer: Layer,
    /// Courtyard body outlines are an open stroke. Silk and fab regions are filled.
    pub filled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub text: String,
    pub at: (f64, f64),
    pub rotation: f64,
    pub size: f64,
    pub thickness: f64,
    pub layer: Layer,
    pub hidden: bool,
    pub mirror: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelSpec {
    pub uuid: String,
    pub translation: [f64; 3],
    pub rotation: [f64; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    FrontCopper,
    BackCopper,
    FrontSilk,
    BackSilk,
    FrontPaste,
    BackPaste,
    FrontMask,
    BackMask,
    EdgeCuts,
    Comments,
    FrontFab,
    BackFab,
    Drawings,
    Courtyard,
    AllCopper,
    AllMask,
}

impl Layer {
    pub fn kicad(self) -> &'static str {
        match self {
            Layer::FrontCopper => "F.Cu",
            Layer::BackCopper => "B.Cu",
            Layer::FrontSilk => "F.SilkS",
            Layer::BackSilk => "B.SilkS",
            Layer::FrontPaste => "F.Paste",
            Layer::BackPaste => "B.Paste",
            Layer::FrontMask => "F.Mask",
            Layer::BackMask => "B.Mask",
            Layer::EdgeCuts => "Edge.Cuts",
            Layer::Comments => "Cmts.User",
            Layer::FrontFab => "F.Fab",
            Layer::BackFab => "B.Fab",
            Layer::Drawings => "Dwgs.User",
            Layer::Courtyard => "F.CrtYd",
            Layer::AllCopper => "*.Cu",
            Layer::AllMask => "*.Mask",
        }
    }

    fn is_back(self) -> bool {
        matches!(
            self,
            Layer::BackCopper
                | Layer::BackSilk
                | Layer::BackPaste
                | Layer::BackMask
                | Layer::BackFab
        )
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EasyedaError {
    #[error("EasyEDA Pro footprints are not supported")]
    ProFormat,
    #[error("no footprint in the EasyEDA component")]
    NoFootprint,
    #[error("footprint origin is missing")]
    NoOrigin,
    #[error("footprint has no pads")]
    NoPads,
}

pub fn convert(lcsc: &str, result: &Value) -> Result<LibraryFootprint, EasyedaError> {
    let package = result
        .get("packageDetail")
        .ok_or(EasyedaError::NoFootprint)?;
    let data = decode_drawing(package.get("dataStr"))?;
    let head = data.get("head").ok_or(EasyedaError::NoOrigin)?;
    let origin_x = json_f64(head.get("x")).ok_or(EasyedaError::NoOrigin)?;
    let origin_y = json_f64(head.get("y")).ok_or(EasyedaError::NoOrigin)?;
    let c_para = head.get("c_para").cloned().unwrap_or(Value::Null);
    let shapes = data
        .get("shape")
        .and_then(Value::as_array)
        .ok_or(EasyedaError::NoPads)?;

    let mut footprint = LibraryFootprint {
        name: footprint_name(lcsc, package, &c_para),
        description: result
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string(),
        lcsc: lcsc.to_string(),
        through_hole: false,
        pads: Vec::new(),
        lines: Vec::new(),
        circles: Vec::new(),
        arcs: Vec::new(),
        polys: Vec::new(),
        texts: Vec::new(),
        model: None,
    };

    for shape in shapes {
        let Some(line) = shape.as_str() else {
            continue;
        };
        let fields: Vec<&str> = line.split('~').collect();
        match fields.first().copied() {
            Some("PAD") => {
                if let Some(pad) = pad(&fields, origin_x, origin_y) {
                    footprint.pads.push(pad);
                }
            }
            Some("HOLE") => {
                if let Some(pad) = hole(&fields, origin_x, origin_y) {
                    footprint.pads.push(pad);
                }
            }
            Some("VIA") => {
                if let Some(pad) = via(&fields, origin_x, origin_y) {
                    footprint.pads.push(pad);
                }
            }
            Some("TRACK") => footprint.lines.extend(track(&fields, origin_x, origin_y)),
            Some("RECT") => footprint
                .lines
                .extend(rectangle(&fields, origin_x, origin_y)),
            Some("CIRCLE") => {
                if let Some(circle) = circle(&fields, origin_x, origin_y) {
                    footprint.circles.push(circle);
                }
            }
            Some("ARC") => {
                if let Some(arc) = arc_shape(&fields, origin_x, origin_y) {
                    footprint.arcs.push(arc);
                }
            }
            Some("TEXT") => {
                if let Some(label) = text_label(&fields, origin_x, origin_y) {
                    footprint.texts.push(label);
                }
            }
            Some("SOLIDREGION") => {
                if let Some(poly) = solid(&fields, origin_x, origin_y) {
                    footprint.polys.push(poly);
                }
            }
            Some("SVGNODE") => {
                if footprint.model.is_none() {
                    footprint.model = model_spec(line, origin_x, origin_y);
                }
            }
            _ => {}
        }
    }

    if footprint.pads.is_empty() {
        return Err(EasyedaError::NoPads);
    }
    footprint.through_hole = footprint.pads.iter().any(|pad| pad.kind != PadKind::Smd);
    Ok(footprint)
}

fn decode_drawing(value: Option<&Value>) -> Result<Value, EasyedaError> {
    let Some(value) = value else {
        return Err(EasyedaError::NoFootprint);
    };
    if let Some(text) = value.as_str() {
        let trimmed = text.trim_start();
        if trimmed.starts_with('[') {
            return Err(EasyedaError::ProFormat);
        }
        return serde_json::from_str(text).map_err(|_| EasyedaError::NoFootprint);
    }
    if value.is_object() {
        return Ok(value.clone());
    }
    Err(EasyedaError::NoFootprint)
}

fn footprint_name(lcsc: &str, package: &Value, c_para: &Value) -> String {
    let title = package.get("title").and_then(Value::as_str).unwrap_or("");
    let package_name = c_para.get("package").and_then(Value::as_str).unwrap_or("");
    let raw = if !title.is_empty() {
        title
    } else {
        package_name
    };
    let mut name = String::new();
    let mut underscore = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '+' | '-') {
            name.push(ch);
            underscore = false;
        } else if !underscore {
            name.push('_');
            underscore = true;
        }
    }
    let name = name.trim_matches('_').to_string();
    if name.is_empty() {
        lcsc.to_string()
    } else {
        name
    }
}

fn pad(fields: &[&str], origin_x: f64, origin_y: f64) -> Option<Pad> {
    if fields.len() < 6 {
        return None;
    }
    let shape = match fields[1] {
        "RECT" => PadShape::Rect,
        "OVAL" => PadShape::Oval,
        "ELLIPSE" => PadShape::Circle,
        "POLYGON" => PadShape::Custom,
        _ => PadShape::Oval,
    };
    let at = point(num(fields, 2)?, num(fields, 3)?, origin_x, origin_y);
    let size = (mm(num(fields, 4)?), mm(num(fields, 5)?));
    if shape != PadShape::Custom && (size.0 <= 0.0 || size.1 <= 0.0) {
        return None;
    }
    let layer_id = fields
        .get(6)
        .and_then(|text| text.parse().ok())
        .unwrap_or(1);
    let hole_radius = mm(num(fields, 9).unwrap_or(0.0));
    let hole_length = mm(num(fields, 13).unwrap_or(0.0));
    let plated = fields.get(15).copied().unwrap_or("Y") != "N";
    let hole = hole_radius > 0.0;
    let kind = if hole && !plated {
        PadKind::NpThruHole
    } else if hole {
        PadKind::ThruHole
    } else {
        PadKind::Smd
    };
    let mut rotation = -num(fields, 11).unwrap_or(0.0);
    let mut polygon = Vec::new();
    let mut size = (size.0.max(0.01), size.1.max(0.01));
    if shape == PadShape::Custom {
        rotation = 0.0;
        polygon = polygon_points(
            fields.get(10).copied().unwrap_or(""),
            at,
            origin_x,
            origin_y,
        );
        if polygon.len() < 3 {
            return None;
        }
        size = (0.005, 0.005);
    }
    if shape == PadShape::Circle {
        rotation = 0.0;
    }
    Some(Pad {
        number: pad_number(fields.get(8).copied().unwrap_or("")),
        kind,
        shape,
        at,
        size,
        rotation,
        drill: drill(hole_radius, hole_length, size.0, size.1),
        layers: pad_layers(layer_id, hole),
        polygon,
    })
}

fn hole(fields: &[&str], origin_x: f64, origin_y: f64) -> Option<Pad> {
    let at = point(num(fields, 1)?, num(fields, 2)?, origin_x, origin_y);
    let diameter = mm(num(fields, 3)?) * 2.0;
    if diameter <= 0.0 {
        return None;
    }
    Some(bare_hole(at, diameter, PadKind::NpThruHole))
}

fn via(fields: &[&str], origin_x: f64, origin_y: f64) -> Option<Pad> {
    let at = point(num(fields, 1)?, num(fields, 2)?, origin_x, origin_y);
    let size = mm(num(fields, 3)?);
    let drill = mm(num(fields, 5)?) * 2.0;
    if size <= 0.0 || drill <= 0.0 {
        return None;
    }
    let mut pad = bare_hole(at, size, PadKind::ThruHole);
    pad.drill = Some(Drill::Round(drill));
    Some(pad)
}

fn bare_hole(at: (f64, f64), diameter: f64, kind: PadKind) -> Pad {
    Pad {
        number: String::new(),
        kind,
        shape: PadShape::Circle,
        at,
        size: (diameter, diameter),
        rotation: 0.0,
        drill: Some(Drill::Round(diameter)),
        layers: vec![Layer::AllCopper, Layer::AllMask],
        polygon: Vec::new(),
    }
}

fn track(fields: &[&str], origin_x: f64, origin_y: f64) -> Vec<Segment> {
    let Some(width) = num(fields, 1) else {
        return Vec::new();
    };
    let Some(layer) = graphic_layer(
        fields
            .get(2)
            .and_then(|text| text.parse().ok())
            .unwrap_or(3),
    ) else {
        return Vec::new();
    };
    let points = xy_list(fields.get(4).copied().unwrap_or(""), origin_x, origin_y);
    segments(&points, stroke(width), layer)
}

fn rectangle(fields: &[&str], origin_x: f64, origin_y: f64) -> Vec<Segment> {
    let Some(x) = num(fields, 1) else {
        return Vec::new();
    };
    let Some(y) = num(fields, 2) else {
        return Vec::new();
    };
    let Some(width) = num(fields, 3) else {
        return Vec::new();
    };
    let Some(height) = num(fields, 4) else {
        return Vec::new();
    };
    let Some(layer) = graphic_layer(
        fields
            .get(5)
            .and_then(|text| text.parse().ok())
            .unwrap_or(3),
    ) else {
        return Vec::new();
    };
    let corners = [
        point(x, y, origin_x, origin_y),
        point(x + width, y, origin_x, origin_y),
        point(x + width, y + height, origin_x, origin_y),
        point(x, y + height, origin_x, origin_y),
    ];
    let mut closed = corners.to_vec();
    closed.push(corners[0]);
    segments(&closed, stroke(num(fields, 8).unwrap_or(1.0)), layer)
}

fn circle(fields: &[&str], origin_x: f64, origin_y: f64) -> Option<Circle> {
    let center = point(num(fields, 1)?, num(fields, 2)?, origin_x, origin_y);
    let radius = mm(num(fields, 3)?);
    if radius <= 0.0 {
        return None;
    }
    let layer = graphic_layer(
        fields
            .get(5)
            .and_then(|text| text.parse().ok())
            .unwrap_or(3),
    )?;
    Some(Circle {
        center,
        radius,
        width: stroke(num(fields, 4).unwrap_or(1.0)),
        layer,
    })
}

fn arc_shape(fields: &[&str], origin_x: f64, origin_y: f64) -> Option<Arc> {
    let layer = graphic_layer(
        fields
            .get(2)
            .and_then(|text| text.parse().ok())
            .unwrap_or(3),
    )?;
    let (start, mid, end) = arc::midpoint(fields.get(4)?)?;
    Some(Arc {
        start: point(start.0, start.1, origin_x, origin_y),
        mid: point(mid.0, mid.1, origin_x, origin_y),
        end: point(end.0, end.1, origin_x, origin_y),
        width: stroke(num(fields, 1).unwrap_or(1.0)),
        layer,
    })
}

fn text_label(fields: &[&str], origin_x: f64, origin_y: f64) -> Option<Label> {
    let kind = fields.get(1).copied().unwrap_or("");
    if kind == "P" || kind == "N" {
        return None;
    }
    let text = fields.get(10).copied().unwrap_or("").trim();
    if text.is_empty() {
        return None;
    }
    let layer = graphic_layer(
        fields
            .get(7)
            .and_then(|text| text.parse().ok())
            .unwrap_or(13),
    )?;
    Some(Label {
        text: text.to_string(),
        at: point(num(fields, 2)?, num(fields, 3)?, origin_x, origin_y),
        rotation: -num(fields, 5).unwrap_or(0.0),
        size: mm(num(fields, 9).unwrap_or(7.0)).max(0.2),
        thickness: stroke(num(fields, 4).unwrap_or(1.0)),
        hidden: fields.get(12).copied() == Some("0"),
        mirror: fields.get(6).copied() == Some("1") || layer.is_back(),
        layer,
    })
}

fn solid(fields: &[&str], origin_x: f64, origin_y: f64) -> Option<Poly> {
    let layer_id: i32 = fields.get(1)?.parse().ok()?;
    if matches!(layer_id, 5 | 6 | 100 | 101) {
        return None;
    }
    let region = fields.get(4).copied().unwrap_or("solid");
    if region != "solid" && region != "npth" {
        return None;
    }
    let layer = graphic_layer(layer_id)?;
    let points = path_points(fields.get(3)?, origin_x, origin_y);
    if points.len() < 3 {
        return None;
    }
    Some(Poly {
        filled: layer != Layer::Courtyard,
        points,
        layer,
    })
}

fn model_spec(line: &str, origin_x: f64, origin_y: f64) -> Option<ModelSpec> {
    let json = json_object(line)?;
    let attrs = json.get("attrs")?;
    let uuid = attrs
        .get("uuid")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if uuid.is_empty() {
        return None;
    }
    let origin = attrs
        .get("c_origin")
        .and_then(Value::as_str)
        .unwrap_or("0,0");
    let mut parts = origin.split(',');
    let anchor_x: f64 = parts.next().unwrap_or("0").parse().unwrap_or(0.0);
    let anchor_y: f64 = parts.next().unwrap_or("0").parse().unwrap_or(0.0);
    let z = attrs
        .get("z")
        .and_then(|value| json_f64(Some(value)).or_else(|| value.as_str()?.parse().ok()))
        .unwrap_or(0.0);
    let rotation = attrs
        .get("c_rotation")
        .and_then(Value::as_str)
        .unwrap_or("0,0,0");
    let angles: Vec<f64> = rotation
        .split(',')
        .map(|text| text.trim().parse().unwrap_or(0.0))
        .collect();
    Some(ModelSpec {
        uuid: uuid.to_string(),
        translation: [mm(anchor_x - origin_x), mm(-(anchor_y - origin_y)), mm(z)],
        rotation: [
            negate_degrees(angles.first().copied().unwrap_or(0.0)),
            negate_degrees(angles.get(1).copied().unwrap_or(0.0)),
            negate_degrees(angles.get(2).copied().unwrap_or(0.0)),
        ],
    })
}

fn json_object(line: &str) -> Option<Value> {
    let start = line.find('{')?;
    let mut depth = 0;
    for (index, ch) in line[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return serde_json::from_str(&line[start..start + index + 1]).ok();
                }
            }
            _ => {}
        }
    }
    None
}

fn pad_number(text: &str) -> String {
    let text = text.trim();
    if let (Some(open), Some(close)) = (text.find('('), text.rfind(')')) {
        if close > open {
            let inner = text[open + 1..close].trim();
            if !inner.is_empty() {
                return inner.to_string();
            }
        }
    }
    text.to_string()
}

fn drill(radius: f64, length: f64, width: f64, height: f64) -> Option<Drill> {
    if radius <= 0.0 {
        return None;
    }
    if length == 0.0 {
        return Some(Drill::Round(radius * 2.0));
    }
    let span = (radius * 2.0).max(length);
    let along_height = height - span;
    let along_width = width - span;
    if along_height >= along_width {
        Some(Drill::Oval(radius * 2.0, length))
    } else {
        Some(Drill::Oval(length, radius * 2.0))
    }
}

fn pad_layers(layer: i32, hole: bool) -> Vec<Layer> {
    if hole {
        return match layer {
            2 => vec![Layer::BackCopper, Layer::BackMask],
            _ => vec![Layer::AllCopper, Layer::AllMask],
        };
    }
    match layer {
        2 => vec![Layer::BackCopper, Layer::BackPaste, Layer::BackMask],
        _ => vec![Layer::FrontCopper, Layer::FrontPaste, Layer::FrontMask],
    }
}

fn graphic_layer(id: i32) -> Option<Layer> {
    Some(match id {
        0 | 13 => Layer::FrontFab,
        1 => Layer::FrontCopper,
        2 => Layer::BackCopper,
        3 => Layer::FrontSilk,
        4 => Layer::BackSilk,
        5 => Layer::FrontPaste,
        6 => Layer::BackPaste,
        7 => Layer::FrontMask,
        8 => Layer::BackMask,
        10 => Layer::EdgeCuts,
        12 => Layer::Comments,
        14 => Layer::BackFab,
        15 => Layer::Drawings,
        99 => Layer::Courtyard,
        _ => return None,
    })
}

fn polygon_points(text: &str, at: (f64, f64), origin_x: f64, origin_y: f64) -> Vec<(f64, f64)> {
    xy_list(text, origin_x, origin_y)
        .into_iter()
        .map(|(x, y)| (x - at.0, y - at.1))
        .collect()
}

fn path_points(path: &str, origin_x: f64, origin_y: f64) -> Vec<(f64, f64)> {
    let mut points = Vec::new();
    let mut cursor = (0.0, 0.0);
    let tokens = command_tokens(path);
    for (command, args) in tokens {
        match command {
            'M' | 'L' if args.len() >= 2 => {
                cursor = (args[0], args[1]);
                points.push(point(cursor.0, cursor.1, origin_x, origin_y));
            }
            'H' if !args.is_empty() => {
                cursor.0 = args[0];
                points.push(point(cursor.0, cursor.1, origin_x, origin_y));
            }
            'V' if !args.is_empty() => {
                cursor.1 = args[0];
                points.push(point(cursor.0, cursor.1, origin_x, origin_y));
            }
            'A' if args.len() >= 7 => {
                cursor = (args[5], args[6]);
                points.push(point(cursor.0, cursor.1, origin_x, origin_y));
            }
            'Z' => {
                if let Some(first) = points.first().copied() {
                    if points.last().copied() != Some(first) {
                        points.push(first);
                    }
                }
            }
            _ => {}
        }
    }
    points
}

fn command_tokens(path: &str) -> Vec<(char, Vec<f64>)> {
    let mut commands = Vec::new();
    let mut command = None;
    let mut args = Vec::new();
    for raw in path.replace(',', " ").split_whitespace() {
        let mut chars = raw.chars();
        if let Some(letter) = chars.next().filter(|ch| ch.is_ascii_alphabetic()) {
            if chars.clone().next().is_none() || chars.clone().all(|ch| !ch.is_ascii_alphabetic()) {
                if let Some(command) = command {
                    commands.push((command, std::mem::take(&mut args)));
                }
                command = Some(letter.to_ascii_uppercase());
                let rest = &raw[letter.len_utf8()..];
                if !rest.is_empty() {
                    if let Ok(value) = rest.parse::<f64>() {
                        args.push(value);
                    }
                }
                continue;
            }
        }
        if let Ok(value) = raw.parse::<f64>() {
            args.push(value);
        }
    }
    if let Some(command) = command {
        commands.push((command, args));
    }
    commands
}

fn xy_list(text: &str, origin_x: f64, origin_y: f64) -> Vec<(f64, f64)> {
    let numbers: Vec<f64> = text
        .split_whitespace()
        .filter_map(|text| text.parse().ok())
        .collect();
    numbers
        .chunks(2)
        .filter(|pair| pair.len() == 2)
        .map(|pair| point(pair[0], pair[1], origin_x, origin_y))
        .collect()
}

fn segments(points: &[(f64, f64)], width: f64, layer: Layer) -> Vec<Segment> {
    points
        .windows(2)
        .map(|pair| Segment {
            start: pair[0],
            end: pair[1],
            width,
            layer,
        })
        .collect()
}

fn point(x: f64, y: f64, origin_x: f64, origin_y: f64) -> (f64, f64) {
    (mm(x - origin_x), mm(-(y - origin_y)))
}

fn mm(units: f64) -> f64 {
    (units * MM_PER_UNIT * 1_000_000.0).round() / 1_000_000.0
}

fn stroke(units: f64) -> f64 {
    mm(units).max(0.01)
}

fn num(fields: &[&str], index: usize) -> Option<f64> {
    let text = fields.get(index)?.trim();
    if text.is_empty() {
        return Some(0.0);
    }
    let value: f64 = text.parse().ok()?;
    value.is_finite().then_some(value)
}

fn json_f64(value: Option<&Value>) -> Option<f64> {
    let value = value?;
    if let Some(number) = value.as_f64() {
        return number.is_finite().then_some(number);
    }
    value.as_str()?.parse().ok()
}

fn negate_degrees(angle: f64) -> f64 {
    let turned = (-angle).rem_euclid(360.0);
    if turned == 0.0 {
        0.0
    } else {
        turned
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn component(shapes: Vec<&str>) -> Value {
        json!({
            "description": "test part",
            "packageDetail": {
                "title": "R/0805",
                "dataStr": {
                    "head": {"x": 4000, "y": 3000, "c_para": {"package": "ignored"}},
                    "shape": shapes
                }
            }
        })
    }

    #[test]
    fn pads_are_centred_on_the_origin_and_y_points_up() {
        let footprint = convert(
            "C1",
            &component(vec![
                "PAD~RECT~3990~3000~10~10~1~~1~0~~0~id~0~~Y~0",
                "PAD~RECT~4000~2990~10~10~1~~2~0~~90~id~0~~Y~0",
            ]),
        )
        .unwrap();
        assert_eq!(footprint.name, "R_0805");
        assert!(!footprint.through_hole);
        assert!((footprint.pads[0].at.0 + 2.54).abs() < 1e-6);
        assert!(footprint.pads[0].at.1.abs() < 1e-6);
        assert!(footprint.pads[1].at.0.abs() < 1e-6);
        assert!(
            (footprint.pads[1].at.1 - 2.54).abs() < 1e-6,
            "{:?}",
            footprint.pads[1].at
        );
        assert!((footprint.pads[1].rotation + 90.0).abs() < 1e-6);
        assert_eq!(footprint.pads[0].layers[0], Layer::FrontCopper);
    }

    #[test]
    fn a_plated_hole_becomes_a_round_drill() {
        let footprint = convert(
            "C2",
            &component(vec!["PAD~ELLIPSE~4000~3000~20~20~11~~A(1)~5~~0~id~0~~Y~0"]),
        )
        .unwrap();
        assert!(footprint.through_hole);
        assert_eq!(footprint.pads[0].number, "1");
        assert_eq!(footprint.pads[0].kind, PadKind::ThruHole);
        assert_eq!(footprint.pads[0].shape, PadShape::Circle);
        assert_eq!(footprint.pads[0].drill, Some(Drill::Round(2.54)));
        assert_eq!(footprint.pads[0].layers[0], Layer::AllCopper);
    }

    #[test]
    fn an_unplated_hole_command_is_not_a_copper_pad_number() {
        let footprint = convert(
            "C3",
            &component(vec![
                "HOLE~4010~3000~5~id~0",
                "PAD~RECT~4000~3000~10~10~1~~1~0~~0~id~0~~Y~0",
            ]),
        )
        .unwrap();
        assert_eq!(footprint.pads[0].kind, PadKind::NpThruHole);
        assert!(footprint.pads[0].number.is_empty());
        assert_eq!(footprint.pads[0].drill, Some(Drill::Round(2.54)));
    }

    #[test]
    fn a_rectangle_uses_its_four_corners_after_the_y_flip() {
        let footprint = convert(
            "C4",
            &component(vec![
                "PAD~RECT~4000~3000~4~4~1~~1~0~~0~id~0~~Y~0",
                "RECT~3990~2990~20~10~3~id~0~1~none",
            ]),
        )
        .unwrap();
        let line = &footprint.lines[0];
        assert_eq!(line.layer, Layer::FrontSilk);
        assert!((line.start.0 + 2.54).abs() < 1e-6);
        assert!((line.start.1 - 2.54).abs() < 1e-6, "{:?}", line.start);
        assert!((line.end.0 - 2.54).abs() < 1e-6);
        assert!((line.end.1 - 2.54).abs() < 1e-6);
    }

    #[test]
    fn a_polygon_pad_is_relative_to_its_centre() {
        let footprint = convert(
            "C5",
            &component(vec![
                "PAD~POLYGON~4000~3000~1~1~1~~1~0~3990 2990 4010 2990 4010 3010~0~id~0~~Y~0",
            ]),
        )
        .unwrap();
        let poly = &footprint.pads[0].polygon;
        assert_eq!(poly.len(), 3);
        assert!((poly[0].0 + 2.54).abs() < 1e-6);
        assert!((poly[0].1 - 2.54).abs() < 1e-6, "{:?}", poly[0]);
    }

    #[test]
    fn an_arc_midpoint_is_flipped_with_the_rest_of_the_drawing() {
        let footprint = convert(
            "C6",
            &component(vec![
                "PAD~RECT~4000~3000~4~4~1~~1~0~~0~id~0~~Y~0",
                "ARC~1~3~~M 4000 3000 A 5 5 0 0 1 4010 3000~~id~0",
            ]),
        )
        .unwrap();
        let arc = &footprint.arcs[0];
        assert!(arc.start.0.abs() < 1e-6 && arc.start.1.abs() < 1e-6);
        assert!((arc.end.0 - 2.54).abs() < 1e-4, "{:?}", arc.end);
        assert!(arc.end.1.abs() < 1e-4);
        // Canvas midpoint (5, -5) units becomes (1.27, 1.27) after the Y flip.
        assert!((arc.mid.0 - 1.27).abs() < 1e-3, "{:?}", arc.mid);
        assert!((arc.mid.1 - 1.27).abs() < 1e-3, "{:?}", arc.mid);
    }

    #[test]
    fn a_model_anchor_uses_the_same_origin_as_the_pads() {
        let node = r#"SVGNODE~{"attrs":{"uuid":"abc","c_origin":"4010,2990","z":"10","c_rotation":"0,0,90"}}~"#;
        let footprint = convert(
            "C7",
            &component(vec!["PAD~RECT~4000~3000~4~4~1~~1~0~~0~id~0~~Y~0", node]),
        )
        .unwrap();
        let model = footprint.model.unwrap();
        assert_eq!(model.uuid, "abc");
        assert!((model.translation[0] - 2.54).abs() < 1e-6);
        assert!((model.translation[1] - 2.54).abs() < 1e-6);
        assert!((model.translation[2] - 2.54).abs() < 1e-6);
        assert_eq!(model.rotation[2], 270.0);
    }

    #[test]
    fn pro_text_and_a_missing_origin_are_errors() {
        let pro = json!({"packageDetail": {"dataStr": "[\"PAD\"]"}});
        assert_eq!(convert("C8", &pro), Err(EasyedaError::ProFormat));
        let no_origin = json!({
            "packageDetail": {"title": "X", "dataStr": {"head": {}, "shape": ["PAD~RECT~0~0~1~1~1~~1~0~~0~id~0~~Y"]}}
        });
        assert_eq!(convert("C9", &no_origin), Err(EasyedaError::NoOrigin));
    }

    #[test]
    fn a_string_data_str_is_decoded() {
        let body = json!({
            "packageDetail": {
                "title": "SOT-23",
                "dataStr": "{\"head\":{\"x\":\"4000\",\"y\":\"3000\"},\"shape\":[\"PAD~RECT~4000~3000~4~4~1~~1~0~~0~id~0~~Y~0\"]}"
            }
        });
        let footprint = convert("C10", &body).unwrap();
        assert_eq!(footprint.name, "SOT-23");
        assert!(footprint.pads[0].at.0.abs() < 1e-9);
    }
}
