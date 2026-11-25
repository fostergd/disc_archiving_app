//! Durable per-attempt journal. An unfinished attempt is never retried automatically.
use anyhow::{bail,Context,Result};
use rusqlite::{Connection,OpenFlags,params};
use serde::Serialize;
use std::path::Path;
const APP_ID:i64=0x4441534a;
pub struct Journal{db:Connection}
// Process-lifetime lock using a named, persistent file and OS advisory locking.
// Never remove the lock file: removal would allow two independent lock identities.
pub(crate) struct RecorderGuard{_file:std::fs::File}
impl RecorderGuard{
    pub(crate) fn acquire(id:&str)->Result<Self>{
        let root=std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is required for recorder coordination")?;
        Self::acquire_in(Path::new(&root).join("DiscArchiveStudio").join("recorder-locks").as_path(),id)
    }
    fn acquire_in(root:&Path,id:&str)->Result<Self>{
        use sha2::{Digest,Sha256};
        if id.trim().is_empty(){bail!("Empty recorder identity");}
        std::fs::create_dir_all(root)?;
        let name=format!("{:x}.lock",Sha256::digest(id.to_lowercase().as_bytes()));
        let file=std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(root.join(name))?;
        file.try_lock().map_err(|e|anyhow::anyhow!("Recorder operation already active or lock unavailable: {e:?}"))?;
        Ok(Self{_file:file})
    }
}
#[derive(Debug,Serialize)]
pub struct Job{
    pub id:i64,pub recorder:String,pub image:String,pub sha256:String,pub disc_number:i64,
    pub label:String,pub state:String,pub detail:String,pub started:String,pub finished:Option<String>,pub reconciliation:Option<String>,pub reconciled_at:Option<String>,
}
impl Journal{
    pub fn create(path:&Path)->Result<Self>{
        std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
        let db=Connection::open_with_flags(path,OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        db.execute_batch("PRAGMA synchronous=FULL; BEGIN IMMEDIATE;
        PRAGMA application_id=1145131850; PRAGMA user_version=2;
        CREATE TABLE attempts(id INTEGER PRIMARY KEY, recorder TEXT NOT NULL,image TEXT NOT NULL,
          sha256 TEXT NOT NULL,disc_number INTEGER NOT NULL,label TEXT NOT NULL,state TEXT NOT NULL
          CHECK(state IN ('in_progress','written_unverified','outcome_unknown')),
          detail TEXT NOT NULL,started TEXT NOT NULL,finished TEXT);
        CREATE INDEX recorder_history ON attempts(recorder);
        CREATE TABLE reconciliations(attempt_id INTEGER PRIMARY KEY REFERENCES attempts(id), note TEXT NOT NULL, at TEXT NOT NULL);
        CREATE TRIGGER block_unresolved BEFORE INSERT ON attempts WHEN EXISTS(SELECT 1 FROM attempts a WHERE a.recorder=NEW.recorder COLLATE NOCASE AND a.state IN ('in_progress','outcome_unknown') AND NOT EXISTS(SELECT 1 FROM reconciliations r WHERE r.attempt_id=a.id)) BEGIN SELECT RAISE(ABORT,'Unresolved recorder attempt'); END;
        COMMIT;")?;
        Ok(Self{db})
    }
    pub fn open(path:&Path)->Result<Self>{
        let db=Connection::open_with_flags(path,OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        let app:i64=db.pragma_query_value(None,"application_id",|r|r.get(0))?;
        let version:i64=db.pragma_query_value(None,"user_version",|r|r.get(0))?;
        if app!=APP_ID||!(1..=2).contains(&version){bail!("Not a supported burn journal");}
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch("PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;")?;
        if version==1{db.execute_batch("BEGIN IMMEDIATE; CREATE TABLE IF NOT EXISTS reconciliations(attempt_id INTEGER PRIMARY KEY REFERENCES attempts(id), note TEXT NOT NULL, at TEXT NOT NULL); DROP INDEX IF EXISTS one_active_recorder; CREATE INDEX IF NOT EXISTS recorder_history ON attempts(recorder); CREATE TRIGGER IF NOT EXISTS block_unresolved BEFORE INSERT ON attempts WHEN EXISTS(SELECT 1 FROM attempts a WHERE a.recorder=NEW.recorder COLLATE NOCASE AND a.state IN ('in_progress','outcome_unknown') AND NOT EXISTS(SELECT 1 FROM reconciliations r WHERE r.attempt_id=a.id)) BEGIN SELECT RAISE(ABORT,'Unresolved recorder attempt'); END; PRAGMA user_version=2; COMMIT;")?;}
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
        if self.db.execute("UPDATE attempts SET state=?1,detail=?2,finished=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?3 AND state='in_progress' AND NOT EXISTS(SELECT 1 FROM reconciliations WHERE attempt_id=?3)",params![state,detail,id])?!=1{bail!("Attempt is not in progress");}
        Ok(())
    }
    pub fn reconcile(&self,id:i64,confirmation:&str,note:&str)->Result<()>{
        if confirmation!="INSPECTED-REMOVE-MEDIA"||note.trim().is_empty(){bail!("Explicit inspection confirmation and note required");}
        let recorder:String=self.db.query_row("SELECT recorder FROM attempts WHERE id=?1 AND state IN ('in_progress','outcome_unknown')",[id],|r|r.get(0))?;
        let _guard=RecorderGuard::acquire(&recorder)?;
        self.reconcile_locked(id,note)
    }
    fn reconcile_locked(&self,id:i64,note:&str)->Result<()>{
        if self.db.execute("INSERT INTO reconciliations SELECT id,?2,strftime('%Y-%m-%dT%H:%M:%fZ','now') FROM attempts WHERE id=?1 AND state IN ('in_progress','outcome_unknown')",params![id,note])?!=1{bail!("Attempt is not unresolved");}
        Ok(())
    }
    pub fn list(&self)->Result<Vec<Job>>{
        let mut q=self.db.prepare("SELECT a.id,a.recorder,a.image,a.sha256,a.disc_number,a.label,a.state,a.detail,a.started,a.finished,r.note,r.at FROM attempts a LEFT JOIN reconciliations r ON r.attempt_id=a.id ORDER BY a.id")?;
        let rows=q.query_map([],|r|Ok(Job{id:r.get(0)?,recorder:r.get(1)?,image:r.get(2)?,sha256:r.get(3)?,disc_number:r.get(4)?,label:r.get(5)?,state:r.get(6)?,detail:r.get(7)?,started:r.get(8)?,finished:r.get(9)?,reconciliation:r.get(10)?,reconciled_at:r.get(11)?}))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn independent_recorder_locks(){
        let t=tempfile::tempdir().unwrap();let a=RecorderGuard::acquire_in(t.path(),"A").unwrap();
        assert!(RecorderGuard::acquire_in(t.path(),"a").is_err());
        let _b=RecorderGuard::acquire_in(t.path(),"B").unwrap();drop(a);
        assert!(RecorderGuard::acquire_in(t.path(),"A").is_ok());
    }
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
        j.reconcile_locked(a,"Removed suspect disc").unwrap();
        assert!(j.reconcile_locked(a,"again").is_err());
        assert!(j.begin("A",Path::new("image.iso"),&h,1,"DISC_001").is_ok());
        assert_eq!(j.list().unwrap()[0].reconciliation.as_deref(),Some("Removed suspect disc"));
    }
}
