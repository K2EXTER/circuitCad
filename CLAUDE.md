# circuitCad — コードマップ

目的: 回路図 → SPICEシミュレーション → 基板 → ガーバー出力を1本のソフトで。初学者向け(日本語UI、ホバー説明、ヒント、サンプル)。
機能を足すときは「GUI表示 + サンプル + 説明文」までセットで作る。UIの文言は日本語、コード・コメントは英語。

作業前に全ファイルを読まず、下の表で目的のファイルを特定してから読むこと。各ファイルの詳しい説明(数式・しきい値・設計判断・ファイルごとの一覧)は `docs/code-notes/` にある。必要な所だけ読む。

```
cargo test --workspace        # 全テスト (CIと同じ)
python tools/test_changed.py  # 変わったクレートと、それを使うクレートだけ test + clippy (コミット前。--list で選んだ理由)
python tools/test_times.py    # 1件ずつの時間と 2 秒を超えたテスト
cargo clippy --workspace --all-targets   # CIはエラーを禁止 (警告は可)
cargo run -p cli -- script.ccad   # 画面なしで実行 (docs/cli.md)
cargo run -p app              # 起動 (引数で .schjson / .pcbjson / .ccproj を開ける: args.rs apply_args)
```

## クレート (依存: app → pcb-core, sch-core, sim-core, part-lib, datasheet) — 詳細: `docs/code-notes/crates.md`

| クレート | 役割 |
|---|---|
| `sch-core` | 回路図モデル(`Schematic`/`Component`/`Wire`/`Label`/`Kind`)、ネット抽出と ERC `netlist.rs`(指摘は `Problem{message,at,hint,severity}`、hintに原因と直し方)、SPICE出力 `spice.rs`・`ngspice.rs`、`backannotate.rs`、`bus.rs`、`programming.rs`、階層 `hierarchy.rs`(`docs/hierarchy-design.md`) |
| `sim-core` | GUI非依存のシミュレータ。`from_schematic.rs` で回路図→`Circuit` |
| `design-spec` | 設計の要件の表の型(`DesignSpec`)。回路図 `Schematic.requirements` と基板 `Board.place_groups`/`keep_apart` が共有 |
| `pcb-core` | 基板モデル `model.rs`、DRC、ラッツネスト、自動配線、Gerber/Excellon/BOM出力 |
| `part-lib` | 部品JSON(`*.part.json`)、`examples/` が同梱部品。メーカーのシリーズ(値ごとの部品に展開。`series.rs`、`series/*.series.json`)。KiCad の記号・フットプリント・回路図の取り込み(`kicad*.rs`)、LTspice の回路図の取り込み(`ltspice*.rs`)、データシートの読み(`fits/*.fit.json`)から等価モデルを当てはめる `fit/`(`ccad fit`)、チップ部品・ダイオード・リード付き部品の計算、メーカーのSPICEモデルzipの索引 |
| `datasheet` | データシート(PDF)の表示・検索・見出し・型番の候補・表からのピン突き合わせ・グラフの軸と点 `graph.rs`(`docs/datasheet-viewer-design.md`) |
| `i18n` | 英語の画面。日本語の文を鍵にした表 `en/*.tsv`、`tr` / `trf!` / `tr_dyn`、`tools/i18n.py`(包む・未訳の一覧)。`docs/code-notes/i18n.md` |
| `app` | egui のGUI。回路図エディタ・基板エディタ・波形表示 |
| `cli` | `ccad`: 画面なしの操作窓口(1行1コマンドのスクリプト。`ccad mcp` で MCP サーバー。`board/` は基板の命令を話題ごとのファイルに分けてある。`docs/cli.md`、`docs/code-notes/cli.md`) |

