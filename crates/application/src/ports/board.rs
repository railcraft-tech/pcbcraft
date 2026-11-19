use std::path::Path;

use anyhow::Result;
use pcbcraft_domain::geometry::BoardGeometry;

pub trait BoardSource {
    fn load(&self, project: &Path) -> Result<BoardGeometry>;
}
