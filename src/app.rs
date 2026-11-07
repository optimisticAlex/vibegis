use crate::{
    browser::{self, Dataset},
    model::{self, Status, Store},
};
use eframe::egui::{self, Color32, RichText, Stroke, Vec2};
use std::{cell::RefCell, collections::HashSet, rc::Rc};
use walkers::{HttpTiles, Map, MapMemory, lon_lat};

const INK: Color32 = Color32::from_rgb(25, 48, 39);
const MUTED: Color32 = Color32::from_rgb(111, 124, 115);
const GREEN: Color32 = Color32::from_rgb(45, 117, 84);
const PAPER: Color32 = Color32::from_rgb(246, 247, 241);
fn color(status: Status) -> Color32 {
    match status {
        Status::Red => Color32::from_rgb(218, 83, 75),
        Status::Yellow => Color32::from_rgb(224, 173, 45),
        Status::Green => Color32::from_rgb(48, 156, 105),
    }
}
fn number(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(c);
    }
    out
}
fn button(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>, primary: bool) -> egui::Response {
    let mut b = egui::Button::new(text).min_size(Vec2::new(ui.available_width(), 36.));
    if primary {
        b = b.fill(GREEN);
    }
    ui.add(b)
}
fn label(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(11.).color(MUTED).strong());
}

