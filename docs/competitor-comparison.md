# 他の電気CADとの機能比較

2026-11-02 に、各社の公式ドキュメント・機能紹介ページを調べて作った。2026-11-05 に、Altium の配線ルール(ルールの種類・対象の指定・順位・点検)を公式ドキュメントで調べて足した。比較の相手は KiCad 9、EasyEDA(Standard / Pro)、Altium Designer、Quadcept。

**読み方**
- 「確認」= 下の出典の公式ページに書いてあったこと。
- 「未確認」= 今回の調べでは書いてあるページに行き着かなかった、または機能紹介の文だけで細部が分からないもの。「無い」という意味ではない。
- circuitCad の列は `CLAUDE.md` と `docs/candidates.md` の記載による。
- 機能紹介は各社の宣伝が中心。何かを判断に使うときは、リンク先の最新の説明を読み直すこと。

## 比較表

| 機能 | circuitCad | KiCad 9 | EasyEDA | Altium Designer | Quadcept |
|---|---|---|---|---|---|
| 回路図 → 基板 → 製造データ | あり | 確認 | 確認 | 確認 | 確認(回路図 Circuit Designer、基板 PCB Designer/3D) |
| SPICE シミュレーション | あり(解析の種類が多い) | 確認(ngspice を回路図エディタに統合。モデル付きの記号で実行し波形表示) | 未確認(ngspice の記述はあるが、深さは出典で食い違う) | 確認(SPICE シミュレーションの機能ページあり) | 未確認(検索では記載なし) |
| 回路図の階層設計 | なし(コードで確認)。複数シートは平らに並べるだけで、シートシンボル・ポートは無い。ネットラベルと電源記号は全シート共通。同じサブ回路の繰り返し利用はできず、部品をコピーして置く(`sheets.rs`) | 未確認(階層シートは一般に知られた機能だが、今回は公式ページで確認していない) | 未確認 | あり(Altium の知識による。今回は公式ページで未確認。シートシンボルとポート、マルチチャンネル) | 未確認 |
| 回路図のバス | 一部(2026-11-04 に実装)。`D[0..7]` のラベルと太いバスの線があり、ERC が番号の抜け・範囲外・幅の食い違いを点検する。バスは何もつながない見た目で、各ピンには D0〜D7 のラベルをラベルツールの連番で付ける。バスエントリ(斜めの引き出し線)・ピン名がバスのピンは無い | 未確認 | 未確認 | あり(Altium の知識による。今回は公式ページで未確認) | 未確認 |
| 自動配線 | あり | 本体には無い(確認)。Freerouting をプラグインとして入れる形で、JRE が要る。KiCad 側は組み込む予定を当面持たない旨の古い書き込みあり | Pro は内蔵、Standard は外部プラグイン(確認) | 内蔵の自動配線(トポロジカルルーター)と、選んだネットを整えて引く ActiveRoute(確認) | セミオートルーター(確認) |
| 手で引くときの配線 | あり(押しのけ、垂直な出入り、延長線トラッキング。差動ペアは相手線も一緒に押して間隔を保つ) | 押しのけ配線の内蔵ルーター(CERN 製。確認、出典は古い講演と掲示板)。差動ペアを無視して押しのけることがあるという報告あり | 未確認 | 歩み寄り(walkaround)・寄り添い(hug)・押しのけ(push)の3モード(確認) | 未確認 |
| 差動ペア・等長配線 | あり(差動ペア、蛇行、ペア間の長さ合わせは2本を入れ子の山でいっしょに曲げて間隔を保つ。山の並べ方は1種類。ペア内の差は1本だけの蛇行で直すので間隔がずれる) | 差動ペアの配線・長さ調整あり(確認、出典は第三者の解説)。遅延での合わせは v10 で新規という書き込みあり。v9 の細部は公式で未確認 | 未確認 | 確認: 蛇行は Accordion / Trombone / Sawtooth の3形。ペア内とペア間を別のルールで合わせる。ActiveRoute にも長さ合わせあり | 確認 |
| 配線ルールの対象の指定 | ネット名の一覧(ネットクラス)と、役割(`Role`)で決める。回路図の「設計の要件」の表に、ネットの役割・近くに置く組・離す組などを書き、基板に取り込む | 確認: ルールごとに、条件の式(`A.hasNetclass('HV')` など。ネット・ネットクラス・層・領域・部品の性質)を書く。条件を書かなければ全部に効く。ネットクラスは1つのネットに複数付けられ、順位の高いクラスから性質を取る(9.0) | 確認(Pro): ルールを「ネット」「ネットクラス」「層」「領域(制約領域)」「部品」「銅ベタ」「差動ペア」などに割り当てる(ファイル形式の説明より)。ネット画面で、ネットごとにルールを選ぶか、ネットクラスを作って割り当てる。2つのネット(クラス)の間のルールも決められる。製造先に合わせたプリセット(JLCPCB など)に切り替えられる | 確認: ルールごとに、ネット・ネットクラス・層などを選ぶか、問い合わせの式(Query Language)を自分で書く。PCB Filter パネルで対象を絞って、そこからルールを作れる。ルール中心の「Rules and Constraints Editor」と、ネットを表のように並べて決める「Constraint Manager」の2つの入口がある | 確認: 「詳細設定」で値を作り、「アサイン」でネットクラスに割り当てる(2段階)。クラス間のクリアランスも個別に決められる(クラスの値より優先)。基板の一部だけ別のルールにする「デザインルール領域」(多角形・矩形・円。クリアランス・配線・ティアドロップ・ベタ接続。中に完全に収まった物にだけ効く) |
| ルールの優先順位・まとまり | クラスごとの値と基板全体の下限(`Rules`)。同じ種類のルールが重なったときに順位で決める仕組みはない。ネットクラスに配線の順番(`order`)はある | 確認: カスタムルールは、ファイルの後ろのものから調べて、最初に合ったものを使う(細かいルールを後ろに書く)。基板全体の最小値(Constraints)は、ネットクラスやカスタムルールでは下回れない | 確認(Pro): ルールは種類ごとに名前を付けて複数作り、1つを「既定」にする(ネットごとに選ばなければ既定)。同じ種類で同じ名前は不可。順位の仕組みは未確認 | 確認: 同じ種類のルールが重なったら、順位の高いものから対象に合うものを1つ選ぶ(広い対象は低く、細かい対象は高く)。順位は種類の中でだけ競う(例外: Matched Lengths は全部が適用される)。ルールセット(インターフェースや製造先ごとにまとめて、切替・複製・書き出し・読み込み)はベータ機能 | 確認: ネットクラスの値より、クラス間クリアランスが優先。デザインルール領域の中では、その領域のルールが使われる。順位付けの一般的な仕組みは未確認 |
| ルールの種類の数 | 基板全体: 最小線幅・間隔・基板端・ドリル・アニュラリング・マスク拡張。ネットごと: 線幅・間隔・ビア・インピーダンス・順番・ビアの可否・使う層・長さの上限・太さの例外・仕上げ。ほかに、バイパスの近さ・鋭角・電流と電圧降下・差動ペア・組の広がりと離す距離を点検する | 確認: 29 種類の制約(annular_width・assertion・clearance・creepage・connection_width・courtyard_clearance・diff_pair_gap・diff_pair_uncoupled・disallow・edge_clearance・hole_clearance・hole_size・hole_to_hole・length・min_resolved_spokes・physical_clearance・physical_hole_clearance・silk_clearance・skew・text_height・text_thickness・thermal_relief_gap・thermal_spoke_width・track_angle・track_segment_length・track_width・via_count・via_diameter・zone_connection)。`assertion` は任意の真偽の式 | 確認(Pro): 安全間隔(ネット間の表)・その他の間隔(部品どうし)・配線幅(最小・既定・最大)・ネット長・ブラインド/ベリードビア・ビアのサイズ・内層のプレーン・銅ベタ(間隔・外形からの距離・パッドの接続・スポーク)・ペーストマスク拡張・ソルダーマスク拡張 | 確認: 65 種類を 10 の分類(電気・配線・SMT・マスク・プレーン・テストポイント・製造・高速信号・配置・信号の完全性)に分ける | 確認: DRC の項目 20 以上(未配線・クリアランス・部品クリアランス・ビア規則・配線幅・配線長/等長・差動ペア・円弧配線・ダングリング・浮きビア/スタブビア/ドリル重複・スタックビア・配線角度・ティアドロップ・パッドオンビア・ベタ分断・浮きベタ・禁止領域・サーマル・テストランド など)。項目ごとに実行するかと、エラーか警告かを選ぶ。製造の観点の MRC は別 |
| 高速信号のルール | 長さの上限(`max_length`)、差動ペアと蛇行、インピーダンス(層構成から線幅を出す)がある。並走の長さ・スタブ・ビア数・戻り電流の経路の上限はない(並走は `quality.rs` で数字を出すだけで、ルールではない) | 確認: length(最小・最大)、skew(ペア内も指定可)、diff_pair_gap、diff_pair_uncoupled(ペアが離れてよい長さ)、via_count(ネットあたり)、track_angle、track_segment_length。長さ調整の既定値も設定画面にある。並走・スタブ・戻り経路・トポロジは見当たらない | 確認(Pro): ネット長のルールは、DRC の制約ではなく、配線中の表示(範囲内は緑、外は赤)。差動ペアの管理・等長グループ・パッドペアのグループ・信号タイミングの等長(許容差を ps で)・xSignals の画面がある。ルールの種類としての並走・スタブ・ビア数は未確認 | 確認: Parallel Segment(同じ層・隣の層で並走してよい長さ)、Length、Matched Lengths(許容差・ペア間とペア内を別に)、Daisy Chain Stub Length、Max Via Stub Length(バックドリル)、Maximum Via Count、Return Path(基準層の連続)、Vias Under SMD。信号の完全性の分類には、インピーダンス・オーバーシュート・飛行時間などもある | 確認: 配線長と許容差(等長配線)、差動ペアのギャップ・配線長・許容差を DRC で点検。差動ペアの配線長チューニングあり。並走・スタブ・戻り経路は未確認 |
| 配線ルールの点検のタイミング | 「DRC を実行」で点検し、ファイルを開いたときにも実行する。自動配線と押しのけ配線は、ネットクラスの値を使って引く | 確認: DRC を実行して点検する。重大度は違反の種類ごとに設定(エラー・警告・無視)。カスタムルールの重大度に「無視」「除外」もある。インタラクティブルーターは、エラー・警告のルールを守って引く(「無視」は守らない) | 確認(Pro): 「DRC をかける」と「DRC(カスタム)」のメニューがある。配線中の自動チェックの有無は未確認 | 確認: オンライン DRC(編集中に新しい違反を見つける)とバッチ DRC(いつでも全部を点検)。ルールの種類ごとにどちらで動かすかを選ぶ | 確認: オンライン DRC(配線中にリアルタイムで点検。同ネットのクリアランスは対象外)と、実行する DRC。項目ごとにエラー・警告を選べる |
| 発注前の点検(DFM) | あり(`fab.rs`) | 未確認 | 未確認 | 未確認 | DFM チェック(確認) |
| 製造データの形式 | Gerber / Excellon / BOM / 配置CSV | Gerber・配置リストなど(確認) | 未確認 | 未確認 | ODB++ に対応(確認)。circuitCad は対象外 |
| 部品店との連携 | 購入リスト(LCSC の型番14件) | 未確認 | LCSC の部品に直接リンク、基板製造と一体(確認。JLCPCB・LCSC・EasyEDA は同じ会社グループ) | 未確認(Octopart 系のページはある) | 未確認 |
| 3D モデルの取り込み | STEP は部品1個ずつ(部品フォーム) | 3D 出力あり(確認)。取り込みの形式は未確認 | OBJ・WRL は両版、STEP は Pro だけ(確認) | STEP の双方向(確認) | 3D 機能あり。取り込み形式は未確認 |
| 3D の書き出し | なし(E3) | 3D 出力あり(確認)。9 で STEP 出力の改善(プレリリース記事の記述) | 未確認 | STEP / Parasolid / 3D PDF(確認) | 未確認 |
| 複数基板(マルチボード) | なし(E5) | 未確認 | 未確認 | あり: 接続の照合(Connection Manager)、3D の嵌合(mating)、システムBOM(確認) | 未確認 |
| 共同作業 | なし | 未確認(Git 運用は一般的だが今回は確認していない) | チーム・権限・ライブラリ共有は両版。ロック・チェックイン/アウトは Pro。リアルタイム共同編集は「Pro の 2025 版で対応」と注記(確認) | 確認(バージョン管理・権限は競合の記述。公式の詳細は未確認) | クラウドベース(確認) |
| 無償利用 | 無償 | 無償・オープンソース(確認) | 未確認 | 有償 | 非商用に限り無償(パッド数・層数に制限あり。確認) |
| 初学者向けの学習機能 | あり(レッスン・チャレンジ・ホバー・学習パス) | 公式には見当たらず。コミュニティの教材が多い(確認) | 未確認 | 未確認 | 未確認 |
| CLI / AI からの操作 | あり(`ccad`、MCP) | 未確認 | EasyEDA 向けの MCP サーバーが第三者から出ている(確認。公式ではない) | 未確認 | 未確認 |

