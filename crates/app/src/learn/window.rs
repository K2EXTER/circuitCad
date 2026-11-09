//! The learning-path window and the welcome card of the first launch.

use super::*;

/// The card shown over an empty sheet on the very first launch: where to begin, in three buttons.
pub fn welcome_card(ctx: &egui::Context, progress: &Progress) -> Option<Welcome> {
    let mut choice = None;
    let mut open = true;
    egui::Window::new(i18n::tr("ようこそ circuitCad へ")).open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).default_width(460.0).show(ctx, |ui| {
        ui.label(spaced("回路図を描き、動かして確かめ、基板にして、発注用のデータまで作れるソフトです。電子工作が初めてでも、レッスンを順にたどれば進められます。"));
        ui.label(spaced("最初のレッスンは、描いてある回路を動かして見るところから。自分で部品を置いて描くのは、そのあとのレッスンからです。").color(NOTE));
        ui.add_space(6.0);
        if let Some((next, action)) = next_item(first_path(), progress).and_then(|i| item_action(i).map(|a| (i, a))) {
            let button = egui::Button::new(egui::RichText::new(i18n::trf!("▶ 最初のレッスンを始める: {}", i18n::tr(next.title))).strong());
            if ui.add(button).on_hover_text(i18n::tr("画面の案内にそって、描いてある回路を解析して、電圧を測ります (部品を置いて配線するのは、このあとのレッスンから)")).clicked() {
                choice = Some(Welcome::Start(action));
            }
        }
        if ui.button(i18n::tr("学習パス全体を見る")).on_hover_text(i18n::tr("電圧と電流の基本から、部品を 1 つずつ知る道すじを開きます。安定化電源・マイコンの基板・基板づくりの道も、上のタブから選べます (「学ぶ」メニューからいつでも開けます)")).clicked() {
            choice = Some(Welcome::Path);
        }
        if ui.button(i18n::tr("操作ガイドを開く (F1)")).on_hover_text(i18n::tr("部品の置き方・配線・シミュレーションのやり方を説明します")).clicked() {
            choice = Some(Welcome::Guide);
        }
        ui.add_space(4.0);
        ui.label(spaced("自由に描きたいときは、このカードを閉じて、左の一覧から部品を選んで画面をクリックします。できあがった回路を先に見たいときは、「ファイル → サンプル回路を開く」から。").color(NOTE));
        if ui.button(i18n::tr("閉じる (次から表示しない)")).on_hover_text(i18n::tr("このカードを閉じます。次の起動から表示しません。学習パスは、メニューの「学ぶ」からいつでも開けます")).clicked() {
            choice = Some(Welcome::Close);
        }
    });
    if !open && choice.is_none() {
        choice = Some(Welcome::Close);
    }
    choice
}

#[derive(Default)]
pub struct LearnWindow {
    pub open: bool,
    /// Which of `PATHS` is shown.
    selected: usize,
}

