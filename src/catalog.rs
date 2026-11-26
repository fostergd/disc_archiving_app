//! Local catalog of planned assignments. Import is not evidence of a physical burn.
use anyhow::{bail,Context,Result};
use rusqlite::{Connection,OpenFlags,OptionalExtension,params};
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
CREATE TABLE IF NOT EXISTS verified_discs(project_id INTEGER NOT NULL, disc_number INTEGER NOT NULL,
 manifest TEXT NOT NULL, image_sha256 TEXT NOT NULL, recorder TEXT NOT NULL, verified_at TEXT NOT NULL,
 journal_path TEXT NOT NULL, attempt_id INTEGER NOT NULL,
 PRIMARY KEY(project_id,disc_number), FOREIGN KEY(project_id,disc_number) REFERENCES discs(project_id,number));
PRAGMA user_version=2;
"#;
const APP_ID:i64=0x44415343;
pub struct Catalog {conn:Connection}
#[derive(Debug,Serialize)]
pub struct Match {
    pub project_id:i64,pub project:String,pub archive:String,pub source:String,pub bytes:i64,
    pub sha256:Option<String>,pub disc_number:Option<i64>,pub disc_label:Option<String>,
    pub status:String,pub verified_at:Option<String>,
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
        if app!=APP_ID||!(1..=2).contains(&version){bail!("Not a supported Disc Archive Studio catalog");}
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        if writable && version==1 { conn.execute_batch("BEGIN IMMEDIATE; CREATE TABLE IF NOT EXISTS verified_discs(project_id INTEGER NOT NULL, disc_number INTEGER NOT NULL,
 manifest TEXT NOT NULL, image_sha256 TEXT NOT NULL, recorder TEXT NOT NULL, verified_at TEXT NOT NULL,
 journal_path TEXT NOT NULL, attempt_id INTEGER NOT NULL,
 PRIMARY KEY(project_id,disc_number), FOREIGN KEY(project_id,disc_number) REFERENCES discs(project_id,number)); PRAGMA user_version=2; COMMIT;")?; }
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
    /// Explicitly link a successful readback to a catalog snapshot. No source access.
    /// Require pre-existing hashes: matching names and sizes alone cannot establish identity.
    pub fn link_verified(&mut self,project:i64,disc:i64,journal_path:&Path,attempt:i64)->Result<()>{
        let journal_path=journal_path.canonicalize()?;
        let journal=crate::jobs::Journal::open(&journal_path)?;
        let (manifest,hash,recorder,at)=journal.verified_evidence(attempt)?;
        if i64::try_from(manifest.number)?!=disc {bail!("Disc number mismatch");}
        let tx=self.conn.transaction()?;
        let (label,capacity):(String,i64)=tx.query_row("SELECT label,capacity FROM discs WHERE project_id=?1 AND number=?2",params![project,disc],|r|Ok((r.get(0)?,r.get(1)?)))?;
        if label!=manifest.label||capacity!=i64::try_from(manifest.capacity)? {bail!("Disc identity mismatch");}
        {
            let mut q=tx.prepare("SELECT archive,bytes,sha256 FROM files WHERE project_id=?1 AND disc_number=?2")?;
            let rows=q.query_map(params![project,disc],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,Option<String>>(2)?)))?;
            let mut expected=std::collections::BTreeMap::new();
            for row in rows {let (name,bytes,hash)=row?;expected.insert(name,(bytes,hash));}
            if expected.len()!=manifest.files.len(){bail!("Manifest file count mismatch");}
            for f in &manifest.files {
                let (bytes,hash)=expected.remove(&f.archive).context("Manifest path mismatch")?;
                if bytes!=i64::try_from(f.bytes)?||!hash.as_deref().is_some_and(|h|h.eq_ignore_ascii_case(&f.sha256)){bail!("Manifest content identity mismatch");}
            }
        }
        tx.execute("INSERT INTO verified_discs VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",params![project,disc,serde_json::to_string(&manifest)?,hash,recorder,at,journal_path.to_str().context("Non-Unicode journal path")?,attempt])?;
        tx.commit()?;Ok(())
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
        let version:i64=self.conn.pragma_query_value(None,"user_version",|r|r.get(0))?;
        let mut q=self.conn.prepare("SELECT p.id,p.name,f.archive,f.source,f.bytes,f.sha256,f.disc_number,d.label FROM files f JOIN projects p ON p.id=f.project_id LEFT JOIN discs d ON d.project_id=f.project_id AND d.number=f.disc_number WHERE (?1 IS NULL OR p.id=?1) ORDER BY p.id,f.archive")?;
        let mut rows=q.query([project])?;
        let mut skipped=0usize;let mut result=Vec::new();
        while let Some(r)=rows.next()?{
            let archive:String=r.get(2)?;let hash:Option<String>=r.get(5)?;
            if !archive.to_lowercase().contains(&needle)&&!hash.as_ref().map(|h|h.to_lowercase().contains(&needle)).unwrap_or(false){continue;}
            if skipped<offset{skipped+=1;continue;}
            let number:Option<i64>=r.get(6)?;
            let verified_at:Option<String>=if version>=2 && number.is_some(){self.conn.query_row("SELECT verified_at FROM verified_discs WHERE project_id=?1 AND disc_number=?2",params![r.get::<_,i64>(0)?,number],|r|r.get(0)).optional()?}else{None};
            result.push(Match{project_id:r.get(0)?,project:r.get(1)?,archive,source:r.get(3)?,bytes:r.get(4)?,sha256:hash,disc_number:number,disc_label:r.get(7)?,status:if verified_at.is_some(){"content_verified_at_import"}else if number.is_some(){"planned"}else{"unassigned"}.into(),verified_at});
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
        let rows=db.search("épisode",Some(id),10,0).unwrap();assert_eq!(rows.len(),1);assert_eq!(rows[0].status,"planned");assert!(rows[0].verified_at.is_none());
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

#[cfg(test)]
#[test]
fn explicit_verified_link_requires_exact_hashed_snapshot(){
    use crate::{Entry,plan_mixed};
    let t=tempfile::tempdir().unwrap();
    let jp=t.path().join("jobs.sqlite");
    let j=crate::jobs::Journal::create(&jp).unwrap();
    let h="a".repeat(64);
    let mut p=Project::default();p.capacities=vec![50000];p.reserve=0;
    p.plan=Some(plan_mixed(&[Entry{source:"offline".into(),archive:"file.bin".into(),bytes:1,sha256:Some(h.clone())}],&p.capacities,0,p.strategy).unwrap());
    let disc=&p.plan.as_ref().unwrap().discs[0];
    let a=j.begin("A",Path::new("image.iso"),&h,disc.number,&disc.label).unwrap();
    let manifest=crate::image::PreparedDisc{number:disc.number,label:disc.label.clone(),capacity:disc.capacity,reserve:0,estimated_used:disc.estimated_used,files:vec![crate::image::PreparedFile{source:"offline".into(),archive:"file.bin".into(),bytes:1,sha256:h.clone()}]};
    j.bind_manifest(a,&manifest).unwrap();j.finish(a,true,"synthetic").unwrap();
    let mut db=Catalog::create(&t.path().join("catalog.sqlite")).unwrap();
    let id=db.import(&p).unwrap();let n=i64::try_from(disc.number).unwrap();
    assert!(db.link_verified(id,n,&jp,a).is_err());
    j.record_readback(a,"B",true,"synthetic").unwrap();
    assert!(db.link_verified(id,n+1,&jp,a).is_err());
    db.link_verified(id,n,&jp,a).unwrap();
    let rows=db.search("",Some(id),10,0).unwrap();assert_eq!(rows[0].status,"content_verified_at_import");assert!(rows[0].verified_at.as_ref().is_some_and(|s|!s.is_empty()));
    assert!(db.link_verified(id,n,&jp,a).is_err());
    let mut wrong=p.clone();wrong.plan.as_mut().unwrap().discs[0].files[0].sha256=Some("b".repeat(64));
    let other=db.import(&wrong).unwrap();assert!(db.link_verified(other,n,&jp,a).is_err());
    j.record_readback(a,"B",false,"synthetic mismatch").unwrap();
    assert!(db.link_verified(other,n,&jp,a).is_err());
}
