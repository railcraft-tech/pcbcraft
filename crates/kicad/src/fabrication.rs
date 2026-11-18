use std::path::{Path, PathBuf};

use anyhow::Result;
use pcbcraft_application::ports::{Fabrication, NetGraphSource};
use pcbcraft_domain::drawing::Side;
use pcbcraft_domain::net::NetGraph;
use pcbcraft_domain::validate::{Design, Violation};

use crate::command::KiCadCli;
use crate::erc::parse_report;
use crate::netlist::parse_design;

pub struct KiCad {
    pub cli: KiCadCli,
}

impl KiCad {
    pub fn from_env() -> Self {
        Self {
            cli: KiCadCli::from_env(),
        }
    }

    /// Orthographic top view of the 3D board. `frame` aims the camera at one square.
    pub fn render_top(
        &self,
        board: &Path,
        output: &Path,
        pixels: u32,
        side: &str,
        frame: Option<pcbcraft_domain::geometry::RenderFrame>,
    ) -> Result<()> {
        let mut args = vec![
            "pcb".into(),
            "render".into(),
            "--side".into(),
            side.into(),
            "--quality".into(),
            "basic".into(),
            "--background".into(),
            "transparent".into(),
            "--width".into(),
            pixels.to_string(),
            "--height".into(),
            pixels.to_string(),
            "--output".into(),
            output.display().to_string(),
        ];
        if let Some(frame) = frame {
            args.push("--pivot".into());
            args.push(format!("{},{},0", frame.pivot_x_cm, frame.pivot_y_cm));
            args.push("--zoom".into());
            args.push(frame.zoom.to_string());
        }
        args.push(board.display().to_string());
        self.cli.run_to_file(&args)
    }
}

impl NetGraphSource for KiCad {
    fn load(&self, project: &Path) -> Result<NetGraph> {
        let resolved = crate::project::resolve(Some(project), None, None)?;
        crate::net::load_graph(&self.cli, &resolved)
    }
}

impl Fabrication for KiCad {
    fn export_bom(&self, schematic: &Path, output: &Path) -> Result<()> {
        self.cli.run_to_file(&[
            "sch".into(),
            "export".into(),
            "bom".into(),
            "--output".into(),
            output.display().to_string(),
            "--exclude-dnp".into(),
            "--fields".into(),
            "Reference,Value,Footprint,LCSC Part #".into(),
            "--labels".into(),
            "Designator,Comment,Footprint,LCSC Part #".into(),
            "--group-by".into(),
            "Footprint,LCSC Part #".into(),
            "--ref-delimiter".into(),
            ",".into(),
            "--ref-range-delimiter".into(),
            String::new(),
            schematic.display().to_string(),
        ])
    }

    fn export_pos(&self, board: &Path, output: &Path) -> Result<()> {
        self.cli.run_to_file(&[
            "pcb".into(),
            "export".into(),
            "pos".into(),
            "--output".into(),
            output.display().to_string(),
            "--format".into(),
            "csv".into(),
            "--units".into(),
            "mm".into(),
            "--side".into(),
            "both".into(),
            "--exclude-dnp".into(),
            board.display().to_string(),
        ])
    }

    fn export_gerbers(&self, board: &Path, output_dir: &Path) -> Result<()> {
        self.cli.run_to_file(&[
            "pcb".into(),
            "export".into(),
            "gerbers".into(),
            "--board-plot-params".into(),
            "--check-zones".into(),
            "--output".into(),
            output_dir.display().to_string(),
            board.display().to_string(),
        ])
    }

    fn export_drill_split(&self, board: &Path, output_dir: &Path) -> Result<()> {
        self.cli.run_to_file(&[
            "pcb".into(),
            "export".into(),
            "drill".into(),
            "--excellon-separate-th".into(),
            "--output".into(),
            output_dir.display().to_string(),
            board.display().to_string(),
        ])
    }

