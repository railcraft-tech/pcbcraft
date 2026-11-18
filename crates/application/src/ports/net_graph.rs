use std::path::Path;

use pcbcraft_domain::net::NetGraph;

pub trait NetGraphSource {
    fn load(&self, project: &Path) -> anyhow::Result<NetGraph>;
}
