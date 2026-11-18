//! SVG page size in millimetres, as KiCad writes `width="12.5mm"`.

pub fn size_mm(svg: &str) -> Option<(f64, f64)> {
    let width = attr(svg, "width")?;
    let height = attr(svg, "height")?;
    Some((unit_mm(&width)?, unit_mm(&height)?))
}

fn attr(svg: &str, name: &str) -> Option<String> {
    let key = format!("{name}=\"");
    let start = svg.find(&key)? + key.len();
    let end = svg[start..].find('"')? + start;
    Some(svg[start..end].to_string())
}

fn unit_mm(text: &str) -> Option<f64> {
    let text = text.trim();
    if let Some(number) = text.strip_suffix("mm") {
        return number.trim().parse().ok();
    }
    text.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_millimetre_page_size() {
        let svg = r#"<svg width="12.5mm" height="8mm" viewBox="0 0 12.5 8"></svg>"#;
        assert_eq!(size_mm(svg), Some((12.5, 8.0)));
        assert_eq!(size_mm("<svg></svg>"), None);
    }
}
