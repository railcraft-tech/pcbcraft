use anyhow::{bail, Context, Result};
use pcbcraft_domain::parts::parse_quantity;
use pcbcraft_domain::parts::{
    lcsc_keyword, ElecParam, LcscQuery, PartRecord, Supplier, SupplierCatalog,
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

fn search_raw(
    keyword: &str,
    in_stock: bool,
    basic_only: bool,
    limit: u32,
) -> Result<Vec<PartRecord>> {
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
    if keyword
        .split_whitespace()
        .any(|part| looks_like_package(part))
    {
        let packages: Vec<&str> = keyword
            .split_whitespace()
            .filter(|part| looks_like_package(part))
            .collect();
        body["componentSpecificationList"] = serde_json::json!(packages);
    }
    let payload = super::http::post_json(SEARCH, &body).context("LCSC search")?;
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
