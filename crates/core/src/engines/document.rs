//! Text documents. macOS `textutil` covers DOCX/DOC/ODT/RTF/HTML/TXT; we add
//! Markdown, EPUB and PDF on top. LibreOffice, when installed, is used for
//! slides and for higher fidelity office → PDF.

use super::{exec, find_tool, scratch};
use crate::kind::ext;
use crate::output::stem;
use crate::{output_path, Error, Options, Result};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const TEXTUTIL: &str = "/usr/bin/textutil";
const TEXTUTIL_IN: &[&str] = &[
    "txt",
    "text",
    "rtf",
    "rtfd",
    "html",
    "htm",
    "doc",
    "docx",
    "odt",
    "wordml",
    "webarchive",
];

pub fn office() -> Option<PathBuf> {
    let app = PathBuf::from("/Applications/LibreOffice.app/Contents/MacOS/soffice");
    if app.is_file() {
        return Some(app);
    }
    find_tool("soffice")
}

/// macOS's `textutil`. Elsewhere LibreOffice does its work.
fn has_textutil() -> bool {
    Path::new(TEXTUTIL).is_file()
}

/// Output formats for a document with extension `e` on this machine.
pub fn targets(e: &str) -> Vec<&'static str> {
    if has_textutil() || office_available() {
        vec!["docx", "odt", "rtf", "pdf", "html", "txt", "md", "epub"]
    } else {
        // Pure Rust: Markdown and EPUB in, HTML / Markdown / EPUB out.
        match e {
            "md" | "markdown" | "epub" => vec!["html", "md", "epub"],
            _ => Vec::new(),
        }
    }
}

pub fn office_available() -> bool {
    office().is_some()
}

pub fn can_read(e: &str) -> bool {
    matches!(e, "md" | "markdown" | "epub")
        || (TEXTUTIL_IN.contains(&e) && (has_textutil() || office_available()))
        || (e == "pages" && office_available())
}

/// Convert with LibreOffice into `dir`, returning the produced file.
fn soffice(input: &Path, target: &str, dir: &Path) -> Result<PathBuf> {
    soffice_with(input, None, target, dir)
}

/// [`soffice`] with an explicit import filter, e.g. `HTML (StarWriter)` so
/// that HTML opens as a Writer document rather than in Writer/Web.
fn soffice_with(input: &Path, filter: Option<&str>, target: &str, dir: &Path) -> Result<PathBuf> {
    let bin = office().ok_or(Error::MissingTool(
        "LibreOffice",
        if cfg!(windows) {
            "Install it from libreoffice.org or with `winget install TheDocumentFoundation.LibreOffice`."
        } else {
            "Install it from libreoffice.org or with `brew install --cask libreoffice`."
        },
    ))?;
    let profile = format!("-env:UserInstallation=file://{}/lo-profile", dir.display());
    let infilter = filter.map(|f| format!("--infilter={f}"));
    let mut args: Vec<&std::ffi::OsStr> = vec![profile.as_ref(), "--headless".as_ref()];
    if let Some(f) = &infilter {
        args.push(f.as_ref());
    }
    args.extend([
        std::ffi::OsStr::new("--convert-to"),
        target.as_ref(),
        "--outdir".as_ref(),
        dir.as_os_str(),
        input.as_os_str(),
    ]);
    exec(&bin, args)?;
    // `target` may carry a filter, e.g. `txt:Text (encoded):UTF8`.
    let ext = target.split(':').next().unwrap_or(target);
    let produced = dir.join(format!("{}.{ext}", stem(input)));
    if produced.is_file() {
        Ok(produced)
    } else {
        Err(Error::ToolFailed {
            tool: "soffice".into(),
            message: "no output produced".into(),
        })
    }
}

fn textutil(src: &Path, format: &str, out: &Path) -> Result<()> {
    if !has_textutil() {
        return via_office(src, format, out);
    }
    let mut args: Vec<&std::ffi::OsStr> = vec!["-convert".as_ref(), format.as_ref()];
    if matches!(ext(src).as_str(), "txt" | "text") {
        args.extend([
            std::ffi::OsStr::new("-inputencoding"),
            std::ffi::OsStr::new("UTF-8"),
        ]);
    }
    args.extend([
        std::ffi::OsStr::new("-output"),
        out.as_os_str(),
        src.as_os_str(),
    ]);
    exec(TEXTUTIL, args)?;
    Ok(())
}

