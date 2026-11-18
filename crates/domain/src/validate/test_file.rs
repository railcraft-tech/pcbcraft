use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One JSON file under the test directory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestFile {
    #[serde(rename = "type")]
    pub type_name: String,
    pub spec: Value,
}
