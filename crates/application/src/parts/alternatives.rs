use anyhow::Result;
use pcbcraft_domain::parts::{
    lcsc_matches, pcbway_matches, Item, PartRecord, Query, SupplierCatalog,
};

#[derive(Debug, Clone)]
pub struct Found {
    pub name: String,
    pub hits: Vec<PartRecord>,
}

pub fn search_all<L, P>(items: &[Item], lcsc: &L, pcbway: &P, limit: usize) -> Result<Vec<Found>>
where
    L: SupplierCatalog<Query = pcbcraft_domain::parts::LcscQuery>,
    P: SupplierCatalog<Query = pcbcraft_domain::parts::PcbwayQuery>,
{
    let mut found = Vec::new();
    for item in items {
        let hits = match &item.query {
            Query::Lcsc(query) => {
                let mut hits = lcsc.search(query)?;
                hits.retain(|record| lcsc_matches(record, query));
                hits.truncate(limit);
                hits
            }
            Query::Pcbway(query) => {
                let mut hits = pcbway.search(query)?;
                hits.retain(|record| pcbway_matches(record, query));
                hits.truncate(limit);
                hits
            }
        };
        found.push(Found {
            name: item.name.clone(),
            hits,
        });
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pcbcraft_domain::parts::{ElecParam, LcscQuery, PartRecord, PcbwayQuery, Supplier};
    use std::cell::RefCell;

    struct Lcsc {
        hits: Vec<PartRecord>,
    }

    impl SupplierCatalog for Lcsc {
        type Query = LcscQuery;
        fn supplier(&self) -> Supplier {
            Supplier::Lcsc
        }
        fn fetch(&self, _: &[String]) -> Result<Vec<PartRecord>> {
            Ok(Vec::new())
        }
        fn search(&self, _: &LcscQuery) -> Result<Vec<PartRecord>> {
            Ok(self.hits.clone())
        }
    }

    struct Pcbway {
        calls: RefCell<usize>,
    }

    impl SupplierCatalog for Pcbway {
        type Query = PcbwayQuery;
        fn supplier(&self) -> Supplier {
            Supplier::Pcbway
        }
        fn fetch(&self, _: &[String]) -> Result<Vec<PartRecord>> {
            Ok(Vec::new())
        }
        fn search(&self, _: &PcbwayQuery) -> Result<Vec<PartRecord>> {
            *self.calls.borrow_mut() += 1;
            Ok(Vec::new())
        }
    }

    #[test]
    fn the_local_filter_drops_a_wrong_package_and_limits_hits() {
        let good = PartRecord {
            supplier: Supplier::Lcsc,
            part_id: "C1".into(),
            mpn: "A".into(),
            manufacturer: String::new(),
            package: "0805".into(),
            category: String::new(),
            description: String::new(),
            stock: Some(1),
            price: None,
            library: None,
            parameters: vec![ElecParam {
                name: "Resistance".into(),
                value: "10k".into(),
                si: Some(10_000.0),
            }],
        };
        let mut bad = good.clone();
        bad.part_id = "C2".into();
        bad.package = "0603".into();
        let spec = pcbcraft_domain::parts::parse_spec(
            r#"
items:
  - name: R1
    supplier: lcsc
    parameters:
      category: resistor
      package: "0805"
      resistance: { min: "9k", max: "11k" }
"#,
        )
        .unwrap();
        let found = search_all(
            &spec.items,
            &Lcsc {
                hits: vec![good, bad],
            },
            &Pcbway {
                calls: RefCell::new(0),
            },
            10,
        )
        .unwrap();
        assert_eq!(found[0].hits.len(), 1);
        assert_eq!(found[0].hits[0].part_id, "C1");
    }
}
