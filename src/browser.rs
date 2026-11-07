use crate::model::{HEADER, House, Store};
use eframe::egui;
use futures::{AsyncReadExt, SinkExt, TryStreamExt};
use gloo_timers::future::TimeoutFuture;
use std::{cell::RefCell, collections::HashSet, rc::Rc};
use wasm_bindgen::{JsCast, closure::Closure};
use wasm_bindgen_futures::{JsFuture, spawn_local};

pub fn now() -> f64 {
    web_sys::window().unwrap().performance().unwrap().now()
}
pub type SharedStore = Rc<RefCell<Store>>;

#[derive(Default)]
pub struct Progress {
    pub rows: usize,
    pub cancelled: bool,
    pub result: Option<Result<Store, String>>,
}
pub struct LoadJob {
    pub state: Rc<RefCell<Progress>>,
    pub name: String,
    pub started: f64,
    pub expected: Option<usize>,
    pub controller: web_sys::AbortController,
}
impl LoadJob {
    pub fn cancel(&self) {
        self.state.borrow_mut().cancelled = true;
        self.controller.abort();
    }
}
fn message(e: impl std::fmt::Debug) -> String {
    format!("{e:?}")
}

pub fn load(
    ctx: egui::Context,
    name: String,
    file: Option<web_sys::File>,
    url: String,
    expected: Option<usize>,
) -> LoadJob {
    let state = Rc::new(RefCell::new(Progress::default()));
    let shared = state.clone();
    let controller = web_sys::AbortController::new().unwrap();
    let signal = controller.signal();
    spawn_local(async move {
        let result = async {
            let stream = if let Some(file) = file { file.stream() } else {
                let opts = web_sys::RequestInit::new();
                opts.set_signal(Some(&signal));
                let response: web_sys::Response = JsFuture::from(web_sys::window().unwrap().fetch_with_str_and_init(&url,&opts))
                    .await.map_err(message)?.dyn_into().map_err(message)?;
                if !response.ok() { return Err(format!("Datensatz nicht verfügbar (HTTP {}).",response.status())); }
                response.body().ok_or("Der Server liefert keine CSV-Datei.")?
            };
            let mut input = wasm_streams::ReadableStream::from_raw(stream.unchecked_into()).into_async_read();
            // csv-async requires a Send reader. Transfer owned byte chunks through
            // a bounded channel, keeping all JS objects on the browser thread.
            let (mut sender, receiver)=futures::channel::mpsc::channel::<std::io::Result<Vec<u8>>>(2);
            spawn_local(async move {
                loop {
                    let mut chunk=vec![0;64*1024];
                    match input.read(&mut chunk).await {
                        Ok(0)=>break,
                        Ok(n)=>{chunk.truncate(n);if sender.send(Ok(chunk)).await.is_err(){break;}},
                        Err(e)=>{let _=sender.send(Err(e)).await;break;}
                    }
                }
            });
            let mut reader = csv_async::AsyncReaderBuilder::new().create_reader(receiver.into_async_read());
            if reader.headers().await.map_err(message)?.iter().ne(HEADER) {
                return Err("CSV-Kopf muss id,status,longitude,latitude lauten. UTF-8 und Kommas verwenden.".into());
            }
            let mut store = Store::default();
            let mut ids = HashSet::new();
            if let Some(count)=expected {
                store.houses.try_reserve_exact(count).map_err(|_| "Nicht genügend Arbeitsspeicher für diesen Datensatz.")?;
                ids.try_reserve(count).map_err(|_| "Nicht genügend Arbeitsspeicher für die ID-Prüfung.")?;
            }
            let mut record = csv_async::StringRecord::new();
            let mut last = now();
            while reader.read_record(&mut record).await.map_err(message)? {
                let row = store.houses.len()+2;
                let house = House::from_fields(record.iter()).map_err(|e|format!("Zeile {row}: {e}"))?;
                if !ids.insert(house.id) { return Err(format!("Doppelte ID in Zeile {row}.")); }
                store.push(house)?;
                if store.houses.len()%512 == 0 && now()-last > 10. {
                    shared.borrow_mut().rows = store.houses.len();
                    if shared.borrow().cancelled { return Err("Import abgebrochen.".into()); }
                    ctx.request_repaint();
                    TimeoutFuture::new(0).await;
                    last = now();
                }
            }
            if shared.borrow().cancelled { return Err("Import abgebrochen.".into()); }
            if store.houses.is_empty() { return Err("Die CSV enthält keine Punkte.".into()); }
            Ok(store)
        }.await;
        shared.borrow_mut().result = Some(result);
        ctx.request_repaint();
    });
    LoadJob {
        state,
        name,
        expected,
        controller,
        started: now(),
    }
}

