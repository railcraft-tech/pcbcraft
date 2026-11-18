use std::path::Path;

use anyhow::{Context, Result};
use pcbcraft_application::ports::{ComponentStore, Usage};
use pcbcraft_domain::parts::{ElecParam, PartRecord, Supplier};
use rusqlite::{params, Connection};

pub struct SqliteComponents {
    path: std::path::PathBuf,
}

impl SqliteComponents {
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        Self { path: path.into() }
    }

    fn open(&self) -> Result<Connection> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let sql = Connection::open(&self.path)
            .with_context(|| format!("database {}", self.path.display()))?;
        sql.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS parts (
                supplier TEXT NOT NULL,
                part_id TEXT NOT NULL,
                mpn TEXT NOT NULL,
                manufacturer TEXT NOT NULL,
                package TEXT NOT NULL,
                category TEXT NOT NULL,
                description TEXT NOT NULL,
                stock INTEGER,
                price REAL,
                library TEXT,
                fetched_at TEXT NOT NULL,
                PRIMARY KEY (supplier, part_id)
            );
            CREATE TABLE IF NOT EXISTS parameters (
                supplier TEXT NOT NULL,
                part_id TEXT NOT NULL,
                name TEXT NOT NULL,
                value TEXT NOT NULL,
                si REAL
            );
            CREATE TABLE IF NOT EXISTS usage (
                reference TEXT NOT NULL,
                supplier TEXT NOT NULL,
                part_id TEXT NOT NULL
            );
            ",
        )?;
        Ok(sql)
    }
}

