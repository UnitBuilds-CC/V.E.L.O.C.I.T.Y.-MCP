//! Native Rust file-to-NDA converters.
//!
//! Ported from C# `Velocity.Core.NdaFileConverter`. Supports:
//! - CSV: Spreadsheet grid with cell triples and visual layout
//! - XLSX: Excel workbook parsing via zip + XML
//! - DOCX: Word document parsing via zip + XML
//! - PDF: Raw text stream extraction
//! - Images (PNG, JPG, WebP): Base64 data URL with DrawImage command
//! - Source code: Syntax-colored code editor layout
//! - Binary: Hex dump viewer with base64 payload recovery
//!
//! Each converter produces an NDA binary document with semantic triples
//! and visual display commands, matching the C# reference implementation.

use crate::nda_document::NdaCompiler;
use std::path::Path;

/// Convert any supported file to NDA binary format.
/// Dispatches to the appropriate converter based on file extension.
pub fn convert_to_nda(file_path: &str) -> Result<Vec<u8>, String> {
    let path = Path::new(file_path);
    if !path.exists() {
        return Err(format!("File not found: {}", file_path));
    }

    let ext = path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "csv" => convert_csv(file_path),
        "xlsx" => convert_xlsx(file_path),
        "docx" => convert_docx(file_path),
        "pdf" => convert_pdf(file_path),
        "png" | "jpg" | "jpeg" | "webp" => convert_image(file_path, None),
        "cs" | "js" | "ts" | "py" | "rs" | "html" | "css" | "json" | "xml"
        | "md" | "sh" | "ps1" | "txt" | "go" | "java" | "cpp" | "c" | "h" | "hpp"
            => convert_code(file_path),
        _ => convert_binary(file_path),
    }
}

// ─── CSV ──────────────────────────────────────────────────────────────────────

fn convert_csv(file_path: &str) -> Result<Vec<u8>, String> {
    let mut compiler = NdaCompiler::new();
    let filename = Path::new(file_path).file_name()
        .and_then(|n| n.to_str()).unwrap_or("unknown.csv");
    let sheet_id = format!("CSV_SHEET_{}", random_hex_id());

    compiler.add_triple(&sheet_id, "TYPE", "SpreadsheetGrid");
    compiler.add_triple(&sheet_id, "FILENAME", filename);

    let content = std::fs::read_to_string(file_path)
        .map_err(|e| format!("Failed to read CSV: {}", e))?;
    let rows: Vec<&str> = content.lines().collect();

    compiler.add_command(1, 0x00E5FFFF, 20, 35, 300, 22, &format!("Spreadsheet: {}", filename));

    let mut current_y: u16 = 80;
    let col_width: u16 = 140;
    let row_height: u16 = 25;

    for r_idx in 0..std::cmp::min(rows.len(), 15) {
        let cells: Vec<&str> = rows[r_idx].split(',').collect();
        compiler.add_command(3, 0x222530FF, 20, current_y.saturating_sub(18), 600, 1, "");

        for c_idx in 0..std::cmp::min(cells.len(), 4) {
            let cell_val = cells[c_idx].trim();
            let cell_id = format!("{}_R{}C{}", sheet_id, r_idx, c_idx);

            compiler.add_triple(&sheet_id, "HAS_CELL", &cell_id);
            compiler.add_triple(&cell_id, "COORDINATE", &format!("{}{}", (b'A' + c_idx as u8) as char, r_idx + 1));
            compiler.add_triple(&cell_id, "VALUE", cell_val);

            let cell_x: u16 = (30 + c_idx as u16 * col_width).min(65535);

            if r_idx == 0 {
                let h = ((rows.len() as u16).min(15)) * row_height;
                compiler.add_command(3, 0x00E5FFFF, cell_x.saturating_sub(5), 60, 1, h, "");
            }

            let color = if r_idx == 0 {
                0xFFFFFFFF
            } else if cell_val.starts_with('-') {
                0xFF5252FF
            } else if cell_val.starts_with('+') {
                0x00E676FF
            } else {
                0xECEFF1FF
            };

            compiler.add_command(1, color, cell_x, current_y, col_width.saturating_sub(10), 16, cell_val);
        }
        current_y = current_y.saturating_add(row_height);
    }

    Ok(compiler.compile())
}

