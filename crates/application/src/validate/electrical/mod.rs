//! One JSON type per electrical check. `pcbcraft tests init defaults` does not write these files; `pcbcraft tests init type` does.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result};
use pcbcraft_domain::electrical::{
    band, divider_span, format_si, parallel_resistance, passive_kind, series_capacitance,
    series_resistance, tau_span, value_within, worst_delta, worst_power, Kind, Span, Tolerance,
};
use pcbcraft_domain::net::{Component, NetGraph};
use pcbcraft_domain::parts::{
    attribute_quantity, parse_quantity, schematic_ohms, tolerance_window, Dimension, PartRecord,
    Supplier,
};
use serde::Deserialize;
use serde_json::{json, Value};

use super::validator::Line;
use crate::ports::{NetGraphSource, PartCards};

mod catalog_value;
mod divider;
mod parallel_passives;
mod resistor_power;
mod series_passives;
mod time_constant;
mod voltage_rating;

pub use catalog_value::CatalogValue;
pub use divider::Divider;
pub use parallel_passives::ParallelPassives;
pub use resistor_power::ResistorPower;
pub use series_passives::SeriesPassives;
pub use time_constant::TimeConstant;
pub use voltage_rating::VoltageRating;

#[derive(Deserialize)]
struct ProjectSpec {
    project: String,
}

#[derive(Deserialize)]
struct Bound {
    min: String,
    max: String,
}

#[derive(Deserialize)]
struct PowerSpec {
    project: String,
    rails: BTreeMap<String, Bound>,
    #[serde(default = "two", rename = "powerHeadroom")]
    power_headroom: f64,
}

#[derive(Deserialize)]
struct VoltageSpec {
    project: String,
    rails: BTreeMap<String, Bound>,
    #[serde(default = "two", rename = "voltageHeadroom")]
    voltage_headroom: f64,
}

#[derive(Deserialize)]
struct DividerSpec {
    project: String,
    rails: BTreeMap<String, Bound>,
    #[serde(default)]
    expect: BTreeMap<String, Bound>,
}

#[derive(Deserialize)]
struct ExpectSpec {
    project: String,
    #[serde(default)]
    expect: BTreeMap<String, Bound>,
}

fn two() -> f64 {
    2.0
}

struct Facts {
    reference: String,
    kind: Kind,
    nets: Vec<String>,
    schematic: Option<f64>,
    nominal: Option<f64>,
    tolerance: Option<Tolerance>,
    power: Option<f64>,
    voltage: Option<f64>,
    issue: Option<String>,
}

struct Board {
    parts: Vec<Facts>,
    pin_counts: BTreeMap<String, usize>,
}

fn catalog(
    file_name: &str,
    spec: &Value,
    graph: &dyn NetGraphSource,
    cards: &dyn PartCards,
) -> Result<Vec<Line>> {
    let spec: ProjectSpec = serde_json::from_value(spec.clone()).context("spec CatalogValue")?;
    let board = load_board(&spec.project, graph, cards)?;
    let parts: Vec<_> = board
        .parts
        .iter()
        .filter(|part| {
            part.kind == Kind::Resistor
                || part.kind == Kind::Capacitor
                || part.kind == Kind::Inductor
        })
        .collect();
    if parts.is_empty() {
        return Ok(vec![plain(
            file_name,
            false,
            "no resistors, capacitors, or inductors on the schematic",
        )]);
    }
    let mut lines = Vec::new();
    for part in parts {
        if let Some(issue) = &part.issue {
            lines.push(sub(&part.reference, false, issue));
            continue;
        }
        let Some(schematic) = part.schematic else {
            lines.push(sub(
                &part.reference,
                false,
                &format!("{} value cannot be read", part.reference),
            ));
            continue;
        };
        let Some(nominal) = part.nominal else {
            lines.push(sub(
                &part.reference,
                false,
                &format!(
                    "{} has no catalog {}",
                    part.reference,
                    part.kind.value_key()
                ),
            ));
            continue;
        };
        let Some(tolerance) = part.tolerance else {
            lines.push(sub(
                &part.reference,
                false,
                &format!("{} has no tolerance", part.reference),
            ));
            continue;
        };
        let dimension = part.kind.dimension();
        if value_within(schematic, nominal, tolerance) {
            lines.push(sub(&part.reference, true, ""));
        } else {
            let (low, high) = band(nominal, tolerance);
            lines.push(sub(
                &part.reference,
                false,
                &format!(
                    "schematic {} is outside {}-{}",
                    format_si(schematic, dimension),
                    format_si(low, dimension),
                    format_si(high, dimension)
                ),
            ));
        }
    }
    Ok(lines)
}

fn power(
    file_name: &str,
    spec: &Value,
    graph: &dyn NetGraphSource,
    cards: &dyn PartCards,
) -> Result<Vec<Line>> {
    let spec: PowerSpec = serde_json::from_value(spec.clone()).context("spec ResistorPower")?;
    let rails = match parse_rails(&spec.rails) {
        Ok(rails) => rails,
        Err(message) => return Ok(vec![plain(file_name, false, &message)]),
    };
    let board = load_board(&spec.project, graph, cards)?;
    let mut lines = Vec::new();
    for part in board
        .parts
        .iter()
        .filter(|part| part.kind == Kind::Resistor)
    {
        let Some((left, right)) = rail_pair(&part.nets, &rails) else {
            continue;
        };
        if let Some(issue) = ready(part) {
            lines.push(sub(&part.reference, false, &issue));
            continue;
        }
        let Some(rating) = part.power else {
            lines.push(sub(
                &part.reference,
                false,
                &format!("{} has no power rating", part.reference),
            ));
            continue;
        };
        let used = worst_power(left, right, part.nominal.unwrap(), part.tolerance.unwrap());
        let allowed = rating / spec.power_headroom;
        let name = format!("{} {}", part.reference, format_si(used, Dimension::Watt));
        if used <= allowed + 1e-12 {
            lines.push(sub(&name, true, ""));
        } else {
            lines.push(sub(
                &name,
                false,
                &format!(
                    "{} at headroom {} exceeds the {} rating",
                    format_si(used, Dimension::Watt),
                    trim_number(spec.power_headroom),
                    format_si(rating, Dimension::Watt)
                ),
            ));
        }
    }
    if lines.is_empty() {
        return Ok(vec![plain(
            file_name,
            false,
            "no resistor between declared rails",
        )]);
    }
    Ok(lines)
}

