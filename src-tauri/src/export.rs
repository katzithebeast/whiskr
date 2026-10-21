//! Export a recording to Word (.docx) — a minimal, valid OOXML document.

use crate::notes::{fmt_ts, Recording};
use std::io::Write;
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

enum P<'a> {
    Title(&'a str),
    Meta(String),
    H(&'a str),
    Text(String),
    Bullet(String),
    Line { time: String, who: String, text: String },
}

fn para(p: &P) -> String {
    let run = |text: &str, bold: bool, size: u32, color: Option<&str>| {
        format!(
            "<w:r><w:rPr>{}{}<w:sz w:val=\"{size}\"/></w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r>",
            if bold { "<w:b/>" } else { "" },
            color.map(|c| format!("<w:color w:val=\"{c}\"/>")).unwrap_or_default(),
            esc(text)
        )
    };
    let spacing = |after: u32| format!("<w:pPr><w:spacing w:after=\"{after}\"/></w:pPr>");
    match p {
        P::Title(t) => format!("<w:p>{}{}</w:p>", spacing(80), run(t, true, 40, None)),
        P::Meta(t) => format!("<w:p>{}{}</w:p>", spacing(240), run(t, false, 20, Some("6B6F76"))),
        P::H(t) => format!("<w:p><w:pPr><w:spacing w:before=\"280\" w:after=\"100\"/></w:pPr>{}</w:p>", run(t, true, 28, None)),
        P::Text(t) => format!("<w:p>{}{}</w:p>", spacing(120), run(t, false, 22, None)),
        P::Bullet(t) => format!("<w:p><w:pPr><w:spacing w:after=\"60\"/><w:ind w:left=\"360\" w:hanging=\"240\"/></w:pPr>{}</w:p>", run(&format!("•  {t}"), false, 22, None)),
        P::Line { time, who, text } => format!(
            "<w:p>{}{}{}{}</w:p>",
            spacing(100),
            run(&format!("[{time}]  "), false, 18, Some("9CA3AF")),
            run(&format!("{who}: "), true, 22, None),
            run(text, false, 22, None)
        ),
    }
}

pub fn docx(rec: &Recording, dir: &Path) -> Result<PathBuf, String> {
    let mut ps: Vec<P> = vec![P::Title(&rec.title)];
    let mins = (rec.seconds / 60.0).round().max(1.0) as u64;
    ps.push(P::Meta(format!("{} · {} min", rec.created.format("%-d. %-m. %Y %H:%M"), mins)));
    if let Some(s) = &rec.summary {
        if !s.overview.is_empty() {
            ps.push(P::H("Shrnutí"));
            ps.push(P::Text(s.overview.clone()));
        }
        if !s.action_items.is_empty() {
            ps.push(P::H("Úkoly"));
            ps.extend(s.action_items.iter().map(|a| P::Bullet(format!("☐ {a}"))));
        }
        if !s.decisions.is_empty() {
            ps.push(P::H("Rozhodnutí"));
            ps.extend(s.decisions.iter().map(|a| P::Bullet(a.clone())));
        }
        for sec in &s.sections {
            ps.push(P::H(&sec.title));
            ps.extend(sec.items.iter().map(|a| P::Bullet(a.clone())));
        }
        if !s.highlights.is_empty() {
            ps.push(P::H("★ Označené momenty"));
            ps.extend(s.highlights.iter().map(|h| P::Bullet(format!("[{}] {}", fmt_ts(h.time), h.text))));
        }
    }
    if !rec.utterances.is_empty() {
        ps.push(P::H("Přepis"));
        for u in &rec.utterances {
            ps.push(P::Line { time: fmt_ts(u.start), who: rec.name_of(&u.speaker), text: u.text.clone() });
        }
    }
    let body: String = ps.iter().map(para).collect();
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}\
<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1134\" w:right=\"1134\" w:bottom=\"1134\" w:left=\"1134\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/></w:sectPr>\
</w:body></w:document>"
    );
    let content_types = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
</Types>";
    let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>\
</Relationships>";

    let safe: String = rec
        .title
        .chars()
        .map(|c| if c.is_alphanumeric() || " -_.".contains(c) { c } else { '_' })
        .collect::<String>()
        .trim()
        .chars()
        .take(80)
        .collect();
    let path = dir.join(format!("{}.docx", if safe.is_empty() { "prepis".to_string() } else { safe }));
    let file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
    let mut z = zip::ZipWriter::new(file);
    let opt = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in [("[Content_Types].xml", content_types), ("_rels/.rels", rels), ("word/document.xml", document.as_str())] {
        z.start_file(name, opt).map_err(|e| e.to_string())?;
        z.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
    }
    z.finish().map_err(|e| e.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_docx() {
        let dir = std::env::temp_dir().join("whiskr-docx-test");
        let _ = std::fs::create_dir_all(&dir);
        let rec: Recording = serde_json::from_str(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/sample_meta.json")).unwrap()).unwrap();
        let p = docx(&rec, &dir).unwrap();
        assert!(std::fs::metadata(&p).unwrap().len() > 500);
        println!("{}", p.display());
    }
}
