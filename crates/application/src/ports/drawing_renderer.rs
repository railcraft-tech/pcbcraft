use std::path::Path;

use pcbcraft_domain::drawing::{BoardGeometry, Page};

pub trait DrawingRenderer {
    fn render_mechanical(
        &self,
        page: &Page,
        geometry: &BoardGeometry,
        output: &Path,
    ) -> anyhow::Result<()>;

    fn render_color(
        &self,
        page: &Page,
        svgs: &[(pcbcraft_domain::drawing::Side, String)],
        output: &Path,
    ) -> anyhow::Result<()>;
}
