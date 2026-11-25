//! Explicitly authorized independent recorder jobs; no automatic retry.
use anyhow::{bail,Result};
use serde::{Deserialize,Serialize};
use std::{path::{Path,PathBuf},collections::HashSet};
use crate::optical::{BurnReceipt,BurnStage};
#[derive(Debug,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request{pub recorder:String,pub image:PathBuf,pub manifest:PathBuf,pub sha256:String}
#[derive(Debug,Serialize)]
pub struct Outcome{pub recorder:String,pub receipt:Option<BurnReceipt>,pub error:Option<String>}
fn validate(requests:&[Request],confirmation:&str)->Result<()>{
    if requests.is_empty()||requests.len()>16{bail!("Batch must contain 1 to 16 explicitly selected recorders");}
    let mut ids=HashSet::new();
    for r in requests{
        crate::optical::validate_authorization(&r.recorder,&r.sha256,confirmation)?;
        if !ids.insert(r.recorder.to_lowercase()){bail!("Duplicate recorder in batch");}
    }
    Ok(())
}
fn run_workers<F>(requests:Vec<Request>,worker:F)->Vec<Outcome>
where F:Fn(Request)->Result<BurnReceipt>+Sync{
    std::thread::scope(|scope|{
        let mut handles=Vec::new();
        for r in requests{
            let id=r.recorder.clone();let worker=&worker;
            match std::thread::Builder::new().spawn_scoped(scope,move||worker(r)){
                Ok(h)=>handles.push((id,Ok(h))),
                Err(e)=>handles.push((id,Err(e))),
            }
        }
        handles.into_iter().map(|(recorder,h)|{
            let result=match h{
                Ok(h)=>h.join().unwrap_or_else(|_|Err(anyhow::anyhow!("Worker panicked; inspect journal and media"))),
                Err(e)=>Err(e.into()),
            };
            match result{
                Ok(receipt)=>Outcome{recorder,receipt:Some(receipt),error:None},
                Err(e)=>Outcome{recorder,receipt:None,error:Some(format!("{e:#}"))},
            }
        }).collect()
    })
}
pub fn execute(requests:Vec<Request>,journal:&Path,confirmation:&str,progress:impl Fn(&str,BurnStage)+Sync)->Result<Vec<Outcome>>{
    validate(&requests,confirmation)?;
    // Validate all input manifests and paths before launching any worker.
    let journal=journal.canonicalize()?;
    drop(crate::jobs::Journal::open(&journal)?);
    let mut images=HashSet::new();
    let mut prepared=std::collections::HashMap::new();
    for r in &requests{
        let image=r.image.canonicalize()?;
        if !images.insert(image.to_string_lossy().to_lowercase()){bail!("Use distinct image paths for simultaneous jobs");}
        let manifest:crate::image::PreparedDisc=serde_json::from_reader(std::io::BufReader::new(std::fs::File::open(&r.manifest)?))?;
        prepared.insert(r.recorder.clone(),(image,manifest));
    }
    Ok(run_workers(requests,|r|{
        let (image,manifest)=&prepared[&r.recorder];
        let j=crate::jobs::Journal::open(&journal)?;
        crate::optical::burn_logged_with_progress(&j,manifest,image,&r.recorder,&r.sha256,confirmation,&mut |stage|progress(&r.recorder,stage))
    }))
}
#[cfg(test)]
mod tests{
    use super::*;
    fn request(id:&str)->Request{Request{recorder:id.into(),image:"image.iso".into(),manifest:"manifest.json".into(),sha256:"a".repeat(64)}}
    #[test]fn rejects_duplicates_and_missing_confirmation(){
        assert!(validate(&[request("A"),request("a")],"WRITE-ONCE").is_err());
        assert!(validate(&[request("A")],"yes").is_err());
        assert!(validate(&[],"WRITE-ONCE").is_err());
    }
    #[test]fn independent_workers_and_failure_isolation(){
        let barrier=std::sync::Barrier::new(2);
        let outcomes=run_workers(vec![request("A"),request("B")],|r|{
            barrier.wait();
            if r.recorder=="A"{bail!("simulated failure");}
            Ok(BurnReceipt{recorder_id:r.recorder,image_sha256:r.sha256,image_bytes:2048,status:"written_unverified".into()})
        });
        assert_eq!(outcomes.len(),2);
        assert!(outcomes[0].error.is_some());
        assert!(outcomes[1].receipt.is_some());
    }
}