/// Markdown → HTML fragment, with GitHub-style tables, strikethrough and
/// task lists.
fn markdown_body(md: &str) -> String {
    use pulldown_cmark::{html, Options as MdOptions, Parser};
    let parser = Parser::new_ext(
        md,
        MdOptions::ENABLE_TABLES | MdOptions::ENABLE_STRIKETHROUGH | MdOptions::ENABLE_TASKLISTS,
    );
    let mut body = String::new();
    html::push_html(&mut body, parser);
    // Plain attributes, as LibreOffice and textutil ignore most CSS.
    body.replace(
        "<table>",
        "<table border=\"1\" cellpadding=\"6\" cellspacing=\"0\">",
    )
}

pub fn markdown_to_html(md: &str, title: &str) -> String {
    let body = markdown_body(md);
    format!(
        "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><title>{}</title></head><body>\n{body}</body></html>\n",
        escape(title)
    )
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Turn `input` into something textutil can read, inside `tmp`.
fn readable(input: &Path, tmp: &Path) -> Result<PathBuf> {
    let e = ext(input);
    match e.as_str() {
        "md" | "markdown" => {
            let html = markdown_to_html(&std::fs::read_to_string(input)?, &stem(input));
            let p = tmp.join("source.html");
            std::fs::write(&p, html)?;
            Ok(p)
        }
        "epub" => {
            let p = tmp.join("source.html");
            std::fs::write(&p, epub_to_html(input)?)?;
            Ok(p)
        }
        "pages" | "key" | "pptx" | "ppt" | "odp" => soffice(input, "docx", tmp),
        _ => Ok(input.to_path_buf()),
    }
}

/// What [`textutil`] does, with LibreOffice (Windows).
fn via_office(src: &Path, format: &str, out: &Path) -> Result<()> {
    let tmp = scratch()?;
    let html = matches!(ext(src).as_str(), "html" | "htm");
    let target = match format {
        "txt" => "txt:Text (encoded):UTF8",
        "html" => "html:XHTML Writer File:UTF8",
        f => f,
    };
    let filter = html.then_some("HTML (StarWriter)");
    let produced = soffice_with(src, filter, target, tmp.path())?;
    std::fs::copy(produced, out)?;
    Ok(())
}

/// Straight LibreOffice conversion (e.g. spreadsheet → PDF).
pub fn office_convert(input: &Path, target: &str, opts: &Options) -> Result<PathBuf> {
    let tmp = scratch()?;
    let out = output_path(input, "", target, opts);
    // LibreOffice guesses CSV separators poorly: hand it a workbook instead.
    let src = if matches!(ext(input).as_str(), "csv" | "tsv") {
        let staged = Options {
            out_dir: Some(tmp.path().to_path_buf()),
            ..opts.clone()
        };
        super::sheet::convert(input, "xlsx", &staged)?.remove(0)
    } else {
        input.to_path_buf()
    };
    std::fs::copy(soffice(&src, target, tmp.path())?, &out)?;
    Ok(out)
}

pub fn convert(input: &Path, target: &str, opts: &Options) -> Result<PathBuf> {
    let out = output_path(input, "", target, opts);
    convert_into(input, &stem(input), target, &out)?;
    Ok(out)
}

/// Core of [`convert`], with an explicit destination and title so that
/// intermediate files (e.g. RTF extracted from a PDF) keep the user's name.
fn convert_into(input: &Path, title: &str, target: &str, out: &Path) -> Result<()> {
    let tmp = scratch()?;
    let e = ext(input);
    let is_slides = matches!(e.as_str(), "pptx" | "ppt" | "odp" | "key");
    if is_slides
        || (target == "pdf"
            && office_available()
            && matches!(e.as_str(), "docx" | "doc" | "odt" | "pages"))
    {
        let produced = soffice(input, target, tmp.path())?;
        std::fs::copy(produced, out)?;
        return Ok(());
    }
    let src = readable(input, tmp.path())?;
    // Without macOS's PDF engine, LibreOffice prints every document.
    if target == "pdf" && !cfg!(target_os = "macos") {
        let html = matches!(ext(&src).as_str(), "html" | "htm");
        let produced = soffice_with(&src, html.then_some("HTML (StarWriter)"), "pdf", tmp.path())?;
        std::fs::copy(produced, out)?;
        return Ok(());
    }
    // Writer lays out HTML (lists, tables, images) far better than textutil
    // and CoreText do.
    if office_available()
        && matches!(target, "docx" | "odt" | "pdf")
        && matches!(ext(&src).as_str(), "html" | "htm")
    {
        let produced = soffice_with(&src, Some("HTML (StarWriter)"), target, tmp.path())?;
        std::fs::copy(produced, out)?;
        return Ok(());
    }
    match target {
        // Markdown and EPUB were already turned into clean HTML.
        "html" if src != input && ext(&src) == "html" => {
            std::fs::copy(&src, out)?;
            Ok(())
        }
        "docx" | "odt" | "rtf" | "txt" | "html" | "doc" => textutil(&src, target, out),
        "md" => {
            let md =
                htmd::convert(&to_html(&src, tmp.path())?).map_err(|e| anyhow::anyhow!("{e}"))?;
            std::fs::write(out, md)?;
            Ok(())
        }
        "pdf" => {
            let print = tmp.path().join("print.html");
            std::fs::write(&print, flatten_for_print(&to_html(&src, tmp.path())?))?;
            let rtf = tmp.path().join("layout.rtf");
            textutil(&print, "rtf", &rtf)?;
            super::pdf::from_rtf(&std::fs::read(&rtf)?, out)
        }
        "epub" => {
            let md = if matches!(e.as_str(), "md" | "markdown") {
                std::fs::read_to_string(input)?
            } else {
                htmd::convert(&to_html(&src, tmp.path())?).map_err(|e| anyhow::anyhow!("{e}"))?
            };
            write_epub(title, &md, out)
        }
        other => Err(anyhow::anyhow!("cannot write {other} documents").into()),
    }
}

/// CoreText draws neither list markers nor tables, which AppKit renders
/// itself. Bake them into the text: `• item`, `3. item`, and table rows as
/// tab-separated lines.
fn flatten_for_print(html: &str) -> String {
    let mut out = String::with_capacity(html.len() + 256);
    // One entry per open list: `None` for <ul>, `Some(n)` for <ol>.
    let mut lists: Vec<Option<u32>> = Vec::new();
    let mut first_cell = true;
    let mut rest = html;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        let Some(j) = rest[i..].find('>') else {
            out.push_str(&rest[i..]);
            return out;
        };
        let tag = &rest[i..i + j + 1];
        rest = &rest[i + j + 1..];
        let inner = tag.trim_start_matches('<').trim_end_matches('>');
        let closing = inner.starts_with('/');
        let name: String = inner
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        match (name.as_str(), closing) {
            ("ul", false) => lists.push(None),
            ("ol", false) => {
                let start = attr(inner, "start")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1);
                lists.push(Some(start));
            }
            ("ul" | "ol", true) => {
                lists.pop();
            }
            ("li", false) => {
                let indent = "\u{2003}".repeat(lists.len().saturating_sub(1));
                let marker = match lists.last_mut() {
                    Some(Some(n)) => {
                        *n += 1;
                        format!("{}. ", *n - 1)
                    }
                    _ => "• ".to_string(),
                };
                out.push_str("<p>");
                out.push_str(&indent);
                out.push_str(&marker);
            }
            ("li", true) => out.push_str("</p>"),
            ("table" | "tbody" | "thead" | "tfoot", _) => {}
            ("tr", false) => {
                out.push_str("<p>");
                first_cell = true;
            }
            ("tr", true) => out.push_str("</p>"),
            ("td" | "th", false) => {
                if !first_cell {
                    out.push('\t');
                }
                first_cell = false;
                if name == "th" {
                    out.push_str("<b>");
                }
            }
            ("td" | "th", true) => {
                if name == "th" {
                    out.push_str("</b>");
                }
            }
            _ => out.push_str(tag),
        }
    }
    out.push_str(rest);
    out
}