fn voltage(
    file_name: &str,
    spec: &Value,
    graph: &dyn NetGraphSource,
    cards: &dyn PartCards,
) -> Result<Vec<Line>> {
    let spec: VoltageSpec = serde_json::from_value(spec.clone()).context("spec VoltageRating")?;
    let rails = match parse_rails(&spec.rails) {
        Ok(rails) => rails,
        Err(message) => return Ok(vec![plain(file_name, false, &message)]),
    };
    let board = load_board(&spec.project, graph, cards)?;
    let mut lines = Vec::new();
    for part in &board.parts {
        let Some((left, right)) = rail_pair(&part.nets, &rails) else {
            continue;
        };
        let rated = match part.kind {
            Kind::Capacitor => true,
            Kind::Resistor => part.voltage.is_some(),
            Kind::Inductor => part.voltage.is_some(),
        };
        if !rated {
            continue;
        }
        if let Some(issue) = &part.issue {
            lines.push(sub(&part.reference, false, issue));
            continue;
        }
        let Some(rating) = part.voltage else {
            lines.push(sub(
                &part.reference,
                false,
                &format!("{} has no voltage rating", part.reference),
            ));
            continue;
        };
        let applied = worst_delta(left, right);
        let name = format!("{} {}", part.reference, format_si(applied, Dimension::Volt));
        let allowed = rating / spec.voltage_headroom;
        if applied <= allowed + 1e-12 {
            lines.push(sub(&name, true, ""));
        } else {
            lines.push(sub(
                &name,
                false,
                &format!(
                    "{} at headroom {} exceeds the {} rating",
                    format_si(applied, Dimension::Volt),
                    trim_number(spec.voltage_headroom),
                    format_si(rating, Dimension::Volt)
                ),
            ));
        }
    }
    if lines.is_empty() {
        return Ok(vec![plain(
            file_name,
            false,
            "no rated part between declared rails",
        )]);
    }
    Ok(lines)
}

fn divider(
    file_name: &str,
    spec: &Value,
    graph: &dyn NetGraphSource,
    cards: &dyn PartCards,
) -> Result<Vec<Line>> {
    let spec: DividerSpec = serde_json::from_value(spec.clone()).context("spec Divider")?;
    let rails = match parse_rails(&spec.rails) {
        Ok(rails) => rails,
        Err(message) => return Ok(vec![plain(file_name, false, &message)]),
    };
    let expect = match parse_expect(&spec.expect, Dimension::Volt) {
        Ok(expect) => expect,
        Err(message) => return Ok(vec![plain(file_name, false, &message)]),
    };
    let board = load_board(&spec.project, graph, cards)?;
    let resistors: Vec<&Facts> = board
        .parts
        .iter()
        .filter(|part| part.kind == Kind::Resistor)
        .collect();
    let mut lines = Vec::new();
    let mut seen = BTreeSet::new();
    let outputs = divider_outputs(&resistors, &rails);
    for (output, pair) in outputs {
        seen.insert(output.clone());
        let (left, right) = pair;
        if let Some(issue) = ready(left).or_else(|| ready(right)) {
            lines.push(sub(
                &join(&[&left.reference, &right.reference]),
                false,
                &issue,
            ));
            continue;
        }
        let span = divider_span(
            rails[&left_rail(left, &rails)],
            rails[&left_rail(right, &rails)],
            left.nominal.unwrap(),
            left.tolerance.unwrap(),
            right.nominal.unwrap(),
            right.tolerance.unwrap(),
        );
        // divider_span assigns r_a to rail a. left's rail is a, right's rail is b.
        let name = format!(
            "{} {}",
            join(&[&left.reference, &right.reference]),
            format_span(span, Dimension::Volt)
        );
        lines.push(expect_line(&name, &output, span, &expect));
    }
    lines.extend(missing_expect(&expect, &seen));
    if lines.is_empty() {
        return Ok(vec![plain(
            file_name,
            false,
            "no resistor divider on declared rails",
        )]);
    }
    Ok(lines)
}

fn time_constant(
    file_name: &str,
    spec: &Value,
    graph: &dyn NetGraphSource,
    cards: &dyn PartCards,
) -> Result<Vec<Line>> {
    let spec: ExpectSpec = serde_json::from_value(spec.clone()).context("spec TimeConstant")?;
    let expect = match parse_time_expect(&spec.expect) {
        Ok(expect) => expect,
        Err(message) => return Ok(vec![plain(file_name, false, &message)]),
    };
    let board = load_board(&spec.project, graph, cards)?;
    let mut groups: BTreeMap<(String, String), Vec<&Facts>> = BTreeMap::new();
    for part in &board.parts {
        if part.nets.len() != 2 {
            continue;
        }
        let mut pair = part.nets.clone();
        pair.sort();
        groups
            .entry((pair[0].clone(), pair[1].clone()))
            .or_default()
            .push(part);
    }
    let mut lines = Vec::new();
    let mut seen = BTreeSet::new();
    for members in groups.values() {
        let resistors: Vec<_> = members
            .iter()
            .copied()
            .filter(|part| part.kind == Kind::Resistor)
            .collect();
        let capacitors: Vec<_> = members
            .iter()
            .copied()
            .filter(|part| part.kind == Kind::Capacitor)
            .collect();
        if resistors.len() != 1 || capacitors.len() != 1 || members.len() != 2 {
            continue;
        }
        let resistor = resistors[0];
        let capacitor = capacitors[0];
        let key = join(&[&resistor.reference, &capacitor.reference]);
        seen.insert(key.clone());
        if let Some(issue) = ready(resistor).or_else(|| ready(capacitor)) {
            lines.push(sub(&key, false, &issue));
            continue;
        }
        let span = tau_span(
            resistor.nominal.unwrap(),
            resistor.tolerance.unwrap(),
            capacitor.nominal.unwrap(),
            capacitor.tolerance.unwrap(),
        );
        let name = format!("{key} {}", format_time_span(span));
        lines.push(expect_line(&name, &key, span, &expect));
    }
    lines.extend(missing_expect(&expect, &seen));
    if lines.is_empty() {
        return Ok(vec![plain(
            file_name,
            false,
            "no parallel resistor and capacitor",
        )]);
    }
    Ok(lines)
}

