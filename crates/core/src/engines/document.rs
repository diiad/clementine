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

pub fn office_available() -> bool {
    office().is_some()
}

pub fn can_read(e: &str) -> bool {
    TEXTUTIL_IN.contains(&e)
        || matches!(e, "md" | "markdown" | "epub")
        || (e == "pages" && office_available())
}

/// Convert with LibreOffice into `dir`, returning the produced file.
fn soffice(input: &Path, target: &str, dir: &Path) -> Result<PathBuf> {
    let bin = office().ok_or(Error::MissingTool(
        "LibreOffice",
        "Install it from libreoffice.org or with `brew install --cask libreoffice`.",
    ))?;
    let profile = format!("-env:UserInstallation=file://{}/lo-profile", dir.display());
    exec(
        &bin,
        [
            profile.as_ref(),
            "--headless".as_ref(),
            "--convert-to".as_ref(),
            target.as_ref(),
            "--outdir".as_ref(),
            dir.as_os_str(),
            input.as_os_str(),
        ],
    )?;
    let produced = dir.join(format!("{}.{target}", stem(input)));
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

pub fn markdown_to_html(md: &str, title: &str) -> String {
    use pulldown_cmark::{html, Options as MdOptions, Parser};
    let parser = Parser::new_ext(
        md,
        MdOptions::ENABLE_TABLES | MdOptions::ENABLE_STRIKETHROUGH | MdOptions::ENABLE_TASKLISTS,
    );
    let mut body = String::new();
    html::push_html(&mut body, parser);
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
            let rtf = tmp.path().join("layout.rtf");
            textutil(&src, "rtf", &rtf)?;
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
    use pulldown_cmark::{html, Parser};
    use zip::write::SimpleFileOptions;
    let mut body = String::new();
    html::push_html(&mut body, Parser::new(markdown));
    let title = escape(title);
    let id = format!(
        "urn:satsuma:{:x}",
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
    fn reads_attributes() {
        assert_eq!(
            attr(r#"item id="a" href='b.xhtml'"#, "href").as_deref(),
            Some("b.xhtml")
        );
        assert_eq!(attr(r#"item xid="a" id="b""#, "id").as_deref(), Some("b"));
    }
}