fn to_html(src: &Path, tmp: &Path) -> Result<String> {
    if matches!(ext(src).as_str(), "html" | "htm") {
        return Ok(std::fs::read_to_string(src)?);
    }
    let html = tmp.join("pivot.html");
    textutil(src, "html", &html)?;
    Ok(std::fs::read_to_string(html)?)
}

/// PDF → text formats. The PDF's text (with styles) is extracted as RTF first.
pub fn from_pdf(input: &Path, target: &str, opts: &Options) -> Result<PathBuf> {
    let out = output_path(input, "", target, opts);
    if target == "txt" {
        std::fs::write(&out, super::pdf::text(input)?)?;
        return Ok(out);
    }
    let tmp = scratch()?;
    let rtf = tmp.path().join(format!("{}.rtf", stem(input)));
    std::fs::write(&rtf, super::pdf::rtf(input)?)?;
    if target == "rtf" {
        std::fs::copy(&rtf, &out)?;
    } else {
        convert_into(&rtf, &stem(input), target, &out)?;
    }
    Ok(out)
}

// --- EPUB -----------------------------------------------------------------

fn attr(tag: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=");
    let mut rest = tag;
    while let Some(i) = rest.find(&needle) {
        let boundary = i == 0 || rest.as_bytes()[i - 1].is_ascii_whitespace();
        let after = &rest[i + needle.len()..];
        if boundary {
            let quote = after.chars().next()?;
            if quote == '"' || quote == '\'' {
                let end = after[1..].find(quote)?;
                return Some(after[1..1 + end].to_string());
            }
        }
        rest = after;
    }
    None
}

