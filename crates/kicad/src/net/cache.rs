//! SQLite cache. A matching set of source hashes skips parsing.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use pcbcraft_domain::net::{Component, Connection, CopperNet, NetGraph, Pin};
use rusqlite::{params, Connection as Sql, OptionalExtension};

const PARSER_VERSION: &str = "2";

pub struct NetCache {
    sql: Sql,
}

impl NetCache {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
        let sql = Sql::open(path).with_context(|| format!("database {}", path.display()))?;
        sql.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS source_files (path TEXT PRIMARY KEY, hash TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS components (
                reference TEXT PRIMARY KEY,
                symbol TEXT NOT NULL,
                value TEXT NOT NULL,
                footprint TEXT NOT NULL,
                description TEXT NOT NULL,
                lcsc TEXT,
                pcbway TEXT,
                dnp INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS copper_nets (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                widths TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS connections (
                net_id INTEGER NOT NULL,
                from_ref TEXT NOT NULL,
                from_num TEXT NOT NULL,
                from_name TEXT NOT NULL,
                to_ref TEXT NOT NULL,
                to_num TEXT NOT NULL,
                to_name TEXT NOT NULL
            );
            ",
        )?;
        migrate(&sql)?;
        Ok(Self { sql })
    }

    pub fn load_if_fresh(&self, hashes: &BTreeMap<String, String>) -> Result<Option<NetGraph>> {
        if self.parser_version()? != PARSER_VERSION {
            return Ok(None);
        }
        let mut stmt = self.sql.prepare("SELECT path, hash FROM source_files")?;
        let stored: BTreeMap<String, String> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<_, _>>()?;
        if stored != *hashes || hashes.is_empty() {
            return Ok(None);
        }
        Ok(Some(self.read_graph()?))
    }

    pub fn store(&mut self, hashes: &BTreeMap<String, String>, graph: &NetGraph) -> Result<()> {
        let tx = self.sql.transaction()?;
        tx.execute("DELETE FROM source_files", [])?;
        tx.execute("DELETE FROM components", [])?;
        tx.execute("DELETE FROM copper_nets", [])?;
        tx.execute("DELETE FROM connections", [])?;
        for (path, hash) in hashes {
            tx.execute(
                "INSERT INTO source_files (path, hash) VALUES (?1, ?2)",
                params![path, hash],
            )?;
        }
        for component in &graph.components {
            tx.execute(
                "INSERT INTO components
                 (reference, symbol, value, footprint, description, lcsc, pcbway, dnp, in_bom, in_pos_files)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    component.reference,
                    component.symbol,
                    component.value,
                    component.footprint,
                    component.description,
                    component.lcsc,
                    component.pcbway,
                    component.dnp,
                    component.in_bom,
                    component.in_pos_files
                ],
            )?;
        }
        for net in &graph.nets {
            tx.execute(
                "INSERT INTO copper_nets (name, widths) VALUES (?1, ?2)",
                params![net.name, encode_widths(&net.track_widths_mm)],
            )?;
            let net_id = tx.last_insert_rowid();
            for connection in &net.connections {
                tx.execute(
                    "INSERT INTO connections
                     (net_id, from_ref, from_num, from_name, to_ref, to_num, to_name)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        net_id,
                        connection.from.reference,
                        connection.from.number,
                        connection.from.name,
                        connection.to.reference,
                        connection.to.number,
                        connection.to.name
                    ],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn read_graph(&self) -> Result<NetGraph> {
        let mut components = Vec::new();
        let mut stmt = self.sql.prepare(
            "SELECT reference, symbol, value, footprint, description, lcsc, pcbway, dnp, in_bom, in_pos_files
             FROM components ORDER BY reference",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Component {
                reference: row.get(0)?,
                symbol: row.get(1)?,
                value: row.get(2)?,
                footprint: row.get(3)?,
                description: row.get(4)?,
                lcsc: row.get(5)?,
                pcbway: row.get(6)?,
                dnp: row.get::<_, i64>(7)? != 0,
                in_bom: row.get::<_, i64>(8)? != 0,
                in_pos_files: row.get::<_, i64>(9)? != 0,
            })
        })?;
        for row in rows {
            components.push(row?);
        }
        let mut nets = Vec::new();
        let mut net_stmt = self
            .sql
            .prepare("SELECT id, name, widths FROM copper_nets ORDER BY id")?;
        let net_rows = net_stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        for row in net_rows {
            let (id, name, widths) = row?;
            let mut conn_stmt = self.sql.prepare(
                "SELECT from_ref, from_num, from_name, to_ref, to_num, to_name
                 FROM connections WHERE net_id = ?1",
            )?;
            let connections = conn_stmt.query_map([id], |row| {
                Ok(Connection {
                    name: name.clone(),
                    from: Pin {
                        reference: row.get(0)?,
                        number: row.get(1)?,
                        name: row.get(2)?,
                    },
                    to: Pin {
                        reference: row.get(3)?,
                        number: row.get(4)?,
                        name: row.get(5)?,
                    },
                })
            })?;
            let mut arms = Vec::new();
            for connection in connections {
                arms.push(connection?);
            }
            nets.push(CopperNet {
                name,
                track_widths_mm: decode_widths(&widths),
                connections: arms,
            });
        }
        Ok(NetGraph { components, nets })
    }
}

