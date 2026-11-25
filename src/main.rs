use anyhow::{bail, Result};
use clap::{Parser, Subcommand, ValueEnum};
use disc_archive::{scan, plan_mixed, duplicates, ScanOptions, Strategy, Inventory, MEDIA};
use std::{fs::File, io::{BufReader, BufWriter, Write}, path::PathBuf};

#[derive(Parser)]
#[command(version, about="Read-only archive inventory and disc planning")]
struct Cli { #[command(subcommand)] command: Command }
#[derive(Clone, Copy, ValueEnum)]
enum Mode { Sequential, Folder, Maximum, Hybrid }
impl From<Mode> for Strategy {
    fn from(m: Mode)->Self { match m {Mode::Sequential=>Self::Sequential,Mode::Folder=>Self::Folder,Mode::Maximum=>Self::Maximum,Mode::Hybrid=>Self::Hybrid} }
}
#[derive(Subcommand)]
enum Command {
    Media,
    Scan { #[arg(required=true)] sources: Vec<PathBuf>, #[arg(short,long)] output: PathBuf, #[arg(long)] extension: Vec<String>, #[arg(long)] exclude: Vec<String>, #[arg(long)] hash: bool },
    Plan { inventory: PathBuf, #[arg(short,long)] output: PathBuf, #[arg(long,default_value_t=50_000_000_000)] capacity: u64, #[arg(long,value_delimiter=',')] capacities: Vec<u64>, #[arg(long,default_value_t=50_000_000)] reserve: u64, #[arg(long,value_enum,default_value="hybrid")] mode: Mode },
    Duplicates { inventory: PathBuf },
    Compare { inventory: PathBuf, #[arg(long,default_value_t=50_000_000_000)] capacity:u64, #[arg(long,value_delimiter=',')] capacities:Vec<u64>, #[arg(long,default_value_t=50_000_000)] reserve:u64 },
    Edit { plan:PathBuf, #[arg(short,long)] output:PathBuf, #[command(subcommand)] action:EditAction },
    Validate { plan:PathBuf },
    Drives,
    /// Experimental physical write; requires manual hardware acceptance.
    Burn { manifest:PathBuf, image:PathBuf, #[arg(long)] recorder:String, #[arg(long)] sha256:String, #[arg(long)] confirm:String },
    BurnPreview { manifest:PathBuf, image:PathBuf, #[arg(long)] recorder:String },
    ImageVerify { manifest:PathBuf, image:PathBuf },
    ImageCreate { plan:PathBuf, disc:usize, #[arg(short,long)] output:PathBuf },
    ImagePrepare { plan:PathBuf, disc:usize, #[arg(short,long)] output:PathBuf },
    CatalogCreate { database:PathBuf },
    CatalogImport { database:PathBuf, project:PathBuf },
    CatalogProjects { database:PathBuf },
    CatalogSearch { database:PathBuf, term:String, #[arg(long)] project_id:Option<i64>, #[arg(long,default_value_t=100)] limit:usize, #[arg(long,default_value_t=0)] offset:usize },
    ProjectCheck { project:PathBuf },
    ProjectPack { inventory:PathBuf, plan:PathBuf, #[arg(short,long)] output:PathBuf, #[arg(long,default_value="Archive")] name:String },
    ProjectExtract { project:PathBuf, #[arg(long)] inventory:Option<PathBuf>, #[arg(long)] plan:Option<PathBuf> },
}
#[derive(Subcommand)]
enum EditAction {
    Move { path:String, disc:usize },
    Lock { disc:usize },
    Unlock { disc:usize },
    Rename { pattern:String },
    AddDisc { capacity:u64 },
    Replan { #[arg(long,value_delimiter=',',required=true)] capacities:Vec<u64>, #[arg(long,value_enum,default_value="hybrid")] mode:Mode, #[arg(long)] pattern:Option<String> },
}
fn write_new<T:serde::Serialize>(path:PathBuf,value:&T)->Result<()> {
    let mut out=BufWriter::new(File::options().write(true).create_new(true).open(path)?);
    serde_json::to_writer_pretty(&mut out,value)?; out.flush()?; Ok(())
}
fn run()->Result<()> {
    match Cli::parse().command {
        Command::Media=>{for (name,bytes) in MEDIA {println!("{name}: {bytes} bytes");}},
        Command::Scan{sources,output,extension,exclude,hash}=>{
            let i=scan(&sources,&ScanOptions{extensions:extension,exclude,hash})?;
            eprintln!("{} files, {} excluded, {} errors",i.files.len(),i.excluded.len(),i.errors.len());
            let incomplete=!i.errors.is_empty(); write_new(output,&i)?;
            if incomplete {bail!("Inventory incomplete; inspect errors before planning");}
        },
        Command::Plan{inventory,output,capacity,capacities,reserve,mode}=>{
            let i:Inventory=serde_json::from_reader(BufReader::new(File::open(inventory)?))?;
            if !i.errors.is_empty(){bail!("Inventory has unresolved scan errors");}
            let capacities=if capacities.is_empty(){vec![capacity]}else{capacities};
            let p=plan_mixed(&i.files,&capacities,reserve,mode.into())?;
            eprintln!("{} discs, {} oversized files",p.discs.len(),p.oversized.len());
            let incomplete=!p.oversized.is_empty(); write_new(output,&p)?;
            if incomplete {bail!("Plan has oversized files requiring manual resolution");}
        },
        Command::Compare{inventory,capacity,capacities,reserve}=>{
            let i:Inventory=serde_json::from_reader(BufReader::new(File::open(inventory)?))?;
            if !i.errors.is_empty(){bail!("Inventory has unresolved scan errors");}
            let caps=if capacities.is_empty(){vec![capacity]}else{capacities};
            println!("{}",serde_json::to_string_pretty(&disc_archive::editing::compare(&i.files,&caps,reserve)?)?);
        },
        Command::Validate{plan}=>{
            let p:disc_archive::Plan=serde_json::from_reader(BufReader::new(File::open(plan)?))?;
            disc_archive::editing::validate(&p)?;
            println!("{}",serde_json::to_string_pretty(&disc_archive::editing::summary(&p))?);
        },
        Command::Edit{plan,output,action}=>{
            use disc_archive::editing::*;
            let mut p:disc_archive::Plan=serde_json::from_reader(BufReader::new(File::open(plan)?))?;
            match action {
                EditAction::Move{path,disc}=>move_file(&mut p,&path,disc)?,
                EditAction::Lock{disc}=>set_lock(&mut p,disc,true)?,
                EditAction::Unlock{disc}=>set_lock(&mut p,disc,false)?,
                EditAction::Rename{pattern}=>rename(&mut p,&pattern)?,
                EditAction::AddDisc{capacity}=>add_disc(&mut p,capacity)?,
                EditAction::Replan{capacities,mode,pattern}=>p=replan(&p,&capacities,mode.into(),pattern.as_deref())?,
            }
            write_new(output,&p)?;
        },
        Command::Burn{manifest,image,recorder,sha256,confirm}=>{
            let m:disc_archive::image::PreparedDisc=serde_json::from_reader(BufReader::new(File::open(manifest)?))?;
            let receipt=disc_archive::optical::burn(&m,&image,&recorder,&sha256,&confirm)?;
            println!("{}",serde_json::to_string_pretty(&receipt)?);
        },
        Command::BurnPreview{manifest,image,recorder}=>{
            let m:disc_archive::image::PreparedDisc=serde_json::from_reader(BufReader::new(File::open(manifest)?))?;
            let preview=disc_archive::optical::burn_preview(&m,&image,&recorder)?;
            println!("{}",serde_json::to_string_pretty(&preview)?);
        },
        Command::Drives=>{println!("{}",serde_json::to_string_pretty(&disc_archive::optical::discover()?)?);},
        Command::ImageVerify{manifest,image}=>{
            let p:disc_archive::image::PreparedDisc=serde_json::from_reader(BufReader::new(File::open(manifest)?))?;
            println!("{}",serde_json::to_string_pretty(&disc_archive::image::verify_udf(&p,&image)?)?);
        },
        Command::ImageCreate{plan,disc,output}=>{
            let p:disc_archive::Plan=serde_json::from_reader(BufReader::new(File::open(plan)?))?;
            let receipt=disc_archive::image::create_udf(&p,disc,&output)?;
            println!("{}",serde_json::to_string_pretty(&receipt)?);
        },
        Command::ImagePrepare{plan,disc,output}=>{
            let p:disc_archive::Plan=serde_json::from_reader(BufReader::new(File::open(plan)?))?;
            let prepared=disc_archive::image::prepare(&p,disc)?;
            write_new(output,&prepared)?;
            println!("Disc preflight passed. No image or physical media written.");
        },
        Command::CatalogCreate{database}=>{disc_archive::catalog::Catalog::create(&database)?;println!("Catalog created");},
        Command::CatalogImport{database,project}=>{let p=disc_archive::project::load(&project)?;let id=disc_archive::catalog::Catalog::open(&database,true)?.import(&p)?;println!("Imported planned snapshot {id}");},
        Command::CatalogProjects{database}=>{println!("{}",serde_json::to_string_pretty(&disc_archive::catalog::Catalog::open(&database,false)?.projects()?)?);},
        Command::CatalogSearch{database,term,project_id,limit,offset}=>{println!("{}",serde_json::to_string_pretty(&disc_archive::catalog::Catalog::open(&database,false)?.search(&term,project_id,limit,offset)?)?);},
        Command::ProjectCheck{project}=>{
            let p=disc_archive::project::load(&project)?;
            println!("Project validated: {}",p.name);
            if let Some(plan)=p.plan{println!("{}",serde_json::to_string_pretty(&disc_archive::editing::summary(&plan))?);}
        },
        Command::ProjectPack{inventory,plan,output,name}=>{
            let i:Inventory=serde_json::from_reader(BufReader::new(File::open(inventory)?))?;
            let p:disc_archive::Plan=serde_json::from_reader(BufReader::new(File::open(plan)?))?;
            let caps=if p.discs.is_empty(){vec![p.reserve.checked_add(1).ok_or_else(||anyhow::anyhow!("Invalid reserve"))?]}else{p.discs.iter().map(|d|d.capacity).collect()};
            let project=disc_archive::project::Project{name,capacities:caps,reserve:p.reserve,strategy:p.strategy,inventory:Some(i),plan:Some(p),..Default::default()};
            disc_archive::project::save(&output,&project)?;
        },
        Command::ProjectExtract{project,inventory,plan}=>{
            let p=disc_archive::project::load(&project)?;
            if inventory.is_none()&&plan.is_none(){bail!("Select at least one output");}
            if let Some(path)=inventory{write_new(path,p.inventory.as_ref().ok_or_else(||anyhow::anyhow!("No inventory stored"))?)?;}
            if let Some(path)=plan{write_new(path,p.plan.as_ref().ok_or_else(||anyhow::anyhow!("No plan stored"))?)?;}
        },
        Command::Duplicates{inventory}=>{
            let i:Inventory=serde_json::from_reader(BufReader::new(File::open(inventory)?))?;
            if i.files.iter().any(|f|f.sha256.is_none()){bail!("Scan with --hash first");}
            println!("{}",serde_json::to_string_pretty(&duplicates(&i.files))?);
        }
    } Ok(())
}
fn main(){if let Err(e)=run(){eprintln!("Error: {e:#}");std::process::exit(1);}}