## sim-core (`crates/sim-core/src`) — 詳細: `docs/code-notes/sim-core.md`(目次。ファイル一覧は `sim-core/files.md`)
- 動作点 `dc/`(build・devices・mna・newton・linalg・digital)/ 過渡 `tran.rs` / `ac.rs` / `noise.rs` / `dcsweep.rs` / `spectrum.rs`(FFT・THD)/ `temperature.rs`
- 回路図→素子 `from_schematic.rs`(新しい部品種別はここ)/ `spice_in.rs` ライブラリ読込 / `spice_flat.rs` ベンダー形式のSPICE文の展開 / `expr.rs` 式の評価 / `netcurrent.rs` ネットごとの電流 / `dc/seq.rs` 記憶つき素子 / `waveform.rs` / `value.rs` / `options.rs` / `par.rs` 並列 / `diagnose.rs` 失敗時の原因
- 解析: `paramsweep` `montecarlo` `worstcase` `sensitivity` `psrr` `troubleshoot` `loopgain` `polezero` `impedance` `measure` `stepmetrics` `power` `spec` `ratings` `ripple` `fault` `chain` `coverage`
- 熱: `selfheat.rs` 自己発熱 / `heat_tran.rs` 温度の時間変化
- `designcalc.rs` 値の計算(足すときは `schematic/wizard.rs` の `CALCS` と `cli/src/design.rs` の `CALCS` にも)
- `tests_*.rs` は機能別

## pcb-core (`crates/pcb-core/src`) — 詳細: `docs/code-notes/pcb-core.md`(目次。ファイル一覧は `pcb-core/output.md` の末尾)
- モデル `model.rs`(層は `Side`、`NetClass`、`Rules`)/ フットプリント `footprints.rs` `package.rs` `parts.rs` / 回路図取込 `netlist.rs`
- `drc/`(clearance・connectivity・path・pour・decoupling・thermal・silk・acute・area・slit・patch・geo …)/ `current.rs` `irdrop.rs` `nettable.rs`(ネットの一覧) `keepout.rs`(禁止領域) `impedance.rs`(式)`fieldsolve/`(断面の電界ソルバ。4分木のメッシュ `quad.rs`) `diffpair.rs` `requirements.rs`
- 配置: `autoplace/`(order・blocks・ics・finish) `align.rs` `connectors.rs` `orient.rs` `swap.rs` `nudge.rs`(配線後に近くの部品を動かして引き直す)`renumber.rs`(部品番号を位置順に振り直す)`snippet.rs`(配置・配線のスニペット)`replicas.rs`(繰り返し構造の検出)
- 熱: `heat_copper.rs`(ピンにつながった銅から部品の θ を見積もる。回路図に反映で自己発熱へ)
- 銅・形: `zonefill.rs`(ベタの銅)`copper.rs`(配線後に足す銅)`groundpour.rs`(GNDベタ)`stitch.rs`(ビアの縫い付け)`fiducial.rs`(フィデューシャル)`outline.rs` `shape.rs` `geom.rs` `pinlabels.rs`
- 配線: `autoroute.rs` `shove.rs` `ratsnest.rs` `reroute.rs` `merge.rs` `breakout.rs` `bridge.rs` `trunk.rs` `slot.rs` `padentry.rs` `meander.rs` `smooth.rs` `quality.rs`
- 3D・ケース: `body.rs` `model3d.rs` `step.rs` `mesh.rs` `case.rs` `dxf.rs` `dxf_in.rs` `dimension.rs`(寸法線) / 絵と図: `svg.rs` `picture.rs`
- 出力: `fab.rs`(発注前チェック)`export.rs` `gerber.rs` `excellon.rs` `assembly.rs` `shops.rs` `gerber_view.rs`
- 修理の練習 `breakage.rs`、文字 `labels.rs`(部品番号をよける。7 か所+力)`strokefont.rs` `silkedit.rs`(シルクの文字・部品番号・線を手で直す。発注先の下限を守る)

