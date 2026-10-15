# circuitCad — コードマップ

目的: 回路図 → SPICEシミュレーション → 基板 → ガーバー出力を1本のソフトで。初学者向け(日本語UI、ホバー説明、ヒント、サンプル)。
機能を足すときは「GUI表示 + サンプル + 説明文」までセットで作る。UIの文言は日本語、コード・コメントは英語。

作業前に全ファイルを読まず、下の表で目的のファイルを特定してから読むこと。各ファイルの詳しい仕様(数式・しきい値・設計判断)は `docs/code-notes/` にある。必要な所だけ読む。

```
cargo test --workspace        # 全テスト (CIと同じ)
cargo clippy --workspace --all-targets   # CIはエラーを禁止 (警告は可)
cargo run -p app              # 起動 (引数で .schjson / .pcbjson / .ccproj を開ける: args.rs apply_args)
```

## クレート (依存: app → pcb-core, sch-core, sim-core, part-lib)

| クレート | 役割 |
|---|---|
| `sch-core` | 回路図モデル (`Schematic`/`Component`/`Wire`/`Label`/`Kind`)、ネット抽出 `netlist.rs` (`extract`, `net_at`, `erc` — 指摘は `Problem{message,at,hint,severity}`、hintに原因と直し方を書く)、SPICE出力 `spice.rs`、ngspice方言 `ngspice.rs` |
| `sim-core` | GUI非依存のシミュレータ。`from_schematic.rs` で回路図→`Circuit` |
| `pcb-core` | 基板モデル `model.rs`、DRC、ラッツネスト、自動配線、Gerber/Excellon/BOM出力 |
| `part-lib` | 部品JSON (`*.part.json`: 記号+パッケージ+SPICEモデル)。`examples/` が同梱部品。`kicad.rs` はKiCadの `.kicad_sym`/`.kicad_mod` を `PartDef` に変換、`passive.rs` はチップ部品のテンプレート、`diode.rs` はダイオードの IS/N/RS をグラフの点から決める計算、`thru_hole.rs` はリード付き部品の穴・パッド・ピッチの目安、`passive.rs` にはコイル・抵抗のインピーダンス当てはめと温度係数の当てはめも |
| `app` | egui のGUI。回路図エディタ・基板エディタ・波形表示 |

## sim-core (`crates/sim-core/src`) — 詳細: `docs/code-notes/sim-core.md`
- `dc/` 動作点 (`build.rs` 回路→コンパイル済み形 / `devices.rs` 素子の式 / `mna.rs` 行列 / `newton.rs` ニュートン反復 / `linalg.rs` 疎LU)
- `tran.rs` 過渡(刻みは自動調整) / `ac.rs` / `noise.rs` / `dcsweep.rs` / `spectrum.rs` FFT・統計 / `paramsweep.rs` パラメータスイープ / `montecarlo.rs` ばらつき解析 / `temperature.rs` 温度
- `from_schematic.rs` 部品→素子の変換 (新しい部品種別はここ) / `waveform.rs` 電源波形 / `value.rs` SPICE数値 / `spice_in.rs` ライブラリ部品の `.model`/`.subckt` 読込
- `options.rs` 計算精度 `SimOptions` / `diagnose.rs` 失敗時の原因と直し方
- `measure.rs` 波形の自動測定・安定余裕 / `stepmetrics.rs` ステップ応答の指標 / `power.rs` 電力と効率 / `spec.rs` スペック(合否の表)
- `ratings.rs` 定格チェック / `ripple.rs` 電解コンデンサのリップルと寿命
- `fault.rs` 故障シミュレーション / `chain.rs` 連鎖故障
- `designcalc.rs` 初学者向けの値の計算 (足すときは `schematic/wizard.rs` の `CALCS` にも)
- `tests_*.rs` は機能別 (555, logic, ac, noise, thermal ...)

