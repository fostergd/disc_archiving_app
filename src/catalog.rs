//! Local catalog of planned assignments. Import is not evidence of a physical burn.
use anyhow::{bail,Context,Result};
use rusqlite::{Connection,OpenFlags,params};
use serde::Serialize;
use std::path::Path;
use crate::project::Project;

const SCHEMA:&str=r#"
CREATE TABLE projects(id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE discs(project_id INTEGER NOT NULL REFERENCES projects(id), number INTEGER NOT NULL,
 label TEXT NOT NULL, capacity INTEGER NOT NULL, estimated_used INTEGER NOT NULL, locked INTEGER NOT NULL,
 PRIMARY KEY(project_id,number));
CREATE TABLE files(project_id INTEGER NOT NULL REFERENCES projects(id), archive TEXT NOT NULL,
 source TEXT NOT NULL, bytes INTEGER NOT NULL, sha256 TEXT, disc_number INTEGER,
 PRIMARY KEY(project_id,archive), FOREIGN KEY(project_id,disc_number) REFERENCES discs(project_id,number));
CREATE INDEX files_hash ON files(sha256);
PRAGMA user_version=1;
"#;
const APP_ID:i64=0x44415343;
pub struct Catalog {conn:Connection}
#[derive(Debug,Serialize)]
pub struct Match {
    pub project_id:i64,pub project:String,pub archive:String,pub source:String,pub bytes:i64,
    pub sha256:Option<String>,pub disc_number:Option<i64>,pub disc_label:Option<String>,
    pub status:String,
}
#[derive(Debug,Serialize)]
pub struct ProjectRow{pub id:i64,pub name:String,pub files:i64,pub discs:i64}
impl Catalog{
    pub fn create(path:&Path)->Result<Self>{
        // Reserve a new pathname. Never initialize an unrelated existing database.
        std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
        let mut conn=Connection::open_with_flags(path,OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        let tx=conn.transaction()?;
        tx.pragma_update(None,"application_id",APP_ID)?;
        tx.execute_batch(SCHEMA)?;
        tx.commit()?;
        Ok(Self{conn})
    }
    pub fn open(path:&Path,writable:bool)->Result<Self>{
        let conn=Connection::open_with_flags(path,if writable{OpenFlags::SQLITE_OPEN_READ_WRITE}else{OpenFlags::SQLITE_OPEN_READ_ONLY})?;
        let app:i64=conn.pragma_query_value(None,"application_id",|r|r.get(0))?;
        let version:i64=conn.pragma_query_value(None,"user_version",|r|r.get(0))?;
        if app!=APP_ID||version!=1{bail!("Not a supported Disc Archive Studio catalog");}
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(Self{conn})
    }
    pub fn import(&mut self,p:&Project)->Result<i64>{
        p.validate()?;
        let plan=p.plan.as_ref().context("Project has no disc plan")?;
        let tx=self.conn.transaction()?;
        tx.execute("INSERT INTO projects(name) VALUES (?1)",[&p.name])?;
        let id=tx.last_insert_rowid();
        {
            let mut disc=tx.prepare("INSERT INTO discs VALUES (?1,?2,?3,?4,?5,?6)")?;
            let mut file=tx.prepare("INSERT INTO files VALUES (?1,?2,?3,?4,?5,?6)")?;
            for d in &plan.discs{
                let n=i64::try_from(d.number)?;
                disc.execute(params![id,n,d.label,i64::try_from(d.capacity)?,i64::try_from(d.estimated_used)?,d.locked])?;
                for f in &d.files{
                    file.execute(params![id,f.archive,f.source.to_str().context("Non-Unicode source")?,i64::try_from(f.bytes)?,f.sha256,Some(n)])?;
                }
            }
            for f in &plan.oversized{
                file.execute(params![id,f.archive,f.source.to_str().context("Non-Unicode source")?,i64::try_from(f.bytes)?,f.sha256,None::<i64>])?;
            }
        }
        tx.commit()?;Ok(id)
    }
    pub fn projects(&self)->Result<Vec<ProjectRow>>{
        let mut q=self.conn.prepare("SELECT p.id,p.name,(SELECT count(*) FROM files f WHERE f.project_id=p.id),(SELECT count(*) FROM discs d WHERE d.project_id=p.id) FROM projects p ORDER BY p.id")?;
        let rows=q.query_map([],|r|Ok(ProjectRow{id:r.get(0)?,name:r.get(1)?,files:r.get(2)?,discs:r.get(3)?}))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
    // Literal substring matching (no wildcard interpretation), with bounded pages.
    // Unicode case-folding is done in Rust; SQLite NOCASE only handles ASCII.
    pub fn search(&self,term:&str,project:Option<i64>,limit:usize,offset:usize)->Result<Vec<Match>>{
        if limit==0||limit>10_000{bail!("Limit must be between 1 and 10000");}
        let needle=term.to_lowercase();
        let mut q=self.conn.prepare("SELECT p.id,p.name,f.archive,f.source,f.bytes,f.sha256,f.disc_number,d.label FROM files f JOIN projects p ON p.id=f.project_id LEFT JOIN discs d ON d.project_id=f.project_id AND d.number=f.disc_number WHERE (?1 IS NULL OR p.id=?1) ORDER BY p.id,f.archive")?;
        let mut rows=q.query([project])?;
        let mut skipped=0usize;let mut result=Vec::new();
        while let Some(r)=rows.next()?{
            let archive:String=r.get(2)?;let hash:Option<String>=r.get(5)?;
            if !archive.to_lowercase().contains(&needle)&&!hash.as_ref().map(|h|h.to_lowercase().contains(&needle)).unwrap_or(false){continue;}
            if skipped<offset{skipped+=1;continue;}
            let number:Option<i64>=r.get(6)?;
            result.push(Match{project_id:r.get(0)?,project:r.get(1)?,archive,source:r.get(3)?,bytes:r.get(4)?,sha256:hash,disc_number:number,disc_label:r.get(7)?,status:if number.is_some(){"planned"}else{"unassigned"}.into()});
            if result.len()==limit{break;}
        }
        Ok(result)
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    use crate::{Entry,plan_mixed};
    #[test] fn catalog_roundtrip_and_search(){
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("archive.sqlite");
        let mut p=Project::default();p.name="Video collection".into();p.capacities=vec![50000];p.reserve=0;
        let files=vec![Entry{source:"unavailable/one".into(),archive:"Media/Épisode 2%_.mkv".into(),bytes:20000,sha256:None},Entry{source:"unavailable/two".into(),archive:"Media/Large.mkv".into(),bytes:100000,sha256:None}];
        p.plan=Some(plan_mixed(&files,&p.capacities,p.reserve,p.strategy).unwrap());
        let mut db=Catalog::create(&path).unwrap();let id=db.import(&p).unwrap();drop(db);
        assert!(Catalog::create(&path).is_err());
        let db=Catalog::open(&path,false).unwrap();
        let rows=db.search("épisode",Some(id),10,0).unwrap();assert_eq!(rows.len(),1);assert_eq!(rows[0].status,"planned");
        assert_eq!(db.search("%_",None,10,0).unwrap().len(),1);
        assert_eq!(db.search("Large",None,10,0).unwrap()[0].status,"unassigned");
        assert_eq!(db.search("",None,1,1).unwrap().len(),1);
        assert_eq!(db.projects().unwrap()[0].files,2);
    }
    #[test]fn rejects_foreign_database_and_rolls_back(){
        let dir=tempfile::tempdir().unwrap();let foreign=dir.path().join("foreign.sqlite");
        drop(Connection::open(&foreign).unwrap());assert!(Catalog::open(&foreign,true).is_err());
        let path=dir.path().join("archive.sqlite");let mut db=Catalog::create(&path).unwrap();
        let mut p=Project::default();p.capacities=vec![u64::MAX];p.reserve=0;
        let f=Entry{source:"a".into(),archive:"a".into(),bytes:1,sha256:None};
        p.plan=Some(plan_mixed(&[f],&p.capacities,0,p.strategy).unwrap());
        assert!(db.import(&p).is_err());assert!(db.projects().unwrap().is_empty());
    }
}
