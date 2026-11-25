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
