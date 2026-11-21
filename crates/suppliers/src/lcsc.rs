use anyhow::{bail, Context, Result};
use pcbcraft_domain::parts::parse_quantity;
use pcbcraft_domain::parts::{
    lcsc_keyword, ElecParam, LcscQuery, PartRecord, ProductAttributes, Supplier, SupplierCatalog,
};

const SEARCH: &str =
    "https://jlcpcb.com/api/overseas-pcb-order/v1/shoppingCart/smtGood/selectSmtComponentList/v2";

pub struct LcscCatalog {
    pub in_stock: bool,
    pub basic_only: bool,
}

impl Default for LcscCatalog {
    fn default() -> Self {
        Self {
            in_stock: true,
            basic_only: false,
        }
    }
}

impl SupplierCatalog for LcscCatalog {
    type Query = LcscQuery;

    fn supplier(&self) -> Supplier {
        Supplier::Lcsc
    }

    fn fetch(&self, part_ids: &[String]) -> Result<Vec<PartRecord>> {
        let mut records = Vec::new();
        for id in part_ids {
            let found = search_raw(id, true, false, 5)?;
            if let Some(record) = found
                .into_iter()
                .find(|record| record.part_id.eq_ignore_ascii_case(id))
            {
                records.push(record);
            } else {
                bail!("LCSC has no part {id}");
            }
        }
        Ok(records)
    }

    fn search(&self, query: &LcscQuery) -> Result<Vec<PartRecord>> {
        let mut body_records =
            search_raw(&lcsc_keyword(query), self.in_stock, self.basic_only, 50)?;
        if !query.package.is_empty() {
            body_records.retain(|record| {
                query.package.iter().any(|package| {
                    record
                        .package
                        .to_ascii_lowercase()
                        .contains(&package.to_ascii_lowercase())
                })
            });
        }
        Ok(body_records)
    }
}

impl LcscCatalog {
    /// Keyword plus the caller's product attributes. Package words in the
    /// keyword are not turned into a package filter; that filter is `package`.
    pub fn search_attributes(
        &self,
        keyword: &str,
        attributes: &ProductAttributes,
        limit: u32,
    ) -> Result<Vec<PartRecord>> {
        let body = request_body(
            keyword,
            self.in_stock,
            self.basic_only,
            limit,
            Some(attributes),
        );
        post_search(&body)
    }
}

fn search_raw(
    keyword: &str,
    in_stock: bool,
    basic_only: bool,
    limit: u32,
) -> Result<Vec<PartRecord>> {
    post_search(&request_body(keyword, in_stock, basic_only, limit, None))
}

fn request_body(
    keyword: &str,
    in_stock: bool,
    basic_only: bool,
    limit: u32,
    attributes: Option<&ProductAttributes>,
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "currentPage": 1,
        "pageSize": limit.min(50),
        "keyword": keyword,
        "searchType": 2
    });
    if in_stock {
        body["presaleType"] = serde_json::json!("stock");
    }
    if basic_only {
        body["componentLibTypes"] = serde_json::json!(["base"]);
        body["preferredComponentFlag"] = serde_json::json!(true);
    }
    if let Some(attributes) = attributes {
        if !attributes.package.is_empty() {
            body["componentSpecificationList"] = serde_json::json!(attributes.package);
        }
        if !attributes.manufacturer.is_empty() {
            body["componentBrandList"] = serde_json::json!(attributes.manufacturer);
        }
        if !attributes.attributes.is_empty() {
            let list = attributes
                .attributes
                .iter()
                .map(|(name, values)| {
                    let mut entry = serde_json::Map::new();
                    entry.insert(name.clone(), serde_json::json!(values));
                    serde_json::Value::Object(entry)
                })
                .collect::<Vec<_>>();
            body["componentAttributeList"] = serde_json::Value::Array(list);
        }
    } else if keyword.split_whitespace().any(looks_like_package) {
        let packages: Vec<&str> = keyword
            .split_whitespace()
            .filter(|part| looks_like_package(part))
            .collect();
        body["componentSpecificationList"] = serde_json::json!(packages);
    }
    body
}

