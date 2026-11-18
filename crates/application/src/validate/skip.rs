//! Names that `pcbcraft.yaml` uses to skip a printed test line.

use std::collections::HashSet;

#[derive(Debug, Default)]
pub struct Skips {
    entries: HashSet<String>,
    matched: HashSet<String>,
}

impl Skips {
    pub fn new(entries: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            entries: entries.into_iter().map(Into::into).collect(),
            matched: HashSet::new(),
        }
    }

    pub fn would_match(&self, group: &str, name: &str, subtest: bool) -> bool {
        !self.hits(group, name, subtest).is_empty()
    }

    pub fn mark(&mut self, group: &str, name: &str, subtest: bool) -> bool {
        let hits = self.hits(group, name, subtest);
        let matched = !hits.is_empty();
        for hit in hits {
            self.matched.insert(hit);
        }
        matched
    }

    pub fn unmatched(&self) -> Vec<String> {
        let mut left: Vec<_> = self.entries.difference(&self.matched).cloned().collect();
        left.sort();
        left
    }

    fn hits(&self, group: &str, name: &str, subtest: bool) -> Vec<String> {
        ids(group, name, subtest)
            .into_iter()
            .filter(|id| self.entries.contains(id))
            .collect()
    }
}

/// Ids a line answers to. A subtest also answers to the name before the measured suffix.
pub fn ids(group: &str, name: &str, subtest: bool) -> Vec<String> {
    if subtest {
        let head = name.split_whitespace().next().unwrap_or(name);
        let mut ids = vec![format!("{group}.{head}")];
        if head != name {
            ids.push(format!("{group}.{name}"));
        }
        ids
    } else {
        vec![format!("{group}.{name}"), format!("{group}: {name}")]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_part_id_matches_the_line_that_carries_a_measurement() {
        let ids = ids("VoltageRating", "C21 3.3V", true);
        assert!(ids.contains(&"VoltageRating.C21".to_string()));
        let skips = Skips::new(["VoltageRating.C21"]);
        assert!(skips.would_match("VoltageRating", "C21 3.3V", true));
        assert!(!skips.would_match("VoltageRating", "C2 3.3V", true));
    }

    #[test]
    fn an_unmatched_entry_is_reported() {
        let mut skips = Skips::new(["VoltageRating.C21", "Divider.R1/R2"]);
        assert!(skips.mark("VoltageRating", "C21 16V", true));
        assert_eq!(skips.unmatched(), vec!["Divider.R1/R2".to_string()]);
    }
}
