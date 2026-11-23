use std::time::Duration;

use anyhow::{bail, Context, Result};
use pcbcraft_application::parts::{FileBody, FileKind, ProductCard, RemoteFile};
use pcbcraft_domain::parts::parse_quantity;
use pcbcraft_domain::parts::{
    lcsc_keyword, ElecParam, LcscQuery, PartRecord, ProductAttributes, Supplier, SupplierCatalog,
};

const SEARCH: &str =
    "https://jlcpcb.com/api/overseas-pcb-order/v1/shoppingCart/smtGood/selectSmtComponentList/v2";
const DETAIL: &str = "https://wmsc.lcsc.com/ftps/wm/product/detail";

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

/// Product photos, RoHS files, and EasyEDA drawings for one LCSC number.
/// The datasheet URL is `ProductCard::datasheet_url`. `pdfLinkUrl` is ignored:
/// on the live API it is either null or the same address as `pdfUrl`.
#[derive(Debug, Default, Clone, Copy)]
pub struct LcscDatasheets;

impl LcscDatasheets {
    pub fn fetch_card(&self, lcsc: &str) -> Result<ProductCard> {
        let url = format!("{DETAIL}?productCode={lcsc}");
        let payload = super::http::get_json(&url).context("LCSC product detail")?;
        product_card(lcsc, &payload)
    }
}

impl pcbcraft_application::parts::DatasheetCatalog for LcscDatasheets {
    fn card(&self, lcsc: &str) -> Result<ProductCard> {
        self.fetch_card(lcsc)
    }
}

impl pcbcraft_application::parts::FileFetch for LcscDatasheets {
    fn get(&self, url: &str) -> Result<FileBody> {
        let body = super::http::get_bytes(url, Duration::from_secs(120))?;
        Ok(FileBody {
            bytes: body.bytes,
            content_type: body.content_type,
        })
    }
}

pub fn product_card(requested: &str, value: &serde_json::Value) -> Result<ProductCard> {
    if value.get("code").and_then(json_code) != Some(200) {
        let message = value
            .get("msg")
            .and_then(|item| item.as_str())
            .or_else(|| value.get("message").and_then(|item| item.as_str()))
            .filter(|text| !text.is_empty())
            .unwrap_or("API error");
        bail!("LCSC: {message}");
    }
    let Some(result) = value.get("result").filter(|item| item.is_object()) else {
        bail!("LCSC has no part {requested}");
    };
    let lcsc = text_at(result, "productCode");
    if lcsc.is_empty() {
        bail!("LCSC has no part {requested}");
    }
    let datasheet_url = absolute_url(&text_at(result, "pdfUrl"));
    if datasheet_url.is_empty() {
        bail!("LCSC {lcsc} has no datasheet");
    }
    let mut extras = Vec::new();
    if let Some(images) = result.get("productImages").and_then(|item| item.as_array()) {
        for image in images {
            let Some(raw) = image.as_str() else {
                continue;
            };
            let url = absolute_url(raw);
            if url.is_empty() {
                continue;
            }
            let Some(name) = image_name(&url) else {
                continue;
            };
            push_named(
                &mut extras,
                RemoteFile {
                    name,
                    url,
                    kind: FileKind::Jpeg,
                },
            );
        }
    }
    if let Some(certs) = result.get("rohsCertList").and_then(|item| item.as_array()) {
        for cert in certs {
            let url = absolute_url(&text_at(cert, "url"));
            if url.is_empty() {
                continue;
            }
            let code = sanitize_token(&text_at(cert, "fileCode"));
            let code = if code.is_empty() {
                "cert".to_string()
            } else {
                code
            };
            let (ext, kind) = match url_extension(&url).as_deref() {
                Some("zip") => ("zip", FileKind::Zip),
                _ => ("pdf", FileKind::Pdf),
            };
            push_named(
                &mut extras,
                RemoteFile {
                    name: format!("rohs-{code}.{ext}"),
                    url,
                    kind,
                },
            );
        }
    }
    if let Some(eda) = result.get("edaSvgInfo").filter(|item| item.is_object()) {
        push_svg(&mut extras, "symbol.svg", &text_at(eda, "schSvg"));
        push_svg(&mut extras, "footprint.svg", &text_at(eda, "pcbSvg"));
    }
    Ok(ProductCard {
        lcsc,
        mpn: text_at(result, "productModel"),
        manufacturer: text_at(result, "brandNameEn"),
        datasheet_url,
        extras,
    })
}

fn push_svg(extras: &mut Vec<RemoteFile>, name: &str, url: &str) {
    let url = absolute_url(url);
    if url.is_empty() {
        return;
    }
    push_named(
        extras,
        RemoteFile {
            name: name.to_string(),
            url,
            kind: FileKind::Svg,
        },
    );
}

