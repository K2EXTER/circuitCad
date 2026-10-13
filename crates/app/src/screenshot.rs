use super::*;

impl App {
    /// `--screenshot`: waits a few frames so the analyses and layout have settled, takes the capture, then quits.
    pub(super) fn tick_auto_screenshot(&mut self, ui: &egui::Ui) {
        let Some((path, left)) = self.auto_shot.as_mut() else { return };
        ui.ctx().request_repaint();
        if *left > 0 {
            *left -= 1;
        } else if self.shot_path.is_none() {
            self.shot_path = Some(path.clone());
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
    }

    /// Writes the captured frame as a PNG once the viewport hands it back.
    pub(super) fn finish_screenshot(&mut self, ui: &egui::Ui) {
        let Some(path) = self.shot_path.clone() else { return };
        let shot = ui.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        let Some(img) = shot else { return };
        self.shot_path = None;
        let [w, h] = img.size;
        let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
        let msg = match image::save_buffer(&path, &rgba, w as u32, h as u32, image::ColorType::Rgba8) {
            Ok(()) => format!("画面を保存しました: {}", path.display()),
            Err(e) => format!("画像を保存できませんでした: {e}。保存先のフォルダに書き込めるか確認してください"),
        };
        if self.auto_shot.take().is_some() {
            // a script closes the window: no question
            self.closing_ok = true;
            println!("{msg}");
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.sch.status = msg.clone();
        self.board_ed.status = msg;
    }
}
