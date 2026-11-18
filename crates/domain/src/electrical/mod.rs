//! Local checks on two-terminal parts. No board geometry and no process.

mod network;
mod value;

pub use network::{
    divider_span, parallel_resistance, series_capacitance, series_resistance, tau_span,
    worst_delta, worst_power, Span,
};
pub use value::{band, format_si, passive_kind, value_within, Kind, Tolerance};
