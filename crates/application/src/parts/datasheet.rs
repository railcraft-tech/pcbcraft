//! Save an LCSC datasheet as markdown, with figures under `images/`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

use super::download::normalize_lcsc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductCard {
    pub lcsc: String,
    pub mpn: String,
    pub manufacturer: String,
    pub datasheet_url: String,
    pub extras: Vec<RemoteFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFile {
    pub name: String,
    pub url: String,
    pub kind: FileKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Jpeg,
    Pdf,
    Zip,
    Svg,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileBody {
    pub bytes: Vec<u8>,
    pub content_type: String,
}

pub trait DatasheetCatalog {
    fn card(&self, lcsc: &str) -> Result<ProductCard>;
}

pub trait FileFetch {
    fn get(&self, url: &str) -> Result<FileBody>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertedPdf {
    pub markdown: String,
    pub pages: u32,
    pub warning: Option<String>,
}

pub trait PdfToMarkdown {
    fn convert(&self, dir: &Path) -> Result<ConvertedPdf>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatasheetReport {
    pub files: Vec<PathBuf>,
    pub warning: Option<String>,
}

pub fn download_datasheet(
    raw: &str,
    output_dir: &Path,
    catalog: &impl DatasheetCatalog,
    fetch: &impl FileFetch,
    converter: &impl PdfToMarkdown,
) -> Result<DatasheetReport> {
    let Some(lcsc) = normalize_lcsc(raw) else {
        bail!("LCSC number must look like C12345");
    };
    let card = catalog.card(&lcsc)?;
    if card.datasheet_url.is_empty() {
        bail!("LCSC {lcsc} has no datasheet");
    }
    let (pdf, source) = load_pdf(fetch, &card.datasheet_url)?;
    let mut extras = Vec::new();
    for file in &card.extras {
        let body = fetch
            .get(&file.url)
            .with_context(|| format!("download {}", file.url))?;
        check_extra(file, &body)?;
        extras.push((file.name.clone(), body.bytes));
    }

    let scratch = Scratch::new()?;
    fs::write(scratch.path.join("datasheet.pdf"), &pdf).context("could not store the datasheet")?;
    if !extras.is_empty() {
        let extra_dir = scratch.path.join("extra");
        fs::create_dir_all(&extra_dir).context("could not create extra")?;
        for (name, bytes) in &extras {
            fs::write(extra_dir.join(name), bytes)
                .with_context(|| format!("could not store {name}"))?;
        }
    }
    let converted = converter.convert(&scratch.path)?;
    check_image_links(&converted.markdown, &scratch.path)?;
    let markdown = datasheet_markdown(
        &card,
        &source,
        &sha256_hex(&pdf),
        converted.pages,
        &converted.markdown,
    );
    fs::write(scratch.path.join("datasheet.md"), markdown)
        .context("could not store datasheet.md")?;
    let files = install(&scratch.path, output_dir)?;
    Ok(DatasheetReport {
        files,
        warning: converted.warning,
    })
}

fn load_pdf(fetch: &impl FileFetch, url: &str) -> Result<(Vec<u8>, String)> {
    let body = fetch.get(url).with_context(|| format!("download {url}"))?;
    if is_pdf(&body.bytes) {
        return Ok((body.bytes, url.to_string()));
    }
    let text = String::from_utf8_lossy(&body.bytes);
    if let Some(next) = preview_pdf_url(&text) {
        if next != url {
            let second = fetch
                .get(&next)
                .with_context(|| format!("download {next}"))?;
            if is_pdf(&second.bytes) {
                return Ok((second.bytes, next));
            }
            bail!("datasheet at {next} is not a PDF ({})", second.content_type);
        }
    }
    bail!("datasheet at {url} is not a PDF ({})", body.content_type);
}

fn check_extra(file: &RemoteFile, body: &FileBody) -> Result<()> {
    let ok = match file.kind {
        FileKind::Jpeg => body.bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
        FileKind::Pdf => is_pdf(&body.bytes),
        FileKind::Zip => body.bytes.starts_with(b"PK"),
        FileKind::Svg => is_svg(&body.bytes),
    };
    if ok {
        return Ok(());
    }
    let kind = match file.kind {
        FileKind::Jpeg => "JPEG",
        FileKind::Pdf => "PDF",
        FileKind::Zip => "ZIP",
        FileKind::Svg => "SVG",
    };
    bail!("{} is not a {kind} ({})", file.name, body.content_type);
}

pub fn is_pdf(bytes: &[u8]) -> bool {
    bytes.starts_with(b"%PDF-")
}

fn is_svg(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes);
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    trimmed.starts_with("<svg") || trimmed.starts_with("<?xml")
}

pub fn preview_pdf_url(text: &str) -> Option<String> {
    let index = text.find("previewPdfUrl")?;
    let rest = &text[index..];
    let http = rest.find("https:").or_else(|| rest.find("http:"))?;
    let tail = &rest[http..];
    let end = tail.find(['"', '\'']).unwrap_or(tail.len());
    let url = tail[..end].replace("\\/", "/");
    if url.starts_with("http://") || url.starts_with("https://") {
        Some(url)
    } else {
        None
    }
}

fn datasheet_markdown(
    card: &ProductCard,
    source: &str,
    sha: &str,
    pages: u32,
    body: &str,
) -> String {
    let title = if card.mpn.is_empty() {
        card.lcsc.clone()
    } else {
        one_line(&card.mpn)
    };
    let mut out = format!(
        "# {title}\n\n| Field | Value |\n| --- | --- |\n| LCSC | {} |\n| Manufacturer | {} |\n| Source | {} |\n| SHA-256 | {sha} |\n| Pages | {pages} |\n\n",
        cell(&card.lcsc),
        cell(&card.manufacturer),
        cell(source),
    );
    out.push_str(body);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn cell(text: &str) -> String {
    one_line(text).replace('|', "\\|")
}

fn one_line(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn check_image_links(markdown: &str, dir: &Path) -> Result<()> {
    for target in image_targets(markdown) {
        if target.starts_with("data:") {
            bail!("datasheet markdown embeds an image; images belong in images/");
        }
        let Some(name) = target.strip_prefix("images/") else {
            bail!("datasheet markdown links an image outside images/: {target}");
        };
        if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
            bail!("datasheet markdown links an image outside images/: {target}");
        }
        let path = dir.join("images").join(name);
        if !path.is_file() {
            bail!("datasheet markdown links a missing image: {target}");
        }
    }
    Ok(())
}

fn image_targets(markdown: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = markdown;
    while let Some(start) = rest.find("![") {
        rest = &rest[start + 2..];
        let Some(close) = rest.find("](") else {
            break;
        };
        rest = &rest[close + 2..];
        let Some(end) = rest.find(')') else {
            break;
        };
        let raw = rest[..end].trim();
        let raw = raw.split_whitespace().next().unwrap_or(raw);
        let raw = raw.trim_matches(|ch| ch == '<' || ch == '>');
        if !raw.is_empty() {
            out.push(raw.to_string());
        }
        rest = &rest[end + 1..];
    }
    out
}

fn install(scratch: &Path, dest: &Path) -> Result<Vec<PathBuf>> {
    fs::create_dir_all(dest).with_context(|| format!("could not create {}", dest.display()))?;
    copy_file(&scratch.join("datasheet.pdf"), &dest.join("datasheet.pdf"))?;
    copy_file(&scratch.join("datasheet.md"), &dest.join("datasheet.md"))?;
    replace_tree(&scratch.join("images"), &dest.join("images"))?;
    replace_tree(&scratch.join("extra"), &dest.join("extra"))?;
    written(dest)
}

fn copy_file(from: &Path, to: &Path) -> Result<()> {
    fs::copy(from, to).with_context(|| format!("could not write {}", to.display()))?;
    Ok(())
}

fn replace_tree(from: &Path, to: &Path) -> Result<()> {
    let staging = match to.file_name() {
        Some(name) => to.with_file_name(format!(".{}.new", name.to_string_lossy())),
        None => bail!("could not replace {}", to.display()),
    };
    if staging.exists() {
        fs::remove_dir_all(&staging)
            .with_context(|| format!("could not replace {}", to.display()))?;
    }
    if from.exists() {
        copy_dir_files(from, &staging)?;
    }
    if to.exists() {
        fs::remove_dir_all(to).with_context(|| format!("could not replace {}", to.display()))?;
    }
    if staging.exists() {
        fs::rename(&staging, to).with_context(|| format!("could not replace {}", to.display()))?;
    }
    Ok(())
}

fn copy_dir_files(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to).with_context(|| format!("could not create {}", to.display()))?;
    for entry in fs::read_dir(from).with_context(|| format!("could not read {}", from.display()))? {
        let entry = entry?;
        if !entry
            .file_type()
            .with_context(|| format!("could not read {}", entry.path().display()))?
            .is_file()
        {
            bail!("{} is not a file", entry.path().display());
        }
        copy_file(&entry.path(), &to.join(entry.file_name()))?;
    }
    Ok(())
}

fn written(dest: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = vec![dest.join("datasheet.pdf"), dest.join("datasheet.md")];
    push_files(&mut paths, &dest.join("images"))?;
    push_files(&mut paths, &dest.join("extra"))?;
    Ok(paths)
}

fn push_files(paths: &mut Vec<PathBuf>, dir: &Path) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("could not read {}", dir.display()))? {
        let entry = entry?;
        if entry
            .file_type()
            .with_context(|| format!("could not read {}", entry.path().display()))?
            .is_file()
        {
            names.push(entry.file_name());
        }
    }
    names.sort();
    for name in names {
        paths.push(dir.join(name));
    }
    Ok(())
}

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new() -> Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let path =
            std::env::temp_dir().join(format!("pcbcraft-datasheet-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&path)
            .with_context(|| format!("could not create {}", path.display()))?;
        Ok(Self { path })
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct Fixed {
        card: ProductCard,
        bodies: HashMap<String, FileBody>,
    }

    impl DatasheetCatalog for Fixed {
        fn card(&self, lcsc: &str) -> Result<ProductCard> {
            assert_eq!(lcsc, "C55266");
            Ok(self.card.clone())
        }
    }

    impl FileFetch for Fixed {
        fn get(&self, url: &str) -> Result<FileBody> {
            self.bodies
                .get(url)
                .cloned()
                .with_context(|| format!("missing {url}"))
        }
    }

    struct Writer {
        markdown: String,
        pages: u32,
        warning: Option<String>,
        images: Vec<String>,
    }

    impl PdfToMarkdown for Writer {
        fn convert(&self, dir: &Path) -> Result<ConvertedPdf> {
            fs::create_dir_all(dir.join("images"))?;
            for name in &self.images {
                fs::write(dir.join("images").join(name), b"png")?;
            }
            Ok(ConvertedPdf {
                markdown: self.markdown.clone(),
                pages: self.pages,
                warning: self.warning.clone(),
            })
        }
    }

    fn card() -> ProductCard {
        ProductCard {
            lcsc: "C55266".into(),
            mpn: "TPS2553DBVR".into(),
            manufacturer: "TI".into(),
            datasheet_url: "https://datasheet.example/a.pdf".into(),
            extras: vec![
                RemoteFile {
                    name: "front.jpg".into(),
                    url: "https://assets.example/front.jpg".into(),
                    kind: FileKind::Jpeg,
                },
                RemoteFile {
                    name: "symbol.svg".into(),
                    url: "https://image.example/sch.svg".into(),
                    kind: FileKind::Svg,
                },
            ],
        }
    }

    fn bodies() -> HashMap<String, FileBody> {
        HashMap::from([
            (
                "https://datasheet.example/a.pdf".into(),
                FileBody {
                    bytes: b"%PDF-1.4\nhello".to_vec(),
                    content_type: "application/pdf".into(),
                },
            ),
            (
                "https://assets.example/front.jpg".into(),
                FileBody {
                    bytes: vec![0xFF, 0xD8, 0xFF, 0xD9],
                    content_type: "image/jpeg".into(),
                },
            ),
            (
                "https://image.example/sch.svg".into(),
                FileBody {
                    bytes: b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>".to_vec(),
                    content_type: "text/html".into(),
                },
            ),
        ])
    }

    fn writer() -> Writer {
        Writer {
            markdown: "Pin 1\n\n![](images/datasheet-0001-00.png)\n\n[TI](https://www.ti.com)\n"
                .into(),
            pages: 43,
            warning: None,
            images: vec!["datasheet-0001-00.png".into()],
        }
    }

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("pcbcraft-{label}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn a_bad_number_does_not_fetch_or_write() {
        struct PanicCat;
        impl DatasheetCatalog for PanicCat {
            fn card(&self, _: &str) -> Result<ProductCard> {
                panic!("catalog was called");
            }
        }
        struct PanicFetch;
        impl FileFetch for PanicFetch {
            fn get(&self, _: &str) -> Result<FileBody> {
                panic!("fetch was called");
            }
        }
        struct PanicPdf;
        impl PdfToMarkdown for PanicPdf {
            fn convert(&self, _: &Path) -> Result<ConvertedPdf> {
                panic!("converter was called");
            }
        }
        let dir = temp_dir("bad-lcsc");
        let err = download_datasheet("nope", &dir, &PanicCat, &PanicFetch, &PanicPdf).unwrap_err();
        assert!(err.to_string().contains("C12345"), "{err}");
        assert!(fs::read_dir(&dir).unwrap().next().is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_datasheet_is_markdown_and_a_hand_note_stays() {
        let dir = temp_dir("datasheet");
        fs::write(dir.join("TPS2553DBVR.md"), "hand note\n").unwrap();
        fs::create_dir_all(dir.join("images")).unwrap();
        fs::write(dir.join("images").join("stale.png"), b"old").unwrap();
        let source = Fixed {
            card: card(),
            bodies: bodies(),
        };
        let report = download_datasheet("c55266", &dir, &source, &source, &writer()).unwrap();
        let markdown = fs::read_to_string(dir.join("datasheet.md")).unwrap();
        assert!(markdown.starts_with("# TPS2553DBVR\n"));
        assert!(markdown.contains("| LCSC | C55266 |"));
        assert!(markdown.contains("| Manufacturer | TI |"));
        assert!(markdown.contains("| Source | https://datasheet.example/a.pdf |"));
        assert!(markdown.contains(&format!("| SHA-256 | {} |", sha256_hex(b"%PDF-1.4\nhello"))));
        assert!(markdown.contains("| Pages | 43 |"));
        assert!(markdown.contains("![](images/datasheet-0001-00.png)"));
        assert!(markdown.contains("[TI](https://www.ti.com)"));
        assert_eq!(
            fs::read(dir.join("datasheet.pdf")).unwrap(),
            b"%PDF-1.4\nhello"
        );
        assert_eq!(
            fs::read(dir.join("extra").join("front.jpg")).unwrap()[0],
            0xFF
        );
        assert!(dir.join("extra").join("symbol.svg").is_file());
        assert!(!dir.join("images").join("stale.png").exists());
        assert_eq!(
            fs::read_to_string(dir.join("TPS2553DBVR.md")).unwrap(),
            "hand note\n"
        );
        assert!(report.warning.is_none());
        let names: Vec<_> = report
            .files
            .iter()
            .map(|path| {
                path.strip_prefix(&dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert_eq!(
            names,
            vec![
                "datasheet.pdf",
                "datasheet.md",
                "images/datasheet-0001-00.png",
                "extra/front.jpg",
                "extra/symbol.svg",
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_html_viewer_is_followed_once_to_the_pdf() {
        let dir = temp_dir("viewer");
        let mut fetched = bodies();
        fetched.insert(
            "https://datasheet.example/a.pdf".into(),
            FileBody {
                bytes: br#"<html>"previewPdfUrl":"https:\/\/cdn.example\/real.pdf"</html>"#
                    .to_vec(),
                content_type: "text/html".into(),
            },
        );
        fetched.insert(
            "https://cdn.example/real.pdf".into(),
            FileBody {
                bytes: b"%PDF-1.7\nreal".to_vec(),
                content_type: "application/pdf".into(),
            },
        );
        let source = Fixed {
            card: card(),
            bodies: fetched,
        };
        download_datasheet("C55266", &dir, &source, &source, &writer()).unwrap();
        let markdown = fs::read_to_string(dir.join("datasheet.md")).unwrap();
        assert!(
            markdown.contains("| Source | https://cdn.example/real.pdf |"),
            "{markdown}"
        );
        assert_eq!(
            fs::read(dir.join("datasheet.pdf")).unwrap(),
            b"%PDF-1.7\nreal"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_page_that_is_not_a_pdf_leaves_the_directory_alone() {
        let dir = temp_dir("html");
        fs::write(dir.join("TPS2553DBVR.md"), "hand note\n").unwrap();
        let mut fetched = bodies();
        fetched.insert(
            "https://datasheet.example/a.pdf".into(),
            FileBody {
                bytes: b"<html>no pdf</html>".to_vec(),
                content_type: "text/html".into(),
            },
        );
        let source = Fixed {
            card: card(),
            bodies: fetched,
        };
        let err = download_datasheet("C55266", &dir, &source, &source, &writer()).unwrap_err();
        assert!(err.to_string().contains("not a PDF"), "{err}");
        assert!(!dir.join("datasheet.md").exists());
        assert_eq!(
            fs::read_to_string(dir.join("TPS2553DBVR.md")).unwrap(),
            "hand note\n"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_figure_and_an_embedded_image_are_rejected() {
        let dir = temp_dir("links");
        let source = Fixed {
            card: card(),
            bodies: bodies(),
        };
        let missing = Writer {
            markdown: "![](images/nope.png)\n".into(),
            pages: 1,
            warning: None,
            images: vec![],
        };
        let err = download_datasheet("C55266", &dir, &source, &source, &missing).unwrap_err();
        assert!(err.to_string().contains("missing image"), "{err}");
        assert!(!dir.join("datasheet.md").exists());

        let embedded = Writer {
            markdown: "![](data:image/png;base64,aaaa)\n".into(),
            pages: 1,
            warning: None,
            images: vec![],
        };
        let err = download_datasheet("C55266", &dir, &source, &source, &embedded).unwrap_err();
        assert!(err.to_string().contains("embeds an image"), "{err}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_preview_url_unescapes_slashes() {
        let html = r#"previewPdfUrl":"https:\/\/datasheet.lcsc.com\/a.pdf?productCode=C1""#;
        assert_eq!(
            preview_pdf_url(html).as_deref(),
            Some("https://datasheet.lcsc.com/a.pdf?productCode=C1")
        );
        assert!(preview_pdf_url("<html></html>").is_none());
    }
}