impl LearnWindow {
    /// Shows the path with this id (command line `--learn <id>`); an unknown id leaves the choice alone.
    pub fn select(&mut self, id: &str) {
        if let Some(k) = PATHS.iter().position(|p| p.id == id) {
            self.selected = k;
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, progress: &mut Progress) -> Option<Action> {
        if !self.open {
            return None;
        }
        let mut open = true;
        let mut action = None;
        let mut chosen = self.selected.min(PATHS.len() - 1);
        let path = &PATHS[chosen];
        // a map is wide: open the window as wide as the screen allows (up to 1200 px) so that it needs no sideways scroll
        let tree_width = (ctx.content_rect().width() * 0.85).clamp(640.0, 1200.0);
        egui::Window::new(i18n::tr("学習パス")).open(&mut open).default_width(if is_tree(path) { tree_width } else { 600.0 }).default_height((ctx.content_rect().height() * 0.92).max(480.0)).vscroll(true).show(ctx, |ui| {
            if PATHS.len() > 1 {
                let tabs: Vec<(usize, &str, &str)> = PATHS.iter().enumerate().map(|(k, p)| (k, p.title, "この学習パスに切り替えます (進み具合は、どのパスでも同じレッスンなら共通です)")).collect();
                crate::widgets::tab_bar(ui, &mut chosen, &tabs);
            }
            ui.heading(i18n::tr(path.title));
            ui.label(spaced(path.goal));
            let (finished, available, all) = summary(path, progress);
            ui.add(egui::ProgressBar::new(finished as f32 / available.max(1) as f32).text(if all > available { i18n::trf!("完了 {finished} / {available} レッスン (準備中 {} 件)", all - available) } else { i18n::trf!("完了 {finished} / {available} レッスン") }));
            let next_ids: Vec<&str> = recommended(path, progress).iter().map(|i| i.id).collect();
            if let Some(next) = next_item(path, progress) {
                ui.horizontal_wrapped(|ui| {
                    ui.label(i18n::tr("次のおすすめ:"));
                    ui.colored_label(crate::theme::WARN, i18n::tr(next.title));
                    if next_ids.len() > 1 {
                        ui.label(i18n::trf!("(ほかに {} 件、すぐ学べます)", next_ids.len() - 1));
                    }
                });
            } else {
                if available == 0 {
                    ui.label(spaced("このパスのレッスンは準備中です。道すじ (地図) だけ先に見られます").color(NOTE));
                } else {
                    ui.colored_label(crate::theme::OK, i18n::tr("いま学べるレッスンは、すべて完了しました。残りは準備中です"));
                }
            }
            // the lessons are done (or none is left to do): the way on is shown open
            let done_all = available > 0 && finished >= available;
            egui::CollapsingHeader::new(i18n::tr("この先へ: 使いこむための機能")).default_open(done_all).show(ui, |ui| {
                ui.label(spaced("レッスンで基本を済ませたあと、慣れてきたら試せる機能です。順番も、使うかどうかも自由です").color(NOTE));
                for s in NEXT_STEPS {
                    ui.add_space(4.0);
                    ui.strong(i18n::tr(s.title));
                    ui.indent(("next_step", s.title), |ui| {
                        ui.label(spaced(s.about));
                        ui.label(spaced(&format!("場所: {}", s.where_)).color(NOTE));
                    });
                }
            }).header_response.on_hover_text(i18n::tr("基板の仕上げ・発注・故障の試験・ほかのソフトとの連携など、レッスンのあとに使える機能の一覧と、その場所です"));
            if is_tree(path) {
                ui.add_space(6.0);
                egui::CollapsingHeader::new(i18n::tr("学習の地図 (上から下へ進みます。線は「先に済ませると分かりやすい」の関係です)")).default_open(true).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 18.0;
                        ui.colored_label(crate::theme::HIGHLIGHT, i18n::tr("★ 次はこれ"));
                        ui.colored_label(crate::theme::WARN, i18n::tr("橙の枠 = いま学べる"));
                        ui.colored_label(crate::theme::OK, i18n::tr("緑 = 完了"));
                        ui.colored_label(Color32::from_gray(190), i18n::tr("灰色の枠 = 先に済ませるレッスンがある"));
                        ui.colored_label(Color32::from_gray(130), i18n::tr("暗い灰色 = 準備中"));
                    });
                    egui::ScrollArea::horizontal().show(ui, |ui| {
                        if let Some(a) = tree_map(ui, path, progress, &next_ids) {
                            action = Some(a);
                        }
                    });
                    ui.label(spaced("緑 = 完了、橙の枠 = いますぐ学べる、灰 = 準備中。順番は強制ではなく、どれからでも始められます").color(NOTE));
                });
            }
            for stage in path.stages {
                ui.add_space(8.0);
                ui.strong(i18n::tr(stage.name));
                ui.label(spaced(stage.about).color(NOTE));
                for item in stage.items {
                    let done = item_done(progress, item);
                    let (mark, color) = match item.kind {
                        Kind::Planned => ("…", Color32::from_gray(120)),
                        _ if done => ("✔", crate::theme::OK),
                        _ if next_ids.contains(&item.id) => ("▶", crate::theme::WARN),
                        _ => ("○", Color32::from_gray(150)),
                    };
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(color, mark);
                        let title = egui::RichText::new(i18n::tr(item.title)).strong();
                        ui.label(if item.kind == Kind::Planned { title.color(Color32::from_gray(130)) } else { title });
                        if item.kind == Kind::Planned {
                            ui.label(egui::RichText::new(i18n::tr("(準備中)")).weak());
                        } else if item.kind != Kind::Build && ui.small_button(if done { i18n::tr("もう一度") } else { i18n::tr("始める") }).on_hover_text(i18n::tr("この内容のレッスンを開きます")).clicked() {
                            action = Some(match item.kind {
                                Kind::Lesson(id) => Action::Lesson(id),
                                Kind::Measure => Action::Measure,
                                Kind::Hands => Action::Hands(item.id),
                                Kind::Board(n) => Action::Board(n),
                                _ => Action::Measure,
                            });
                        }
                        if item.build {
                            let passed = progress.is_done(&crate::schematic::challenge_progress_key(item.id));
                            if ui.small_button(if passed { i18n::tr("✔ 作ってみよう") } else { i18n::tr("作ってみよう") }).on_hover_text(i18n::tr("目標だけを示して、自分で部品を置き、配線し、値を決める練習です。作ったあと、判定ボタンで調べます")).clicked() {
                                action = Some(Action::Build(item.id));
                            }
                        }
                        if item.real.is_some() {
                            ui.colored_label(crate::theme::INFO, i18n::tr("[実験あり]")).on_hover_text(i18n::tr(item.real.unwrap_or_default()));
                        }
                    });
                    ui.indent(("path_item", path.id, item.id), |ui| {
                        ui.label(spaced(item.about).color(NOTE));
                        if item.requires.is_some_and(|r| !r.is_empty()) {
                            let names: Vec<&str> = requirements(path, item).iter().map(|p| i18n::tr(p.title)).collect();
                            ui.label(spaced(&format!("先に済ませると分かりやすい: {}", names.join(if i18n::is_ja() { "、" } else { ", " }))).color(NOTE));
                        }
                    });
                    ui.add_space(6.0);
                }
            }
            ui.add_space(8.0);
            ui.separator();
            ui.label(spaced("[実験あり] = 実機で確かめられる実験があります (ポインタを置くと内容が出ます)。実験は、安全のため低い電圧 (12V 以下) だけで行い、商用電源 (100V) の配線は行いません"));
            if let Some(e) = &progress.error {
                ui.colored_label(crate::theme::ERROR, e);
            }
            if ui.button(i18n::tr("進み具合をリセット")).on_hover_text(i18n::tr("完了の印をすべて消して、最初からやり直します")).clicked() {
                progress.reset();
            }
        });
        self.selected = chosen;
        if !open {
            self.open = false;
        }
        action
    }
}