## 分かったこと

**circuitCad が強いところ**
- 回路図・シミュレーション・基板が1本で、解析が多い(過渡・AC・ノイズ・極零・ループゲイン・最悪ケース・モンテカルロ・故障・自己発熱)。KiCad は ngspice を統合しているが、解析ごとの専用画面や測定は circuitCad のほうが多い、という見立て(KiCad 側の細部は未確認)。
- 学習機能と日本語UI。調べた範囲で、公式の機能として持っているものは見つからなかった。
- CLI と MCP で、AI が回路を作って検証できる。

**差が出ているところ**
- 3D と STEP。Altium は STEP/Parasolid の入出力と、機械設計ツールとの同期(MCAD CoDesigner)まである。EasyEDA も Pro なら STEP を取り込める。circuitCad は部品1個の STEP 読み込みだけ(E1〜E4)。
- マルチボード。Altium は「接続の照合 → 3D で嵌合 → システム全体の部品表」の3本立て(E5 の根拠)。
- 発注との一体感。EasyEDA は部品店と基板製造まで同じ会社グループ。circuitCad は購入リストまで(A2)。
- 共同作業。EasyEDA は権限・ロック・チェックイン/アウトまである。circuitCad は個人利用の前提。
- 回路図の階層設計とバス。階層設計は無い(コードで確認)。バスは、ラベルと線と点検だけを実装した(2026-11-04)。チャンネルが繰り返される回路では、部品のコピーが要る。Altium 側は知識によるもので、公式ページでは未確認。

