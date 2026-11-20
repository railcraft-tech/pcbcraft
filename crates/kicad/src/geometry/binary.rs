//! Compact binary form of a parsed board. It is read straight back into the
//! domain types, with no JSON tree in between, and keeps every field exactly.
//!
//! Layout: magic, format byte, a table of strings, then the board. Names such
//! as layers and nets repeat on thousands of shapes, so shapes refer to the
//! table by index. Numbers are little-endian `f64` and LEB128 varints.

use std::collections::HashMap;

use pcbcraft_domain::geometry::{
    BoardGeometry, ClearanceClass, Curve, FootprintName, Outline, PadForm, PathOutline, Point,
    Shape, Source,
};

const MAGIC: &[u8; 4] = b"PCBG";
/// Bump when the layout below changes.
const FORMAT: u8 = 1;

const SOURCES: [Source; 9] = [
    Source::Track,
    Source::Pad,
    Source::Via,
    Source::Zone,
    Source::Keepout,
    Source::Courtyard,
    Source::Hole,
    Source::Edge,
    Source::Graphic,
];

const PAD_FORMS: [PadForm; 3] = [PadForm::Smd, PadForm::Through, PadForm::NonPlated];

const FLAG_BLOCKS: u8 = 1;
const FLAG_INSET: u8 = 2;
const FLAG_INWARD: u8 = 4;
/// The clearance outline equals the occupied one, so it is stored once.
const FLAG_SAME_OUTLINE: u8 = 8;

pub fn encode(board: &BoardGeometry) -> Vec<u8> {
    let mut writer = Writer::default();
    writer.varint(board.copper_layers.len() as u64);
    for layer in &board.copper_layers {
        writer.string(layer);
    }
    writer.opt_f64(board.outer_copper_mm);
    writer.opt_f64(board.inner_copper_mm);
    writer.varint(board.footprints.len() as u64);
    for footprint in &board.footprints {
        writer.string(&footprint.reference);
        writer.string(&footprint.name);
    }
    writer.varint(board.shapes.len() as u64);
    for shape in &board.shapes {
        writer.shape(shape);
    }
    writer.finish()
}

/// `None` when the bytes are not a cache this version wrote.
pub fn decode(bytes: &[u8]) -> Option<BoardGeometry> {
    let mut reader = Reader::new(bytes)?;
    let copper_layers = reader.list(1, |reader| reader.string())?;
    let outer_copper_mm = reader.opt_f64()?;
    let inner_copper_mm = reader.opt_f64()?;
    let footprints = reader.list(2, |reader| {
        Some(FootprintName {
            reference: reader.string()?,
            name: reader.string()?,
        })
    })?;
    let shapes = reader.list(30, |reader| reader.shape())?;
    reader.done()?;
    Some(BoardGeometry {
        copper_layers,
        shapes,
        outer_copper_mm,
        inner_copper_mm,
        footprints,
    })
}

#[derive(Default)]
struct Writer {
    body: Vec<u8>,
    strings: Vec<String>,
    index: HashMap<String, u64>,
}

