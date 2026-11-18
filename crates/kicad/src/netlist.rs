use anyhow::Result;
use pcbcraft_domain::net::{Pin, Pin as NetPin};
use pcbcraft_domain::validate::{Design, Net, PinRef, Symbol};
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::net::PinGroup;

pub fn parse_design(text: &str) -> Result<(Design, Vec<PinGroup>)> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut symbols = Vec::new();
    let mut groups = Vec::new();
    let mut buf = Vec::new();
    let mut current_ref = String::new();
    let mut value = String::new();
    let mut footprint = String::new();
    let mut library = String::new();
    let mut part = String::new();
    let mut properties = std::collections::BTreeMap::new();
    let mut in_comp = false;
    let mut text_for = String::new();
    let mut net_name = String::new();
    let mut net_pins = Vec::new();
    let mut in_net = false;
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(event) => {
                let tag = String::from_utf8_lossy(event.name().as_ref()).into_owned();
                match tag.as_str() {
                    "comp" => {
                        in_comp = true;
                        current_ref = attr(&event, b"ref").unwrap_or_default();
                        value.clear();
                        footprint.clear();
                        library.clear();
                        part.clear();
                        properties.clear();
                    }
                    "value" | "footprint" if in_comp => text_for = tag,
                    "libsource" if in_comp => {
                        library = attr(&event, b"lib").unwrap_or_default();
                        part = attr(&event, b"part").unwrap_or_default();
                    }
                    "property" if in_comp => {
                        if let (Some(name), Some(value)) =
                            (attr(&event, b"name"), attr(&event, b"value"))
                        {
                            properties.insert(name, value);
                        }
                    }
                    "net" => {
                        in_net = true;
                        net_name = attr(&event, b"name").unwrap_or_default();
                        net_pins.clear();
                    }
                    "node" if in_net => {
                        let reference = attr(&event, b"ref").unwrap_or_default();
                        let pin = attr(&event, b"pin").unwrap_or_default();
                        let name = attr(&event, b"pinfunction").unwrap_or_default();
                        net_pins.push(Pin {
                            reference: reference.clone(),
                            number: pin.clone(),
                            name,
                        });
                    }
                    _ => {}
                }
            }
            Event::Empty(event) => {
                let tag = String::from_utf8_lossy(event.name().as_ref()).into_owned();
                if tag == "libsource" && in_comp {
                    library = attr(&event, b"lib").unwrap_or_default();
                    part = attr(&event, b"part").unwrap_or_default();
                }
                if tag == "property" && in_comp {
                    if let (Some(name), Some(value)) =
                        (attr(&event, b"name"), attr(&event, b"value"))
                    {
                        properties.insert(name, value);
                    }
                }
                if tag == "node" && in_net {
                    net_pins.push(NetPin {
                        reference: attr(&event, b"ref").unwrap_or_default(),
                        number: attr(&event, b"pin").unwrap_or_default(),
                        name: attr(&event, b"pinfunction").unwrap_or_default(),
                    });
                }
            }
            Event::Text(text) => {
                let text = text.unescape()?.into_owned();
                match text_for.as_str() {
                    "value" => value = text,
                    "footprint" => footprint = text,
                    _ => {}
                }
                text_for.clear();
            }
            Event::End(event) => {
                let tag = String::from_utf8_lossy(event.name().as_ref()).into_owned();
                if tag == "comp" {
                    let lib = if library.is_empty() {
                        part.clone()
                    } else if part.is_empty() {
                        library.clone()
                    } else {
                        format!("{library}:{part}")
                    };
                    symbols.push(Symbol {
                        reference: current_ref.clone(),
                        value: value.clone(),
                        footprint: footprint.clone(),
                        library: lib,
                        properties: properties.clone(),
                    });
                    in_comp = false;
                }
                if tag == "net" {
                    groups.push(PinGroup {
                        name: net_name.clone(),
                        pins: net_pins.clone(),
                    });
                    in_net = false;
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    let nets = groups
        .iter()
        .map(|group| Net {
            name: group.name.clone(),
            pins: group
                .pins
                .iter()
                .map(|pin| PinRef {
                    reference: pin.reference.clone(),
                    pin: pin.number.clone(),
                })
                .collect(),
        })
        .collect();
    Ok((Design { symbols, nets }, groups))
}

fn attr(event: &quick_xml::events::BytesStart<'_>, key: &[u8]) -> Option<String> {
    event
        .attributes()
        .filter_map(|attr| attr.ok())
        .find_map(|attr| {
            if attr.key.as_ref() == key {
                Some(String::from_utf8_lossy(&attr.value).into_owned())
            } else {
                None
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_symbol_and_a_net() {
        let xml = r#"
<export>
  <components>
    <comp ref="R1">
      <value>10k</value>
      <footprint>Resistor_SMD:R_0805</footprint>
      <libsource lib="Device" part="R"/>
      <property name="LCSC Part #" value="C1"/>
    </comp>
  </components>
  <nets>
    <net code="1" name="GND">
      <node ref="R1" pin="2" pinfunction="~"/>
      <node ref="C1" pin="2"/>
    </net>
  </nets>
</export>
"#;
        let (design, groups) = parse_design(xml).unwrap();
        assert_eq!(design.symbols[0].library, "Device:R");
        assert_eq!(design.symbols[0].properties["LCSC Part #"], "C1");
        assert_eq!(groups[0].pins.len(), 2);
    }
}
