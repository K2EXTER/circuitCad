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
    Colors,
}

/// The sizes of the screen offered (a free number would make the window jump under the pointer while it is dragged).
const SCALES: &[f32] = &[0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0];
/// How often to keep a copy of what is not saved, seconds (0 = never).
const AUTOSAVES: &[(u32, &str)] = &[(30, "30 秒"), (60, "1 分"), (120, "2 分"), (300, "5 分"), (600, "10 分"), (0, "しない")];

impl App {
    /// The settings as the app has them now (some live in the editors or in globals, not in `self.prefs`).
    fn current_prefs(&self) -> Prefs {
        Prefs { icon_only: icons::icon_only(), show_hints: self.show_hints, smooth_edges: self.smooth_edges, link_selection: self.link_selection, lang: i18n::lang().code().to_string(), flow_dots: crate::flow_dots::on(), flow_speed: crate::flow_dots::speed(), panel_light: crate::theme::is_light(), ..self.prefs.clone() }
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
        crate::canvas_theme::set_palette(crate::canvas_theme::build(crate::canvas_theme::Theme::parse(&p.canvas_theme).unwrap_or_default(), &p.canvas_colors));
        self.sch.default_author = p.default_author.clone();
        self.sch.analysis_pins = p.analysis_pins.clone();
        self.sch.wire_hops = p.wire_hops;
        crate::flow_dots::set_on(p.flow_dots);
        crate::flow_dots::set_speed(p.flow_speed);
        if crate::theme::is_light() != p.panel_light {
            crate::theme::set_light(p.panel_light);
            crate::startup::apply_panel_theme(ctx);
        }
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
                    (SettingsTab::Colors, "色", "回路図のキャンバスの色 (暗い地 / 紙) と、役ごとの色"),
                ],
            );
            ui.add_space(4.0);
            egui::Grid::new("settings_grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| match tab {
                SettingsTab::Look => look_tab(ui, &mut p, restart_note),
                SettingsTab::Input => input_tab(ui, &mut p),
                SettingsTab::Files => files_tab(ui, &mut p),
                SettingsTab::Board => board_tab(ui, &mut p),
                SettingsTab::Colors => colors_tab(ui, &mut p),
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
            canvas_theme: p.canvas_theme.clone(),
            canvas_colors: p.canvas_colors.clone(),
            wire_hops: p.wire_hops,
            flow_dots: p.flow_dots,
            flow_speed: p.flow_speed,
            panel_light: p.panel_light,
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
    ui.label(i18n::tr("Language / 言語"));
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
        crate::widgets::unless_ui(ui, (!p.show_hints).then_some("「ヒントを出す」を入れると選べます"), |ui| {
            ui.checkbox(&mut p.show_tips, i18n::tr("豆知識を出す")).on_hover_text(i18n::tr("画面下の右に、12 秒ごとに入れ替わる豆知識を表示します。慣れてきて気が散るときは切ります"));
        });
    });
    ui.end_row();
    ui.label(i18n::tr("ツール"));
    ui.checkbox(&mut p.icon_only, i18n::tr("アイコンだけで表示")).on_hover_text(i18n::tr("ツールボタンの文字を省き、アイコンだけにします。ボタンにカーソルを重ねると名前と説明が出ます。慣れてきたら画面が広く使えます"));
    ui.end_row();
    ui.label(i18n::tr("描き方"));
    let note = if restart_note { i18n::tr(" (次の起動から)") } else { "" };
    ui.vertical(|ui| {
    ui.checkbox(&mut p.wire_hops, i18n::tr("回路図で、つながっていない線の交差を飛び越しで描く")).on_hover_text(i18n::tr("横の線が縦の線をまたぐ所を、小さな半円の山で描きます。つながっている所 (黒い丸) と見分けやすくなります。オフにすると、ただの十字で描きます (つながっていないのは同じです)"));
    crate::flow_dots::settings_rows(ui, &mut p.flow_dots, &mut p.flow_speed);
    ui.checkbox(&mut p.smooth_edges, i18n::trf!("線の縁をなめらかにする{note}")).on_hover_text(i18n::tr("画面の描画を4倍のマルチサンプリング(MSAA)にして、配線やシルクの縁をなめらかに見せます。オンのほうが、描く三角形が少なく軽くなります。起動のときに決まるので、変えると次の起動から効きます。画面が崩れる、極端に重いなど、グラフィックスカードとの相性が悪いときはオフにしてください (起動のときに --no-msaa を付けても、その回だけオフになります)"));
    });
    ui.end_row();
}

