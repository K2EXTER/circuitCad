//! The settings window (設定 → 設定…): what is kept between launches in `settings.json` (`prefs.rs`), in four tabs.
//! A change takes effect at once and is saved at once; only the smooth edges wait for the next start.

use super::*;
use crate::prefs::Prefs;

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum SettingsTab {
    #[default]
    Look,
    Input,
    Files,
    Board,
}

/// The sizes of the screen offered (a free number would make the window jump under the pointer while it is dragged).
const SCALES: &[f32] = &[0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0];
/// How often to keep a copy of what is not saved, seconds (0 = never).
const AUTOSAVES: &[(u32, &str)] = &[(30, "30 秒"), (60, "1 分"), (120, "2 分"), (300, "5 分"), (600, "10 分"), (0, "しない")];

impl App {
    /// The settings as the app has them now (some live in the editors or in globals, not in `self.prefs`).
    fn current_prefs(&self) -> Prefs {
        Prefs { icon_only: icons::icon_only(), show_hints: self.show_hints, smooth_edges: self.smooth_edges, link_selection: self.link_selection, lang: i18n::lang().code().to_string(), ..self.prefs.clone() }
    }

    /// Puts the settings into effect (at start-up and after a change in the window). The language is not set here: at start-up the
    /// command line may choose another one.
    pub(super) fn apply_prefs(&mut self, ctx: &egui::Context, p: &Prefs) {
        self.show_hints = p.show_hints;
        self.smooth_edges = p.smooth_edges;
        self.link_selection = p.link_selection;
        icons::set_icon_only(p.icon_only);
        ctx.set_zoom_factor(p.ui_scale);
        prefs::set_wheel(p.wheel_speed, p.wheel_invert);
        self.sch.default_author = p.default_author.clone();
        self.prefs = p.clone();
    }

    pub(super) fn open_settings(&mut self) {
        self.settings_open = true;
    }

    pub(super) fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let before = self.current_prefs();
        let mut p = before.clone();
        let mut open = true;
        let mut tab = self.settings_tab;
        let restart_note = self.smooth_edges_running != p.smooth_edges;
        egui::Window::new(i18n::tr("設定")).open(&mut open).default_pos(crate::widgets::beside_panel()).default_width(420.0).resizable(false).show(ctx, |ui| {
            // the same width for every tab, so the window does not change size when the tab does
            ui.set_min_width(420.0);
            crate::widgets::tab_bar(
                ui,
                &mut tab,
                &[
                    (SettingsTab::Look, "表示", "言語・画面の大きさ・ヒントと豆知識・アイコン・線の縁"),
                    (SettingsTab::Input, "操作", "マウスのホイールでの拡大・縮小と、回路図・基板・3D の連動"),
                    (SettingsTab::Files, "ファイル", "自動保存と、図枠の作者"),
                    (SettingsTab::Board, "基板", "基板エディタの初めの値"),
                ],
            );
            ui.add_space(4.0);
            egui::Grid::new("settings_grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| match tab {
                SettingsTab::Look => look_tab(ui, &mut p, restart_note),
                SettingsTab::Input => input_tab(ui, &mut p),
                SettingsTab::Files => files_tab(ui, &mut p),
                SettingsTab::Board => board_tab(ui, &mut p),
            });
            if tab == SettingsTab::Input {
                ui.separator();
                if ui.button(i18n::tr("シミュレーション設定…")).on_hover_text(i18n::tr("計算の精度や収束のしやすさを調整する窓を開きます")).clicked() {
                    self.sch.show_sim_opts = true;
                }
            }
        });
        self.settings_open = open;
        self.settings_tab = tab;
        if p == before {
            return;
        }
        if p.lang != before.lang
            && let Some(l) = i18n::Lang::parse(&p.lang)
        {
            i18n::set_lang(l);
            install_japanese_font(ctx);
        }
        if p.board_grid != before.board_grid {
            self.board_ed.grid = p.board_grid;
        }
        self.apply_prefs(ctx, &p);
        // only what this window owns is written: the other keys (the requirement frames, the model zips) keep what the file has
        let file = Prefs::default_file();
        let saved = Prefs {
            icon_only: p.icon_only,
            show_hints: p.show_hints,
            smooth_edges: p.smooth_edges,
            link_selection: p.link_selection,
            lang: p.lang.clone(),
            ui_scale: p.ui_scale,
            show_tips: p.show_tips,
            autosave_secs: p.autosave_secs,
            wheel_speed: p.wheel_speed,
            wheel_invert: p.wheel_invert,
            default_author: p.default_author.clone(),
            board_grid: p.board_grid,
            ..Prefs::load(&file)
        }
        .save(&file);
        if let Err(e) = saved {
            self.sch.status = e;
        }
    }
}

