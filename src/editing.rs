use super::*;
use std::collections::{BTreeMap,BTreeSet};

#[derive(Debug, Serialize)]
pub struct Summary {
    pub strategy: Strategy,
    pub discs: usize,
    pub files: usize,
    pub unassigned: usize,
    pub payload_bytes: u128,
    pub estimated_used_bytes: u128,
    pub capacity_bytes: u128,
    pub split_folders: usize,
}
pub fn summary(p:&Plan)->Summary {
    let mut folders:BTreeMap<&str,BTreeSet<usize>>=BTreeMap::new();
    for d in &p.discs {
        for f in &d.files {
            for (i,_) in f.archive.match_indices('/') {
                folders.entry(&f.archive[..i]).or_default().insert(d.number);
            }
        }
    }
    Summary{strategy:p.strategy,discs:p.discs.len(),files:p.discs.iter().map(|d|d.files.len()).sum(),
        unassigned:p.oversized.len(),payload_bytes:p.discs.iter().flat_map(|d|&d.files).map(|f|f.bytes as u128).sum(),
        estimated_used_bytes:p.discs.iter().map(|d|d.estimated_used as u128).sum(),
        capacity_bytes:p.discs.iter().map(|d|d.capacity as u128).sum(),
        split_folders:folders.values().filter(|s|s.len()>1).count()}
}
pub fn compare(files:&[Entry], capacities:&[u64],reserve:u64)->Result<Vec<Summary>> {
    [Strategy::Sequential,Strategy::Folder,Strategy::Maximum,Strategy::Hybrid].into_iter()
        .map(|s|Ok(summary(&plan_mixed(files,capacities,reserve,s)?))).collect()
}
fn valid_label(s:&str)->bool {
    !s.is_empty() && s.len()<=32 && s.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'||b==b'-')
}
pub fn validate(p:&Plan)->Result<()> {
    let mut numbers=HashSet::new();let mut names=HashSet::new();let mut paths=HashSet::new();
    for d in &p.discs {
        if d.number==0||!numbers.insert(d.number){bail!("Invalid or repeated disc number");}
        if !valid_label(&d.label)||!names.insert(d.label.to_ascii_lowercase()){bail!("Invalid or repeated disc label");}
        let used=d.files.iter().try_fold(0u64,|n,f|n.checked_add(cost(f))).context("Disc size overflow")?;
        if used!=d.estimated_used||d.capacity<=p.reserve||used>d.capacity-p.reserve {bail!("Invalid capacity accounting on disc {}",d.number);}
    }
    for f in p.discs.iter().flat_map(|d|&d.files).chain(&p.oversized) {
        if f.archive.is_empty()||f.archive.split('/').any(|s|s.is_empty()||s=="."||s==".."||s.contains('\\')||s.contains(':'))||!paths.insert(f.archive.to_lowercase()){bail!("Invalid or repeated archive path: {}",f.archive);}
    }
    Ok(())
}
pub fn set_lock(p:&mut Plan,number:usize,locked:bool)->Result<()> {
    validate(p)?;
    p.discs.iter_mut().find(|d|d.number==number).context("Disc not found")?.locked=locked;
    Ok(())
}
pub fn rename(p:&mut Plan,pattern:&str)->Result<()> {
    validate(p)?;
    if !pattern.contains("{n}")&&!pattern.contains("{n:03}")&&!pattern.contains("{n:04}") {bail!("Pattern requires a disc number placeholder");}
    let mut next=p.clone();
    for d in &mut next.discs {
        if !d.locked {
            d.label=pattern.replace("{n:03}",&format!("{:03}",d.number)).replace("{n:04}",&format!("{:04}",d.number)).replace("{n}",&d.number.to_string());
        }
    }
    validate(&next)?;*p=next;Ok(())
}
pub fn move_file(p:&mut Plan,path:&str,target:usize)->Result<()> {
    validate(p)?;
    let dest=p.discs.iter().position(|d|d.number==target).context("Destination disc not found")?;
    if p.discs[dest].locked {bail!("Destination is locked");}
    let source=p.discs.iter().enumerate().find_map(|(i,d)|d.files.iter().position(|f|f.archive==path).map(|j|(i,j)));
    let unassigned=p.oversized.iter().position(|f|f.archive==path);
    let f=if let Some((i,j))=source {
        if p.discs[i].locked {bail!("Source is locked");}
        if i==dest {return Ok(());}
        &p.discs[i].files[j]
    }else{&p.oversized[unassigned.context("File not found")?]};
    let size=cost(f);
    if size>p.discs[dest].capacity-p.reserve-p.discs[dest].estimated_used {bail!("File does not fit destination");}
    let f=if let Some((i,j))=source {p.discs[i].estimated_used-=size;p.discs[i].files.remove(j)}else{p.oversized.remove(unassigned.unwrap())};
    p.discs[dest].estimated_used+=size;p.discs[dest].files.push(f);
    p.discs[dest].files.sort_by(|a,b|natural_cmp(&a.archive,&b.archive));Ok(())
}
pub fn add_disc(p:&mut Plan,capacity:u64)->Result<()> {
    validate(p)?;
    if capacity<=p.reserve {bail!("Capacity must exceed reserve");}
    let number=p.discs.iter().map(|d|d.number).max().unwrap_or(0).checked_add(1).context("Disc number overflow")?;
    let mut next=p.clone();next.discs.push(Disc{number,capacity,estimated_used:0,files:vec![],locked:false,label:format!("DISC_{:03}",number)});
    validate(&next)?;*p=next;Ok(())
}
// Capacities describe only the unlocked portion. Existing locked media are excluded.
pub fn replan(p:&Plan,capacities:&[u64],strategy:Strategy,pattern:Option<&str>)->Result<Plan> {
    validate(p)?;
    let files:Vec<_>=p.discs.iter().filter(|d|!d.locked).flat_map(|d|d.files.iter().cloned()).chain(p.oversized.iter().cloned()).collect();
    let mut next=plan_mixed(&files,capacities,p.reserve,strategy)?;
    let locked:Vec<_>=p.discs.iter().filter(|d|d.locked).cloned().collect();
    let mut used:HashSet<_>=locked.iter().map(|d|d.number).collect();
    let mut number=1usize;
    for d in &mut next.discs {
        while used.contains(&number){number=number.checked_add(1).context("Disc number overflow")?;}
        d.number=number;
        d.label=format!("DISC_{:03}",number);
        used.insert(number);
    }
    next.discs.extend(locked);
    next.discs.sort_by_key(|d|d.number);
    // Apply the naming pattern before final validation, allowing default labels
    // to collide temporarily with existing custom locked names.
    if let Some(pattern)=pattern {
        if !pattern.contains("{n}")&&!pattern.contains("{n:03}")&&!pattern.contains("{n:04}") {bail!("Pattern requires a disc number placeholder");}
        for d in &mut next.discs {if !d.locked {d.label=pattern.replace("{n:03}",&format!("{:03}",d.number)).replace("{n:04}",&format!("{:04}",d.number)).replace("{n}",&d.number.to_string());}}
    }
    validate(&next)?;
    Ok(next)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn entry(name:&str,bytes:u64)->Entry{Entry{source:name.into(),archive:name.into(),bytes,sha256:None}}
    #[test] fn editing_preserves_files_and_locks(){
        let mut p=plan(&[entry("A/1",20000),entry("A/2",20000)],30000,0,Strategy::Sequential).unwrap();
        add_disc(&mut p,60000).unwrap();
        set_lock(&mut p,1,true).unwrap();
        assert!(move_file(&mut p,"A/1",3).is_err());
        assert!(move_file(&mut p,"A/2",1).is_err());
        move_file(&mut p,"A/2",3).unwrap();
        rename(&mut p,"VIDEO_{n:04}").unwrap();
        assert_eq!(p.discs[0].label,"DISC_001");
        assert_eq!(p.discs[2].label,"VIDEO_0003");
        validate(&p).unwrap();assert_eq!(summary(&p).files,2);
    }
    #[test] fn reject_overfill_and_recover_oversized(){
        let mut p=plan(&[entry("a",20000),entry("b",40000)],30000,0,Strategy::Sequential).unwrap();
        assert!(move_file(&mut p,"b",1).is_err());assert_eq!(p.oversized.len(),1);
        add_disc(&mut p,60000).unwrap();move_file(&mut p,"b",2).unwrap();assert!(p.oversized.is_empty());validate(&p).unwrap();
    }
    #[test] fn replan_preserves_locked_disc_and_every_entry(){
        let mut p=plan(&[entry("A/1",20000),entry("A/2",20000),entry("B/1",40000)],30000,0,Strategy::Sequential).unwrap();
        rename(&mut p,"VIDEO_{n:03}").unwrap();set_lock(&mut p,1,true).unwrap();
        let locked=serde_json::to_value(&p.discs[0]).unwrap();
        let q=replan(&p,&[80000],Strategy::Hybrid,Some("VIDEO_{n:03}")).unwrap();
        assert_eq!(serde_json::to_value(&q.discs[0]).unwrap(),locked);
        assert_eq!(summary(&q).files,3);assert!(q.oversized.is_empty());assert_eq!(q.discs.len(),2);
        let mut before:Vec<_>=p.discs.iter().flat_map(|d|&d.files).chain(&p.oversized).map(|f|f.archive.clone()).collect();
        let mut after:Vec<_>=q.discs.iter().flat_map(|d|&d.files).chain(&q.oversized).map(|f|f.archive.clone()).collect();
        before.sort();after.sort();assert_eq!(before,after);
    }
    #[test] fn replan_all_locked(){
        let mut p=plan(&[entry("A/1",20000)],30000,0,Strategy::Sequential).unwrap();
        set_lock(&mut p,1,true).unwrap();
        let q=replan(&p,&[50000],Strategy::Maximum,None).unwrap();
        assert_eq!(serde_json::to_value(&q.discs).unwrap(),serde_json::to_value(&p.discs).unwrap());
    }
    #[test] fn compare_all_modes(){assert_eq!(compare(&[entry("A/1",100)],&[50000],0).unwrap().len(),4);}
}
