//! Read-only database comparison. Identifiers are quoted; the UI never supplies
//! executable SQL. Rows are matched by explicit unique non-null keys.
use crate::files::cancelled;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::AtomicBool;

pub mod mongo_db;
pub mod mysql_db;
pub mod postgres_db;
pub mod sqlite;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DbProfile {
    pub name: String,
    pub engine: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub database: String,
    pub schema: String,
    pub path: String,
    pub tls: bool,
    pub ca_path: String,
    pub auth_database: String,
}
impl Default for DbProfile {
    fn default() -> Self {
        Self {
            name: String::new(),
            engine: "sqlite".into(),
            host: "127.0.0.1".into(),
            port: 3306,
            username: String::new(),
            database: String::new(),
            schema: "public".into(),
            path: String::new(),
            tls: true,
            ca_path: String::new(),
            auth_database: "admin".into(),
        }
    }
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbRequest {
    pub profile: DbProfile,
    pub secret: Option<String>,
    pub relation: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbColumn {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub default_value: Option<String>,
    pub extra: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbObject {
    pub kind: String,
    pub name: String,
    pub definition: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbSchema {
    pub columns: Vec<DbColumn>,
    pub primary_key: Vec<String>,
    pub objects: Vec<DbObject>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbValue {
    pub kind: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
}
pub type Row = BTreeMap<String, DbValue>;
pub type Rows = BTreeMap<String, Row>;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DbOptions {
    pub mode: String,
    pub keys: Vec<String>,
    pub ignore_columns: Vec<String>,
}
impl Default for DbOptions {
    fn default() -> Self {
        Self {
            mode: "both".into(),
            keys: Vec::new(),
            ignore_columns: Vec::new(),
        }
    }
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbDifference {
    pub kind: String,
    pub name: String,
    pub status: String,
    pub left: Option<String>,
    pub right: Option<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbRowDifference {
    pub key: String,
    pub status: String,
    pub columns: Vec<String>,
    pub left: Option<Row>,
    pub right: Option<Row>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbResult {
    pub schema: Vec<DbDifference>,
    pub rows: Vec<DbRowDifference>,
    pub left_rows: u64,
    pub right_rows: u64,
    pub equal_rows: u64,
    pub keys: Vec<String>,
    pub data_compared: bool,
}
pub(crate) fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
pub(crate) fn value(kind: &str, value: impl Into<String>) -> DbValue {
    DbValue {
        kind: kind.into(),
        value: value.into(),
        raw: None,
    }
}
pub(crate) fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 15) as usize] as char)
    }
    out
}
pub(crate) fn quote(name: &str, mark: char) -> Result<String, String> {
    if name.is_empty() || name.contains('\0') {
        return Err("Empty or invalid database identifier.".into());
    }
    Ok(format!(
        "{mark}{}{mark}",
        name.replace(mark, &format!("{mark}{mark}"))
    ))
}
pub(crate) fn key_for(row: &Row, keys: &[String]) -> Result<String, String> {
    let mut values = Vec::new();
    for key in keys {
        let v = row
            .get(key)
            .ok_or_else(|| format!("Key column is missing: {key}"))?;
        if v.kind == "null" {
            return Err(format!("Key column contains NULL: {key}"));
        }
        values.push(v);
    }
    serde_json::to_string(&values).map_err(err)
}
pub(crate) fn insert(
    rows: &mut Rows,
    row: Row,
    keys: &[String],
    ignore: &[String],
) -> Result<(), String> {
    let key = key_for(&row, keys)?;
    let mut row = row;
    row.retain(|k, _| !ignore.contains(k));
    if rows.insert(key.clone(), row).is_some() {
        return Err(format!("Duplicate comparison key: {key}"));
    }
    Ok(())
}
fn check_profile(profile: &DbProfile) -> Result<(), String> {
    if !["sqlite", "mysql", "postgres", "mongodb"].contains(&profile.engine.as_str()) {
        return Err("Unsupported database engine.".into());
    }
    if profile.engine == "sqlite" {
        if profile.path.is_empty() {
            return Err("Choose a SQLite database file.".into());
        }
    } else if profile.host.trim().is_empty()
        || profile.host.contains(['\0', '/', '@', '\\'])
        || profile.port == 0
        || profile.database.is_empty()
    {
        return Err("Enter a database host, port and database name.".into());
    }
    Ok(())
}
pub fn catalog(request: &DbRequest) -> Result<Vec<String>, String> {
    check_profile(&request.profile)?;
    match request.profile.engine.as_str() {
        "sqlite" => sqlite::catalog(request),
        "mysql" => mysql_db::catalog(request),
        "postgres" => postgres_db::catalog(request),
        "mongodb" => mongo_db::catalog(request),
        _ => unreachable!(),
    }
}
pub trait DbReader {
    fn schema(&mut self, relation: &str) -> Result<DbSchema, String>;
    fn rows(
        &mut self,
        relation: &str,
        schema: &DbSchema,
        keys: &[String],
        ignore: &[String],
        cancel: &AtomicBool,
    ) -> Result<Rows, String>;
}
fn connect(request: &DbRequest) -> Result<Box<dyn DbReader>, String> {
    check_profile(&request.profile)?;
    match request.profile.engine.as_str() {
        "sqlite" => Ok(Box::new(sqlite::Reader::open(request)?)),
        "mysql" => Ok(Box::new(mysql_db::Reader::open(request)?)),
        "postgres" => Ok(Box::new(postgres_db::Reader::open(request)?)),
        "mongodb" => Ok(Box::new(mongo_db::Reader::open(request)?)),
        _ => unreachable!(),
    }
}
fn schema_index(schema: &DbSchema) -> Result<BTreeMap<(String, String), String>, String> {
    let mut map = BTreeMap::new();
    for col in &schema.columns {
        if map
            .insert(
                ("column".into(), col.name.clone()),
                serde_json::to_string_pretty(col).map_err(err)?,
            )
            .is_some()
        {
            return Err("Duplicate schema column".into());
        }
    }
    map.insert(
        ("primaryKey".into(), "PRIMARY".into()),
        serde_json::to_string(&schema.primary_key).map_err(err)?,
    );
    // Column order is a structure difference in its own right.
    map.insert(
        ("columnOrder".into(), "COLUMNS".into()),
        serde_json::to_string(&schema.columns.iter().map(|c| &c.name).collect::<Vec<_>>())
            .map_err(err)?,
    );
    for obj in &schema.objects {
        if map
            .insert((obj.kind.clone(), obj.name.clone()), obj.definition.clone())
            .is_some()
        {
            return Err("Duplicate schema object".into());
        }
    }
    Ok(map)
}
fn status<T: PartialEq>(l: Option<&T>, r: Option<&T>) -> String {
    match (l, r) {
        (Some(a), Some(b)) if a == b => "same",
        (Some(_), Some(_)) => "different",
        (Some(_), None) => "leftOnly",
        (None, Some(_)) => "rightOnly",
        (None, None) => "same",
    }
    .into()
}
pub fn compare_schema(left: &DbSchema, right: &DbSchema) -> Result<Vec<DbDifference>, String> {
    let mut a = schema_index(left)?;
    let mut b = schema_index(right)?;
    let keys: BTreeSet<_> = a.keys().chain(b.keys()).cloned().collect();
    Ok(keys
        .into_iter()
        .map(|(kind, name)| {
            let key = (kind.clone(), name.clone());
            let left = a.remove(&key);
            let right = b.remove(&key);
            DbDifference {
                kind,
                name,
                status: status(left.as_ref(), right.as_ref()),
                left,
                right,
            }
        })
        .collect())
}
pub fn compare_rows(
    mut a: Rows,
    mut b: Rows,
    flag: &AtomicBool,
) -> Result<(Vec<DbRowDifference>, u64), String> {
    let keys: BTreeSet<_> = a.keys().chain(b.keys()).cloned().collect();
    let mut differences = Vec::new();
    let mut equal = 0;
    for key in keys {
        cancelled(flag)?;
        let left = a.remove(&key);
        let right = b.remove(&key);
        let state = status(left.as_ref(), right.as_ref());
        if state == "same" {
            equal += 1;
            continue;
        }
        let columns: BTreeSet<String> = left
            .iter()
            .flat_map(|r| r.keys())
            .chain(right.iter().flat_map(|r| r.keys()))
            .cloned()
            .collect();
        let columns = columns
            .into_iter()
            .filter(|c| {
                left.as_ref().and_then(|r| r.get(c)) != right.as_ref().and_then(|r| r.get(c))
            })
            .collect();
        differences.push(DbRowDifference {
            key,
            status: state,
            columns,
            left,
            right,
        });
    }
    Ok((differences, equal))
}
pub fn compare(
    left: DbRequest,
    right: DbRequest,
    options: DbOptions,
    flag: &AtomicBool,
) -> Result<DbResult, String> {
    cancelled(flag)?;
    if left.profile.engine != right.profile.engine {
        return Err(
            "Compare databases of the same engine; cross-engine type conversion is not implicit."
                .into(),
        );
    }
    if left.relation.is_empty() || right.relation.is_empty() {
        return Err("Select two tables or collections.".into());
    }
    if !["schema", "data", "both"].contains(&options.mode.as_str()) {
        return Err("Invalid database comparison mode.".into());
    }
    let mut a = connect(&left)?;
    let mut b = connect(&right)?;
    let sa = a.schema(&left.relation)?;
    let sb = b.schema(&right.relation)?;
    let schema = if options.mode == "data" {
        Vec::new()
    } else {
        compare_schema(&sa, &sb)?
    };
    if options.mode == "schema" {
        return Ok(DbResult {
            schema,
            rows: vec![],
            left_rows: 0,
            right_rows: 0,
            equal_rows: 0,
            keys: vec![],
            data_compared: false,
        });
    }
    let keys = if options.keys.is_empty() {
        if sa.primary_key.is_empty() || sa.primary_key != sb.primary_key {
            return Err("The primary keys do not match. Enter explicit unique key columns.".into());
        }
        sa.primary_key.clone()
    } else {
        options.keys.clone()
    };
    if keys
        .iter()
        .any(|k| k.is_empty() || options.ignore_columns.contains(k))
        || keys.iter().collect::<BTreeSet<_>>().len() != keys.len()
    {
        return Err("Key columns must be nonempty, unique and not ignored.".into());
    }
    if left.profile.engine != "mongodb"
        && keys.iter().any(|k| {
            !sa.columns.iter().any(|c| &c.name == k) || !sb.columns.iter().any(|c| &c.name == k)
        })
    {
        return Err("A comparison key does not exist on both sides.".into());
    }
    let ra = a.rows(&left.relation, &sa, &keys, &options.ignore_columns, flag)?;
    let rb = b.rows(&right.relation, &sb, &keys, &options.ignore_columns, flag)?;
    let left_rows = ra.len() as u64;
    let right_rows = rb.len() as u64;
    let (rows, equal_rows) = compare_rows(ra, rb, flag)?;
    Ok(DbResult {
        schema,
        rows,
        left_rows,
        right_rows,
        equal_rows,
        keys,
        data_compared: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn key_distinguishes_null_numeric_and_string_and_refuses_duplicates() {
        let keys = vec!["id".into()];
        let mut rows = Rows::new();
        let r = |kind: &str, v: &str| BTreeMap::from([("id".into(), value(kind, v))]);
        insert(&mut rows, r("integer", "1"), &keys, &[]).unwrap();
        insert(&mut rows, r("string", "1"), &keys, &[]).unwrap();
        assert!(insert(&mut rows, r("integer", "1"), &keys, &[]).is_err());
        assert!(key_for(&r("null", ""), &keys).is_err());
        assert_eq!(quote("a`b", '`').unwrap(), "`a``b`");
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    #[test]
    fn isolated_database_fixture() {
        let Ok(engine) = std::env::var("DIFF_EX_DB_ENGINE") else {
            return;
        };
        let profile = |database: &str| DbProfile {
            engine: engine.clone(),
            host: "127.0.0.1".into(),
            port: std::env::var("DIFF_EX_DB_PORT").unwrap().parse().unwrap(),
            username: std::env::var("DIFF_EX_DB_USER").unwrap(),
            database: database.into(),
            tls: false,
            auth_database: "admin".into(),
            ..DbProfile::default()
        };
        let request = |database: &str| DbRequest {
            profile: profile(database),
            secret: Some(std::env::var("DIFF_EX_DB_PASSWORD").unwrap()),
            relation: "items".into(),
        };
        let left = request("diff_left");
        let right = request("diff_right");
        assert!(catalog(&left).unwrap().contains(&"items".into()));
        let result = compare(
            left.clone(),
            right.clone(),
            DbOptions::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(result.left_rows, 3);
        assert_eq!(result.right_rows, 3);
        assert_eq!(result.equal_rows, 1);
        assert_eq!(result.rows.len(), 3);
        assert!(result
            .rows
            .iter()
            .any(|row| row.status == "different" && row.columns == vec!["note"]));
        assert!(result.rows.iter().any(|row| row.status == "leftOnly"));
        assert!(result.rows.iter().any(|row| row.status == "rightOnly"));
        assert!(result
            .schema
            .iter()
            .any(|row| row.kind == "index" && row.name == "by_note" && row.status == "leftOnly"));
        let mut opts = DbOptions::default();
        opts.ignore_columns = vec!["note".into()];
        let result = compare(left, right, opts, &AtomicBool::new(false)).unwrap();
        assert_eq!(result.equal_rows, 2);
        assert_eq!(result.rows.len(), 2);
    }
}