pub struct FilePicker {
    input: web_sys::HtmlInputElement,
    pub chosen: Rc<RefCell<Option<web_sys::File>>>,
    _change: Closure<dyn FnMut()>,
}
impl FilePicker {
    pub fn new(ctx: egui::Context) -> Self {
        let document = web_sys::window().unwrap().document().unwrap();
        let input: web_sys::HtmlInputElement =
            document.create_element("input").unwrap().unchecked_into();
        input.set_type("file");
        input.set_accept(".csv,text/csv");
        input.set_id("csv-file");
        input.set_attribute("style", "display:none").unwrap();
        input
            .set_attribute("aria-label", "CSV Daten laden")
            .unwrap();
        document.body().unwrap().append_child(&input).unwrap();
        let chosen = Rc::new(RefCell::new(None));
        let output = chosen.clone();
        let element = input.clone();
        let change = Closure::new(move || {
            if let Some(file) = element.files().and_then(|f| f.get(0)) {
                *output.borrow_mut() = Some(file);
                ctx.request_repaint();
            }
        });
        input.set_onchange(Some(change.as_ref().unchecked_ref()));
        Self {
            input,
            chosen,
            _change: change,
        }
    }
    pub fn open(&self) {
        self.input.set_value("");
        self.input.click();
    }
}

#[derive(Default)]
pub struct ExportProgress {
    pub rows: usize,
    pub result: Option<Result<(), String>>,
}
pub fn export(
    ctx: egui::Context,
    store: SharedStore,
    filename: String,
) -> Rc<RefCell<ExportProgress>> {
    let state = Rc::new(RefCell::new(ExportProgress::default()));
    let shared = state.clone();
    spawn_local(async move {
        let result = async {
            let parts = js_sys::Array::new();
            let mut writer = csv::Writer::from_writer(Vec::new());
            writer.write_record(HEADER).map_err(message)?;
            let total = store.borrow().houses.len();
            for start in (0..total).step_by(10_000) {
                {
                    let data = store.borrow();
                    for house in &data.houses[start..(start + 10_000).min(total)] {
                        writer.write_record(house.record()).map_err(message)?;
                    }
                }
                writer.flush().map_err(message)?;
                let bytes = writer.into_inner().map_err(message)?;
                parts.push(&js_sys::Uint8Array::from(bytes.as_slice()));
                writer = csv::Writer::from_writer(Vec::new());
                shared.borrow_mut().rows = (start + 10_000).min(total);
                ctx.request_repaint();
                TimeoutFuture::new(0).await;
            }
            let opts = web_sys::BlobPropertyBag::new();
            opts.set_type("text/csv;charset=utf-8");
            let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts)
                .map_err(message)?;
            let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(message)?;
            let document = web_sys::window().unwrap().document().unwrap();
            let a: web_sys::HtmlAnchorElement = document
                .create_element("a")
                .map_err(message)?
                .unchecked_into();
            a.set_href(&url);
            a.set_download(&filename);
            document.body().unwrap().append_child(&a).map_err(message)?;
            a.click();
            a.remove();
            spawn_local(async move {
                TimeoutFuture::new(30_000).await;
                let _ = web_sys::Url::revoke_object_url(&url);
            });
            Ok(())
        }
        .await;
        shared.borrow_mut().result = Some(result);
        ctx.request_repaint();
    });
    state
}

