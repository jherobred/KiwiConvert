//! Word documents and plain text: reading DOCX as text or HTML, writing simple DOCX files,
//! and the document conversions built on them.

use super::print::{self, Page};
use super::write_atomic;
use crate::jobs::Span;
use crate::naming;
use crate::settings::Settings;
use anyhow::{Context, Result, bail};
use base64::Engine;
use image::GenericImageView;
use quick_xml::{Reader, XmlVersion};
use quick_xml::events::{BytesStart, Event};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------------------
// Reading DOCX
// ---------------------------------------------------------------------------------------

struct Package {
    zip: zip::ZipArchive<std::fs::File>,
}

impl Package {
    fn open(path: &Path) -> Result<Self> {
        let f = std::fs::File::open(path)?;
        let zip = zip::ZipArchive::new(f).context("This isn't a valid .docx file")?;
        Ok(Self { zip })
    }

    fn read(&mut self, name: &str) -> Option<Vec<u8>> {
        let mut entry = self.zip.by_name(name).ok()?;
        // Guard against zip bombs: a Word part is never this large.
        if entry.size() > 256 * 1024 * 1024 {
            return None;
        }
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf).ok()?;
        Some(buf)
    }
}

/// The text an entity reference such as `&amp;` or `&#233;` stands for. quick-xml reports
/// references as separate events from the surrounding text.
pub fn entity_text(r: &quick_xml::events::BytesRef) -> String {
    if let Ok(Some(c)) = r.resolve_char_ref() {
        return c.to_string();
    }
    match &**r {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        _ => "",
    }
    .to_string()
}

fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == name)
        .and_then(|a| a.normalized_value(XmlVersion::Implicit1_0).ok().map(|v| v.into_owned()))
}

/// `rId` -> target path inside the package (or external URL for hyperlinks).
fn relationships(pkg: &mut Package) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Some(xml) = pkg.read("word/_rels/document.xml.rels") else { return map };
    let mut r = Reader::from_reader(xml.as_slice());
    let mut buf = Vec::new();
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) if e.local_name().as_ref() == "Relationship" => {
                if let (Some(id), Some(target)) = (attr(&e, "Id"), attr(&e, "Target")) {
                    let external = attr(&e, "TargetMode").as_deref() == Some("External");
                    let target = if external || target.starts_with('/') {
                        target.trim_start_matches('/').to_string()
                    } else {
                        format!("word/{target}")
                    };
                    map.insert(id, target);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    map
}

/// styleId -> lowercase style name ("heading 1", "title").
fn style_names(pkg: &mut Package) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Some(xml) = pkg.read("word/styles.xml") else { return map };
    let mut r = Reader::from_reader(xml.as_slice());
    let mut buf = Vec::new();
    let mut current: Option<String> = None;
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if e.local_name().as_ref() == "style" => current = attr(&e, "w:styleId"),
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) if e.local_name().as_ref() == "name" => {
                if let (Some(id), Some(name)) = (current.clone(), attr(&e, "w:val")) {
                    map.insert(id, name.to_ascii_lowercase());
                }
            }
            Ok(Event::End(e)) if e.local_name().as_ref() == "style" => current = None,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    map
}

/// numId -> (level -> is ordered list).
fn numbering(pkg: &mut Package) -> HashMap<String, HashMap<u32, bool>> {
    let mut out = HashMap::new();
    let Some(xml) = pkg.read("word/numbering.xml") else { return out };
    let mut abstract_levels: HashMap<String, HashMap<u32, bool>> = HashMap::new();
    let mut num_to_abstract: HashMap<String, String> = HashMap::new();
    let mut r = Reader::from_reader(xml.as_slice());
    let mut buf = Vec::new();
    let (mut cur_abs, mut cur_lvl, mut cur_num): (Option<String>, Option<u32>, Option<String>) = (None, None, None);
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                "abstractNum" => cur_abs = attr(&e, "w:abstractNumId"),
                "lvl" => cur_lvl = attr(&e, "w:ilvl").and_then(|v| v.parse().ok()),
                "num" => cur_num = attr(&e, "w:numId"),
                _ => {}
            },
            Ok(Event::Empty(e)) => match e.local_name().as_ref() {
                "numFmt" => {
                    if let (Some(a), Some(l)) = (cur_abs.clone(), cur_lvl) {
                        let ordered = !matches!(attr(&e, "w:val").as_deref(), Some("bullet" | "none"));
                        abstract_levels.entry(a).or_default().insert(l, ordered);
                    }
                }
                "abstractNumId" => {
                    if let (Some(n), Some(a)) = (cur_num.clone(), attr(&e, "w:val")) {
                        num_to_abstract.insert(n, a);
                    }
                }
                _ => {}
            },
            Ok(Event::End(e)) => match e.local_name().as_ref() {
                "abstractNum" => cur_abs = None,
                "lvl" => cur_lvl = None,
                "num" => cur_num = None,
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    for (num, abs) in num_to_abstract {
        if let Some(levels) = abstract_levels.get(&abs) {
            out.insert(num, levels.clone());
        }
    }
    out
}

