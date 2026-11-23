//! EasyEDA footprint documents, in millimetres with Y pointing up.

mod arc;
mod convert;
mod place;

pub use convert::{
    convert, Arc, Circle, Drill, EasyedaError, Label, Layer, LibraryFootprint, ModelSpec, Pad,
    PadKind, PadShape, Poly, Segment,
};
pub use place::{place_model, ModelPlacement};