fn parallel(
    file_name: &str,
    spec: &Value,
    graph: &dyn NetGraphSource,
    cards: &dyn PartCards,
) -> Result<Vec<Line>> {
    let spec: ExpectSpec = serde_json::from_value(spec.clone()).context("spec ParallelPassives")?;
    let expect = match parse_expect(&spec.expect, Dimension::Ohm) {
        Ok(expect) => expect,
        Err(message) => return Ok(vec![plain(file_name, false, &message)]),
    };
    let board = load_board(&spec.project, graph, cards)?;
    let mut groups: BTreeMap<(String, String), Vec<&Facts>> = BTreeMap::new();
    for part in board
        .parts
        .iter()
        .filter(|part| part.kind == Kind::Resistor && part.nets.len() == 2)
    {
        let mut pair = part.nets.clone();
        pair.sort();
        groups
            .entry((pair.remove(0), pair.remove(0)))
            .or_default()
            .push(part);
    }
    let mut lines = Vec::new();
    let mut seen = BTreeSet::new();
    for members in groups.values() {
        if members.len() < 2 {
            continue;
        }
        let refs: Vec<&str> = members.iter().map(|part| part.reference.as_str()).collect();
        let key = join(&refs);
        seen.insert(key.clone());
        if let Some(issue) = members.iter().find_map(|part| ready(part)) {
            lines.push(sub(&key, false, &issue));
            continue;
        }
        let values: Vec<(f64, Tolerance)> = members
            .iter()
            .map(|part| (part.nominal.unwrap(), part.tolerance.unwrap()))
            .collect();
        let span = parallel_resistance(&values);
        let name = format!("{key} {}", format_span(span, Dimension::Ohm));
        lines.push(expect_line(&name, &key, span, &expect));
    }
    lines.extend(missing_expect(&expect, &seen));
    if lines.is_empty() {
        return Ok(vec![plain(file_name, false, "no parallel resistors")]);
    }
    Ok(lines)
}

fn series(
    file_name: &str,
    spec: &Value,
    graph: &dyn NetGraphSource,
    cards: &dyn PartCards,
) -> Result<Vec<Line>> {
    let spec: ExpectSpec = serde_json::from_value(spec.clone()).context("spec SeriesPassives")?;
    let board = load_board(&spec.project, graph, cards)?;
    let mut lines = Vec::new();
    let mut seen = BTreeSet::new();
    for part in &board.parts {
        if part.nets.len() != 2 || !matches!(part.kind, Kind::Resistor | Kind::Capacitor) {
            continue;
        }
        for net in &part.nets {
            if board.pin_counts.get(net).copied() != Some(2) {
                continue;
            }
            let Some(other) = board.parts.iter().find(|candidate| {
                candidate.reference != part.reference
                    && candidate.kind == part.kind
                    && candidate.nets.len() == 2
                    && candidate.nets.iter().any(|name| name == net)
            }) else {
                continue;
            };
            if part.reference > other.reference {
                continue;
            }
            let outer_left = part.nets.iter().find(|name| *name != net);
            let outer_right = other.nets.iter().find(|name| *name != net);
            if outer_left == outer_right {
                continue;
            }
            let key = join(&[&part.reference, &other.reference]);
            if !seen.insert(key.clone()) {
                continue;
            }
            if let Some(issue) = ready(part).or_else(|| ready(other)) {
                lines.push(sub(&key, false, &issue));
                continue;
            }
            let values = [
                (part.nominal.unwrap(), part.tolerance.unwrap()),
                (other.nominal.unwrap(), other.tolerance.unwrap()),
            ];
            let (span, dimension) = match part.kind {
                Kind::Resistor => (series_resistance(&values), Dimension::Ohm),
                Kind::Capacitor => (series_capacitance(&values), Dimension::Farad),
                Kind::Inductor => continue,
            };
            let window = match window_for(&spec.expect, &key, dimension) {
                Ok(window) => window,
                Err(message) => return Ok(vec![plain(file_name, false, &message)]),
            };
            let name = format!("{key} {}", format_span(span, dimension));
            lines.push(match window {
                Some(window) => {
                    expect_line(&name, &key, span, &BTreeMap::from([(key.clone(), window)]))
                }
                None => sub(&name, true, ""),
            });
        }
    }
    for key in spec.expect.keys() {
        if !seen.contains(key) {
            lines.push(sub(key, false, &format!("no network named {key}")));
        }
    }
    if lines.is_empty() {
        return Ok(vec![plain(file_name, false, "no series passives")]);
    }
    Ok(lines)
}

fn schematic_value(kind: Kind, value: &str) -> Option<f64> {
    if kind == Kind::Resistor {
        if let Some(ohms) = schematic_ohms(value) {
            return Some(ohms);
        }
    }
    parse_quantity(value)
        .ok()
        .filter(|quantity| quantity.dimension == kind.dimension())
        .map(|quantity| quantity.si)
}

fn load_board(project: &str, graph: &dyn NetGraphSource, cards: &dyn PartCards) -> Result<Board> {
    let loaded = graph.load(Path::new(project))?;
    let mut parts = Vec::new();
    for component in &loaded.components {
        if component.dnp {
            continue;
        }
        let Some(kind) = passive_kind(&component.reference) else {
            continue;
        };
        parts.push(facts_for(
            component,
            kind,
            &loaded,
            cards,
            Path::new(project),
        )?);
    }
    parts.sort_by(|left, right| natural(&left.reference).cmp(&natural(&right.reference)));
    Ok(Board {
        parts,
        pin_counts: pin_counts(&loaded),
    })
}

