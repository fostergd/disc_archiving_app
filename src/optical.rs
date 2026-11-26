//! Recorder discovery, explicitly authorized writing and read-only verification.
use anyhow::{bail,Result};
use serde::{Serialize,Deserialize};

#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct Recorder {
    pub id:String,
    pub vendor:String,
    pub product:String,
    pub revision:String,
    pub paths:Vec<String>,
    pub profiles:Vec<i32>,
    pub media:Option<Media>,
    pub inspection_error:Option<String>,
}
#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct Media{
    pub physical_type:i32,
    pub supported:bool,
    pub physically_blank:bool,
    pub heuristically_blank:bool,
    pub status:i32,
    pub free_sectors:i64,
    pub total_sectors:i64,
}
impl Media{
    // IMAPI physical media enum: CD-R=2, DVD+R=6, DVD+R DL=8,
    // DVD-R=9, DVD-R DL=11, BD-R=18. Unknown/rewriteable media fail closed.
    pub fn type_name(&self)->&'static str{
        match self.physical_type{2=>"CD-R",6=>"DVD+R",8=>"DVD+R DL",9=>"DVD-R",11=>"DVD-R DL",18=>"BD-R",_=>"Other / unsupported"}
    }
    pub fn write_once(&self)->bool{matches!(self.physical_type,2|6|8|9|11|18)}
    pub fn blank_candidate(&self)->bool{
        self.supported&&self.write_once()&&self.physically_blank&&self.heuristically_blank
        &&self.free_sectors>0&&self.total_sectors>=self.free_sectors
        &&self.status==2 // require explicit IMAPI blank state
    }
    pub fn fits_image(&self,bytes:u64)->bool{
        self.blank_candidate()&&bytes>0&&bytes%2048==0&&self.free_bytes().is_some_and(|free|bytes<=free)
    }
    pub fn free_bytes(&self)->Option<u64>{
        u64::try_from(self.free_sectors).ok()?.checked_mul(2048)
    }
}
#[derive(Debug,Serialize,Deserialize)]
pub struct Discovery{
    pub recorders:Vec<Recorder>,
    pub errors:Vec<String>,
}
impl Discovery{
    pub fn validate(&self)->Result<()>{
        let mut ids=std::collections::HashSet::new();
        for r in &self.recorders{
            if r.id.trim().is_empty()||!ids.insert(&r.id){bail!("Invalid or duplicate recorder identity");}
            if r.media.is_some()&&r.inspection_error.is_some(){bail!("Conflicting recorder inspection result");}
        }
        Ok(())
    }
}
#[cfg(windows)]
pub fn discover()->Result<Discovery>{
    use anyhow::Context;
    use std::process::Command;
    let work=tempfile::Builder::new().prefix("das-optical-").tempdir()?;
    let script=work.path().join("discover.ps1");
    std::fs::write(&script,include_str!("imapi_discover.ps1"))?;
    let result=Command::new("powershell.exe")
        .args(["-NoLogo","-NoProfile","-NonInteractive","-ExecutionPolicy","Bypass","-File"])
        .arg(script).output().context("Starting IMAPI recorder discovery")?;
    if !result.status.success(){bail!("Recorder discovery failed: {}",String::from_utf8_lossy(&result.stderr));}
    let result:Discovery=serde_json::from_slice(&result.stdout).context("Invalid IMAPI discovery response")?;
    result.validate()?;Ok(result)
}
#[cfg(not(windows))]
pub fn discover()->Result<Discovery>{bail!("Optical discovery requires Windows")}

