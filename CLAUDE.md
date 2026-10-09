# circuitCad — コードマップ

目的: 回路図 → SPICEシミュレーション → 基板 → ガーバー出力を1本のソフトで。初学者向け(日本語UI、ホバー説明、ヒント、サンプル)。
機能を足すときは「GUI表示 + サンプル + 説明文」までセットで作る。UIの文言は日本語、コード・コメントは英語。

作業前に全ファイルを読まず、下の表で目的のファイルを特定してから読むこと。

```
cargo test --workspace        # 全テスト (CIと同じ)
cargo clippy --workspace --all-targets   # CIはエラーを禁止 (警告は可)
cargo run -p app              # 起動 (引数で .schjson / .pcbjson を開ける: main.rs apply_args)
```

## クレート (依存: app → pcb-core, sch-core, sim-core, part-lib)

| クレート | 役割 |
|---|---|
| `sch-core` | 回路図モデル (`Schematic`/`Component`/`Wire`/`Label`/`Kind`)、ネット抽出 `netlist.rs` (`extract`, `net_at`, `erc` — 指摘は `Problem{message,at,hint,severity}`、hintに原因と直し方を書く)、SPICE出力 `spice.rs` |
| `sim-core` | GUI非依存のシミュレータ。`from_schematic.rs` で回路図→`Circuit` |
| `pcb-core` | 基板モデル `model.rs`、DRC、ラッツネスト、自動配線、Gerber/Excellon/BOM出力 |
| `part-lib` | 部品JSON (`*.part.json`: 記号+パッケージ+SPICEモデル)。`examples/` が同梱部品 |
| `app` | egui のGUI。回路図エディタ・基板エディタ・波形表示 |

## sim-core (`crates/sim-core/src`)
- `dc/` 動作点 (`dc_op`、`DcResult`/`ElementResult`、収束戦略 `Strategy` は `mod.rs`)。`build.rs` 回路→コンパイル済み形(`Ce`,`Compiled`) / `devices.rs` トランジスタ等の式 / `mna.rs` 行列の組み立てと各素子のスタンプ(`Ctx`,`Dyn`) / `newton.rs` ニュートン反復 / `linalg.rs` 疎LU分解
- `tran.rs` 過渡 / `ac.rs` AC / `noise.rs` ノイズ / `dcsweep.rs` DCスイープ / `spectrum.rs` FFT・統計
- `waveform.rs` 電源波形 (SIN/PULSE/PWL) / `value.rs` `parse_value`・`format_value`(SPICE数値) / `spice_in.rs` ライブラリ部品の `.model`/`.subckt` 読込
- `from_schematic.rs` 部品→素子の変換 (素子名 = 部品番号 refdes)。新しい部品種別はここ
- `temperature.rs` 温度 (`Circuit::at_temperature` がモデルを書き換える、`temp_sweep`) / `montecarlo.rs` 許容差ばらつき解析 (`monte_carlo`、感度つき。測る量は `McMeasure`: DC電圧 / ACの-3dBカットオフ / 過渡のステップ応答指標)
- `stepmetrics.rs` ステップ応答の指標 `step_metrics` (立ち上がり・行き過ぎ・整定、波形ビューアと `McMeasure::Tran` が使う)
- `tests_*.rs` は機能別 (555, logic, ac, noise, thermal ...)

## pcb-core (`crates/pcb-core/src`)
- `model.rs` `Board`/`Track`/`Pad`/`Via`/`Zone`/`Rules` ほか全データ型
- `drc.rs` `check`(全体)・`clearance`・`shorts`・`connectivity`(銅の接続グループ、ネット判定) — 自動配線もこれを使う
- `ratsnest.rs` 未配線(`Airwire`) / `autoroute.rs` 格子A*の自動配線 (`autoroute`, `route_airwire`、DRCで検証して採用)
- `netlist.rs` 回路図→基板の取込 / `parts.rs` `place_part` / `footprints.rs`・`package.rs` フットプリント
- `fab.rs` 発注先プリセット(`PRESETS`)と発注前チェック(`preflight`) / `gerber_view.rs` 書き出したガーバー・ドリルを読み戻して絵にする(`Preview`)
- `export.rs` 製造一式 (zip) / `gerber.rs` / `excellon.rs` / `assembly.rs` BOM・部品配置CSV / `strokefont.rs` シルク文字

