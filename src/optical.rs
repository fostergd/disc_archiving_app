//! Read-only recorder discovery. No exclusive access, eject, erase or write.
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
    // IMAPI physical media enum: CD-R=2, DVD+R=5, DVD+R DL=8,
    // DVD-R=9, DVD-R DL=10, BD-R=18. Unknown/rewriteable media fail closed.
    pub fn type_name(&self)->&'static str{
        match self.physical_type{2=>"CD-R",5=>"DVD+R",8=>"DVD+R DL",9=>"DVD-R",10=>"DVD-R DL",18=>"BD-R",_=>"Other / unsupported"}
    }
    pub fn write_once(&self)->bool{matches!(self.physical_type,2|5|8|9|10|18)}
    pub fn blank_candidate(&self)->bool{
        self.supported&&self.write_once()&&self.physically_blank&&self.heuristically_blank
        &&self.free_sectors>0&&self.total_sectors>=self.free_sectors
        &&matches!(self.status,0|2) // unknown or blank only; preliminary inspection
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
    fn media(kind:i32)->Media{Media{physical_type:kind,supported:true,physically_blank:true,heuristically_blank:true,status:0,free_sectors:100,total_sectors:100}}
    #[test]fn media_policy(){
        for n in [2,5,8,9,10,18]{assert!(media(n).blank_candidate());}
        for n in [0,1,3,4,6,7,11,12,13,14,15,16,17,19,20,21,22]{assert!(!media(n).blank_candidate());}
        let mut m=media(18);m.physically_blank=false;assert!(!m.blank_candidate());
        let mut m=media(18);m.free_sectors=-1;assert_eq!(m.free_bytes(),None);assert!(!m.blank_candidate());
        let mut m=media(18);m.status=0x8000;assert!(!m.blank_candidate());
        let m=media(18);assert_eq!(m.free_bytes(),Some(204800));
        assert!(m.fits_image(204800));assert!(!m.fits_image(206848));assert!(!m.fits_image(1));assert!(!m.fits_image(0));
        for status in [1,4,8,16,0x4000,0x8000,-1]{let mut m=media(18);m.status=status;assert!(!m.fits_image(2048));}
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
#[cfg(windows)]
pub fn burn(manifest:&crate::image::PreparedDisc,image:&std::path::Path,id:&str,hash:&str,confirmation:&str)->Result<BurnReceipt>{
    use anyhow::Context;
    use std::{os::windows::fs::OpenOptionsExt,process::Command};
    validate_authorization(id,hash,confirmation)?;
    let image=image.canonicalize()?;
    // Deny modification and deletion until the helper has completed.
    let guard=std::fs::OpenOptions::new().read(true).share_mode(1).open(&image)?;
    let verified=crate::image::verify_udf(manifest,&image)?;
    if !verified.sha256.eq_ignore_ascii_case(hash){bail!("Image differs from explicitly approved checksum");}
    let discovery=discover()?;
    let media=eligible(&discovery,id,verified.image_bytes)?;
    let work=tempfile::Builder::new().prefix("das-burn-").tempdir()?;
    let script=work.path().join("burn.ps1");
    std::fs::write(&script,include_str!("imapi_burn.ps1"))?;
    let result=Command::new("powershell.exe")
        .args(["-NoLogo","-NoProfile","-NonInteractive","-ExecutionPolicy","Bypass","-File"])
        .arg(script).env("DAS_BURN_IMAGE",&image).env("DAS_BURN_ID",id)
        .env("DAS_BURN_HASH",hash).env("DAS_BURN_BYTES",verified.image_bytes.to_string())
        .env("DAS_BURN_MEDIA",media.physical_type.to_string())
        .env("DAS_BURN_CONFIRM",confirmation)
        .output().context("Starting experimental writer; if interrupted, inspect media before retrying")?;
    drop(guard);
    if !result.status.success(){bail!("Writer failed; media may be partially written. Do not retry automatically: {}",String::from_utf8_lossy(&result.stderr));}
    if String::from_utf8_lossy(&result.stdout).trim()!="WRITE_COMPLETED"{bail!("Unknown write outcome; inspect media before retrying");}
    Ok(BurnReceipt{recorder_id:id.into(),image_sha256:verified.sha256,image_bytes:verified.image_bytes,status:"written_unverified".into()})
}
#[cfg(not(windows))]
pub fn burn(_manifest:&crate::image::PreparedDisc,_image:&std::path::Path,id:&str,hash:&str,confirmation:&str)->Result<BurnReceipt>{
    validate_authorization(id,hash,confirmation)?;
    bail!("Native physical writing requires Windows")
}
#[cfg(test)]
mod burn_tests{
    use super::*;
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

pub fn burn_logged(journal:&crate::jobs::Journal,manifest:&crate::image::PreparedDisc,image:&std::path::Path,id:&str,hash:&str,confirmation:&str)->Result<BurnReceipt>{
    validate_authorization(id,hash,confirmation)?;
    let image=image.canonicalize()?;
    let attempt=journal.begin(id,&image,hash,manifest.number,&manifest.label)?;
    match burn(manifest,&image,id,hash,confirmation){
        Ok(receipt)=>{journal.finish(attempt,true,"Native writer returned successfully; physical readback pending")?;Ok(receipt)}
        Err(e)=>{journal.finish(attempt,false,&format!("{e:#}"))?;Err(e)}
    }
}