## pcb-core (`crates/pcb-core/src`) — 詳細: `docs/code-notes/pcb-core.md`
- `model.rs` 全データ型 (層は `Side`、内層は `Board.inner_layers`) / `footprints.rs`・`package.rs` フットプリント / `parts.rs` `place_part` / `netlist.rs` 回路図→基板の取込
- `drc/` DRC (`mod.rs`=`check`・`clearance`・`shorts` / `connectivity.rs` 銅の接続グループ / `geo.rs` 距離計算 / `silk.rs` シルクとマスク帯 / `thermal.rs` サーマルビア / `pour.rs` ベタ / `decoupling.rs` バイパスコンデンサ / `acute.rs` 鋭角 / `tests.rs`) / `current.rs` 配線の太さと電流 / `irdrop.rs` 電圧降下 / `diffpair.rs` 差動ペア
- `smooth.rs` 配線の仕上げ(角丸め・涙型・分かれ目の凹み。`Board.finish` の設定で表示・出力・DRCのときに毎回計算)
- `ratsnest.rs` 未配線 / `autoroute.rs` 自動配線 / `shove.rs` 押しのけ配線
- `body.rs` 部品の本体外形 / `outline.rs` 曲線の外形 / `zonefill.rs` ベタの銅 / `model3d.rs` 3D用の立体
- `fab.rs` 発注先と発注前チェック / `export.rs` 製造一式 / `gerber.rs` / `excellon.rs` / `assembly.rs` BOM・配置CSV / `gerber_view.rs` 書き出しの読み戻し / `strokefont.rs` シルク文字

## app (`crates/app/src`) — 詳細: `docs/code-notes/app.md`
| ファイル | 内容 |
|---|---|
| `main.rs` | `App`・ウィンドウ・メニュー |
| `args.rs` / `files.rs` / `screenshot.rs` / `startup.rs` / `tips.rs` | 起動引数 / 保存・読込・未保存の確認 / 画面保存 / クラッシュログ・フォント / 豆知識 `TIPS` |
| `tests/` | 統合テスト `flow.rs` と、サンプル回路の生成 `examples.rs` の `write_*` (`--ignored` で実行) |
| `plot.rs` | 波形描画 (`scale_bar`・`draw_group`・測定カーソル) |
| `part_form/` | 部品作成フォーム (`parse` 入力→`PartDef` / `kind` 種類 / `tabs` 各タブ / `chip` チップ部品 / `template` 充電IC / `preview` / `checklist`) |
| `learn.rs` / `measure_guide.rs` / `help.rs` / `project.rs` | 学習パス(ウェルカムカードも) / 学習パス最後の「組み立てて測る」の窓 / 操作ガイド窓 (操作の説明はここに足す) / `.ccproj` |
| `samples.rs` | 「ファイル → サンプル回路を開く」の一覧 (`docs/examples` を埋め込む) |
| `source_form.rs` | 電源値フォーム |
| `xcheck.rs` / `xcheck_tran.rs` | 開発用: ngspice との検算 (`--xcheck`) |

エディタは1つの型を複数ファイルの `impl` に分割している (子モジュールから親の非公開フィールドが見える)。型・フィールドは各 `mod.rs`。

### `schematic/` (`SchEditor`) — ファイル一覧: `docs/code-notes/app.md`
`mod.rs` 型 / `canvas.rs`・`draw.rs`・`interact.rs`・`symbols.rs` 描画と操作 / `panel.rs`・`analysis_panel.rs`・`hint.rs` 左パネル / `edit.rs`・`history.rs`・`file.rs`・`library.rs` / `quick.rs` クイック追加 / `lessons/`(`mod.rs`=型と窓、`conditions.rs`=手順の自動チェック、`basics/devices/analog/supply.rs`=レッスンのデータ)・`challenge/`(`mod.rs`=型、`basic.rs`/`power.rs`=データ、`judge.rs`=判定、`window.rs`)・`wizard.rs` 学習 / `dc.rs`・`tran.rs`・`ac.rs`・`sweep.rs`・`noise.rs`・`wave.rs` 各解析 / `fault.rs`・`param.rs`・`variation.rs`・`spec_table.rs`・`power_ui.rs`・`settings.rs`