fn facts_for(
    component: &Component,
    kind: Kind,
    graph: &NetGraph,
    cards: &dyn PartCards,
    project: &Path,
) -> Result<Facts> {
    let mut nets: Vec<String> = nets_of(graph, &component.reference).into_iter().collect();
    nets.sort();
    let mut fact = Facts {
        reference: component.reference.clone(),
        kind,
        nets,
        schematic: schematic_value(kind, &component.value),
        nominal: None,
        tolerance: None,
        power: None,
        voltage: None,
        issue: None,
    };
    let Some(lcsc) = component.lcsc.as_deref().filter(|value| !value.is_empty()) else {
        fact.issue = Some(format!("{} has no LCSC part number", component.reference));
        return Ok(fact);
    };
    let Some(card) = cards.card(project, Supplier::Lcsc, lcsc)? else {
        fact.issue = Some(format!(
            "{} ({lcsc}) is not in the component database; run pcbcraft components sync",
            component.reference
        ));
        return Ok(fact);
    };
    fill_card(&mut fact, &card);
    Ok(fact)
}

fn fill_card(fact: &mut Facts, card: &PartRecord) {
    if let Some(quantity) = attribute_quantity(card, fact.kind.value_key()) {
        fact.nominal = Some(quantity.si);
    }
    if let Some(window) = tolerance_window(card, fact.kind.dimension()) {
        fact.tolerance = Some(window);
    }
    if let Some(quantity) = attribute_quantity(card, "power") {
        fact.power = Some(quantity.si);
    }
    if let Some(quantity) = attribute_quantity(card, "voltage") {
        fact.voltage = Some(quantity.si);
    }
}

fn ready(part: &Facts) -> Option<String> {
    if let Some(issue) = &part.issue {
        return Some(issue.clone());
    }
    if part.nominal.is_none() {
        return Some(format!(
            "{} has no catalog {}",
            part.reference,
            part.kind.value_key()
        ));
    }
    if part.tolerance.is_none() {
        return Some(format!("{} has no tolerance", part.reference));
    }
    if matches!(part.tolerance, Some(Tolerance::Relative(fraction)) if fraction >= 1.0) {
        return Some(format!("{} tolerance must be below 100%", part.reference));
    }
    None
}

fn rail_pair<'a>(nets: &[String], rails: &'a BTreeMap<String, Span>) -> Option<(Span, Span)> {
    if nets.len() != 2 {
        return None;
    }
    Some((rails.get(&nets[0])?.clone(), rails.get(&nets[1])?.clone()))
}

fn left_rail(part: &Facts, rails: &BTreeMap<String, Span>) -> String {
    part.nets
        .iter()
        .find(|net| rails.contains_key(net.as_str()))
        .cloned()
        .unwrap_or_default()
}

fn divider_outputs<'a>(
    resistors: &[&'a Facts],
    rails: &BTreeMap<String, Span>,
) -> Vec<(String, (&'a Facts, &'a Facts))> {
    let mut found = Vec::new();
    let outputs: BTreeSet<String> = resistors
        .iter()
        .flat_map(|part| {
            part.nets
                .iter()
                .filter(|net| !rails.contains_key(net.as_str()))
                .cloned()
        })
        .collect();
    for output in outputs {
        let touching: Vec<&Facts> = resistors
            .iter()
            .copied()
            .filter(|part| part.nets.iter().any(|net| net == &output))
            .collect();
        if touching.len() != 2 {
            continue;
        }
        let rails_of = |part: &Facts| -> Option<String> {
            let on_rail: Vec<_> = part
                .nets
                .iter()
                .filter(|net| rails.contains_key(net.as_str()))
                .cloned()
                .collect();
            let on_out = part.nets.iter().any(|net| net == &output);
            if on_rail.len() == 1 && on_out && part.nets.len() == 2 {
                Some(on_rail[0].clone())
            } else {
                None
            }
        };
        let (Some(left_name), Some(right_name)) = (rails_of(touching[0]), rails_of(touching[1]))
        else {
            continue;
        };
        if left_name == right_name {
            continue;
        }
        found.push((output, (touching[0], touching[1])));
    }
    found
}

fn parse_rails(raw: &BTreeMap<String, Bound>) -> Result<BTreeMap<String, Span>, String> {
    if raw.is_empty() {
        return Err("no rails in spec".into());
    }
    let mut rails = BTreeMap::new();
    for (name, bound) in raw {
        rails.insert(
            name.clone(),
            parse_span(&bound.min, &bound.max, Dimension::Volt, name)?,
        );
    }
    Ok(rails)
}

fn parse_expect(
    raw: &BTreeMap<String, Bound>,
    dimension: Dimension,
) -> Result<BTreeMap<String, Span>, String> {
    let mut expect = BTreeMap::new();
    for (name, bound) in raw {
        expect.insert(
            name.clone(),
            parse_span(&bound.min, &bound.max, dimension, name)?,
        );
    }
    Ok(expect)
}

fn parse_time_expect(raw: &BTreeMap<String, Bound>) -> Result<BTreeMap<String, Span>, String> {
    let mut expect = BTreeMap::new();
    for (name, bound) in raw {
        let min = parse_seconds(&bound.min).map_err(|error| format!("expect {name}: {error}"))?;
        let max = parse_seconds(&bound.max).map_err(|error| format!("expect {name}: {error}"))?;
        expect.insert(name.clone(), Span::new(min, max));
    }
    Ok(expect)
}

fn parse_seconds(text: &str) -> Result<f64, String> {
    let lower: String = text
        .trim()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    let (body, scale) = if let Some(body) = lower.strip_suffix("ms") {
        (body, 1e-3)
    } else if let Some(body) = lower.strip_suffix("us") {
        (body, 1e-6)
    } else if let Some(body) = lower.strip_suffix("ns") {
        (body, 1e-9)
    } else if let Some(body) = lower.strip_suffix('s') {
        (body, 1.0)
    } else {
        return Err(format!("unrecognized time '{text}'"));
    };
    let value: f64 = body
        .parse()
        .map_err(|_| format!("unrecognized time '{text}'"))?;
    Ok(value * scale)
}

