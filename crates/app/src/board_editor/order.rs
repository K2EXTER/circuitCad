//! "発注の準備": pick the fab, run the pre-order check, look at the board as the fab will see it,
//! write the files and follow the steps for that fab.

use super::*;
use pcb_core::fab::{self, Check, FabPreset, Level};
use pcb_core::gerber_view::Preview;

/// State of the order window.
pub(super) struct OrderState {
    pub(super) open: bool,
    pub(super) fab_id: String,
    pub(super) checks: Vec<Check>,
    pub(super) ran: bool,
    pub(super) preview: Option<PreviewView>,
    /// Where the files were last written.
    pub(super) exported_to: Option<PathBuf>,
    /// How the board differs from the schematic (blocks the order or only warns), as last seen by `note_schematic_diff`.
    pub(super) sync: Option<(bool, String)>,
    /// The shopping list: how many boards to buy parts for, and whether chip parts get spares.
    pub(super) boards: usize,
    pub(super) spare: bool,
}

impl Default for OrderState {
    fn default() -> Self {
        Self { open: false, fab_id: "standard".into(), checks: vec![], ran: false, preview: None, exported_to: None, sync: None, boards: 1, spare: true }
    }
}

pub(super) struct PreviewView {
    pub(super) texture: egui::TextureHandle,
    pub(super) size_px: [usize; 2],
    pub(super) drills: Vec<(f64, usize, bool)>,
    pub(super) size_mm: (f64, f64),
    pub(super) bottom: bool,
    pub(super) mask: bool,
    pub(super) silk: bool,
    pub(super) zoom: f32,
}

/// What each exported file is, for a beginner reading the folder.
const FILE_KINDS: &[(&str, &str)] = &[
    (".GTL", "表面の銅箔 (配線とパッド)"),
    (".GBL", "裏面の銅箔"),
    (".GTS", "表面のソルダーマスク (緑のレジストの穴あけ)"),
    (".GBS", "裏面のソルダーマスク"),
    (".GTO", "表面のシルク (白い文字)"),
    (".GBO", "裏面のシルク"),
    (".GKO", "基板の外形"),
    ("-PTH.DRL", "メッキ穴 (部品の足・ビア)"),
    ("-NPTH.DRL", "メッキなしの穴 (取付穴)"),
    ("-BOM.csv", "部品表 (部品実装を頼むとき)"),
    ("-CPL.csv", "部品の位置 (部品実装を頼むとき)"),
    ("_gerber.zip", "上のガーバーとドリルをまとめた zip。発注先へはこれを渡します"),
];

impl BoardEditor {
    pub fn open_order(&mut self) {
        self.order.open = true;
        self.run_checks();
    }

    /// Remember how the board differs from the schematic, for the order window's checks (call every frame the window is up).
    pub fn note_schematic_diff(&mut self, sch: &SchEditor) {
        let d = self.schematic_diff(sch);
        self.order.sync = (!d.is_empty()).then(|| (d.blocking(), d.summary()));
    }

    #[cfg(test)]
    pub fn order_checks(&self) -> &[Check] {
        &self.order.checks
    }