#[derive(Default, Clone)]
struct RunStyle {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    color: Option<String>,
    size_pt: Option<f32>,
    highlight: Option<String>,
    vert: Option<String>,
}

#[derive(Default)]
struct Para {
    heading: u8,
    title: bool,
    align: Option<String>,
    list: Option<(String, u32)>,
    page_break_before: bool,
    html: String,
    text: String,
}

/// A parsed document: paragraphs as HTML blocks and as plain text lines.
pub struct DocxContent {
    pub html: String,
    pub text: String,
    pub title: String,
}

fn highlight_css(name: &str) -> &'static str {
    match name {
        "yellow" => "#ffff00",
        "green" => "#00ff00",
        "cyan" => "#00ffff",
        "magenta" => "#ff00ff",
        "blue" => "#0000ff",
        "red" => "#ff0000",
        "darkBlue" => "#000080",
        "darkCyan" => "#008080",
        "darkGreen" => "#008000",
        "darkMagenta" => "#800080",
        "darkRed" => "#800000",
        "darkYellow" => "#808000",
        "darkGray" => "#808080",
        "lightGray" => "#c0c0c0",
        "black" => "#000000",
        _ => "transparent",
    }
}

pub fn read_docx(path: &Path) -> Result<DocxContent> {
    let mut pkg = Package::open(path)?;
    let rels = relationships(&mut pkg);
    let styles = style_names(&mut pkg);
    let lists = numbering(&mut pkg);
    let xml = pkg.read("word/document.xml").context("This .docx has no document body")?;

    let mut r = Reader::from_reader(xml.as_slice());
    r.config_mut().trim_text(false);
    let mut buf = Vec::new();

    let mut html = String::new();
    let mut text = String::new();
    let mut title = String::new();

    let mut para: Option<Para> = None;
    let mut run = RunStyle::default();
    let mut in_rpr = false;
    let mut in_ppr = false;
    let mut in_text = false;
    let mut link: Option<String> = None;
    let mut table_depth = 0usize;
    let mut cell_text: Vec<String> = Vec::new();
    let mut row_cells: Vec<String> = Vec::new();
    let mut drawing_extent: Option<(i64, i64)> = None;
    let mut open_list: Option<(String, u32, bool)> = None;
    let mut counters: HashMap<(String, u32), u32> = HashMap::new();

    let close_list = |html: &mut String, open_list: &mut Option<(String, u32, bool)>| {
        if let Some((_, _, ordered)) = open_list.take() {
            html.push_str(if ordered { "</ol>" } else { "</ul>" });
        }
    };

    loop {
        let event = match r.read_event_into(&mut buf) {
            Ok(e) => e,
            Err(_) => break,
        };
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let empty = matches!(event, Event::Empty(_));
                match e.local_name().as_ref() {
                    "p" if !empty => {
                        para = Some(Para::default());
                    }
                    "pPr" if !empty => in_ppr = true,
                    "pStyle" if in_ppr => {
                        if let (Some(p), Some(id)) = (para.as_mut(), attr(e, "w:val")) {
                            let name = styles.get(&id).cloned().unwrap_or_else(|| id.to_ascii_lowercase());
                            if let Some(level) = name.strip_prefix("heading ").and_then(|l| l.trim().parse::<u8>().ok()) {
                                p.heading = level.clamp(1, 6);
                            } else if let Some(level) = name.strip_prefix("heading").and_then(|l| l.trim().parse::<u8>().ok()) {
                                p.heading = level.clamp(1, 6);
                            } else if name == "title" {
                                p.title = true;
                            }
                        }
                    }
                    "jc" if in_ppr => {
                        if let Some(p) = para.as_mut() {
                            p.align = attr(e, "w:val").map(|v| match v.as_str() {
                                "both" | "distribute" => "justify".to_string(),
                                "start" => "left".to_string(),
                                "end" => "right".to_string(),
                                other => other.to_string(),
                            });
                        }
                    }
                    "numId" if in_ppr => {
                        if let (Some(p), Some(id)) = (para.as_mut(), attr(e, "w:val")) {
                            if id != "0" {
                                let level = p.list.as_ref().map(|l| l.1).unwrap_or(0);
                                p.list = Some((id, level));
                            }
                        }
                    }
                    "ilvl" if in_ppr => {
                        if let (Some(p), Some(level)) = (para.as_mut(), attr(e, "w:val").and_then(|v| v.parse().ok())) {
                            let id = p.list.as_ref().map(|l| l.0.clone()).unwrap_or_default();
                            p.list = Some((id, level));
                        }
                    }
                    "pageBreakBefore" if in_ppr => {
                        if let Some(p) = para.as_mut() {
                            p.page_break_before = attr(e, "w:val").as_deref() != Some("0");
                        }
                    }
                    "r" if !empty => run = RunStyle::default(),
                    "rPr" if !empty && !in_ppr => in_rpr = true,
                    "b" if in_rpr => run.bold = attr(e, "w:val").as_deref() != Some("0"),
                    "i" if in_rpr => run.italic = attr(e, "w:val").as_deref() != Some("0"),
                    "u" if in_rpr => run.underline = attr(e, "w:val").as_deref() != Some("none"),
                    "strike" if in_rpr => run.strike = attr(e, "w:val").as_deref() != Some("0"),
                    "color" if in_rpr => run.color = attr(e, "w:val").filter(|c| c != "auto"),
                    "sz" if in_rpr => run.size_pt = attr(e, "w:val").and_then(|v| v.parse::<f32>().ok()).map(|h| h / 2.0),
                    "highlight" if in_rpr => run.highlight = attr(e, "w:val"),
                    "vertAlign" if in_rpr => run.vert = attr(e, "w:val"),
                    "t" if !empty => in_text = true,
                    "tab" if !in_ppr => {
                        if let Some(p) = para.as_mut() {
                            p.html.push_str("&emsp;");
                            p.text.push('\t');
                        }
                    }
                    "br" => {
                        if let Some(p) = para.as_mut() {
                            if attr(e, "w:type").as_deref() == Some("page") {
                                p.html.push_str("<span class=\"page-break\"></span>");
                            } else {
                                p.html.push_str("<br>");
                            }
                            p.text.push('\n');
                        }
                    }
                    "hyperlink" if !empty => {
                        link = attr(e, "r:id").and_then(|id| rels.get(&id).cloned()).filter(|t| t.starts_with("http"));
                        if let (Some(p), Some(href)) = (para.as_mut(), link.as_ref()) {
                            p.html.push_str(&format!("<a href=\"{}\">", print::escape(href)));
                        }
                    }
                    "extent" => {
                        let cx = attr(e, "cx").and_then(|v| v.parse().ok()).unwrap_or(0);
                        let cy = attr(e, "cy").and_then(|v| v.parse().ok()).unwrap_or(0);
                        drawing_extent = Some((cx, cy));
                    }
                    "blip" => {
                        let target = attr(e, "r:embed").and_then(|id| rels.get(&id).cloned());
                        if let (Some(p), Some(target)) = (para.as_mut(), target) {
                            if let Some(bytes) = pkg.read(&target) {
                                let mime = match target.rsplit('.').next().map(|s| s.to_ascii_lowercase()).as_deref() {
                                    Some("png") => "image/png",
                                    Some("gif") => "image/gif",
                                    Some("bmp") => "image/bmp",
                                    Some("svg") => "image/svg+xml",
                                    _ => "image/jpeg",
                                };
                                let (w, h) = drawing_extent.map(|(cx, cy)| (cx / 9525, cy / 9525)).unwrap_or((0, 0));
                                let size = if w > 0 { format!(" width=\"{w}\" height=\"{h}\"") } else { String::new() };
                                p.html.push_str(&format!(
                                    "<img src=\"data:{mime};base64,{}\"{size}>",
                                    base64::engine::general_purpose::STANDARD.encode(bytes)
                                ));
                            }
                        }
                    }
                    "tbl" if !empty => {
                        if let Some(p) = para.take() {
                            flush_para(&mut html, &mut text, &mut title, p, &lists, &mut open_list, &mut counters);
                        }
                        close_list(&mut html, &mut open_list);
                        table_depth += 1;
                        if table_depth == 1 {
                            html.push_str("<table>");
                        }
                    }
                    "tr" if !empty && table_depth == 1 => {
                        html.push_str("<tr>");
                        row_cells.clear();
                    }
                    "tc" if !empty && table_depth == 1 => {
                        html.push_str("<td>");
                        cell_text.clear();
                    }
                    _ => {}
                }
            }
            Event::Text(_) | Event::GeneralRef(_) if in_text => {
                let s = match &event {
                    Event::Text(t) => t.xml_content(XmlVersion::Implicit1_0).into_owned(),
                    Event::GeneralRef(r) => entity_text(r),
                    _ => String::new(),
                };
                if let Some(p) = para.as_mut() {
                    let mut css = String::new();
                    if let Some(c) = &run.color {
                        css.push_str(&format!("color:#{c};"));
                    }
                    if let Some(sz) = run.size_pt {
                        css.push_str(&format!("font-size:{sz}pt;"));
                    }
                    if let Some(h) = &run.highlight {
                        css.push_str(&format!("background:{};", highlight_css(h)));
                    }
                    let mut piece = print::escape(&s);
                    if run.bold {
                        piece = format!("<b>{piece}</b>");
                    }
                    if run.italic {
                        piece = format!("<i>{piece}</i>");
                    }
                    if run.underline {
                        piece = format!("<u>{piece}</u>");
                    }
                    if run.strike {
                        piece = format!("<s>{piece}</s>");
                    }
                    match run.vert.as_deref() {
                        Some("superscript") => piece = format!("<sup>{piece}</sup>"),
                        Some("subscript") => piece = format!("<sub>{piece}</sub>"),
                        _ => {}
                    }
                    if !css.is_empty() {
                        piece = format!("<span style=\"{css}\">{piece}</span>");
                    }
                    p.html.push_str(&piece);
                    p.text.push_str(&s);
                }
            }
            Event::End(e) => match e.local_name().as_ref() {
                "t" => in_text = false,
                "rPr" => in_rpr = false,
                "pPr" => in_ppr = false,
                "hyperlink" => {
                    if link.take().is_some() {
                        if let Some(p) = para.as_mut() {
                            p.html.push_str("</a>");
                        }
                    }
                }
                "drawing" => drawing_extent = None,
                "p" => {
                    if let Some(p) = para.take() {
                        if table_depth > 0 {
                            if !cell_text.is_empty() {
                                html.push_str("<br>");
                            }
                            html.push_str(&p.html);
                            cell_text.push(p.text);
                        } else {
                            flush_para(&mut html, &mut text, &mut title, p, &lists, &mut open_list, &mut counters);
                        }
                    }
                }
                "tc" if table_depth == 1 => {
                    html.push_str("</td>");
                    row_cells.push(cell_text.join(" "));
                }
                "tr" if table_depth == 1 => {
                    html.push_str("</tr>");
                    text.push_str(&row_cells.join("\t"));
                    text.push('\n');
                }
                "tbl" => {
                    table_depth = table_depth.saturating_sub(1);
                    if table_depth == 0 {
                        html.push_str("</table>");
                        text.push('\n');
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    close_list(&mut html, &mut open_list);
    Ok(DocxContent { html, text, title })
}

fn flush_para(
    html: &mut String,
    text: &mut String,
    title: &mut String,
    p: Para,
    lists: &HashMap<String, HashMap<u32, bool>>,
    open_list: &mut Option<(String, u32, bool)>,
    counters: &mut HashMap<(String, u32), u32>,
) {
    if p.page_break_before {
        html.push_str("<div class=\"page-break\"></div>");
    }
    let style = p.align.as_ref().map(|a| format!(" style=\"text-align:{a}\"")).unwrap_or_default();
    match &p.list {
        Some((id, level)) => {
            let ordered = lists.get(id).and_then(|l| l.get(level)).copied().unwrap_or(false);
            let same = matches!(open_list, Some((oid, ol, _)) if oid == id && ol == level);
            if !same {
                if let Some((_, _, was_ordered)) = open_list.take() {
                    html.push_str(if was_ordered { "</ol>" } else { "</ul>" });
                }
                let indent = level * 18;
                html.push_str(&if ordered {
                    format!("<ol style=\"margin-left:{indent}pt\">")
                } else {
                    format!("<ul style=\"margin-left:{indent}pt\">")
                });
                *open_list = Some((id.clone(), *level, ordered));
            }
            html.push_str(&format!("<li{style}>{}</li>", p.html));
            let n = counters.entry((id.clone(), *level)).or_insert(0);
            *n += 1;
            let marker = if ordered { format!("{n}. ") } else { "• ".into() };
            text.push_str(&"  ".repeat(*level as usize));
            text.push_str(&marker);
            text.push_str(&p.text);
            text.push('\n');
            return;
        }
        None => {
            if let Some((_, _, was_ordered)) = open_list.take() {
                html.push_str(if was_ordered { "</ol>" } else { "</ul>" });
            }
        }
    }
    let body = if p.html.is_empty() { "&nbsp;".to_string() } else { p.html };
    if p.title {
        if title.is_empty() {
            *title = p.text.clone();
        }
        html.push_str(&format!("<h1{style}>{body}</h1>"));
    } else if p.heading > 0 {
        if title.is_empty() && p.heading == 1 {
            *title = p.text.clone();
        }
        html.push_str(&format!("<h{0}{style}>{body}</h{0}>", p.heading));
    } else {
        html.push_str(&format!("<p{style}>{body}</p>"));
    }
    text.push_str(&p.text);
    text.push('\n');
}

// ---------------------------------------------------------------------------------------
// Writing DOCX
// ---------------------------------------------------------------------------------------

pub enum Block {
    Heading(String),
    Paragraph(String),
    PageBreak,
    Image { bytes: Vec<u8>, ext: &'static str, width: u32, height: u32 },
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // Characters XML 1.0 can't carry.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

fn run_xml(text: &str) -> String {
    // Tabs and line breaks become their own elements.
    let mut out = String::from("<w:r>");
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push_str("<w:br/>");
        }
        for (j, part) in line.split('\t').enumerate() {
            if j > 0 {
                out.push_str("<w:tab/>");
            }
            if !part.is_empty() {
                out.push_str(&format!("<w:t xml:space=\"preserve\">{}</w:t>", xml_escape(part)));
            }
        }
    }
    out.push_str("</w:r>");
    out
}

/// Writes a DOCX with A4 or Letter pages and Word's default styles.
pub fn write_docx(out: &Path, title: &str, blocks: &[Block], letter: bool) -> Result<()> {
    let (page_w, page_h) = if letter { (12240, 15840) } else { (11906, 16838) };
    let margin_twips = 1440;
    let content_emu = ((page_w - 2 * margin_twips) as i64) * 635;

    let mut body = String::new();
    let mut media: Vec<(String, &[u8])> = Vec::new();
    for block in blocks {
        match block {
            Block::Heading(t) => body.push_str(&format!("<w:p><w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr>{}</w:p>", run_xml(t))),
            Block::Paragraph(t) => body.push_str(&format!("<w:p>{}</w:p>", run_xml(t))),
            Block::PageBreak => body.push_str("<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>"),
            Block::Image { bytes, ext, width, height } => {
                let n = media.len() + 1;
                let name = format!("image{n}.{ext}");
                let rid = format!("rIdImg{n}");
                // 96 DPI, shrunk to the text width.
                let mut cx = *width as i64 * 9525;
                let mut cy = *height as i64 * 9525;
                if cx > content_emu {
                    cy = cy * content_emu / cx;
                    cx = content_emu;
                }
                body.push_str(&format!(
                    "<w:p><w:pPr><w:jc w:val=\"center\"/></w:pPr><w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:docPr id=\"{n}\" name=\"Picture {n}\"/><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic><pic:nvPicPr><pic:cNvPr id=\"{n}\" name=\"{name}\"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed=\"{rid}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
                ));
                media.push((name, bytes.as_slice()));
            }
        }
    }

    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><w:body>{body}<w:sectPr><w:pgSz w:w="{page_w}" w:h="{page_h}"/><w:pgMar w:top="{margin_twips}" w:right="{margin_twips}" w:bottom="{margin_twips}" w:left="{margin_twips}" w:header="708" w:footer="708" w:gutter="0"/></w:sectPr></w:body></w:document>"#
    );
    let mut rels = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdStyles" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#);
    for (i, (name, _)) in media.iter().enumerate() {
        rels.push_str(&format!(
            r#"<Relationship Id="rIdImg{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/{name}"/>"#,
            i + 1
        ));
    }
    rels.push_str("</Relationships>");

    let content_types = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Default Extension="jpeg" ContentType="image/jpeg"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/></Types>"#;
    let root_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/></Relationships>"#;
    let styles = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:eastAsia="Calibri" w:cs="Calibri"/><w:sz w:val="22"/><w:szCs w:val="22"/><w:lang w:val="en-US"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before="240" w:after="120"/><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/><w:sz w:val="32"/><w:szCs w:val="32"/></w:rPr></w:style></w:styles>"#;
    let core = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:title>{}</dc:title></cp:coreProperties>"#,
        xml_escape(title)
    );

    let file = std::fs::File::create(out)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in [
        ("[Content_Types].xml", content_types.as_bytes()),
        ("_rels/.rels", root_rels.as_bytes()),
        ("word/document.xml", document.as_bytes()),
        ("word/_rels/document.xml.rels", rels.as_bytes()),
        ("word/styles.xml", styles.as_bytes()),
        ("docProps/core.xml", core.as_bytes()),
    ] {
        zip.start_file(name, opts)?;
        zip.write_all(data)?;
    }
    for (name, data) in media {
        zip.start_file(format!("word/media/{name}"), opts)?;
        zip.write_all(data)?;
    }
    zip.finish()?;
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Conversions
// ---------------------------------------------------------------------------------------

/// Reads a text file as UTF-8, UTF-16 (with BOM), or Windows-1252 as a fallback.
pub fn read_text(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    if bytes.len() > 64 * 1024 * 1024 {
        bail!("This text file is too large to convert (over 64 MB).");
    }
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return Ok(String::from_utf8_lossy(rest).into_owned());
    }
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        let le = bytes[0] == 0xFF;
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) })
            .collect();
        return Ok(String::from_utf16_lossy(&units));
    }
    match String::from_utf8(bytes) {
        Ok(s) => Ok(s),
        Err(e) => Ok(e.into_bytes().iter().map(|b| *b as char).collect()),
    }
}

