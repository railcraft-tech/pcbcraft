use std::path::Path;

use pcbcraft_domain::parts::Supplier;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsedPart {
    pub supplier: Supplier,
    pub part_id: String,
}

/// Part numbers taken from the schematic.
pub trait UsedParts {
    fn list(&self, project: &Path) -> anyhow::Result<Vec<UsedPart>>;
}

/// Live stock for one catalog id. `Ok(None)` means the catalog returned no quantity.
pub trait PartStock {
    fn lookup(&self, supplier: Supplier, part_id: &str) -> anyhow::Result<Option<u64>>;
}