impl ComponentStore for SqliteComponents {
    fn part_ids(&self) -> Result<Vec<(Supplier, String)>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let sql = self.open()?;
        let mut stmt = sql.prepare("SELECT supplier, part_id FROM parts")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.map(|row| {
            let (supplier, id) = row?;
            Ok((parse_supplier(&supplier)?, id))
        })
        .collect()
    }

    fn card(&self, supplier: Supplier, part_id: &str) -> Result<Option<PartRecord>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let sql = self.open()?;
        let mut stmt = sql.prepare(
            "SELECT mpn, manufacturer, package, category, description, stock, price, library
             FROM parts WHERE supplier = ?1 AND part_id = ?2",
        )?;
        let mut rows = stmt.query(params![supplier.as_str(), part_id])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let mut record = PartRecord {
            supplier,
            part_id: part_id.to_string(),
            mpn: row.get(0)?,
            manufacturer: row.get(1)?,
            package: row.get(2)?,
            category: row.get(3)?,
            description: row.get(4)?,
            stock: row.get::<_, Option<i64>>(5)?.map(|value| value as u64),
            price: row.get(6)?,
            library: row.get(7)?,
            parameters: Vec::new(),
        };
        let mut params_stmt = sql.prepare(
            "SELECT name, value, si FROM parameters WHERE supplier = ?1 AND part_id = ?2 ORDER BY name",
        )?;
        let params_rows = params_stmt.query_map(params![supplier.as_str(), part_id], |param| {
            Ok(ElecParam {
                name: param.get(0)?,
                value: param.get(1)?,
                si: param.get(2)?,
            })
        })?;
        for param in params_rows {
            record.parameters.push(param?);
        }
        Ok(Some(record))
    }

    fn find(&self, part_id: &str) -> Result<Vec<PartRecord>> {
        if !self.path.exists() {
            anyhow::bail!(
                "components database is missing ({}); run pcbcraft components sync",
                self.path.display()
            );
        }
        let sql = self.open()?;
        let mut stmt =
            sql.prepare("SELECT supplier FROM parts WHERE part_id = ?1 ORDER BY supplier")?;
        let suppliers = stmt.query_map(params![part_id], |row| row.get::<_, String>(0))?;
        let mut found = Vec::new();
        for supplier in suppliers {
            let supplier = parse_supplier(&supplier?)?;
            if let Some(record) = self.card(supplier, part_id)? {
                found.push(record);
            }
        }
        Ok(found)
    }

    fn replace(&self, usage: &[Usage], fetched: &[PartRecord], drop_missing: bool) -> Result<()> {
        let mut sql = self.open()?;
        let tx = sql.transaction()?;
        tx.execute("DELETE FROM usage", [])?;
        for row in usage {
            tx.execute(
                "INSERT INTO usage (reference, supplier, part_id) VALUES (?1, ?2, ?3)",
                params![row.reference, row.supplier.as_str(), row.part_id],
            )?;
        }
        for record in fetched {
            upsert(&tx, record)?;
        }
        if drop_missing {
            tx.execute(
                "DELETE FROM parameters WHERE (supplier, part_id) NOT IN (SELECT supplier, part_id FROM usage)",
                [],
            )?;
            tx.execute(
                "DELETE FROM parts WHERE (supplier, part_id) NOT IN (SELECT supplier, part_id FROM usage)",
                [],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

fn upsert(tx: &rusqlite::Transaction<'_>, record: &PartRecord) -> Result<()> {
    tx.execute(
        "INSERT INTO parts (supplier, part_id, mpn, manufacturer, package, category, description, stock, price, library, fetched_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, datetime('now'))
         ON CONFLICT(supplier, part_id) DO UPDATE SET
            mpn = excluded.mpn,
            manufacturer = excluded.manufacturer,
            package = excluded.package,
            category = excluded.category,
            description = excluded.description,
            stock = excluded.stock,
            price = excluded.price,
            library = excluded.library,
            fetched_at = excluded.fetched_at",
        params![
            record.supplier.as_str(),
            record.part_id,
            record.mpn,
            record.manufacturer,
            record.package,
            record.category,
            record.description,
            record.stock.map(|value| value as i64),
            record.price,
            record.library
        ],
    )?;
    tx.execute(
        "DELETE FROM parameters WHERE supplier = ?1 AND part_id = ?2",
        params![record.supplier.as_str(), record.part_id],
    )?;
    for param in &record.parameters {
        write_param(tx, record, param)?;
    }
    Ok(())
}

fn write_param(
    tx: &rusqlite::Transaction<'_>,
    record: &PartRecord,
    param: &ElecParam,
) -> Result<()> {
    tx.execute(
        "INSERT INTO parameters (supplier, part_id, name, value, si) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            record.supplier.as_str(),
            record.part_id,
            param.name,
            param.value,
            param.si
        ],
    )?;
    Ok(())
}

fn parse_supplier(text: &str) -> Result<Supplier> {
    match text {
        "lcsc" => Ok(Supplier::Lcsc),
        "pcbway" => Ok(Supplier::Pcbway),
        other => anyhow::bail!("unknown supplier {other}"),
    }
}

pub fn open_project_db(path: &Path) -> SqliteComponents {
    SqliteComponents::new(path)
}

/// Opens `.pcbcraft/components.sqlite3` next to a project file.
pub struct ProjectCards;

impl pcbcraft_application::ports::PartCards for ProjectCards {
    fn card(
        &self,
        project: &Path,
        supplier: Supplier,
        part_id: &str,
    ) -> Result<Option<PartRecord>> {
        let resolved = crate::project::resolve(Some(project), None, None)?;
        let path = resolved.component_db();
        if !path.exists() {
            anyhow::bail!(
                "components database is missing ({}); run pcbcraft components sync",
                path.display()
            );
        }
        SqliteComponents::new(path).card(supplier, part_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pcbcraft_application::ports::{ComponentStore, Usage};
    use pcbcraft_domain::parts::{ElecParam, PartRecord, Supplier};

    #[test]
    fn a_refresh_replaces_parameters_and_drops_unused_parts() {
        let dir = std::env::temp_dir().join(format!("pcbcraft-parts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = SqliteComponents::new(dir.join("components.sqlite3"));
        let usage = vec![Usage {
            reference: "R1".into(),
            supplier: Supplier::Lcsc,
            part_id: "C1".into(),
        }];
        let record = PartRecord {
            supplier: Supplier::Lcsc,
            part_id: "C1".into(),
            mpn: "RC".into(),
            manufacturer: "Yageo".into(),
            package: "0805".into(),
            category: "Resistors".into(),
            description: String::new(),
            stock: Some(4),
            price: Some(0.01),
            library: Some("basic".into()),
            parameters: vec![ElecParam {
                name: "Resistance".into(),
                value: "10k".into(),
                si: Some(10_000.0),
            }],
        };
        store.replace(&usage, &[record], true).unwrap();
        assert_eq!(
            store.part_ids().unwrap(),
            vec![(Supplier::Lcsc, "C1".into())]
        );
        let card = store.card(Supplier::Lcsc, "C1").unwrap().unwrap();
        assert_eq!(card.parameters[0].value, "10k");
        assert_eq!(card.parameters[0].si, Some(10_000.0));
        let found = store.find("C1").unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].parameters[0].name, "Resistance");
        assert!(store.find("C999").unwrap().is_empty());
        store.replace(&[], &[], true).unwrap();
        assert!(store.part_ids().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