// ─── XLSX ─────────────────────────────────────────────────────────────────────

fn convert_xlsx(file_path: &str) -> Result<Vec<u8>, String> {
    let mut compiler = NdaCompiler::new();
    let filename = Path::new(file_path).file_name()
        .and_then(|n| n.to_str()).unwrap_or("unknown.xlsx");
    let sheet_id = format!("XLSX_SHEET_{}", random_hex_id());

    compiler.add_triple(&sheet_id, "TYPE", "SpreadsheetGrid");
    compiler.add_triple(&sheet_id, "FILENAME", filename);

    let file_bytes = std::fs::read(file_path)
        .map_err(|e| format!("Failed to read XLSX: {}", e))?;

    let (_shared_strings, cells) = parse_xlsx_data(&file_bytes)?;

    compiler.add_command(1, 0x00E5FFFF, 20, 35, 300, 22, &format!("Spreadsheet: {}", filename));

    let col_width: u16 = 140;
    let row_height: u16 = 25;
    let start_y: u16 = 80;

    for r in 0..15u32 {
        let current_y = start_y.saturating_add((r as u16) * row_height);
        compiler.add_command(3, 0x222530FF, 20, current_y.saturating_sub(18), 600, 1, "");

        for c in 0..4u32 {
            let coord = format!("{}{}", (b'A' + c as u8) as char, r + 1);
            let cell_val = cells.get(&coord).cloned().unwrap_or_default();
            if cell_val.is_empty() { continue; }

            let cell_id = format!("{}_{}", sheet_id, coord);
            compiler.add_triple(&sheet_id, "HAS_CELL", &cell_id);
            compiler.add_triple(&cell_id, "COORDINATE", &coord);
            compiler.add_triple(&cell_id, "VALUE", &cell_val);

            let cell_x: u16 = (30 + c as u16 * col_width).min(65535);

            if r == 0 {
                compiler.add_command(3, 0x00E5FFFF, cell_x.saturating_sub(5), 60, 1, (15 * row_height) as u16, "");
            }

            let color = if r == 0 {
                0xFFFFFFFF
            } else if cell_val.starts_with('-') {
                0xFF5252FF
            } else if cell_val.starts_with('+') {
                0x00E676FF
            } else {
                0xECEFF1FF
            };

            compiler.add_command(1, color, cell_x, current_y, col_width.saturating_sub(10), 16, &cell_val);
        }
    }

    Ok(compiler.compile())
}

/// Parse XLSX shared strings and cell values from a zip archive.
fn parse_xlsx_data(data: &[u8]) -> Result<(Vec<String>, std::collections::HashMap<String, String>), String> {
    use std::collections::HashMap;
    use zip::ZipArchive;
    use std::io::Read;

    let cursor = std::io::Cursor::new(data);
    let mut archive = ZipArchive::new(cursor)
        .map_err(|e| format!("Failed to open XLSX zip: {}", e))?;

    let mut shared_strings = Vec::new();
    let mut cells: HashMap<String, String> = HashMap::new();

    // 1. Read shared strings
    if let Ok(mut ss_entry) = archive.by_name("xl/sharedStrings.xml") {
        let mut ss_xml = String::new();
        ss_entry.read_to_string(&mut ss_xml)
            .map_err(|e| format!("Failed to read sharedStrings.xml: {}", e))?;
        shared_strings = extract_xml_texts(&ss_xml, "t");
    }

    // 2. Read sheet1 cells
    if let Ok(mut sheet_entry) = archive.by_name("xl/worksheets/sheet1.xml") {
        let mut sheet_xml = String::new();
        sheet_entry.read_to_string(&mut sheet_xml)
            .map_err(|e| format!("Failed to read sheet1.xml: {}", e))?;
        cells = parse_xlsx_cells(&sheet_xml, &shared_strings);
    }

    Ok((shared_strings, cells))
}

