//! Durable per-attempt journal. An unfinished attempt is never retried automatically.
use anyhow::{bail,Context,Result};
use rusqlite::{Connection,OpenFlags,params};
use serde::Serialize;
use std::path::Path;
const APP_ID:i64=0x4441534a;
pub struct Journal{db:Connection}
#[derive(Debug,Serialize)]
pub struct Job{
    pub id:i64,pub recorder:String,pub image:String,pub sha256:String,pub disc_number:i64,
    pub label:String,pub state:String,pub detail:String,pub started:String,pub finished:Option<String>,
}
impl Journal{
    pub fn create(path:&Path)->Result<Self>{
        std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
        let db=Connection::open_with_flags(path,OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        db.execute_batch("PRAGMA synchronous=FULL; BEGIN IMMEDIATE;
        PRAGMA application_id=1145131850; PRAGMA user_version=1;
        CREATE TABLE attempts(id INTEGER PRIMARY KEY, recorder TEXT NOT NULL,image TEXT NOT NULL,
          sha256 TEXT NOT NULL,disc_number INTEGER NOT NULL,label TEXT NOT NULL,state TEXT NOT NULL
          CHECK(state IN ('in_progress','written_unverified','outcome_unknown')),
          detail TEXT NOT NULL,started TEXT NOT NULL,finished TEXT);
        CREATE UNIQUE INDEX one_active_recorder ON attempts(recorder) WHERE state IN ('in_progress','outcome_unknown');
        COMMIT;")?;
        Ok(Self{db})
    }
    pub fn open(path:&Path)->Result<Self>{
        let db=Connection::open_with_flags(path,OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        let app:i64=db.pragma_query_value(None,"application_id",|r|r.get(0))?;
        let version:i64=db.pragma_query_value(None,"user_version",|r|r.get(0))?;
        if app!=APP_ID||version!=1{bail!("Not a supported burn journal");}
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch("PRAGMA synchronous=FULL;")?;
        Ok(Self{db})
    }
    pub fn begin(&self,recorder:&str,image:&Path,sha256:&str,number:usize,label:&str)->Result<i64>{
        if recorder.trim().is_empty()||sha256.len()!=64||!sha256.bytes().all(|b|b.is_ascii_hexdigit()){bail!("Invalid job identity");}
        self.db.execute("INSERT INTO attempts(recorder,image,sha256,disc_number,label,state,detail,started) VALUES (?1,?2,?3,?4,?5,'in_progress','Outcome unknown until writer returns',strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
            params![recorder,image.to_str().context("Non-Unicode image path")?,sha256.to_ascii_lowercase(),i64::try_from(number)?,label])?;
        Ok(self.db.last_insert_rowid())
    }
    pub fn finish(&self,id:i64,written:bool,detail:&str)->Result<()>{
        let state=if written{"written_unverified"}else{"outcome_unknown"};
        if self.db.execute("UPDATE attempts SET state=?1,detail=?2,finished=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?3 AND state='in_progress'",params![state,detail,id])?!=1{bail!("Attempt is not in progress");}
        Ok(())
    }
    pub fn list(&self)->Result<Vec<Job>>{
        let mut q=self.db.prepare("SELECT id,recorder,image,sha256,disc_number,label,state,detail,started,finished FROM attempts ORDER BY id")?;
        let rows=q.query_map([],|r|Ok(Job{id:r.get(0)?,recorder:r.get(1)?,image:r.get(2)?,sha256:r.get(3)?,disc_number:r.get(4)?,label:r.get(5)?,state:r.get(6)?,detail:r.get(7)?,started:r.get(8)?,finished:r.get(9)?}))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn journal_reopens_and_blocks_uncertain_attempts(){
        let t=tempfile::tempdir().unwrap();let p=t.path().join("jobs.sqlite");let h="a".repeat(64);
        let j=Journal::create(&p).unwrap();
        let a=j.begin("A",Path::new("image.iso"),&h,1,"DISC_001").unwrap();drop(j);
        let j=Journal::open(&p).unwrap();assert_eq!(j.list().unwrap()[0].state,"in_progress");
        assert!(j.begin("A",Path::new("image.iso"),&h,1,"DISC_001").is_err());
        let b=j.begin("B",Path::new("image.iso"),&h,1,"DISC_001").unwrap();
        j.finish(a,false,"interrupted").unwrap();j.finish(b,true,"writer returned").unwrap();
        assert!(j.finish(b,true,"again").is_err());
        assert!(j.begin("A",Path::new("image.iso"),&h,1,"DISC_001").is_err());
        assert!(j.begin("B",Path::new("image.iso"),&h,1,"DISC_001").is_ok());
        assert!(Journal::create(&p).is_err());
    }
}