**ここから言える次の手**
- 「初学者が基板を発注して終わるまで」を考えると、効きそうなのは **発注との一体感(A2 の型番確認、購入リスト)** と **3D(E1〜E3)**。マルチボードは3Dのあと(`candidates.md` の方針どおり)。
- 共同作業は、個人向けの方針とは別の話なので、今は候補にしない。

## 配線ルール: Altium との対応(2026-11-05、公式ドキュメントで確認)
Altium の分類ごとに、circuitCad で近いものを挙げる。「なし」は、コードと `docs/candidates.md` の記載で見つからなかったもの。

| Altium のルール(分類) | circuitCad |
|---|---|
| 配線: Width(最小・推奨・最大。層ごとの値、インピーダンスの設定から) | ネットクラスの `track_width` と `impedance`(層構成から線幅)。最小・最大の幅は基板全体の `min_track_width` のみ |
| 配線: Routing Neck-Down(細く引いてよい合計の長さの上限。BGA の引き出しなど) | `width_exceptions`(太さの例外の区間)。合計の長さの上限はなし |
| 配線: Routing Topology(最短・デイジーチェーン・スター など) | なし(ネットは最短でつなぐ) |
| 配線: Routing Priority・Routing Layers・Routing Via Style | 順番 `order`・使う層 `layers`・ビアの径とドリルと可否 |
| 配線: Routing Corners・Fanout Control・Differential Pairs Routing | 仕上げ(角丸め・涙型。`smooth.rs`)・出口の線 `breakout.rs`・差動ペア `diffpair.rs` |
| 電気: Clearance | 基板全体の最小間隔とクラスごとの間隔。2つのクラスの間は、大きいほう(`clearance_between`)。条件式で「AとBの間だけ」とは書けない |
| 電気: Short-Circuit・Un-Routed Net・Un-Connected Pin | DRC のショート・ラッツネスト(未配線)・ERC の未接続 |
| 電気: Creepage Distance・Z-Axis Clearance・Modified Polygon | なし |
| 製造: Hole Size・Minimum Annular Ring・Board Outline Clearance・Acute Angle・Silk To Silk | 最小ドリル・アニュラリング・基板端・鋭角 `acute.rs`・シルクの点検あり。Net Antennae・Layer Pairs はなし |
| 高速信号: Length・Matched Lengths | 長さの上限と蛇行、差動ペアの長さ合わせ |
| 高速信号: Parallel Segment・Daisy Chain Stub・Max Via Stub・Maximum Via Count・Return Path・Vias Under SMD | なし |
| 信号の完全性: Impedance | 目標インピーダンスの点検あり(発注前チェック)。Overshoot・Undershoot・Flight Time・Supply Nets はなし |
| プレーン: Polygon Connect Style・Power Plane Clearance・Power Plane Connect | ベタの接続・島・細いかけらの点検あり(`drc/pour.rs`)。内層のプレーン専用のルールはなし |
| SMT・マスク・テストポイント・配置の分類 | マスク拡張・サーマルビアの点検あり。それ以外の SMT・テストポイントの専用ルールはなし。配置は、設計の要件の「近くに置く組」「離す組」で近い |

