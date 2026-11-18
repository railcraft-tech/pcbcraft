use super::supplier::Supplier;

#[derive(Debug, Clone, PartialEq)]
pub struct ElecParam {
    pub name: String,
    pub value: String,
    pub si: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PartRecord {
    pub supplier: Supplier,
    pub part_id: String,
    pub mpn: String,
    pub manufacturer: String,
    pub package: String,
    pub category: String,
    pub description: String,
    pub stock: Option<u64>,
    pub price: Option<f64>,
    pub library: Option<String>,
    pub parameters: Vec<ElecParam>,
}
