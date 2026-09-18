//! `RawRow`: an order-preserving, lossless view of one source row — all 31
//! source columns survive into `raw_data` JSONB (spec IN-8, design D-4).

use serde_json::{Map, Value};

/// The AGESIC source's full column catalog — 31 columns. Used to assert the
/// column-name set (task 47) and to seed synthetic rows in tests.
pub const SOURCE_COLUMNS: [&str; 31] = [
    "id",
    "nombre_tramite",
    "ques_es",
    "dependencia",
    "institucion_nombre",
    "institucion_oid",
    "institucion_padre_organizacional_id",
    "institucion_padre_organizacional_nombre",
    "url",
    "tematica",
    "tematica_especifica",
    "palabras_clave",
    "en_que_consiste",
    "que_necesito_para_hacerlo",
    "que_obtengo",
    "como_y_donde_hacerlo",
    "moneda",
    "valor",
    "tiene_costo",
    "forma_pago",
    "informacion_adicional",
    "requisitos",
    "vigencia",
    "tiempo_estimado",
    "canonical",
    "actualizado",
    "creado",
    "fecha_publicacion",
    "geonumericas",
    "enlace",
    "observaciones",
];

/// One lossless source row: column order preserved, values verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRow {
    values: Vec<(String, String)>,
}

impl RawRow {
    /// Builds a row from ordered (column, value) pairs.
    pub fn new(values: Vec<(String, String)>) -> Self {
        Self { values }
    }

    /// Column names in source order.
    pub fn column_names(&self) -> impl Iterator<Item = &str> {
        self.values.iter().map(|(c, _)| c.as_str())
    }

    /// Verbatim value for a column, if present.
    pub fn get(&self, column: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(c, _)| c == column)
            .map(|(_, v)| v.as_str())
    }

    /// Sets or replaces a column value (test helper / normalization seam).
    pub fn set(&mut self, column: &str, value: &str) {
        if let Some(slot) = self.values.iter_mut().find(|(c, _)| c == column) {
            slot.1 = value.to_string();
        } else {
            self.values.push((column.to_string(), value.to_string()));
        }
    }

    /// The full row as a JSON object — the value destined for
    /// `procedures.raw_data` JSONB (D-4: no source column is dropped, so
    /// parent-organization semantics can be refined later without
    /// re-downloading).
    pub fn to_raw_data_json(&self) -> Value {
        let mut obj = Map::new();
        for (c, v) in &self.values {
            obj.insert(c.clone(), Value::String(v.clone()));
        }
        Value::Object(obj)
    }
}