impl Writer {
    fn finish(self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.body.len() + 4096);
        out.extend_from_slice(MAGIC);
        out.push(FORMAT);
        put_varint(&mut out, self.strings.len() as u64);
        for text in &self.strings {
            put_varint(&mut out, text.len() as u64);
            out.extend_from_slice(text.as_bytes());
        }
        out.extend_from_slice(&self.body);
        out
    }

    fn u8(&mut self, value: u8) {
        self.body.push(value);
    }

    fn varint(&mut self, value: u64) {
        put_varint(&mut self.body, value);
    }

    fn f64(&mut self, value: f64) {
        self.body.extend_from_slice(&value.to_le_bytes());
    }

    fn opt_f64(&mut self, value: Option<f64>) {
        match value {
            Some(value) => {
                self.u8(1);
                self.f64(value);
            }
            None => self.u8(0),
        }
    }

    fn point(&mut self, point: Point) {
        self.f64(point.x);
        self.f64(point.y);
    }

    /// Index into the string table, added on first use.
    fn intern(&mut self, text: &str) -> u64 {
        if let Some(index) = self.index.get(text) {
            return *index;
        }
        let index = self.strings.len() as u64;
        self.strings.push(text.to_string());
        self.index.insert(text.to_string(), index);
        index
    }

    fn string(&mut self, text: &str) {
        let index = self.intern(text);
        self.varint(index);
    }

    /// 0 for none, otherwise the table index plus one.
    fn opt_string(&mut self, text: Option<&str>) {
        match text {
            Some(text) => {
                let index = self.intern(text);
                self.varint(index + 1);
            }
            None => self.varint(0),
        }
    }

    fn shape(&mut self, shape: &Shape) {
        let source = SOURCES.iter().position(|item| *item == shape.source);
        self.u8(source.unwrap_or(0) as u8);
        self.string(&shape.layer);
        self.opt_string(shape.reference.as_deref());
        self.opt_string(shape.pad.as_deref());
        self.opt_string(shape.net.as_deref());
        self.opt_f64(shape.width);
        let same = shape.clearance == shape.occupied;
        let mut flags = 0;
        if shape.blocks {
            flags |= FLAG_BLOCKS;
        }
        if shape.inset {
            flags |= FLAG_INSET;
        }
        if shape.inward {
            flags |= FLAG_INWARD;
        }
        if same {
            flags |= FLAG_SAME_OUTLINE;
        }
        self.u8(flags);
        match &shape.clearance_class {
            ClearanceClass::None => self.u8(0),
            ClearanceClass::Hole => self.u8(1),
            ClearanceClass::Edge => self.u8(2),
            ClearanceClass::Copper { net } => {
                self.u8(3);
                self.opt_string(net.as_deref());
            }
        }
        self.f64(shape.clearance_mm);
        self.f64(shape.mask_expansion_mm);
        self.opt_f64(shape.drill_length_mm);
        let form = shape
            .pad_form
            .and_then(|form| PAD_FORMS.iter().position(|item| *item == form));
        self.u8(form.map_or(0, |index| index as u8 + 1));
        self.opt_f64(shape.text_height_mm);
        self.outline(&shape.occupied);
        if !same {
            self.outline(&shape.clearance);
        }
    }

    fn outline(&mut self, outline: &Outline) {
        match outline {
            Outline::Circle { center, radius } => {
                self.u8(0);
                self.point(*center);
                self.f64(*radius);
            }
            Outline::Path(path) => {
                self.u8(1);
                self.point(path.start);
                self.varint(path.segments.len() as u64);
                for curve in &path.segments {
                    match curve {
                        Curve::Line { to } => {
                            self.u8(0);
                            self.point(*to);
                        }
                        Curve::Arc { to, mid } => {
                            self.u8(1);
                            self.point(*to);
                            self.point(*mid);
                        }
                    }
                }
            }
        }
    }
}

fn put_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value & 0x7f) as u8 | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
    strings: Vec<String>,
}

impl<'a> Reader<'a> {
    /// Checks the header and reads the string table.
    fn new(data: &'a [u8]) -> Option<Self> {
        let mut reader = Reader {
            data,
            at: 0,
            strings: Vec::new(),
        };
        if reader.take(MAGIC.len())? != MAGIC || reader.u8()? != FORMAT {
            return None;
        }
        let count = reader.count(1)?;
        let mut strings = Vec::with_capacity(count);
        for _ in 0..count {
            let length = reader.count(1)?;
            let bytes = reader.take(length)?;
            strings.push(String::from_utf8(bytes.to_vec()).ok()?);
        }
        reader.strings = strings;
        Some(reader)
    }

    fn done(&self) -> Option<()> {
        (self.at == self.data.len()).then_some(())
    }

