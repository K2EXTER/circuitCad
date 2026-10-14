use super::*;

impl SchEditor {
    /// Floating window of the running challenge.
    pub fn challenge_window(&mut self, ctx: &egui::Context) {
        let Some(active) = self.challenge.as_ref() else { return };
        let Some(ch) = challenge(active.id) else { return };
        let (level, hints, passed) = (active.level, active.hints, active.passed);
        let mut judge = false;
        let mut new_level = None;
        let mut more_hint = false;
        let mut show_answer = false;
        let mut back = false;
        let mut close = false;
        egui::Window::new(format!("作ってみよう: {}", ch.title)).default_width(400.0).anchor(Align2::RIGHT_TOP, Vec2::new(-8.0, 40.0)).show(ctx, |ui| {
            ui.label(ch.goal);
            ui.horizontal(|ui| {
                ui.label("むずかしさ");
                if ui.selectable_label(level == 0, "配線だけ").on_hover_text("部品は置いてあります。線でつなぐ練習です (やり直すと、最初の状態に戻ります)").clicked() && level != 0 {
                    new_level = Some(0);
                }
                if ui.selectable_label(level == 1, "部品も置く").on_hover_text("電源だけが置いてあります。部品を選んで置き、配線し、値も決めます (やり直すと、最初の状態に戻ります)").clicked() && level != 1 {
                    new_level = Some(1);
                }
            });
            ui.separator();
            let results = self.challenge.as_ref().map(|c| &c.results[..]).unwrap_or(&[]);
            if results.is_empty() {
                ui.label("作り終えたら、「判定する」を押してください。回路を実際にシミュレーションして、次の項目を調べます");
            }
            for r in results.iter() {
                let (mark, color) = match r.ok {
                    Some(true) => ("✔", Color32::from_rgb(60, 160, 90)),
                    Some(false) => ("✖", Color32::from_rgb(230, 90, 70)),
                    None => ("○", Color32::from_gray(140)),
                };
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(color, mark);
                    ui.label(egui::RichText::new(&r.label).strong());
                });
                if !r.detail.is_empty() {
                    ui.indent(("challenge_detail", &r.label), |ui| {
                        ui.label(egui::RichText::new(&r.detail).color(if r.ok == Some(false) { Color32::from_rgb(230, 150, 120) } else { Color32::from_gray(170) }));
                    });
                }
            }
            if passed {
                ui.separator();
                ui.colored_label(Color32::from_rgb(60, 160, 90), "できました!");
                ui.label("自分で作った回路が、判定のすべての条件を満たしています。値や部品を変えても通るか (答えは 1 つではありません)、条件のぎりぎりを探してみましょう");
            }
            for (k, h) in ch.hints.iter().enumerate().take(hints) {
                ui.separator();
                ui.label(egui::RichText::new(format!("ヒント {}", k + 1)).strong());
                ui.label(*h);
            }
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if ui.button("判定する").on_hover_text("いまの回路をシミュレーションして、目標を満たしているか調べます。何度でも押せます").clicked() {
                    judge = true;
                }
                if ui.add_enabled(hints < ch.hints.len(), egui::Button::new(format!("ヒントを見る ({hints} / {})", ch.hints.len()))).on_hover_text("行き詰まったら、少しずつヒントを出します。最後のヒントは、答えの目安です").clicked() {
                    more_hint = true;
                }
                if ui.button("答えの回路を見る").on_hover_text("できあがりの回路を、いまの回路と入れ替えます (Ctrl+Z で戻せます)。答えは 1 つではありません。判定を通れば、別の値や形でも正解です").clicked() {
                    show_answer = true;
                }
            });
            ui.horizontal(|ui| {
                if ui.button("レッスンに戻る").on_hover_text("このチャレンジをやめて、レッスンの手順に戻ります").clicked() {
                    back = true;
                }
                if ui.button("閉じる").clicked() {
                    close = true;
                }
            });
        });
        if let Some(l) = new_level {
            self.start_challenge(ch.id, l);
        } else if judge {
            self.run_judge();
        } else if more_hint {
            if let Some(a) = self.challenge.as_mut() {
                a.hints = (a.hints + 1).min(ch.hints.len());
            }
        } else if show_answer {
            // not through `load_sample`: the history stays, so Ctrl+Z brings the learner's own circuit back
            self.sch = solution_of(ch);
            self.fit_pending = true;
        } else if back {
            let lesson = ch.lesson;
            self.challenge = None;
            if let Some(i) = lesson_ids().iter().position(|l| *l == lesson) {
                self.start_lesson(i);
            }
        } else if close {
            self.challenge = None;
        }
    }
}