## app (`crates/app/src`) — 詳細: `docs/code-notes/app.md`(目次。ファイル一覧は `app/files.md`)
| ファイル | 内容 |
|---|---|
| `main.rs` / `prefs.rs` / `settings_window.rs` / `icons.rs` | `App`・メニュー / 設定の保存 / 設定の窓 / ツールのアイコン |
| `args.rs` `files.rs` `autosave.rs` `screenshot.rs` `startup.rs` `tips.rs` | 起動引数 / 保存・未保存の確認 / 自動保存と復旧 / 画面保存 / クラッシュログ・フォント / 豆知識 `TIPS` |
| `plot.rs` / `part_form/` | 波形描画 / 部品作成フォーム(`parse` 入力→部品 / `tabs` `kind` `chip` `template` `thru` `diode` 種類ごとの入力 / `pintable` `padtable` `pin_import` ピンとパッド / `symedit` 記号のピン配置をドラッグで / `step3d` / `datasheet_view` `pdf_*` データシートの窓 / `graph_reader` グラフから点を読む / `checklist` `trial` `vendor` `zchart` `preview`) |
| `glossary.rs` | 用語集(`TERMS` に語を足す) |
| `learn/`(`paths` パスの中身 / `window` 窓とウェルカムカード)`measure_guide.rs` `bench_guide.rs` `help.rs` `project.rs` | 学習パス / 組み立てて測る窓 / 実機作業の窓 / 操作ガイド(操作の説明はここ) / `.ccproj` |
| `theme.rs` `widgets.rs` | 色・余白・ボタン・窓の位置の決まり(`docs/code-notes/app/ui-rules.md`) |
| `samples.rs` `source_form.rs` `link.rs` `xcheck*.rs` | サンプル一覧(`docs/examples`)/ 電源値フォーム / 回路図・基板・3D の連動 / ngspice との検算 |
| `tests/` | 統合テスト `flow.rs` と、サンプル生成 `examples.rs` の `write_*`(`--ignored`) |

エディタは1つの型を複数ファイルの `impl` に分割している(子モジュールから親の非公開フィールドが見える)。

- `schematic/`(`SchEditor`): `mod.rs` 型 / `canvas` `draw` `interact` `symbols` 描画と操作 / `panel/`(`mod` ツール・ERC・フォーム / `parts` 部品一覧 / `properties` 選択部品の設定)`analysis_panel` `hint` 左パネル / `edit` `stretch` `history` `file` `clipboard` `snippet` `sheets_ui` `hierarchy_ui` `title_block`(図枠)`svg`・`pdf`(回路図の書き出し)`requirements`(設計の要件の窓)`bench_data`(実測を波形に重ねる)`repair_practice`(修理の練習の仮想テスター)`quick` `library` `settings` `glyphs` `erc_skip` `net_hints` `kicad_import` `result_tables` `linked`/ 各解析 `dc` `tran` `ac` `sweep` `noise` `wave` `fault` `param` `variation` `sensitivity` `psrr` `troubleshoot` `spec_table` `power_ui` `selfheat*` / 学習 `lessons/` `challenge/` `wizard.rs`
- `board_editor/`(`BoardEditor`): `mod.rs` 型 / `canvas` `interact` `draw` `panel` / `select` `route` `tools` `ribbon` `padlead` `livepath` `tracking` `shoving` / `align` `lengths` `pairs` `layers` `netclass` `netwindow` `nettable`(ネットの一覧の窓。電流と推奨の線幅も) `fieldview`(断面と電界の窓。インピーダンス) `zoneedit` `repair` `ruler` `measure`(距離を測るツール)`dimensions`(寸法線)`livedrc`(作業中の点検)`keepouts`(禁止領域のツール)`snippet``smoothing` `linked` `netglow` `width` `order` / `file` `actions` `history` `view` / `view3d/`(camera・scene・glow)と `raster3d.rs`(3D)/ `requirements.rs`

