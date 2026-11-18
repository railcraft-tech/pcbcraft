use pcbcraft_domain::parts::{PartRecord, Supplier};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Usage {
    pub reference: String,
    pub supplier: Supplier,
    pub part_id: String,
}

pub trait ComponentStore {
    fn part_ids(&self) -> anyhow::Result<Vec<(Supplier, String)>>;
    fn card(&self, supplier: Supplier, part_id: &str) -> anyhow::Result<Option<PartRecord>>;
    /// Every card with this catalog number. Suppliers are not part of the number.
    fn find(&self, part_id: &str) -> anyhow::Result<Vec<PartRecord>>;
    fn replace(
        &self,
        usage: &[Usage],
        fetched: &[PartRecord],
        drop_missing: bool,
    ) -> anyhow::Result<()>;
}