fn tags<'a>(xml: &'a str, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    xml.split('<').filter(move |t| {
        t.strip_prefix(name)
            .is_some_and(|r| r.starts_with(|c: char| c.is_whitespace() || c == '/' || c == '>'))
            || t.split_once(':').is_some_and(|(_, local)| {
                local.starts_with(name)
                    && local[name.len()..]
                        .starts_with(|c: char| c.is_whitespace() || c == '/' || c == '>')
            })
    })
}

fn epub_to_html(path: &Path) -> Result<String> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path)?).map_err(anyhow::Error::from)?;
    let mut read = |name: &str| -> Result<String> {
        let mut s = String::new();
        zip.by_name(name)
            .map_err(anyhow::Error::from)?
            .read_to_string(&mut s)?;
        Ok(s)
    };
    let container = read("META-INF/container.xml")?;
    let opf_path = tags(&container, "rootfile")
        .find_map(|t| attr(t, "full-path"))
        .ok_or_else(|| anyhow::anyhow!("invalid EPUB: no rootfile"))?;
    let opf = read(&opf_path)?;
    let base = Path::new(&opf_path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let items: Vec<(String, String)> = tags(&opf, "item")
        .filter_map(|t| Some((attr(t, "id")?, attr(t, "href")?)))
        .collect();
    let mut body = String::new();
    for idref in tags(&opf, "itemref").filter_map(|t| attr(t, "idref")) {
        let Some((_, href)) = items.iter().find(|(id, _)| *id == idref) else {
            continue;
        };
        let full = if base.is_empty() {
            href.clone()
        } else {
            format!("{base}/{href}")
        };
        let Ok(doc) = read(&full) else { continue };
        let start = doc
            .find("<body")
            .and_then(|i| doc[i..].find('>').map(|j| i + j + 1))
            .unwrap_or(0);
        let end = doc.rfind("</body>").unwrap_or(doc.len());
        body.push_str(&doc[start..end.max(start)]);
        body.push('\n');
    }
    Ok(format!("<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"></head><body>\n{body}</body></html>\n"))
}

fn write_epub(title: &str, markdown: &str, out: &Path) -> Result<()> {
    use zip::write::SimpleFileOptions;
    let body = markdown_body(markdown);
    let title = escape(title);
    let id = format!(
        "urn:clementine:{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let chapter = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>{title}</title></head><body>\n{body}</body></html>\n"
    );
    let nav = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\"><head><title>{title}</title></head><body><nav epub:type=\"toc\"><ol><li><a href=\"chapter.xhtml\">{title}</a></li></ol></nav></body></html>\n"
    );
    let opf = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="id">{id}</dc:identifier>
    <dc:title>{title}</dc:title>
    <dc:language>en</dc:language>
    <meta property="dcterms:modified">2000-01-01T00:00:00Z</meta>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="chapter"/></spine>
</package>
"#
    );
    let container = r#"<?xml version="1.0" encoding="utf-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>
"#;
    let mut zip = zip::ZipWriter::new(std::fs::File::create(out)?);
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated =
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let z = |e: zip::result::ZipError| Error::Other(e.into());
    zip.start_file("mimetype", stored).map_err(z)?;
    zip.write_all(b"application/epub+zip")?;
    for (name, content) in [
        ("META-INF/container.xml", container.to_string()),
        ("OEBPS/content.opf", opf),
        ("OEBPS/nav.xhtml", nav),
        ("OEBPS/chapter.xhtml", chapter),
    ] {
        zip.start_file(name, deflated).map_err(z)?;
        zip.write_all(content.as_bytes())?;
    }
    zip.finish().map_err(z)?;
    Ok(())
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn markdown_goes_everywhere() {
        let dir = tempfile::tempdir().unwrap();
        let md = dir.path().join("notes.md");
        std::fs::write(
            &md,
            "# Titre\n\nUn paragraphe avec **du gras** et é à ü.\n\n- un\n- deux\n",
        )
        .unwrap();
        let opts = Options::default();
        for t in ["docx", "odt", "rtf", "txt", "html", "pdf", "epub"] {
            let out = convert(&md, t, &opts).unwrap();
            assert!(std::fs::metadata(&out).unwrap().len() > 0, "{t}");
        }
        let docx = dir.path().join("notes.docx");
        let back = convert(&docx, "md", &opts).unwrap();
        let text = std::fs::read_to_string(back).unwrap();
        assert!(text.contains("gras") && text.contains("é"), "{text}");
        let epub = dir.path().join("notes.epub");
        let txt = convert(&epub, "txt", &opts).unwrap();
        assert!(std::fs::read_to_string(txt).unwrap().contains("paragraphe"));
        let pdf = dir.path().join("notes.pdf");
        assert!(super::super::pdf::text(&pdf)
            .unwrap()
            .contains("paragraphe"));
        let from_pdf = from_pdf(&pdf, "docx", &opts).unwrap();
        assert!(from_pdf.exists());
    }

    #[test]
    fn libreoffice_keeps_tables() {
        if !office_available() {
            eprintln!("LibreOffice not installed, skipping");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let md = dir.path().join("table.md");
        std::fs::write(&md, "# T\n\n| Nom | Âge |\n|---|---|\n| Ada | 36 |\n").unwrap();
        let opts = Options::default();
        let pdf = convert(&md, "pdf", &opts).unwrap();
        let text = super::super::pdf::text(&pdf).unwrap();
        assert!(text.contains("Ada") && !text.contains("|---"), "{text}");
        let csv = dir.path().join("people.csv");
        std::fs::write(&csv, "nom;age\nAda;36\n").unwrap();
        let pdf = office_convert(&csv, "pdf", &opts).unwrap();
        let text = super::super::pdf::text(&pdf).unwrap();
        assert!(text.contains("Ada") && !text.contains(';'), "{text}");
    }

    #[test]
    fn flattens_lists_and_tables() {
        let html = "<ul><li>a</li><li>b<ol start=\"3\"><li>c</li></ol></li></ul><table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2</td></tr></table>";
        let flat = flatten_for_print(html);
        assert!(flat.contains("<p>• a</p>"), "{flat}");
        assert!(flat.contains("\u{2003}3. c"), "{flat}");
        assert!(
            flat.contains("<p><b>A</b>\t<b>B</b></p><p>1\t2</p>"),
            "{flat}"
        );
    }

    #[test]
    fn reads_attributes() {
        assert_eq!(
            attr(r#"item id="a" href='b.xhtml'"#, "href").as_deref(),
            Some("b.xhtml")
        );
        assert_eq!(attr(r#"item xid="a" id="b""#, "id").as_deref(), Some("b"));
    }
}
