use anyhow::{bail, Context, Result};
use globset::GlobSetBuilder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{cmp::Ordering, collections::{HashMap, HashSet}, fs::File, io::Read, path::{Path, PathBuf}};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entry {
    pub source: PathBuf,
    pub archive: String,
    pub bytes: u64,
    pub sha256: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Inventory {
    pub files: Vec<Entry>,
    pub excluded: Vec<String>,
    pub errors: Vec<String>,
}
#[derive(Default)]
pub struct ScanOptions {
    pub extensions: Vec<String>,
    pub exclude: Vec<String>,
    pub hash: bool,
}

// Compare arbitrary-length ASCII digit runs numerically without integer overflow.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    natural_cmp_folded(a,b,&a.to_lowercase(),&b.to_lowercase())
}
fn natural_cmp_folded(a:&str,b:&str,aa:&str,bb:&str)->Ordering {
    let (mut x, mut y) = (aa.as_bytes(), bb.as_bytes());
    while !x.is_empty() && !y.is_empty() {
        if x[0].is_ascii_digit() && y[0].is_ascii_digit() {
            let nx = x.iter().take_while(|c| c.is_ascii_digit()).count();
            let ny = y.iter().take_while(|c| c.is_ascii_digit()).count();
            let dx = &x[..nx]; let dy = &y[..ny];
            let sx = &dx[dx.iter().take_while(|&&c| c == b'0').count()..];
            let sy = &dy[dy.iter().take_while(|&&c| c == b'0').count()..];
            let o = sx.len().cmp(&sy.len()).then_with(|| sx.cmp(sy));
            if o != Ordering::Equal { return o; }
            x = &x[nx..]; y = &y[ny..];
        } else {
            let o = x[0].cmp(&y[0]);
            if o != Ordering::Equal { return o; }
            x = &x[1..]; y = &y[1..];
        }
    }
    x.len().cmp(&y.len()).then_with(|| a.cmp(b))
}

// Cache case-folded keys once per sort rather than allocating on every comparison.
fn sort_entries(files:&mut Vec<Entry>) {
    let mut keyed:Vec<_>=std::mem::take(files).into_iter().map(|f|(f.archive.to_lowercase(),f)).collect();
    keyed.sort_by(|(ka,a),(kb,b)|natural_cmp_folded(&a.archive,&b.archive,ka,kb));
    *files=keyed.into_iter().map(|(_,f)|f).collect();
}

pub fn digest(path: &Path) -> Result<String> {
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop { let n = f.read(&mut buf)?; if n == 0 { break; } h.update(&buf[..n]); }
    Ok(format!("{:x}", h.finalize()))
}