fn is_markdown(path: &Path) -> bool {
    matches!(crate::registry::ext_of(path).as_str(), "md" | "markdown")
}

/// Minimal Markdown: headings, lists, emphasis, code blocks, and paragraphs.
fn markdown_html(md: &str) -> String {
    let inline = |s: &str| {
        let mut out = print::escape(s);
        // Underscore emphasis is skipped: it would mangle snake_case words.
        for (mark, tag) in [("**", "b"), ("*", "i"), ("`", "code")] {
            let mut result = String::new();
            let mut open = false;
            let mut rest = out.as_str();
            while let Some(i) = rest.find(mark) {
                result.push_str(&rest[..i]);
                let marker = if open { format!("</{tag}>") } else { format!("<{tag}>") };
                result.push_str(&marker);
                open = !open;
                rest = &rest[i + mark.len()..];
            }
            result.push_str(rest);
            if open {
                // Unbalanced marker: leave the text as it was.
                continue;
            }
            out = result;
        }
        out
    };
    let mut html = String::new();
    let mut in_code = false;
    let mut in_list: Option<&str> = None;
    let mut para: Vec<String> = Vec::new();
    let flush = |html: &mut String, para: &mut Vec<String>| {
        if !para.is_empty() {
            html.push_str(&format!("<p>{}</p>", para.join(" ")));
            para.clear();
        }
    };
    for line in md.lines() {
        if line.trim_start().starts_with("```") {
            flush(&mut html, &mut para);
            html.push_str(if in_code { "</pre>" } else { "<pre>" });
            in_code = !in_code;
            continue;
        }
        if in_code {
            html.push_str(&print::escape(line));
            html.push('\n');
            continue;
        }
        let trimmed = line.trim();
        let bullet = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* "));
        let numbered = trimmed.split_once(". ").filter(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
        let list_kind = if bullet.is_some() { Some("ul") } else if numbered.is_some() { Some("ol") } else { None };
        if list_kind != in_list {
            flush(&mut html, &mut para);
            if let Some(k) = in_list {
                html.push_str(&format!("</{k}>"));
            }
            if let Some(k) = list_kind {
                html.push_str(&format!("<{k}>"));
            }
            in_list = list_kind;
        }
        if let Some(item) = bullet.or(numbered.map(|(_, t)| t)) {
            html.push_str(&format!("<li>{}</li>", inline(item)));
            continue;
        }
        if trimmed.is_empty() {
            flush(&mut html, &mut para);
            continue;
        }
        let level = trimmed.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&level) && trimmed[level..].starts_with(' ') {
            flush(&mut html, &mut para);
            html.push_str(&format!("<h{level}>{}</h{level}>", inline(trimmed[level..].trim())));
            continue;
        }
        para.push(inline(trimmed));
    }
    flush(&mut html, &mut para);
    if let Some(k) = in_list {
        html.push_str(&format!("</{k}>"));
    }
    if in_code {
        html.push_str("</pre>");
    }
    html
}

