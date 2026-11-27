#![cfg_attr(target_os="windows", windows_subsystem="windows")]
use disc_archive::{scan,plan_mixed,ScanOptions,Inventory,Plan,Strategy,MEDIA};
use disc_archive::editing;
use eframe::egui;
use std::{path::PathBuf,sync::mpsc::{self,Receiver},fs::File,io::{BufReader,BufWriter,Write}};
enum Work { Inventory(Inventory), Plan(Plan), Comparison(Vec<editing::Summary>), Duplicates(Vec<Vec<String>>), Saved(String), Exported(String), Project(disc_archive::project::Project), CatalogOpened(PathBuf,Vec<disc_archive::catalog::ProjectRow>), CatalogResults(Vec<disc_archive::catalog::Match>,usize), CatalogImported(i64,Vec<disc_archive::catalog::ProjectRow>), CatalogLinked, ImageCreated(disc_archive::image::ImageReceipt), ImageVerified(disc_archive::image::VerificationReceipt), ManifestSaved(PathBuf), Drives(disc_archive::optical::Discovery), Jobs(PathBuf,Vec<disc_archive::jobs::Job>,String), BatchPreview(Vec<disc_archive::optical::BurnPreview>), BatchDone(Vec<disc_archive::batch::Outcome>,Vec<disc_archive::jobs::Job>), Error(String) }
struct Studio {
    burn_requests:Vec<disc_archive::batch::Request>, burn_previews:Vec<disc_archive::optical::BurnPreview>, burn_confirm:bool, burn_progress:Option<Receiver<(String,disc_archive::optical::BurnStage)>>, burn_stages:std::collections::BTreeMap<String,disc_archive::optical::BurnStage>, burn_outcomes:Vec<disc_archive::batch::Outcome>,
    drives:Option<disc_archive::optical::Discovery>,
    journal_path:Option<PathBuf>, jobs:Vec<disc_archive::jobs::Job>, job_selection:Option<i64>, job_recorder:String, reconcile_note:String, reconcile_confirm:bool, verify_confirm:bool,
    sources:Vec<PathBuf>, source_text:String, extensions:String, exclusions:String, hash:bool,
    capacities:String, reserve:String, strategy:Strategy, inventory:Option<Inventory>, plan:Option<Plan>,
    comparison:Vec<editing::Summary>, worker:Option<Receiver<Work>>, status:String, selected:Option<usize>,
    pattern:String, move_path:String, destination:usize, search:String, planner_folder:Option<String>, tab:usize,
    inventory_stale:bool, inventory_search:String, duplicate_groups:Vec<Vec<String>>, dirty:bool, close_prompt:bool,
    project_name:String,
    image_disc:usize, image_receipt:Option<disc_archive::image::ImageReceipt>, verify_receipt:Option<disc_archive::image::VerificationReceipt>, image_path:Option<PathBuf>, manifest_path:Option<PathBuf>,
    catalog_path:Option<PathBuf>, catalog_projects:Vec<disc_archive::catalog::ProjectRow>, catalog_rows:Vec<disc_archive::catalog::Match>, catalog_term:String, catalog_project:Option<i64>, catalog_offset:usize, catalog_has_next:bool, catalog_query:Option<(String,Option<i64>)>,
}
impl Default for Studio {
    fn default()->Self{Self{burn_requests:vec![],burn_previews:vec![],burn_confirm:false,burn_progress:None,burn_stages:Default::default(),burn_outcomes:vec![],drives:None,journal_path:None,jobs:vec![],job_selection:None,job_recorder:String::new(),reconcile_note:String::new(),reconcile_confirm:false,verify_confirm:false,sources:vec![],source_text:String::new(),extensions:String::new(),exclusions:String::new(),hash:false,
        capacities:"50000000000".into(),reserve:"50000000".into(),strategy:Strategy::Hybrid,inventory:None,plan:None,comparison:vec![],
        worker:None,status:"Add files or folders to begin.".into(),selected:None,pattern:"DISC_{n:03}".into(),move_path:String::new(),destination:1,search:String::new(),planner_folder:None,tab:11,inventory_stale:false,inventory_search:String::new(),duplicate_groups:vec![],dirty:false,close_prompt:false,project_name:"Untitled archive".into(),image_disc:1,image_receipt:None,verify_receipt:None,image_path:None,manifest_path:None,catalog_path:None,catalog_projects:vec![],catalog_rows:vec![],catalog_term:String::new(),catalog_project:None,catalog_offset:0,catalog_has_next:false,catalog_query:None}}
}
fn caps(text:&str)->anyhow::Result<Vec<u64>> {let v=text.split(',').map(|s|s.trim().parse::<u64>()).collect::<Result<Vec<_>,_>>()?;Ok(v)}
fn save_new<T:serde::Serialize>(path:PathBuf,value:&T)->anyhow::Result<()>{
    let mut f=BufWriter::new(File::options().write(true).create_new(true).open(path)?);
    serde_json::to_writer_pretty(&mut f,value)?;f.flush()?;Ok(())
}
fn gb(n:u64)->String{format!("{:.2} GB",n as f64/1e9)}
impl Studio{
    fn source_changed(&mut self){
        self.dirty=true;
        self.inventory_stale=self.inventory.is_some();
        self.comparison.clear();
        self.duplicate_groups.clear();
    }
    fn add(&mut self,p:PathBuf){
        if self.plan.is_some(){self.status="Close the existing plan before changing sources.".into();return;}
        if !self.sources.contains(&p){self.sources.push(p);self.source_changed();}
    }
    fn inventory_ready(&self)->bool{
        !self.inventory_stale&&self.inventory.as_ref().is_some_and(|i|!i.files.is_empty()&&i.errors.is_empty())
    }
    fn spawn(&mut self,ctx:&egui::Context,f:impl FnOnce()->anyhow::Result<Work>+Send+'static){
        let (tx,rx)=mpsc::channel();let ctx=ctx.clone();
        std::thread::spawn(move||{let result=f().unwrap_or_else(|e|Work::Error(format!("{e:#}")));let _=tx.send(result);ctx.request_repaint();});
        self.worker=Some(rx);self.status="Working in background...".into();
    }
    fn poll(&mut self){
        if let Some(rx)=&self.burn_progress{for (id,stage) in rx.try_iter(){self.burn_stages.insert(id,stage);}}
        let event=match self.worker.as_ref().map(|r|r.try_recv()){
            Some(Ok(v))=>Some(v),Some(Err(mpsc::TryRecvError::Disconnected))=>Some(Work::Error("Worker stopped unexpectedly".into())),_=>None};
        if let Some(event)=event{
            self.worker=None;
            match event{
                Work::Inventory(i)=>{self.status=format!("Scanned {} files; {} exclusions; {} errors",i.files.len(),i.excluded.len(),i.errors.len());self.inventory=Some(i);self.plan=None;self.comparison.clear();self.duplicate_groups.clear();self.inventory_stale=false;self.dirty=true;self.tab=0;}
                Work::Plan(p)=>{self.status=format!("{} discs; {} unassigned files",p.discs.len(),p.oversized.len());self.selected=p.discs.first().map(|d|d.number);self.planner_folder=None;self.move_path.clear();self.reserve=p.reserve.to_string();self.strategy=p.strategy;self.plan=Some(p);self.comparison.clear();self.dirty=true;self.tab=1;}
                Work::Comparison(v)=>{self.comparison=v;self.status="Comparison complete".into();self.tab=2;}
                Work::Duplicates(v)=>{self.status=format!("{} duplicate groups",v.len());self.duplicate_groups=v;self.tab=4;}
                Work::Project(p)=>{self.project_name=p.name;self.sources=p.sources;self.extensions=p.extensions.join(",");self.exclusions=p.exclusions.join("\n");self.hash=p.hash;self.capacities=p.capacities.iter().map(u64::to_string).collect::<Vec<_>>().join(",");self.reserve=p.reserve.to_string();self.strategy=p.strategy;self.pattern=p.naming_pattern;self.inventory=p.inventory;self.plan=p.plan;self.selected=self.plan.as_ref().and_then(|p|p.discs.first()).map(|d|d.number);self.comparison.clear();self.duplicate_groups.clear();self.planner_folder=None;self.move_path.clear();self.tab=if self.plan.is_some(){1}else{0};self.inventory_stale=false;self.dirty=false;self.status="Project opened".into();}
                Work::Saved(message)=>{self.status=message;self.dirty=false;}
                Work::Exported(message)=>self.status=message,
                Work::CatalogOpened(path,projects)=>{self.catalog_path=Some(path);self.catalog_projects=projects;self.catalog_rows.clear();self.catalog_project=None;self.catalog_offset=0;self.catalog_has_next=false;self.catalog_query=None;self.status="Catalog opened".into();self.tab=6;}
                Work::CatalogResults(mut rows,offset)=>{self.catalog_has_next=rows.len()>100;rows.truncate(100);self.catalog_rows=rows;self.catalog_offset=offset;self.status=format!("Catalog page {}",offset/100+1);}
                Work::CatalogImported(id,projects)=>{self.catalog_projects=projects;self.catalog_rows.clear();self.catalog_query=None;self.catalog_has_next=false;self.catalog_offset=0;self.catalog_project=Some(id);self.status=format!("Imported planned snapshot {}",id);}
                Work::CatalogLinked=>{self.catalog_rows.clear();self.catalog_query=None;self.catalog_has_next=false;self.catalog_offset=0;self.status="Historical verification linked. Search to refresh.".into();}
                Work::ImageCreated(receipt)=>{self.status=format!("Image created and {} files verified",receipt.verified_files);self.image_path=Some(receipt.output.clone());self.image_receipt=Some(receipt);}
                Work::ImageVerified(receipt)=>{self.status=format!("Image verified: {} files",receipt.files);self.verify_receipt=Some(receipt);}
                Work::ManifestSaved(path)=>{self.status=format!("Preparation manifest saved: {}",path.display());self.manifest_path=Some(path);}
                Work::Drives(d)=>{self.status=format!("{} recorders detected; {} discovery errors",d.recorders.len(),d.errors.len());self.drives=Some(d);}
                Work::Jobs(path,jobs,message)=>{if self.journal_path.as_ref()!=Some(&path){self.burn_previews.clear();self.burn_confirm=false;}self.journal_path=Some(path);self.jobs=jobs;self.status=message;self.reconcile_confirm=false;self.verify_confirm=false;}
                Work::BatchPreview(v)=>{self.burn_previews=v;self.burn_confirm=false;self.status="Batch preview complete; inspect each target before authorizing".into();}
                Work::BatchDone(outcomes,jobs)=>{self.burn_progress=None;self.jobs=jobs;self.status=format!("Batch finished: {} written, {} errors; physical verification separate",outcomes.iter().filter(|o|o.receipt.is_some()).count(),outcomes.iter().filter(|o|o.error.is_some()).count());self.burn_outcomes=outcomes;}
                Work::Error(e)=>{self.burn_progress=None;self.burn_confirm=false;self.status=e;self.reconcile_confirm=false;self.verify_confirm=false;},
            }
        }
    }
    fn snapshot(&self)->anyhow::Result<disc_archive::project::Project>{
        if self.inventory_stale{anyhow::bail!("Rescan changed sources before saving a project with inventory.");}
        let (reserve,strategy)=self.plan.as_ref().map(|p|(p.reserve,p.strategy)).unwrap_or((self.reserve.trim().parse()?,self.strategy));
        Ok(disc_archive::project::Project{name:self.project_name.clone(),sources:self.sources.clone(),extensions:self.extensions.split(',').map(str::trim).filter(|s|!s.is_empty()).map(str::to_owned).collect(),exclusions:self.exclusions.lines().map(str::trim).filter(|s|!s.is_empty()).map(str::to_owned).collect(),hash:self.hash,capacities:caps(&self.capacities)?,reserve,strategy,naming_pattern:self.pattern.clone(),inventory:self.inventory.clone(),plan:self.plan.clone(),..Default::default()})
    }
    fn begin_plan(&mut self,ctx:&egui::Context,compare:bool){
        let result=(||->anyhow::Result<_>{
            if self.inventory_stale{anyhow::bail!("Source settings changed; scan sources again before planning.");}
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
    fn catalog_search(&mut self,ctx:&egui::Context,offset:usize){
        let Some(path)=self.catalog_path.clone() else{return;};
        let term=self.catalog_term.clone();let project=self.catalog_project;
        self.catalog_query=Some((term.clone(),project));
        self.catalog_rows.clear();self.catalog_has_next=false;
        self.spawn(ctx,move||{let db=disc_archive::catalog::Catalog::open(&path,false)?;Ok(Work::CatalogResults(db.search(&term,project,101,offset)?,offset))});
    }
    fn overview_ui(&mut self,ui:&mut egui::Ui,ctx:&egui::Context){
        ui.add_space(12.0);
        ui.label(egui::RichText::new("Your archive workspace").size(28.0).strong());
        ui.label("Create an archive from files or folders, or resume a saved project.");
        ui.add_space(12.0);
        ui.horizontal_wrapped(|ui|{
            if ui.add_sized([180.0,42.0],egui::Button::new("Start selecting files")).clicked(){self.tab=0;}
            let can_open=!self.dirty&&self.plan.is_none()&&self.inventory.is_none();
            if ui.add_enabled(can_open,egui::Button::new("Open saved project").min_size(egui::vec2(180.0,42.0))).clicked(){
                if let Some(path)=rfd::FileDialog::new().add_filter("Archive project",&["json"]).pick_file(){
                    self.spawn(ctx,move||Ok(Work::Project(disc_archive::project::load(&path)?)));
                }
            }
        });
        ui.add_space(14.0);
        ui.group(|ui|{
            ui.heading(&self.project_name);
            ui.horizontal_wrapped(|ui|{
                ui.label(format!("{} sources",self.sources.len()));
                ui.separator();
                ui.label(match &self.inventory{Some(i)=>format!("{} inventoried files",i.files.len()),None=>"Not scanned".into()});
                ui.separator();
                ui.label(match &self.plan{Some(p)=>format!("{} planned discs",p.discs.len()),None=>"Not planned".into()});
            });
        });
        ui.add_space(14.0);
        ui.heading("Archive workflow");
        for (id,title,description) in [
            (0,"1. Select and scan sources","Choose files, folders or network shares; inventory without changing original files."),
            (1,"2. Plan disc distribution","Choose media capacity, compare allocation strategies and adjust assignments."),
            (7,"3. Create and verify images","Generate UDF images and preparation manifests; validate contents before writing."),
            (10,"4. Burn discs","Experimental physical writing. Explicit authorization and a burn journal are required."),
            (9,"5. Verify and review jobs","Read back written discs and review persistent job history."),
            (6,"6. Search your archive","Browse planned and historically verified catalog entries."),
        ]{
            ui.add_space(5.0);
            ui.group(|ui|{
                ui.horizontal(|ui|{
                    ui.vertical(|ui|{ui.strong(title);ui.label(description);});
                });
                if ui.button("Open workspace").clicked(){self.tab=id;}
            });
        }
        ui.add_space(12.0);
        ui.label(egui::RichText::new("Preview notice: physical optical writing has not yet passed hardware acceptance.").weak());
    }
    fn catalog_ui(&mut self,ui:&mut egui::Ui,ctx:&egui::Context){
        use disc_archive::catalog::Catalog;
        ui.heading("Archive catalog");
        ui.label("Search saved assignments without connecting the source drives. Planned does not mean burned or verified.");
        ui.horizontal_wrapped(|ui|{
            if ui.button("Create catalog").clicked(){
                if let Some(path)=rfd::FileDialog::new().set_file_name("archive.sqlite").save_file(){
                    self.spawn(ctx,move||{let db=Catalog::create(&path)?;let rows=db.projects()?;Ok(Work::CatalogOpened(path,rows))});
                }
            }
            if ui.button("Open catalog").clicked(){
                if let Some(path)=rfd::FileDialog::new().add_filter("SQLite catalog",&["sqlite","db"]).pick_file(){
                    self.spawn(ctx,move||{let db=Catalog::open(&path,false)?;let rows=db.projects()?;Ok(Work::CatalogOpened(path,rows))});
                }
            }
            if ui.add_enabled(self.catalog_path.is_some(),egui::Button::new("Close catalog")).clicked(){
                self.catalog_path=None;self.catalog_projects.clear();self.catalog_rows.clear();self.catalog_project=None;self.catalog_query=None;self.catalog_has_next=false;self.catalog_offset=0;
            }
        });
        let Some(path)=self.catalog_path.clone() else{return;};
        ui.label(path.display().to_string());
        ui.horizontal_wrapped(|ui|{
            if ui.button("Import saved project").clicked(){
                if let Some(project)=rfd::FileDialog::new().add_filter("Archive project",&["json"]).pick_file(){
                    let path=path.clone();
                    self.spawn(ctx,move||{let p=disc_archive::project::load(&project)?;let mut db=Catalog::open(&path,true)?;let id=db.import(&p)?;Ok(Work::CatalogImported(id,db.projects()?))});
                }
            }
            if ui.add_enabled(self.plan.is_some(),egui::Button::new("Import current plan snapshot")).clicked(){
                match self.snapshot(){
                    Ok(p)=>{let path=path.clone();self.spawn(ctx,move||{let mut db=Catalog::open(&path,true)?;let id=db.import(&p)?;Ok(Work::CatalogImported(id,db.projects()?))});}
                    Err(e)=>self.status=e.to_string(),
                }
            }
        });
        ui.label("Each import adds a separate planned snapshot. It does not replace previous imports.");
        let label=self.catalog_project.and_then(|id|self.catalog_projects.iter().find(|p|p.id==id)).map(|p|format!("{} (# {})",p.name,p.id)).unwrap_or_else(||"All projects".into());
        egui::ComboBox::from_label("Project filter").selected_text(label).show_ui(ui,|ui|{
            ui.selectable_value(&mut self.catalog_project,None,"All projects");
            for p in &self.catalog_projects{ui.selectable_value(&mut self.catalog_project,Some(p.id),format!("{} (# {}) - {} files, {} discs",p.name,p.id,p.files,p.discs));}
        });
        ui.separator();
        ui.label("Link historical readback evidence from the selected Burn journal attempt. This does not assert current media condition.");
        let target=self.catalog_project.zip(self.journal_path.clone()).zip(self.job_selection).and_then(|((project,journal),attempt)|{
            self.jobs.iter().find(|j|j.id==attempt).map(|j|(project,journal,attempt,j.disc_number))
        });
        if let Some((project,journal,attempt,disc))=target {
            ui.label(format!("Project #{} | disc #{} | attempt #{} | {}",project,disc,attempt,journal.display()));
            if ui.button("Link selected attempt to selected project").clicked(){
                let path=path.clone();
                self.spawn(ctx,move||{let mut db=Catalog::open(&path,true)?;db.link_verified(project,disc,&journal,attempt)?;Ok(Work::CatalogLinked)});
            }
        }else{ui.label("Select a project and open a burn journal with an attempt selected.");}
        ui.horizontal(|ui|{ui.label("Filename, path or hash");ui.text_edit_singleline(&mut self.catalog_term);if ui.button("Search").clicked(){self.catalog_search(ctx,0);}});
        let current=self.catalog_query.as_ref().map(|(term,project)|term==&self.catalog_term&&project==&self.catalog_project).unwrap_or(false);
        if !current{ui.label("Search to refresh results for the current filters.");return;}
        ui.horizontal(|ui|{
            if ui.add_enabled(self.catalog_offset>0,egui::Button::new("Previous")).clicked(){self.catalog_search(ctx,self.catalog_offset.saturating_sub(100));}
            ui.label(format!("Page {} - {} results",self.catalog_offset/100+1,self.catalog_rows.len()));
            if ui.add_enabled(self.catalog_has_next,egui::Button::new("Next")).clicked(){self.catalog_search(ctx,self.catalog_offset.saturating_add(100));}
        });
        egui::ScrollArea::vertical().id_salt("catalog_results").max_height(480.).show_rows(ui,70.,self.catalog_rows.len(),|ui,range|{
            for j in range{
                let f=&self.catalog_rows[j];
                ui.group(|ui|{
                    ui.label(&f.archive);
                    ui.label(format!("{} (# {}) | {} | {} | {} bytes",f.project,f.project_id,f.disc_label.as_deref().unwrap_or("Unassigned"),f.status,f.bytes));
                    if let Some(at)=&f.verified_at{ui.label(format!("Historical readback: {}",at));}
                    ui.label(&f.source);
                });
            }
        });
    }
    fn burn_ui(&mut self,ui:&mut egui::Ui,ctx:&egui::Context){
        use disc_archive::{batch,optical,jobs::Journal};
        ui.heading("Experimental concurrent burning");
        ui.label("Physical writing requires hardware acceptance. Use expendable blank write-once media only. No automatic retry.");
        if let Some(p)=&self.journal_path{ui.label(format!("Journal: {}",p.display()));}else{ui.label("Open a burn journal in the Burn journal tab first.");}
        let mut changed=false;
        if ui.button("Load explicit batch assignments (JSON)").clicked(){
            if let Some(path)=rfd::FileDialog::new().add_filter("Batch JSON",&["json"]).pick_file(){
                self.burn_requests.clear();changed=true;
                match File::open(path).map_err(anyhow::Error::from).and_then(|f|serde_json::from_reader::<_,Vec<batch::Request>>(BufReader::new(f)).map_err(anyhow::Error::from)).and_then(|v|{batch::validate(&v,"WRITE-ONCE")?;Ok(v)}){
                    Ok(v)=>self.burn_requests=v,Err(e)=>self.status=format!("{e:#}"),
                }
            }
        }
        if ui.button("Add assignment").clicked()&&self.burn_requests.len()<16{
            self.burn_requests.push(batch::Request{recorder:String::new(),image:PathBuf::new(),manifest:PathBuf::new(),sha256:String::new()});changed=true;
        }
        if ui.button("Refresh recorders").clicked(){self.drives=None;self.burn_previews.clear();self.burn_confirm=false;self.spawn(ctx,move||Ok(Work::Drives(optical::discover()?)));return;}
        let mut remove=None;
        for (i,r) in self.burn_requests.iter_mut().enumerate(){
            ui.push_id(i,|ui|ui.group(|ui|{
                ui.label(format!("Assignment {}",i+1));
                let prior=r.recorder.clone();
                egui::ComboBox::from_label("Recorder").selected_text(if r.recorder.is_empty(){"Select recorder"}else{&r.recorder}).show_ui(ui,|ui|{
                    if let Some(d)=&self.drives{for rec in &d.recorders{ui.selectable_value(&mut r.recorder,rec.id.clone(),format!("{} {} {}",rec.vendor,rec.product,rec.paths.join(", ")));}}
                });
                changed|=prior!=r.recorder;
                if ui.button("Choose ISO").clicked(){if let Some(p)=rfd::FileDialog::new().add_filter("Image",&["iso"]).pick_file(){r.image=p;changed=true;}}
                ui.label(r.image.display().to_string());
                if ui.button("Choose manifest").clicked(){if let Some(p)=rfd::FileDialog::new().add_filter("Manifest",&["json"]).pick_file(){r.manifest=p;changed=true;}}
                ui.label(r.manifest.display().to_string());
                ui.label("Expected SHA-256 (from verified image receipt)");
                changed|=ui.text_edit_singleline(&mut r.sha256).changed();
                if ui.button("Remove assignment").clicked(){remove=Some(i);}
            }));
        }
        if let Some(i)=remove{self.burn_requests.remove(i);changed=true;}
        if changed{self.burn_previews.clear();self.burn_confirm=false;}
        if ui.add_enabled(!self.burn_requests.is_empty(),egui::Button::new("Validate all assignments")).clicked(){
            self.burn_previews.clear();self.burn_confirm=false;
            let requests=self.burn_requests.clone();
            self.spawn(ctx,move||{
                batch::validate(&requests,"WRITE-ONCE")?;
                let mut previews=Vec::new();
                let mut paths=std::collections::HashSet::new();
                for r in requests{
                    let image=r.image.canonicalize()?;
                    if !paths.insert(image.to_string_lossy().to_lowercase()){anyhow::bail!("Duplicate image path");}
                    let manifest=serde_json::from_reader(BufReader::new(File::open(r.manifest)?))?;
                    let p=optical::burn_preview(&manifest,&image,&r.recorder)?;
                    if !p.image_sha256.eq_ignore_ascii_case(&r.sha256){anyhow::bail!("Approved SHA-256 mismatch");}
                    previews.push(p);
                }
                Ok(Work::BatchPreview(previews))
            });return;
        }
        for p in &self.burn_previews{ui.label(format!("{} | {} | {} bytes / {} free | SHA-256 {}",p.recorder_id,p.media_type,p.image_bytes,p.free_bytes,p.image_sha256));}
        let ready=batch_preview_matches(&self.burn_requests,&self.burn_previews)&&self.journal_path.is_some();
        if !ready {self.burn_confirm=false;}
        ui.add_enabled_ui(ready,|ui|{ui.checkbox(&mut self.burn_confirm,"I authorize irreversible writing to every listed recorder and confirm the intended blank media are inserted.");});
        if ui.add_enabled(ready&&self.burn_confirm,egui::Button::new("Start authorized batch")).clicked(){
            let path=self.journal_path.clone().unwrap();let requests=self.burn_requests.clone();
            self.burn_confirm=false;self.burn_previews.clear();self.burn_outcomes.clear();self.burn_stages.clear();
            let (tx,rx)=mpsc::channel();self.burn_progress=Some(rx);
            let repaint=ctx.clone();
            self.spawn(ctx,move||{
                let outcomes=batch::execute(requests,&path,"WRITE-ONCE",|id,stage|{let _=tx.send((id.to_owned(),stage));repaint.request_repaint();})?;
                let jobs=Journal::open(&path)?.list()?;
                Ok(Work::BatchDone(outcomes,jobs))
            });
        }
        for o in &self.burn_outcomes{ui.label(format!("{}: {}",o.recorder,o.error.as_deref().unwrap_or("Written, unverified")));}
    }
    fn jobs_ui(&mut self,ui:&mut egui::Ui,ctx:&egui::Context){
        use disc_archive::jobs::Journal;
        ui.heading("Burn journal");
        ui.label("Manage recorded attempts and read back an intended disc. This tab does not initiate burning.");
        let mut open=None;
        if ui.button("Create journal").clicked(){if let Some(p)=rfd::FileDialog::new().set_file_name("jobs.sqlite").save_file(){open=Some((p,true));}}
        if ui.button("Open journal").clicked(){if let Some(p)=rfd::FileDialog::new().add_filter("Burn journal",&["sqlite","db"]).pick_file(){open=Some((p,false));}}
        if let Some((path,create))=open{
            self.burn_previews.clear();self.burn_confirm=false;self.journal_path=None;self.jobs.clear();self.job_selection=None;self.job_recorder.clear();self.reconcile_note.clear();self.reconcile_confirm=false;self.verify_confirm=false;
            self.spawn(ctx,move||{let j=if create{Journal::create(&path)?}else{Journal::open(&path)?};Ok(Work::Jobs(path,j.list()?,"Journal opened".into()))});return;
        }
        let Some(path)=self.journal_path.clone() else{return;};
        ui.label(path.display().to_string());
        if ui.button("Refresh job history").clicked(){self.spawn(ctx,move||{let j=Journal::open(&path)?;Ok(Work::Jobs(path,j.list()?,"Journal refreshed".into()))});return;}
        let previous=self.job_selection;
        egui::ScrollArea::vertical().id_salt("job_history").max_height(240.).show_rows(ui,26.,self.jobs.len(),|ui,range|{
            for i in range{let j=&self.jobs[i];ui.selectable_value(&mut self.job_selection,Some(j.id),format!("#{} {} | {} | {}",j.id,j.label,j.state,j.verification_status.as_deref().unwrap_or("no readback")));}
        });
        if previous!=self.job_selection{self.reconcile_confirm=false;self.verify_confirm=false;self.reconcile_note.clear();self.job_recorder.clear();}
        let Some(j)=self.jobs.iter().find(|j|Some(j.id)==self.job_selection) else{return;};
        let id=j.id;
        let unresolved=(j.state=="in_progress"||j.state=="outcome_unknown")&&j.reconciliation.is_none();
        let written=j.state=="written_unverified";
        ui.separator();
        ui.label(format!("Recorder: {}",j.recorder));ui.label(format!("Image: {}",j.image));ui.label(format!("SHA-256: {}",j.sha256));
        ui.label(format!("Started: {} | Finished: {}",j.started,j.finished.as_deref().unwrap_or("not recorded")));
        ui.label(&j.detail);
        if let Some(note)=&j.reconciliation{ui.label(format!("Reconciliation: {} ({})",note,j.reconciled_at.as_deref().unwrap_or("unknown")));}
        if unresolved{
            ui.label("Stop any orphaned writer and remove suspect media before reconciling. This does not assert a successful burn.");
            ui.text_edit_multiline(&mut self.reconcile_note);
            ui.checkbox(&mut self.reconcile_confirm,"I confirmed the writer has stopped and suspect media was removed.");
            if ui.add_enabled(self.reconcile_confirm&&!self.reconcile_note.trim().is_empty(),egui::Button::new("Record reconciliation")).clicked(){
                let note=self.reconcile_note.clone();self.reconcile_confirm=false;
                self.spawn(ctx,move||{let j=Journal::open(&path)?;j.reconcile(id,"INSPECTED-REMOVE-MEDIA",&note)?;Ok(Work::Jobs(path,j.list()?,"Reconciliation recorded".into()))});return;
            }
        }
        if written{
            ui.label("Readback uses the stored manifest. Insert the intended disc and leave it inserted throughout verification.");
            let prior=self.job_recorder.clone();
            egui::ComboBox::from_label("Readback recorder").selected_text(if self.job_recorder.is_empty(){"Select recorder"}else{&self.job_recorder}).show_ui(ui,|ui|{
                if let Some(d)=&self.drives{for r in &d.recorders{ui.selectable_value(&mut self.job_recorder,r.id.clone(),format!("{} {} {}",r.vendor,r.product,r.paths.join(", ")));}}
            });
            if prior!=self.job_recorder{self.verify_confirm=false;}
            if ui.button("Refresh recorder list").clicked(){self.job_recorder.clear();self.verify_confirm=false;self.drives=None;self.spawn(ctx,move||Ok(Work::Drives(disc_archive::optical::discover()?)));return;}
            ui.checkbox(&mut self.verify_confirm,"The intended disc is inserted in the selected recorder.");
            if ui.add_enabled(self.verify_confirm&&!self.job_recorder.is_empty(),egui::Button::new("Verify recorded disc")).clicked(){
                let recorder=self.job_recorder.clone();self.verify_confirm=false;
                self.spawn(ctx,move||{let j=Journal::open(&path)?;let message=match disc_archive::optical::verify_logged_disc(&j,id,&recorder){Ok(r)=>format!("Verified {} files",r.files),Err(e)=>format!("Verification failed: {e:#}")};Ok(Work::Jobs(path,j.list()?,message))});
            }
        }
    }
    fn drives_ui(&mut self,ui:&mut egui::Ui,ctx:&egui::Context){
        ui.heading("Optical recorders");
        ui.label("Read-only snapshot. Refresh after changing media. No writing or eject controls are available.");
        if ui.button("Refresh recorders and media").clicked(){
            self.drives=None;
            self.spawn(ctx,move||Ok(Work::Drives(disc_archive::optical::discover()?)));
        }
        let Some(d)=&self.drives else{ui.label("No current discovery result.");return;};
        for e in &d.errors{ui.label(format!("Discovery error: {e}"));}
        if d.recorders.is_empty(){ui.label("No recorders reported by IMAPI.");}
        for r in &d.recorders{
            ui.group(|ui|{
                ui.heading(format!("{} {} ({})",r.vendor,r.product,r.revision));
                ui.label(format!("Paths: {}",r.paths.join(", ")));
                ui.collapsing("Recorder identity and profiles",|ui|{ui.label(&r.id);ui.label(format!("{:?}",r.profiles));});
                if let Some(m)=&r.media{
                    ui.label(format!("{} (type {}) | status 0x{:04X}",m.type_name(),m.physical_type,m.status));
                    ui.label(format!("Physically blank: {} | Heuristically blank: {}",m.physically_blank,m.heuristically_blank));
                    ui.label(format!("Free sectors: {} / {}",m.free_sectors,m.total_sectors));
                    if let Some(bytes)=m.free_bytes(){ui.label(format!("Reported free: {}",gb(bytes)));}
                    ui.label(if m.blank_candidate(){"Preliminary blank write-once candidate"}else{"Not a blank write-once candidate"});
                    if let Some(image)=&self.image_receipt{
                        ui.label(format!("Last created image {}: {}",image.label,if m.fits_image(image.bytes){"fits reported capacity (not authorization to burn)"}else{"not eligible or insufficient capacity"}));
                    }
                }
                if let Some(e)=&r.inspection_error{ui.label(format!("Media inspection: {e}"));}
            });
        }
    }
    fn image_ui(&mut self,ui:&mut egui::Ui,ctx:&egui::Context){
        ui.heading("Disc images");
        ui.label("Create UDF 2.01 images and verify their contents. No physical media is written.");
        ui.label("Use a local NTFS destination with free space for staging and the image; Windows mounting must be available.");
        let mut chosen=None;
        if let Some(p)=self.plan.as_ref(){
            if !p.discs.iter().any(|d|d.number==self.image_disc){self.image_disc=p.discs.first().map(|d|d.number).unwrap_or(0);}
            egui::ComboBox::from_label("Disc to prepare").selected_text(format!("Disc {}",self.image_disc)).show_ui(ui,|ui|{
                for d in &p.discs{ui.selectable_value(&mut self.image_disc,d.number,format!("{} - {} files",d.label,d.files.len()));}
            });
            if let Some(d)=p.discs.iter().find(|d|d.number==self.image_disc){
                ui.label(format!("{} | {} files | {} estimated / {} usable",d.label,d.files.len(),gb(d.estimated_used),gb(d.capacity-p.reserve)));
                let mut prepare=false;let mut create=false;
                ui.horizontal(|ui|{
                    prepare=ui.add_enabled(!d.files.is_empty(),egui::Button::new("Save preparation manifest")).clicked();
                    create=ui.add_enabled(!d.files.is_empty(),egui::Button::new("Create verified image")).clicked();
                });
                if prepare||create{chosen=Some((p.clone(),d.number,d.label.clone(),create));}
            }
        }else{ui.label("Open or create a disc plan to generate an image.");}
        if let Some((p,n,label,create))=chosen{
            if create{
                if let Some(path)=rfd::FileDialog::new().add_filter("UDF image",&["iso"]).set_file_name(format!("{label}.iso")).save_file(){
                    self.image_receipt=None;self.verify_receipt=None;
                    self.spawn(ctx,move||Ok(Work::ImageCreated(disc_archive::image::create_udf(&p,n,&path)?)));
                }
            }else if let Some(path)=rfd::FileDialog::new().add_filter("Preparation manifest",&["json"]).set_file_name(format!("{label}.prepared.json")).save_file(){
                self.spawn(ctx,move||{let manifest=disc_archive::image::prepare(&p,n)?;save_new(path.clone(),&manifest)?;Ok(Work::ManifestSaved(path))});
            }
        }
        ui.separator();
        ui.heading("Verify an existing image");
        ui.horizontal_wrapped(|ui|{
            if ui.button("Choose manifest").clicked(){if let Some(path)=rfd::FileDialog::new().add_filter("JSON",&["json"]).pick_file(){self.manifest_path=Some(path);self.verify_receipt=None;}}
            ui.label(self.manifest_path.as_ref().map(|p|p.display().to_string()).unwrap_or_else(||"No manifest selected".into()));
        });
        ui.horizontal_wrapped(|ui|{
            if ui.button("Choose image").clicked(){if let Some(path)=rfd::FileDialog::new().add_filter("UDF image",&["iso"]).pick_file(){self.image_path=Some(path);self.verify_receipt=None;}}
            ui.label(self.image_path.as_ref().map(|p|p.display().to_string()).unwrap_or_else(||"No image selected".into()));
        });
        if ui.add_enabled(self.manifest_path.is_some()&&self.image_path.is_some(),egui::Button::new("Verify image against manifest")).clicked(){
            let manifest=self.manifest_path.clone().unwrap();let image=self.image_path.clone().unwrap();
            self.verify_receipt=None;
            self.spawn(ctx,move||{let p:disc_archive::image::PreparedDisc=serde_json::from_reader(BufReader::new(File::open(manifest)?))?;Ok(Work::ImageVerified(disc_archive::image::verify_udf(&p,&image)?))});
        }
        if let Some(r)=&self.image_receipt{
            ui.separator();ui.heading("Last successful image creation");
            ui.label(r.output.display().to_string());ui.label(format!("{} | {} bytes | {} verified files",r.label,r.bytes,r.verified_files));
            ui.label(format!("SHA-256: {}",r.sha256));
        }
        if let Some(r)=&self.verify_receipt{
            ui.separator();ui.heading("Last successful independent verification");
            ui.label(format!("{} files | {} payload bytes | {} image bytes",r.files,r.payload_bytes,r.image_bytes));
            ui.label(format!("SHA-256: {}",r.sha256));
        }
    }
    fn sources_ui(&mut self,ui:&mut egui::Ui,ctx:&egui::Context){
        ui.label(egui::RichText::new("01 / SOURCES").small().weak().strong());
        ui.heading("Select your archive files");
        ui.label("Choose what to archive. Scanning reads files and metadata; originals are never modified.");
        let editable=self.plan.is_none();
        if !editable{ui.label("This plan uses the current inventory. Close the plan explicitly before changing its sources.");}
        ui.add_enabled_ui(editable,|ui|{
            ui.add_space(8.0);
            ui.group(|ui|{
                ui.set_min_height(96.0);
                ui.vertical_centered(|ui|{
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("Drop files or folders here").size(20.0).strong());
                    ui.label("Local drives, mapped drives and network shares");
                    ui.horizontal_wrapped(|ui|{
                        if ui.button("Browse folders").clicked(){if let Some(v)=rfd::FileDialog::new().pick_folders(){for p in v{self.add(p);}}}
                        if ui.button("Browse files").clicked(){if let Some(v)=rfd::FileDialog::new().pick_files(){for p in v{self.add(p);}}}
                    });
                });
            });
            ui.horizontal_wrapped(|ui|{
                let response=ui.add(egui::TextEdit::singleline(&mut self.source_text).hint_text("Enter a local or UNC path").desired_width(340.0));
                let enter=response.lost_focus()&&ui.input(|i|i.key_pressed(egui::Key::Enter));
                if (ui.button("Add path").clicked()||enter)&&!self.source_text.trim().is_empty(){
                    self.add(PathBuf::from(self.source_text.trim()));self.source_text.clear();
                }
            });
            ui.horizontal(|ui|{
                ui.strong(format!("Selected sources ({})",self.sources.len()));
                if ui.add_enabled(!self.sources.is_empty(),egui::Button::new("Clear list")).clicked(){self.sources.clear();self.source_changed();}
            });
            if self.sources.is_empty(){ui.label("No sources selected.");}
            let mut remove=None;
            egui::ScrollArea::vertical().id_salt("source_list").max_height(180.0).show(ui,|ui|{
                for (i,p) in self.sources.iter().enumerate(){
                    ui.horizontal_wrapped(|ui|{if ui.small_button("Remove").clicked(){remove=Some(i);}ui.label(p.display().to_string());});
                }
            });
            if let Some(i)=remove{self.sources.remove(i);self.source_changed();}
            ui.add_space(8.0);
            ui.heading("Scan options");
            if ui.checkbox(&mut self.hash,"Calculate file checksums (SHA-256)").changed(){self.source_changed();}
            ui.label("Recommended for duplicate detection and verified archive catalog records. Reads every file and may take longer.");
            ui.collapsing("Advanced filters",|ui|{
                ui.label("Include only these extensions (comma separated; empty includes all files)");
                if ui.add(egui::TextEdit::singleline(&mut self.extensions).hint_text("mkv, mp4, srt").desired_width(340.0)).changed(){self.source_changed();}
                ui.label("Exclude paths matching glob patterns (one per line)");
                if ui.add(egui::TextEdit::multiline(&mut self.exclusions).desired_rows(3).desired_width(340.0)).changed(){self.source_changed();}
            });
            ui.add_space(8.0);
            if ui.add_enabled(!self.sources.is_empty(),egui::Button::new(if self.inventory.is_some(){"Rescan selected sources"}else{"Scan selected sources"}).min_size(egui::vec2(190.0,36.0))).clicked(){
                let roots=self.sources.clone();
                let options=ScanOptions{extensions:self.extensions.split(',').map(str::trim).filter(|s|!s.is_empty()).map(str::to_owned).collect(),
                    exclude:self.exclusions.lines().map(str::trim).filter(|s|!s.is_empty()).map(str::to_owned).collect(),hash:self.hash};
                self.spawn(ctx,move||Ok(Work::Inventory(scan(&roots,&options)?)));
            }
        });
        ui.add_space(10.0);
        if self.inventory_stale{
            ui.group(|ui|{ui.strong("Sources or scan options changed");ui.label("The previous inventory is retained for reference only. Rescan before planning or saving a project.");});
        }
        let ready=self.inventory_ready();
        let mut duplicates=None;
        if let Some(i)=&self.inventory{
            ui.separator();
            ui.heading(if self.inventory_stale{"Previous scan results"}else{"Scan results"});
            ui.horizontal_wrapped(|ui|{
                ui.strong(format!("{} files",i.files.len()));
                ui.separator();ui.label(format!("{} excluded",i.excluded.len()));
                ui.separator();ui.label(format!("{} errors",i.errors.len()));
                ui.separator();ui.label(gb(i.files.iter().fold(0u64,|sum,f|sum.saturating_add(f.bytes))));
            });
            if !i.errors.is_empty(){ui.strong("Resolve scan errors before creating a plan.");}
            for e in i.errors.iter().take(20){ui.colored_label(ui.visuals().error_fg_color,e);}
            if i.errors.len()>20{ui.label("Save inventory for the complete error report.");}
            ui.horizontal_wrapped(|ui|{
                if ui.add_enabled(ready,egui::Button::new("Continue to disc planner").min_size(egui::vec2(200.0,36.0))).clicked(){self.tab=1;}
                if ui.add_enabled(ready&&i.files.iter().all(|f|f.sha256.is_some()),egui::Button::new("Review duplicate content")).clicked(){duplicates=Some(i.files.clone());}
            });
            if i.files.iter().any(|f|f.sha256.is_none()){ui.label("For duplicate review, scan with file checksums enabled.");}
            ui.add(egui::TextEdit::singleline(&mut self.inventory_search).hint_text("Search archive paths").desired_width(340.0));
            let filter=self.inventory_search.to_lowercase();
            let visible:Vec<_>=i.files.iter().filter(|f|filter.is_empty()||f.archive.to_lowercase().contains(&filter)).collect();
            ui.label(format!("{} matching files",visible.len()));
            egui::ScrollArea::vertical().id_salt("inventory").max_height(350.).show_rows(ui,30.,visible.len(),|ui,range|{
                for j in range{let f=visible[j];ui.label(format!("{}  |  {}",f.archive,gb(f.bytes)));}
            });
            ui.collapsing("Excluded paths",|ui|{
                egui::ScrollArea::vertical().id_salt("exclusions").max_height(180.).show_rows(ui,30.,i.excluded.len(),|ui,range|{for j in range{ui.label(&i.excluded[j]);}});
            });
        }else{ui.label("Scan selected sources to see file counts, exclusions and any errors.");}
        if let Some(files)=duplicates{self.spawn(ctx,move||Ok(Work::Duplicates(disc_archive::duplicates(&files))));}
    }
    fn planner_ui(&mut self,ui:&mut egui::Ui,ctx:&egui::Context){
        ui.label(egui::RichText::new("02 / DISC PLANNER").small().weak().strong());
        ui.heading("Plan your optical archive");
        ui.label("Choose media, compare distribution methods and review every assigned file before creating images.");
        if !self.inventory_ready()&&self.plan.is_none(){
            ui.group(|ui|{
                ui.strong("A scanned inventory is required");
                ui.label("Scan valid sources or import a saved inventory from the Project menu.");
                if ui.button("Go to sources").clicked(){self.tab=0;}
            });
        }
        ui.group(|ui|{
            ui.heading("Media and distribution");
            ui.label("Choose a media preset or enter your own capacities.");
            ui.horizontal_wrapped(|ui|{
                for &(name,bytes) in MEDIA{
                    let selected=self.capacities==bytes.to_string();
                    if ui.add(egui::Button::new(name).selected(selected)).clicked(){
                        self.capacities=bytes.to_string();self.dirty=true;
                    }
                }
            });
            ui.label("Capacity sequence in bytes (last capacity repeats)");
            if ui.add(egui::TextEdit::singleline(&mut self.capacities).desired_width(340.0)).changed(){self.dirty=true;}
            ui.horizontal_wrapped(|ui|{
                ui.label("Reserve per disc (bytes)");
                if ui.add_enabled(self.plan.is_none(),egui::TextEdit::singleline(&mut self.reserve).desired_width(140.0)).changed(){self.dirty=true;}
            });
            if self.plan.is_some(){ui.label("The current plan's reserve is fixed during replanning.");}
            let prior=self.strategy;
            egui::ComboBox::from_label("Distribution method").selected_text(strategy_label(self.strategy)).show_ui(ui,|ui|{
                for mode in [Strategy::Sequential,Strategy::Folder,Strategy::Maximum,Strategy::Hybrid]{
                    ui.selectable_value(&mut self.strategy,mode,strategy_label(mode));
                }
            });
            if prior!=self.strategy{self.dirty=true;}
            ui.label(strategy_description(self.strategy));
            ui.horizontal_wrapped(|ui|{
                if ui.add_enabled(self.inventory_ready()&&self.plan.is_none(),egui::Button::new("Create disc plan").min_size(egui::vec2(160.,36.))).clicked(){self.begin_plan(ctx,false);}
                if ui.add_enabled(self.inventory_ready(),egui::Button::new("Compare strategies")).clicked(){self.begin_plan(ctx,true);}
                if ui.add_enabled(self.plan.is_some(),egui::Button::new("Replan unlocked discs")).clicked(){
                    match caps(&self.capacities){
                        Ok(c)=>{let p=self.plan.as_ref().unwrap().clone();let mode=self.strategy;let pattern=self.pattern.clone();self.spawn(ctx,move||Ok(Work::Plan(editing::replan(&p,&c,mode,Some(&pattern))?)));}
                        Err(e)=>self.status=e.to_string(),
                    }
                }
            });
        });
        let Some(p)=self.plan.as_mut() else{return;};
        ui.add_space(8.0);
        let assigned:usize=p.discs.iter().map(|d|d.files.len()).sum();
        let used:u128=p.discs.iter().map(|d|d.estimated_used as u128).sum();
        let usable:u128=p.discs.iter().map(|d|d.capacity.saturating_sub(p.reserve) as u128).sum();
        ui.group(|ui|{
            ui.heading("Distribution overview");
            ui.horizontal_wrapped(|ui|{
                ui.strong(format!("{} discs",p.discs.len()));ui.separator();
                ui.label(format!("{assigned} assigned files"));ui.separator();
                ui.label(format!("{} unassigned",p.oversized.len()));ui.separator();
                ui.label(format!("{} locked",p.discs.iter().filter(|d|d.locked).count()));
            });
            ui.add(egui::ProgressBar::new(utilization(used,usable)).text(format!("{:.2} / {:.2} GB usable",used as f64/1e9,usable as f64/1e9)));
            ui.label(format!("Current plan: {}. Includes estimated filesystem overhead; reserved space excluded.",strategy_label(p.strategy)));
            if !p.oversized.is_empty(){ui.colored_label(ui.visuals().warn_fg_color,"Resolve unassigned files before archiving the entire collection.");}
            if ui.add_enabled(p.oversized.is_empty()&&!p.discs.is_empty(),egui::Button::new("Continue to image creation")).clicked(){self.tab=7;}
        });
        ui.collapsing("Disc names and capacity adjustments",|ui|{
            ui.horizontal_wrapped(|ui|{
                ui.label("Naming pattern");
                if ui.text_edit_singleline(&mut self.pattern).changed(){self.dirty=true;}
                if ui.button("Apply to unlocked discs").clicked(){self.status=editing::rename(p,&self.pattern).map(|_|{self.dirty=true;"Names updated".to_owned()}).unwrap_or_else(|e|e.to_string());}
            });
            if ui.button("Add empty disc (first capacity)").clicked(){self.status=caps(&self.capacities).and_then(|c|editing::add_disc(p,c[0])).map(|_|{self.dirty=true;"Disc added".into()}).unwrap_or_else(|e|e.to_string());}
        });
        ui.add_space(8.0);
        ui.heading("Discs");
        let previous=self.selected;
        if ui.selectable_label(self.selected.is_none(),format!("Unassigned files ({})",p.oversized.len())).clicked(){self.selected=None;}
        egui::ScrollArea::vertical().id_salt("discs").max_height(260.).show_rows(ui,64.,p.discs.len(),|ui,range|{
            for j in range{
                let d=&p.discs[j];let available=d.capacity.saturating_sub(p.reserve);
                ui.vertical(|ui|{
                    ui.horizontal_wrapped(|ui|{
                        if ui.selectable_label(self.selected==Some(d.number),egui::RichText::new(&d.label).strong()).clicked(){self.selected=Some(d.number);}
                        ui.label(format!("{} files",d.files.len()));
                        if d.locked{ui.strong("Locked");}
                    });
                    ui.add(egui::ProgressBar::new(utilization(d.estimated_used as u128,available as u128)).text(format!("{} used / {} usable | {} free",gb(d.estimated_used),gb(available),gb(available.saturating_sub(d.estimated_used)))));
                });
            }
        });
        if previous!=self.selected{self.planner_folder=None;self.move_path.clear();}
        let locked=self.selected.and_then(|n|p.discs.iter().find(|d|d.number==n)).is_some_and(|d|d.locked);
        if let Some(n)=self.selected{
            if p.discs.iter().any(|d|d.number==n){
                if ui.button(if locked{"Unlock selected disc"}else{"Lock selected disc"}).clicked(){self.status=editing::set_lock(p,n,!locked).map(|_|{self.dirty=true;"Lock updated".into()}).unwrap_or_else(|e|e.to_string());}
            }
        }
        ui.separator();
        ui.heading(match self.selected.and_then(|n|p.discs.iter().find(|d|d.number==n)){Some(d)=>format!("Files on {}",d.label),None=>"Unassigned files".into()});
        ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search archive paths").desired_width(340.0));
        let filter=self.search.to_lowercase();
        let entries=if let Some(n)=self.selected{p.discs.iter().find(|d|d.number==n).map(|d|d.files.as_slice()).unwrap_or(&[])}else{p.oversized.as_slice()};
        let folders=folder_counts(entries);
        if self.planner_folder.as_ref().is_some_and(|name|!folders.contains_key(name.as_str())){self.planner_folder=None;}
        ui.collapsing("Browse folders",|ui|{
            if ui.selectable_label(self.planner_folder.is_none(),"All folders").clicked(){self.planner_folder=None;}
            let groups:Vec<_>=folders.iter().collect();
            egui::ScrollArea::vertical().id_salt("planner_folders").max_height(180.).show_rows(ui,30.,groups.len(),|ui,range|{
                for j in range{
                    let (name,count)=groups[j];
                    if ui.selectable_label(self.planner_folder.as_deref()==Some(*name),format!("{} ({count})",if name.is_empty(){"Root files"}else{*name})).clicked(){self.planner_folder=Some((*name).to_owned());}
                }
            });
        });
        let visible:Vec<_>=entries.iter().filter(|f|{
            self.planner_folder.as_deref().is_none_or(|folder|folder==archive_folder(&f.archive))&&(filter.is_empty()||f.archive.to_lowercase().contains(&filter))
        }).collect();
        ui.label(format!("{} matching files",visible.len()));
        if visible.is_empty(){ui.label("No files match the current view.");}
        egui::ScrollArea::vertical().id_salt("files").max_height(260.).show_rows(ui,30.,visible.len(),|ui,range|{
            for j in range{let f=visible[j];if ui.selectable_label(self.move_path==f.archive,format!("{} | {}",f.archive,gb(f.bytes))).clicked(){self.move_path=f.archive.clone();}}
        });
        ui.group(|ui|{
            ui.heading("Move a file to another disc");
            ui.label("Select a file above or enter its exact archive path. Locked discs cannot be changed.");
            ui.add(egui::TextEdit::singleline(&mut self.move_path).hint_text("Archive path").desired_width(340.0));
            let eligible:Vec<_>=p.discs.iter().filter(|d|!d.locked&&Some(d.number)!=self.selected).collect();
            if !eligible.iter().any(|d|d.number==self.destination){self.destination=eligible.first().map(|d|d.number).unwrap_or(0);}
            let target_label=eligible.iter().find(|d|d.number==self.destination).map(|d|d.label.as_str()).unwrap_or("No unlocked destination");
            egui::ComboBox::from_label("Destination disc").selected_text(target_label).show_ui(ui,|ui|{
                for d in &eligible{ui.selectable_value(&mut self.destination,d.number,format!("{} - {} free",d.label,gb(d.capacity.saturating_sub(p.reserve).saturating_sub(d.estimated_used))));}
            });
            let ready=!self.move_path.is_empty()&&self.destination!=0;
            if ui.add_enabled(ready,egui::Button::new("Move selected file")).clicked(){
                match editing::move_file(p,&self.move_path,self.destination){
                    Ok(())=>{self.dirty=true;self.status="File moved".into();self.move_path.clear();}
                    Err(e)=>self.status=e.to_string(),
                }
            }
        });
    }
    fn comparison_ui(&mut self,ui:&mut egui::Ui){
        ui.heading("Compare distribution strategies");
        ui.label("Compare estimates using the same inventory, capacity sequence and reserve. The current plan is not changed.");
        if self.comparison.is_empty(){
            ui.label("No comparison yet. Generate one in the disc planner.");
        }else{
            for s in &self.comparison{
                ui.group(|ui|{
                    ui.heading(strategy_label(s.strategy));
                    ui.label(strategy_description(s.strategy));
                    ui.horizontal_wrapped(|ui|{
                        ui.strong(format!("{} discs",s.discs));ui.label(format!("{} assigned",s.files));ui.label(format!("{} unassigned",s.unassigned));ui.label(format!("{} split folders",s.split_folders));
                    });
                    ui.add(egui::ProgressBar::new(utilization(s.estimated_used_bytes,s.capacity_bytes)).text(format!("{:.2} / {:.2} GB nominal capacity",s.estimated_used_bytes as f64/1e9,s.capacity_bytes as f64/1e9)));
                });
            }
        }
        if ui.button("Return to disc planner").clicked(){self.tab=1;}
    }
    fn duplicates_ui(&mut self,ui:&mut egui::Ui){
        ui.heading("Review duplicate content");
        ui.label("Groups have matching SHA-256 and file size. No files are removed or omitted automatically.");
        if self.duplicate_groups.is_empty(){ui.label("No duplicate groups displayed. Scan with checksums and run duplicate review from Sources.");}
        else{ui.strong(format!("{} duplicate groups",self.duplicate_groups.len()));}
        for (i,group) in self.duplicate_groups.iter().enumerate(){
            ui.collapsing(format!("Group {} - {} files",i+1,group.len()),|ui|{for path in group{ui.label(path);}});
        }
        if ui.button("Return to sources").clicked(){self.tab=0;}
    }

}
impl eframe::App for Studio{
    fn update(&mut self,ctx:&egui::Context,_frame:&mut eframe::Frame){
        self.poll();
        if ctx.input(|i|i.viewport().close_requested()) && (self.dirty||self.worker.is_some()) {ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);self.close_prompt=true;}
        if self.close_prompt {egui::Window::new("Close application?").collapsible(false).show(ctx,|ui|{ui.label("Wait for running work to finish before exiting; unsaved project changes may be lost.");if ui.button("Keep working").clicked(){self.close_prompt=false;}if ui.add_enabled(self.worker.is_none(),egui::Button::new("Discard and exit")).clicked(){self.close_prompt=false;self.dirty=false;self.worker=None;ctx.send_viewport_cmd(egui::ViewportCommand::Close);}});}
        if self.worker.is_none(){
            let dropped=ctx.input(|i|i.raw.dropped_files.clone());
            for f in dropped{if let Some(p)=f.path{self.add(p);if self.plan.is_none(){self.tab=0;}}}
        }
        egui::TopBottomPanel::top("top").show(ctx,|ui|{
            ui.add_space(8.0);
            ui.horizontal(|ui|{
                ui.vertical(|ui|{
                    ui.label(egui::RichText::new("DISC ARCHIVE STUDIO").size(22.0).strong());
                    ui.label(egui::RichText::new("Archive workspace  /  Windows preview").small().weak());
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{
                    let dark=ctx.style().visuals.dark_mode;
                    if ui.button(if dark{"Light theme"}else{"Dark theme"}).clicked(){apply_theme(ctx,!dark);}
                    if self.dirty{ui.label(egui::RichText::new("Unsaved changes").strong());}
                });
            });
            ui.add_space(5.0);
            ui.add_enabled_ui(self.worker.is_none(),|ui|{
                ui.horizontal_wrapped(|ui|{
                    ui.menu_button("Project",|ui|{
                    if ui.add_enabled(!self.dirty&&self.plan.is_none()&&self.inventory.is_none(),egui::Button::new("Open project")).clicked(){if let Some(path)=rfd::FileDialog::new().add_filter("Archive project",&["json"]).pick_file(){self.spawn(ctx,move||Ok(Work::Project(disc_archive::project::load(&path)?)));}}
                    if ui.button("Save project as").clicked(){match self.snapshot(){Ok(p)=>{if let Some(path)=rfd::FileDialog::new().set_file_name("archive.das.json").save_file(){self.spawn(ctx,move||{disc_archive::project::save(&path,&p)?;Ok(Work::Saved("Project saved".into()))});}},Err(e)=>self.status=e.to_string()}}
                    if ui.button("Close project").clicked(){self.tab=5;}
                    if ui.add_enabled(self.plan.is_none()&&self.inventory.is_none()&&self.sources.is_empty(),egui::Button::new("Load inventory")).clicked(){if let Some(path)=rfd::FileDialog::new().add_filter("JSON",&["json"]).pick_file(){
                        self.spawn(ctx,move||Ok(Work::Inventory(serde_json::from_reader(BufReader::new(File::open(path)?))?)));
                    }}
                    if ui.add_enabled(self.inventory.is_some(),egui::Button::new("Save inventory")).clicked(){if let Some(path)=rfd::FileDialog::new().set_file_name("inventory.json").save_file(){self.status=save_new(path,self.inventory.as_ref().unwrap()).map(|_|"Inventory saved".into()).unwrap_or_else(|e|e.to_string());}}
                    if ui.add_enabled(self.plan.is_none()&&!self.inventory_stale,egui::Button::new("Load plan")).clicked(){if let Some(path)=rfd::FileDialog::new().add_filter("JSON",&["json"]).pick_file(){let inventory=self.inventory.clone();self.spawn(ctx,move||{let p:Plan=serde_json::from_reader(BufReader::new(File::open(path)?))?;editing::validate(&p)?;let project=disc_archive::project::Project{inventory,plan:Some(p.clone()),reserve:p.reserve,strategy:p.strategy,..Default::default()};project.validate()?;Ok(Work::Plan(p))});}}
                    if ui.add_enabled(self.plan.is_some(),egui::Button::new("Save plan as")).clicked(){if let Some(path)=rfd::FileDialog::new().set_file_name("plan.json").save_file(){let p=self.plan.as_ref().unwrap().clone();self.spawn(ctx,move||{save_new(path,&p)?;Ok(Work::Exported("Plan exported; project changes remain unsaved".into()))});}}
                    if ui.add_enabled(self.plan.is_some(),egui::Button::new("Close plan")).clicked(){self.status="Save the plan before closing; use the separate confirmation below.".into();self.tab=3;}
                    });
                    ui.separator();
                    ui.label("Project name");
                    if ui.text_edit_singleline(&mut self.project_name).changed(){self.dirty=true;}
                });
            });
            ui.add_space(6.0);
        });
        egui::TopBottomPanel::bottom("status").show(ctx,|ui|{
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui|{
                if self.worker.is_some(){ui.spinner();ui.strong("Working");}else{ui.strong("Status");}
                ui.label(&self.status);
            });
            ui.add_space(4.0);
        });
        egui::SidePanel::left("workspace_navigation").exact_width(208.0).resizable(false).show(ctx,|ui|{
            ui.add_space(12.0);
            egui::ScrollArea::vertical().id_salt("workspace_navigation_scroll").show(ui,|ui|{
                ui.add_enabled_ui(self.worker.is_none(),|ui|{
                    for (section,pages) in NAVIGATION{
                        ui.add_space(9.0);
                        ui.label(egui::RichText::new(*section).small().strong().weak());
                        for &(id,label) in *pages{
                            if ui.add_sized([ui.available_width(),32.0],egui::Button::new(label).selected(self.tab==id)).clicked(){self.tab=id;}
                        }
                    }
                });
            });
        });
        egui::CentralPanel::default().show(ctx,|ui|{
            if self.burn_progress.is_some(){ui.heading("Active burning jobs");for (id,stage) in &self.burn_stages{
                ui.label(id);
                match stage{
                    disc_archive::optical::BurnStage::WritingProgress{written_sectors,total_sectors}=>{
                        let fraction=(*written_sectors as f64 / *total_sectors as f64) as f32;
                        ui.add(egui::ProgressBar::new(fraction).text(format!("Data written: {:.1}% (finalization and verification separate)",fraction*100.0)));
                    }
                    _=>{ui.label(format!("{stage:?}"));}
                }
            }}
            ui.add_enabled_ui(self.worker.is_none(),|ui|{egui::ScrollArea::vertical().show(ui,|ui|match self.tab{
                11=>self.overview_ui(ui,ctx),
                0=>self.sources_ui(ui,ctx),
                1=>self.planner_ui(ui,ctx),
                2=>self.comparison_ui(ui),
                4=>self.duplicates_ui(ui),
                6=>self.catalog_ui(ui,ctx),
                7=>self.image_ui(ui,ctx),
                8=>self.drives_ui(ui,ctx),
                9=>self.jobs_ui(ui,ctx),
                10=>self.burn_ui(ui,ctx),
                5=>{ui.label("Closing the project discards all unsaved project settings, inventory and plan. Save a new project revision first if needed.");if ui.button("Confirm close project").clicked(){*self=Self::default();}if ui.button("Cancel").clicked(){self.tab=0;}},
                _=>{ui.label("Closing disc plan discards unsaved edits.");if ui.button("Confirm close plan").clicked(){self.plan=None;self.selected=None;self.dirty=true;self.tab=0;self.status="Plan closed".into();}if ui.button("Cancel").clicked(){self.tab=1;}}
            });});
        });
    }
}
fn strategy_label(mode:Strategy)->&'static str{
    match mode{Strategy::Sequential=>"Sequential order",Strategy::Folder=>"Keep folders together",Strategy::Maximum=>"Maximum utilization",Strategy::Hybrid=>"Balanced (hybrid)"}
}
fn strategy_description(mode:Strategy)->&'static str{
    match mode{Strategy::Sequential=>"Natural file order.",Strategy::Folder=>"Prefer keeping folder groups together.",Strategy::Maximum=>"Prioritize filling available disc capacity.",Strategy::Hybrid=>"Balance related files and capacity utilization."}
}
fn utilization(used:u128,capacity:u128)->f32{
    if capacity==0{0.0}else{(used as f64/capacity as f64).clamp(0.0,1.0) as f32}
}
fn archive_folder(path:&str)->&str{path.rsplit_once('/').map(|(folder,_)|folder).unwrap_or("")}
fn folder_counts(entries:&[disc_archive::Entry])->std::collections::BTreeMap<&str,usize>{
    let mut result=std::collections::BTreeMap::new();
    for f in entries{*result.entry(archive_folder(&f.archive)).or_insert(0)+=1;}
    result
}
#[cfg(test)]
#[test]
fn planner_folder_and_capacity_display(){
    let entries=["root.bin","Series/A/1.mkv","Series/A/2.mkv","Series/B/1.mkv"].map(|path|disc_archive::Entry{source:path.into(),archive:path.into(),bytes:1,sha256:None});
    let folders=folder_counts(&entries);
    assert_eq!(folders.get(""),Some(&1));
    assert_eq!(folders.get("Series/A"),Some(&2));
    assert_eq!(folders.get("Series/B"),Some(&1));
    assert_eq!(folders.values().sum::<usize>(),entries.len());
    assert_eq!(utilization(0,0),0.0);
    assert_eq!(utilization(50,100),0.5);
    assert_eq!(utilization(101,100),1.0);
}
const NAVIGATION:&[(&str,&[(usize,&str)])]=&[
    ("WORKSPACE",&[(11,"Overview"),(0,"Sources"),(1,"Disc planner"),(2,"Compare strategies"),(4,"Duplicate review")]),
    ("OPTICAL MEDIA",&[(7,"Create images"),(8,"Optical drives"),(10,"Burn discs"),(9,"Burn journal & verify")]),
    ("LIBRARY",&[(6,"Archive catalog")]),
];
fn apply_theme(ctx:&egui::Context,dark:bool){
    let mut style=(*ctx.style()).clone();
    let mut visuals=if dark{egui::Visuals::dark()}else{egui::Visuals::light()};
    visuals.panel_fill=if dark{egui::Color32::from_rgb(24,31,43)}else{egui::Color32::from_rgb(246,248,252)};
    visuals.window_fill=visuals.panel_fill;
    visuals.selection.bg_fill=if dark{egui::Color32::from_rgb(44,80,125)}else{egui::Color32::from_rgb(210,229,255)};
    style.visuals=visuals;
    style.spacing.item_spacing=egui::vec2(10.0,8.0);
    style.spacing.button_padding=egui::vec2(12.0,7.0);
    style.spacing.interact_size=egui::vec2(40.0,28.0);
    for (kind,size) in [(egui::TextStyle::Body,15.0),(egui::TextStyle::Button,15.0),(egui::TextStyle::Heading,21.0),(egui::TextStyle::Small,12.0)]{
        style.text_styles.insert(kind,egui::FontId::proportional(size));
    }
    ctx.set_style(style);
}
#[cfg(test)]
#[test]
fn navigation_preserves_every_workspace(){
    let mut ids:Vec<_>=NAVIGATION.iter().flat_map(|(_,pages)|pages.iter().map(|(id,_)|*id)).collect();
    ids.sort_unstable();
    assert_eq!(ids,vec![0,1,2,4,6,7,8,9,10,11]);
    assert_eq!(Studio::default().tab,11);
}
fn main()->eframe::Result<()>{
    eframe::run_native("Disc Archive Studio",eframe::NativeOptions{viewport:egui::ViewportBuilder::default().with_inner_size([1200.,850.]).with_min_inner_size([800.,600.]),..Default::default()},Box::new(|cc|{apply_theme(&cc.egui_ctx,false);Ok(Box::<Studio>::default())}))
}

 // A count match alone is not authorization: bind every displayed preview to its target and checksum.