/// The map of a tree path: one row per level, an item per box, a line from each prerequisite down to what needs it.
fn tree_map(ui: &mut egui::Ui, path: &LearningPath, progress: &Progress, next_ids: &[&str]) -> Option<Action> {
    const W: f32 = 196.0;
    const H: f32 = 60.0;
    const GX: f32 = 12.0;
    const GY: f32 = 26.0;
    // room above the first row for the badge of the recommended box, which sits on the top edge of its box
    const TOP: f32 = 22.0;
    let all: Vec<&'static Item> = items(path).collect();
    let levels: Vec<usize> = all.iter().map(|i| level(path, i, 0)).collect();
    let rows_of_level = levels.iter().max().copied().unwrap_or(0) + 1;
    // a level with more boxes than fit in the window's width goes on to the next row, so the map never needs a sideways scroll
    let max_cols = (((ui.available_width() + GX) / (W + GX)).floor() as usize).max(1);
    let mut in_level = vec![0usize; rows_of_level];
    let index_in_level: Vec<usize> = levels.iter().map(|&l| { in_level[l] += 1; in_level[l] - 1 }).collect();
    let mut first_row = vec![0usize; rows_of_level];
    let mut rows = 0;
    for l in 0..rows_of_level {
        first_row[l] = rows;
        rows += in_level[l].div_ceil(max_cols).max(1);
    }
    let cols_in_row = |row: usize| -> usize {
        let l = (0..rows_of_level).rev().find(|&l| first_row[l] <= row).unwrap_or(0);
        let r = row - first_row[l];
        (in_level[l] - r * max_cols).min(max_cols)
    };
    let widest = (0..rows).map(cols_in_row).max().unwrap_or(1);
    let size = egui::vec2(widest as f32 * (W + GX) - GX, rows as f32 * (H + GY) - GY + TOP);
    let (area, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let boxes: Vec<egui::Rect> = (0..all.len())
        .map(|k| {
            let l = levels[k];
            let row = first_row[l] + index_in_level[k] / max_cols;
            let col = index_in_level[k] % max_cols;
            let left = (widest - cols_in_row(row)) as f32 * (W + GX) / 2.0 + col as f32 * (W + GX);
            egui::Rect::from_min_size(area.min + egui::vec2(left, TOP + row as f32 * (H + GY)), egui::vec2(W, H))
        })
        .collect();
    let line = ui.visuals().widgets.noninteractive.fg_stroke.color;
    for (k, item) in all.iter().enumerate() {
        for p in requirements(path, item) {
            if let Some(j) = all.iter().position(|i| i.id == p.id) {
                ui.painter().line_segment([boxes[j].center_bottom(), boxes[k].center_top()], egui::Stroke::new(1.0, line));
            }
        }
    }
    let mut action = None;
    // the one "次のおすすめ" the line above the map names
    let recommended = next_item(path, progress).map(|i| i.id);
    for (k, item) in all.iter().enumerate() {
        let is_recommended = recommended == Some(item.id) && !item_done(progress, item);
        let (fill, stroke, text) = if item.kind == Kind::Planned {
            (Color32::from_gray(60), Color32::from_gray(90), Color32::from_gray(150))
        } else if item_done(progress, item) {
            (Color32::from_rgb(40, 100, 60), crate::theme::OK, Color32::WHITE)
        } else if next_ids.contains(&item.id) {
            (Color32::from_gray(45), crate::theme::WARN, Color32::WHITE)
        } else {
            (Color32::from_gray(45), Color32::from_gray(120), Color32::from_gray(210))
        };
        let response = ui.interact(boxes[k], ui.id().with(("tree_item", item.id)), egui::Sense::click());
        let painter = ui.painter().with_clip_rect(boxes[k]);
        painter.rect_filled(boxes[k], 4.0, fill);
        let width = if is_recommended { 3.5 } else if next_ids.contains(&item.id) { 2.0 } else { 1.0 };
        painter.rect_stroke(boxes[k], 4.0, egui::Stroke::new(width, if is_recommended { crate::theme::HIGHLIGHT } else { stroke }), egui::StrokeKind::Inside);
        let galley = painter.layout(i18n::tr(item.title).to_string(), egui::FontId::proportional(14.0), text, W - 10.0);
        painter.galley(boxes[k].min + egui::vec2(5.0, 4.0), galley, text);
        if is_recommended {
            // a badge on the top edge: this is the one to take next
            let badge = egui::Rect::from_min_size(boxes[k].right_top() + egui::vec2(-80.0, -19.0), egui::vec2(78.0, 18.0));
            ui.painter().rect_filled(badge, 9.0, crate::theme::HIGHLIGHT);
            ui.painter().text(badge.center(), egui::Align2::CENTER_CENTER, i18n::tr("★ 次はこれ"), egui::FontId::proportional(12.0), Color32::BLACK);
        }
        let response = response.on_hover_text(if item.kind == Kind::Planned { format!("{}\n{}", i18n::tr(item.about), i18n::tr("(準備中)")) } else { i18n::tr(item.about).to_string() });
        if response.clicked() {
            action = item_action(item);
        }
    }
    action
}