fn window_for(
    raw: &BTreeMap<String, Bound>,
    key: &str,
    dimension: Dimension,
) -> Result<Option<Span>, String> {
    let Some(bound) = raw.get(key) else {
        return Ok(None);
    };
    Ok(Some(parse_span(&bound.min, &bound.max, dimension, key)?))
}

fn parse_span(min: &str, max: &str, dimension: Dimension, name: &str) -> Result<Span, String> {
    let min_q = parse_quantity(min).map_err(|error| format!("{name}: {error}"))?;
    let max_q = parse_quantity(max).map_err(|error| format!("{name}: {error}"))?;
    if min_q.dimension != dimension || max_q.dimension != dimension {
        return Err(format!("{name} must be a {}", dimension_name(dimension)));
    }
    Ok(Span::new(min_q.si, max_q.si))
}

fn expect_line(name: &str, key: &str, span: Span, expect: &BTreeMap<String, Span>) -> Line {
    let Some(window) = expect.get(key) else {
        return sub(name, true, "");
    };
    if span.min + 1e-12 >= window.min && span.max - 1e-12 <= window.max {
        sub(name, true, "")
    } else {
        sub(
            name,
            false,
            &format!("{key} is outside the expected window"),
        )
    }
}

fn missing_expect(expect: &BTreeMap<String, Span>, seen: &BTreeSet<String>) -> Vec<Line> {
    expect
        .keys()
        .filter(|key| !seen.contains(key.as_str()))
        .map(|key| sub(key, false, &format!("no network named {key}")))
        .collect()
}

fn nets_of(graph: &NetGraph, reference: &str) -> BTreeSet<String> {
    let mut nets = BTreeSet::new();
    for net in &graph.nets {
        let hit = net.connections.iter().any(|connection| {
            connection.from.reference == reference || connection.to.reference == reference
        });
        if hit {
            nets.insert(net.name.clone());
        }
    }
    nets
}

fn pin_counts(graph: &NetGraph) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for net in &graph.nets {
        let mut pins = BTreeSet::new();
        for connection in &net.connections {
            pins.insert((
                connection.from.reference.clone(),
                connection.from.number.clone(),
            ));
            pins.insert((
                connection.to.reference.clone(),
                connection.to.number.clone(),
            ));
        }
        counts.insert(net.name.clone(), pins.len());
    }
    counts
}

fn join(references: &[&str]) -> String {
    let mut references: Vec<&str> = references.to_vec();
    references.sort_by_key(|reference| natural(reference));
    references.join("/")
}

fn natural(reference: &str) -> (char, u64, String) {
    let prefix = reference.chars().next().unwrap_or(' ');
    let rest: String = reference.chars().skip(1).collect();
    let number = rest.parse::<u64>().unwrap_or(0);
    (prefix, number, rest)
}

fn plain(name: &str, passed: bool, detail: &str) -> Line {
    Line {
        name: name.to_string(),
        subtest: false,
        status: if passed {
            crate::ports::TestStatus::Passed
        } else {
            crate::ports::TestStatus::Failed
        },
        details: if detail.is_empty() {
            Vec::new()
        } else {
            vec![detail.to_string()]
        },
        assertions: 1,
    }
}

fn sub(name: &str, passed: bool, detail: &str) -> Line {
    Line {
        name: name.to_string(),
        subtest: true,
        status: if passed {
            crate::ports::TestStatus::Passed
        } else {
            crate::ports::TestStatus::Failed
        },
        details: if detail.is_empty() {
            Vec::new()
        } else {
            vec![detail.to_string()]
        },
        assertions: 1,
    }
}

fn format_span(span: Span, dimension: Dimension) -> String {
    format!(
        "{}-{}",
        format_si(span.min, dimension),
        format_si(span.max, dimension)
    )
}

fn format_time_span(span: Span) -> String {
    format!("{}-{}", format_time(span.min), format_time(span.max))
}

fn format_time(value: f64) -> String {
    let (scale, unit) = if value.abs() >= 1.0 {
        (1.0, "s")
    } else if value.abs() >= 1e-3 {
        (1e-3, "ms")
    } else if value.abs() >= 1e-6 {
        (1e-6, "us")
    } else {
        (1e-9, "ns")
    };
    format!("{}{unit}", trim_number(value / scale))
}