**見立て**
- 値の種類は、Altium の「線幅・間隔・ビア・層・長さ・インピーダンス」の中心部分は持っている。差が大きいのは、**高速信号の専用ルール(並走・スタブ・ビア数・戻り経路)** と、**ルールの対象を式で書く自由度と順位付け** の 2 点。
- 初学者向けの目的(2層の小さな基板)では、前者はほぼ要らない。後者は、「クラスの値」+「役割」で足りる範囲に収めている。
- 次に足すなら、小さくて効果がありそうなのは、Routing Neck-Down の「細い区間の合計の長さの上限」、Parallel Segment(並走の長さの上限。`quality.rs` の数字をルールにする)、Routing Topology のうちデイジーチェーン。優先順位とルールセットは大きく、今の使い方では必要になっていない。

## 配線ルール: 4つのソフトの考え方の違い(2026-11-05、公式ドキュメントで確認)
| 点 | Altium | KiCad 9 | EasyEDA Pro | Quadcept | circuitCad |
|---|---|---|---|---|---|
| ルールの書き方 | 画面の表(ルールの種類ごと)と、対象を問い合わせの式で指定 | **文字で書く**(s 式のカスタムルール)。画面は構文チェックと補完つきの編集欄 | 画面の表。ルールに名前を付けて、ネット・クラス・領域などに割り当てる | 画面の表(詳細設定とアサインの2段階) | 画面の表(ネットクラス・設計の要件の表)と、役割 |
| 基板全体の下限 | あり(全体を指す既定のルール) | あり。ネットクラスやカスタムルールでも下回れない | あり(既定のルール) | あり(クリアランスの表など) | あり(`Rules`) |
| ネットクラスの扱い | ルールの対象の1つ | **値は既定値で、強制されない**(手で違う幅にしても DRC は怒らない)。強制は、カスタムルール | ルールを割り当てる単位 | ルールを割り当てる単位 | 値のうち、間隔・使う層・長さの上限は DRC が点検する。役割で既定が付く |
| 領域ごとのルール | 式で指定(領域での指定は未確認) | ルールエリア(キープアウト)と、式での指定 | 制約領域(間隔・配線・差動・ビア・ベタ) | デザインルール領域 | 設計の要件で「組」と「離す距離」。領域ごとのルールはなし |
| 高速信号 | 専用のルールが多い | 長さ・スキュー・ペアの間隔・ビア数 | 差動・等長・タイミングの管理画面 | 配線長・差動ペアの DRC | 長さの上限・差動ペア・蛇行・インピーダンス |

