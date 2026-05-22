//! PDF text extraction for `read_file`.
//!
//! Uses `pdf-extract` to pull plain text out of PDFs. The library is best-effort
//! for scanned PDFs (no OCR) — for those we return whatever metadata we can
//! get with a clear note that the document looks scanned.
//!
//! Pagination via `offset`/`limit` happens at the line level after extraction.

use std::path::Path;

const SCANNED_PDF_HINT: &str =
    "[no extractable text — this looks like a scanned PDF. OCR is not built in; \
     run the file through an OCR tool first.]";

pub fn render(
    path: &Path,
    display_path: &str,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<String, anyhow::Error> {
    let bytes = std::fs::read(path)?;
    let text = pdf_extract::extract_text_from_mem(&bytes)
        .map_err(|e| anyhow::anyhow!("pdf parse failed: {e}"))?;

    let trimmed_text = text.trim();
    if trimmed_text.is_empty() {
        return Ok(format!(
            "PDF: {display_path}\n{} bytes, no text extracted.\n\n{}\n",
            bytes.len(),
            SCANNED_PDF_HINT
        ));
    }

    let lines: Vec<&str> = text.lines().collect();
    let start = offset.unwrap_or(0).min(lines.len());
    let end = limit.map_or(lines.len(), |l| (start + l).min(lines.len()));
    let selected = &lines[start..end];

    let mut out = format!(
        "PDF: {display_path}\n{} bytes, {} lines extracted.\n\n",
        bytes.len(),
        lines.len()
    );
    for (i, line) in selected.iter().enumerate() {
        out.push_str(&format!("{:>6} | {}\n", start + i + 1, line));
    }
    if end < lines.len() {
        out.push_str(&format!(
            "\n... ({} more lines, {} total)\n",
            lines.len() - end,
            lines.len()
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_pdf_errors() {
        let r = render(
            std::path::Path::new("/nonexistent/file.pdf"),
            "nope.pdf",
            None,
            None,
        );
        assert!(r.is_err());
    }
}
