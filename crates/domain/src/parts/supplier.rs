use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Supplier {
    Lcsc,
    Pcbway,
}

impl Supplier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lcsc => "lcsc",
            Self::Pcbway => "pcbway",
        }
    }
}

impl fmt::Display for Supplier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