#[derive(Default)]
struct RenderCache {
    key: Option<([f64; 4], u64)>,
    visible: Vec<u32>,
    selected: HashSet<u32>,
}
struct EditJob {
    target: Option<Status>,
    cursor: usize,
    changes: Vec<(u32, Status)>,
}
pub struct App {
    store: browser::SharedStore,
    tiles: Option<HttpTiles>,
    memory: MapMemory,
    picker: browser::FilePicker,
    manifest: browser::ManifestResult,
    datasets: Vec<Dataset>,
    load: Option<browser::LoadJob>,
    export: Option<Rc<RefCell<browser::ExportProgress>>>,
    selection_job: Option<Rc<RefCell<browser::SelectionProgress>>>,
    editing: Option<EditJob>,
    pending_home: Option<([f64; 2], f64)>,
    dataset_name: String,
    active_key: String,
    home: ([f64; 2], f64),
    import_open: bool,
    user_chose_data: bool,
    drawing: bool,
    vertices: Vec<[f64; 2]>,
    selected: Vec<u32>,
    undo: Vec<(u32, Status)>,
    dirty: bool,
    cache: RenderCache,
    notice: String,
    error: bool,
    pointer: Option<[f64; 2]>,
    visible_count: usize,
}
impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = cc.egui_ctx.clone();
        ctx.set_theme(egui::Theme::Light);
        let mut style = (*ctx.style_of(egui::Theme::Light)).clone();
        style.visuals = egui::Visuals::light();
        style.visuals.override_text_color = Some(INK);
        style.visuals.panel_fill = PAPER;
        style.visuals.window_fill = PAPER;
        style.visuals.selection.bg_fill = GREEN;
        style.visuals.selection.stroke = Stroke::new(1., Color32::WHITE);
        style.spacing.item_spacing = Vec2::new(10., 10.);
        style.spacing.button_padding = Vec2::new(12., 8.);
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(14.));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(14.));
        ctx.set_style_of(egui::Theme::Light, style);
        let mut memory = MapMemory::default();
        let _ = memory.set_zoom(14.);
        Self {
            tiles: if cfg!(test) {
                None
            } else {
                Some(HttpTiles::new(walkers::sources::OpenStreetMap, ctx.clone()))
            },
            memory,
            picker: browser::FilePicker::new(ctx.clone()),
            manifest: if cfg!(test) {
                Rc::new(RefCell::new(None))
            } else {
                browser::manifest(ctx)
            },
            datasets: vec![],
            store: Rc::new(RefCell::new(Store::default())),
            load: None,
            export: None,
            selection_job: None,
            editing: None,
            pending_home: None,
            dataset_name: "Noch keine Daten".into(),
            active_key: String::new(),
            home: ([12.0969, 49.0134], 14.),
            import_open: false,
            user_chose_data: false,
            drawing: false,
            vertices: vec![],
            selected: vec![],
            undo: vec![],
            dirty: false,
            cache: RenderCache::default(),
            notice: "Wähle einen Demodatensatz oder lade deine eigene CSV.".into(),
            error: false,
            pointer: None,
            visible_count: 0,
        }
    }
    fn busy(&self) -> bool {
        self.load.is_some()
            || self.export.is_some()
            || self.selection_job.is_some()
            || self.editing.is_some()
    }
    fn clear_selection(&mut self) {
        self.vertices.clear();
        self.selected.clear();
        self.cache.selected.clear();
        self.drawing = false;
    }
    fn start_dataset(&mut self, ctx: &egui::Context, d: Dataset) {
        self.user_chose_data = true;
        self.pending_home = Some((d.center, d.zoom));
        self.active_key = d.key.clone();
        self.load = Some(browser::load(
            ctx.clone(),
            d.name,
            None,
            format!("data/{}.csv", d.key),
            Some(d.rows),
        ));
        self.import_open = false;
    }
    fn finish_selection(&mut self, ctx: &egui::Context) {
        if let Err(e) = model::selection_polygon(&self.vertices) {
            self.notice = e;
            self.error = true;
            return;
        }
        self.selection_job = Some(browser::select(
            ctx.clone(),
            self.store.clone(),
            self.vertices.clone(),
        ));
        self.drawing = false;
    }
    fn poll(&mut self, ctx: &egui::Context) {
        if let Some(mut job) = self.editing.take() {
            let end = if job.target.is_some() {
                self.selected.len()
            } else {
                self.undo.len()
            };
            let started = browser::now();
            while job.cursor < end && browser::now() - started < 10. {
                let next = (job.cursor + 2048).min(end);
                if let Some(status) = job.target {
                    job.changes.extend(
                        self.store
                            .borrow_mut()
                            .change(&self.selected[job.cursor..next], status),
                    );
                } else {
                    self.store.borrow_mut().undo(&self.undo[job.cursor..next]);
                }
                job.cursor = next;
            }
            if job.cursor == end {
                self.notice = if let Some(status) = job.target {
                    let count = job.changes.len();
                    if count > 0 {
                        self.undo = job.changes;
                    }
                    self.dirty |= count > 0;
                    format!("{} Punkte auf {} gesetzt.", number(count), status.name())
                } else {
                    let count = self.undo.len();
                    self.undo.clear();
                    format!("{} Änderungen zurückgenommen.", number(count))
                };
                self.error = false;
            } else {
                self.editing = Some(job);
            }
            ctx.request_repaint();
        }
        let manifest = self.manifest.borrow_mut().take();
        if let Some(result) = manifest {
            match result {
                Ok(data) => {
                    self.datasets = data;
                    if !self.user_chose_data
                        && !self.busy()
                        && self.store.borrow().houses.is_empty()
                        && let Some(d) = self.datasets.first().cloned()
                    {
                        self.start_dataset(ctx, d);
                    }
                }
                Err(e) => {
                    if !self.user_chose_data {
                        self.notice = e;
                        self.import_open = true;
                    }
                }
            }
        }
        let file = self.picker.chosen.borrow_mut().take();
        if let Some(file) = file
            && !self.busy()
        {
            self.user_chose_data = true;
            self.load = Some(browser::load(
                ctx.clone(),
                file.name(),
                Some(file),
                String::new(),
                None,
            ));
            self.active_key.clear();
            self.import_open = false;
        }
        let loaded = self
            .load
            .as_ref()
            .and_then(|j| j.state.borrow_mut().result.take());
        if let Some(result) = loaded {
            let job = self.load.take().unwrap();
            match result {
                Ok(data) => {
                    self.notice = format!(
                        "{} Punkte geladen in {:.1} s.",
                        number(data.houses.len()),
                        (browser::now() - job.started) / 1000.
                    );
                    self.dataset_name = job.name;
                    if let Some(home) = self.pending_home.take() {
                        self.home = home;
                    }
                    if self.active_key.is_empty() {
                        let (min, max) = data.extent.unwrap();
                        self.home = (
                            [(min[0] + max[0]) / 2., (min[1] + max[1]) / 2.],
                            (8. / (max[0] - min[0]).max(max[1] - min[1]).max(0.001)).log2() + 6.,
                        );
                    }
                    self.store = Rc::new(RefCell::new(data));
                    self.clear_selection();
                    self.undo.clear();
                    self.dirty = false;
                    self.cache = RenderCache::default();
                    self.memory
                        .center_at(lon_lat(self.home.0[0], self.home.0[1]));
                    let _ = self.memory.set_zoom(self.home.1.clamp(2., 18.));
                    self.error = false;
                }
                Err(e) => {
                    self.pending_home = None;
                    self.notice = e;
                    self.error = true;
                }
            }
        }
        let selected = self
            .selection_job
            .as_ref()
            .and_then(|j| j.borrow_mut().result.take());
        if let Some(result) = selected {
            self.selection_job = None;
            match result {
                Ok(indices) => {
                    self.selected = indices;
                    self.cache.selected = self.selected.iter().copied().collect();
                    self.notice = format!(
                        "{} Punkte ausgewählt. Wähle einen neuen Status.",
                        number(self.selected.len())
                    );
                    self.error = false;
                }
                Err(e) => {
                    self.notice = e;
                    self.error = true;
                }
            }
        }
        let exported = self
            .export
            .as_ref()
            .and_then(|j| j.borrow_mut().result.take());
        if let Some(result) = exported {
            self.export = None;
            match result {
                Ok(()) => {
                    self.notice =
                        "CSV-Download gestartet. Die Datei kann wieder importiert werden.".into();
                    self.error = false;
                }
                Err(e) => {
                    self.notice = e;
                    self.error = true;
                }
            }
        }
    }
    fn header(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("header")
            .exact_size(68.)
            .frame(
                egui::Frame::new()
                    .fill(INK)
                    .inner_margin(egui::Margin::symmetric(24, 12)),
            )
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(
                        RichText::new("vibegis")
                            .size(30.)
                            .strong()
                            .color(Color32::WHITE),
                    );
                    ui.label(
                        RichText::new(".")
                            .size(30.)
                            .strong()
                            .color(Color32::from_rgb(121, 218, 158)),
                    );
                    ui.add_space(12.);
                    ui.label(
                        RichText::new("DEIN BLICK AUF DIE STADT")
                            .size(10.)
                            .color(Color32::from_rgb(166, 189, 174)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(
                                !self.busy(),
                                egui::Button::new(RichText::new("CSV Daten laden").color(INK))
                                    .fill(Color32::from_rgb(218, 237, 217)),
                            )
                            .clicked()
                        {
                            self.import_open = true;
                            self.user_chose_data = true;
                        }
                        if ui
                            .add_enabled(
                                !self.busy() && !self.store.borrow().houses.is_empty(),
                                egui::Button::new("CSV exportieren"),
                            )
                            .clicked()
                        {
                            self.export = Some(browser::export(
                                ui.ctx().clone(),
                                self.store.clone(),
                                "vibegis-export.csv".into(),
                            ));
                        }
                    });
                });
            });
    }
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("sidebar").exact_size(292.).resizable(false).frame(egui::Frame::new().fill(PAPER).inner_margin(20)).show(ui,|ui|{
            egui::ScrollArea::vertical().show(ui,|ui|{
                ui.add_space(8.); label(ui,"ARBEITSBEREICH");
                ui.label(RichText::new(&self.dataset_name).size(25.).strong());
                ui.label(RichText::new(if self.dirty{"Bearbeitet · nur in dieser Sitzung"}else{"OSM-Adresspunkte · lokale Sitzung"}).size(12.).color(MUTED));
                ui.add_space(10.);
                let totals=self.store.borrow().totals;
                let total:usize=totals.iter().sum();
                egui::Frame::new().fill(Color32::WHITE).corner_radius(10).inner_margin(16).show(ui,|ui|{
                    ui.set_width(ui.available_width());
                    label(ui,"PUNKTE IM SPEICHER");
                    ui.label(RichText::new(number(total)).size(32.).strong());
                    ui.add_space(6.);
                    for status in Status::ALL {ui.horizontal(|ui|{
                        let (r,_)=ui.allocate_exact_size(Vec2::splat(10.),egui::Sense::hover());ui.painter().circle_filled(r.center(),4.,color(status));
                        ui.label(status.name());ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{ui.label(RichText::new(number(totals[status as usize])).strong());});
                    });}
                });
                ui.add_space(16.);label(ui,"GEBIET BEARBEITEN");
                ui.label(RichText::new("Eine Auswahl.\nViele Möglichkeiten.").size(21.).strong());
                ui.add_space(2.);
                ui.add_enabled_ui(!self.busy()&&total>0,|ui|{
                    if button(ui,RichText::new(if self.drawing{"Polygon neu beginnen"}else{"Polygon auswählen"}).color(Color32::WHITE),true).clicked(){self.clear_selection();self.drawing=true;self.notice="Eckpunkte auf der Karte setzen. Mit Enter oder Doppelklick abschließen.".into();self.error=false;}
                    if self.drawing {
                        ui.label(format!("{} Eckpunkte gesetzt",self.vertices.len()));
                        if ui.add_enabled(self.vertices.len()>=3,egui::Button::new("Auswahl abschließen  ↵")).clicked(){self.finish_selection(ui.ctx());}
                        ui.label(RichText::new("Backspace: letzter Punkt · Esc: abbrechen").size(11.).color(MUTED));
                    } else if self.selected.is_empty(){
                        ui.label(RichText::new("1   Eckpunkte auf der Karte setzen\n2   Polygon schließen\n3   Status für alle Treffer wählen").size(13.).color(MUTED));
                    }
                    if !self.selected.is_empty(){
                        ui.add_space(8.);ui.label(RichText::new(format!("{} Punkte ausgewählt",number(self.selected.len()))).strong());
                        ui.label(RichText::new("NEUEN STATUS SETZEN").size(10.).color(MUTED));
                        ui.horizontal(|ui|{for status in Status::ALL {
                            if ui.add(egui::Button::new(RichText::new(status.name()).color(Color32::WHITE)).fill(color(status))).clicked(){
                                self.editing=Some(EditJob{target:Some(status),cursor:0,changes:Vec::new()});
                            }
                        }});
                    }
                    if !self.vertices.is_empty() && button(ui,"Auswahl aufheben",false).clicked(){self.clear_selection();}
                    if ui.add_enabled(!self.undo.is_empty(),egui::Button::new("↶  Letzte Statusänderung zurücknehmen")).clicked(){
                        self.editing=Some(EditJob{target:None,cursor:0,changes:Vec::new()});
                    }
                });
                ui.add_space(18.);ui.separator();ui.add_space(8.);
                label(ui,"GUT ZU WISSEN");
                ui.label(RichText::new("Die Farben sind Demo-Statuswerte. Änderungen bleiben im Arbeitsspeicher. Sichere deinen Stand als CSV.").size(12.).color(MUTED));
                ui.add_space(8.);
                ui.hyperlink_to("Daten: © OpenStreetMap · ODbL","https://www.openstreetmap.org/copyright");
                ui.label(RichText::new("Punkte sind OSM-Adressobjekte, kein vollständiges amtliches Hausverzeichnis.").size(11.).color(MUTED));
            });
        });
    }
    fn import_dialog(&mut self, ctx: &egui::Context) {
        let mut open = self.import_open;
        let mut chosen = None;
        let mut local = false;
        egui::Window::new("CSV Daten laden").open(&mut open).collapsible(false).resizable(false).default_width(460.).show(ctx,|ui|{
            ui.label(RichText::new("Wie groß ist dein nächstes Gebiet?").size(22.).strong());
            ui.label("Wähle OSM-Demodaten oder importiere eine eigene CSV.");
            if self.dirty{ui.colored_label(color(Status::Yellow),"Beim Laden wird der aktuelle Stand ersetzt. Bei Bedarf zuerst exportieren.");}
            ui.add_space(8.);
            for d in &self.datasets{
                egui::Frame::new().fill(Color32::WHITE).corner_radius(8).inner_margin(12).show(ui,|ui|{
                    ui.set_min_width(410.);
                    ui.horizontal(|ui|{
                        ui.vertical(|ui|{ui.label(RichText::new(&d.name).size(18.).strong());ui.label(RichText::new(format!("{} Punkte · {:.1} MB Download",number(d.rows),d.bytes as f64/1e6)).size(12.).color(MUTED));});
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{if ui.add_enabled(!self.busy(),egui::Button::new("Laden →")).clicked(){chosen=Some(d.clone());}});
                    });
                    ui.label(RichText::new(&d.description).size(11.).color(MUTED));
                });
            }
            ui.add_space(8.);ui.separator();
            if button(ui,"Eigene CSV-Datei auswählen …",false).clicked(){local=true;}
            ui.label(RichText::new("UTF-8 · Komma als Trennzeichen · WGS84").size(12.).color(MUTED));
            ui.code("id,status,longitude,latitude\nn123,gruen,12.0969,49.0134");
            ui.label(RichText::new("Status: rot, gelb, gruen. IDs: Zahl oder n/w/r + OSM-ID. Der Import ersetzt den Datensatz erst nach erfolgreicher Prüfung.").size(11.).color(MUTED));
        });
        self.import_open = open;
        if let Some(d) = chosen {
            self.start_dataset(ctx, d);
        }
        if local {
            self.user_chose_data = true;
            self.picker.open();
        }
    }
}

