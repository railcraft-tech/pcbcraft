//! PCBWay assembly identity. Electrical ranges are not part of this query.

use super::offer::PartRecord;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcbwayQuery {
    pub mpn: Vec<String>,
    pub manufacturer: Vec<String>,
    pub package: Vec<String>,
    pub description: Vec<String>,
}

pub fn matches(record: &PartRecord, query: &PcbwayQuery) -> bool {
    list_hit(&query.mpn, &record.mpn, &record.part_id)
        && (query.manufacturer.is_empty()
            || list_hit(&query.manufacturer, &record.manufacturer, ""))
        && (query.package.is_empty() || list_hit(&query.package, &record.package, ""))
        && (query.description.is_empty()
            || query.description.iter().any(|item| {
                contains_fold(&record.description, item) || contains_fold(&record.mpn, item)
            }))
}

fn list_hit(accepted: &[String], primary: &str, extra: &str) -> bool {
    accepted.iter().any(|item| {
        fold(primary) == fold(item) || fold(extra) == fold(item) || contains_fold(primary, item)
    })
}

fn contains_fold(haystack: &str, needle: &str) -> bool {
    fold(haystack).contains(&fold(needle))
}

fn fold(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parts::{PartRecord, Supplier};

    fn record() -> PartRecord {
        PartRecord {
            supplier: Supplier::Pcbway,
            part_id: "STM32F103C8T6".into(),
            mpn: "STM32F103C8T6".into(),
            manufacturer: "STMicroelectronics".into(),
            package: "LQFP-48".into(),
            category: String::new(),
            description: "ARM Cortex-M3".into(),
            stock: None,
            price: None,
            library: None,
            parameters: Vec::new(),
        }
    }

    #[test]
    fn manufacturer_and_description_narrow_the_hit() {
        let query = PcbwayQuery {
            mpn: vec!["STM32F103C8T6".into()],
            manufacturer: vec!["ST".into()],
            package: vec!["LQFP-48".into()],
            description: vec!["Cortex".into()],
        };
        assert!(matches(&record(), &query));
        let mut wrong = record();
        wrong.manufacturer = "NXP".into();
        assert!(!matches(&wrong, &query));
    }
}
