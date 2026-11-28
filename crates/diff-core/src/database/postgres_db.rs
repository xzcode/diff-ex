use super::*;
use postgres::{Client,Config,NoTls,config::SslMode};
use postgres_native_tls::MakeTlsConnector;
use std::time::Duration;

pub struct Reader{client:Client,schema:String}
fn connect(request:&DbRequest)->Result<Client,String>{
    let p=&request.profile;
    let mut cfg=Config::new();
    cfg.host(&p.host).port(p.port).user(&p.username).dbname(&p.database).connect_timeout(Duration::from_secs(12));
    if let Some(secret)=&request.secret{cfg.password(secret);}
    if p.tls{
        cfg.ssl_mode(SslMode::Require);
        let mut builder=native_tls::TlsConnector::builder();
        if !p.ca_path.is_empty(){
            let bytes=std::fs::read(&p.ca_path).map_err(err)?;
            let cert=native_tls::Certificate::from_pem(&bytes).or_else(|_|native_tls::Certificate::from_der(&bytes)).map_err(err)?;
            builder.add_root_certificate(cert);
        }
        cfg.connect(MakeTlsConnector::new(builder.build().map_err(err)?)).map_err(err)
    }else{
        cfg.ssl_mode(SslMode::Disable);cfg.connect(NoTls).map_err(err)
    }
}
fn schema_name(request:&DbRequest)->String{if request.profile.schema.is_empty(){"public".into()}else{request.profile.schema.clone()}}
pub fn catalog(request:&DbRequest)->Result<Vec<String>,String>{
    let mut client=connect(request)?;
    let schema=schema_name(request);
    let rows=client.query("SELECT c.relname::text FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=$1 AND c.relkind IN ('r','p') ORDER BY c.relname",&[&schema]).map_err(err)?;
    rows.into_iter().map(|r|r.try_get(0).map_err(err)).collect()
}
impl Reader{
    pub fn open(request:&DbRequest)->Result<Self,String>{
        let mut client=connect(request)?;
        client.batch_execute("SET statement_timeout='120s'; SET lock_timeout='15s'; SET TIME ZONE 'UTC'; SET DateStyle TO 'ISO, YMD'; SET IntervalStyle TO 'iso_8601'; SET extra_float_digits=3; SET bytea_output='hex'; BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;").map_err(err)?;
        Ok(Self{client,schema:schema_name(request)})
    }
}
impl DbReader for Reader{
    fn schema(&mut self,relation:&str)->Result<DbSchema,String>{
        let table=self.client.query_opt("SELECT c.oid FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=$1 AND c.relname=$2 AND c.relkind IN ('r','p')",&[&self.schema,&relation]).map_err(err)?.ok_or("Base table not found.")?;
        let oid:u32=table.try_get(0).map_err(err)?;
        let mut columns=Vec::new();
        for row in self.client.query("SELECT a.attname::text,pg_catalog.format_type(a.atttypid,a.atttypmod),a.attnotnull,pg_catalog.pg_get_expr(d.adbin,d.adrelid),a.attidentity::text,a.attgenerated::text,COALESCE(coll.collname::text,'') FROM pg_catalog.pg_attribute a LEFT JOIN pg_catalog.pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum LEFT JOIN pg_catalog.pg_collation coll ON coll.oid=a.attcollation WHERE a.attrelid=$1 AND a.attnum>0 AND NOT a.attisdropped ORDER BY a.attnum",&[&oid]).map_err(err)?{
            columns.push(DbColumn{name:row.try_get(0).map_err(err)?,data_type:row.try_get(1).map_err(err)?,nullable:!row.try_get::<_,bool>(2).map_err(err)?,default_value:row.try_get(3).map_err(err)?,extra:serde_json::json!([row.try_get::<_,String>(4).map_err(err)?,row.try_get::<_,String>(5).map_err(err)?,row.try_get::<_,String>(6).map_err(err)?]).to_string()});
        }
        let primary_key=self.client.query("SELECT a.attname::text FROM pg_catalog.pg_index i CROSS JOIN LATERAL unnest(i.indkey) WITH ORDINALITY k(attnum,ord) JOIN pg_catalog.pg_attribute a ON a.attrelid=i.indrelid AND a.attnum=k.attnum WHERE i.indrelid=$1 AND i.indisprimary AND k.ord<=i.indnkeyatts ORDER BY k.ord",&[&oid]).map_err(err)?.into_iter().map(|r|r.try_get(0).map_err(err)).collect::<Result<Vec<String>,_>>()?;
        let mut objects=Vec::new();
        for row in self.client.query("SELECT ci.relname::text,pg_catalog.pg_get_indexdef(i.indexrelid) FROM pg_catalog.pg_index i JOIN pg_catalog.pg_class ci ON ci.oid=i.indexrelid WHERE i.indrelid=$1 ORDER BY ci.relname",&[&oid]).map_err(err)?{
            objects.push(DbObject{kind:"index".into(),name:row.try_get(0).map_err(err)?,definition:row.try_get(1).map_err(err)?});
        }
        for row in self.client.query("SELECT conname::text,pg_catalog.pg_get_constraintdef(oid) FROM pg_catalog.pg_constraint WHERE conrelid=$1 ORDER BY conname",&[&oid]).map_err(err)?{
            objects.push(DbObject{kind:"constraint".into(),name:row.try_get(0).map_err(err)?,definition:row.try_get(1).map_err(err)?});
        }
        Ok(DbSchema{columns,primary_key,objects})
    }
    fn rows(&mut self,relation:&str,schema:&DbSchema,keys:&[String],ignore:&[String],cancel:&AtomicBool)->Result<Rows,String>{
        // Server-native text representation handles arrays, numerics, dates and
        // extension types without lossy conversion through JS floating point.
        let fields=schema.columns.iter().map(|c|quote(&c.name,'"').map(|n|format!("{n}::text"))).collect::<Result<Vec<_>,_>>()?.join(",");
        let sql=format!("SELECT {fields} FROM {}.{}",quote(&self.schema,'"')?,quote(relation,'"')?);
        let mut query=self.client.query_raw(&sql,std::iter::empty::<&(dyn postgres::types::ToSql+Sync)>()).map_err(err)?;
        use postgres::fallible_iterator::FallibleIterator;
        let mut rows=Rows::new();
        while let Some(row)=query.next().map_err(err)?{
            cancelled(cancel)?;
            let mut result=Row::new();
            for(i,col)in schema.columns.iter().enumerate(){
                let item:Option<String>=row.try_get(i).map_err(err)?;
                result.insert(col.name.clone(),match item{Some(v)=>value(&format!("pg:{}",col.data_type),v),None=>value("null","")});
            }
            insert(&mut rows,result,keys,ignore)?;
        }
        Ok(rows)
    }
}