pub fn docx_to_pdf(span: &Span, input: &Path, settings: &Settings, out_dir: Option<&Path>) -> Result<PathBuf> {
    let content = read_docx(input)?;
    span.progress(0.3);
    let html = print::document(&print::escape(&content.title), &content.html, "");
    let out = naming::output_for(input, out_dir, "", "pdf");
    let app = span.ctx().app().clone();
    let page = Page::from_setting(&settings.page_size);
    write_atomic(&out, |tmp| print::html_to_pdf(&app, &html, tmp, page))
}

pub fn docx_to_text(span: &Span, input: &Path, out_dir: Option<&Path>) -> Result<PathBuf> {
    let content = read_docx(input)?;
    span.progress(0.8);
    let out = naming::output_for(input, out_dir, "", "txt");
    write_atomic(&out, |tmp| Ok(std::fs::write(tmp, content.text.replace('\n', "\r\n"))?))
}

pub fn text_to_pdf(span: &Span, input: &Path, settings: &Settings, out_dir: Option<&Path>) -> Result<PathBuf> {
    let text = read_text(input)?;
    span.progress(0.2);
    let title = naming::stem(input);
    let html = if is_markdown(input) {
        print::document(&print::escape(&title), &markdown_html(&text), "")
    } else {
        print::document(&print::escape(&title), &format!("<pre>{}</pre>", print::escape(&text)), "")
    };
    let out = naming::output_for(input, out_dir, "", "pdf");
    let app = span.ctx().app().clone();
    let page = Page::from_setting(&settings.page_size);
    write_atomic(&out, |tmp| print::html_to_pdf(&app, &html, tmp, page))
}

