//! circuitCad in the browser (B9): the learning part only (DC voltages, digital circuits that run), nothing saved, no board /
//! 3D / external tools.
//! `trunk serve` (in this folder) builds it for wasm32 and opens it; `cargo run -p web` runs the same screens in a window,
//! to try a change without the browser.

mod app;
mod live_ui;
mod samples;
mod view;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    let options = eframe::NativeOptions { viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1000.0, 700.0]), ..Default::default() };
    eframe::run_native("circuitCad web", options, Box::new(|cc| Ok(Box::new(app::WebApp::new(cc)))))
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast;
    let options = eframe::WebOptions::default();
    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window().expect("no window").document().expect("no document");
        let canvas = document.get_element_by_id("ccad_canvas").expect("no #ccad_canvas in index.html").dyn_into::<web_sys::HtmlCanvasElement>().expect("#ccad_canvas is not a canvas");
        let started = eframe::WebRunner::new().start(canvas, options, Box::new(|cc| Ok(Box::new(app::WebApp::new(cc))))).await;
        // the loading text is replaced by the app, or by what went wrong
        if let Some(loading) = document.get_element_by_id("ccad_loading") {
            match started {
                Ok(()) => loading.remove(),
                Err(e) => loading.set_inner_html(&format!("<p>起動できませんでした (Could not start): {e:?}</p>")),
            }
        }
    });
}
