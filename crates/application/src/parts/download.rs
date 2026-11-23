//! Download one or more LCSC footprints, and a STEP model when EasyEDA has one.

use std::path::{Path, PathBuf};

use pcbcraft_domain::easyeda::{self, EasyedaError};

use crate::ports::{CadDocument, EasyedaCad, FootprintStore};

#[derive(Debug, Clone)]
pub struct DownloadJob {
    pub ids: Vec<String>,
    pub pretty_dir: PathBuf,
    pub shapes_dir: PathBuf,
    pub anchor: PathBuf,
    pub table: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadReport {
    pub pretty_dir: PathBuf,
    pub parts: Vec<PartOutcome>,
    pub lib_uri: String,
    pub registered: Option<PathBuf>,
    pub register_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartOutcome {
    pub lcsc: String,
    pub kind: PartKind,
    pub footprint: Option<String>,
    pub step: Option<PathBuf>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartKind {
    Model,
    Footprint,
    Failed,
}

pub fn kicad_alias(anchor: &Path, target: &Path) -> String {
    match target.strip_prefix(anchor) {
        Ok(relative) if relative.as_os_str().is_empty() => "${KIPRJMOD}".to_string(),
        Ok(relative) => {
            let relative = relative.to_string_lossy().replace('\\', "/");
            format!("${{KIPRJMOD}}/{relative}")
        }
        Err(_) => target.to_string_lossy().replace('\\', "/"),
    }
}

pub fn download_models(
    job: &DownloadJob,
    source: &impl EasyedaCad,
    store: &impl FootprintStore,
) -> DownloadReport {
    let lib_uri = kicad_alias(&job.anchor, &job.pretty_dir);
    let shapes_uri = kicad_alias(&job.anchor, &job.shapes_dir);
    let mut parts = Vec::new();
    for raw in &job.ids {
        parts.push(one_part(raw, job, &shapes_uri, source, store));
    }
    let mut registered = None;
    let mut register_error = None;
    let saved = parts.iter().any(|part| part.kind != PartKind::Failed);
    if saved {
        if let Some(table) = &job.table {
            match store.register(table, &lib_uri) {
                Ok(()) => registered = Some(table.clone()),
                Err(err) => register_error = Some(format!("could not update fp-lib-table: {err}")),
            }
        }
    }
    DownloadReport {
        pretty_dir: job.pretty_dir.clone(),
        parts,
        lib_uri,
        registered,
        register_error,
    }
}

fn one_part(
    raw: &str,
    job: &DownloadJob,
    shapes_uri: &str,
    source: &impl EasyedaCad,
    store: &impl FootprintStore,
) -> PartOutcome {
    let Some(lcsc) = normalize_lcsc(raw) else {
        return failed(raw.trim(), "LCSC number must look like C12345");
    };
    let document = match source.component(&lcsc) {
        Ok(document) => document,
        Err(err) => return failed(&lcsc, &err.to_string()),
    };
    let CadDocument::Found(result) = document else {
        return failed(&lcsc, "no component for this LCSC number");
    };
    let footprint = match easyeda::convert(&lcsc, &result) {
        Ok(footprint) => footprint,
        Err(err) => return failed(&lcsc, &failure_text(&err)),
    };
    let mut notes = Vec::new();
    let (placement, step) = match &footprint.model {
        None => {
            notes.push("no 3D model in the EasyEDA footprint".to_string());
            (None, None)
        }
        Some(spec) => match source.step(&spec.uuid) {
            Err(_) => {
                notes.push("3D model download failed".to_string());
                (None, None)
            }
            Ok(None) => {
                notes.push("EasyEDA did not return a STEP file".to_string());
                (None, None)
            }
            Ok(Some(bytes)) => {
                let obj = source.obj(&spec.uuid).ok().flatten();
                let placement = easyeda::place_model(spec, obj.as_deref());
                if !placement.bounds_known {
                    notes.push("3D offset has no mesh bounds".to_string());
                }
                (Some(placement), Some(bytes))
            }
        },
    };
    let model_uri = format!("{shapes_uri}/{}.step", footprint.name);
    let name = format!("EasyEDA:{}", footprint.name);
    match store.save(
        &job.pretty_dir,
        &job.shapes_dir,
        &footprint,
        placement.as_ref(),
        step.as_deref(),
        &model_uri,
    ) {
        Ok(saved) => PartOutcome {
            lcsc,
            kind: if saved.step_path.is_some() {
                PartKind::Model
            } else {
                PartKind::Footprint
            },
            footprint: Some(name),
            step: saved.step_path,
            notes,
        },
        Err(err) => failed(&lcsc, &format!("could not write the footprint: {err}")),
    }
}

fn failed(lcsc: &str, note: &str) -> PartOutcome {
    PartOutcome {
        lcsc: lcsc.to_string(),
        kind: PartKind::Failed,
        footprint: None,
        step: None,
        notes: vec![note.to_string()],
    }
}

fn failure_text(err: &EasyedaError) -> String {
    err.to_string()
}

pub fn normalize_lcsc(raw: &str) -> Option<String> {
    let text = raw.trim();
    let mut chars = text.chars();
    let first = chars.next()?;
    if !first.eq_ignore_ascii_case(&'c') {
        return None;
    }
    let digits: String = chars.collect();
    if digits.is_empty() || !digits.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    Some(format!("C{digits}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::CadDocument;
    use anyhow::Result;
    use serde_json::json;
    use std::cell::RefCell;
    use std::collections::HashMap;

    struct Source {
        documents: HashMap<String, CadDocument>,
        steps: HashMap<String, Vec<u8>>,
        fail_step: bool,
    }

    impl EasyedaCad for Source {
        fn component(&self, lcsc: &str) -> Result<CadDocument> {
            Ok(self
                .documents
                .get(lcsc)
                .cloned()
                .unwrap_or(CadDocument::Missing))
        }

        fn step(&self, uuid: &str) -> Result<Option<Vec<u8>>> {
            if self.fail_step {
                anyhow::bail!("offline");
            }
            Ok(self.steps.get(uuid).cloned())
        }

        fn obj(&self, _: &str) -> Result<Option<String>> {
            Ok(Some("v 0 0 0\nv 2 0 0\nv 2 2 1\nv 0 2 1\n".into()))
        }
    }

    struct Store {
        saved: RefCell<Vec<String>>,
        fail_register: bool,
    }

    impl FootprintStore for Store {
        fn save(
            &self,
            _: &Path,
            _: &Path,
            footprint: &pcbcraft_domain::easyeda::LibraryFootprint,
            _: Option<&pcbcraft_domain::easyeda::ModelPlacement>,
            step: Option<&[u8]>,
            _: &str,
        ) -> Result<crate::ports::SavedFootprint> {
            self.saved.borrow_mut().push(footprint.name.clone());
            Ok(crate::ports::SavedFootprint {
                footprint_path: PathBuf::from(format!("{}.kicad_mod", footprint.name)),
                step_path: step.map(|_| PathBuf::from(format!("{}.step", footprint.name))),
            })
        }

        fn register(&self, _: &Path, _: &str) -> Result<()> {
            if self.fail_register {
                anyhow::bail!("disk full");
            }
            Ok(())
        }
    }

    fn drawing(uuid: Option<&str>) -> CadDocument {
        let mut shapes = vec!["PAD~RECT~3990~3000~10~10~1~~1~0~~0~id~0~~Y~0".to_string()];
        if let Some(uuid) = uuid {
            shapes.push(format!(
                "SVGNODE~{{\"attrs\":{{\"uuid\":\"{uuid}\",\"c_origin\":\"4000,3000\",\"z\":\"0\",\"c_rotation\":\"0,0,0\"}}}}~"
            ));
        }
        CadDocument::Found(json!({
            "packageDetail": {
                "title": "R0805",
                "dataStr": {"head": {"x": 4000, "y": 3000}, "shape": shapes}
            }
        }))
    }

    fn job() -> DownloadJob {
        DownloadJob {
            ids: vec!["c2040".into(), "C0".into(), "nope".into()],
            pretty_dir: PathBuf::from("/proj/.pcbcraft/EasyEDA.pretty"),
            shapes_dir: PathBuf::from("/proj/.pcbcraft/EasyEDA.3dshapes"),
            anchor: PathBuf::from("/proj"),
            table: Some(PathBuf::from("/proj/fp-lib-table")),
        }
    }

    #[test]
    fn one_failure_does_not_stop_the_parts_that_can_be_saved() {
        let mut documents = HashMap::new();
        documents.insert("C2040".into(), drawing(Some("model-1")));
        documents.insert("C0".into(), drawing(None));
        let source = Source {
            documents,
            steps: HashMap::from([("model-1".into(), b"ISO-10303-21;".to_vec())]),
            fail_step: false,
        };
        let store = Store {
            saved: RefCell::new(Vec::new()),
            fail_register: false,
        };
        let report = download_models(&job(), &source, &store);
        assert_eq!(report.parts[0].kind, PartKind::Model);
        assert_eq!(report.parts[0].lcsc, "C2040");
        assert_eq!(report.parts[0].footprint.as_deref(), Some("EasyEDA:R0805"));
        assert!(report.parts[0].notes.is_empty());
        assert_eq!(report.parts[1].kind, PartKind::Footprint);
        assert_eq!(
            report.parts[1].notes,
            vec!["no 3D model in the EasyEDA footprint".to_string()]
        );
        assert_eq!(report.parts[2].kind, PartKind::Failed);
        assert_eq!(report.parts[2].lcsc, "nope");
        assert_eq!(store.saved.borrow().len(), 2);
        assert_eq!(report.lib_uri, "${KIPRJMOD}/.pcbcraft/EasyEDA.pretty");
        assert!(report.registered.is_some());
    }

    #[test]
    fn a_library_outside_the_project_uses_an_absolute_model_path() {
        assert_eq!(
            kicad_alias(Path::new("/proj"), Path::new("/other/EasyEDA.pretty")),
            "/other/EasyEDA.pretty"
        );
    }
}