    fn take(&mut self, length: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(length)?;
        let bytes = self.data.get(self.at..end)?;
        self.at = end;
        Some(bytes)
    }

    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }

    fn varint(&mut self) -> Option<u64> {
        let mut value = 0u64;
        for shift in (0..70).step_by(7) {
            let byte = self.u8()?;
            value |= u64::from(byte & 0x7f).checked_shl(shift)?;
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }

    /// A length or element count. Each element needs at least `min_bytes` more
    /// bytes, so a corrupt count cannot ask for a huge allocation.
    fn count(&mut self, min_bytes: usize) -> Option<usize> {
        let value = usize::try_from(self.varint()?).ok()?;
        let left = self.data.len() - self.at;
        (value.checked_mul(min_bytes)? <= left).then_some(value)
    }

    fn list<T>(
        &mut self,
        min_bytes: usize,
        mut item: impl FnMut(&mut Self) -> Option<T>,
    ) -> Option<Vec<T>> {
        let count = self.count(min_bytes)?;
        let mut items = Vec::with_capacity(count);
        for _ in 0..count {
            items.push(item(self)?);
        }
        Some(items)
    }

    fn f64(&mut self) -> Option<f64> {
        Some(f64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    fn opt_f64(&mut self) -> Option<Option<f64>> {
        match self.u8()? {
            0 => Some(None),
            1 => Some(Some(self.f64()?)),
            _ => None,
        }
    }

    fn point(&mut self) -> Option<Point> {
        Some(Point::new(self.f64()?, self.f64()?))
    }

    fn string(&mut self) -> Option<String> {
        let index = usize::try_from(self.varint()?).ok()?;
        self.strings.get(index).cloned()
    }

    fn opt_string(&mut self) -> Option<Option<String>> {
        match usize::try_from(self.varint()?).ok()? {
            0 => Some(None),
            index => Some(Some(self.strings.get(index - 1)?.clone())),
        }
    }

    fn shape(&mut self) -> Option<Shape> {
        let source = *SOURCES.get(usize::from(self.u8()?))?;
        let layer = self.string()?;
        let reference = self.opt_string()?;
        let pad = self.opt_string()?;
        let net = self.opt_string()?;
        let width = self.opt_f64()?;
        let flags = self.u8()?;
        let clearance_class = match self.u8()? {
            0 => ClearanceClass::None,
            1 => ClearanceClass::Hole,
            2 => ClearanceClass::Edge,
            3 => ClearanceClass::Copper {
                net: self.opt_string()?,
            },
            _ => return None,
        };
        let clearance_mm = self.f64()?;
        let mask_expansion_mm = self.f64()?;
        let drill_length_mm = self.opt_f64()?;
        let pad_form = match self.u8()? {
            0 => None,
            index => Some(*PAD_FORMS.get(usize::from(index) - 1)?),
        };
        let text_height_mm = self.opt_f64()?;
        let occupied = self.outline()?;
        let clearance = if flags & FLAG_SAME_OUTLINE != 0 {
            occupied.clone()
        } else {
            self.outline()?
        };
        Some(Shape {
            source,
            layer,
            reference,
            pad,
            net,
            width,
            blocks: flags & FLAG_BLOCKS != 0,
            clearance_class,
            clearance_mm,
            mask_expansion_mm,
            drill_length_mm,
            pad_form,
            text_height_mm,
            inset: flags & FLAG_INSET != 0,
            inward: flags & FLAG_INWARD != 0,
            occupied,
            clearance,
        })
    }

    fn outline(&mut self) -> Option<Outline> {
        match self.u8()? {
            0 => Some(Outline::Circle {
                center: self.point()?,
                radius: self.f64()?,
            }),
            1 => {
                let start = self.point()?;
                // A line is a tag and a point, so at least 17 bytes each.
                let segments = self.list(17, |reader| match reader.u8()? {
                    0 => Some(Curve::Line {
                        to: reader.point()?,
                    }),
                    1 => Some(Curve::Arc {
                        to: reader.point()?,
                        mid: reader.point()?,
                    }),
                    _ => None,
                })?;
                Some(Outline::Path(PathOutline { start, segments }))
            }
            _ => None,
        }
    }
}
