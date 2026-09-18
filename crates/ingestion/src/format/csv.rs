//! CSV `FormatStrategy`: UTF-8, comma delimiter, standard double-quote
//! quoting, embedded newlines inside quoted fields (spec IN-3). The `csv`
//! crate provides RFC 4180 framing; naive line splitting is never used.

use crate::error::ParseError;
use crate::ports::FormatStrategy;
use crate::row::RawRow;

/// CSV strategy using the `csv` crate with RFC 4180 defaults.
#[derive(Debug, Clone, Copy, Default)]
pub struct CsvStrategy;

impl FormatStrategy for CsvStrategy {
    fn parse(&self, bytes: &[u8]) -> Result<Vec<RawRow>, ParseError> {
        let text = std::str::from_utf8(bytes)
            .map_err(|e| ParseError::Malformed(format!("payload is not UTF-8: {e}")))?;

        let mut reader = csv::ReaderBuilder::new()
            .has_headers(true)
            .flexible(false)
            .from_reader(text.as_bytes());

        let headers = reader
            .headers()
            .map_err(|e| ParseError::Malformed(format!("missing CSV header row: {e}")))?
            .clone();

        let mut rows = Vec::new();
        for record in reader.records() {
            let record =
                record.map_err(|e| ParseError::Malformed(format!("bad CSV record: {e}")))?;
            let values: Vec<(String, String)> = headers
                .iter()
                .zip(record.iter())
                .map(|(h, v)| (h.to_string(), v.to_string()))
                .collect();
            rows.push(RawRow::new(values));
        }
        Ok(rows)
    }
}
