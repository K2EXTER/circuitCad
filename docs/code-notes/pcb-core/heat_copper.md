# heat_copper.rs (基板の銅から部品の熱抵抗を見積もる、2026-11-12)

部品の熱抵抗 θ(K/W)は「最小のパッド」の値(データシート、または `sim_core::default_theta` のパッケージの定格電力からの目安 85 K ÷ P)。基板でピンにつながった銅は熱を広げるので、θ と並列に熱コンダクタンス `G`(W/K)を足す: **θ = 1 / (1/θ_部品 + G)**。

- `part_copper(board, 厚さµm)`: 部品ごと・ネットごとに、部品のパッドにつながった銅(`drc::CopperCtx::joined_area`。`pad_copper_area` と同じ数え方: 同じ接続の組のパッド・配線・ビアと、それを含むベタの塊、層ごとに合わせて層を足す)のうち、**部品の中心から `REACH_MM` = 20 mm 以内**の面積 `area` と、部品自身のパッドを引いた `extra`。ネットごとの `G` を足す(ピンごとに別の熱の道)。`conductances` は回路図へ渡す形(参照 → (G, extra mm²))。
- `copper_conductance(extra, 厚さ)` = `k · extra^n · √(厚さ/70µm)`。`k`・`n` は **NDT3055L(onsemi、SOT-223、2 oz)の 3 点**から求める: 最小のパッド 110 K/W、0.066 in² で 95 K/W、1 in² で 42 K/W。SOT-223 のタブとピン 2 のパッド(10.6 mm²)を最小のパッドとし、それを超える面積で合わせる(n ≈ 0.78: 遠くの銅ほど効きが落ちる)。厚さの √ は、広い銅板の放熱(フィン)のコンダクタンスが √(k·t·h) で増えることから。1 oz・1 in² だと約 51 K/W。
- 確かめたのは 1 つのデータシートの曲線だけ。空気の流れ、近くの部品、内層、FR4 を通る熱は入っていない。抵抗の「定格電力から」の θ も、もともと試験基板の銅を少し含むので、目安として扱う。
- 使い道: 基板エディタで「回路図に反映」(post-layout、`App::apply_post_layout`)すると、配線の抵抗と一緒に `SchEditor.heat_copper` に入り、解析タブの「自己発熱」と「温度の時間変化」の θ に効く(`sim_core::part_theta`)。表の熱抵抗の欄は部品自身の θ を編集し、銅があると横に「→ 銅込みの θ」(ホバーに銅の面積)。解除すると消える。CLI は `board postlayout on` のあとの `heat` / `heattran`(`theta_k_per_w` が銅込み、`own_theta_k_per_w`、`board_copper_mm2`)。
- 重さ: ベタの塗りは全ゾーンを 1 回(並列)、部品ごとの面積も並列。
- テスト: `heat_copper::tests`(データシートの点を通る、ベタで θ が下がる、遠い細い配線は効かない)、CLI `heat_with_postlayout_lowers_theta_by_the_copper_joined_to_the_pins`。
