# app: ファイルごとの一覧(CLAUDE.md の地図の元の文)

(`docs/code-notes/app.md` の目次から分けた。内容は元のまま)

## ファイル一覧(app、schematic/、board_editor/)

(CLAUDE.md の地図から移した、ファイルごとの詳しい一覧。CLAUDE.md には1行ずつの要約だけ残してある)
| ファイル | 内容 |
|---|---|
| `main.rs` | `App`・ウィンドウ・メニュー |
| `prefs.rs` | 表示設定の保存 (`settings.json`) |
| `icons.rs` | ツールのアイコンと「アイコン+文字/アイコンのみ」切替 |
| `args.rs` / `files.rs` / `autosave.rs` / `screenshot.rs` / `startup.rs` / `tips.rs` | 起動引数 / 保存・読込・未保存の確認 / 自動保存と復旧(1分ごと) / 画面保存 / クラッシュログ・フォント / 豆知識 `TIPS` |
| `tests/` | 統合テスト `flow.rs` と、サンプル回路の生成 `examples.rs` の `write_*` (`--ignored` で実行) |
| `plot.rs` | 波形描画 (`scale_bar`・`draw_group`・測定カーソル) |
| `part_form/` | 部品作成フォーム (`parse` 入力→`PartDef` / `kind` 種類 / `tabs` 各タブ / `chip` チップ部品 / `template` 充電IC / `step3d` STEPの3Dの形 / `preview` / `checklist` / `datasheet_view` データシート(PDF)の窓) |
| `glossary.rs` | 用語集・索引 (「学ぶ」→「用語集・索引…」、`--glossary [語]`)。`TERMS` に語(名前・読み・別名・説明。回路の言葉とソフトの言葉)を足す。どのレッスンに出るかは、レッスンの本文から自動で探す(手で対応表は書かない)。レッスンの窓の「このレッスンの用語」にも使う |
| `learn/`(`mod.rs` 記録・型・進み具合の計算 / `paths.rs` 各パスの段階と項目 / `window.rs` 窓・樹形図・ウェルカムカード / `tests.rs`) / `measure_guide.rs` / `bench_guide.rs` / `help.rs` / `project.rs` | 学習パス(直線と樹形図。ウェルカムカードも) / 学習パス最後の「組み立てて測る」の窓 / 実機の作業のチェックリスト窓(はんだ付け・ブートローダ。`Kind::Hands`) / 操作ガイド窓 (操作の説明はここに足す) / `.ccproj` |
| `samples.rs` | 「ファイル → サンプル回路を開く」の一覧 (`docs/examples` を埋め込む) |
| `source_form.rs` | 電源値フォーム |
| `xcheck.rs` / `xcheck_tran.rs` / `xcheck_ac.rs` | 開発用: ngspice との検算 (`--xcheck`) |

エディタは1つの型を複数ファイルの `impl` に分割している (子モジュールから親の非公開フィールドが見える)。型・フィールドは各 `mod.rs`。

### `schematic/` (`SchEditor`) — ファイル一覧: `docs/code-notes/app.md`
`mod.rs` 型 / `canvas.rs`・`draw.rs`・`interact.rs`・`symbols.rs` 描画と操作 / `panel/`(`mod.rs` タブ・ツール・ERC・フォーム / `parts.rs` 部品一覧とライブラリの木 / `properties.rs` 選択部品の設定と右クリックの窓)・`analysis_panel.rs`・`hint.rs` 左パネル / `edit.rs`・`stretch.rs` 部品を動かすと配線が付いてくる・`erc_skip.rs` 描きかけの範囲をERC対象外に・`net_hints.rs` ネットの電流・差動ペアの指定・`sch-core/noconnect.rs` 意図して未接続のピンの印 (NC)・`selfheat.rs`・`selfheat_tran.rs` 自己発熱と温度の時間変化の窓・`requirements.rs` 設計の要件の表の窓・`snippet.rs` 選択を保存して再利用・`sheets_ui.rs` シートのタブ(複数シートの中身は `sch-core/sheets.rs`)・`clipboard.rs` コピー&貼り付け・`history.rs`・`file.rs`・`library.rs` / `quick.rs` クイック追加 / `lessons/`(`mod.rs`=型と窓、`conditions.rs`=手順の自動チェック、`basics/devices/analog/supply/models/signals/debug.rs`=レッスンのデータ)・`challenge/`(`mod.rs`=型、`basic.rs`/`power.rs`=データ、`judge.rs`=判定、`window.rs`)・`wizard.rs` 学習 / `dc.rs`・`tran.rs`・`ac.rs`・`sweep.rs`・`noise.rs`・`wave.rs` 各解析 / `fault.rs`・`param.rs`・`variation.rs`・`sensitivity.rs` 感度の実行と表・`psrr.rs` 電源除去比・`troubleshoot.rs` 症状から原因を探す・`repair_practice.rs` 修理の練習 (仮想テスター)・`spec_table.rs`・`bench_data.rs` 実測を波形に重ねる・`power_ui.rs`・`settings.rs`

### `board_editor/` (`BoardEditor`)
`mod.rs` 型 / `ribbon.rs` 配線を鎖にして帯の三角形で描く / `padlead.rs` パッドの両端の吸着点と垂直な出入り / `align.rs` 選んだ部品の整列ボタン / `livepath.rs` 自動経路(出発点からカーソルまでの探索) / `lengths.rs` 長さ合わせ(蛇行) / `canvas.rs`・`interact.rs`・`draw.rs`・`panel.rs` / `select.rs`・`route.rs`・`tools.rs` / `view3d/`(`mod.rs`=状態、`window.rs`=窓、`scene.rs`=立体と描画、`camera.rs`、`picture.rs`、`glow.rs`)・`raster3d.rs` 3D / `width.rs` 配線の太さ・電圧降下 / `order.rs` 発注の準備 / `netwindow.rs` ネットをクリックしたときの設定の窓(電流・推奨の線幅・ネットクラス) / `file.rs`・`actions.rs`・`history.rs`・`view.rs` / `layers.rs` 層 / `pairs.rs` 差動ペア / `shoving.rs` 押しのけ / `tracking.rs` 延長線トラッキング(吸着点からのガイド) / `requirements.rs` 取り込んだ組と距離の点検の表示
