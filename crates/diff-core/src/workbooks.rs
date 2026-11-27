//! Read spreadsheet cell values; no macro execution or workbook writes.
use crate::assets::{open_verified, verify_asset, AssetRef};
use calamine::{open_workbook_from_rs, Data, Ods, Reader, Sheets, Xls, Xlsb, Xlsx};
use serde::Serialize;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableCell {
    pub value: String,
    pub kind: &'static str,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableSheet {
    pub row_start: u32,
    pub col_start: u32,
    pub rows: Vec<Vec<Option<TableCell>>>,
}
fn workbook(asset: &AssetRef) -> Result<Sheets<BufReader<File>>, String> {
    let reader = BufReader::new(open_verified(asset)?);
    let ext = Path::new(&asset.name)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "xls" | "xla" => open_workbook_from_rs::<Xls<_>, _>(reader)
            .map(Sheets::Xls)
            .map_err(|e| e.to_string()),
        "xlsx" | "xlsm" | "xlam" => open_workbook_from_rs::<Xlsx<_>, _>(reader)
            .map(Sheets::Xlsx)
            .map_err(|e| e.to_string()),
        "xlsb" => open_workbook_from_rs::<Xlsb<_>, _>(reader)
            .map(Sheets::Xlsb)
            .map_err(|e| e.to_string()),
        "ods" => open_workbook_from_rs::<Ods<_>, _>(reader)
            .map(Sheets::Ods)
            .map_err(|e| e.to_string()),
        _ => Err("Unsupported spreadsheet format.".into()),
    }
}
pub fn sheet_names(asset: &AssetRef) -> Result<Vec<String>, String> {
    let names = workbook(asset)?.sheet_names();
    verify_asset(asset)?;
    Ok(names)
}
fn cell(value: &Data) -> Option<TableCell> {
    let kind = match value {
        Data::Empty => return None,
        Data::Int(_) | Data::Float(_) => "number",
        Data::String(_) => "string",
        Data::Bool(_) => "boolean",
        Data::DateTime(_) | Data::DateTimeIso(_) => "datetime",
        Data::DurationIso(_) => "duration",
        Data::Error(_) => "error",
    };
    // Retain serial date system and date/duration interpretation.
    let value = match value {
        Data::DateTime(date) => format!("{date:?}"),
        other => other.to_string(),
    };
    Some(TableCell { value, kind })
}
pub fn read_sheet(asset: &AssetRef, sheet: &str) -> Result<TableSheet, String> {
    let mut book = workbook(asset)?;
    let range = book.worksheet_range(sheet).map_err(|e| e.to_string())?;
    let (row_start, col_start) = range.start().unwrap_or((0, 0));
    let rows = range
        .rows()
        .map(|row| row.iter().map(cell).collect())
        .collect();
    verify_asset(asset)?;
    Ok(TableSheet {
        row_start,
        col_start,
        rows,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_cells_preserve_text_and_empty_value_distinctions() {
        assert!(cell(&Data::Empty).is_none());
        let a = cell(&Data::String("0001".into())).unwrap();
        assert_eq!(a.kind, "string");
        assert_eq!(a.value, "0001");
        let b = cell(&Data::Int(1)).unwrap();
        assert_eq!(b.kind, "number");
        assert_eq!(b.value, "1");
    }
    #[test]
    fn read_actual_xlsx_fixture() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sample.xlsx");
        let asset = crate::assets::inspect_asset(path).unwrap();
        assert_eq!(
            sheet_names(&asset).unwrap(),
            vec!["Data".to_owned(), "Other".to_owned()]
        );
        let sheet = read_sheet(&asset, "Data").unwrap();
        assert_eq!((sheet.row_start, sheet.col_start), (1, 1));
        assert_eq!(sheet.rows[0][0].as_ref().unwrap().value, "id");
        assert_eq!(sheet.rows[1][0].as_ref().unwrap().value, "0001");
        assert_eq!(sheet.rows[1][1].as_ref().unwrap().kind, "number");
        assert!(read_sheet(&asset, "missing").is_err());
    }
}