fn push_named(extras: &mut Vec<RemoteFile>, mut file: RemoteFile) {
    file.name = unique_name(extras, &file.name);
    extras.push(file);
}

fn unique_name(files: &[RemoteFile], name: &str) -> String {
    if files.iter().all(|have| have.name != name) {
        return name.to_string();
    }
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    let mut index = 2;
    loop {
        let candidate = if ext.is_empty() {
            format!("{stem}-{index}")
        } else {
            format!("{stem}-{index}.{ext}")
        };
        if files.iter().all(|have| have.name != candidate) {
            return candidate;
        }
        index += 1;
    }
}

fn image_name(url: &str) -> Option<String> {
    let file = url_file_name(url)?.to_ascii_lowercase();
    if file.contains("_blank") {
        return None;
    }
    if file.contains("_front") {
        return Some("front.jpg".into());
    }
    if file.contains("_back") {
        return Some("back.jpg".into());
    }
    let name = sanitize_token(&url_file_name(url)?);
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn url_file_name(url: &str) -> Option<String> {
    let path = url.split('?').next().unwrap_or(url);
    let name = path.rsplit('/').next().unwrap_or("").trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn url_extension(url: &str) -> Option<String> {
    let name = url_file_name(url)?;
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    if ext.len() <= 5 && ext.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        Some(ext)
    } else {
        None
    }
}

fn absolute_url(url: &str) -> String {
    let url = url.trim();
    if let Some(rest) = url.strip_prefix("//") {
        format!("https://{rest}")
    } else if url.starts_with("https://") || url.starts_with("http://") {
        url.to_string()
    } else {
        String::new()
    }
}

fn sanitize_token(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

fn text_at(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn json_code(value: &serde_json::Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
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

    #[test]
    fn a_product_detail_keeps_the_datasheet_and_the_other_files() {
        let card = product_card("C55266", &sample_detail()).unwrap();
        assert_eq!(card.lcsc, "C55266");
        assert_eq!(card.mpn, "TPS2553DBVR");
        assert_eq!(card.manufacturer, "TI");
        assert_eq!(
            card.datasheet_url,
            "https://datasheet.lcsc.com/datasheet/pdf/abc.pdf?productCode=C55266"
        );
        let names: Vec<_> = card.extras.iter().map(|file| file.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "front.jpg",
                "back.jpg",
                "rohs-wj1.pdf",
                "rohs-wj2.zip",
                "symbol.svg",
                "footprint.svg",
            ]
        );
        assert!(card
            .extras
            .iter()
            .any(|file| file.url == "https://image.easyeda.com/component_svgs/prod/sch.svg"));
        assert!(card
            .extras
            .iter()
            .any(|file| file.kind == FileKind::Zip && file.name == "rohs-wj2.zip"));
        assert!(card.extras.iter().all(|file| !file.name.contains("blank")));
    }

    #[test]
    fn a_missing_datasheet_and_a_missing_part_are_errors() {
        let mut detail = sample_detail();
        detail["result"]["pdfUrl"] = serde_json::json!("");
        let err = product_card("C1", &detail).unwrap_err();
        assert!(err.to_string().contains("no datasheet"), "{err}");

        let err =
            product_card("C1", &serde_json::json!({"code": 200, "result": null})).unwrap_err();
        assert!(err.to_string().contains("no part"), "{err}");

        let err =
            product_card("C1", &serde_json::json!({"code": 500, "msg": "closed"})).unwrap_err();
        assert!(err.to_string().contains("closed"), "{err}");
    }

    fn sample_detail() -> serde_json::Value {
        serde_json::json!({
            "code": 200,
            "result": {
                "productCode": "C55266",
                "productModel": "TPS2553DBVR",
                "brandNameEn": "TI",
                "pdfUrl": "https://datasheet.lcsc.com/datasheet/pdf/abc.pdf?productCode=C55266",
                "pdfLinkUrl": "https://datasheet.lcsc.com/datasheet/pdf/abc.pdf?productCode=C55266",
                "productImages": [
                    "https://assets.lcsc.com/images/lcsc/900x900/x_C55266_front.jpg",
                    "https://assets.lcsc.com/images/lcsc/900x900/x_C55266_back.jpg",
                    "https://assets.lcsc.com/images/lcsc/900x900/x_C55266_blank.jpg"
                ],
                "rohsCertList": [
                    {"fileCode": "wj1", "fileName": "cert.pdf", "url": "https://datasheet.lcsc.com/certification/rohs/a.pdf"},
                    {"fileCode": "wj2", "fileName": "cert.zip", "url": "https://datasheet.lcsc.com/certification/rohs/b.zip"}
                ],
                "edaSvgInfo": {
                    "schSvg": "//image.easyeda.com/component_svgs/prod/sch.svg",
                    "pcbSvg": "//image.easyeda.com/component_svgs/prod/pcb.svg?t=1"
                }
            }
        })
    }
}