#[derive(Default)]
pub struct SelectionProgress {
    pub checked: usize,
    pub cancelled: bool,
    pub result: Option<Result<Vec<u32>, String>>,
}
// The UI disables all mutations while selection_job exists. Holding this read
// borrow keeps the spatial-index iterator alive without allocating millions of IDs.
#[expect(
    clippy::await_holding_refcell_ref,
    reason = "Read-only selection holds the index; App::busy disables mutation until it finishes"
)]
pub fn select(
    ctx: egui::Context,
    store: SharedStore,
    vertices: Vec<[f64; 2]>,
) -> Rc<RefCell<SelectionProgress>> {
    use geo::Intersects;
    let state = Rc::new(RefCell::new(SelectionProgress::default()));
    let shared = state.clone();
    spawn_local(async move {
        let result = async {
            let polygon = crate::model::selection_polygon(&vertices)?;
            let (min, max) = crate::model::bounds(&vertices);
            let data = store.borrow();
            let mut found = Vec::new();
            let mut checked = 0;
            let mut last = now();
            for i in data.candidates(min, max) {
                let p = crate::model::mercator(data.houses[i as usize].position);
                if polygon.intersects(&geo::Point::new(p[0], p[1])) {
                    found.push(i);
                }
                checked += 1;
                if checked % 1024 == 0 && now() - last > 10. {
                    if shared.borrow().cancelled {
                        return Err("Auswahl abgebrochen.".into());
                    }
                    shared.borrow_mut().checked = checked;
                    ctx.request_repaint();
                    TimeoutFuture::new(0).await;
                    last = now();
                }
            }
            if shared.borrow().cancelled {
                return Err("Auswahl abgebrochen.".into());
            }
            Ok(found)
        }
        .await;
        shared.borrow_mut().result = Some(result);
        ctx.request_repaint();
    });
    state
}

#[derive(Clone, serde::Deserialize)]
pub struct Dataset {
    pub key: String,
    pub name: String,
    pub description: String,
    pub rows: usize,
    pub bytes: u64,
    pub center: [f64; 2],
    pub zoom: f64,
}
#[derive(serde::Deserialize)]
struct Manifest {
    datasets: Vec<Dataset>,
}
pub type ManifestResult = Rc<RefCell<Option<Result<Vec<Dataset>, String>>>>;
pub fn manifest(ctx: egui::Context) -> ManifestResult {
    let state = Rc::new(RefCell::new(None));
    let shared = state.clone();
    spawn_local(async move {
        let result = async {
            let response: web_sys::Response = JsFuture::from(
                web_sys::window()
                    .unwrap()
                    .fetch_with_str("data/manifest.json"),
            )
            .await
            .map_err(message)?
            .unchecked_into();
            if !response.ok() {
                return Err("Demodaten noch nicht vorbereitet.".into());
            }
            let text = JsFuture::from(response.text().map_err(message)?)
                .await
                .map_err(message)?
                .as_string()
                .unwrap_or_default();
            serde_json::from_str::<Manifest>(&text)
                .map(|m| m.datasets)
                .map_err(message)
        }
        .await;
        *shared.borrow_mut() = Some(result);
        ctx.request_repaint();
    });
    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;
    wasm_bindgen_test_configure!(run_in_browser);

    fn file(csv: &str) -> web_sys::File {
        let parts = js_sys::Array::new();
        parts.push(&wasm_bindgen::JsValue::from_str(csv));
        web_sys::File::new_with_str_sequence(&parts, "test.csv").unwrap()
    }
    async fn result(job: &LoadJob) -> Result<Store, String> {
        let start = now();
        loop {
            if let Some(result) = job.state.borrow_mut().result.take() {
                return result;
            }
            assert!(now() - start < 30_000., "CSV import timed out");
            TimeoutFuture::new(10).await;
        }
    }
    #[wasm_bindgen_test]
    async fn streams_file_csv_and_preserves_last_record_without_newline() {
        let mut csv = String::from("id,status,longitude,latitude\n");
        for i in 0..6000 {
            csv.push_str(&format!("n{i},gruen,12.0969,49.0134\n"));
        }
        csv.pop();
        let job = load(
            egui::Context::default(),
            "test".into(),
            Some(file(&csv)),
            String::new(),
            Some(6000),
        );
        let store = result(&job).await.unwrap();
        assert_eq!(store.houses.len(), 6000);
        assert_eq!(store.totals, [0, 0, 6000]);
    }
    #[wasm_bindgen_test]
    async fn rejects_duplicate_ids_in_browser_stream() {
        let csv = "id,status,longitude,latitude\nn1,rot,12,49\nn1,gelb,12,49\n";
        let job = load(
            egui::Context::default(),
            "test".into(),
            Some(file(csv)),
            String::new(),
            None,
        );
        assert!(result(&job).await.err().unwrap().contains("Doppelte ID"));
    }
    #[wasm_bindgen_test]
    async fn cancelled_import_does_not_publish_partial_store() {
        let job = load(
            egui::Context::default(),
            "test".into(),
            Some(file("id,status,longitude,latitude\nn1,rot,12,49\n")),
            String::new(),
            None,
        );
        job.cancel();
        assert!(result(&job).await.is_err());
    }
}