impl eframe::App for App {
    #[cfg(test)]
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        if self.editing.is_none() && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if let Some(job) = &self.selection_job {
                job.borrow_mut().cancelled = true;
            } else {
                self.clear_selection();
            }
        }
        if self.drawing && !self.busy() {
            if ctx.input(|i| i.key_pressed(egui::Key::Backspace)) {
                self.vertices.pop();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.finish_selection(&ctx);
            }
        }
        self.header(ui);
        egui::Panel::bottom("status")
            .exact_size(34.)
            .frame(
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .inner_margin(egui::Margin::symmetric(16, 7)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&self.notice).size(12.).color(if self.error {
                        color(Status::Red)
                    } else {
                        MUTED
                    }));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(p) = self.pointer {
                            ui.label(
                                RichText::new(format!("{:.5}° N  {:.5}° E", p[1], p[0]))
                                    .monospace()
                                    .size(11.)
                                    .color(MUTED),
                            );
                        }
                    });
                });
            });
        self.sidebar(ui);
        let mut finish = false;
        egui::CentralPanel::no_frame().show(ui, |ui| {
            let busy = self.busy();
            let data = self.store.borrow();
            Map::new(
                self.tiles
                    .as_mut()
                    .map(|tiles| tiles as &mut dyn walkers::Tiles),
                &mut self.memory,
                lon_lat(12.0969, 49.0134),
            )
            .double_click_to_zoom(!self.drawing)
            .double_click_to_zoom_out(false)
            .panning(!self.drawing)
            .show(ui, |ui, response, projector, memory| {
                let rect = response.rect;
                let p = projector.unproject(rect.left_bottom().to_vec2());
                let q = projector.unproject(rect.right_top().to_vec2());
                let min = [p.x().min(q.x()), p.y().min(q.y())];
                let max = [p.x().max(q.x()), p.y().max(q.y())];
                if memory.zoom() < 12. {
                    self.visible_count = 0;
                    for (&(x, y), counts) in &data.overview {
                        let point = [(x as f64 + 0.5) / 20., (y as f64 + 0.5) / 20.];
                        if point[0] < min[0] - 0.05
                            || point[0] > max[0] + 0.05
                            || point[1] < min[1] - 0.05
                            || point[1] > max[1] + 0.05
                        {
                            continue;
                        }
                        let pos = projector.project(lon_lat(point[0], point[1])).to_pos2();
                        for status in Status::ALL {
                            let count = counts[status as usize];
                            if count > 0 {
                                self.visible_count += count as usize;
                                let offset = (status as i32 - 1) as f32 * 5.;
                                ui.painter().circle_filled(
                                    pos + Vec2::new(offset, 0.),
                                    (1.3 + (count as f32).log10() * 0.7).min(4.),
                                    color(status),
                                );
                            }
                        }
                    }
                } else {
                    let key = ([min[0], min[1], max[0], max[1]], data.revision);
                    if self.cache.key != Some(key) {
                        self.cache.visible = data.candidates(min, max).collect();
                        self.cache.key = Some(key);
                    }
                    self.visible_count = self.cache.visible.len();
                    let mut mesh = egui::Mesh::default();
                    let radius = if memory.zoom() > 15. { 3.2 } else { 2.2 };
                    for &i in &self.cache.visible {
                        let h = &data.houses[i as usize];
                        let pos = projector
                            .project(lon_lat(h.position[0], h.position[1]))
                            .to_pos2();
                        if self.cache.selected.contains(&i) {
                            mesh.add_colored_rect(
                                egui::Rect::from_center_size(pos, Vec2::splat(radius * 2. + 3.)),
                                INK,
                            );
                        }
                        mesh.add_colored_rect(
                            egui::Rect::from_center_size(pos, Vec2::splat(radius * 2.)),
                            color(h.status),
                        );
                    }
                    ui.painter().add(mesh);
                }
                if response.hovered()
                    && let Some(pos) = ctx.pointer_hover_pos()
                {
                    let p = projector.unproject(pos.to_vec2());
                    self.pointer = Some([p.x(), p.y()]);
                }
                if self.drawing && !busy {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                    if response.double_clicked() {
                        finish = true;
                    } else if response.clicked()
                        && let Some(pos) = response.interact_pointer_pos()
                    {
                        let p = projector.unproject(pos.to_vec2());
                        self.vertices.push([p.x(), p.y()]);
                    }
                }
                let mut line: Vec<_> = self
                    .vertices
                    .iter()
                    .map(|p| projector.project(lon_lat(p[0], p[1])).to_pos2())
                    .collect();
                for p in &line {
                    ui.painter().circle_filled(*p, 4., INK);
                    ui.painter().circle_filled(*p, 2., Color32::WHITE);
                }
                if self.drawing {
                    if response.hovered()
                        && let Some(p) = ctx.pointer_hover_pos()
                    {
                        line.push(p);
                    }
                } else if let Some(&first) = line.first() {
                    line.push(first);
                }
                if line.len() > 1 {
                    ui.painter()
                        .add(egui::Shape::line(line, Stroke::new(2., INK)));
                }
                let hint = if self.drawing {
                    "POLYGONAUSWAHL  ·  Eckpunkte setzen, Enter zum Abschließen"
                } else if memory.zoom() < 12. {
                    "ÜBERSICHT  ·  Status je Gebiet verdichtet. Für Einzelpunkte hineinzoomen."
                } else {
                    "EINZELPUNKTE  ·  Ziehen zum Verschieben, Scrollen zum Zoomen"
                };
                let hint_rect = egui::Rect::from_min_size(
                    rect.left_top() + Vec2::new(18., 18.),
                    Vec2::new(530_f32.min(rect.width() - 36.), 32.),
                );
                ui.painter()
                    .rect_filled(hint_rect, 6., Color32::from_white_alpha(240));
                ui.painter().text(
                    hint_rect.left_center() + Vec2::new(12., 0.),
                    egui::Align2::LEFT_CENTER,
                    hint,
                    egui::FontId::proportional(11.),
                    INK,
                );
            });
        });
        if finish {
            self.finish_selection(&ctx);
        }
        egui::Area::new(egui::Id::unique("zoom"))
            .anchor(egui::Align2::RIGHT_TOP, Vec2::new(-20., 88.))
            .show(&ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .inner_margin(6)
                    .corner_radius(8)
                    .show(ui, |ui| {
                        if ui.button(RichText::new("+").size(22.)).clicked() {
                            let _ = self.memory.zoom_in();
                        }
                        if ui.button(RichText::new("−").size(22.)).clicked() {
                            let _ = self.memory.zoom_out();
                        }
                        if ui.button("⌂").on_hover_text("Zum Datensatz").clicked() {
                            self.memory
                                .center_at(lon_lat(self.home.0[0], self.home.0[1]));
                            let _ = self.memory.set_zoom(self.home.1.clamp(2., 18.));
                        }
                    });
            });
        egui::Area::new(egui::Id::unique("attribution"))
            .anchor(egui::Align2::RIGHT_BOTTOM, Vec2::new(-12., -43.))
            .show(&ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_white_alpha(245))
                    .inner_margin(6)
                    .show(ui, |ui| {
                        ui.hyperlink_to(
                            "© OpenStreetMap contributors",
                            "https://www.openstreetmap.org/copyright",
                        );
                    });
            });
        if self.import_open {
            self.import_dialog(&ctx);
        }
        if let Some(job) = &self.load {
            egui::Window::new("Daten werden geladen")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(&ctx, |ui| {
                    ui.label(RichText::new(&job.name).size(22.).strong());
                    let rows = job.state.borrow().rows;
                    ui.label(format!(
                        "{} Punkte eingelesen und räumlich indiziert",
                        number(rows)
                    ));
                    if let Some(total) = job.expected {
                        ui.add(
                            egui::ProgressBar::new(rows as f32 / total.max(1) as f32)
                                .show_percentage(),
                        );
                    } else {
                        ui.spinner();
                    }
                    ui.label(
                        RichText::new(
                            "Große Datensätze benötigen Zeit und freien Arbeitsspeicher.",
                        )
                        .size(12.)
                        .color(MUTED),
                    );
                    if ui.button("Import abbrechen").clicked() {
                        job.cancel();
                    }
                });
        }
        if let Some(job) = &self.selection_job {
            egui::Window::new("Polygon wird ausgewertet")
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(&ctx, |ui| {
                    ui.spinner();
                    ui.label(format!(
                        "{} Kandidaten geprüft",
                        number(job.borrow().checked)
                    ));
                    if ui.button("Abbrechen").clicked() {
                        job.borrow_mut().cancelled = true;
                    }
                });
        }
        if let Some(job) = &self.export {
            egui::Window::new("CSV wird exportiert")
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(&ctx, |ui| {
                    ui.spinner();
                    ui.label(format!("{} Punkte geschrieben", number(job.borrow().rows)));
                });
        }
        if let Some(job) = &self.editing {
            egui::Window::new("Status wird aktualisiert")
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(&ctx, |ui| {
                    ui.spinner();
                    ui.label(format!("{} Punkte bearbeitet", number(job.cursor)));
                });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gloo_timers::future::TimeoutFuture;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_test::*;

    #[wasm_bindgen_test]
    async fn webgl_app_renders_and_applies_a_polygon_edit() {
        let document = web_sys::window().unwrap().document().unwrap();
        let canvas: web_sys::HtmlCanvasElement =
            document.create_element("canvas").unwrap().unchecked_into();
        canvas.set_id("vibegis-test");
        canvas
            .set_attribute("style", "width:1024px;height:768px")
            .unwrap();
        document.body().unwrap().append_child(&canvas).unwrap();
        let runner = eframe::WebRunner::new();
        runner
            .start(
                canvas.clone(),
                eframe::WebOptions::default(),
                Box::new(|cc| {
                    let mut app = App::new(cc);
                    app.store = Rc::new(RefCell::new(
                        model::import_csv(
                            b"id,status,longitude,latitude\nn1,rot,12.0969,49.0134\n",
                        )
                        .unwrap(),
                    ));
                    Ok(Box::new(app))
                }),
            )
            .await
            .unwrap();
        TimeoutFuture::new(500).await;
        assert!(!runner.has_panicked());
        {
            let mut app = runner.app_mut::<App>().unwrap();
            assert_eq!(app.visible_count, 1);
            app.vertices = vec![
                [12.09, 49.01],
                [12.10, 49.01],
                [12.10, 49.02],
                [12.09, 49.02],
            ];
            app.finish_selection(&egui::Context::default());
        }
        TimeoutFuture::new(100).await;
        {
            let mut app = runner.app_mut::<App>().unwrap();
            app.poll(&egui::Context::default());
            assert_eq!(app.selected, vec![0]);
            app.editing = Some(EditJob {
                target: Some(Status::Green),
                cursor: 0,
                changes: Vec::new(),
            });
            app.poll(&egui::Context::default());
            assert_eq!(app.store.borrow().totals, [0, 0, 1]);
            app.editing = Some(EditJob {
                target: Some(Status::Green),
                cursor: 0,
                changes: Vec::new(),
            });
            app.poll(&egui::Context::default());
            assert_eq!(
                app.undo.len(),
                1,
                "A no-op must preserve the previous undo operation"
            );
            app.editing = Some(EditJob {
                target: None,
                cursor: 0,
                changes: Vec::new(),
            });
            app.poll(&egui::Context::default());
            assert_eq!(app.store.borrow().totals, [1, 0, 0]);
        }
        runner.destroy();
        canvas.remove();
        if let Some(input) = document.get_element_by_id("csv-file") {
            input.remove();
        }
    }
}