    fn export_dxf_outline(&self, board: &Path, output: &Path) -> Result<()> {
        self.cli.run_to_file(&[
            "pcb".into(),
            "export".into(),
            "dxf".into(),
            "--layers".into(),
            "Edge.Cuts".into(),
            "--output-units".into(),
            "mm".into(),
            "--use-drill-origin".into(),
            "--exclude-refdes".into(),
            "--exclude-value".into(),
            "--mode-single".into(),
            "--output".into(),
            output.display().to_string(),
            board.display().to_string(),
        ])
    }

    fn export_excellon(&self, board: &Path, output: &Path) -> Result<()> {
        self.cli.run_to_file(&[
            "pcb".into(),
            "export".into(),
            "drill".into(),
            "--format".into(),
            "excellon".into(),
            "--excellon-units".into(),
            "mm".into(),
            "--drill-origin".into(),
            "plot".into(),
            "--excellon-zeros-format".into(),
            "decimal".into(),
            "--output".into(),
            output.display().to_string(),
            board.display().to_string(),
        ])
    }

    fn export_svg(&self, board: &Path, side: Side, full: bool, output: &Path) -> Result<()> {
        let layers = match (side, full) {
            (Side::Side, false) => "Edge.Cuts",
            (Side::Back, false) => "Edge.Cuts",
            (Side::Side, true) => "F.Cu,F.Mask,F.SilkS,F.Paste,Edge.Cuts",
            (Side::Back, true) => "B.Cu,B.Mask,B.SilkS,B.Paste,Edge.Cuts",
        };
        let mut args = vec![
            "pcb".into(),
            "export".into(),
            "svg".into(),
            "--page-size-mode".into(),
            "2".into(),
            "--exclude-drawing-sheet".into(),
            "--scale".into(),
            "1".into(),
            "--drill-shape-opt".into(),
            "2".into(),
            "--mode-single".into(),
            "--layers".into(),
            layers.into(),
            "--output".into(),
            output.display().to_string(),
        ];
        if side == Side::Back {
            args.push("--mirror".into());
        }
        args.push(board.display().to_string());
        self.cli.run_to_file(&args)
    }

    fn erc(&self, schematic: &Path) -> Result<Vec<Violation>> {
        let path = schematic_file(schematic);
        let dir = std::env::temp_dir().join(format!("pcbcraft-erc-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let output = dir.join("erc.json");
        self.cli.run_to_file(&[
            "sch".into(),
            "erc".into(),
            "--format".into(),
            "json".into(),
            "--severity-all".into(),
            "--units".into(),
            "mm".into(),
            "--output".into(),
            output.display().to_string(),
            path.display().to_string(),
        ])?;
        let text = std::fs::read_to_string(&output)?;
        parse_report(&text, "erc")
    }

    fn drc(&self, board: &Path) -> Result<Vec<Violation>> {
        let path = board_file(board);
        let dir = std::env::temp_dir().join(format!("pcbcraft-drc-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let output = dir.join("drc.json");
        self.cli.run_to_file(&[
            "pcb".into(),
            "drc".into(),
            "--format".into(),
            "json".into(),
            "--severity-all".into(),
            "--units".into(),
            "mm".into(),
            "--refill-zones".into(),
            "--output".into(),
            output.display().to_string(),
            path.display().to_string(),
        ])?;
        let text = std::fs::read_to_string(&output)?;
        parse_report(&text, "drc")
    }

    fn design(&self, schematic: &Path) -> Result<Design> {
        let path = schematic_file(schematic);
        let dir = std::env::temp_dir().join(format!("pcbcraft-net-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let output = dir.join("net.xml");
        self.cli.run_to_file(&[
            "sch".into(),
            "export".into(),
            "netlist".into(),
            "--format".into(),
            "kicadxml".into(),
            "--output".into(),
            output.display().to_string(),
            path.display().to_string(),
        ])?;
        let text = std::fs::read_to_string(&output)?;
        Ok(parse_design(&text)?.0)
    }
}

fn schematic_file(path: &Path) -> PathBuf {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("kicad_pro") => path.with_extension("kicad_sch"),
        _ => path.to_path_buf(),
    }
}

fn board_file(path: &Path) -> PathBuf {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("kicad_pro") => path.with_extension("kicad_pcb"),
        _ => path.to_path_buf(),
    }
}