fn look_tab(ui: &mut egui::Ui, p: &mut Prefs, restart_note: bool) {
    // written in both languages on purpose, so it can be found whichever one is showing
    ui.label("Language / 言語");
    ui.horizontal(|ui| {
        for (l, name) in [(i18n::Lang::Ja, "日本語"), (i18n::Lang::En, "English")] {
            if ui.selectable_label(p.lang == l.code(), name).on_hover_text(i18n::tr("画面の言語 / The language of the screen (English is partly translated: untranslated text stays in Japanese)")).clicked() {
                p.lang = l.code().to_string();
            }
        }
    });
    ui.end_row();
    ui.label(i18n::tr("画面の大きさ")).on_hover_text(i18n::tr("文字・ボタン・窓など、画面のすべての大きさです。文字が小さくて読みにくいとき、高解像度の画面で小さく見えるときに大きくします。Ctrl を押しながら + / − でも変えられます (その回だけ)"));
    egui::ComboBox::from_id_salt("ui_scale").selected_text(format!("{:.0} %", p.ui_scale * 100.0)).show_ui(ui, |ui| {
        for &s in SCALES {
            ui.selectable_value(&mut p.ui_scale, s, format!("{:.0} %", s * 100.0));
        }
    });
    ui.end_row();
    ui.label(i18n::tr("画面の下"));
    ui.vertical(|ui| {
        ui.checkbox(&mut p.show_hints, i18n::tr("ヒントを出す")).on_hover_text(i18n::tr("画面下に、いま何をするとよいかのヒントを表示します"));
        ui.add_enabled_ui(p.show_hints, |ui| {
            ui.checkbox(&mut p.show_tips, i18n::tr("豆知識を出す")).on_hover_text(i18n::tr("画面下の右に、12 秒ごとに入れ替わる豆知識を表示します。慣れてきて気が散るときは切ります"));
        });
    });
    ui.end_row();
    ui.label(i18n::tr("ツール"));
    ui.checkbox(&mut p.icon_only, i18n::tr("アイコンだけで表示")).on_hover_text(i18n::tr("ツールボタンの文字を省き、アイコンだけにします。ボタンにカーソルを重ねると名前と説明が出ます。慣れてきたら画面が広く使えます"));
    ui.end_row();
    ui.label(i18n::tr("描き方"));
    let note = if restart_note { i18n::tr(" (次の起動から)") } else { "" };
    ui.checkbox(&mut p.smooth_edges, i18n::trf!("線の縁をなめらかにする{note}")).on_hover_text(i18n::tr("画面の描画を4倍のマルチサンプリング(MSAA)にして、配線やシルクの縁をなめらかに見せます。オンのほうが、描く三角形が少なく軽くなります。起動のときに決まるので、変えると次の起動から効きます。画面が崩れる、極端に重いなど、グラフィックスカードとの相性が悪いときはオフにしてください (起動のときに --no-msaa を付けても、その回だけオフになります)"));
    ui.end_row();
}

fn input_tab(ui: &mut egui::Ui, p: &mut Prefs) {
    ui.label(i18n::tr("ホイールの拡大")).on_hover_text(i18n::tr("回路図・基板・3D・波形で、マウスのホイールを回したときの拡大・縮小の速さです"));
    ui.add(egui::Slider::new(&mut p.wheel_speed, 0.25..=4.0).logarithmic(true).fixed_decimals(2).suffix(" ×")).on_hover_text(i18n::tr("1 が初めの速さです。タッチパッドで速すぎるときは小さく、ホイールで遅いときは大きくします"));
    ui.end_row();
    ui.label("");
    ui.checkbox(&mut p.wheel_invert, i18n::tr("向きを逆にする")).on_hover_text(i18n::tr("ホイールを奥に回したときに縮小、手前に回したときに拡大にします (ほかの CAD に合わせたいとき)"));
    ui.end_row();
    ui.label(i18n::tr("連動"));
    ui.checkbox(&mut p.link_selection, i18n::tr("選んだ部品・ネットを回路図・基板・3Dで連動")).on_hover_text(i18n::tr("回路図で選んだ部品やネット (配線・ネットラベル) が、基板と3D表示でも光り、基板や3Dで選んだものが、回路図でも光ります (水色の枠と線)。画面を切り替えたときに、同じものをすぐ探せます。オフにすると、それぞれ別々に選びます"));
    ui.end_row();
}

fn files_tab(ui: &mut egui::Ui, p: &mut Prefs) {
    ui.label(i18n::tr("自動保存")).on_hover_text(i18n::tr("保存していない回路図・基板の写しを、この間隔で取っておきます。アプリが落ちたとき、次の起動で「復旧しますか?」と聞きます。元のファイルは書き換えません"));
    let shown = AUTOSAVES.iter().find(|(s, _)| *s == p.autosave_secs).map_or_else(|| i18n::trf!("{} 秒", p.autosave_secs).to_string(), |(_, n)| i18n::tr(n).to_string());
    egui::ComboBox::from_id_salt("autosave").selected_text(shown).show_ui(ui, |ui| {
        for (s, name) in AUTOSAVES {
            ui.selectable_value(&mut p.autosave_secs, *s, i18n::tr(name));
        }
    });
    ui.end_row();
    ui.label(i18n::tr("図枠の作者")).on_hover_text(i18n::tr("「ファイル → 図枠」を開いたとき、作者の欄が空なら、この名前が入ります。書き出した回路図の右下の枠に出ます"));
    ui.add(egui::TextEdit::singleline(&mut p.default_author).desired_width(200.0).hint_text(i18n::tr("あなたの名前")));
    ui.end_row();
}

fn board_tab(ui: &mut egui::Ui, p: &mut Prefs) {
    ui.label(i18n::tr("グリッド")).on_hover_text(i18n::tr("基板エディタでカーソルが吸い付く間隔の、起動したときの値です。変えると、いまの基板にもすぐ効きます (作業中に変えるのは「手で描く」タブの「作業の設定」で)"));
    ui.horizontal(|ui| {
        ui.add(egui::DragValue::new(&mut p.board_grid).speed(0.05).range(0.0..=5.0).suffix(" mm"));
        for g in [0.5, 1.27, 2.54] {
            if ui.small_button(format!("{g}")).on_hover_text(i18n::tr("よく使う間隔。0.5 mm は表面実装、1.27 mm と 2.54 mm はリード部品の足の間隔です")).clicked() {
                p.board_grid = g;
            }
        }
    });
    ui.end_row();
}