fn batch_preview_matches(requests:&[disc_archive::batch::Request],previews:&[disc_archive::optical::BurnPreview])->bool{
    !requests.is_empty() && requests.len()==previews.len() &&
    requests.iter().zip(previews).all(|(r,p)|r.recorder==p.recorder_id && r.sha256.eq_ignore_ascii_case(&p.image_sha256))
}
#[cfg(test)]
mod authorization_tests {
    use super::*;
    #[test]
    fn preview_must_match_target_and_checksum(){
        let mut r=disc_archive::batch::Request{recorder:"A".into(),image:PathBuf::new(),manifest:PathBuf::new(),sha256:"a".repeat(64)};
        let p=disc_archive::optical::BurnPreview{recorder_id:"A".into(),image_bytes:2048,image_sha256:"A".repeat(64),media_type:"BD-R".into(),free_bytes:4096,verified_files:1};
        assert!(batch_preview_matches(std::slice::from_ref(&r),std::slice::from_ref(&p)));
        r.recorder="B".into();
        assert!(!batch_preview_matches(std::slice::from_ref(&r),std::slice::from_ref(&p)));
        r.recorder="A".into();r.sha256="b".repeat(64);
        assert!(!batch_preview_matches(std::slice::from_ref(&r),std::slice::from_ref(&p)));
        assert!(!batch_preview_matches(&[],&[]));
        assert!(!batch_preview_matches(std::slice::from_ref(&r),&[]));
    }
}

#[cfg(test)]
mod source_workflow_tests{
    use super::*;
    #[test]
    fn source_change_invalidates_inventory_without_erasing_it(){
        let mut s=Studio::default();
        s.inventory=Some(Inventory{files:vec![disc_archive::Entry{source:"fixture.mkv".into(),archive:"fixture.mkv".into(),bytes:10,sha256:None}],..Default::default()});
        assert!(s.inventory_ready());
        s.add("new-source".into());
        assert!(s.dirty&&s.inventory_stale&&!s.inventory_ready());
        assert_eq!(s.inventory.as_ref().unwrap().files.len(),1);
        assert!(s.snapshot().is_err());
        s.inventory_stale=false;
        assert!(s.inventory_ready());
    }
    #[test]
    fn existing_plan_blocks_source_addition(){
        let mut s=Studio::default();
        s.plan=Some(plan_mixed(&[],&[100000000],0,Strategy::Hybrid).unwrap());
        s.add("unexpected-source".into());
        assert!(s.sources.is_empty());
        assert!(!s.dirty);
    }
}
