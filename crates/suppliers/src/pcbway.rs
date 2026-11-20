use anyhow::{bail, Result};
use pcbcraft_domain::parts::{PartRecord, PcbwayQuery, Supplier, SupplierCatalog};

pub struct PcbwayCatalog;

impl SupplierCatalog for PcbwayCatalog {
    type Query = PcbwayQuery;

    fn supplier(&self) -> Supplier {
        Supplier::Pcbway
    }

    fn fetch(&self, part_ids: &[String]) -> Result<Vec<PartRecord>> {
        part_ids.iter().map(|id| self.lookup(id)).collect()
    }

    fn search(&self, query: &PcbwayQuery) -> Result<Vec<PartRecord>> {
        let mut hits = Vec::new();
        for mpn in &query.mpn {
            match self.lookup(mpn) {
                Ok(record) => hits.push(record),
                Err(err) => return Err(err),
            }
        }
        Ok(hits)
    }
}

impl PcbwayCatalog {
    fn lookup(&self, mpn: &str) -> Result<PartRecord> {
        // Partner API quotes boards and assembly. It does not publish a part catalog.
        let _ = mpn;
        bail!("PCBWay catalog cannot be queried: the public partner API has no part search ({mpn})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pcbcraft_domain::parts::SupplierCatalog;

    #[test]
    fn fetch_reports_that_the_catalog_is_not_queryable() {
        let err = PcbwayCatalog.fetch(&["STM32".into()]).unwrap_err();
        assert!(err.to_string().contains("STM32"));
    }
}