// Reparse points include junctions and other Windows filesystem redirects.
fn redirected(metadata:&std::fs::Metadata)->bool {
    if metadata.file_type().is_symlink(){return true;}
    #[cfg(windows)]
    { use std::os::windows::fs::MetadataExt; metadata.file_attributes() & 0x400 != 0 }
    #[cfg(not(windows))]
    { false }
}
fn archive_path(root:&Path,path:&Path)->Result<String> {
    let relative=path.strip_prefix(root)?;
    let prefix=if let Some(name)=root.file_name(){name.to_str().context("Non-Unicode source name")?.to_owned()}
    else {
        #[cfg(windows)]
        {
            use std::path::{Component,Prefix};
            match root.components().next(){
                Some(Component::Prefix(p))=>match p.kind(){
                    Prefix::Disk(d)|Prefix::VerbatimDisk(d)=>format!("Drive_{}",d as char),
                    Prefix::UNC(_,share)|Prefix::VerbatimUNC(_,share)=>share.to_str().context("Non-Unicode share name")?.to_owned(),
                    _=>bail!("Unsupported volume root: {}",root.display()),
                },
                _=>bail!("Unsupported volume root: {}",root.display()),
            }
        }
        #[cfg(not(windows))]
        {"Root".to_owned()}
    };
    let mut parts=vec![prefix];
    for c in relative.components(){parts.push(c.as_os_str().to_str().context("Non-Unicode filename cannot be represented safely")?.to_owned());}
    Ok(parts.join("/"))
}
pub fn scan(roots:&[PathBuf], options:&ScanOptions)->Result<Inventory>{
    let mut builder=GlobSetBuilder::new();
    for pattern in &options.exclude{
        builder.add(globset::GlobBuilder::new(&pattern.replace('\\',"/")).case_insensitive(true).literal_separator(true).build()?);
    }
    let excludes=builder.build()?;
    let extensions:HashSet<_>=options.extensions.iter().map(|e|e.trim_start_matches('.').to_lowercase()).collect();
    let mut inventory=Inventory::default();
    let mut canonical=Vec::new();
    for input in roots{
        let metadata=std::fs::symlink_metadata(input).with_context(||format!("Source unavailable: {}",input.display()))?;
        if redirected(&metadata){inventory.excluded.push(format!("Redirected source: {}",input.display()));continue;}
        let root=input.canonicalize().with_context(||format!("Source unavailable: {}",input.display()))?;
        canonical.push(root);
    }
    // Parent selections take precedence regardless of input order.
    canonical.sort_by_key(|p|p.components().count());
    let mut selected:Vec<PathBuf>=vec![];
    for root in canonical{
        if selected.iter().any(|p|root.starts_with(p)){
            inventory.excluded.push(format!("Overlapping source: {}",root.display()));
        }else{selected.push(root);}
    }
    let mut paths=HashSet::new();
    for root in selected{
        let mut walker=WalkDir::new(&root).follow_links(false).into_iter();
        while let Some(item)=walker.next(){
            let item=match item{Ok(v)=>v,Err(e)=>{inventory.errors.push(e.to_string());continue;}};
            let archive=archive_path(&root,item.path())?;
            let metadata=match std::fs::symlink_metadata(item.path()){
                Ok(m)=>m,Err(e)=>{if item.file_type().is_dir(){walker.skip_current_dir();} inventory.errors.push(format!("{archive}: {e}"));continue;}
            };
            if redirected(&metadata)||excludes.is_match(&archive){
                if item.file_type().is_dir(){walker.skip_current_dir();}
                inventory.excluded.push(archive);continue;
            }
            if !metadata.is_file(){continue;}
            if !extensions.is_empty()&&!item.path().extension().and_then(|s|s.to_str()).map(|s|extensions.contains(&s.to_lowercase())).unwrap_or(false){
                inventory.excluded.push(archive);continue;
            }
            if !paths.insert(archive.to_lowercase()){bail!("Archive path collision: {archive}");}
            let result=(||->Result<Entry>{
                let sha256=if options.hash{Some(digest(item.path())?)}else{None};
                Ok(Entry{source:item.path().to_owned(),archive:archive.clone(),bytes:metadata.len(),sha256})
            })();
            match result{Ok(e)=>inventory.files.push(e),Err(e)=>inventory.errors.push(format!("{archive}: {e:#}"))}
        }
    }
    sort_entries(&mut inventory.files);
    inventory.excluded.sort();inventory.errors.sort();
    Ok(inventory)
}

