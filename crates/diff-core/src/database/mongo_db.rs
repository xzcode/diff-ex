use super::*;
use mongodb::{bson::{self,doc,Bson,Document},options::{ClientOptions,Credential,ServerAddress,Tls,TlsOptions},sync::{Client,Database}};
use std::time::Duration;

pub struct Reader{db:Database}
fn connection(request:&DbRequest)->Result<Database,String>{
    let p=&request.profile;
    let mut opts=ClientOptions::default();
    opts.hosts=vec![ServerAddress::Tcp{host:p.host.clone(),port:Some(p.port)}];
    opts.direct_connection=Some(true);
    opts.app_name=Some("Diff EX".into());
    opts.connect_timeout=Some(Duration::from_secs(12));
    opts.server_selection_timeout=Some(Duration::from_secs(15));
    opts.retry_writes=Some(false);
    opts.tls=Some(if p.tls{
        let mut tls=TlsOptions::default();if !p.ca_path.is_empty(){tls.ca_file_path=Some(p.ca_path.clone().into())}Tls::Enabled(tls)
    }else{Tls::Disabled});
    if !p.username.is_empty(){
        opts.credential=Some(Credential::builder().username(Some(p.username.clone())).password(request.secret.clone()).source(Some(if p.auth_database.is_empty(){"admin".into()}else{p.auth_database.clone()})).build());
    }
    Ok(Client::with_options(opts).map_err(err)?.database(&p.database))
}
pub fn catalog(request:&DbRequest)->Result<Vec<String>,String>{
    let db=connection(request)?;
    let mut names=Vec::new();
    for spec in db.list_collections().run().map_err(err)?{
        let spec=spec.map_err(err)?;
        if format!("{:?}",spec.collection_type)=="Collection"{names.push(spec.name)}
    }
    names.sort();Ok(names)
}
impl Reader{
    pub fn open(request:&DbRequest)->Result<Self,String>{Ok(Self{db:connection(request)?})}
}
fn document_definition(doc:Document)->String{
    // Preserve index field ordering and BSON types in the serialized value.
    let bytes=bson::to_vec(&doc).map(|v|hex(&v)).unwrap_or_default();
    format!("{}\nBSON: {}",Bson::Document(doc).into_canonical_extjson(),bytes)
}
impl DbReader for Reader{
    fn schema(&mut self,relation:&str)->Result<DbSchema,String>{
        let mut objects=Vec::new();
        let mut found=false;
        for spec in self.db.list_collections().filter(doc!{"name":relation}).run().map_err(err)?{
            let spec=spec.map_err(err)?;
            if spec.name!=relation{continue}
            if format!("{:?}",spec.collection_type)!="Collection"{return Err("Select a regular MongoDB collection.".into())}
            found=true;
            let options=bson::to_document(&spec.options).map_err(err)?;
            objects.push(DbObject{kind:"collection".into(),name:"OPTIONS".into(),definition:document_definition(options)});
        }
        if !found{return Err("Collection not found.".into())}
        let coll=self.db.collection::<Document>(relation);
        for model in coll.list_indexes().run().map_err(err)?{
            let model=model.map_err(err)?;
            let name=model.options.as_ref().and_then(|o|o.name.clone()).ok_or("Index has no name.")?;
            let mut definition=bson::to_document(&model).map_err(err)?;
            definition.remove("ns");definition.remove("name");
            objects.push(DbObject{kind:"index".into(),name,definition:document_definition(definition)});
        }
        Ok(DbSchema{columns:vec![],primary_key:vec!["_id".into()],objects})
    }
    fn rows(&mut self,relation:&str,_schema:&DbSchema,keys:&[String],ignore:&[String],cancel:&AtomicBool)->Result<Rows,String>{
        let cursor=self.db.collection::<Document>(relation).find(doc!{}).run().map_err(err)?;
        let mut rows=Rows::new();
        for document in cursor{
            cancelled(cancel)?;
            let document=document.map_err(err)?;
            let mut row=Row::new();
            for (name,b) in document{
                let kind=if b==Bson::Null{"null".into()}else{format!("bson:{:?}",b.element_type())};
                let raw=bson::to_vec(&doc!{"v":b.clone()}).map_err(err)?;
                let val=DbValue{kind,value:b.into_canonical_extjson().to_string(),raw:Some(hex(&raw))};
                row.insert(name,val);
            }
            insert(&mut rows,row,keys,ignore)?;
        }
        Ok(rows)
    }
}
