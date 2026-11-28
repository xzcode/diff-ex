use super::*;
use rusqlite::{types::ValueRef, Connection, OpenFlags};
use std::time::Duration;
pub struct Reader {
    conn: Connection,
}
impl Reader {
    pub fn open(r: &DbRequest) -> Result<Self, String> {
        let conn = Connection::open_with_flags(
            &r.profile.path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(err)?;
        conn.busy_timeout(Duration::from_secs(15)).map_err(err)?;
        conn.execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF; BEGIN;")
            .map_err(err)?;
        Ok(Self { conn })
    }
}
pub fn catalog(r: &DbRequest) -> Result<Vec<String>, String> {
    let reader = Reader::open(r)?;
    let mut stmt=reader.conn.prepare("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' AND sql NOT LIKE 'CREATE VIRTUAL TABLE%' ORDER BY name").map_err(err)?;
    let rows = stmt
        .query_map([], |r| r.get(0))
        .map_err(err)?
        .collect::<Result<Vec<String>, _>>()
        .map_err(err)?;
    Ok(rows)
}
impl DbReader for Reader {
    fn schema(&mut self, relation: &str) -> Result<DbSchema, String> {
        let definition: String = self
            .conn
            .query_row(
                "SELECT sql FROM sqlite_schema WHERE type='table' AND name=?1",
                [relation],
                |r| r.get(0),
            )
            .map_err(err)?;
        if definition
            .trim_start()
            .to_uppercase()
            .starts_with("CREATE VIRTUAL TABLE")
        {
            return Err("Virtual tables are not supported.".into());
        }
        let mut stmt = self
            .conn
            .prepare(
                "SELECT name,type,\"notnull\",dflt_value,pk,hidden FROM pragma_table_xinfo(?1)",
            )
            .map_err(err)?;
        let mut q = stmt.query([relation]).map_err(err)?;
        let mut columns = Vec::new();
        let mut keys = Vec::new();
        while let Some(row) = q.next().map_err(err)? {
            let name: String = row.get(0).map_err(err)?;
            let pk: i64 = row.get(4).map_err(err)?;
            if pk > 0 {
                keys.push((pk, name.clone()))
            }
            columns.push(DbColumn {
                name,
                data_type: row.get(1).map_err(err)?,
                nullable: row.get::<_, i64>(2).map_err(err)? == 0,
                default_value: row.get(3).map_err(err)?,
                extra: format!("hidden={}", row.get::<_, i64>(5).map_err(err)?),
            });
        }
        keys.sort_by_key(|v| v.0);
        drop(q);
        drop(stmt);
        let mut stmt=self.conn.prepare("SELECT type,name,coalesce(sql,'') FROM sqlite_schema WHERE tbl_name=?1 AND type IN ('index','trigger') ORDER BY type,name").map_err(err)?;
        let objects = stmt
            .query_map([relation], |r| {
                Ok(DbObject {
                    kind: r.get(0)?,
                    name: r.get(1)?,
                    definition: r.get(2)?,
                })
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        let mut objects = objects;
        objects.push(DbObject {
            kind: "table".into(),
            name: "DEFINITION".into(),
            definition,
        });
        Ok(DbSchema {
            columns,
            primary_key: keys.into_iter().map(|(_, n)| n).collect(),
            objects,
        })
    }
    fn rows(
        &mut self,
        relation: &str,
        schema: &DbSchema,
        keys: &[String],
        ignore: &[String],
        cancel: &AtomicBool,
    ) -> Result<Rows, String> {
        let sql = format!(
            "SELECT {} FROM {}",
            schema
                .columns
                .iter()
                .map(|c| quote(&c.name, '"'))
                .collect::<Result<Vec<_>, _>>()?
                .join(","),
            quote(relation, '"')?
        );
        let mut stmt = self.conn.prepare(&sql).map_err(err)?;
        let mut query = stmt.query([]).map_err(err)?;
        let mut rows = Rows::new();
        while let Some(row) = query.next().map_err(err)? {
            cancelled(cancel)?;
            let mut result = Row::new();
            for (i, col) in schema.columns.iter().enumerate() {
                let v = match row.get_ref(i).map_err(err)? {
                    ValueRef::Null => value("null", ""),
                    ValueRef::Integer(n) => value("integer", n.to_string()),
                    ValueRef::Real(n) => value("real", format!("{:016x}", n.to_bits())),
                    ValueRef::Text(v) => match std::str::from_utf8(v) {
                        Ok(s) => value("string", s),
                        Err(_) => value("textBytes", hex(v)),
                    },
                    ValueRef::Blob(v) => value("binary", hex(v)),
                };
                result.insert(col.name.clone(), v);
            }
            insert(&mut rows, result, keys, ignore)?;
        }
        Ok(rows)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_sqlite_schema_and_composite_key_records() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.db");
        let b = dir.path().join("b.db");
        for (path, data) in [
            (&a, "INSERT INTO items VALUES(1,'a','old'),(2,'b',NULL)"),
            (&b, "INSERT INTO items VALUES(1,'a','new'),(3,'c','added')"),
        ] {
            let c = Connection::open(path).unwrap();
            c.execute_batch("CREATE TABLE items (id INTEGER NOT NULL,sub TEXT NOT NULL,note TEXT,PRIMARY KEY(id,sub)); CREATE INDEX by_note ON items(note);").unwrap();
            c.execute_batch(data).unwrap();
        }
        let request = |p: &std::path::Path| DbRequest {
            profile: DbProfile {
                path: p.to_str().unwrap().into(),
                ..DbProfile::default()
            },
            relation: "items".into(),
            secret: None,
        };
        let r = compare(
            request(&a),
            request(&b),
            DbOptions::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(r.keys, vec!["id", "sub"]);
        assert_eq!(r.rows.len(), 3);
        assert!(r
            .schema
            .iter()
            .any(|e| e.kind == "index" && e.name == "by_note"));
        assert!(r.schema.iter().all(|e| e.status == "same"));
        assert!(r
            .rows
            .iter()
            .any(|r| r.status == "different" && r.columns == vec!["note"]));
        let reader = Reader::open(&request(&a)).unwrap();
        assert!(reader.conn.execute("DELETE FROM items", []).is_err());
    }
}
