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
