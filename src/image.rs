//! Read-only preflight for optical image generation. No media is written.
use super::*;
use serde::Serialize;

#[derive(Debug,Serialize)]
pub struct PreparedFile {
    pub source:PathBuf,
    pub archive:String,
    pub bytes:u64,
    pub sha256:String,
}
#[derive(Debug,Serialize)]
pub struct PreparedDisc {
    pub number:usize,
    pub label:String,
    pub capacity:u64,
    pub estimated_used:u64,
    pub reserve:u64,
    pub files:Vec<PreparedFile>,
}
fn safe_component(s:&str)->bool {
    if s.is_empty()||s=="."||s==".."||s.ends_with('.')||s.ends_with(' ')||s.encode_utf16().count()>127{return false;}
    if s.chars().any(|c|c.is_control()||"<>:\"\\|?*".contains(c)){return false;}
    let stem=s.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    if ["CON","PRN","AUX","NUL","CONIN$","CONOUT$"].contains(&stem.as_str()){return false;}
    if let Some(n)=stem.strip_prefix("COM").or_else(||stem.strip_prefix("LPT")){
        if ["1","2","3","4","5","6","7","8","9","¹","²","³"].contains(&n){return false;}
    }
    true
}
fn check_paths(files:&[Entry])->Result<()>{
    let mut leaf=HashSet::new();let mut dirs:HashMap<String,String>=HashMap::new();
    for f in files{
        if f.archive.split('/').any(|s|!safe_component(s)){bail!("Unsupported optical archive path: {}",f.archive);}
        let key=f.archive.to_lowercase();
        if !leaf.insert(key){bail!("Duplicate optical archive path: {}",f.archive);}
        for (i,_) in f.archive.match_indices('/'){
            let path=&f.archive[..i];let key=path.to_lowercase();
            if let Some(prior)=dirs.insert(key,path.to_owned()){
                if prior!=path{bail!("Conflicting directory spelling: {prior} and {path}");}
            }
        }
    }
    if let Some(key)=leaf.iter().find(|s|dirs.contains_key(*s)){bail!("File/directory collision: {key}");}
    Ok(())
}
// This snapshot checks bytes and hashes at preflight time, not during a subsequent image write.
pub fn prepare(p:&Plan,number:usize)->Result<PreparedDisc>{
    editing::validate(p)?;
    let d=p.discs.iter().find(|d|d.number==number).context("Disc not found")?;
    if d.files.is_empty(){bail!("Cannot prepare an empty disc");}
    check_paths(&d.files)?;
    let mut files=Vec::with_capacity(d.files.len());
    for f in &d.files {
        let before=std::fs::symlink_metadata(&f.source).with_context(||format!("Source unavailable: {}",f.source.display()))?;
        if redirected(&before)||!before.is_file()||before.len()!=f.bytes{bail!("Source type or size changed: {}",f.archive);}
        let before_time=before.modified()?;
        let sha256=digest(&f.source).with_context(||format!("Hashing {}",f.archive))?;
        let after=std::fs::symlink_metadata(&f.source)?;
        if redirected(&after)||!after.is_file()||after.len()!=f.bytes||after.modified()?!=before_time{bail!("Source changed during hashing: {}",f.archive);}
        if let Some(expected)=&f.sha256{if !expected.eq_ignore_ascii_case(&sha256){bail!("Source checksum changed: {}",f.archive);}}
        files.push(PreparedFile{source:f.source.clone(),archive:f.archive.clone(),bytes:f.bytes,sha256});
    }
    Ok(PreparedDisc{number:d.number,label:d.label.clone(),capacity:d.capacity,estimated_used:d.estimated_used,reserve:p.reserve,files})
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn preflight_hashes_and_detects_changes(){
        let tmp=tempfile::tempdir().unwrap();let src=tmp.path().join("movie.mkv");
        std::fs::write(&src,b"abcd").unwrap();
        let f=Entry{source:src.clone(),archive:"Movies/movie.mkv".into(),bytes:4,sha256:Some(digest(&src).unwrap())};
        let p=plan(&[f],50000,0,Strategy::Sequential).unwrap();
        let prepared=prepare(&p,1).unwrap();assert_eq!(prepared.files[0].sha256,digest(&src).unwrap());
        std::fs::write(&src,b"wxyz").unwrap();assert!(prepare(&p,1).is_err());
        std::fs::remove_file(&src).unwrap();assert!(prepare(&p,1).is_err());
    }
    #[test]fn reject_unsafe_optical_paths(){
        let f=|name:&str|Entry{source:"unused".into(),archive:name.into(),bytes:0,sha256:None};
        for name in ["Media/CON.txt","Media/bad?.txt","Media/trailing.","Media/bad:stream"]{assert!(check_paths(&[f(name)]).is_err());}
        assert!(check_paths(&[f("Media/A"),f("Media/a/B")]).is_err());
        assert!(check_paths(&[f("Media/A/one"),f("Media/a/two")]).is_err());
        assert!(check_paths(&[f("Media/音楽.flac")]).is_ok());
    }
}


#[derive(Debug,Serialize)]
pub struct ImageReceipt {
    pub number:usize,pub label:String,pub bytes:u64,pub sha256:String,pub filesystem:String,
    pub output:PathBuf,
}

