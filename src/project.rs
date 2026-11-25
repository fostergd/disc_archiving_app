use super::*;
use std::io::{BufReader,BufWriter,Write};

pub const PROJECT_VERSION:u32=1;
#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct Project {
    pub version:u32,
    pub name:String,
    pub sources:Vec<PathBuf>,
    pub extensions:Vec<String>,
    pub exclusions:Vec<String>,
    pub hash:bool,
    pub capacities:Vec<u64>,
    pub reserve:u64,
    pub strategy:Strategy,
    pub naming_pattern:String,
    pub inventory:Option<Inventory>,
    pub plan:Option<Plan>,
}
impl Default for Project {
    fn default()->Self{Self{version:PROJECT_VERSION,name:"Untitled archive".into(),sources:vec![],extensions:vec![],exclusions:vec![],hash:false,capacities:vec![50_000_000_000],reserve:50_000_000,strategy:Strategy::Hybrid,naming_pattern:"DISC_{n:03}".into(),inventory:None,plan:None}}
}
impl Project {
    pub fn validate(&self)->Result<()>{
        if self.version!=PROJECT_VERSION{bail!("Unsupported project version {}",self.version);}
        if self.name.trim().is_empty(){bail!("Project name cannot be empty");}
        if self.capacities.is_empty()||self.capacities.iter().any(|&c|c<=self.reserve){bail!("Invalid project capacities");}
        if let Some(p)=&self.plan{
            editing::validate(p)?;
            if p.reserve!=self.reserve||p.strategy!=self.strategy{bail!("Plan settings disagree with project settings");}
            if let Some(i)=&self.inventory{
                if !i.errors.is_empty(){bail!("Planned inventory contains scan errors");}
                let mut inventory=HashMap::new();
                for f in &i.files{if inventory.insert(f.archive.as_str(),f).is_some(){bail!("Repeated inventory entry");}}
                for f in p.discs.iter().flat_map(|d|&d.files).chain(&p.oversized){
                    if inventory.remove(f.archive.as_str())!=Some(f){bail!("Plan entry differs from inventory: {}",f.archive);}
                }
                if !inventory.is_empty(){bail!("Plan omits inventory files");}
            }
        }
        Ok(())
    }
}
// Write to a temporary sibling, flush it, and publish without overwriting.
// A failed save never replaces an earlier project.
pub fn save(path:&Path,p:&Project)->Result<()>{
    p.validate()?;
    let parent=path.parent().filter(|p|!p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let mut tmp=tempfile::NamedTempFile::new_in(parent)?;
    {
        let mut writer=BufWriter::new(tmp.as_file_mut());
        serde_json::to_writer_pretty(&mut writer,p)?;
        writer.flush()?;
    }
    tmp.as_file().sync_all()?;
    tmp.persist_noclobber(path).map_err(|e|e.error)?;
    Ok(())
}
pub fn load(path:&Path)->Result<Project>{
    let p:Project=serde_json::from_reader(BufReader::new(File::open(path)?))?;
    p.validate()?;Ok(p)
}
#[cfg(test)]
mod tests{
    use super::*;
    fn sample()->Project{
        let files=vec![Entry{source:"source.mkv".into(),archive:"Media/source.mkv".into(),bytes:20000,sha256:None}];
        let mut p=Project::default();
        p.inventory=Some(Inventory{files:files.clone(),..Default::default()});
        p.plan=Some(plan_mixed(&files,&p.capacities,p.reserve,p.strategy).unwrap());
        editing::set_lock(p.plan.as_mut().unwrap(),1,true).unwrap();p
    }
    #[test]fn roundtrip_and_no_clobber(){
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("archive.das.json");
        let p=sample();save(&path,&p).unwrap();let before=std::fs::read(&path).unwrap();
        let q=load(&path).unwrap();assert_eq!(serde_json::to_value(&p).unwrap(),serde_json::to_value(&q).unwrap());
        assert!(save(&path,&Project::default()).is_err());assert_eq!(std::fs::read(&path).unwrap(),before);
    }
    #[test]fn reject_omitted_files_and_unknown_version(){
        let mut p=sample();p.plan.as_mut().unwrap().discs.clear();assert!(p.validate().is_err());
        let mut p=sample();p.version+=1;assert!(p.validate().is_err());
    }
}