## 「〜を変えたい」早見表
- 新しい解析を足す → `sim-core/<名前>.rs` + `schematic/<名前>.rs` (実行+パネル) + `schematic/panel/mod.rs` の `ANALYSIS_SECTIONS` にボタン・`ANALYSIS_GROUPS` に鍵・`analysis_has_result` + `mod.rs` の `WaveMode` と `wave.rs` + `docs/examples/` にサンプル
- 新しい部品種別(基本素子) → `sch-core/lib.rs` の `Kind`、`symbols.rs`、`sim-core/from_schematic.rs`
- ライブラリ部品を足す → `part-lib/examples/*.part.json` (形式は `docs/part-format.md`)
- レッスン → `schematic/lessons/` に1件の `const` を足して `mod.rs` の `LESSONS` に並べる / チャレンジ → `schematic/challenge/` に1件の `const` を足して `mod.rs` の `CHALLENGES` に並べる (答えが通るテストも) / 学習パス → `learn.rs` の `PATHS`
- 値の計算 → `sim-core/designcalc.rs` + `schematic/wizard.rs` の `CALCS`
- DRCルール → `pcb-core/drc/` / 設計ルール既定値 → `model.rs` の `Rules`
- 英訳 → 画面の日本語は `i18n::tr` / `trf!` で包み(`python tools/i18n.py wrap crates/app/src`)、訳を `crates/i18n/en/*.tsv` に足す
- ヒント文 → `schematic/hint.rs`, `board_editor/panel.rs` の `hint` / 豆知識 → `tips.rs` の `TIPS` / 操作の説明 → `help.rs`

## ドキュメント・サンプル
- `docs/code-notes/` コードの詳細メモ。`sim-core.md` `pcb-core.md` `app.md` は目次で、話題ごとのファイル(`sim-core/` `pcb-core/` `app/`)に分けてある。ほかに `samples.md` `cli.md` `part-lib.md` `crates.md`
- `docs/candidates.md` 機能の候補と優先順位(次に何を作るかの一覧。着手したら該当行を消す)
- `docs/competitor-comparison.md` 他の電気CADとの機能比較 / `docs/quality-bench.md` 配置・配線の質を前後で比べる道具(配置・自動配線を変えたら、前後で回して `problems` が 0 のままか確かめる) / `tools/sim_compare.py` シミュレータの答え・波形・速さを前後で比べる道具(sim-core を変えたら前後で回す。`.claude/skills/sim-compare`)
- `docs/routing-guidelines.md` 配線の考え方 / `docs/learning-path.md` 学習パス / `docs/board-editor.md` 基板の使い方 / `docs/library.md` 部品ライブラリ / `docs/part-format.md` 部品JSON形式
- `docs/model-policy.md` 同梱する部品のモデルの作り方(等価モデル・使ってよい情報・LTspice の許諾)・表示・誤差の表 / `docs/shop-api-design.md` 部品店の API(在庫・候補、J5)の計画
- `docs/examples/*.schjson` サンプル回路 (説明と生成テストは `docs/code-notes/samples.md`)

## 配布 — 詳細: `docs/release-process.md`
依存クレートを変えたら `python tools/gen_third_party_licenses.py` で `THIRD_PARTY_LICENSES.txt` を作り直す(CIが検査)。リリースはタグ `v*` で Windows 用の zip を作る。**タグを打つ前に `docs/release-notes/<タグ>.md` を置くこと**。READMEの画像は `tools/gen_screenshots.ps1`(英語の画面は `-Lang en` で `docs/images/en/` へ。README.en.md が使う)、操作録画は `tools/gen_demo_live.ps1` で作り直す。

## 規約
- テストの回し方と足し方の決まり(2 秒を超えたら直す・同じ形は表に 1 行・正しさは下の層で)は `docs/code-notes/tests.md`。
- ボタン・入力欄には `on_hover_text` で説明を付ける。エラーメッセージは原因と対処が分かる日本語で。
- 回路図の変更はフレームごとの比較(`schematic/history.rs`)でUndoに入る。`self.sch` を直接変えれば自動で履歴になる。基板は変更前に `checkpoint()` を呼ぶ。
- 自動配線・自動再計算のように「結果を採用する前に検証する」処理は、失敗時に元の状態を変えないこと。
- 新しい機能を足したら、詳細を `docs/code-notes/` の該当する話題のファイルに書き(1ファイルが大きくなったら話題で分ける)、CLAUDE.md には地図の1行だけ足す(長い説明をここに戻さない)。
