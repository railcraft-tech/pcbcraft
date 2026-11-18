use std::path::Path;

use pcbcraft_domain::parts::{PartRecord, Supplier};

/// Catalog card for one part number, read from the project component database.
pub trait PartCards {
    fn card(
        &self,
        project: &Path,
        supplier: Supplier,
        part_id: &str,
    ) -> anyhow::Result<Option<PartRecord>>;
}
