#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    pub reference: String,
    pub symbol: String,
    pub value: String,
    pub footprint: String,
    pub description: String,
    pub lcsc: Option<String>,
    pub pcbway: Option<String>,
    pub dnp: bool,
    /// KiCad `(in_bom yes|no)`. Missing flag means yes.
    pub in_bom: bool,
    /// KiCad `(in_pos_files yes|no)`. Missing flag means yes.
    pub in_pos_files: bool,
}