## app (`crates/app/src`)
`main.rs` ウィンドウ・メニュー・`TIPS`(豆知識)・引数処理・統合テスト。`plot.rs` 波形描画、`part_form.rs` 部品作成フォーム、`source_form.rs` 電源値フォーム。

エディタは1つの型を複数ファイルの `impl` に分割している (子モジュールから親の非公開フィールドが見える)。型・フィールドは各 `mod.rs`。

### `schematic/` (`SchEditor`)
| ファイル | 内容 |
|---|---|
| `mod.rs` | 型定義(`Tool`,`Sel`,`DcView`,`TranView`...)、`SchEditor`のフィールド、`volt_color`,`eng`,`kind_tip` |
| `canvas.rs` | 描画エリアの流れ(`canvas`: パン・ズーム、キー、描画→操作の順) / `draw.rs` シートの描画(配線・部品・電圧/電流オーバーレイ・ERCの丸) / `interact.rs` マウス操作(範囲選択・ドラッグ・ツールのクリック) / `symbols.rs` 記号の図形・座標変換・重なり判定 |
| `panel.rs` | 左パネル上部(ツール・部品一覧・選択部品のプロパティと値スライダー・ERC・部品作成フォーム) / `analysis_panel.rs` 左パネルの解析欄(DC・過渡・AC・スイープ・ノイズ・温度とばらつき・結果一覧) / `hint.rs` 画面下のヒント文 |
| `edit.rs` 選択・当たり判定・削除・配線追加 / `history.rs` Undo・Redo / `file.rs` 保存・読込 / `library.rs` ライブラリ |
| `lessons.rs` 「学ぶ」メニューのレッスン(サンプル回路埋め込み+手順、各手順は状態で自動チェック。足すときは `LESSONS` に追加) / `variation.rs` 温度スイープ・ばらつき解析(モンテカルロ)の実行と表示 / `dc.rs` `run_dc`・`sim_circuit`(温度適用済みの回路) / `tran.rs` / `ac.rs` / `sweep.rs` / `noise.rs` 各解析の実行と波形パネル / `wave.rs` 表示中の解析切替・自動再計算 `live_update` |

### `board_editor/` (`BoardEditor`)
| ファイル | 内容 |
|---|---|
| `mod.rs` | 型定義(`Tool`,`Item`,`RouteMode`)、`BoardEditor`のフィールド、`snap`等の補助 |
| `canvas.rs` 描画エリアの流れ・キー操作 / `interact.rs` 範囲選択とポインタ操作 / `draw.rs` 描画(銅、ゾーン、ハイライト) / `panel.rs` 左パネル・選択パネル・ヒント |
| `select.rs` 当たり判定・範囲選択・移動・削除 / `route.rs` 配線の折れ方 / `tools.rs` クリック時のツール動作 |
| `order.rs` 「発注の準備」ウィンドウ(発注先・チェック・プレビュー・書き出し・注文手順) / `file.rs` 保存・読込・製造出力・回路図取込 / `actions.rs` 自動配線・DRC実行 / `history.rs` / `view.rs` |

## 「〜を変えたい」早見表
- 新しい解析を足す → `sim-core/<名前>.rs` + `schematic/<名前>.rs` (実行+パネル) + `schematic/panel.rs` にボタン + `mod.rs` の `WaveMode` と `wave.rs` + `docs/examples/` にサンプル
- 新しい部品種別(基本素子) → `sch-core/lib.rs` の `Kind`、`symbols.rs`、`sim-core/from_schematic.rs`
- ライブラリ部品を足す → `part-lib/examples/*.part.json` (形式は `docs/part-format.md`)
- DRCルール → `pcb-core/drc.rs` / 設計ルール既定値 → `model.rs` の `Rules`
- ヒント文 → `schematic/hint.rs`, `board_editor/panel.rs` の `hint` / 豆知識 → `main.rs` の `TIPS`

## ドキュメント・サンプル
`docs/board-editor.md` 基板の使い方 / `docs/library.md` 部品ライブラリ / `docs/part-format.md` 部品JSON形式 / `docs/examples/*.schjson` サンプル回路。

## 規約
- ボタン・入力欄には `on_hover_text` で説明を付ける。エラーメッセージは原因と対処が分かる日本語で。
- 回路図の変更はフレームごとの比較(`schematic/history.rs`)でUndoに入る。`self.sch` を直接変えれば自動で履歴になる。基板は変更前に `checkpoint()` を呼ぶ。
- 自動配線・自動再計算のように「結果を採用する前に検証する」処理は、失敗時に元の状態を変えないこと。