fn migrate(sql: &Sql) -> Result<()> {
    sql.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )?;
    let mut stmt = sql.prepare("PRAGMA table_info(components)")?;
    let names: Vec<String> = stmt
        .query_map([], |row| row.get(1))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut changed = false;
    if !names.iter().any(|name| name == "dnp") {
        sql.execute(
            "ALTER TABLE components ADD COLUMN dnp INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
        changed = true;
    }
    if !names.iter().any(|name| name == "in_bom") {
        sql.execute(
            "ALTER TABLE components ADD COLUMN in_bom INTEGER NOT NULL DEFAULT 1",
            [],
        )?;
        changed = true;
    }
    if !names.iter().any(|name| name == "in_pos_files") {
        sql.execute(
            "ALTER TABLE components ADD COLUMN in_pos_files INTEGER NOT NULL DEFAULT 1",
            [],
        )?;
        changed = true;
    }
    let version: Option<String> = sql
        .query_row(
            "SELECT value FROM meta WHERE key = 'parser_version'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if changed || version.as_deref() != Some(PARSER_VERSION) {
        sql.execute("DELETE FROM source_files", [])?;
        sql.execute(
            "INSERT INTO meta (key, value) VALUES ('parser_version', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![PARSER_VERSION],
        )?;
    }
    Ok(())
}

impl NetCache {
    fn parser_version(&self) -> Result<String> {
        Ok(self
            .sql
            .query_row(
                "SELECT value FROM meta WHERE key = 'parser_version'",
                [],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or_default())
    }
}

fn encode_widths(widths: &[f64]) -> String {
    widths
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn decode_widths(text: &str) -> Vec<f64> {
    if text.is_empty() {
        return Vec::new();
    }
    text.split(',')
        .filter_map(|item| item.parse().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pcbcraft_domain::net::copper_net;

    #[test]
    fn the_second_open_reads_sqlite() {
        let dir = std::env::temp_dir().join(format!("pcbcraft-net-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("net.sqlite");
        let hashes = BTreeMap::from([("board.kicad_pcb".into(), "abc".into())]);
        let graph = NetGraph {
            components: vec![Component {
                reference: "R1".into(),
                symbol: "Device:R".into(),
                value: "10k".into(),
                footprint: String::new(),
                description: String::new(),
                lcsc: Some("C1".into()),
                pcbway: None,
                dnp: false,
                in_bom: false,
                in_pos_files: true,
            }],
            nets: vec![copper_net(
                "GND",
                vec![
                    pcbcraft_domain::net::Pin {
                        reference: "R1".into(),
                        number: "1".into(),
                        name: String::new(),
                    },
                    pcbcraft_domain::net::Pin {
                        reference: "C1".into(),
                        number: "2".into(),
                        name: String::new(),
                    },
                ],
                vec![0.25],
            )],
        };
        let mut cache = NetCache::open(&path).unwrap();
        assert!(cache.load_if_fresh(&hashes).unwrap().is_none());
        cache.store(&hashes, &graph).unwrap();
        let loaded = cache.load_if_fresh(&hashes).unwrap().unwrap();
        assert_eq!(loaded.components[0].lcsc.as_deref(), Some("C1"));
        assert!(!loaded.components[0].in_bom);
        assert!(loaded.components[0].in_pos_files);
        assert_eq!(loaded.nets[0].track_widths_mm, vec![0.25]);
        cache
            .sql
            .execute(
                "UPDATE meta SET value = '1' WHERE key = 'parser_version'",
                [],
            )
            .unwrap();
        assert!(cache.load_if_fresh(&hashes).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