#[cfg(test)]
mod tests{
    use super::*;
    fn media(kind:i32)->Media{Media{physical_type:kind,supported:true,physically_blank:true,heuristically_blank:true,status:2,free_sectors:100,total_sectors:100}}
    #[test]fn media_policy(){
        for n in [2,6,8,9,11,18]{assert!(media(n).blank_candidate());}
        for n in [0,1,3,4,5,7,10,12,13,14,15,16,17,19,20,21,22]{assert!(!media(n).blank_candidate());}
        let mut m=media(18);m.physically_blank=false;assert!(!m.blank_candidate());
        let mut m=media(18);m.free_sectors=-1;assert_eq!(m.free_bytes(),None);assert!(!m.blank_candidate());
        let mut m=media(18);m.status=0x8000;assert!(!m.blank_candidate());
        let m=media(18);assert_eq!(m.free_bytes(),Some(204800));
        assert!(m.fits_image(204800));assert!(!m.fits_image(206848));assert!(!m.fits_image(1));assert!(!m.fits_image(0));
        for status in [0,1,3,4,8,16,0x400,0x800,0x1000,0x2000,0x4000,0x8000,-1]{let mut m=media(18);m.status=status;assert!(!m.fits_image(2048));}
    }
    #[test]fn native_media_policy_matches_documented_enum(){
        for (kind,name) in [(2,"CD-R"),(6,"DVD+R"),(8,"DVD+R DL"),(9,"DVD-R"),(11,"DVD-R DL"),(18,"BD-R")]{
            assert_eq!(media(kind).type_name(),name);
            assert!(media(kind).write_once());
        }
        for kind in [5,7,10,13,19]{assert!(!media(kind).write_once());}
        let script=include_str!("imapi_burn.ps1");
        assert!(script.contains("@(2,6,8,9,11,18)"));
        assert!(script.contains("CurrentMediaStatus -ne 2"));
    }
    #[test]fn empty_discovery_and_duplicate_ids(){
        let d:Discovery=serde_json::from_str(r#"{"recorders":[],"errors":[]}"#).unwrap();d.validate().unwrap();
        let r=Recorder{id:"id".into(),vendor:String::new(),product:String::new(),revision:String::new(),paths:vec![],profiles:vec![],media:None,inspection_error:Some("No medium".into())};
        let d=Discovery{recorders:vec![r.clone(),r],errors:vec![]};assert!(d.validate().is_err());
    }
    #[cfg(windows)]
    #[test]fn native_discovery_smoke(){discover().expect("Native IMAPI discovery").validate().unwrap();}
}


/// Read-only authorization preview. This is never permission to write later:
/// the actual writer must repeat all checks under exclusive access.
#[derive(Debug,Serialize)]
pub struct BurnPreview{
    pub recorder_id:String,pub image_bytes:u64,pub image_sha256:String,
    pub media_type:String,pub free_bytes:u64,pub verified_files:usize,
}
fn eligible<'a>(d:&'a Discovery,id:&str,bytes:u64)->Result<&'a Media>{
    use anyhow::Context;
    d.validate()?;
    let r=d.recorders.iter().find(|r|r.id==id).context("Selected recorder not found")?;
    let m=r.media.as_ref().context("Selected recorder has no inspected medium")?;
    if !m.fits_image(bytes){bail!("Selected medium is not an eligible blank write-once disc or lacks capacity");}
    Ok(m)
}
pub fn burn_preview(manifest:&crate::image::PreparedDisc,image:&std::path::Path,id:&str)->Result<BurnPreview>{
    if id.trim().is_empty(){bail!("Explicit recorder identity required");}
    // Image verification mounts a virtual ISO only, never a physical recorder.
    let verified=crate::image::verify_udf(manifest,image)?;
    let discovery=discover()?;
    let m=eligible(&discovery,id,verified.image_bytes)?;
    Ok(BurnPreview{recorder_id:id.to_owned(),image_bytes:verified.image_bytes,image_sha256:verified.sha256,
        media_type:m.type_name().into(),free_bytes:m.free_bytes().unwrap_or(0),verified_files:verified.files})
}
#[cfg(test)]
mod preview_tests{
    use super::*;
    #[test]fn explicit_target_and_capacity(){
        let m=Media{physical_type:18,supported:true,physically_blank:true,heuristically_blank:true,status:2,free_sectors:100,total_sectors:100};
        let r=Recorder{id:"drive-A".into(),vendor:"test".into(),product:"test".into(),revision:String::new(),paths:vec![],profiles:vec![],media:Some(m),inspection_error:None};
        let mut d=Discovery{recorders:vec![r],errors:vec![]};
        assert!(eligible(&d,"drive-A",204800).is_ok());
        assert!(eligible(&d,"drive-B",2048).is_err());
        assert!(eligible(&d,"drive-A",206848).is_err());
        assert!(eligible(&d,"drive-A",2047).is_err());
        d.recorders[0].media.as_mut().unwrap().physically_blank=false;
        assert!(eligible(&d,"drive-A",2048).is_err());
        d.recorders[0].media=None;
        assert!(eligible(&d,"drive-A",2048).is_err());
    }
}