pub fn text_to_docx(span: &Span, input: &Path, settings: &Settings, out_dir: Option<&Path>) -> Result<PathBuf> {
    let text = read_text(input)?;
    span.progress(0.5);
    let markdown = is_markdown(input);
    let blocks: Vec<Block> = text
        .replace("\r\n", "\n")
        .split('\n')
        .map(|line| match line.strip_prefix("# ").filter(|_| markdown) {
            Some(h) => Block::Heading(h.to_string()),
            None => Block::Paragraph(line.to_string()),
        })
        .collect();
    let out = naming::output_for(input, out_dir, "", "docx");
    write_atomic(&out, |tmp| write_docx(tmp, &naming::stem(input), &blocks, settings.page_size == "letter"))
}

/// Joins wrapped lines of a PDF page back into paragraphs.
pub fn reflow(page: &str) -> Vec<String> {
    let lines: Vec<&str> = page.lines().map(str::trim_end).collect();
    let longest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let mut paras = Vec::new();
    let mut cur = String::new();
    for line in lines {
        let t = line.trim();
        if t.is_empty() {
            if !cur.is_empty() {
                paras.push(std::mem::take(&mut cur));
            }
            continue;
        }
        if cur.is_empty() {
            cur.push_str(t);
        } else if cur.ends_with('-') && !cur.ends_with(" -") {
            cur.pop();
            cur.push_str(t);
        } else {
            cur.push(' ');
            cur.push_str(t);
        }
        // A short line usually ends a paragraph.
        let short = (line.chars().count() as f32) < longest as f32 * 0.6;
        if short && (t.ends_with('.') || t.ends_with(':') || t.ends_with('!') || t.ends_with('?')) {
            paras.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        paras.push(cur);
    }
    paras
}

pub fn pdf_to_docx(span: &Span, input: &Path, settings: &Settings, out_dir: Option<&Path>) -> Result<PathBuf> {
    let pages = super::pdf::page_texts(input)?;
    span.progress(0.6);
    if pages.iter().all(|p| p.trim().is_empty()) {
        bail!("This PDF has no selectable text (it may be scanned images). Try PDF to JPG or PNG instead.");
    }
    let mut blocks = Vec::new();
    for (i, page) in pages.iter().enumerate() {
        if i > 0 {
            blocks.push(Block::PageBreak);
        }
        blocks.extend(reflow(&page.replace("\r\n", "\n")).into_iter().map(Block::Paragraph));
    }
    let out = naming::output_for(input, out_dir, "", "docx");
    write_atomic(&out, |tmp| write_docx(tmp, &naming::stem(input), &blocks, settings.page_size == "letter"))
}

pub fn image_to_docx(span: &Span, input: &Path, settings: &Settings, out_dir: Option<&Path>) -> Result<PathBuf> {
    let loaded = super::image::load(input)?;
    span.progress(0.4);
    let (width, height) = loaded.img.dimensions();
    let meta = super::image::Meta { icc: None, exif: None };
    let (bytes, ext) = if loaded.img.color().has_alpha() {
        (super::image::png_bytes(&loaded.img, &meta, false)?, "png")
    } else {
        (super::image::jpeg_bytes(&loaded.img, 92, &meta)?, "jpeg")
    };
    let out = naming::output_for(input, out_dir, "", "docx");
    let blocks = [Block::Image { bytes, ext, width, height }];
    write_atomic(&out, |tmp| write_docx(tmp, &naming::stem(input), &blocks, settings.page_size == "letter"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docx_round_trip() {
        let dir = std::env::temp_dir().join(format!("kiwi-docx-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.docx");
        let blocks = vec![
            Block::Heading("Hello & welcome".into()),
            Block::Paragraph("Line one\twith tab".into()),
            Block::PageBreak,
            Block::Paragraph("Second <page>".into()),
        ];
        write_docx(&path, "T", &blocks, false).unwrap();
        let c = read_docx(&path).unwrap();
        assert!(c.text.contains("Hello & welcome"));
        assert!(c.text.contains("Line one\twith tab"), "{:?}", c.text);
        assert!(c.text.contains("Second <page>"));
        assert!(c.html.contains("<h1>Hello &amp; welcome</h1>"));
        assert!(c.html.contains("Second &lt;page&gt;"));
        assert_eq!(c.title, "Hello & welcome");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reflow_joins_wrapped_lines() {
        let page = "This is a long line of text that wraps\naround to the next line.\n\nNew para-\ngraph here.";
        let p = reflow(page);
        assert_eq!(p[0], "This is a long line of text that wraps around to the next line.");
        assert_eq!(p[1], "New paragraph here.");
    }

    #[test]
    fn markdown_basics() {
        let h = markdown_html("# Title\n\nSome **bold** text\n\n- a\n- b\n");
        assert!(h.contains("<h1>Title</h1>"));
        assert!(h.contains("<b>bold</b>"));
        assert!(h.contains("<ul><li>a</li><li>b</li></ul>"));
    }
}
