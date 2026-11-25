use anyhow::{bail, Result};
use clap::{Parser, Subcommand, ValueEnum};
use disc_archive::{scan, plan, duplicates, ScanOptions, Strategy, Inventory, MEDIA};
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
    Plan { inventory: PathBuf, #[arg(short,long)] output: PathBuf, #[arg(long,default_value_t=50_000_000_000)] capacity: u64, #[arg(long,default_value_t=50_000_000)] reserve: u64, #[arg(long,value_enum,default_value="hybrid")] mode: Mode },
    Duplicates { inventory: PathBuf },
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
        Command::Plan{inventory,output,capacity,reserve,mode}=>{
            let i:Inventory=serde_json::from_reader(BufReader::new(File::open(inventory)?))?;
            if !i.errors.is_empty(){bail!("Inventory has unresolved scan errors");}
            let p=plan(&i.files,capacity,reserve,mode.into())?;
            eprintln!("{} discs, {} oversized files",p.discs.len(),p.oversized.len());
            let incomplete=!p.oversized.is_empty(); write_new(output,&p)?;
            if incomplete {bail!("Plan has oversized files requiring manual resolution");}
        },
        Command::Duplicates{inventory}=>{
            let i:Inventory=serde_json::from_reader(BufReader::new(File::open(inventory)?))?;
            if i.files.iter().any(|f|f.sha256.is_none()){bail!("Scan with --hash first");}
            println!("{}",serde_json::to_string_pretty(&duplicates(&i.files))?);
        }
    } Ok(())
}
fn main(){if let Err(e)=run(){eprintln!("Error: {e:#}");std::process::exit(1);}}
