# thermal.rs (基板の熱の地図、候補 H7 の段 ①、2026-11-16)

基板を升目に切り、銅の層ごとに 1 枚の升目の板として、定常の熱伝導を解く。`thermal_map(board, &[HeatSource], 銅の厚さ µm, &ThermalSpec) -> ThermalMap`。

- **升目**: 既定 0.5 mm(1 層 6 万升を超える基板は粗く)。升の中心が外形の中で、取り付け穴の外なら基板。銅の被り(0〜1)を 3 × 3 の副標本で数える: ベタは `zonefill::merged_fills`(塗った形、穴あき)、配線 `track_piece`、パッド `pad_piece`(スルーホールは全層)、ビア `via_piece`(全層)。多角形は行ごとの交点で塗る(`paint`、偶奇)。
- **伝わり方**: 層の中は隣の升へ、銅 `被り × 390 W/m·K × 厚さ` + FR-4(0.8 W/m·K、基板 1.6 mm を層の数で割った厚さ)のシート。2 つの升は半分ずつの直列(調和平均)。層の間は FR-4 の厚さ方向(0.3 W/m·K、層の間隔 = 1.6 /(層 − 1))と、ビア・スルーホールのめっき(25 µm)の筒。
- **空気へ** `h_air(上昇, 周囲, 長さ, 向き, 放射率, 気圧)`: 空気は理想気体(動粘度・拡散率は T^1.75 / P)。Ra = gβΔT·L³/(να)、平らに置けば L = 面積 / 周、熱い面が上 Nu = 0.54 Ra^¼(1e7 を超えたら 0.15 Ra^⅓)、下 0.27 Ra^¼、立てれば L = 縦の長さ、0.59 Ra^¼。放射は εσ(Ts² + Ta²)(Ts + Ta)(ε 0.9)。升ごとに自分の上昇で h を出し、地図と h を交互に(ゆるく解きながら、h が 0.5 % 以内に落ち着いたら最後にきっちり 1 回)。80 mm・30 K で対流 約 6.6、放射 約 6.7 W/m²K(試験)。標高は標準大気の気圧(3000 m で 0.69 倍)。
- **部品**: 部品ごとに接合部の節を 1 つ足す。パッドの下の升へ θjc(升の数で等分)、空気へ部品の本体と足 θbody。`HeatSource::new(名前, 電力, 自分の θ, θjc, 放熱板)` は θbody = (θ − θjc) / `BODY_SHARE` と放熱板の並列。`BODY_SHARE` = 0.231 は NDT3055L の最小のパッドの点だけで合わせた(データシート 98 K/W に対し、本体なしの格子は 127.5 K/W: 1 − 98/127.5)。これで 0.066 in²・1 in² の点も 5 % 以内(試験 `the_grid_follows_the_datasheet_curve_of_a_sot223`: 105.6 / 90.5 / 43.6 K/W、データシート 110 / 95 / 42。JEDEC の大きさ 76.2 × 114.3 mm の 1 層の銅、2 oz、1 mm の升)。本体なしの格子だけでも 2〜3 割高めに出るだけで、曲線の形は同じ。
- **解き方**: 共役勾配(ヤコビの前処理、圧縮行 `Csr`)。76 × 114 mm の 2 層・0.5 mm(7 万節)で、開発ビルド(opt-level 1)0.5 s、1 mm で 0.03〜0.2 s。1 mm と 0.5 mm の差は 1.5 % 以内なので、試験は 1 mm。
- **画面**: 基板エディタの「③ 確認する」の「熱の地図…」(`board_editor/thermalmap.rs`)。窓で置き方・標高、「熱の地図を計算」で App が回路図の自己発熱を計算して `thermal_sources`(部品ごとの電力・θ・θjc・放熱板)を渡す(`take_thermal_request` → `run_thermal_map`)。結果: いちばん熱い所・h・表(電力・部品の温度・パッドの銅・自己発熱の温度)、「基板に色を重ねる」と層の選択。色は 3D の温度の色と同じ `heat_color`、半透明の絵を銅のあと・シルクの前に描き、左下に目盛り。撮影用 `--thermal-map [standing]`(サンプル `heat_copper.ccproj`: R1 はベタで 67 ℃、R2 は細い配線で 108 ℃、自己発熱の式では両方 91 ℃)。
- **CLI**: `board heatmap [temp=] [standing] [alt=] [cell=]`。`open FILE.ccproj` で基板 1 枚のプロジェクトの回路図と基板を読めるようにした。試験 `heatmap_shows_the_pour_spreading_one_resistors_heat`。
- **回路図に反映(段 ②、2026-11-16)**: `thermal_matrix(board, sources, 銅, spec) -> (R, 地図)`。まず実際の電力で地図を解いて h を決め、その h のまま、部品ごとに接合部へ 1 W を入れて解く(`par_map` で並列)。`R[i][j]` = 部品 j の 1 W で部品 i の接合部が何 K 上がるか。h を固定するので R·P は地図の接合部と一致し、R は対称(試験 `the_matrix_adds_the_heat_of_the_parts_as_the_map_does`)。sim-core 側:
  - `board_heaters(sch, circuit, heaters, on_board)`: 冷えた動作点で 1 mW 以上・いちばん大きい部品の 1 % 以上の電力の部品を、大きい順に最大 24 個(`BOARD_HEATERS_MAX`)。ほかは θ の式(`heat_copper`)のまま。
  - 自己発熱: `heaters_with_matrix` で部品の θ を R の対角に、残りを結合 `(a, b, K/W)` にし、`self_heat_coupled` が目標を `周囲 + θ·P + Σ R·P_ほか` に。`HeatedPart.from_others` にほかの部品から受けた分。
  - 温度の時間変化: `network_from_matrix` が G = R⁻¹ を、行の和(空気への θ)と、対角でない成分の符号を変えたもの(部品どうしの結合 `ThermalLink`)にする(負の結合は落とす)。`nodes_with_matrix` は、走らせない部品への結合をその部品の空気への道に足し、手で入れた結合は行列にない組だけ残す。
  - アプリ: 「シミュレーションに反映」で基板の複製を `SchEditor.board_heat`(`schematic/board_heat.rs`)に持ち、`run_self_heat`・`run_heat_tran` が使う。行列は、基板の形・選んだ部品・その θ/θjc/放熱板・周囲温度の鍵で取っておく(電力は h にしか効かないので鍵に入れない)。表の θ の横の「→ θ」は基板からの値(ホバーで説明)、上昇のホバーに「ほかの部品から受けた熱」、表の下に「基板の熱の地図から: R1, R2」。撮影用に `.ccproj` でも `--section` が効くようにした(`--post-layout --section selfheat --selfheat`)。
  - CLI: `board postlayout on` のあとの `heat`・`heattran` が同じ行列を使う(`heat` に `board_grid`・`from_others_k`)。
  - サンプル `heat_copper` を反映すると、R1 は θ 85 → 53 K/W・+40 K、R2 は 85 → 107 K/W・+80 K(細い配線では、θ の式の 82 より高い。格子は FR-4 と細い配線だけでは熱が広がらないと見る)。互いに 0.09 K。過渡は 2000 秒で定常の上昇に 2 % 以内で合う(試験 `heat_copper_project_shows_the_pour_cooling_one_resistor`)。CLI の試験 `heat_with_postlayout_lowers_theta_by_the_copper_joined_to_the_pins` は、自動配置で隣り合った 2 本の抵抗が基板を通って約 13 K 温め合うようになった(θ は 100 → 86 に下がるが、上昇は 25 → 34 K)。
- **限界**: 強制空冷なし、まわりの部品の陰なし、FR-4 は 1 つの材料、内層は等間隔、部品の本体は 1 つの割合。±20〜30 %。
- **残り(H7 の続き)**: 時間の変化の地図(熱容量と後退オイラー)、発熱する部品が近いときの `keep_apart` の候補、配線の抵抗に局所の温度。過渡の部品の熱容量 `cth` は部品の値のまま(基板の熱容量は入っていない)。
