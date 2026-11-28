//! Read-only document extraction. No macros, external relationships, or PDF
//! actions are executed. Structural signatures are not rendered equivalence.
use crate::assets::{open_verified, verify_asset, AssetRef};
use crate::files::cancelled;
use serde::Serialize;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocUnit {
    pub label: String,
    pub text: String,
    pub layout: String,
    pub layout_fingerprint: Option<String>,
    pub warning: Option<String>,
    pub page: Option<u32>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentData {
    pub format: String,
    pub units: Vec<DocUnit>,
    pub notes: Vec<String>,
    pub fingerprint: String,
}
fn word_node(node: roxmltree::Node<'_, '_>, name: &str) -> bool {
    node.is_element()
        && node.tag_name().name() == name
        && matches!(
            node.tag_name().namespace(),
            Some(
                "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
                    | "http://purl.oclc.org/ooxml/wordprocessingml/main"
            )
        )
}
fn word_xml(input: &str, label: &str, cancel: &AtomicBool) -> Result<Vec<DocUnit>, String> {
    if input.contains("<!DOCTYPE") {
        return Err("DTD is not allowed in Word XML.".into());
    }
    let doc = roxmltree::Document::parse(input).map_err(|e| format!("{label}: {e}"))?;
    let mut units = Vec::new();
    for para in doc.descendants().filter(|n| word_node(*n, "p")) {
        cancelled(cancel)?;
        let mut text = String::new();
        let mut layout_parts = Vec::new();
        let mut style = String::new();
        for node in para.descendants().filter(|n| n.is_element()) {
            if node != para && node.ancestors().skip(1).find(|n| word_node(*n, "p")) != Some(para) {
                continue;
            }
            if word_node(node, "t") {
                text.push_str(node.text().unwrap_or(""))
            } else if word_node(node, "tab") {
                text.push('\t')
            } else if word_node(node, "br") || word_node(node, "cr") {
                text.push('\n')
            } else if word_node(node, "pStyle") {
                style = node
                    .attributes()
                    .find(|a| a.name() == "val")
                    .map(|a| a.value().to_owned())
                    .unwrap_or_default();
            }
            if word_node(node, "pPr")
                || word_node(node, "rPr")
                || word_node(node, "drawing")
                || word_node(node, "pict")
            {
                layout_parts.push(&input[node.range()]);
            }
        }
        let in_table = para.ancestors().any(|n| word_node(n, "tbl"));
        let layout = format!(
            "{}; paragraph style: {}; property blocks: {}",
            if in_table { "Table cell" } else { "Paragraph" },
            if style.is_empty() { "default" } else { &style },
            layout_parts.len()
        );
        let mut hash = blake3::Hasher::new();
        hash.update(layout.as_bytes());
        for part in layout_parts {
            hash.update(part.as_bytes());
            hash.update(b"\0");
        }
        units.push(DocUnit {
            label: format!("{label} / {}", units.len() + 1),
            text,
            layout,
            layout_fingerprint: Some(hash.finalize().to_hex().to_string()),
            warning: None,
            page: None,
        });
    }
    Ok(units)
}
fn word_text_part(name: &str) -> bool {
    name == "word/document.xml"
        || name == "word/footnotes.xml"
        || name == "word/endnotes.xml"
        || name == "word/comments.xml"
        || (name.starts_with("word/header") || name.starts_with("word/footer"))
            && name.ends_with(".xml")
}
fn word_layout_part(name: &str) -> bool {
    name.starts_with("word/")
        && (name.ends_with(".xml")
            || name.ends_with(".rels")
            || name.starts_with("word/media/")
            || name.starts_with("word/embeddings/"))
}
fn docx(reader: impl Read + std::io::Seek, cancel: &AtomicBool) -> Result<Vec<DocUnit>, String> {
    let mut archive = zip::ZipArchive::new(reader).map_err(|e| e.to_string())?;
    if archive.has_duplicate_names() {
        return Err("Duplicate names in Word package.".into());
    }
    let mut parts = std::collections::BTreeMap::new();
    let mut signatures = std::collections::BTreeMap::new();
    for i in 0..archive.len() {
        cancelled(cancel)?;
        let mut item = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = item.name().map_err(|e| e.to_string())?.into_owned();
        if item.is_dir() {
            continue;
        }
        if word_text_part(&name) {
            let mut xml = String::new();
            item.read_to_string(&mut xml)
                .map_err(|e| format!("{name}: {e}"))?;
            let doc = roxmltree::Document::parse(&xml).map_err(|e| format!("{name}: {e}"))?;
            // Section/page properties, tables and relationship IDs contribute to
            // structure; text runs themselves are compared in separate units.
            let mut hash = blake3::Hasher::new();
            for n in doc.descendants().filter(|n| n.is_element()) {
                if word_node(n, "sectPr")
                    || word_node(n, "tblPr")
                    || word_node(n, "tblGrid")
                    || word_node(n, "tcPr")
                    || word_node(n, "trPr")
                {
                    hash.update(xml[n.range()].as_bytes());
                    hash.update(b"\0");
                }
                if n.tag_name().name() == "blip" {
                    for a in n.attributes() {
                        hash.update(a.name().as_bytes());
                        hash.update(a.value().as_bytes());
                    }
                }
            }
            signatures.insert(name.clone(), hash.finalize().to_hex().to_string());
            let units = word_xml(&xml, &name, cancel)?;
            parts.insert(name, units);
        } else if word_layout_part(&name) {
            let (_, hash) = crate::snapshots::stream_hash(&mut item, cancel)?;
            signatures.insert(name, hash);
        }
    }
    let mut units = parts
        .remove("word/document.xml")
        .ok_or("Missing word/document.xml")?;
    for (_, part) in parts {
        units.extend(part)
    }
    let summary = signatures
        .iter()
        .map(|(p, h)| format!("{p}: {h}"))
        .collect::<Vec<_>>()
        .join("\n");
    units.insert(
        0,
        DocUnit {
            label: "Word package layout and resources".into(),
            text: String::new(),
            layout: summary.clone(),
            layout_fingerprint: Some(blake3::hash(summary.as_bytes()).to_hex().to_string()),
            warning: None,
            page: None,
        },
    );
    Ok(units)
}
fn inherited(doc: &lopdf::Document, mut id: lopdf::ObjectId, key: &[u8]) -> Option<String> {
    let mut seen = std::collections::HashSet::new();
    while seen.insert(id) {
        let dict = doc.get_dictionary(id).ok()?;
        if let Ok(value) = dict.get(key) {
            // Resolve a referenced box before producing its summary.
            let value = if let Ok(reference) = value.as_reference() {
                doc.get_object(reference).ok()?
            } else {
                value
            };
            return Some(format!("{value:?}"));
        }
        id = dict.get(b"Parent").ok()?.as_reference().ok()?;
    }
    None
}
fn pdf(reader: impl Read, cancel: &AtomicBool) -> Result<Vec<DocUnit>, String> {
    let mut bytes = Vec::new();
    let mut reader = reader;
    reader.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    let doc = lopdf::Document::load_mem(&bytes).map_err(|e| e.to_string())?;
    if doc.is_encrypted() {
        return Err("Encrypted PDF is not supported.".into());
    }
    let pages = doc.get_pages();
    if pages.is_empty() {
        return Err("PDF has no readable pages.".into());
    }
    let mut units = Vec::with_capacity(pages.len());
    for (page, id) in pages {
        cancelled(cancel)?;
        let extraction = doc.extract_text(&[page]);
        let (text, warning) = match extraction {
            Ok(text) if text.trim().is_empty() => (
                text,
                Some("No extractable text on this page; it may be scanned or blank.".into()),
            ),
            Ok(text) => (text, None),
            Err(e) => (String::new(), Some(format!("Text extraction failed: {e}"))),
        };
        let mut layout = format!(
            "MediaBox: {}\nCropBox: {}\nRotate: {}",
            inherited(&doc, id, b"MediaBox").unwrap_or_else(|| "unknown".into()),
            inherited(&doc, id, b"CropBox").unwrap_or_else(|| "default".into()),
            inherited(&doc, id, b"Rotate").unwrap_or_else(|| "0".into())
        );
        let signature = match doc.get_page_content_with_limit(id, usize::MAX) {
            Ok(content) => {
                let hash = blake3::hash(&content).to_hex().to_string();
                layout.push_str(&format!("\nDecoded content stream: {hash}"));
                // References and resource changes can alter rendering even
                // when this stream is identical, so call this structural only.
                Some(blake3::hash(layout.as_bytes()).to_hex().to_string())
            }
            Err(e) => {
                layout.push_str(&format!("\nContent stream unavailable: {e}"));
                None
            }
        };
        units.push(DocUnit {
            label: format!("Page {page}"),
            text,
            layout,
            layout_fingerprint: signature,
            warning,
            page: Some(page),
        });
    }
    Ok(units)
}
fn helper(
    command: &str,
    args: &[std::ffi::OsString],
    cancel: &AtomicBool,
) -> Result<Vec<u8>, String> {
    let mut stdout = tempfile::tempfile().map_err(|e| e.to_string())?;
    let mut stderr = tempfile::tempfile().map_err(|e| e.to_string())?;
    let mut child = Command::new(command)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout.try_clone().map_err(|e| e.to_string())?))
        .stderr(Stdio::from(stderr.try_clone().map_err(|e| e.to_string())?))
        .spawn()
        .map_err(|e| format!("{command} is not available. Install the optional local tool: {e}"))?;
    loop {
        if let Err(e) = cancelled(cancel) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => {
                if !status.success() {
                    stderr.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
                    let mut message = String::new();
                    let _ = stderr.read_to_string(&mut message);
                    return Err(format!("{command} failed ({status}): {message}"));
                }
                stdout.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
                let mut bytes = Vec::new();
                stdout.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
                return Ok(bytes);
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}
pub fn load(asset: &AssetRef, cancel: &AtomicBool) -> Result<DocumentData, String> {
    let file = open_verified(asset)?;
    let path = fs::canonicalize(&asset.path).map_err(|e| e.to_string())?;
    let ext = Path::new(&asset.name)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let (format,units,notes)=match ext.as_str(){
        "docx"|"docm"=>{
            ("DOCX",docx(file,cancel)?,vec!["Paragraph and table text plus structural package/property signatures. Not a rendered Word page comparison.".into()])
        },
        "pdf"=>("PDF",pdf(file,cancel)?,vec!["Extracted page text and structural page signatures. Use rendered PDF comparison to verify visual differences; extraction is not OCR.".into()]),
        "doc"=>{
            let args=["-m".into(),"UTF-8.txt".into(),path.as_os_str().to_owned()];
            let bytes=helper("antiword",&args,cancel)?;
            let text=String::from_utf8(bytes).map_err(|_|"Legacy Word helper did not return UTF-8.")?;
            ("DOC",vec![DocUnit{label:"Legacy Word text".into(),text,layout:String::new(),layout_fingerprint:None,warning:Some("Legacy DOC extraction does not include layout.".into()),page:None}],vec!["Legacy .doc text requires the optional local antiword executable.".into()])
        },
        _=>return Err("Unsupported document format. Select DOCX, DOCM, DOC or PDF.".into())
    };
    verify_asset(asset)?;
    let mut file = open_verified(asset)?;
    let (_, fingerprint) = crate::snapshots::stream_hash(&mut file, cancel)?;
    verify_asset(asset)?;
    Ok(DocumentData {
        format: format.into(),
        units,
        notes,
        fingerprint,
    })
}
pub fn render_pdf(
    asset: &AssetRef,
    page: u32,
    dpi: u32,
    cancel: &AtomicBool,
) -> Result<Vec<u8>, String> {
    if page == 0 || !(36..=300).contains(&dpi) {
        return Err("Choose a valid page and render resolution (36–300 DPI).".into());
    }
    let _file = open_verified(asset)?;
    let path = fs::canonicalize(&asset.path).map_err(|e| e.to_string())?;
    if !asset.name.to_ascii_lowercase().ends_with(".pdf") {
        return Err("Only PDF is supported by this page renderer.".into());
    }
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let output = dir.path().join("page");
    let args = [
        "-f".into(),
        page.to_string().into(),
        "-l".into(),
        page.to_string().into(),
        "-r".into(),
        dpi.to_string().into(),
        "-singlefile".into(),
        "-png".into(),
        path.as_os_str().to_owned(),
        output.as_os_str().to_owned(),
    ];
    helper("pdftoppm", &args, cancel)?;
    let bytes = fs::read(output.with_extension("png")).map_err(|e| e.to_string())?;
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("PDF renderer did not produce PNG.".into());
    }
    verify_asset(asset)?;
    Ok(bytes)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;
    #[test]
    fn extracts_word_paragraphs_tables_styles_and_parts() {
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Hello</w:t><w:tab/><w:t>World</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>Cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:sectPr><w:pgSz w:w="100" w:h="200"/></w:sectPr></w:body></w:document>"#;
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("word/document.xml", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
        zip.start_file("word/styles.xml", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"<styles/>").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        let units = docx(Cursor::new(bytes), &AtomicBool::new(false)).unwrap();
        assert_eq!(units.len(), 3);
        assert_eq!(units[1].text, "Hello\tWorld");
        assert_eq!(units[2].text, "Cell");
        assert!(units[1].layout.contains("Heading1"));
        assert!(units[2].layout.contains("Table cell"));
        assert!(units[0].layout.contains("word/styles.xml"));
    }
    #[test]
    fn actual_pdf_page_text_and_geometry() {
        use lopdf::content::{Content, Operation};
        use lopdf::{dictionary, Document, Object, Stream};
        let mut doc = Document::with_version("1.4");
        let pages = doc.new_object_id();
        let font =
            doc.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica"});
        let resources = doc.add_object(dictionary! {"Font"=>dictionary!{"F1"=>font}});
        let content = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 12.into()]),
                Operation::new("Td", vec![10.into(), 20.into()]),
                Operation::new("Tj", vec![Object::string_literal("Diff EX")]),
                Operation::new("ET", vec![]),
            ],
        };
        let stream = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        let page = doc.add_object(dictionary! {"Type"=>"Page","Parent"=>pages,"Contents"=>stream});
        doc.objects.insert(pages,dictionary!{"Type"=>"Pages","Kids"=>vec![page.into()],"Count"=>1,"Resources"=>resources,"MediaBox"=>vec![0.into(),0.into(),200.into(),300.into()]}.into());
        let catalog = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        let units = pdf(Cursor::new(bytes), &AtomicBool::new(false)).unwrap();
        assert_eq!(units.len(), 1);
        assert!(units[0].text.contains("Diff EX"));
        assert!(units[0].layout.contains("MediaBox"));
    }
}
