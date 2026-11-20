//! Windowed geometry reports. The board parser stays in the KiCad adapter.

mod dump;
mod occupied;

pub use dump::{dump_json, dump_view, find_symbol, symbol_view, view_json, DumpOptions, DumpView};
pub use occupied::{occupied_json, OccupiedOptions};