#[derive(Debug,Serialize)]
pub struct BurnReceipt {
    pub recorder_id:String,pub image_sha256:String,pub image_bytes:u64,
    pub status:String,
}
pub(crate) fn validate_authorization(id:&str,hash:&str,confirmation:&str)->Result<()>{
    if id.trim().is_empty(){bail!("Explicit recorder identity required");}
    if hash.len()!=64||!hash.bytes().all(|b|b.is_ascii_hexdigit()){bail!("Expected SHA-256 required");}
    if confirmation!="WRITE-ONCE"{bail!("Explicit WRITE-ONCE confirmation required");}
    Ok(())
}
#[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize)]
#[serde(rename_all="snake_case")]
pub enum BurnStage{VerifyingImage,InspectingRecorder,CheckingMedia,WritingAndFinalizing,WritingProgress{written_sectors:u64,total_sectors:u64},ReleasingRecorder,WrittenUnverified}
fn parse_stage(line:&str)->Option<BurnStage>{
    match line.trim(){
        "DAS_STAGE CHECKING_MEDIA"=>Some(BurnStage::CheckingMedia),
        "DAS_STAGE WRITING"=>Some(BurnStage::WritingAndFinalizing),
        "DAS_STAGE RELEASING"=>Some(BurnStage::ReleasingRecorder),
        _=>None,
    }
}
// Accept only the ordered native protocol. A completion marker alone is not evidence
// that the expected writer lifecycle ran.
#[derive(Default)]
struct WriterProtocol { next: usize, invalid: bool, last_progress:Option<(u64,u64)> }
impl WriterProtocol {
    fn observe(&mut self,line:&str)->Option<BurnStage>{
        let line=line.trim();
        if line.is_empty(){return None;}
        if let Some(data)=line.strip_prefix("DAS_PROGRESS "){
            if self.next!=2{self.invalid=true;return None;}
            let mut fields=data.split_ascii_whitespace();
            let sample=fields.next().and_then(|s|s.parse::<u64>().ok()).zip(fields.next().and_then(|s|s.parse::<u64>().ok()));
            if let Some((written,total))=sample{
                if fields.next().is_none()&&total>0&&written<=total{
                    if self.last_progress.is_none_or(|(last,count)|total==count&&written>=last){
                        self.last_progress=Some((written,total));
                        return Some(BurnStage::WritingProgress{written_sectors:written,total_sectors:total});
                    }
                }
            }
            // Optional numerical telemetry can be dropped without changing write outcome.
            return None;
        }
        let expected=["DAS_STAGE CHECKING_MEDIA","DAS_STAGE WRITING","DAS_STAGE RELEASING","WRITE_COMPLETED"];
        if expected.get(self.next).copied()!=Some(line){self.invalid=true;return None;}
        self.next+=1;
        parse_stage(line)
    }
    fn completed(&self)->bool{!self.invalid&&self.next==4}
}
#[cfg(windows)]
fn burn(manifest:&crate::image::PreparedDisc,image:&std::path::Path,id:&str,hash:&str,confirmation:&str,progress:&mut dyn FnMut(BurnStage))->Result<BurnReceipt>{
    use anyhow::Context;
    use std::{os::windows::fs::OpenOptionsExt,process::Command};
    validate_authorization(id,hash,confirmation)?;
    let image=image.canonicalize()?;
    // Deny modification and deletion until the helper has completed.
    let guard=std::fs::OpenOptions::new().read(true).share_mode(1).open(&image)?;
    progress(BurnStage::VerifyingImage);
    let verified=crate::image::verify_udf(manifest,&image)?;
    if !verified.sha256.eq_ignore_ascii_case(hash){bail!("Image differs from explicitly approved checksum");}
    progress(BurnStage::InspectingRecorder);
    let discovery=discover()?;
    let media=eligible(&discovery,id,verified.image_bytes)?;
    let work=tempfile::Builder::new().prefix("das-burn-").tempdir()?;
    let script=work.path().join("burn.ps1");
    std::fs::write(&script,include_str!("imapi_burn.ps1"))?;
    use std::io::{BufRead,BufReader,Read};
    use std::process::Stdio;
    let mut child=Command::new("powershell.exe")
        .args(["-NoLogo","-NoProfile","-NonInteractive","-ExecutionPolicy","Bypass","-File"])
        .arg(script).env("DAS_BURN_IMAGE",&image).env("DAS_BURN_ID",id)
        .env("DAS_BURN_HASH",hash).env("DAS_BURN_BYTES",verified.image_bytes.to_string())
        .env("DAS_BURN_MEDIA",media.physical_type.to_string())
        .env("DAS_BURN_CONFIRM",confirmation)
        .stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().context("Starting experimental writer; if interrupted, inspect media before retrying")?;
    let stderr=child.stderr.take().context("Missing writer stderr")?;
    // Drain stderr concurrently to prevent pipe deadlock during the physical write.
    let drain=std::thread::spawn(move||{let mut data=Vec::new();let mut reader=stderr;let result=reader.read_to_end(&mut data);(result,data)});
    let stdout=child.stdout.take().context("Missing writer stdout")?;
    let mut protocol=WriterProtocol::default();
    let mut read_error=None;
    for line in BufReader::new(stdout).lines(){
        match line{
            Ok(line)=>{if let Some(stage)=protocol.observe(&line){progress(stage);}}
            Err(e)=>{read_error=Some(e);break;}
        }
    }
    let status=child.wait()?;
    let (stderr_result,stderr)=drain.join().map_err(|_|anyhow::anyhow!("Writer diagnostics worker failed"))?;
    drop(guard);
    stderr_result?;
    if !status.success(){bail!("Writer failed; media may be partially written. Do not retry automatically: {}",String::from_utf8_lossy(&stderr));}
    if !protocol.completed()||read_error.is_some(){bail!("Unknown write outcome; inspect media before retrying");}
    Ok(BurnReceipt{recorder_id:id.into(),image_sha256:verified.sha256,image_bytes:verified.image_bytes,status:"written_unverified".into()})
}
#[cfg(not(windows))]
fn burn(_manifest:&crate::image::PreparedDisc,_image:&std::path::Path,id:&str,hash:&str,confirmation:&str,_progress:&mut dyn FnMut(BurnStage))->Result<BurnReceipt>{
    validate_authorization(id,hash,confirmation)?;
    bail!("Native physical writing requires Windows")
}
#[cfg(test)]
mod burn_tests{
    use super::*;
    #[test]fn writer_protocol_requires_ordered_lifecycle(){
        let valid=["DAS_STAGE CHECKING_MEDIA","DAS_STAGE WRITING","DAS_STAGE RELEASING","WRITE_COMPLETED"];
        let mut p=WriterProtocol::default();
        for line in valid {p.observe(line);}
        assert!(p.completed());
        for lines in [
            vec!["WRITE_COMPLETED"],
            vec![valid[0],valid[2],valid[1],valid[3]],
            vec![valid[0],valid[1],valid[1],valid[2],valid[3]],
            vec![valid[0],valid[1],valid[2]],
            vec![valid[0],valid[1],valid[2],valid[3],"unexpected"],
            vec![valid[0],valid[1],valid[2],valid[3],valid[3]],
        ] {
            let mut p=WriterProtocol::default();
            for line in lines {p.observe(line);}
            assert!(!p.completed());
        }
    }
    #[test]fn detailed_progress_does_not_replace_lifecycle(){
        let mut p=WriterProtocol::default();
        p.observe("DAS_STAGE CHECKING_MEDIA");p.observe("DAS_STAGE WRITING");
        assert_eq!(p.observe("DAS_PROGRESS 25 100"),Some(BurnStage::WritingProgress{written_sectors:25,total_sectors:100}));
        for line in ["DAS_PROGRESS 24 100","DAS_PROGRESS 101 100","DAS_PROGRESS 30 200","DAS_PROGRESS bad 100","DAS_PROGRESS 0 0"]{assert_eq!(p.observe(line),None);}
        assert_eq!(p.observe("DAS_PROGRESS 100 100"),Some(BurnStage::WritingProgress{written_sectors:100,total_sectors:100}));
        assert!(!p.completed());
        p.observe("DAS_STAGE RELEASING");p.observe("WRITE_COMPLETED");assert!(p.completed());
        let mut p=WriterProtocol::default();p.observe("DAS_PROGRESS 1 2");
        for line in ["DAS_STAGE CHECKING_MEDIA","DAS_STAGE WRITING","DAS_STAGE RELEASING","WRITE_COMPLETED"]{p.observe(line);}
        assert!(!p.completed());
    }
    #[cfg(windows)]
    #[test]fn native_telemetry_compiles_without_hardware(){
        let tmp=tempfile::tempdir().unwrap();let script=tmp.path().join("burn.ps1");
        std::fs::write(&script,include_str!("imapi_burn.ps1")).unwrap();
        let result=std::process::Command::new("powershell.exe").args(["-NoProfile","-NonInteractive","-ExecutionPolicy","Bypass","-File"]).arg(script).arg("-TelemetrySelfTest").output().unwrap();
        assert!(result.status.success(),"{}",String::from_utf8_lossy(&result.stderr));
        assert_eq!(String::from_utf8_lossy(&result.stdout).trim(),"TELEMETRY_SELFTEST_PASSED");
    }
    #[test]fn explicit_authorization(){
        let hash="a".repeat(64);
        assert!(validate_authorization("drive",&hash,"WRITE-ONCE").is_ok());
        assert!(validate_authorization("",&hash,"WRITE-ONCE").is_err());
        assert!(validate_authorization("drive","bad","WRITE-ONCE").is_err());
        assert!(validate_authorization("drive",&hash,"yes").is_err());
    }
    #[cfg(windows)]
    #[test]fn writer_helper_rejects_missing_confirmation(){
        let tmp=tempfile::tempdir().unwrap();let script=tmp.path().join("burn.ps1");
        std::fs::write(&script,include_str!("imapi_burn.ps1")).unwrap();
        let result=std::process::Command::new("powershell.exe").args(["-NoProfile","-NonInteractive","-ExecutionPolicy","Bypass","-File"]).arg(script)
            .env("DAS_BURN_CONFIRM","").output().unwrap();
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("Explicit WRITE-ONCE confirmation required"));
    }
}