fn trim_number(value: f64) -> String {
    let text = format!("{value:.4}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn dimension_name(dimension: Dimension) -> &'static str {
    match dimension {
        Dimension::Volt => "voltage",
        Dimension::Ohm => "resistance",
        Dimension::Farad => "capacitance",
        Dimension::Henry => "inductance",
        Dimension::Watt => "power",
        Dimension::Ampere => "current",
        Dimension::Percent => "percent",
    }
}

fn with_rails(mut extra: Value) -> Value {
    let body = json!({
        "project": { "type": "string", "minLength": 1 },
        "rails": rail_object()
    });
    if let (Some(body), Some(extra)) = (body.as_object(), extra.as_object_mut()) {
        let mut merged = body.clone();
        for (key, value) in extra {
            merged.insert(key.clone(), value.clone());
        }
        return json!(merged);
    }
    body
}

fn expect_schema() -> Value {
    json!({ "expect": expect_object() })
}

fn expect_object() -> Value {
    json!({
        "type": "object",
        "additionalProperties": bound_schema()
    })
}

fn rail_object() -> Value {
    json!({
        "type": "object",
        "additionalProperties": bound_schema()
    })
}

fn bound_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["min", "max"],
        "properties": {
            "min": { "type": "string", "minLength": 1 },
            "max": { "type": "string", "minLength": 1 }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::super::scheduler::TestContext;
    use super::super::validator::find;
    use super::*;
    use crate::ports::{Fabrication, PartStock, UsedParts};
    use pcbcraft_domain::drawing::Side;
    use pcbcraft_domain::net::{copper_net, Component, Pin};
    use pcbcraft_domain::parts::{ElecParam, PartRecord, Supplier};
    use pcbcraft_domain::validate::{Design, Violation};

    struct NoBoard;
    impl crate::ports::BoardSource for NoBoard {
        fn load(&self, _: &std::path::Path) -> Result<pcbcraft_domain::geometry::BoardGeometry> {
            Ok(pcbcraft_domain::geometry::BoardGeometry {
                copper_layers: Vec::new(),
                shapes: Vec::new(),
                outer_copper_mm: None,
                inner_copper_mm: None,
                footprints: Vec::new(),
            })
        }
    }
    const NO_BOARD: NoBoard = NoBoard;

    fn run(
        type_name: &str,
        file_name: &str,
        spec: &Value,
        graph: &FixedGraph,
        cards: &FixedCards,
    ) -> Result<Vec<Line>> {
        let validator = find(type_name).expect(type_name);
        let fab = NoFab;
        let parts = NoParts;
        let stock = NoStock;
        let ctx = TestContext {
            fabrication: &fab,
            used_parts: &parts,
            stock: &stock,
            graph,
            cards,
            boards: &NO_BOARD,
        };
        validator
            .run(file_name, spec, &ctx)?
            .into_iter()
            .map(|job| (job.run)(&ctx))
            .collect()
    }

    struct NoFab;
    impl Fabrication for NoFab {
        fn export_bom(&self, _: &std::path::Path, _: &std::path::Path) -> Result<()> {
            Ok(())
        }
        fn export_pos(&self, _: &std::path::Path, _: &std::path::Path) -> Result<()> {
            Ok(())
        }
        fn export_gerbers(&self, _: &std::path::Path, _: &std::path::Path) -> Result<()> {
            Ok(())
        }
        fn export_drill_split(&self, _: &std::path::Path, _: &std::path::Path) -> Result<()> {
            Ok(())
        }
        fn export_dxf_outline(&self, _: &std::path::Path, _: &std::path::Path) -> Result<()> {
            Ok(())
        }
        fn export_excellon(&self, _: &std::path::Path, _: &std::path::Path) -> Result<()> {
            Ok(())
        }
        fn export_svg(
            &self,
            _: &std::path::Path,
            _: Side,
            _: bool,
            _: &std::path::Path,
        ) -> Result<()> {
            Ok(())
        }
        fn erc(&self, _: &std::path::Path) -> Result<Vec<Violation>> {
            Ok(Vec::new())
        }
        fn drc(&self, _: &std::path::Path) -> Result<Vec<Violation>> {
            Ok(Vec::new())
        }
        fn design(&self, _: &std::path::Path) -> Result<Design> {
            Ok(Design::default())
        }
    }
    struct NoParts;
    impl UsedParts for NoParts {
        fn list(&self, _: &std::path::Path) -> Result<Vec<crate::ports::UsedPart>> {
            Ok(Vec::new())
        }
    }
    struct NoStock;
    impl PartStock for NoStock {
        fn lookup(&self, _: Supplier, _: &str) -> Result<Option<u64>> {
            Ok(None)
        }
    }

    struct FixedGraph(NetGraph);
    impl NetGraphSource for FixedGraph {
        fn load(&self, _: &Path) -> Result<NetGraph> {
            Ok(self.0.clone())
        }
    }

    struct FixedCards(Vec<PartRecord>);
    impl PartCards for FixedCards {
        fn card(&self, _: &Path, _: Supplier, part_id: &str) -> Result<Option<PartRecord>> {
            Ok(self
                .0
                .iter()
                .find(|record| record.part_id == part_id)
                .cloned())
        }
    }

    fn component(reference: &str, value: &str, lcsc: &str) -> Component {
        Component {
            reference: reference.into(),
            symbol: "Device:R".into(),
            value: value.into(),
            footprint: String::new(),
            description: String::new(),
            lcsc: Some(lcsc.into()),
            pcbway: None,
            dnp: false,
            in_bom: true,
            in_pos_files: true,
        }
    }

    fn pin(reference: &str, number: &str) -> Pin {
        Pin {
            reference: reference.into(),
            number: number.into(),
            name: String::new(),
        }
    }

    fn card(id: &str, resistance: &str, tolerance: &str, power: &str, voltage: &str) -> PartRecord {
        let mut parameters = vec![
            ElecParam {
                name: "Resistance".into(),
                value: resistance.into(),
                si: Some(parse_quantity(resistance).unwrap().si),
            },
            ElecParam {
                name: "Tolerance".into(),
                value: tolerance.into(),
                si: Some(parse_quantity(tolerance).unwrap().si),
            },
        ];
        if !power.is_empty() {
            parameters.push(ElecParam {
                name: "Power".into(),
                value: power.into(),
                si: Some(parse_quantity(power).unwrap().si),
            });
        }
        if !voltage.is_empty() {
            parameters.push(ElecParam {
                name: "Voltage Rating".into(),
                value: voltage.into(),
                si: Some(parse_quantity(voltage).unwrap().si),
            });
        }
        PartRecord {
            supplier: Supplier::Lcsc,
            part_id: id.into(),
            mpn: id.into(),
            manufacturer: "Yageo".into(),
            package: "0805".into(),
            category: "Resistors".into(),
            description: String::new(),
            stock: Some(100),
            price: None,
            library: None,
            parameters,
        }
    }

    fn between(
        reference: &str,
        left_net: &str,
        right_net: &str,
        left_pin: &str,
        right_pin: &str,
    ) -> NetGraph {
        let _ = (reference, left_pin, right_pin);
        NetGraph {
            components: vec![component(reference, "10k", "C10")],
            nets: vec![
                copper_net(left_net, vec![pin(reference, "1"), pin("U1", "1")], vec![]),
                copper_net(right_net, vec![pin(reference, "2"), pin("U1", "2")], vec![]),
            ],
        }
    }

    #[test]
    fn catalog_value_fails_when_the_schematic_disagrees() {
        let mut graph = between("R1", "+3V3", "GND", "1", "2");
        graph.components[0].value = "4.7k".into();
        let lines = run(
            "CatalogValue",
            "values",
            &json!({"project": "board.kicad_pro"}),
            &FixedGraph(graph),
            &FixedCards(vec![card("C10", "10k", "1%", "0.1W", "50V")]),
        )
        .unwrap();
        assert_eq!(lines[0].status, crate::ports::TestStatus::Failed);
        assert!(lines[0].details[0].contains("4.7kohm"));
    }

    #[test]
    fn an_absolute_tolerance_is_applied_in_the_part_unit() {
        let mut graph = between("C14", "RF", "GND", "1", "2");
        graph.components[0].value = "1.5pF".into();
        graph.components[0].symbol = "Device:C".into();
        let lines = run(
            "CatalogValue",
            "values",
            &json!({"project": "board.kicad_pro"}),
            &FixedGraph(graph),
            &FixedCards(vec![PartRecord {
                supplier: Supplier::Lcsc,
                part_id: "C10".into(),
                mpn: "C".into(),
                manufacturer: "Murata".into(),
                package: "0201".into(),
                category: "Capacitors".into(),
                description: String::new(),
                stock: Some(10),
                price: None,
                library: None,
                parameters: vec![
                    ElecParam {
                        name: "Capacitance".into(),
                        value: "1.5pF".into(),
                        si: Some(1.5e-12),
                    },
                    ElecParam {
                        name: "Tolerance".into(),
                        value: "±0.25pF".into(),
                        si: Some(0.25e-12),
                    },
                ],
            }]),
        )
        .unwrap();
        assert_eq!(
            lines[0].status,
            crate::ports::TestStatus::Passed,
            "{:?}",
            lines[0]
        );
    }

    #[test]
    fn a_dnp_symbol_is_not_a_catalog_line() {
        let mut graph = between("C16", "RF", "GND", "1", "2");
        graph.components[0].dnp = true;
        graph.components[0].lcsc = None;
        graph.components[0].value = "DNP".into();
        let lines = run(
            "CatalogValue",
            "values",
            &json!({"project": "board.kicad_pro"}),
            &FixedGraph(graph),
            &FixedCards(vec![]),
        )
        .unwrap();
        assert_eq!(lines.len(), 1);
        assert!(!lines[0].subtest);
        assert!(lines[0].details[0].contains("no resistors"));
    }

    #[test]
    fn a_bare_resistor_number_is_ohms() {
        let mut graph = between("R3", "A", "B", "1", "2");
        graph.components[0].value = "22".into();
        let lines = run(
            "CatalogValue",
            "values",
            &json!({"project": "board.kicad_pro"}),
            &FixedGraph(graph),
            &FixedCards(vec![card("C10", "22ohm", "1%", "0.1W", "")]),
        )
        .unwrap();
        assert_eq!(
            lines[0].status,
            crate::ports::TestStatus::Passed,
            "{:?}",
            lines[0]
        );
    }

    #[test]
    fn resistor_power_uses_the_high_end_of_the_rail() {
        let graph = between("R1", "+3V3", "GND", "1", "2");
        let lines = run(
            "ResistorPower",
            "power",
            &json!({
                "project": "board.kicad_pro",
                "rails": {
                    "+3V3": { "min": "3.0V", "max": "4.2V" },
                    "GND": { "min": "0V", "max": "0V" }
                },
                "powerHeadroom": 2
            }),
            &FixedGraph(graph),
            &FixedCards(vec![card("C10", "10k", "1%", "0.001W", "")]),
        )
        .unwrap();
        assert_eq!(lines[0].status, crate::ports::TestStatus::Failed);
        assert!(lines[0].name.starts_with("R1 "));
    }

    #[test]
    fn a_divider_reports_the_output_span() {
        let graph = NetGraph {
            components: vec![
                component("R1", "10k", "C1"),
                component("R2", "10k", "C2"),
                component("U1", "MCU", ""),
            ],
            nets: vec![
                copper_net("+3V3", vec![pin("R1", "1"), pin("U1", "1")], vec![]),
                copper_net(
                    "MID",
                    vec![pin("R1", "2"), pin("R2", "1"), pin("U1", "2")],
                    vec![],
                ),
                copper_net("GND", vec![pin("R2", "2"), pin("U1", "3")], vec![]),
            ],
        };
        let lines = run(
            "Divider",
            "div",
            &json!({
                "project": "board.kicad_pro",
                "rails": {
                    "+3V3": { "min": "3.3V", "max": "3.3V" },
                    "GND": { "min": "0V", "max": "0V" }
                }
            }),
            &FixedGraph(graph),
            &FixedCards(vec![
                card("C1", "10k", "1%", "0.1W", ""),
                card("C2", "10k", "1%", "0.1W", ""),
            ]),
        )
        .unwrap();
        assert_eq!(
            lines[0].status,
            crate::ports::TestStatus::Passed,
            "{:?}",
            lines[0]
        );
        assert!(lines[0].name.contains("1.6"));
        assert!(lines[0].subtest);
    }

    fn cap_card(id: &str, capacitance: &str, voltage: &str) -> PartRecord {
        PartRecord {
            supplier: Supplier::Lcsc,
            part_id: id.into(),
            mpn: id.into(),
            manufacturer: "Samsung".into(),
            package: "0402".into(),
            category: "Capacitors".into(),
            description: String::new(),
            stock: Some(100),
            price: None,
            library: None,
            parameters: vec![
                ElecParam {
                    name: "Capacitance".into(),
                    value: capacitance.into(),
                    si: Some(parse_quantity(capacitance).unwrap().si),
                },
                ElecParam {
                    name: "Tolerance".into(),
                    value: "10%".into(),
                    si: Some(10.0),
                },
                ElecParam {
                    name: "Voltage Rating".into(),
                    value: voltage.into(),
                    si: Some(parse_quantity(voltage).unwrap().si),
                },
            ],
        }
    }

    #[test]
    fn voltage_rating_fails_when_the_rail_exceeds_the_headroom() {
        let mut graph = between("C1", "+3V3", "GND", "1", "2");
        graph.components[0] = component("C1", "100nF", "C100");
        graph.components[0].symbol = "Device:C".into();
        let lines = run(
            "VoltageRating",
            "voltage",
            &json!({
                "project": "board.kicad_pro",
                "rails": {
                    "+3V3": { "min": "3.3V", "max": "3.3V" },
                    "GND": { "min": "0V", "max": "0V" }
                }
            }),
            &FixedGraph(graph),
            &FixedCards(vec![cap_card("C100", "100nF", "6.3V")]),
        )
        .unwrap();
        assert_eq!(
            lines[0].status,
            crate::ports::TestStatus::Failed,
            "{:?}",
            lines[0]
        );
        assert!(lines[0].details[0].contains("6.3V"));
    }

    #[test]
    fn parallel_resistors_report_the_equivalent() {
        let graph = NetGraph {
            components: vec![component("R1", "10k", "C1"), component("R2", "10k", "C2")],
            nets: vec![
                copper_net(
                    "A",
                    vec![pin("R1", "1"), pin("R2", "1"), pin("U1", "1")],
                    vec![],
                ),
                copper_net(
                    "B",
                    vec![pin("R1", "2"), pin("R2", "2"), pin("U1", "2")],
                    vec![],
                ),
            ],
        };
        let lines = run(
            "ParallelPassives",
            "parallel",
            &json!({"project": "board.kicad_pro"}),
            &FixedGraph(graph),
            &FixedCards(vec![
                card("C1", "10k", "0%", "", ""),
                card("C2", "10k", "0%", "", ""),
            ]),
        )
        .unwrap();
        assert_eq!(
            lines[0].status,
            crate::ports::TestStatus::Passed,
            "{:?}",
            lines[0]
        );
        assert!(lines[0].name.contains("5kohm"), "{}", lines[0].name);
    }

    #[test]
    fn series_resistors_add() {
        let graph = NetGraph {
            components: vec![component("R1", "10k", "C1"), component("R2", "10k", "C2")],
            nets: vec![
                copper_net("IN", vec![pin("R1", "1"), pin("U1", "1")], vec![]),
                copper_net("MID", vec![pin("R1", "2"), pin("R2", "1")], vec![]),
                copper_net("OUT", vec![pin("R2", "2"), pin("U1", "2")], vec![]),
            ],
        };
        let lines = run(
            "SeriesPassives",
            "series",
            &json!({"project": "board.kicad_pro"}),
            &FixedGraph(graph),
            &FixedCards(vec![
                card("C1", "10k", "0%", "", ""),
                card("C2", "10k", "0%", "", ""),
            ]),
        )
        .unwrap();
        assert_eq!(
            lines[0].status,
            crate::ports::TestStatus::Passed,
            "{:?}",
            lines[0]
        );
        assert!(lines[0].name.contains("20kohm"), "{}", lines[0].name);
    }

    #[test]
    fn a_parallel_rc_reports_tau() {
        let graph = NetGraph {
            components: vec![
                component("R1", "10k", "C1"),
                component("C1", "100nF", "C100"),
            ],
            nets: vec![
                copper_net(
                    "A",
                    vec![pin("R1", "1"), pin("C1", "1"), pin("U1", "1")],
                    vec![],
                ),
                copper_net(
                    "B",
                    vec![pin("R1", "2"), pin("C1", "2"), pin("U1", "2")],
                    vec![],
                ),
            ],
        };
        let lines = run(
            "TimeConstant",
            "tau",
            &json!({"project": "board.kicad_pro"}),
            &FixedGraph(graph),
            &FixedCards(vec![
                card("C1", "10k", "0%", "", ""),
                cap_card("C100", "100nF", "16V"),
            ]),
        )
        .unwrap();
        assert_eq!(
            lines[0].status,
            crate::ports::TestStatus::Passed,
            "{:?}",
            lines[0]
        );
        assert!(lines[0].name.contains("1ms"), "{}", lines[0].name);
    }

    #[test]
    fn each_schema_accepts_its_example_and_rejects_a_foreign_field() {
        use super::super::scheduler::{check_schemas, LoadedTest};
        let samples = [
            ("CatalogValue", json!({"project": "a.kicad_pro"})),
            (
                "ResistorPower",
                json!({"project": "a.kicad_pro", "rails": {"GND": {"min": "0V", "max": "0V"}}, "powerHeadroom": 2}),
            ),
            (
                "VoltageRating",
                json!({"project": "a.kicad_pro", "rails": {"GND": {"min": "0V", "max": "0V"}}}),
            ),
            (
                "Divider",
                json!({"project": "a.kicad_pro", "rails": {"GND": {"min": "0V", "max": "0V"}}}),
            ),
            ("TimeConstant", json!({"project": "a.kicad_pro"})),
            ("ParallelPassives", json!({"project": "a.kicad_pro"})),
            ("SeriesPassives", json!({"project": "a.kicad_pro"})),
        ];
        for (type_name, spec) in samples {
            let tests = vec![LoadedTest {
                name: "t".into(),
                type_name: type_name.into(),
                spec,
            }];
            check_schemas(&tests).unwrap_or_else(|err| panic!("{type_name}: {err:?}"));
        }
        let rejected = vec![LoadedTest {
            name: "t".into(),
            type_name: "CatalogValue".into(),
            spec: json!({"project": "a.kicad_pro", "rails": {}}),
        }];
        assert!(check_schemas(&rejected).is_err());
    }

    #[test]
    fn empty_rails_fail_the_power_test() {
        let lines = run(
            "ResistorPower",
            "power",
            &json!({"project": "board.kicad_pro", "rails": {}}),
            &FixedGraph(NetGraph::default()),
            &FixedCards(vec![]),
        )
        .unwrap();
        assert_eq!(lines[0].status, crate::ports::TestStatus::Failed);
        assert!(!lines[0].subtest);
        assert!(lines[0].details[0].contains("no rails"));
    }
}
