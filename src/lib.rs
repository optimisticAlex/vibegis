pub mod model;

#[cfg(target_arch = "wasm32")]
mod app;
#[cfg(target_arch = "wasm32")]
mod browser;

#[cfg(all(target_arch = "wasm32", not(test)))]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub async fn start() -> Result<(), wasm_bindgen::JsValue> {
    use wasm_bindgen::JsCast;
    console_error_panic_hook::set_once();
    let document = web_sys::window().unwrap().document().unwrap();
    let canvas = document
        .get_element_by_id("vibegis")
        .unwrap()
        .dyn_into::<web_sys::HtmlCanvasElement>()?;
    let result = eframe::WebRunner::new()
        .start(
            canvas,
            eframe::WebOptions::default(),
            Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
        )
        .await;
    if let Err(error) = result {
        if let Some(loading) = document.get_element_by_id("loading") {
            loading.set_text_content(Some("vibegis konnte die Kartenoberfläche nicht starten. Bitte WebGL aktivieren und einen aktuellen Browser verwenden."));
        }
        return Err(error);
    }
    if let Some(loading) = document.get_element_by_id("loading") {
        loading.remove();
    }
    Ok(())
}
