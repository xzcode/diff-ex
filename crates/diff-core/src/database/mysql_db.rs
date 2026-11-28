use super::*;
use mysql::{prelude::Queryable, Conn, OptsBuilder, SslOpts, Value};
use std::time::Duration;

pub struct Reader {
    conn: Conn,
}
fn connection(request: &DbRequest) -> Result<Conn, String> {
    let p = &request.profile;
    let mut opts = OptsBuilder::default()
        .ip_or_hostname(Some(p.host.clone()))
        .tcp_port(p.port)
        .user(Some(p.username.clone()))
        .pass(request.secret.clone())
        .db_name(Some(p.database.clone()))
        .prefer_socket(false)
        .tcp_connect_timeout(Some(Duration::from_secs(12)))
        .read_timeout(Some(Duration::from_secs(60)))
        .write_timeout(Some(Duration::from_secs(30)));
    if p.tls {
        let mut ssl = SslOpts::default();
        if !p.ca_path.is_empty() {
            ssl = ssl.with_root_cert_path(Some(std::path::PathBuf::from(&p.ca_path)))
        }
        opts = opts.ssl_opts(Some(ssl));
    }
    Conn::new(opts).map_err(err)
}
pub fn catalog(r: &DbRequest) -> Result<Vec<String>, String> {
    connection(r)?.exec_map("SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA=DATABASE() AND TABLE_TYPE='BASE TABLE' ORDER BY TABLE_NAME",(),|name:String|name).map_err(err)
}
impl Reader {
    pub fn open(r: &DbRequest) -> Result<Self, String> {
        let mut conn = connection(r)?;
        conn.query_drop("SET SESSION TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .map_err(err)?;
        conn.query_drop("SET SESSION time_zone = '+00:00'")
            .map_err(err)?;
        conn.query_drop("START TRANSACTION READ ONLY, WITH CONSISTENT SNAPSHOT")
            .map_err(err)?;
        Ok(Self { conn })
    }
}
impl DbReader for Reader {
    fn schema(&mut self, relation: &str) -> Result<DbSchema, String> {
        let table:Option<(Option<String>,Option<String>,String)>=self.conn.exec_first("SELECT ENGINE,TABLE_COLLATION,CREATE_OPTIONS FROM information_schema.TABLES WHERE TABLE_SCHEMA=DATABASE() AND TABLE_NAME=? AND TABLE_TYPE='BASE TABLE'",(relation,)).map_err(err)?;
        let table = table.ok_or("Base table not found.")?;
        let columns=self.conn.exec_map("SELECT COLUMN_NAME,COLUMN_TYPE,IS_NULLABLE,COLUMN_DEFAULT,EXTRA,CHARACTER_SET_NAME,COLLATION_NAME,GENERATION_EXPRESSION FROM information_schema.COLUMNS WHERE TABLE_SCHEMA=DATABASE() AND TABLE_NAME=? ORDER BY ORDINAL_POSITION",(relation,),
            |(name,data_type,nullable,default_value,extra,charset,collation,generation):(String,String,String,Option<String>,String,Option<String>,Option<String>,String)| DbColumn{name,data_type,nullable:nullable=="YES",default_value,extra:serde_json::json!([extra,charset,collation,generation]).to_string()}).map_err(err)?;
        let primary_key=self.conn.exec_map("SELECT COLUMN_NAME FROM information_schema.STATISTICS WHERE TABLE_SCHEMA=DATABASE() AND TABLE_NAME=? AND INDEX_NAME='PRIMARY' ORDER BY SEQ_IN_INDEX",(relation,),|name:String|name).map_err(err)?;
        let mut objects = vec![DbObject {
            kind: "table".into(),
            name: "OPTIONS".into(),
            definition: serde_json::to_string(&table).map_err(err)?,
        }];
        let indices:Vec<(String,u64,u64,Option<String>,Option<String>,Option<u64>,String)>=self.conn.exec("SELECT INDEX_NAME,NON_UNIQUE,SEQ_IN_INDEX,COLUMN_NAME,COLLATION,SUB_PART,INDEX_TYPE FROM information_schema.STATISTICS WHERE TABLE_SCHEMA=DATABASE() AND TABLE_NAME=? ORDER BY INDEX_NAME,SEQ_IN_INDEX",(relation,)).map_err(err)?;
        let mut grouped: BTreeMap<String, Vec<Json>> = BTreeMap::new();
        for (name, non_unique, seq, col, sort, prefix, kind) in indices {
            grouped.entry(name).or_default().push(serde_json::json!([
                non_unique, seq, col, sort, prefix, kind
            ]));
        }
        for (name, items) in grouped {
            objects.push(DbObject {
                kind: "index".into(),
                name,
                definition: serde_json::to_string(&items).map_err(err)?,
            })
        }
        let constraints:Vec<(String,String,Option<String>,Option<String>,Option<String>,u64)>=self.conn.exec("SELECT CONSTRAINT_NAME,COLUMN_NAME,REFERENCED_TABLE_SCHEMA,REFERENCED_TABLE_NAME,REFERENCED_COLUMN_NAME,ORDINAL_POSITION FROM information_schema.KEY_COLUMN_USAGE WHERE TABLE_SCHEMA=DATABASE() AND TABLE_NAME=? ORDER BY CONSTRAINT_NAME,ORDINAL_POSITION",(relation,)).map_err(err)?;
        let mut grouped: BTreeMap<String, Vec<Json>> = BTreeMap::new();
        for (name, col, schema, table, reference, pos) in constraints {
            grouped
                .entry(name)
                .or_default()
                .push(serde_json::json!([pos, col, schema, table, reference]));
        }
        for (name, items) in grouped {
            objects.push(DbObject {
                kind: "constraint".into(),
                name,
                definition: serde_json::to_string(&items).map_err(err)?,
            })
        }
        Ok(DbSchema {
            columns,
            primary_key,
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
                .map(|c| quote(&c.name, '`'))
                .collect::<Result<Vec<_>, _>>()?
                .join(","),
            quote(relation, '`')?
        );
        let query = self.conn.query_iter(sql).map_err(err)?;
        let mut rows = Rows::new();
        for row in query {
            cancelled(cancel)?;
            let raw = row.map_err(err)?.unwrap();
            if raw.len() != schema.columns.len() {
                return Err("Column count changed during comparison.".into());
            }
            let mut result = Row::new();
            for (col, v) in schema.columns.iter().zip(raw) {
                let item = match v {
                    Value::NULL => value("null", ""),
                    Value::Int(v) => value("integer", v.to_string()),
                    Value::UInt(v) => value("unsigned", v.to_string()),
                    Value::Float(v) => value("float32", format!("{:08x}", v.to_bits())),
                    Value::Double(v) => value("float64", format!("{:016x}", v.to_bits())),
                    Value::Bytes(v) => {
                        let kind = col.data_type.to_lowercase();
                        if kind.contains("blob")
                            || kind.contains("binary")
                            || kind.starts_with("bit")
                            || kind.starts_with("geometry")
                        {
                            value("binary", hex(&v))
                        } else if let Ok(text) = std::str::from_utf8(&v) {
                            value("bytesText", text)
                        } else {
                            value("binary", hex(&v))
                        }
                    }
                    Value::Date(y, m, d, h, min, s, us) => value(
                        "date",
                        format!("{y:04}-{m:02}-{d:02} {h:02}:{min:02}:{s:02}.{us:06}"),
                    ),
                    Value::Time(neg, days, h, m, s, us) => value(
                        "time",
                        format!(
                            "{}{} {h:02}:{m:02}:{s:02}.{us:06}",
                            if neg { "-" } else { "+" },
                            days
                        ),
                    ),
                };
                result.insert(col.name.clone(), item);
            }
            insert(&mut rows, result, keys, ignore)?;
        }
        Ok(rows)
    }
}