### `board_editor/` (`BoardEditor`)
`mod.rs` 型 / `padlead.rs` パッドの両端の吸着点と垂直な出入り / `livepath.rs` 自動経路(出発点からカーソルまでの探索) / `canvas.rs`・`interact.rs`・`draw.rs`・`panel.rs` / `select.rs`・`route.rs`・`tools.rs` / `view3d/`(`mod.rs`=状態、`window.rs`=窓、`scene.rs`=立体と描画、`camera.rs`、`picture.rs`、`glow.rs`)・`raster3d.rs` 3D / `width.rs` 配線の太さ・電圧降下 / `order.rs` 発注の準備 / `file.rs`・`actions.rs`・`history.rs`・`view.rs` / `layers.rs` 層 / `pairs.rs` 差動ペア / `shoving.rs` 押しのけ

## 「〜を変えたい」早見表
- 新しい解析を足す → `sim-core/<名前>.rs` + `schematic/<名前>.rs` (実行+パネル) + `schematic/panel.rs` にボタン + `mod.rs` の `WaveMode` と `wave.rs` + `docs/examples/` にサンプル
- 新しい部品種別(基本素子) → `sch-core/lib.rs` の `Kind`、`symbols.rs`、`sim-core/from_schematic.rs`
- ライブラリ部品を足す → `part-lib/examples/*.part.json` (形式は `docs/part-format.md`)
- レッスン → `schematic/lessons/` に1件の `const` を足して `mod.rs` の `LESSONS` に並べる / チャレンジ → `schematic/challenge/` に1件の `const` を足して `mod.rs` の `CHALLENGES` に並べる (答えが通るテストも) / 学習パス → `learn.rs` の `PATHS`
- 値の計算 → `sim-core/designcalc.rs` + `schematic/wizard.rs` の `CALCS`
- DRCルール → `pcb-core/drc/` / 設計ルール既定値 → `model.rs` の `Rules`
- ヒント文 → `schematic/hint.rs`, `board_editor/panel.rs` の `hint` / 豆知識 → `tips.rs` の `TIPS` / 操作の説明 → `help.rs`

## ドキュメント・サンプル
- `docs/code-notes/` コードの詳細メモ (`sim-core.md` `pcb-core.md` `app.md` `samples.md`)
- `docs/learning-path.md` 学習パス(安定化電源)の設計 / `docs/board-editor.md` 基板の使い方 / `docs/library.md` 部品ライブラリ / `docs/part-format.md` 部品JSON形式
- `docs/examples/*.schjson` サンプル回路 (各サンプルの説明と生成テストは `docs/code-notes/samples.md`)

## 配布
依存クレートを変えたら `python tools/gen_third_party_licenses.py` で `THIRD_PARTY_LICENSES.txt` を作り直す(CIが `--check` で古さを検査、リリースのzipに同梱)。`.github/workflows/release.yml` がタグ `v*` でWindows用のzipを作り、プレリリースとして公開する(macOS/Linuxは動作確認ができるまでmatrixでコメントアウト中)。日本語フォントは `startup.rs` の `install_japanese_font` がOSごとの候補パスから読み、無ければ `assets/fonts/BIZUDGothic-Regular.ttf`(OFL)を使う。

## 規約
- ボタン・入力欄には `on_hover_text` で説明を付ける。エラーメッセージは原因と対処が分かる日本語で。
- 回路図の変更はフレームごとの比較(`schematic/history.rs`)でUndoに入る。`self.sch` を直接変えれば自動で履歴になる。基板は変更前に `checkpoint()` を呼ぶ。
- 自動配線・自動再計算のように「結果を採用する前に検証する」処理は、失敗時に元の状態を変えないこと。
- 新しい機能を足したら、詳細を `docs/code-notes/` の該当ファイルに書き、CLAUDE.md には地図の1行だけ足す(長い説明をここに戻さない)。
