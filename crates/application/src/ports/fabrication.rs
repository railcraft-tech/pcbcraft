use std::path::Path;

use pcbcraft_domain::drawing::Side;
use pcbcraft_domain::validate::{Design, Violation};

/// What a use case needs from KiCad. The adapter runs `kicad-cli`.
pub trait Fabrication {
    fn export_bom(&self, schematic: &Path, output: &Path) -> anyhow::Result<()>;
    fn export_pos(&self, board: &Path, output: &Path) -> anyhow::Result<()>;
    fn export_gerbers(&self, board: &Path, output_dir: &Path) -> anyhow::Result<()>;
    fn export_drill_split(&self, board: &Path, output_dir: &Path) -> anyhow::Result<()>;
    fn export_dxf_outline(&self, board: &Path, output: &Path) -> anyhow::Result<()>;
    fn export_excellon(&self, board: &Path, output: &Path) -> anyhow::Result<()>;
    fn export_svg(&self, board: &Path, side: Side, full: bool, output: &Path)
        -> anyhow::Result<()>;
    fn erc(&self, schematic: &Path) -> anyhow::Result<Vec<Violation>>;
    fn drc(&self, board: &Path) -> anyhow::Result<Vec<Violation>>;
    fn design(&self, schematic: &Path) -> anyhow::Result<Design>;
}
