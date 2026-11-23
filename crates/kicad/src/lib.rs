#![forbid(unsafe_code)]

pub mod board_setup;
pub mod command;
pub mod components_db;
pub mod dxf;
pub mod erc;
pub mod excellon;
pub mod fabrication;
pub mod footprint_lib;
pub mod geometry;

pub mod gerbers;
pub mod net;
pub mod netlist;
pub mod project;
pub mod rotation_db;
pub mod used_parts;

pub use command::KiCadCli;
pub use components_db::{ProjectCards, SqliteComponents};
pub use fabrication::KiCad;
pub use footprint_lib::{register_library, render_footprint, write_footprint, EasyedaLibrary};
