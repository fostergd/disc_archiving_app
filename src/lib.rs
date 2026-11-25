use anyhow::{bail, Context, Result};
use globset::{Glob, GlobSetBuilder};
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
#[derive(Debug, Default, Serialize, Deserialize)]
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
    let aa = a.to_lowercase();
    let bb = b.to_lowercase();
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

pub fn digest(path: &Path) -> Result<String> {
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop { let n = f.read(&mut buf)?; if n == 0 { break; } h.update(&buf[..n]); }
    Ok(format!("{:x}", h.finalize()))
}

pub fn scan(roots: &[PathBuf], options: &ScanOptions) -> Result<Inventory> {
    let mut builder = GlobSetBuilder::new();
    for pattern in &options.exclude { builder.add(Glob::new(pattern)?); }
    let excludes = builder.build()?;
    let extensions: HashSet<_> = options.extensions.iter().map(|e| e.trim_start_matches('.').to_lowercase()).collect();
    let mut inventory = Inventory::default();
    let mut paths = HashSet::new();
    for input in roots {
        let root = input.canonicalize().with_context(|| format!("Source unavailable: {}", input.display()))?;
        let prefix = root.file_name().and_then(|s| s.to_str()).context("Select a named folder or file, not an unnamed volume root")?;
        let base = root.parent().context("Source has no parent")?;
        for item in WalkDir::new(&root).follow_links(false) {
            let item = match item { Ok(v) => v, Err(e) => { inventory.errors.push(e.to_string()); continue; } };
            if item.file_type().is_symlink() { inventory.excluded.push(format!("Link: {}", item.path().display())); continue; }
            if !item.file_type().is_file() { continue; }
            let relative = item.path().strip_prefix(base)?;
            let mut parts = Vec::new();
            for c in relative.components() { parts.push(c.as_os_str().to_str().context("Non-Unicode filename cannot be represented safely")?); }
            let archive = parts.join("/");
            debug_assert!(archive.starts_with(prefix));
            if excludes.is_match(&archive) || (!extensions.is_empty() && !item.path().extension().and_then(|s| s.to_str()).map(|s| extensions.contains(&s.to_lowercase())).unwrap_or(false)) {
                inventory.excluded.push(archive); continue;
            }
            let key = archive.to_lowercase();
            if !paths.insert(key) { bail!("Archive path collision: {archive}"); }
            let result = (|| -> Result<Entry> {
                let bytes = item.metadata()?.len();
                let sha256 = if options.hash { Some(digest(item.path())?) } else { None };
                Ok(Entry { source: item.path().to_owned(), archive: archive.clone(), bytes, sha256 })
            })();
            match result { Ok(e) => inventory.files.push(e), Err(e) => inventory.errors.push(format!("{archive}: {e:#}")) }
        }
    }
    inventory.files.sort_by(|a,b| natural_cmp(&a.archive,&b.archive));
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
#[derive(Debug, Serialize, Deserialize)]
pub struct Disc {
    pub number: usize,
    pub capacity: u64,
    pub estimated_used: u64,
    pub files: Vec<Entry>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Plan {
    pub strategy: Strategy,
    pub discs: Vec<Disc>,
    pub oversized: Vec<Entry>,
}
pub const MEDIA: &[(&str,u64)] = &[("CD",700_000_000),("DVD",4_700_000_000),("DVD-DL",8_500_000_000),("BD",25_000_000_000),("BD-DL",50_000_000_000),("BDXL-100",100_000_000_000),("BDXL-128",128_000_000_000)];
// Provisional estimate only. Real filesystem image must be measured before writing.
fn cost(f: &Entry) -> u64 { f.bytes.saturating_add(2047)/2048*2048 + 4096 }
pub fn plan(files: &[Entry], capacity: u64, reserve: u64, strategy: Strategy) -> Result<Plan> {
    let usable = capacity.checked_sub(reserve).filter(|x| *x>0).context("Reserve must be smaller than capacity")?;
    let mut result = Plan { strategy, discs: vec![], oversized: vec![] };
    let mut sorted = files.to_vec();
    sorted.sort_by(|a,b| natural_cmp(&a.archive,&b.archive));
    let mut groups: Vec<Vec<Entry>> = vec![];
    for f in sorted {
        if cost(&f)>usable { result.oversized.push(f); continue; }
        let parent = f.archive.rsplit_once('/').map(|v|v.0).unwrap_or("");
        let same_parent = groups.last().and_then(|g|g.first()).map(|x|x.archive.rsplit_once('/').map(|v|v.0).unwrap_or("") == parent).unwrap_or(false);
        if matches!(strategy, Strategy::Folder|Strategy::Hybrid) && same_parent {
            groups.last_mut().unwrap().push(f);
        } else { groups.push(vec![f]); }
    }
    let mut units = vec![];
    for g in groups {
        let size = g.iter().fold(0u64, |s,f|s.saturating_add(cost(f)));
        if size<=usable { units.push(g); } else { units.extend(g.into_iter().map(|f|vec![f])); }
    }
    if strategy==Strategy::Maximum { units.sort_by_key(|g|std::cmp::Reverse(g.iter().map(cost).sum::<u64>())); }
    for g in units {
        let size = g.iter().map(cost).sum::<u64>();
        let fits = |d: &Disc| size <= usable-d.estimated_used;
        let index = match strategy {
            Strategy::Maximum|Strategy::Hybrid => result.discs.iter().position(fits),
            _ => result.discs.len().checked_sub(1).filter(|&i|fits(&result.discs[i])),
        };
        let i = index.unwrap_or_else(|| {
            let i = result.discs.len();
            result.discs.push(Disc { number:i+1, capacity, estimated_used:0, files:vec![] }); i
        });
        result.discs[i].estimated_used += size;
        result.discs[i].files.extend(g);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(name:&str, bytes:u64)->Entry { Entry {source:PathBuf::from(name),archive:name.into(),bytes,sha256:None} }
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
