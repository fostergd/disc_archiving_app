#![cfg_attr(target_os="windows", windows_subsystem="windows")]
use disc_archive::{scan,plan_mixed,ScanOptions,Inventory,Plan,Strategy,MEDIA};
use disc_archive::editing;
use eframe::egui;
use std::{path::PathBuf,sync::mpsc::{self,Receiver},fs::File,io::{BufReader,BufWriter,Write}};
enum Work { Inventory(Inventory), Plan(Plan), Comparison(Vec<editing::Summary>), Duplicates(Vec<Vec<String>>), Saved(String), Project(disc_archive::project::Project), CatalogOpened(PathBuf,Vec<disc_archive::catalog::ProjectRow>), CatalogResults(Vec<disc_archive::catalog::Match>,usize), CatalogImported(i64,Vec<disc_archive::catalog::ProjectRow>), CatalogLinked, ImageCreated(disc_archive::image::ImageReceipt), ImageVerified(disc_archive::image::VerificationReceipt), ManifestSaved(PathBuf), Drives(disc_archive::optical::Discovery), Jobs(PathBuf,Vec<disc_archive::jobs::Job>,String), BatchPreview(Vec<disc_archive::optical::BurnPreview>), BatchDone(Vec<disc_archive::batch::Outcome>,Vec<disc_archive::jobs::Job>), Error(String) }
struct Studio {
    burn_requests:Vec<disc_archive::batch::Request>, burn_previews:Vec<disc_archive::optical::BurnPreview>, burn_confirm:bool, burn_progress:Option<Receiver<(String,disc_archive::optical::BurnStage)>>, burn_stages:std::collections::BTreeMap<String,disc_archive::optical::BurnStage>, burn_outcomes:Vec<disc_archive::batch::Outcome>,
    drives:Option<disc_archive::optical::Discovery>,
    journal_path:Option<PathBuf>, jobs:Vec<disc_archive::jobs::Job>, job_selection:Option<i64>, job_recorder:String, reconcile_note:String, reconcile_confirm:bool, verify_confirm:bool,
    sources:Vec<PathBuf>, source_text:String, extensions:String, exclusions:String, hash:bool,
    capacities:String, reserve:String, strategy:Strategy, inventory:Option<Inventory>, plan:Option<Plan>,
    comparison:Vec<editing::Summary>, worker:Option<Receiver<Work>>, status:String, selected:Option<usize>,
    pattern:String, move_path:String, destination:usize, search:String, tab:usize,
    inventory_search:String, duplicate_groups:Vec<Vec<String>>, dirty:bool, close_prompt:bool,
    project_name:String,
    image_disc:usize, image_receipt:Option<disc_archive::image::ImageReceipt>, verify_receipt:Option<disc_archive::image::VerificationReceipt>, image_path:Option<PathBuf>, manifest_path:Option<PathBuf>,
    catalog_path:Option<PathBuf>, catalog_projects:Vec<disc_archive::catalog::ProjectRow>, catalog_rows:Vec<disc_archive::catalog::Match>, catalog_term:String, catalog_project:Option<i64>, catalog_offset:usize, catalog_has_next:bool, catalog_query:Option<(String,Option<i64>)>,
}
impl Default for Studio {
    fn default()->Self{Self{burn_requests:vec![],burn_previews:vec![],burn_confirm:false,burn_progress:None,burn_stages:Default::default(),burn_outcomes:vec![],drives:None,journal_path:None,jobs:vec![],job_selection:None,job_recorder:String::new(),reconcile_note:String::new(),reconcile_confirm:false,verify_confirm:false,sources:vec![],source_text:String::new(),extensions:String::new(),exclusions:String::new(),hash:false,
        capacities:"50000000000".into(),reserve:"50000000".into(),strategy:Strategy::Hybrid,inventory:None,plan:None,comparison:vec![],
        worker:None,status:"Add files or folders to begin.".into(),selected:None,pattern:"DISC_{n:03}".into(),move_path:String::new(),destination:1,search:String::new(),tab:0,inventory_search:String::new(),duplicate_groups:vec![],dirty:false,close_prompt:false,project_name:"Untitled archive".into(),image_disc:1,image_receipt:None,verify_receipt:None,image_path:None,manifest_path:None,catalog_path:None,catalog_projects:vec![],catalog_rows:vec![],catalog_term:String::new(),catalog_project:None,catalog_offset:0,catalog_has_next:false,catalog_query:None}}
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
        if let Some(rx)=&self.burn_progress{for (id,stage) in rx.try_iter(){self.burn_stages.insert(id,stage);}}
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
    fn catalog_search(&mut self,ctx:&egui::Context,offset:usize){
        let Some(path)=self.catalog_path.clone() else{return;};
        let term=self.catalog_term.clone();let project=self.catalog_project;
        self.catalog_query=Some((term.clone(),project));
        self.catalog_rows.clear();self.catalog_has_next=false;
        self.spawn(ctx,move||{let db=disc_archive::catalog::Catalog::open(&path,false)?;Ok(Work::CatalogResults(db.search(&term,project,101,offset)?,offset))});
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
        let ready=!self.burn_requests.is_empty()&&self.burn_previews.len()==self.burn_requests.len()&&self.journal_path.is_some();
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
        if self.close_prompt {egui::Window::new("Close application?").collapsible(false).show(ctx,|ui|{ui.label("Wait for running work to finish before exiting; unsaved plan changes may be lost.");if ui.button("Keep working").clicked(){self.close_prompt=false;}if ui.add_enabled(self.worker.is_none(),egui::Button::new("Discard and exit")).clicked(){self.close_prompt=false;self.dirty=false;self.worker=None;ctx.send_viewport_cmd(egui::ViewportCommand::Close);}});}
        if self.worker.is_none(){
            let dropped=ctx.input(|i|i.raw.dropped_files.clone());
            for f in dropped{if let Some(p)=f.path{self.add(p);}}
        }
        egui::TopBottomPanel::top("top").show(ctx,|ui|{
            ui.horizontal_wrapped(|ui|{
                ui.heading("Disc Archive Studio");ui.label("Archive planning, verified images and experimental physical writing");if self.dirty{ui.label("Unsaved plan");}
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
            ui.horizontal(|ui|{ui.selectable_value(&mut self.tab,0,"Sources");ui.selectable_value(&mut self.tab,1,"Planner");ui.selectable_value(&mut self.tab,2,"Comparison");ui.selectable_value(&mut self.tab,4,"Duplicates");ui.selectable_value(&mut self.tab,6,"Catalog");ui.selectable_value(&mut self.tab,7,"Images");ui.selectable_value(&mut self.tab,8,"Drives");ui.selectable_value(&mut self.tab,9,"Burn journal");ui.selectable_value(&mut self.tab,10,"Burn");});
        });
        egui::TopBottomPanel::bottom("status").show(ctx,|ui|{ui.horizontal(|ui|{if self.worker.is_some(){ui.spinner();}ui.label(&self.status);});});
        egui::CentralPanel::default().show(ctx,|ui|{
            if self.burn_progress.is_some(){ui.heading("Active burning jobs");for (id,stage) in &self.burn_stages{ui.label(format!("{id}: {stage:?}"));}}
            ui.add_enabled_ui(self.worker.is_none(),|ui|{egui::ScrollArea::vertical().show(ui,|ui|match self.tab{
                0=>self.sources_ui(ui,ctx),
                1=>self.planner_ui(ui,ctx),
                2=>{ui.heading("Distribution comparison");for s in &self.comparison{ui.label(format!("{:?}: {} discs | {} assigned | {} unassigned | {} split folders | {:.2} GB used / {:.2} GB capacity",s.strategy,s.discs,s.files,s.unassigned,s.split_folders,s.estimated_used_bytes as f64/1e9,s.capacity_bytes as f64/1e9));}},
                4=>{ui.heading("Duplicate content");ui.label("Identical SHA-256 and file size. No files are removed.");for (i,group) in self.duplicate_groups.iter().enumerate(){ui.collapsing(format!("Group {} - {} files",i+1,group.len()),|ui|{for path in group{ui.label(path);}});}},
                6=>self.catalog_ui(ui,ctx),
                7=>self.image_ui(ui,ctx),
                8=>self.drives_ui(ui,ctx),
                9=>self.jobs_ui(ui,ctx),
                10=>self.burn_ui(ui,ctx),
                5=>{ui.label("Closing the project discards all unsaved project settings, inventory and plan. Save a new project revision first if needed.");if ui.button("Confirm close project").clicked(){*self=Self::default();}if ui.button("Cancel").clicked(){self.tab=0;}},
                _=>{ui.label("Closing disc plan discards unsaved edits.");if ui.button("Confirm close plan").clicked(){self.plan=None;self.selected=None;self.dirty=false;self.tab=0;self.status="Plan closed".into();}if ui.button("Cancel").clicked(){self.tab=1;}}
            });});
        });
    }
}
fn main()->eframe::Result<()>{
    eframe::run_native("Disc Archive Studio",eframe::NativeOptions{viewport:egui::ViewportBuilder::default().with_inner_size([1100.,800.]),..Default::default()},Box::new(|_|Ok(Box::<Studio>::default())))
}