#[cfg(windows)]
pub fn create_udf(p:&Plan,number:usize,output:&Path)->Result<ImageReceipt>{
    use std::io::Write;
    use std::process::Command;
    if output.exists(){bail!("Image output already exists");}
    let prepared=prepare(p,number)?;
    let parent=output.parent().filter(|p|!p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let parent=parent.canonicalize()?;
    let filename=output.file_name().context("Invalid image filename")?;
    let final_path=parent.join(filename);
    let work=tempfile::Builder::new().prefix("das-image-").tempdir_in(&parent)?;
    let stage=work.path().join("stage");
    std::fs::create_dir(&stage)?;
    for f in &prepared.files{
        let dest=stage.join(f.archive.replace('/',"\\"));
        std::fs::create_dir_all(dest.parent().context("Invalid staging path")?)?;
        let mut input=File::open(&f.source)?;
        let mut target=File::options().write(true).create_new(true).open(&dest)?;
        let mut hash=Sha256::new();
        let mut copied=0u64;
        let mut buf=vec![0u8;1024*1024];
        loop{
            let n=input.read(&mut buf)?;
            if n==0{break;}
            copied=copied.checked_add(n as u64).context("Size overflow")?;
            if copied>f.bytes{bail!("Source grew during staging: {}",f.archive);}
            target.write_all(&buf[..n])?;
            hash.update(&buf[..n]);
        }
        if copied!=f.bytes||format!("{:x}",hash.finalize())!=f.sha256{bail!("Source changed during staging: {}",f.archive);}
        target.sync_all()?;
    }
    // The script is embedded in the binary and receives data via environment variables.
    // No user path or label is interpolated into PowerShell source.
    let script=work.path().join("build.ps1");
    std::fs::write(&script,include_str!("imapi_image.ps1"))?;
    let image=work.path().join("image.iso");
    let usable=prepared.capacity.checked_sub(prepared.reserve).context("Invalid reserve")?;
    let result=Command::new("powershell.exe")
        .args(["-NoLogo","-NoProfile","-NonInteractive","-ExecutionPolicy","Bypass","-File"])
        .arg(&script)
        .env("DAS_STAGE",&stage).env("DAS_IMAGE",&image)
        .env("DAS_LABEL",&prepared.label).env("DAS_MAX_BLOCKS",(usable/2048).to_string())
        .output().context("Starting Windows IMAPI image helper")?;
    if !result.status.success(){bail!("IMAPI image generation failed: {}",String::from_utf8_lossy(&result.stderr));}
    let bytes=std::fs::metadata(&image)?.len();
    if bytes==0||bytes%2048!=0||bytes>usable{bail!("Generated image has invalid size or exceeds usable capacity: {bytes}");}
    // A hard link publishes the already flushed image without a second full copy.
    // Windows CreateHardLink fails if the destination exists. Unsupported
    // destination filesystems fail closed rather than replacing anything.
    let sha256=digest(&image)?;
    std::fs::hard_link(&image,&final_path).context("Publishing image requires hard-link support on the destination filesystem (NTFS recommended)")?;
    Ok(ImageReceipt{number,label:prepared.label,bytes,sha256,filesystem:"UDF 2.01".into(),output:final_path})
}
#[cfg(not(windows))]
pub fn create_udf(_p:&Plan,_number:usize,_output:&Path)->Result<ImageReceipt>{
    bail!("Native IMAPI image creation requires Windows");
}

#[cfg(all(test,windows))]
mod native_tests{
    use super::*;
    #[test]
    fn native_udf_image_smoke(){
        let tmp=tempfile::tempdir().unwrap();
        let src=tmp.path().join("source.bin");
        let payload:Vec<u8>=(0..65536).map(|i|(i%251) as u8).collect();
        std::fs::write(&src,&payload).unwrap();
        let empty=tmp.path().join("empty.bin");std::fs::write(&empty,[]).unwrap();
        let entries=vec![
            Entry{source:src.clone(),archive:"Media/音楽.bin".into(),bytes:payload.len() as u64,sha256:Some(digest(&src).unwrap())},
            Entry{source:empty,archive:"Media/empty.bin".into(),bytes:0,sha256:None}
        ];
        let p=plan(&entries,64*1024*1024,1024*1024,Strategy::Sequential).unwrap();
        let output=tmp.path().join("DISC_001.iso");
        let receipt=create_udf(&p,1,&output).expect("Native Windows IMAPI image generation");
        assert_eq!(receipt.bytes,std::fs::metadata(&output).unwrap().len());
        assert_eq!(receipt.sha256,digest(&output).unwrap());
        assert_eq!(receipt.bytes%2048,0);
        assert!(receipt.bytes<=p.discs[0].capacity-p.reserve);
        let image=std::fs::read(&output).unwrap();
        assert!(image.windows(5).any(|w|w==b"NSR03"),"Missing UDF volume recognition marker");
        assert!(image.windows(32).enumerate().any(|(i,w)|w==&payload[..32] && image.get(i..i+payload.len())==Some(payload.as_slice())),"Missing exact fixture bytes");
        assert!(create_udf(&p,1,&output).is_err());
        assert_eq!(receipt.sha256,digest(&output).unwrap());
        assert_eq!(std::fs::read(&src).unwrap(),payload);
        assert!(!std::fs::read_dir(tmp.path()).unwrap().any(|e|e.unwrap().file_name().to_string_lossy().starts_with("das-image-")));
    }
}
