#![cfg_attr(target_os="windows", windows_subsystem="windows")]
use disc_archive::{scan,plan_mixed,ScanOptions,Inventory,Plan,Strategy,MEDIA};
use disc_archive::editing;
use eframe::egui;
use std::{path::PathBuf,sync::mpsc::{self,Receiver},fs::File,io::{BufReader,BufWriter,Write}};
enum Work { Inventory(Inventory), Plan(Plan), Comparison(Vec<editing::Summary>), Duplicates(Vec<Vec<String>>), Saved(String), Project(disc_archive::project::Project), Error(String) }
struct Studio {
    sources:Vec<PathBuf>, source_text:String, extensions:String, exclusions:String, hash:bool,
    capacities:String, reserve:String, strategy:Strategy, inventory:Option<Inventory>, plan:Option<Plan>,
    comparison:Vec<editing::Summary>, worker:Option<Receiver<Work>>, status:String, selected:Option<usize>,
    pattern:String, move_path:String, destination:usize, search:String, tab:usize,
    inventory_search:String, duplicate_groups:Vec<Vec<String>>, dirty:bool, close_prompt:bool,
    project_name:String,
}
impl Default for Studio {
    fn default()->Self{Self{sources:vec![],source_text:String::new(),extensions:String::new(),exclusions:String::new(),hash:false,
        capacities:"50000000000".into(),reserve:"50000000".into(),strategy:Strategy::Hybrid,inventory:None,plan:None,comparison:vec![],
        worker:None,status:"Add files or folders to begin.".into(),selected:None,pattern:"DISC_{n:03}".into(),move_path:String::new(),destination:1,search:String::new(),tab:0,inventory_search:String::new(),duplicate_groups:vec![],dirty:false,close_prompt:false,project_name:"Untitled archive".into()}}
}
fn caps(text:&str)->anyhow::Result<Vec<u64>> {let v=text.split(',').map(|s|s.trim().parse::<u64>()).collect::<Result<Vec<_>,_>>()?;Ok(v)}
fn save_new<T:serde::Serialize>(path:PathBuf,value:&T)->anyhow::Result<()>{
    let mut f=BufWriter::new(File::options().write(true).create_new(true).open(path)?);
    serde_json::to_writer_pretty(&mut f,value)?;f.flush()?;Ok(())
}
fn gb(n:u64)->String{format!("{:.2} GB",n as f64/1e9)}
impl Studio{
    fn add(&mut self,p:PathBuf){if !self.sources.contains(&p){self.sources.push(p);}}
    fn spawn(&mut self,ctx:&egui::Context,f:impl FnOnce()->anyhow::Result<Work>+Send+'static){
        let (tx,rx)=mpsc::channel();let ctx=ctx.clone();
        std::thread::spawn(move||{let result=f().unwrap_or_else(|e|Work::Error(format!("{e:#}")));let _=tx.send(result);ctx.request_repaint();});
        self.worker=Some(rx);self.status="Working in background...".into();
    }
    fn poll(&mut self){
        let event=match self.worker.as_ref().map(|r|r.try_recv()){
            Some(Ok(v))=>Some(v),Some(Err(mpsc::TryRecvError::Disconnected))=>Some(Work::Error("Worker stopped unexpectedly".into())),_=>None};
        if let Some(event)=event{
            self.worker=None;
            match event{
                Work::Inventory(i)=>{self.status=format!("Scanned {} files; {} exclusions; {} errors",i.files.len(),i.excluded.len(),i.errors.len());self.inventory=Some(i);self.plan=None;self.comparison.clear();self.duplicate_groups.clear();self.dirty=false;}
                Work::Plan(p)=>{self.status=format!("{} discs; {} unassigned files",p.discs.len(),p.oversized.len());self.selected=p.discs.first().map(|d|d.number);self.reserve=p.reserve.to_string();self.strategy=p.strategy;self.plan=Some(p);self.comparison.clear();self.dirty=true;self.tab=1;}
                Work::Comparison(v)=>{self.comparison=v;self.status="Comparison complete".into();self.tab=2;}
                Work::Duplicates(v)=>{self.status=format!("{} duplicate groups",v.len());self.duplicate_groups=v;self.tab=4;}
                Work::Project(p)=>{self.project_name=p.name;self.sources=p.sources;self.extensions=p.extensions.join(",");self.exclusions=p.exclusions.join("\n");self.hash=p.hash;self.capacities=p.capacities.iter().map(u64::to_string).collect::<Vec<_>>().join(",");self.reserve=p.reserve.to_string();self.strategy=p.strategy;self.pattern=p.naming_pattern;self.inventory=p.inventory;self.plan=p.plan;self.selected=self.plan.as_ref().and_then(|p|p.discs.first()).map(|d|d.number);self.comparison.clear();self.duplicate_groups.clear();self.tab=if self.plan.is_some(){1}else{0};self.dirty=false;self.status="Project opened".into();}
                Work::Saved(message)=>{self.status=message;self.dirty=false;}
                Work::Error(e)=>self.status=e,
            }
        }
    }
    fn snapshot(&self)->anyhow::Result<disc_archive::project::Project>{
        let (reserve,strategy)=self.plan.as_ref().map(|p|(p.reserve,p.strategy)).unwrap_or((self.reserve.trim().parse()?,self.strategy));
        Ok(disc_archive::project::Project{name:self.project_name.clone(),sources:self.sources.clone(),extensions:self.extensions.split(',').map(str::trim).filter(|s|!s.is_empty()).map(str::to_owned).collect(),exclusions:self.exclusions.lines().map(str::trim).filter(|s|!s.is_empty()).map(str::to_owned).collect(),hash:self.hash,capacities:caps(&self.capacities)?,reserve,strategy,naming_pattern:self.pattern.clone(),inventory:self.inventory.clone(),plan:self.plan.clone(),..Default::default()})
    }
    fn begin_plan(&mut self,ctx:&egui::Context,compare:bool){
        let result=(||->anyhow::Result<_>{
            let capacities=caps(&self.capacities)?;let reserve=self.reserve.trim().parse::<u64>()?;
            let i=self.inventory.as_ref().ok_or_else(||anyhow::anyhow!("Scan or load an inventory first"))?;
            if !i.errors.is_empty(){anyhow::bail!("Inventory has unresolved scan errors");}
            Ok((i.files.clone(),capacities,reserve))
        })();
        match result{
            Ok((files,capacities,reserve))=>{
                if self.plan.is_some()&&!compare{self.status="Use Replan or explicitly close the current plan first.".into();return;}
                let strategy=self.strategy;
                self.spawn(ctx,move||if compare{Ok(Work::Comparison(editing::compare(&files,&capacities,reserve)?))}else{Ok(Work::Plan(plan_mixed(&files,&capacities,reserve,strategy)?))});
            }
            Err(e)=>self.status=e.to_string(),
        }
    }
    fn sources_ui(&mut self,ui:&mut egui::Ui,ctx:&egui::Context){
        ui.heading("Archive sources");
        ui.label("Drop files or folders into this window, browse, or enter a local/UNC path.");
        ui.horizontal(|ui|{
            if ui.button("Add folders").clicked(){if let Some(v)=rfd::FileDialog::new().pick_folders(){for p in v{self.add(p);}}}
            if ui.button("Add files").clicked(){if let Some(v)=rfd::FileDialog::new().pick_files(){for p in v{self.add(p);}}}
            if ui.button("Clear sources").clicked(){self.sources.clear();}
        });
        ui.horizontal(|ui|{ui.text_edit_singleline(&mut self.source_text);if ui.button("Add path").clicked()&&!self.source_text.trim().is_empty(){self.add(PathBuf::from(self.source_text.trim()));self.source_text.clear();}});
        let mut remove=None;
        for (i,p) in self.sources.iter().enumerate(){ui.horizontal(|ui|{if ui.small_button("Remove").clicked(){remove=Some(i);}ui.label(p.display().to_string());});}
        if let Some(i)=remove{self.sources.remove(i);}
        ui.separator();
        ui.label("Extension allowlist (comma separated; blank includes all files)");
        ui.text_edit_singleline(&mut self.extensions);
        ui.label("Exclusion glob patterns (one per line)");
        ui.text_edit_multiline(&mut self.exclusions);
        ui.checkbox(&mut self.hash,"Calculate SHA-256 while scanning (reads all file contents)");
        if ui.add_enabled(!self.sources.is_empty()&&self.plan.is_none(),egui::Button::new("Scan sources")).clicked(){
            let roots=self.sources.clone();
            let options=ScanOptions{extensions:self.extensions.split(',').map(str::trim).filter(|s|!s.is_empty()).map(str::to_owned).collect(),
                exclude:self.exclusions.lines().map(str::trim).filter(|s|!s.is_empty()).map(str::to_owned).collect(),hash:self.hash};
            self.spawn(ctx,move||Ok(Work::Inventory(scan(&roots,&options)?)));
        }
        if self.plan.is_some(){ui.label("Close the current plan before scanning another collection.");}
        if let Some(i)=&self.inventory{
            ui.separator();ui.label(format!("{} files | {} exclusions | {} errors",i.files.len(),i.excluded.len(),i.errors.len()));
            for e in i.errors.iter().take(20){ui.colored_label(egui::Color32::RED,e);}
            if i.errors.len()>20{ui.label("Save inventory for the complete error report.");}
            ui.horizontal(|ui|{ui.label("Search inventory");ui.text_edit_singleline(&mut self.inventory_search);});
            let filter=self.inventory_search.to_lowercase();
            let visible:Vec<_>=i.files.iter().filter(|f|filter.is_empty()||f.archive.to_lowercase().contains(&filter)).collect();
            ui.label(format!("{} matching files",visible.len()));
            egui::ScrollArea::vertical().id_salt("inventory").max_height(350.).show_rows(ui,22.,visible.len(),|ui,range|{
                for j in range{let f=visible[j];ui.label(format!("{}  |  {}",f.archive,gb(f.bytes)));}
            });
            let mut show_exclusions=false;
            ui.collapsing("Excluded paths",|ui|{
                egui::ScrollArea::vertical().id_salt("exclusions").max_height(180.).show_rows(ui,22.,i.excluded.len(),|ui,range|{for j in range{ui.label(&i.excluded[j]);}});
            });
            if ui.button("Find duplicate content").clicked(){show_exclusions=true;}
            if show_exclusions{
                if !i.errors.is_empty(){self.status="Resolve scan errors before duplicate analysis.".into();}
                else if i.files.iter().any(|f|f.sha256.is_none()){self.status="Rescan with SHA-256 enabled to detect duplicate content.".into();}
                else {let files=i.files.clone();self.spawn(ctx,move||Ok(Work::Duplicates(disc_archive::duplicates(&files))));}
            }
        }
    }
    fn planner_ui(&mut self,ui:&mut egui::Ui,ctx:&egui::Context){
        ui.heading("Disc planner");
        ui.horizontal_wrapped(|ui|{for &(name,bytes) in MEDIA{if ui.button(name).clicked(){self.capacities=bytes.to_string();}}});
        ui.label("Capacities in bytes, comma separated (last capacity repeats)");
        ui.text_edit_singleline(&mut self.capacities);
        ui.horizontal(|ui|{ui.label("Reserve bytes");ui.text_edit_singleline(&mut self.reserve);});
        egui::ComboBox::from_label("Distribution").selected_text(format!("{:?}",self.strategy)).show_ui(ui,|ui|{
            for s in [Strategy::Sequential,Strategy::Folder,Strategy::Maximum,Strategy::Hybrid]{ui.selectable_value(&mut self.strategy,s,format!("{s:?}"));}
        });
        ui.horizontal(|ui|{
            if ui.button("Create plan").clicked(){self.begin_plan(ctx,false);}
            if ui.button("Compare strategies").clicked(){self.begin_plan(ctx,true);}
            if ui.add_enabled(self.plan.is_some(),egui::Button::new("Replan unlocked")).clicked(){
                match caps(&self.capacities){
                    Ok(c)=>{let p=self.plan.as_ref().unwrap().clone();let s=self.strategy;let pattern=self.pattern.clone();self.spawn(ctx,move||Ok(Work::Plan(editing::replan(&p,&c,s,Some(&pattern))?)));}
                    Err(e)=>self.status=e.to_string(),
                }
            }
        });
        let Some(p)=self.plan.as_mut() else{return;};
        ui.separator();
        ui.label(format!("{} discs | {} unassigned",p.discs.len(),p.oversized.len()));
        ui.horizontal(|ui|{ui.label("Naming pattern");ui.text_edit_singleline(&mut self.pattern);if ui.button("Apply").clicked(){self.status=editing::rename(p,&self.pattern).map(|_|{self.dirty=true;"Names updated".to_owned()}).unwrap_or_else(|e|e.to_string());}});
        ui.horizontal(|ui|{
            if ui.button("Add disc using first capacity").clicked(){self.status=caps(&self.capacities).and_then(|c|editing::add_disc(p,c[0])).map(|_|{self.dirty=true;"Disc added".into()}).unwrap_or_else(|e|e.to_string());}
        });
        egui::ScrollArea::vertical().id_salt("discs").max_height(220.).show_rows(ui,26.,p.discs.len(),|ui,range|{
            for j in range{let d=&p.discs[j];ui.horizontal(|ui|{
                if ui.selectable_label(self.selected==Some(d.number),format!("{} {}",d.label,if d.locked{"[locked]"}else{""})).clicked(){self.selected=Some(d.number);}
                let usable=d.capacity-p.reserve;
                ui.add(egui::ProgressBar::new((d.estimated_used as f64/usable as f64) as f32).desired_width(180.).text(format!("{} / {}",gb(d.estimated_used),gb(d.capacity))));
                ui.label(format!("{} files",d.files.len()));
            });}
        });
        if let Some(n)=self.selected{
            if let Some(d)=p.discs.iter().find(|d|d.number==n){
                let locked=d.locked;
                if ui.button(if locked{"Unlock selected disc"}else{"Lock selected disc"}).clicked(){self.status=editing::set_lock(p,n,!locked).map(|_|{self.dirty=true;"Lock updated".into()}).unwrap_or_else(|e|e.to_string());}
            }
        }
        ui.horizontal(|ui|{ui.label("Filter files");ui.text_edit_singleline(&mut self.search);});
        let filter=self.search.to_lowercase();
        let entries=if let Some(n)=self.selected{p.discs.iter().find(|d|d.number==n).map(|d|d.files.as_slice()).unwrap_or(&[])}else{p.oversized.as_slice()};
        if ui.button("Show unassigned files").clicked(){self.selected=None;}
        let visible:Vec<_>=entries.iter().filter(|f|filter.is_empty()||f.archive.to_lowercase().contains(&filter)).collect();
        egui::ScrollArea::vertical().id_salt("files").max_height(240.).show_rows(ui,22.,visible.len(),|ui,range|{
            for j in range{let f=visible[j];if ui.selectable_label(self.move_path==f.archive,format!("{} | {}",f.archive,gb(f.bytes))).clicked(){self.move_path=f.archive.clone();}}
        });
        ui.horizontal(|ui|{ui.label("Move archive path");ui.text_edit_singleline(&mut self.move_path);ui.label("to disc");ui.add(egui::DragValue::new(&mut self.destination).range(1..=usize::MAX));if ui.button("Move").clicked(){self.status=editing::move_file(p,&self.move_path,self.destination).map(|_|{self.dirty=true;"File moved".into()}).unwrap_or_else(|e|e.to_string());}});
    }
}
impl eframe::App for Studio{
    fn update(&mut self,ctx:&egui::Context,_frame:&mut eframe::Frame){
        self.poll();
        if ctx.input(|i|i.viewport().close_requested()) && (self.dirty||self.worker.is_some()) {ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);self.close_prompt=true;}
        if self.close_prompt {egui::Window::new("Close application?").collapsible(false).show(ctx,|ui|{ui.label("Unsaved plan changes or running work may be lost.");if ui.button("Keep working").clicked(){self.close_prompt=false;}if ui.button("Discard and exit").clicked(){self.close_prompt=false;self.dirty=false;self.worker=None;ctx.send_viewport_cmd(egui::ViewportCommand::Close);}});}
        if self.worker.is_none(){
            let dropped=ctx.input(|i|i.raw.dropped_files.clone());
            for f in dropped{if let Some(p)=f.path{self.add(p);}}
        }
        egui::TopBottomPanel::top("top").show(ctx,|ui|{
            ui.horizontal_wrapped(|ui|{
                ui.heading("Disc Archive Studio");ui.label("Planning preview - no optical writing");if self.dirty{ui.label("Unsaved plan");}
                if ui.button("Toggle theme").clicked(){let dark=ctx.style().visuals.dark_mode;ctx.set_visuals(if dark{egui::Visuals::light()}else{egui::Visuals::dark()});}
            });
            ui.add_enabled_ui(self.worker.is_none(),|ui|{
                ui.horizontal_wrapped(|ui|{
                    if ui.add_enabled(!self.dirty&&self.plan.is_none()&&self.inventory.is_none(),egui::Button::new("Open project")).clicked(){if let Some(path)=rfd::FileDialog::new().add_filter("Archive project",&["json"]).pick_file(){self.spawn(ctx,move||Ok(Work::Project(disc_archive::project::load(&path)?)));}}
                    if ui.button("Save project as").clicked(){match self.snapshot(){Ok(p)=>{if let Some(path)=rfd::FileDialog::new().set_file_name("archive.das.json").save_file(){self.spawn(ctx,move||{disc_archive::project::save(&path,&p)?;Ok(Work::Saved("Project saved".into()))});}},Err(e)=>self.status=e.to_string()}}
                    if ui.button("Close project").clicked(){self.tab=5;}
                    if ui.button("Load inventory").clicked()&&self.plan.is_none(){if let Some(path)=rfd::FileDialog::new().add_filter("JSON",&["json"]).pick_file(){
                        self.spawn(ctx,move||Ok(Work::Inventory(serde_json::from_reader(BufReader::new(File::open(path)?))?)));
                    }}
                    if ui.add_enabled(self.inventory.is_some(),egui::Button::new("Save inventory")).clicked(){if let Some(path)=rfd::FileDialog::new().set_file_name("inventory.json").save_file(){self.status=save_new(path,self.inventory.as_ref().unwrap()).map(|_|"Inventory saved".into()).unwrap_or_else(|e|e.to_string());}}
                    if ui.add_enabled(self.plan.is_none(),egui::Button::new("Load plan")).clicked(){if let Some(path)=rfd::FileDialog::new().add_filter("JSON",&["json"]).pick_file(){self.spawn(ctx,move||{let p:Plan=serde_json::from_reader(BufReader::new(File::open(path)?))?;editing::validate(&p)?;Ok(Work::Plan(p))});}}
                    if ui.add_enabled(self.plan.is_some(),egui::Button::new("Save plan as")).clicked(){if let Some(path)=rfd::FileDialog::new().set_file_name("plan.json").save_file(){let p=self.plan.as_ref().unwrap().clone();self.spawn(ctx,move||{save_new(path,&p)?;Ok(Work::Saved("Plan saved".into()))});}}
                    if ui.add_enabled(self.plan.is_some(),egui::Button::new("Close plan")).clicked(){self.status="Save the plan before closing; use the separate confirmation below.".into();self.tab=3;}
                });
            });
            ui.horizontal(|ui|{ui.label("Project name");if ui.text_edit_singleline(&mut self.project_name).changed(){self.dirty=true;}});
            ui.horizontal(|ui|{ui.selectable_value(&mut self.tab,0,"Sources");ui.selectable_value(&mut self.tab,1,"Planner");ui.selectable_value(&mut self.tab,2,"Comparison");ui.selectable_value(&mut self.tab,4,"Duplicates");});
        });
        egui::TopBottomPanel::bottom("status").show(ctx,|ui|{ui.horizontal(|ui|{if self.worker.is_some(){ui.spinner();}ui.label(&self.status);});});
        egui::CentralPanel::default().show(ctx,|ui|{
            ui.add_enabled_ui(self.worker.is_none(),|ui|{egui::ScrollArea::vertical().show(ui,|ui|match self.tab{
                0=>self.sources_ui(ui,ctx),
                1=>self.planner_ui(ui,ctx),
                2=>{ui.heading("Distribution comparison");for s in &self.comparison{ui.label(format!("{:?}: {} discs | {} assigned | {} unassigned | {} split folders | {:.2} GB used / {:.2} GB capacity",s.strategy,s.discs,s.files,s.unassigned,s.split_folders,s.estimated_used_bytes as f64/1e9,s.capacity_bytes as f64/1e9));}},
                4=>{ui.heading("Duplicate content");ui.label("Identical SHA-256 and file size. No files are removed.");for (i,group) in self.duplicate_groups.iter().enumerate(){ui.collapsing(format!("Group {} - {} files",i+1,group.len()),|ui|{for path in group{ui.label(path);}});}},
                5=>{ui.label("Closing the project discards all unsaved project settings, inventory and plan. Save a new project revision first if needed.");if ui.button("Confirm close project").clicked(){*self=Self::default();}if ui.button("Cancel").clicked(){self.tab=0;}},
                _=>{ui.label("Closing disc plan discards unsaved edits.");if ui.button("Confirm close plan").clicked(){self.plan=None;self.selected=None;self.dirty=false;self.tab=0;self.status="Plan closed".into();}if ui.button("Cancel").clicked(){self.tab=1;}}
            });});
        });
    }
}
fn main()->eframe::Result<()>{
    eframe::run_native("Disc Archive Studio",eframe::NativeOptions{viewport:egui::ViewportBuilder::default().with_inner_size([1100.,800.]),..Default::default()},Box::new(|_|Ok(Box::<Studio>::default())))
}