**見立て(2026-11-05)**
- 4 つとも、「基板全体の下限 + ネットクラス(または同等のもの)」が土台で、circuitCad も同じ形。
- KiCad は、自由度を**カスタムルールの文**に寄せている(`assertion` で式を書ける)。circuitCad の方向とは違うが、`length` `skew` `via_count` `track_angle` `track_segment_length` `creepage` のような**単位の細かい制約**は参考になる。
- EasyEDA Pro と Quadcept は、画面の表を中心に、領域・クラス間・製造先ごとのプリセットで調整する。**製造先のプリセットに切り替える**(EasyEDA の JLCPCB など)のは、発注まで通す目的に合う(circuitCad は `fab.rs` の発注先と発注前チェックで近い)。
- circuitCad は、点検の message に原因と直し方を日本語で付けている。他のソフトの点検の説明の書き方は未確認。

## 未確認として残したもの(次に調べるなら)
- 配線ルール: KiCad の Board Setup の Constraints の各項目の細部、KiCad のカスタムルールの条件に使える性質の一覧、EasyEDA の差動ペアのルールの細部と優先順位、EasyEDA Standard 版のルール、Quadcept のクラス間クリアランスや配線長の設定の細部。
- KiCad: 配線機能の公式の説明(今回は公式マニュアルの PCB エディタの章が取れず、掲示板・第三者の解説・古い講演に頼った)、STEP の取り込み、DFM に近いチェック。
- Altium: SPICE の解析の種類、ActiveRoute の使い方の細部(ビアを置けない、電源ネットの戦略が無い、という制限は ActiveRoute の改善点の説明より)。
- EasyEDA: シミュレーションの種類と深さ(Pro の公式ドキュメント)。
- Quadcept: シミュレーション、STEP の入出力(公式の機能一覧)。