fn post_search(body: &serde_json::Value) -> Result<Vec<PartRecord>> {
    let payload = super::http::post_json(SEARCH, body).context("LCSC search")?;
    if payload.get("code").and_then(|code| code.as_i64()) != Some(200) {
        let message = payload
            .get("message")
            .and_then(|value| value.as_str())
            .unwrap_or("API error");
        bail!("LCSC: {message}");
    }
    let list = payload
        .pointer("/data/componentPageInfo/list")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(list.iter().filter_map(record_from).collect())
}

fn looks_like_package(text: &str) -> bool {
    let digits = text.chars().filter(|c| c.is_ascii_digit()).count();
    digits >= 4 && text.chars().all(|c| c.is_ascii_digit())
}

fn record_from(value: &serde_json::Value) -> Option<PartRecord> {
    let part_id = value.get("componentCode")?.as_str()?.to_string();
    let mut parameters = Vec::new();
    if let Some(attrs) = value.get("attributes").and_then(|value| value.as_array()) {
        for attr in attrs {
            let name = attr.get("attribute_name_en")?.as_str()?.to_string();
            let text = attr.get("attribute_value_name")?.as_str()?.to_string();
            if text == "-" || text.is_empty() {
                continue;
            }
            let si = parse_quantity(&text).ok().map(|quantity| quantity.si);
            parameters.push(ElecParam {
                name,
                value: text,
                si,
            });
        }
    }
    Some(PartRecord {
        supplier: Supplier::Lcsc,
        part_id,
        mpn: value
            .get("componentModelEn")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string(),
        manufacturer: value
            .get("componentBrandEn")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string(),
        package: value
            .get("componentSpecificationEn")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string(),
        category: value
            .get("componentTypeEn")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string(),
        description: value
            .get("describe")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string(),
        stock: value.get("stockCount").and_then(|value| value.as_u64()),
        price: value
            .pointer("/componentPrices/0/productPrice")
            .and_then(|value| value.as_f64()),
        library: value
            .get("componentLibraryType")
            .and_then(|value| value.as_str())
            .map(|text| {
                if text == "base" {
                    "basic".into()
                } else {
                    "extended".into()
                }
            }),
        parameters,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pcbcraft_domain::parts::parse_product_attributes_text;

    #[test]
    fn a_keyword_package_stays_on_the_alternatives_request() {
        let body = request_body("resistor 0805", true, false, 10, None);
        assert_eq!(body["keyword"], "resistor 0805");
        assert_eq!(body["pageSize"], 10);
        assert_eq!(body["presaleType"], "stock");
        assert_eq!(
            body["componentSpecificationList"],
            serde_json::json!(["0805"])
        );
        assert!(body.get("componentAttributeList").is_none());
    }

    #[test]
    fn product_attributes_are_the_catalog_filters() {
        let attributes = parse_product_attributes_text(
            r#"{"package":["0805","0603"],"manufacturer":"Yageo","Resistance":"10kΩ","Tolerance":["±1%"]}"#,
        )
        .unwrap();
        let body = request_body("resistor 0805", true, true, 80, Some(&attributes));
        assert_eq!(body["keyword"], "resistor 0805");
        assert_eq!(body["pageSize"], 50);
        assert_eq!(body["componentLibTypes"], serde_json::json!(["base"]));
        assert_eq!(body["preferredComponentFlag"], true);
        assert_eq!(
            body["componentSpecificationList"],
            serde_json::json!(["0805", "0603"])
        );
        assert_eq!(body["componentBrandList"], serde_json::json!(["Yageo"]));
        assert_eq!(
            body["componentAttributeList"],
            serde_json::json!([
                {"Resistance": ["10kΩ"]},
                {"Tolerance": ["±1%"]}
            ])
        );
    }

    #[test]
    fn an_explicit_attribute_set_does_not_guess_a_package_from_the_keyword() {
        let body = request_body(
            "resistor 0805",
            false,
            false,
            10,
            Some(&ProductAttributes::default()),
        );
        assert!(body.get("componentSpecificationList").is_none());
        assert!(body.get("presaleType").is_none());
    }
}
