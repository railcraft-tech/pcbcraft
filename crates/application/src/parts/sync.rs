use std::collections::HashSet;

use anyhow::Result;
use pcbcraft_domain::net::NetGraph;
use pcbcraft_domain::parts::{PartRecord, Supplier, SupplierCatalog};

use crate::ports::{ComponentStore, Usage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncStats {
    pub fetched: usize,
    pub kept: usize,
    pub used: usize,
}

pub fn usages_from_graph(graph: &NetGraph) -> Vec<Usage> {
    let mut rows = Vec::new();
    for component in &graph.components {
        if let Some(id) = component.lcsc.as_deref().filter(|id| !id.is_empty()) {
            rows.push(Usage {
                reference: component.reference.clone(),
                supplier: Supplier::Lcsc,
                part_id: id.to_string(),
            });
        }
        if let Some(id) = component.pcbway.as_deref().filter(|id| !id.is_empty()) {
            rows.push(Usage {
                reference: component.reference.clone(),
                supplier: Supplier::Pcbway,
                part_id: id.to_string(),
            });
        }
    }
    rows
}

pub fn sync_components<L, P>(
    usage: &[Usage],
    store: &dyn ComponentStore,
    lcsc: &L,
    pcbway: &P,
    refresh: bool,
) -> Result<SyncStats>
where
    L: SupplierCatalog<Query = pcbcraft_domain::parts::LcscQuery>,
    P: SupplierCatalog<Query = pcbcraft_domain::parts::PcbwayQuery>,
{
    let existing: HashSet<(Supplier, String)> = store.part_ids()?.into_iter().collect();
    let mut fetched = Vec::new();
    let mut fetched_count = 0;
    let mut kept = 0;
    for supplier in [Supplier::Lcsc, Supplier::Pcbway] {
        let ids: Vec<String> = usage
            .iter()
            .filter(|row| row.supplier == supplier)
            .map(|row| row.part_id.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let (need, skip): (Vec<_>, Vec<_>) = ids
            .into_iter()
            .partition(|id| refresh || !existing.contains(&(supplier, id.clone())));
        kept += skip.len();
        if need.is_empty() {
            continue;
        }
        let records = match supplier {
            Supplier::Lcsc => lcsc.fetch(&need)?,
            Supplier::Pcbway => pcbway.fetch(&need)?,
        };
        fetched_count += records.len();
        fetched.extend(records);
    }
    store.replace(usage, &fetched, true)?;
    Ok(SyncStats {
        fetched: fetched_count,
        kept,
        used: usage.len(),
    })
}

#[allow(dead_code)]
fn touch(record: &PartRecord) -> &str {
    record.part_id.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pcbcraft_domain::parts::{ElecParam, PartRecord, Supplier};
    use std::cell::RefCell;

    struct FakeStore {
        ids: RefCell<Vec<(Supplier, String)>>,
        saved: RefCell<Vec<PartRecord>>,
    }

    impl ComponentStore for FakeStore {
        fn part_ids(&self) -> Result<Vec<(Supplier, String)>> {
            Ok(self.ids.borrow().clone())
        }
        fn card(&self, supplier: Supplier, part_id: &str) -> Result<Option<PartRecord>> {
            Ok(self
                .saved
                .borrow()
                .iter()
                .find(|record| record.supplier == supplier && record.part_id == part_id)
                .cloned())
        }
        fn find(&self, part_id: &str) -> Result<Vec<PartRecord>> {
            Ok(self
                .saved
                .borrow()
                .iter()
                .filter(|record| record.part_id == part_id)
                .cloned()
                .collect())
        }
        fn replace(&self, _: &[Usage], fetched: &[PartRecord], _: bool) -> Result<()> {
            self.saved.borrow_mut().extend(fetched.iter().cloned());
            for record in fetched {
                self.ids
                    .borrow_mut()
                    .push((record.supplier, record.part_id.clone()));
            }
            Ok(())
        }
    }

    struct Lcsc {
        calls: RefCell<usize>,
    }

    impl SupplierCatalog for Lcsc {
        type Query = pcbcraft_domain::parts::LcscQuery;
        fn supplier(&self) -> Supplier {
            Supplier::Lcsc
        }
        fn fetch(&self, ids: &[String]) -> Result<Vec<PartRecord>> {
            *self.calls.borrow_mut() += 1;
            Ok(ids
                .iter()
                .map(|id| PartRecord {
                    supplier: Supplier::Lcsc,
                    part_id: id.clone(),
                    mpn: "R".into(),
                    manufacturer: "Yageo".into(),
                    package: "0805".into(),
                    category: "Resistors".into(),
                    description: String::new(),
                    stock: Some(10),
                    price: Some(0.01),
                    library: Some("basic".into()),
                    parameters: vec![ElecParam {
                        name: "Resistance".into(),
                        value: "10 kΩ".into(),
                        si: Some(10_000.0),
                    }],
                })
                .collect())
        }
        fn search(&self, _: &Self::Query) -> Result<Vec<PartRecord>> {
            Ok(Vec::new())
        }
    }

    struct Pcbway;

    impl SupplierCatalog for Pcbway {
        type Query = pcbcraft_domain::parts::PcbwayQuery;
        fn supplier(&self) -> Supplier {
            Supplier::Pcbway
        }
        fn fetch(&self, _: &[String]) -> Result<Vec<PartRecord>> {
            Ok(Vec::new())
        }
        fn search(&self, _: &Self::Query) -> Result<Vec<PartRecord>> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn second_sync_does_not_fetch_a_known_number() {
        let store = FakeStore {
            ids: RefCell::new(Vec::new()),
            saved: RefCell::new(Vec::new()),
        };
        let lcsc = Lcsc {
            calls: RefCell::new(0),
        };
        let usage = vec![Usage {
            reference: "R1".into(),
            supplier: Supplier::Lcsc,
            part_id: "C1".into(),
        }];
        sync_components(&usage, &store, &lcsc, &Pcbway, false).unwrap();
        sync_components(&usage, &store, &lcsc, &Pcbway, false).unwrap();
        assert_eq!(*lcsc.calls.borrow(), 1);
        assert_eq!(store.saved.borrow()[0].parameters[0].name, "Resistance");
    }
}