pub fn burn_logged_with_progress(journal:&crate::jobs::Journal,manifest:&crate::image::PreparedDisc,image:&std::path::Path,id:&str,hash:&str,confirmation:&str,progress:&mut dyn FnMut(BurnStage))->Result<BurnReceipt>{
    validate_authorization(id,hash,confirmation)?;
    let image=image.canonicalize()?;
    let _guard=crate::jobs::RecorderGuard::acquire(id)?;
    let attempt=journal.begin(id,&image,hash,manifest.number,&manifest.label)?;
    journal.bind_manifest(attempt,manifest)?;
    match burn(manifest,&image,id,hash,confirmation,progress){
        Ok(receipt)=>{journal.finish(attempt,true,"Native writer returned successfully; physical readback pending")?;progress(BurnStage::WrittenUnverified);Ok(receipt)}
        Err(e)=>{journal.finish(attempt,false,&format!("{e:#}"))?;Err(e)}
    }
}

pub fn burn_logged(journal:&crate::jobs::Journal,manifest:&crate::image::PreparedDisc,image:&std::path::Path,id:&str,hash:&str,confirmation:&str)->Result<BurnReceipt>{
    burn_logged_with_progress(journal,manifest,image,id,hash,confirmation,&mut |_|{})
}
#[cfg(test)]
#[test]fn stage_protocol(){
    assert_eq!(parse_stage("DAS_STAGE WRITING"),Some(BurnStage::WritingAndFinalizing));
    assert_eq!(parse_stage("DAS_STAGE CHECKING_MEDIA"),Some(BurnStage::CheckingMedia));
    assert_eq!(parse_stage("DAS_STAGE RELEASING"),Some(BurnStage::ReleasingRecorder));
    assert_eq!(parse_stage("WRITE_COMPLETED"),None);
    assert_eq!(parse_stage("DAS_STAGE UNKNOWN"),None);
}