/// The canvas's theme and the colour of each role. A colour that is hard to see on the ground is marked ▲; each
/// changed colour has a button that puts the theme's back. The pictures written out (SVG, PDF) are always on paper.
fn colors_tab(ui: &mut egui::Ui, p: &mut Prefs) {
    use crate::canvas_theme::{build, contrast, hard_to_see, Role, Theme, ROLES};
    ui.label(i18n::tr("パネルと窓")).on_hover_text(i18n::tr("左右のパネル・窓・メニューの地の明るさです。回路図や基板を描く所 (キャンバス) の色は、下で別に選びます"));
    ui.horizontal(|ui| {
        for (light, name, hint) in [(false, "暗い", "暗い灰色の地に明るい文字。長い時間でも目が疲れにくい"), (true, "明るい", "白い地に黒い文字。明るい部屋や、プロジェクターで見せるとき")] {
            if ui.selectable_label(p.panel_light == light, i18n::tr(name)).on_hover_text(i18n::tr(hint)).clicked() {
                p.panel_light = light;
            }
        }
    });
    ui.end_row();
    let theme = Theme::parse(&p.canvas_theme).unwrap_or_default();
    ui.label(i18n::tr("キャンバス")).on_hover_text(i18n::tr("回路図を描く所の色です。書き出す絵 (SVG・PDF) は、どちらでもいつも紙の色です"));
    ui.horizontal(|ui| {
        for (t, name, hint) in [(Theme::Dark, "暗い地", "暗い地に明るい灰色の部品。色は解析の結果 (電圧・電流・まちがい) にだけ使います"), (Theme::Paper, "紙", "印刷した回路図と同じ、白い紙の色")] {
            if ui.selectable_label(theme == t, i18n::tr(name)).on_hover_text(i18n::tr(hint)).clicked() && theme != t {
                p.canvas_theme = t.key().to_string();
            }
        }
    });
    ui.end_row();
    let pal = build(theme, &p.canvas_colors);
    for (role, key, name, what) in ROLES {
        let mut c = pal.get(*role);
        ui.label(i18n::tr(name)).on_hover_text(i18n::tr(what));
        ui.horizontal(|ui| {
            if ui.color_edit_button_srgba(&mut c).on_hover_text(i18n::tr("押して色を選びます")).changed() {
                let [r, g, b, _] = c.to_array();
                p.canvas_colors.retain(|(k, _)| k != key);
                p.canvas_colors.push((key.to_string(), [r, g, b]));
            }
            if hard_to_see(&pal, *role) {
                ui.colored_label(crate::theme::warn(), "▲").on_hover_text(i18n::trf!("地との差が小さく、見えにくい色です (コントラスト {:.1} : 1。線や印は 3 : 1 以上が目安)", contrast(c, pal.get(Role::Bg))));
            }
            if p.canvas_colors.iter().any(|(k, _)| k == key) && ui.small_button(i18n::tr("戻す")).on_hover_text(i18n::tr("このテーマの色に戻します")).clicked() {
                p.canvas_colors.retain(|(k, _)| k != key);
            }
        });
        ui.end_row();
    }
    ui.label("");
    if crate::widgets::unless(ui, p.canvas_colors.is_empty().then_some("テーマから変えた色はありません"), egui::Button::new(i18n::tr("すべての色をテーマに戻す"))).on_hover_text(i18n::tr("変えた色をすべて、選んでいるテーマの色に戻します")).on_disabled_hover_text(i18n::tr("色はまだ変えていません")).clicked() {
        p.canvas_colors.clear();
    }
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