    pub(super) fn selected_fab(&self) -> Option<&'static FabPreset> {
        fab::preset(&self.order.fab_id)
    }

    pub(super) fn run_checks(&mut self) {
        self.order.checks = fab::preflight(&self.board, self.selected_fab());
        if let Some((blocking, summary)) = &self.order.sync {
            self.order.checks.insert(0, fab::schematic_sync_check(summary, *blocking));
        }
        self.order.ran = true;
    }

    /// Use this fab's minimums for the DRC and as drawing defaults.
    pub(super) fn apply_fab(&mut self, f: &FabPreset) {
        self.checkpoint();
        f.apply_rules(&mut self.board.rules);
        self.track_width = f.track_width;
        self.via_dia = f.via_diameter;
        self.via_drill = f.via_drill;
        self.violations.clear();
        self.status = format!("{} のルールに合わせました (線幅 {} mm、ビア {} / {} mm)。DRC をやり直してください", f.name, f.track_width, f.via_diameter, f.via_drill);
    }

    pub(super) fn build_preview(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let files = export::fab_files(&self.board);
        let pv = Preview::from_files(&files, 1100)?;
        let img = egui::ColorImage::from_rgba_unmultiplied([pv.frame.w, pv.frame.h], &pv.render(false, true, true).iter().flatten().copied().collect::<Vec<u8>>());
        let texture = ctx.load_texture("order_preview", img, egui::TextureOptions::LINEAR);
        let (bottom, mask, silk, zoom) = self.order.preview.as_ref().map_or((false, true, true, 1.0), |p| (p.bottom, p.mask, p.silk, p.zoom));
        self.order.preview = Some(PreviewView { texture, size_px: [pv.frame.w, pv.frame.h], drills: pv.drills.clone(), size_mm: pv.size_mm, bottom, mask, silk, zoom });
        self.order_preview_src = Some(pv);
        self.repaint_preview(ctx);
        Ok(())
    }

    pub(super) fn repaint_preview(&mut self, ctx: &egui::Context) {
        let (Some(pv), Some(view)) = (self.order_preview_src.as_ref(), self.order.preview.as_mut()) else { return };
        let rgba: Vec<u8> = pv.render(view.bottom, view.mask, view.silk).iter().flatten().copied().collect();
        let img = egui::ColorImage::from_rgba_unmultiplied(view.size_px, &rgba);
        view.texture = ctx.load_texture("order_preview", img, egui::TextureOptions::LINEAR);
    }

    /// The floating "発注の準備" window.
    /// "部品の購入リスト": what to buy for the board, with the shops' links; a few spares of the tiny parts; a CSV to keep.
    fn shopping_list(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("部品の購入リスト (買うもの)").id_salt("shopping_list").show(ui, |ui| {
            ui.small("基板に載せる部品の一覧です。店のボタンで、その店の部品のページ (型番が分かるもの) か、検索のページ (分からないもの) が開きます。買う前に、店のページで値と大きさを確かめます");
            ui.horizontal(|ui| {
                ui.label("基板の枚数");
                ui.add(egui::DragValue::new(&mut self.order.boards).range(1..=1000)).on_hover_text("この枚数の基板に載せる分を買います");
                ui.checkbox(&mut self.order.spare, "予備を足す").on_hover_text("小さなチップ部品 (抵抗・コンデンサ・コイル・LED) は、落としたり飛ばしたりしやすいので、2個か全体の1割の多いほうを、多めに買います。ICやコネクタには足しません。目安です");
            });
            let (boards, spare) = (self.order.boards.max(1), self.order.spare);
            let lines = pcb_core::shops::lines(&self.board);
            if lines.is_empty() {
                ui.small("基板に部品がありません");
                return;
            }
            egui::Grid::new("shopping_grid").striped(true).show(ui, |ui| {
                for h in ["値", "形", "番号", "必要", "予備", "買う数", "店"] {
                    ui.small(h);
                }
                ui.end_row();
                for l in &lines {
                    let (need, extra) = (l.qty * boards, if spare { l.spare(boards) } else { 0 });
                    ui.label(&l.value);
                    ui.small(&l.footprint);
                    ui.small(l.refdes.join(" ")).on_hover_text("この部品が使う場所 (部品番号)");
                    ui.label(need.to_string());
                    ui.label(if extra > 0 { format!("+{extra}") } else { String::new() });
                    ui.strong((need + extra).to_string());
                    ui.horizontal(|ui| {
                        for (shop, s) in pcb_core::shops::SHOPS.iter().zip(&l.sources) {
                            let (text, tip) = match s {
                                pcb_core::shops::Source::Code { number, .. } => (format!("{} {number}", shop.name), "この店の部品のページを開きます (部品に登録された型番)"),
                                pcb_core::shops::Source::Search { .. } => (shop.name.to_string(), "この店の検索のページを開きます (型番が分からないので、値と大きさで探します)"),
                            };
                            ui.hyperlink_to(text, s.url()).on_hover_text(tip);
                        }
                    });
                    ui.end_row();
                }
            });
            if ui.button("CSVに保存…").on_hover_text("この一覧を CSV (表計算ソフトで開けます) に保存します。店のリンクと型番も入ります").clicked()
                && let Some(path) = rfd::FileDialog::new().add_filter("CSV", &["csv"]).set_file_name(format!("{}-shopping.csv", self.board.name)).save_file()
            {
                self.status = match std::fs::write(&path, pcb_core::shops::purchase_csv(&self.board, boards, spare)) {
                    Ok(()) => format!("購入リストを保存しました: {}", path.display()),
                    Err(e) => format!("購入リストを保存できませんでした: {e}。保存先に書き込めるか確認してください"),
                };
            }
        });
    }

    pub fn order_window(&mut self, ctx: &egui::Context) {
        if !self.order.open {
            return;
        }
        let mut open = true;
        let mut run = false;
        let mut apply = None;
        let mut jump = None;
        let mut export_now = false;
        let mut make_preview = false;
        let mut repaint = false;
        egui::Window::new("発注の準備").open(&mut open).default_width(520.0).default_height(640.0).vscroll(true).show(ctx, |ui| {
            // the small grey notes of this window are what a first-time orderer has to read: not the 9 px of the default
            ui.style_mut().text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
            ui.label("基板を発注する前の確認です。上から順に見ていきましょう。");
            ui.separator();
            ui.strong("1. 発注先を選ぶ");
            egui::ComboBox::from_id_salt("fab_pick")
                .selected_text(self.selected_fab().map_or("選んでください", |f| f.name))
                .width(300.0)
                .show_ui(ui, |ui| {
                    for p in fab::PRESETS {
                        if ui.selectable_value(&mut self.order.fab_id, p.id.to_string(), p.name).on_hover_text(p.summary).clicked() {
                            run = true;
                        }
                    }
                });
            if let Some(f) = self.selected_fab() {
                ui.small(f.summary);
                for n in f.notes {
                    ui.small(format!("・{n}"));
                }
                ui.horizontal(|ui| {
                    if ui.button("ルールをこの発注先に合わせる").on_hover_text("DRC の最小線幅・最小間隔・最小穴径などを、この発注先の値にします。配線の太さやビアの大きさの初期値も変わります").clicked() {
                        apply = Some(f);
                    }
                    if !f.url.is_empty() {
                        ui.hyperlink_to("発注先の最新の仕様を見る", f.url);
                    }
                });
                ui.small("数値は目安です。発注の前に、必ず発注先のページで最新の値を確認してください。");
            }

            ui.separator();
            ui.horizontal(|ui| {
                ui.strong("2. 発注前のチェック");
                if ui.button("もう一度チェック").on_hover_text("いまの基板でチェックをやり直します。直したあとに押します").clicked() {
                    run = true;
                }
            });
            let mut number = 0;
            for (idx, c) in self.order.checks.iter().enumerate() {
                let (mark, color) = match c.level {
                    Level::Ok => ("✔", Color32::from_rgb(60, 160, 90)),
                    Level::Info => ("ℹ", Color32::from_gray(150)),
                    Level::Warn => ("▲", Color32::from_rgb(210, 150, 20)),
                    Level::Error => ("✖", Color32::from_rgb(220, 40, 60)),
                };
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(color, mark);
                    if c.at.is_some() && c.level != Level::Ok {
                        number += 1;
                        // the same number is drawn on the board at that place
                        ui.colored_label(color, egui::RichText::new(format!("[{number}]")).strong());
                    }
                    let text = egui::RichText::new(&c.title);
                    if c.at.is_some() {
                        if ui.link(text).on_hover_text("クリックで、基板のその場所へ移動して、印をつけます。基板の上にも、同じ番号の印が出ています").clicked() {
                            jump = Some((idx, c.at));
                        }
                    } else {
                        ui.label(text);
                    }
                });
                if !c.detail.is_empty() && c.level != Level::Ok {
                    ui.indent(("detail", &c.title), |ui| {
                        // the explanation is the part to read: normal size, not the small print
                        ui.label(&c.detail);
                    });
                }
            }
            let ready = fab::ready(&self.order.checks);
            if self.order.ran {
                if ready {
                    ui.colored_label(Color32::from_rgb(60, 160, 90), "直すところはありません。次に進めます");
                } else {
                    ui.colored_label(Color32::from_rgb(220, 40, 60), "赤い ✖ の項目を直してから、もう一度チェックしてください");
                }
            }

            ui.separator();
            ui.strong("3. 出来上がりを見る");
            ui.small("書き出すファイルを読み戻して、基板の絵にします。発注先に届くのはこの形です。配線や穴が想定どおりか確認しましょう");
            if ui.button(if self.order.preview.is_some() { "プレビューを更新" } else { "プレビューを作る" }).on_hover_text("書き出すファイルを読み戻して、出来上がりの絵を作り直します").clicked() {
                make_preview = true;
            }
            if let Some(pv) = self.order.preview.as_mut() {
                ui.horizontal(|ui| {
                    repaint |= ui.selectable_value(&mut pv.bottom, false, "表面から").on_hover_text("基板の表面 (部品を載せる面) から見た絵にします").changed();
                    repaint |= ui.selectable_value(&mut pv.bottom, true, "裏面から").on_hover_text("基板を裏返して、裏面から見た絵にします").changed();
                    repaint |= ui.checkbox(&mut pv.mask, "ソルダーマスク").on_hover_text("緑のレジスト。オフにすると、銅の配線だけが見えます").changed();
                    repaint |= ui.checkbox(&mut pv.silk, "シルク").on_hover_text("シルク (白い印刷の文字と枠) の表示を切り替えます").changed();
                    ui.add(egui::Slider::new(&mut pv.zoom, 0.5..=3.0).text("拡大")).on_hover_text("プレビューの拡大率です");
                });
                let w = (ui.available_width() - 8.0).max(200.0) * pv.zoom;
                let size = Vec2::new(w, w * pv.size_px[1] as f32 / pv.size_px[0] as f32);
                // inside the window's own scroll area, egui shrinks this one to its minimum scrolled height (64 px) unless told otherwise
                egui::ScrollArea::both().max_height(380.0).min_scrolled_height(380.0).id_salt("preview_scroll").show(ui, |ui| {
                    ui.add(egui::Image::new(egui::load::SizedTexture::new(pv.texture.id(), size)));
                });
                ui.small(format!("基板 {:.1} × {:.1} mm。金色 = むき出しの銅 (はんだが付く所)、緑 = マスクをかぶった配線、黒 = 穴、白 = シルク", pv.size_mm.0, pv.size_mm.1));
                egui::CollapsingHeader::new("穴の一覧").show(ui, |ui| {
                    egui::Grid::new("drill_grid").striped(true).show(ui, |ui| {
                        ui.small("直径");
                        ui.small("個数");
                        ui.small("種類");
                        ui.end_row();
                        for (d, n, plated) in &pv.drills {
                            ui.small(format!("{d:.2} mm"));
                            ui.small(n.to_string());
                            ui.small(if *plated { "メッキ穴" } else { "メッキなし" });
                            ui.end_row();
                        }
                    });
                });
            }

            ui.separator();
            ui.strong("4. 製造ファイルを書き出す");
            ui.small("ガーバーは、基板の工場に渡す設計図のデータです (銅の層・シルク・外形・穴が、層ごとのファイルになります)。まとめて zip にします");
            ui.add_enabled_ui(ready, |ui| {
                if ui.button("書き出し先のフォルダを選んで書き出す").on_hover_text("ガーバー・ドリル・部品表・部品位置・zip をまとめて作ります").clicked() {
                    export_now = true;
                }
            });
            if !ready {
                ui.small("チェックの ✖ を直すと押せます");
            }
            if let Some(dir) = &self.order.exported_to {
                ui.colored_label(Color32::from_rgb(60, 160, 90), format!("書き出しました: {}", dir.display()));
                ui.colored_label(Color32::from_rgb(60, 160, 90), format!("発注先に渡すのは「{}_gerber.zip」の1つです (部品の実装も頼むときは、BOM と CPL の csv も)", self.board.name));
                egui::CollapsingHeader::new("ファイルの中身").show(ui, |ui| {
                    for (suffix, what) in FILE_KINDS {
                        ui.small(format!("{}{suffix}  …  {what}", self.board.name));
                    }
                });
                if let Some(f) = self.selected_fab() {
                    ui.strong(format!("5. {} で注文する", f.name));
                    for (k, step) in f.steps.iter().enumerate() {
                        ui.small(format!("{}. {}", k + 1, step.replace("基板名", &self.board.name)));
                    }
                }
            }
            ui.separator();
            self.shopping_list(ui);
        });
        if run {
            self.run_checks();
        }
        if let Some(f) = apply {
            self.apply_fab(f);
            self.run_checks();
        }
        if let Some((idx, Some(at))) = jump {
            self.pan = Vec2::new(at.x as f32, at.y as f32);
            // zoom in enough to see what the note is about (never zoom out)
            self.zoom = self.zoom.max(30.0);
            self.check_focus = Some(idx);
        }
        if make_preview {
            match self.build_preview(ctx) {
                Ok(()) => {}
                Err(e) => self.status = format!("プレビューを作れませんでした: {e}"),
            }
        } else if repaint {
            self.repaint_preview(ctx);
        }
        if export_now {
            self.export();
            if self.status.starts_with("書き出し完了") {
                self.order.exported_to = self.last_export_dir.clone();
            }
        }
        if !open {
            self.order.open = false;
        }
    }
}