#[derive(Debug,Serialize)]
pub struct DiscReadback{
    pub recorder_id:String,pub disc_number:usize,pub label:String,pub files:usize,pub payload_bytes:u64,
    pub status:String,
}
#[cfg(windows)]
pub fn verify_disc(manifest:&crate::image::PreparedDisc,id:&str)->Result<DiscReadback>{
    use anyhow::Context;
    crate::image::validate_manifest(manifest)?;
    let _guard=crate::jobs::RecorderGuard::acquire(id)?;
    let payload_bytes=manifest.files.iter().try_fold(0u64,|sum,f|sum.checked_add(f.bytes).context("Payload size overflow"))?;
    let work=tempfile::Builder::new().prefix("das-readback-").tempdir()?;
    let script=work.path().join("readback.ps1");
    let path=work.path().join("manifest.json");
    std::fs::write(&script,include_str!("imapi_readback.ps1"))?;
    std::fs::write(&path,serde_json::to_vec(manifest)?)?;
    let result=std::process::Command::new("powershell.exe")
        .args(["-NoLogo","-NoProfile","-NonInteractive","-ExecutionPolicy","Bypass","-File"])
        .arg(script).env("DAS_READBACK_ID",id).env("DAS_READBACK_MANIFEST",path)
        .output().context("Starting physical readback verifier")?;
    if !result.status.success(){bail!("Physical readback failed: {}",String::from_utf8_lossy(&result.stderr));}
    if String::from_utf8_lossy(&result.stdout).trim()!="READBACK_COMPLETED"{bail!("Unknown readback outcome");}
    Ok(DiscReadback{recorder_id:id.into(),disc_number:manifest.number,label:manifest.label.clone(),files:manifest.files.len(),payload_bytes,status:"content_verified".into()})
}
#[cfg(not(windows))]
pub fn verify_disc(_manifest:&crate::image::PreparedDisc,_id:&str)->Result<DiscReadback>{bail!("Physical readback requires Windows")}
#[cfg(test)]
mod readback_tests{
    use super::*;
    #[test]fn malformed_manifest_rejected(){
        let p=crate::image::PreparedDisc{number:1,label:"DISC_001".into(),capacity:10000,estimated_used:0,reserve:0,files:vec![]};
        assert!(verify_disc(&p,"").is_err());
    }
}

pub fn verify_logged_disc(journal:&crate::jobs::Journal,attempt:i64,recorder:&str)->Result<DiscReadback>{
    let manifest=journal.verification_manifest(attempt)?;
    match verify_disc(&manifest,recorder){
        Ok(receipt)=>{journal.record_readback(attempt,recorder,true,&serde_json::to_string(&receipt)?)?;Ok(receipt)}
        Err(e)=>{journal.record_readback(attempt,recorder,false,&format!("{e:#}"))?;Err(e)}
    }
}