pub fn duplicates(files: &[Entry]) -> Vec<Vec<String>> {
    let mut groups: HashMap<(u64, &str), Vec<String>> = HashMap::new();
    for f in files { if let Some(h) = &f.sha256 { groups.entry((f.bytes,h)).or_default().push(f.archive.clone()); } }
    let mut groups: Vec<_> = groups.into_values().filter(|v| v.len()>1).collect();
    groups.sort(); groups
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Strategy { Sequential, Folder, Maximum, Hybrid }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Disc {
    pub number: usize,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub label: String,
    pub capacity: u64,
    pub estimated_used: u64,
    pub files: Vec<Entry>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub strategy: Strategy,
    #[serde(default)]
    pub reserve: u64,
    pub discs: Vec<Disc>,
    pub oversized: Vec<Entry>,
}
pub const MEDIA: &[(&str,u64)] = &[("CD",700_000_000),("DVD",4_700_000_000),("DVD-DL",8_500_000_000),("BD",25_000_000_000),("BD-DL",50_000_000_000),("BDXL-100",100_000_000_000),("BDXL-128",128_000_000_000)];
// Provisional estimate only. Real filesystem image must be measured before writing.
fn cost(f: &Entry) -> u64 { (f.bytes.saturating_add(2047)/2048*2048).saturating_add(4096) }
// Recursively preserve the largest folder that fits a disc.
fn folder_units(files: Vec<Entry>, depth: usize, limit: u64, out: &mut Vec<Vec<Entry>>) {
    let total = files.iter().fold(0u64, |n,f| n.saturating_add(cost(f)));
    if total <= limit { if !files.is_empty() { out.push(files); } return; }
    let mut groups: Vec<(String,Vec<Entry>)> = Vec::new();
    let mut indices = HashMap::new();
    for f in files {
        let parts: Vec<_> = f.archive.split('/').collect();
        let key = if depth+1 < parts.len() { format!("D:{}",parts[depth]) } else { format!("F:{}",f.archive) };
        let i = *indices.entry(key.clone()).or_insert_with(|| {let i=groups.len();groups.push((key,vec![]));i});
        groups[i].1.push(f);
    }
    // Match sidecars to a unique audio/video stem, including language suffixes.
    let media = ["mkv","mp4","avi","mov","m4v","webm","mpg","mpeg","ts","flac","mp3","wav","aac","m4a","ogg"];
    let sidecars = ["srt","ass","ssa","vtt","sub","idx","nfo","jpg","jpeg","png","txt"];
    let mut owners: HashMap<String,Vec<usize>> = HashMap::new();
    for (i,(key,g)) in groups.iter().enumerate() {
        if key.starts_with("F:") {
            if let Some((stem,ext)) = g[0].archive.rsplit_once('.') {
                if media.contains(&ext.to_ascii_lowercase().as_str()) { owners.entry(stem.to_lowercase()).or_default().push(i); }
            }
        }
    }
    for i in 0..groups.len() {
        if !groups[i].0.starts_with("F:") || groups[i].1.is_empty() { continue; }
        let name=groups[i].1[0].archive.to_lowercase();
        if !name.rsplit_once('.').map(|(_,e)|sidecars.contains(&e)).unwrap_or(false) {continue;}
        let mut stem=name.as_str();
        let mut matches=vec![];
        while let Some((prefix,_))=stem.rsplit_once('.') {
            if let Some(found)=owners.get(prefix){matches.extend(found.iter().copied());}
            stem=prefix;
        }
        if matches.len()==1 && matches[0]!=i {
            let mut related=std::mem::take(&mut groups[i].1);
            groups[matches[0]].1.append(&mut related);
        }
    }
    for (key,mut g) in groups {
        if key.starts_with("D:") { folder_units(g,depth+1,limit,out); }
        else if !g.is_empty() {
            sort_entries(&mut g);
            if g.iter().fold(0u64,|n,f|n.saturating_add(cost(f)))<=limit {out.push(g);}
            else {out.extend(g.into_iter().map(|f|vec![f]));}
        }
    }
}

pub fn plan(files: &[Entry], capacity: u64, reserve: u64, strategy: Strategy) -> Result<Plan> {
    plan_mixed(files, &[capacity], reserve, strategy)
}

// Dynamic segment tree for first-fit packing. Leaves are disc free capacities.
#[derive(Default)]
struct FreeIndex { len:usize, base:usize, tree:Vec<u64> }
impl FreeIndex {
    fn push(&mut self,value:u64){
        if self.len==self.base{
            let base=self.base.max(1)*if self.base==0{1}else{2};
            let mut tree=vec![0;2*base];
            if self.base>0{tree[base..base+self.len].copy_from_slice(&self.tree[self.base..self.base+self.len]);}
            for i in (1..base).rev(){tree[i]=tree[2*i].max(tree[2*i+1]);}
            self.base=base;self.tree=tree;
        }
        let i=self.len;self.len+=1;self.set(i,value);
    }
    fn set(&mut self,i:usize,value:u64){
        let mut node=self.base+i;self.tree[node]=value;
        while node>1{node/=2;self.tree[node]=self.tree[node*2].max(self.tree[node*2+1]);}
    }
    fn first(&self,size:u64)->Option<usize>{
        if self.len==0||self.tree[1]<size{return None;}
        let mut node=1;
        while node<self.base{node=if self.tree[node*2]>=size{node*2}else{node*2+1};}
        let i=node-self.base;if i<self.len{Some(i)}else{None}
    }
}

// Ordered capacities. Unused smaller media remain available for later groups.
// Once all listed media are used, the final capacity repeats.
pub fn plan_mixed(files: &[Entry], capacities: &[u64], reserve: u64, strategy: Strategy) -> Result<Plan> {
    if capacities.is_empty() || capacities.iter().any(|&c|c<=reserve) {bail!("Every capacity must exceed reserve");}
    let limit=capacities.iter().copied().max().unwrap()-reserve;
    let mut result=Plan {strategy,reserve,discs:vec![],oversized:vec![]};
    let mut sorted=files.to_vec();
    let mut paths=HashSet::new();
    for f in &sorted {
        if f.archive.is_empty() || f.archive.split('/').any(|p|p.is_empty()||p=="."||p==".."||p.contains('\\')||p.contains(':')) {bail!("Invalid archive path: {}",f.archive);}
        if !paths.insert(f.archive.to_lowercase()) {bail!("Duplicate archive path: {}",f.archive);}
    }
    sort_entries(&mut sorted);
    let mut eligible=vec![];
    for f in sorted {if cost(&f)>limit {result.oversized.push(f);} else {eligible.push(f);}}
    let mut units=vec![];
    if matches!(strategy,Strategy::Folder|Strategy::Hybrid) {
        // Partition recursively from the first path component.
        folder_units(eligible,0,limit,&mut units);
    } else {units.extend(eligible.into_iter().map(|f|vec![f]));}
    if strategy==Strategy::Maximum {units.sort_by_key(|g|std::cmp::Reverse(g.iter().map(cost).sum::<u64>()));}
    let mut available=capacities.to_vec();
    let mut free=FreeIndex::default();
    // If a group cannot fit remaining media, split it instead of silently omitting files.
    let mut pending: std::collections::VecDeque<_>=units.into();
    while let Some(g)=pending.pop_front() {
        let size=g.iter().map(cost).sum::<u64>();
        let fits=|d:&Disc|size<=d.capacity-reserve-d.estimated_used;
        let index=match strategy {
            Strategy::Maximum|Strategy::Hybrid=>free.first(size),
            _=>result.discs.len().checked_sub(1).filter(|&i|fits(&result.discs[i])),
        };
        let i=if let Some(i)=index {i} else {
            let cap=if let Some(j)=available.iter().position(|&c|c-reserve>=size){available.remove(j)}
            else if capacities.last().unwrap()-reserve>=size {*capacities.last().unwrap()}
            else {
                if g.len()>1 {for f in g.into_iter().rev(){pending.push_front(vec![f]);}}
                else {result.oversized.extend(g);}
                continue;
            };
            let i=result.discs.len();
            result.discs.push(Disc{number:i+1,locked:false,label:format!("DISC_{:03}",i+1),capacity:cap,estimated_used:0,files:vec![]});
            free.push(cap-reserve);
            i
        };
        result.discs[i].estimated_used+=size;
        free.set(i,result.discs[i].capacity-reserve-result.discs[i].estimated_used);
        result.discs[i].files.extend(g);
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn entry(name:&str, bytes:u64)->Entry { Entry {source:PathBuf::from(name),archive:name.into(),bytes,sha256:None} }
    #[test] fn free_index_matches_linear_reference(){
        let mut index=FreeIndex::default();let mut linear=vec![];
        for i in 0..1000usize{
            let value=(i*37%997) as u64;
            index.push(value);linear.push(value);
            let j=i/3;let changed=(i*13%1000) as u64;
            index.set(j,changed);linear[j]=changed;
            for size in [1,50,200,500,900,1000]{assert_eq!(index.first(size),linear.iter().position(|&v|v>=size));}
        }
    }
    #[test] fn large_inventory_conservation(){
        let files:Vec<_>=(0..50_000).map(|i|entry(&format!("Series {}/Season {}/Episode {}.mkv",i/1000,i/100,i),1_000_000+(i%1000) as u64*1024)).collect();
        for strategy in [Strategy::Sequential,Strategy::Folder,Strategy::Maximum,Strategy::Hybrid]{
            let p=plan(&files,700_000_000,1_000_000,strategy).unwrap();
            crate::editing::validate(&p).unwrap();
            assert!(p.oversized.is_empty());
            let paths:HashSet<_>=p.discs.iter().flat_map(|d|&d.files).map(|f|f.archive.as_str()).collect();
            assert_eq!(paths.len(),files.len());assert!(files.iter().all(|f|paths.contains(f.archive.as_str())));
        }
    }
    #[test] fn natural_order() {
        let mut names = ["Show S02E1", "Show S01E10", "Show S01E2", "Show S01E999999999999999999999"];
        names.sort_by(|a,b|natural_cmp(a,b));
        assert_eq!(names[0],"Show S01E2"); assert_eq!(names[1],"Show S01E10");
    }
    #[test] fn plans_account_for_every_file() {
        let files = vec![entry("A/1",20000),entry("A/2",30000),entry("B/1",25000),entry("B/2",100000)];
        for s in [Strategy::Sequential,Strategy::Folder,Strategy::Maximum,Strategy::Hybrid] {
            let p = plan(&files,70000,5000,s).unwrap();
            assert_eq!(p.oversized.len(),1);
            assert_eq!(p.discs.iter().map(|d|d.files.len()).sum::<usize>(),3);
            assert!(p.discs.iter().all(|d|d.estimated_used<=65000));
        }
    }
    #[test] fn recursive_groups_and_mixed_media() {
        let files=vec![entry("Shows/A/Season 1/E1.mkv",20000),entry("Shows/A/Season 2/E2.mkv",20000),entry("Shows/B/E1.mkv",40000)];
        let p=plan_mixed(&files,&[60000,85000],0,Strategy::Folder).unwrap();
        assert_eq!(p.discs.len(),2);
        assert_eq!(p.discs[0].files.len(),2);
        assert_eq!(p.discs[0].capacity,60000);
        assert_eq!(p.discs[1].capacity,85000);
        assert!(p.oversized.is_empty());
    }
    #[test] fn related_files_stay_together() {
        let files=vec![entry("Movies/Film 1.mkv",30000),entry("Movies/Film 1.en.srt",100),entry("Movies/Film 2.mkv",30000)];
        let p=plan(&files,50000,0,Strategy::Folder).unwrap();
        assert_eq!(p.discs.len(),2);
        assert!(p.discs.iter().any(|d|d.files.len()==2 && d.files.iter().all(|f|f.archive.contains("Film 1."))));
    }
    #[test] fn invalid_paths_rejected() {
        assert!(plan(&[entry("../bad",1)],50000,0,Strategy::Hybrid).is_err());
        assert!(plan(&[entry("A/x",1),entry("a/X",1)],50000,0,Strategy::Hybrid).is_err());
    }
    #[test] fn overlapping_sources_and_directory_exclusions(){
        let tmp=tempfile::tempdir().unwrap();
        let root=tmp.path().join("Collection");let sub=root.join("Season");let skip=root.join("Cache");
        std::fs::create_dir_all(&sub).unwrap();std::fs::create_dir_all(&skip).unwrap();
        std::fs::write(sub.join("E1.mkv"),b"video").unwrap();std::fs::write(skip.join("hidden.txt"),b"cache").unwrap();
        let i=scan(&[sub,root.clone(),root],&ScanOptions{exclude:vec!["collection/cache".into()],..Default::default()}).unwrap();
        assert!(i.errors.is_empty());assert_eq!(i.files.len(),1);assert_eq!(i.files[0].archive,"Collection/Season/E1.mkv");
        assert_eq!(i.excluded.len(),3);
    }
    #[test] fn long_unicode_path(){
        let tmp=tempfile::tempdir().unwrap();
        let root=tmp.path().join("Collection");std::fs::create_dir(&root).unwrap();
        let mut deep=root.canonicalize().unwrap();
        for _ in 0..8{deep.push("長いフォルダ_0123456789012345678901234567890123456789");}
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("音楽.flac"),b"test").unwrap();
        let i=scan(&[root],&ScanOptions{hash:true,..Default::default()}).unwrap();
        assert!(i.errors.is_empty());assert_eq!(i.files.len(),1);assert!(i.files[0].archive.ends_with("音楽.flac"));
        assert!(i.files[0].archive.encode_utf16().count()>260);
    }
    #[cfg(windows)]
    #[test] fn junction_is_not_traversed(){
        let tmp=tempfile::tempdir().unwrap();
        let root=tmp.path().join("Collection");let outside=tmp.path().join("Outside");
        std::fs::create_dir(&root).unwrap();std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"),b"outside").unwrap();
        let link=root.join("Junction");
        let status=std::process::Command::new("cmd").args(["/C","mklink","/J"]).arg(&link).arg(&outside).status().unwrap();
        assert!(status.success());
        let result=scan(&[root.clone(),link.clone()],&ScanOptions::default());
        std::fs::remove_dir(&link).unwrap();
        let i=result.unwrap();assert!(i.files.is_empty());assert!(i.errors.is_empty());assert_eq!(i.excluded.len(),2);
    }
    #[test] fn mixed_types_and_hashes() {
        let tmp=tempfile::tempdir().unwrap();
        let root=tmp.path().join("Collection"); std::fs::create_dir(&root).unwrap();
        for name in ["Video 2.mkv","Video 10.mkv","音楽.flac","metadata.nfo"] { std::fs::write(root.join(name),b"abc").unwrap(); }
        let i=scan(&[root.clone()],&ScanOptions{hash:true,..Default::default()}).unwrap();
        assert!(i.errors.is_empty()); assert_eq!(i.files.len(),4); assert_eq!(duplicates(&i.files)[0].len(),4);
        let i=scan(&[root],&ScanOptions{extensions:vec!["mkv".into()],..Default::default()}).unwrap();
        assert_eq!(i.files.len(),2); assert_eq!(i.excluded.len(),2);
    }
}

pub mod editing;

pub mod project;

pub mod catalog;

pub mod image;

pub mod optical;