/// Extract text content from XML elements with the given tag name.
/// Uses quick-xml for safe, spec-compliant XML parsing.
fn extract_xml_texts(xml: &str, tag: &str) -> Vec<String> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut results = Vec::new();
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();
    let mut inside_target = false;
    let mut text_buf = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) if e.name().as_ref() == tag.as_bytes() => {
                inside_target = true;
                text_buf.clear();
            }
            Ok(Event::Text(ref e)) if inside_target => {
                let text = std::str::from_utf8(e.as_ref()).unwrap_or("");
                if let Ok(unescaped) = quick_xml::escape::unescape(text) {
                    text_buf.push_str(&unescaped);
                } else {
                    text_buf.push_str(text);
                }
            }
            Ok(Event::End(ref e)) if e.name().as_ref() == tag.as_bytes() && inside_target => {
                results.push(text_buf.clone());
                inside_target = false;
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    results
}

/// Parse XLSX cell elements from sheet XML.
/// Uses quick-xml for safe, spec-compliant XML parsing.
fn parse_xlsx_cells(xml: &str, shared_strings: &[String]) -> std::collections::HashMap<String, String> {
    use quick_xml::Reader;
    use quick_xml::events::Event;
    use std::collections::HashMap;

    let mut cells: HashMap<String, String> = HashMap::new();
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();

    // State tracking for nested parsing
    let mut in_cell = false;
    let mut in_v = false;
    let mut cell_coord = String::new();
    let mut cell_type = String::new();
    let mut cell_value = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = e.name();
                match name.as_ref() {
                    b"c" => {
                        in_cell = true;
                        cell_coord.clear();
                        cell_type.clear();
                        cell_value.clear();
                        // Extract attributes
                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"r" => {
                                    cell_coord = String::from_utf8_lossy(&attr.value).to_string();
                                }
                                b"t" => {
                                    cell_type = String::from_utf8_lossy(&attr.value).to_string();
                                }
                                _ => {}
                            }
                        }
                    }
                    b"v" if in_cell => {
                        in_v = true;
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) if in_v => {
                let text = std::str::from_utf8(e.as_ref()).unwrap_or("");
                if let Ok(unescaped) = quick_xml::escape::unescape(text) {
                    cell_value.push_str(&unescaped);
                } else {
                    cell_value.push_str(text);
                }
            }
            Ok(Event::End(ref e)) => {
                match e.name().as_ref() {
                    b"v" if in_v => {
                        in_v = false;
                    }
                    b"c" if in_cell => {
                        // Process completed cell
                        if !cell_coord.is_empty() && !cell_value.is_empty() {
                            if cell_type == "s" {
                                if let Ok(idx) = cell_value.parse::<usize>() {
                                    if idx < shared_strings.len() {
                                        cells.insert(cell_coord.clone(), shared_strings[idx].clone());
                                    }
                                }
                            } else {
                                cells.insert(cell_coord.clone(), cell_value.clone());
                            }
                        }
                        in_cell = false;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    cells
}

// ─── DOCX ─────────────────────────────────────────────────────────────────────

fn convert_docx(file_path: &str) -> Result<Vec<u8>, String> {
    let mut compiler = NdaCompiler::new();
    let filename = Path::new(file_path).file_name()
        .and_then(|n| n.to_str()).unwrap_or("unknown.docx");
    let doc_id = format!("DOCX_FLOW_{}", random_hex_id());

    compiler.add_triple(&doc_id, "TYPE", "FlowLayoutDocument");
    compiler.add_triple(&doc_id, "FILENAME", filename);

    let file_bytes = std::fs::read(file_path)
        .map_err(|e| format!("Failed to read DOCX: {}", e))?;

    let paragraphs = parse_docx_paragraphs(&file_bytes)?;

    compiler.add_command(1, 0xFFFFFFFF, 40, 40, 400, 25, filename);
    compiler.add_command(3, 0x00E5FFFF, 40, 55, 560, 2, "");

    let mut current_y: u16 = 90;
    for (i, p_text) in paragraphs.iter().enumerate() {
        let p_id = format!("{}_paragraph_{}", doc_id, i);
        compiler.add_triple(&doc_id, "HAS_PARAGRAPH", &p_id);
        compiler.add_triple(&p_id, "INDEX", &i.to_string());
        compiler.add_triple(&p_id, "TEXT", p_text);

        // Simple text wrapping
        let words: Vec<&str> = p_text.split(' ').collect();
        let mut line = String::new();
        for word in &words {
            if (line.len() + 1 + word.len()) * 8 > 520 {
                compiler.add_command(1, 0x90A4AEFF, 50, current_y, 500, 16, line.trim());
                current_y = current_y.saturating_add(22);
                line = word.to_string();
            } else {
                if !line.is_empty() { line.push(' '); }
                line.push_str(word);
            }
        }
        if !line.trim().is_empty() {
            let color = if i == 0 { 0xECEFF1FF } else { 0xB0BEC5FF };
            compiler.add_command(1, color, 50, current_y, 500, 16, line.trim());
            current_y = current_y.saturating_add(32);
        }
    }

    Ok(compiler.compile())
}

/// Parse paragraphs from a DOCX zip archive.
fn parse_docx_paragraphs(data: &[u8]) -> Result<Vec<String>, String> {
    use quick_xml::Reader;
    use quick_xml::events::Event;
    use zip::ZipArchive;
    use std::io::Read;

    let cursor = std::io::Cursor::new(data);
    let mut archive = ZipArchive::new(cursor)
        .map_err(|e| format!("Failed to open DOCX zip: {}", e))?;

    let mut doc_xml = String::new();
    if let Ok(mut entry) = archive.by_name("word/document.xml") {
        entry.read_to_string(&mut doc_xml)
            .map_err(|e| format!("Failed to read document.xml: {}", e))?;
    } else {
        return Err("DOCX does not contain word/document.xml".to_string());
    }

    // Parse paragraphs using quick-xml
    let mut paragraphs = Vec::new();
    let mut reader = Reader::from_str(&doc_xml);
    let mut buf = Vec::new();

    let mut in_paragraph = false;
    let mut in_text = false;
    let mut para_text = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = e.name();
                let local = name.as_ref();
                // Match w:p (paragraph) and w:t (text) with namespace prefix
                if local == b"p" || local.ends_with(b":p") {
                    in_paragraph = true;
                    para_text.clear();
                } else if (local == b"t" || local.ends_with(b":t")) && in_paragraph {
                    in_text = true;
                }
            }
            Ok(Event::Text(ref e)) if in_text => {
                let text = std::str::from_utf8(e.as_ref()).unwrap_or("");
                if let Ok(unescaped) = quick_xml::escape::unescape(text) {
                    para_text.push_str(&unescaped);
                } else {
                    para_text.push_str(text);
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.name();
                let local = name.as_ref();
                if (local == b"t" || local.ends_with(b":t")) && in_text {
                    in_text = false;
                } else if (local == b"p" || local.ends_with(b":p")) && in_paragraph {
                    let trimmed = para_text.trim().to_string();
                    if !trimmed.is_empty() {
                        paragraphs.push(trimmed);
                    }
                    in_paragraph = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(paragraphs)
}

// ─── PDF ──────────────────────────────────────────────────────────────────────

fn convert_pdf(file_path: &str) -> Result<Vec<u8>, String> {
    let mut compiler = NdaCompiler::new();
    let filename = Path::new(file_path).file_name()
        .and_then(|n| n.to_str()).unwrap_or("unknown.pdf");
    let pdf_id = format!("PDF_DOC_{}", random_hex_id());

    compiler.add_triple(&pdf_id, "TYPE", "PdfDocument");
    compiler.add_triple(&pdf_id, "FILENAME", filename);

    let file_bytes = std::fs::read(file_path)
        .map_err(|e| format!("Failed to read PDF: {}", e))?;

    // Try to extract text from PDF using ASCII + regex (same approach as C#)
    let ascii_text = String::from_utf8_lossy(&file_bytes);
    let mut lines = Vec::new();

    if let Ok(re) = regex::Regex::new(r"\(([^)]+)\)\s*Tj") {
        for cap in re.captures_iter(&ascii_text) {
            let text = cap.get(1).map_or("", |m| m.as_str()).trim().to_string();
            if text.len() > 1 && !text.contains('\\') {
                lines.push(text);
            }
        }
    }

    // Fallback if no text found
    if lines.is_empty() {
        lines.push("Scanned PDF File Ingested".to_string());
        lines.push("Direct text stream parsing found zero plain text objects.".to_string());
        lines.push("Document requires OCR/vision model context parsing (MCP).".to_string());
    }

    compiler.add_command(1, 0x00E5FFFF, 40, 40, 400, 25, filename);
    compiler.add_command(3, 0x424242FF, 40, 55, 560, 1, "");

    let mut current_y: u16 = 90;
    for (i, line_text) in lines.iter().take(15).enumerate() {
        let line_id = format!("{}_line_{}", pdf_id, i);
        compiler.add_triple(&pdf_id, "HAS_LINE", &line_id);
        compiler.add_triple(&line_id, "TEXT", line_text);

        let color = if i == 0 {
            0xFFFFFFFF
        } else if line_text.contains("SUCCESSFUL") || line_text.contains("SECURE") {
            0x00E676FF
        } else {
            0xB0BEC5FF
        };

        compiler.add_command(1, color, 60, current_y, 500, 16, line_text);
        current_y = current_y.saturating_add(25);
    }

    compiler.add_command(3, 0xFFFFFF0A, 20, 20, 600, 440, ""); // page border

    Ok(compiler.compile())
}

// ─── Image ────────────────────────────────────────────────────────────────────

fn convert_image(file_path: &str, ocr_text: Option<&str>) -> Result<Vec<u8>, String> {
    let mut compiler = NdaCompiler::new();
    let path = Path::new(file_path);
    let filename = path.file_name()
        .and_then(|n| n.to_str()).unwrap_or("unknown.img");
    let img_id = format!("IMG_ASSET_{}", random_hex_id());

    let ext = path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    compiler.add_triple(&img_id, "TYPE", "ImageDocument");
    compiler.add_triple(&img_id, "FILENAME", filename);
    compiler.add_triple(&img_id, "FORMAT", &ext.to_uppercase());

    let file_bytes = std::fs::read(file_path)
        .map_err(|e| format!("Failed to read image: {}", e))?;

    let mime_type = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    };

    use base64::{Engine as _, engine::general_purpose};
    let base64_data = general_purpose::STANDARD.encode(&file_bytes);
    let data_url = format!("data:{};base64,{}", mime_type, base64_data);

    compiler.add_command(4, 0xFFFFFFFF, 10, 10, 620, 460, &data_url);

    if let Some(ocr) = ocr_text {
        compiler.add_triple(&img_id, "OCR_TEXT", ocr);
        compiler.add_command(3, 0x00E5FF33, 30, 30, 200, 30, "");
        compiler.add_command(1, 0x00E5FFFF, 35, 50, 190, 16, ocr);
    } else {
        compiler.add_triple(&img_id, "AI_OCR_CAPTION", "Image asset compiled into unilateral NDA frame.");
    }

    Ok(compiler.compile())
}

// ─── Source Code ──────────────────────────────────────────────────────────────

fn convert_code(file_path: &str) -> Result<Vec<u8>, String> {
    let mut compiler = NdaCompiler::new();
    let path = Path::new(file_path);
    let filename = path.file_name()
        .and_then(|n| n.to_str()).unwrap_or("unknown.txt");
    let code_id = format!("CODE_ASSET_{}", random_hex_id());

    let ext = path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    compiler.add_triple(&code_id, "TYPE", "SourceCode");
    compiler.add_triple(&code_id, "FILENAME", filename);
    compiler.add_triple(&code_id, "LANGUAGE", &ext.to_uppercase());

    let content = std::fs::read_to_string(file_path)
        .map_err(|e| format!("Failed to read source file: {}", e))?;
    let lines: Vec<&str> = content.lines().collect();

    compiler.add_triple(&code_id, "LINE_COUNT", &lines.len().to_string());

    compiler.add_command(1, 0x00E5FFFF, 30, 35, 400, 22, &format!("Code Editor: {}", filename));
    compiler.add_command(3, 0x0A0B0EFF, 20, 50, 600, 420, ""); // dark bg

    let mut current_y: u16 = 80;
    for (i, line_text) in lines.iter().take(18).enumerate() {
        let line_num = format!("{:>3} |", i + 1);
        compiler.add_command(1, 0x546E7AFF, 30, current_y, 40, 16, &line_num);

        let trimmed = line_text.trim();
        let color = if trimmed.starts_with("//") || trimmed.starts_with('#')
            || trimmed.starts_with("/*") || trimmed.starts_with('*')
        {
            0x00E676FF // comment green
        } else if trimmed.starts_with("using ") || trimmed.starts_with("import ")
            || trimmed.starts_with("namespace ") || trimmed.starts_with("public ")
            || trimmed.starts_with("private ") || trimmed.starts_with("class ")
            || trimmed.starts_with("struct ") || trimmed.starts_with("return ")
            || trimmed.starts_with("void ") || trimmed.starts_with("fn ")
            || trimmed.starts_with("let ") || trimmed.starts_with("const ")
            || trimmed.starts_with("package ")
        {
            0xF48FB1FF // keyword pink
        } else {
            0xECEFF1FF // default white
        };

        compiler.add_command(1, color, 85, current_y, 500, 16, line_text);
        current_y = current_y.saturating_add(20);
    }

    Ok(compiler.compile())
}

// ─── Binary ───────────────────────────────────────────────────────────────────

fn convert_binary(file_path: &str) -> Result<Vec<u8>, String> {
    let mut compiler = NdaCompiler::new();
    let filename = Path::new(file_path).file_name()
        .and_then(|n| n.to_str()).unwrap_or("unknown.bin");
    let bin_id = format!("BIN_ASSET_{}", random_hex_id());

    compiler.add_triple(&bin_id, "TYPE", "BinaryPayload");
    compiler.add_triple(&bin_id, "FILENAME", filename);

    let bytes = std::fs::read(file_path)
        .map_err(|e| format!("Failed to read binary file: {}", e))?;
    compiler.add_triple(&bin_id, "SIZE_BYTES", &bytes.len().to_string());

    use base64::{Engine as _, engine::general_purpose};
    let base64_data = general_purpose::STANDARD.encode(&bytes);
    compiler.add_triple(&bin_id, "BASE64_DATA", &base64_data);

    // Terminal view layout
    compiler.add_command(1, 0x00E5FFFF, 30, 35, 400, 22, &format!("Binary Ingestion: {}", filename));
    compiler.add_command(3, 0x0D0E12FF, 20, 55, 600, 330, ""); // dark terminal bg

    // Hex dump (up to 160 bytes = 10 lines)
    let dump_len = std::cmp::min(bytes.len(), 160);
    let mut current_y: u16 = 90;
    let num_lines = (dump_len as f64 / 16.0).ceil() as usize;

    for l in 0..num_lines {
        let line_offset = l * 16;
        let mut hex_parts = String::new();
        let mut ascii_parts = String::new();

        for i in 0..16 {
            let idx = line_offset + i;
            if idx < bytes.len() {
                hex_parts.push_str(&format!("{:02X} ", bytes[idx]));
                let ch = bytes[idx];
                ascii_parts.push(if ch >= 32 && ch <= 126 { ch as char } else { '.' });
            } else {
                hex_parts.push_str("   ");
            }
        }

        let full_line = format!("{:08X}  {} |{}|", line_offset, hex_parts, ascii_parts);
        compiler.add_command(1, 0x90A4AEFF, 40, current_y, 560, 16, &full_line);
        current_y = current_y.saturating_add(22);
    }

    // Download button
    compiler.add_command(3, 0x00E676FF, 120, 400, 400, 40, "");
    compiler.add_command(1, 0x073B1BFF, 195, 425, 250, 18, "UNPACK & DOWNLOAD ORIGINAL FILE");

    Ok(compiler.compile())
}

// ─── Utilities ────────────────────────────────────────────────────────────────

/// Generate a random 8-character hex ID (matches C# Guid.Substring(0,8)).
fn random_hex_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    // Mix with a simple hash for variety within same second
    let mixed = nanos.wrapping_mul(2654435761);
    format!("{:08X}", mixed).to_uppercase()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nda_document::NdaDocument;

    #[test]
    fn test_convert_csv_round_trip() {
        // Create a temp CSV
        let dir = std::env::temp_dir();
        let csv_path = dir.join("test_velocity_convert.csv");
        std::fs::write(&csv_path, "Name,Value,Status\nAlice,100,OK\nBob,-50,Pending\n").unwrap();

        let result = convert_to_nda(csv_path.to_str().unwrap());
        assert!(result.is_ok(), "CSV conversion should succeed: {:?}", result);
        let nda_data = result.unwrap();

        // Verify it's a valid NDA document
        let doc = NdaDocument::read(&nda_data).unwrap();
        assert!(doc.triples.len() > 0, "Should have triples");
        assert!(doc.commands.len() > 0, "Should have commands");

        // Check for spreadsheet type triple
        let has_type = doc.triples.iter().any(|t| {
            doc.get_string(t.predicate_offset).unwrap_or_default() == "TYPE"
            && doc.get_string(t.object_offset).unwrap_or_default() == "SpreadsheetGrid"
        });
        assert!(has_type, "Should have TYPE=SpreadsheetGrid triple");

        let _ = std::fs::remove_file(&csv_path);
    }

    #[test]
    fn test_convert_code_round_trip() {
        let dir = std::env::temp_dir();
        let code_path = dir.join("test_velocity_convert.rs");
        std::fs::write(&code_path, "fn main() {\n    println!(\"Hello\");\n}\n").unwrap();

        let result = convert_to_nda(code_path.to_str().unwrap());
        assert!(result.is_ok(), "Code conversion should succeed: {:?}", result);
        let nda_data = result.unwrap();

        let doc = NdaDocument::read(&nda_data).unwrap();
        let has_type = doc.triples.iter().any(|t| {
            doc.get_string(t.predicate_offset).unwrap_or_default() == "TYPE"
            && doc.get_string(t.object_offset).unwrap_or_default() == "SourceCode"
        });
        assert!(has_type, "Should have TYPE=SourceCode triple");

        let has_lang = doc.triples.iter().any(|t| {
            doc.get_string(t.predicate_offset).unwrap_or_default() == "LANGUAGE"
            && doc.get_string(t.object_offset).unwrap_or_default() == "RS"
        });
        assert!(has_lang, "Should have LANGUAGE=RS triple");

        let _ = std::fs::remove_file(&code_path);
    }

    #[test]
    fn test_convert_binary_round_trip() {
        let dir = std::env::temp_dir();
        let bin_path = dir.join("test_velocity_convert.dat");
        std::fs::write(&bin_path, &[0x00, 0x01, 0x02, 0xFF, 0xFE, 0x41, 0x42]).unwrap();

        let result = convert_to_nda(bin_path.to_str().unwrap());
        assert!(result.is_ok(), "Binary conversion should succeed: {:?}", result);
        let nda_data = result.unwrap();

        let doc = NdaDocument::read(&nda_data).unwrap();
        let has_type = doc.triples.iter().any(|t| {
            doc.get_string(t.predicate_offset).unwrap_or_default() == "TYPE"
            && doc.get_string(t.object_offset).unwrap_or_default() == "BinaryPayload"
        });
        assert!(has_type, "Should have TYPE=BinaryPayload triple");

        // Should have BASE64_DATA triple
        let has_b64 = doc.triples.iter().any(|t| {
            doc.get_string(t.predicate_offset).unwrap_or_default() == "BASE64_DATA"
        });
        assert!(has_b64, "Should have BASE64_DATA triple");

        let _ = std::fs::remove_file(&bin_path);
    }

    #[test]
    fn test_convert_unknown_extension_falls_back_to_binary() {
        let dir = std::env::temp_dir();
        let path = dir.join("test_velocity_convert.xyz");
        std::fs::write(&path, b"some data").unwrap();

        let result = convert_to_nda(path.to_str().unwrap());
        assert!(result.is_ok());
        let doc = NdaDocument::read(&result.unwrap()).unwrap();
        let has_type = doc.triples.iter().any(|t| {
            doc.get_string(t.object_offset).unwrap_or_default() == "BinaryPayload"
        });
        assert!(has_type);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_missing_file_returns_error() {
        let result = convert_to_nda("/nonexistent/path/file.csv");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }
}
