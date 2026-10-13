//! Import/backup/Excel — port of `importjob` + `cloudbackup`.
//! Excel via calamine (read) / rust_xlsxwriter (write).
use calamine::{Reader, Xlsx, open_workbook_from_rs};
use rust_xlsxwriter::Workbook;
use std::io::Cursor;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ImportError {
    #[error("excel read: {0}")]
    Read(String),
    #[error("excel write: {0}")]
    Write(String),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sheet {
    pub name: String,
    pub rows: Vec<Vec<String>>,
}

/// Write a workbook (one sheet) → xlsx bytes.
pub fn write_xlsx(sheet: &Sheet) -> Result<Vec<u8>, ImportError> {
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name(&sheet.name).map_err(|e| ImportError::Write(e.to_string()))?;
    for (r, row) in sheet.rows.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            ws.write_string(r as u32, c as u16, cell).map_err(|e| ImportError::Write(e.to_string()))?;
        }
    }
    wb.save_to_buffer().map_err(|e| ImportError::Write(e.to_string()))
}

/// Read first sheet of xlsx bytes → Sheet.
pub fn read_xlsx(bytes: &[u8]) -> Result<Sheet, ImportError> {
    let cursor = Cursor::new(bytes.to_vec());
    let mut wb: Xlsx<_> = open_workbook_from_rs::<Xlsx<_>, _>(cursor).map_err(|e| ImportError::Read(e.to_string()))?;
    let name = wb.sheet_names().first().cloned().unwrap_or_default();
    let range = wb.worksheet_range(&name).map_err(|e| ImportError::Read(e.to_string()))?;
    let rows = range.rows().map(|r| r.iter().map(|c| c.to_string()).collect()).collect();
    Ok(Sheet { name, rows })
}

/// Import rows: map each row through `f`, collecting outputs.
pub fn import_rows<T, F>(rows: &[Vec<String>], mut f: F) -> Vec<T>
where
    F: FnMut(&[String]) -> Option<T>,
{
    rows.iter().filter_map(|r| f(r)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excel_round_trip() {
        let sheet = Sheet {
            name: "data".into(),
            rows: vec![vec!["a".into(), "b".into()], vec!["1".into(), "2".into()]],
        };
        let bytes = write_xlsx(&sheet).expect("write");
        let back = read_xlsx(&bytes).expect("read");
        assert_eq!(back, sheet);
    }

    #[test]
    fn import_rows_skips_none() {
        let rows = vec![vec!["1".into()], vec!["x".into()]];
        let out: Vec<i64> = import_rows(&rows, |r| r[0].parse().ok());
        assert_eq!(out, vec![1]);
    }
}