## 出典
- KiCad 9.0 紹介: https://docs.kicad.org/9.0/en/introduction/introduction.html
- KiCad 9 の新機能(紹介記事): https://www.protoexpress.com/blog/kicad-version-9-exploring-the-new-features/
- EasyEDA Standard と Pro の機能比較: https://prodocs.easyeda.com/en/introduction/std-vs-pro/
- EasyEDA について: https://easyeda.com/page/about
- LCSC のブログ(EasyEDA と KiCad の連携): https://lcsc.com/blog/smarter-pcb-design-easyeda/
- KiCad の配線(CERN のルーターの講演、2015): https://archive.fosdem.org/2015/schedule/event/pcb_routing/
- KiCad 9 の差動ペアの長さ調整(第三者の解説): https://techexplorations.com/guides/kicad/high-speed-pcb-design/kicad-9-differential-pair-length-tuning-guide
- KiCad の差動ペアを無視した押しのけの報告: https://gitlab.com/kicad/code/kicad/-/issues/16146
- Freerouting の連携(KiCad): https://www.github.com/freerouting/freerouting/blob/master/docs/integrations.md
- KiCad の自動配線についての掲示板: https://forum.kicad.info/t/autoroute-is-it-coming-mod-edit-short-answer-is-not-in-the-foreseeable-future-so-nothing-to-see-here/30441
- Altium ActiveRoute の改善: https://altium.com/documentation/altium-designer/nfs-18-0activeroute-improvements-ad
- Altium の長さ調整: https://www.altium.com/documentation/altium-designer/length-tuning-ad
- Altium Designer のマルチボード: https://www.altium.com/altium-designer/features/multi-board
- Altium Designer の機能一覧: https://altium.com/altium-designer/de/features
- Quadcept Community(窓の杜): https://forest.watch.impress.co.jp/library/software/quadcept/
- Quadcept(MONOist): https://monoist.itmedia.co.jp/mn/articles/1302/28/news133.html
- Altium の PCB 設計ルールの種類(65 種類・10 分類): https://www.altium.com/documentation/altium-designer/pcb/design-rule-types
- Altium のルールの定義・対象の指定・順位・ルールセット: https://www.altium.com/documentation/altium-designer/pcb/defining-scoping-managing-design-rules
- Altium の配線ルール(Width・Neck-Down・Topology など): https://www.altium.com/documentation/altium-designer/pcb/design-rule-types/routing
- Altium の高速信号ルール: https://www.altium.com/documentation/altium-designer/pcb/design-rule-types/high-speed
- Altium の DRC の設定と実行(オンライン・バッチ): https://www.altium.com/documentation/altium-designer/pcb/drc/setting-up-running
- KiCad 9.0 PCB エディタ(Board Setup の制約・ネットクラス・カスタムルール): https://docs.kicad.org/9.0/en/pcbnew/pcbnew.html
- EasyEDA Pro の設計ルール: https://prodocs.easyeda.com/en/pcb/design-design-rule/
- EasyEDA Pro のネットクラス管理: https://prodocs.easyeda.com/en/pcb/design-net-class-manager/
- EasyEDA Pro の設計ルールのファイル形式(ルールの割り当て先): https://prodocs.easyeda.com/en/format/pcb/rule/
- Quadcept のネットクラス定義: https://www.quadcept.com/ja/manual/pcb/post-100
- Quadcept の DRC 実行項目設定: https://www.quadcept.com/ja/manual/pcb/post-5
- Quadcept のデザインルール領域: https://www.quadcept.com/ja/manual/pcb/post-121
- Quadcept のクリアランス設定: https://www.quadcept.com/ja/manual/pcb/post-14
- Quadcept のオンライン DRC: https://www.quadcept.com/ja/manual/pcb/drc-2
