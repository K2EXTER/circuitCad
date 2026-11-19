# 他の電気CADとの機能比較

2026-11-02 に、各社の公式ドキュメント・機能紹介ページを調べて作った。2026-11-05 に、Altium の配線ルール(ルールの種類・対象の指定・順位・点検)を公式ドキュメントで調べて足した。2026-11-06 に、Altium の階層設計(シートシンボル・ポート・移動)を公式ページで調べて、階層の行を直した。比較の相手は KiCad 9、EasyEDA(Standard / Pro)、Altium Designer、Quadcept。2026-11-16 に、circuitCad の列を 11-15〜16 の実装(STEP の書き出し・GPU の 3D ビュー・部品の形のひな形・本体の重なりの DRC・データシートからの下書き・熱の解析)に合わせて直し、KiCad 10 のリリースの記事(2026-03)と EasyEDA の Standard / Pro の比較ページを読み直して、KiCad の列と新しい行を足した。2026-11-19 に、circuitCad の列を 11-16〜19 の実装(複数の基板・曲面の STEP・ケース・基板の熱の地図・試作と実機のタブ・ピンの電気的な種類の ERC・設計レビュー・解析の追加)に合わせて直した。他のソフトの列は調べ直していない。

**読み方**
- 「確認」= 下の出典の公式ページに書いてあったこと。
- 「未確認」= 今回の調べでは書いてあるページに行き着かなかった、または機能紹介の文だけで細部が分からないもの。「無い」という意味ではない。
- circuitCad の列は `CLAUDE.md` と `docs/candidates.md` の記載による。
- 機能紹介は各社の宣伝が中心。何かを判断に使うときは、リンク先の最新の説明を読み直すこと。

## 比較表

| 機能 | circuitCad | KiCad 9 / 10 | EasyEDA | Altium Designer | Quadcept |
|---|---|---|---|---|---|
| 回路図 → 基板 → 製造データ | あり | 確認 | 確認 | 確認 | 確認(回路図 Circuit Designer、基板 PCB Designer/3D) |
| SPICE シミュレーション | あり(解析の種類が多い。11-16 以降に、ナイキスト・ニコルス線図・根軌跡、CMRR、合格率と Cpk、周囲の温度を振る解析、突入電流、電源の予算表、電池の持ち時間、最初の電源投入のガイド、フィルタの設計、波形を音で聴く を足した) | 確認(ngspice を回路図エディタに統合。モデル付きの記号で実行し波形表示。10 で波形の表示を改善) | Standard はあり、Pro は「2025 版で対応」と注記(確認、比較ページ)。深さは未確認 | 確認(SPICE シミュレーションの機能ページあり) | 未確認(検索では記載なし) |
| 熱の解析 | あり: 自己発熱(定常・時間変化)、過渡熱抵抗の多段(データシートの曲線に当てはめ)、基板の銅からの θ の見積もり、θjc でケースと基板に分ける、温度の上限の点検、周囲の温度を振る最悪ケース、基板の熱の地図(升目の熱伝導と対流・放射、時間の変化、配線そのものの発熱、熱い部品どうしを離す候補)、閉じたケースの中の温度、3D の温度の色 | 未確認(記事に記載なし) | 未確認 | 未確認 | 未確認 |
| データシートから部品を作る | あり: PDF から下書き(パッケージ名・ピンの表・ピン配置の図・推奨ランドパターン・QFN の放熱パッド・絶対最大定格・熱抵抗・推奨の周辺部品・グラフの曲線)、スキャンした PDF や絵の文字は Windows の OCR で読む、表とグラフの値からモデルの当てはめ(MOSFET・トランジスタ・ダーリントン・フォトカプラ・オペアンプ・コンパレータ・レギュレータ)、電圧・パッケージ違いの型番をまとめて作る、型番の一覧の表から電解コンデンサ・パワーインダクタ | ピンの表を CSV で書き出し・読み込み(10、確認) | パッケージのウィザード(両版)。Pro は IPC-7351B の名前と 3D モデルを作る機能を「2025 版で対応」と注記(確認) | 未確認 | 未確認 |
| 回路図の階層設計 | あり(2026-11 に実装、設計は `docs/hierarchy-design.md`)。シートの記号(`sub`)とポート(In/Out/Io)で、ポートが記号のピンになる。同じ子シートを何回も置け、中を直すと全部に効く(部品番号は `R1_CH1` 形式で固定)。記号のダブルクリックで中へ入り、「親へ戻る」で戻る。コピーごとに値を変える表あり。タブに(親)(子)の目印、記号は青い塗り、ピン上のポートは部品の移動・回転に付いてくる。ERC は、自分を含む記号・ポートの無い子シート・同名ポート・つながらないピンとポート・出力のポートどうしが同じネットにつながっていること(Error)・入力のポートに信号を出すものがつながっていないこと(Warning。電源・信号源・ICの部品・別の記号の出力のポートを、抵抗やコイルを通った先まで探す)を点検する。ピンの電気的な種類(`pinkind.rs`、2026-11-17〜)を持つようになり、ERC がピンの入出力も使う。無いもの: 記号から子シートを作る機能とポートの再同期・クロスリファレンス・階層の木の一覧。ネットラベルと電源記号は全シート共通 | 未確認(階層シートは一般に知られた機能だが、今回は公式ページで確認していない) | 未確認 | 確認(公式ページ): シートシンボル(親)のシートエントリと子シートのポートが対で、ポート名で接続が決まる。ポートに I/O タイプがあり、ERC で互換性のない向きを検出できる。シンボルから子シートを作るコマンド(トップダウン)とポートの再同期。Up/Down Hierarchy・Ctrl+ダブルクリック・Navigator パネルでの移動、ポート間の Jump To、クロスリファレンスの自動表示。Net Identifier Scope(Flat / Global など)でシートをまたぐ同名ネットの扱いを選ぶ。マルチチャンネル(Repeat)の記法は今回のページで見つからず未確認。シートシンボルの既定の色は読み取れず未確認(ポートの Fill は利用者が選ぶ設定) | 未確認 |
| 回路図のバス | 一部(2026-11-04 に実装)。`D[0..7]` のラベルと太いバスの線があり、ERC が番号の抜け・範囲外・幅の食い違いを点検する。バスは何もつながない見た目で、各ピンには D0〜D7 のラベルをラベルツールの連番で付ける。バスエントリ(斜めの引き出し線)・ピン名がバスのピンは無い | 未確認 | 未確認 | あり(Altium の知識による。今回は公式ページで未確認) | 未確認 |
| 自動配線 | あり | 本体には無い(確認)。Freerouting をプラグインとして入れる形で、JRE が要る。KiCad 側は組み込む予定を当面持たない旨の古い書き込みあり | Pro は内蔵、Standard は外部プラグイン(確認) | 内蔵の自動配線(トポロジカルルーター)と、選んだネットを整えて引く ActiveRoute(確認) | セミオートルーター(確認) |
| 手で引くときの配線 | あり(押しのけ、垂直な出入り、延長線トラッキング。差動ペアは相手線も一緒に押して間隔を保つ) | 押しのけ配線の内蔵ルーター(CERN 製。確認、出典は古い講演と掲示板)。差動ペアを無視して押しのけることがあるという報告あり | 未確認 | 歩み寄り(walkaround)・寄り添い(hug)・押しのけ(push)の3モード(確認) | 未確認 |
| 差動ペア・等長配線 | あり(差動ペア、蛇行、ペア間の長さ合わせは2本を入れ子の山でいっしょに曲げて間隔を保つ。山の並べ方は1種類。ペア内の差は1本だけの蛇行で直すので間隔がずれる) | 差動ペアの配線・長さ調整あり(確認、出典は第三者の解説)。10 で調整の仕組みを作り直し、長さでなく時間(遅延)で制約を書け、層ごとの値を「Tuning Profile」にまとめる(確認、10 のリリース記事) | 未確認 | 確認: 蛇行は Accordion / Trombone / Sawtooth の3形。ペア内とペア間を別のルールで合わせる。ActiveRoute にも長さ合わせあり | 確認 |
| 配線ルールの対象の指定 | ネット名の一覧(ネットクラス)と、役割(`Role`)で決める。回路図の「設計の要件」の表に、ネットの役割・近くに置く組・離す組などを書き、基板に取り込む | 確認: ルールごとに、条件の式(`A.hasNetclass('HV')` など。ネット・ネットクラス・層・領域・部品の性質)を書く。条件を書かなければ全部に効く。ネットクラスは1つのネットに複数付けられ、順位の高いクラスから性質を取る(9.0)。10 で、カスタムルールを画面で作る「Graphical DRC Rule editor」が入った(確認) | 確認(Pro): ルールを「ネット」「ネットクラス」「層」「領域(制約領域)」「部品」「銅ベタ」「差動ペア」などに割り当てる(ファイル形式の説明より)。ネット画面で、ネットごとにルールを選ぶか、ネットクラスを作って割り当てる。2つのネット(クラス)の間のルールも決められる。製造先に合わせたプリセット(JLCPCB など)に切り替えられる | 確認: ルールごとに、ネット・ネットクラス・層などを選ぶか、問い合わせの式(Query Language)を自分で書く。PCB Filter パネルで対象を絞って、そこからルールを作れる。ルール中心の「Rules and Constraints Editor」と、ネットを表のように並べて決める「Constraint Manager」の2つの入口がある | 確認: 「詳細設定」で値を作り、「アサイン」でネットクラスに割り当てる(2段階)。クラス間のクリアランスも個別に決められる(クラスの値より優先)。基板の一部だけ別のルールにする「デザインルール領域」(多角形・矩形・円。クリアランス・配線・ティアドロップ・ベタ接続。中に完全に収まった物にだけ効く) |
| ルールの優先順位・まとまり | クラスごとの値と基板全体の下限(`Rules`)。同じ種類のルールが重なったときに順位で決める仕組みはない。ネットクラスに配線の順番(`order`)はある | 確認: カスタムルールは、ファイルの後ろのものから調べて、最初に合ったものを使う(細かいルールを後ろに書く)。基板全体の最小値(Constraints)は、ネットクラスやカスタムルールでは下回れない | 確認(Pro): ルールは種類ごとに名前を付けて複数作り、1つを「既定」にする(ネットごとに選ばなければ既定)。同じ種類で同じ名前は不可。順位の仕組みは未確認 | 確認: 同じ種類のルールが重なったら、順位の高いものから対象に合うものを1つ選ぶ(広い対象は低く、細かい対象は高く)。順位は種類の中でだけ競う(例外: Matched Lengths は全部が適用される)。ルールセット(インターフェースや製造先ごとにまとめて、切替・複製・書き出し・読み込み)はベータ機能 | 確認: ネットクラスの値より、クラス間クリアランスが優先。デザインルール領域の中では、その領域のルールが使われる。順位付けの一般的な仕組みは未確認 |
| ルールの種類の数 | 基板全体: 最小線幅・間隔・基板端・ドリル・アニュラリング・マスク拡張。ネットごと: 線幅・間隔・ビア・インピーダンス・順番・ビアの可否・使う層・長さの上限・太さの例外・仕上げ。ほかに、バイパスの近さ・鋭角・電流と電圧降下・差動ペア・組の広がりと離す距離を点検する | 確認: 29 種類の制約(annular_width・assertion・clearance・creepage・connection_width・courtyard_clearance・diff_pair_gap・diff_pair_uncoupled・disallow・edge_clearance・hole_clearance・hole_size・hole_to_hole・length・min_resolved_spokes・physical_clearance・physical_hole_clearance・silk_clearance・skew・text_height・text_thickness・thermal_relief_gap・thermal_spoke_width・track_angle・track_segment_length・track_width・via_count・via_diameter・zone_connection)。`assertion` は任意の真偽の式 | 確認(Pro): 安全間隔(ネット間の表)・その他の間隔(部品どうし)・配線幅(最小・既定・最大)・ネット長・ブラインド/ベリードビア・ビアのサイズ・内層のプレーン・銅ベタ(間隔・外形からの距離・パッドの接続・スポーク)・ペーストマスク拡張・ソルダーマスク拡張 | 確認: 65 種類を 10 の分類(電気・配線・SMT・マスク・プレーン・テストポイント・製造・高速信号・配置・信号の完全性)に分ける | 確認: DRC の項目 20 以上(未配線・クリアランス・部品クリアランス・ビア規則・配線幅・配線長/等長・差動ペア・円弧配線・ダングリング・浮きビア/スタブビア/ドリル重複・スタックビア・配線角度・ティアドロップ・パッドオンビア・ベタ分断・浮きベタ・禁止領域・サーマル・テストランド など)。項目ごとに実行するかと、エラーか警告かを選ぶ。製造の観点の MRC は別 |
| 高速信号のルール | 長さの上限(`max_length`)、ビア数の上限(`max_vias`、2026-11-05)、差動ペアと蛇行、インピーダンス(層構成から線幅を出す)がある。並走の長さ・スタブ・戻り電流の経路の上限はない(並走は `quality.rs` で数字を出すだけで、ルールではない) | 確認: length(最小・最大)、skew(ペア内も指定可)、diff_pair_gap、diff_pair_uncoupled(ペアが離れてよい長さ)、via_count(ネットあたり)、track_angle、track_segment_length。長さ調整の既定値も設定画面にある。並走・スタブ・戻り経路・トポロジは見当たらない | 確認(Pro): ネット長のルールは、DRC の制約ではなく、配線中の表示(範囲内は緑、外は赤)。差動ペアの管理・等長グループ・パッドペアのグループ・信号タイミングの等長(許容差を ps で)・xSignals の画面がある。ルールの種類としての並走・スタブ・ビア数は未確認 | 確認: Parallel Segment(同じ層・隣の層で並走してよい長さ)、Length、Matched Lengths(許容差・ペア間とペア内を別に)、Daisy Chain Stub Length、Max Via Stub Length(バックドリル)、Maximum Via Count、Return Path(基準層の連続)、Vias Under SMD。信号の完全性の分類には、インピーダンス・オーバーシュート・飛行時間などもある | 確認: 配線長と許容差(等長配線)、差動ペアのギャップ・配線長・許容差を DRC で点検。差動ペアの配線長チューニングあり。並走・スタブ・戻り経路は未確認 |
| 配線ルールの点検のタイミング | 「DRC を実行」で点検し、ファイルを開いたときにも実行する。自動配線と押しのけ配線は、ネットクラスの値を使って引く | 確認: DRC を実行して点検する。重大度は違反の種類ごとに設定(エラー・警告・無視)。カスタムルールの重大度に「無視」「除外」もある。インタラクティブルーターは、エラー・警告のルールを守って引く(「無視」は守らない) | 確認(Pro): 「DRC をかける」と「DRC(カスタム)」のメニューがある。配線中の自動チェックの有無は未確認 | 確認: オンライン DRC(編集中に新しい違反を見つける)とバッチ DRC(いつでも全部を点検)。ルールの種類ごとにどちらで動かすかを選ぶ | 確認: オンライン DRC(配線中にリアルタイムで点検。同ネットのクリアランスは対象外)と、実行する DRC。項目ごとにエラー・警告を選べる |
| 発注前の点検(DFM) | あり(`fab.rs`) | 未確認 | 未確認 | 未確認 | DFM チェック(確認) |
| 製造データの形式 | Gerber / Excellon / BOM / 配置CSV / 組み立て表(はんだ付けの順・向きの印の SVG) | Gerber・配置リストなど(確認) | 未確認 | 未確認 | ODB++ に対応(確認)。circuitCad は対象外 |
| 部品店との連携 | 購入リスト(LCSC の型番14件) | 未確認 | LCSC の部品に直接リンク、基板製造と一体(確認。JLCPCB・LCSC・EasyEDA は同じ会社グループ) | 未確認(Octopart 系のページはある) | 未確認 |
| 3D モデルの取り込み | STEP と OBJ は部品1個ずつ(部品フォーム)。SolidCraft があれば STEP の曲面をその三角形で読む | 3D 出力あり(確認)。公式ライブラリの 3D は 10 から STEP だけを配る(確認)。取り込みの形式の一覧は未確認 | OBJ・WRL は両版、STEP は Pro だけ。細かさの調整は Pro(確認) | STEP の双方向(確認) | 3D 機能あり。取り込み形式は未確認 |
| STEP の無い部品の 3D の形 | あり: 寸法を入れるひな形(DC ジャック・タクト・リレー・端子台)を外部の SolidCraft で立体に、外形図を下絵にして SolidCraft で作る、部品の種類ごとの推測の形(電池ホルダ・水晶・スイッチ など) | 公式ライブラリは、フットプリントと 3D を同じデータから作る生成器を持つ(確認。利用者向けの機能かは未確認) | Pro の「パッケージのウィザードで 3D モデルを作る」は 2025 版で対応と注記(確認) | 未確認 | 未確認 |
| 3D ビュー | GPU で描く(陰・白い見た目)、ポインタの下と選択の縁、回路図・基板と連動、自己発熱と基板の熱の地図の温度の色、複数の基板を重ねて表示 | 3D ビューあり(確認)。細部は未確認 | 未確認 | 未確認 | 3D あり(確認)。細部は未確認 |
| 部品の本体どうしの干渉 | DRC と自動配置が、3D の形かデータシートの本体の大きさで重なりを見る(2026-11-16) | 未確認(コートヤードの点検は知識。今回の出典には無い) | 未確認 | 未確認 | 部品クリアランスの DRC(確認) |
| 3D の書き出し | STEP(基板と部品の立体。2026-11-16 に曲面のまま・部品ごとの組み立てに。E3b)、OBJ | 3D 出力あり(確認)。9 で STEP 出力の改善(プレリリース記事の記述) | 未確認 | STEP / Parasolid / 3D PDF(確認) | 未確認 |
| ケース(機械設計との往復) | ケースの STEP を読んで基板に重ね、ぶつかる部品を出す(E4)。ねじ柱と壁の穴(E2)。ケースの外形と穴の DXF の読み書き。閉じたケースの中の温度(H10) | 未確認 | 未確認 | STEP の双方向と、機械設計ツールとの同期 MCAD CoDesigner(確認) | 未確認 |
| 複数基板(マルチボード) | あり(E5、2026-11-16): プロジェクト(`.ccproj` 形式 2)に回路図と基板の組をいくつも持つ、基板どうしのつなぎの照合、3D で重ねてぶつかりと置き方を見る、全体のシミュレーションと全体の部品表 | 未確認 | 未確認 | あり: 接続の照合(Connection Manager)、3D の嵌合(mating)、システムBOM(確認) | 未確認 |
| 共同作業 | なし | 未確認(Git 運用は一般的だが今回は確認していない) | チーム・権限・ライブラリ共有は両版。ロック・チェックイン/アウトは Pro。リアルタイム共同編集は「Pro の 2025 版で対応」と注記(確認) | 確認(バージョン管理・権限は競合の記述。公式の詳細は未確認) | クラウドベース(確認) |
| 無償利用 | 無償 | 無償・オープンソース(確認) | 未確認 | 有償 | 非商用に限り無償(パッド数・層数に制限あり。確認) |
| 初学者向けの学習機能 | あり(レッスン・チャレンジ・ホバー・学習パス、デジタル回路を動かしながら見るモード、選んだ部品の役割の説明) | 公式には見当たらず。コミュニティの教材が多い(確認) | 未確認 | 未確認 | 未確認 |
| 試作(ブレッドボード) | あり(試作のタブ): 回路図から差す穴を自動で並べて照合、差したとおりの回路が実時間で動く(スイッチ・ボリューム・ブザーの音・定格を超えると焦げる)、テスターとオシロ、Arduino のスケッチを一緒に動かす、ストリップ基板・ユニバーサル基板の並べ方 | 未確認(調べた範囲で記載なし) | 未確認 | 未確認 | 未確認 |
| 届いた基板を動かす | あり(実機のタブ): 組み立て → 最初の電源投入(電流制限の値) → 測る(テスターの実測を予測と比べる) → 合否 → 修理 の手順と、光る基板の配置図 | 未確認(調べた範囲で記載なし) | 未確認 | 未確認 | 未確認 |
| 設計レビューの点検 | あり: 推奨の周辺部品の付け忘れ・水晶の負荷容量・出力に直接の LED・電源の入口を点検(`review.rs`) | 未確認 | 未確認 | 未確認 | 未確認 |
| CLI / AI からの操作 | あり(`ccad`、MCP) | 未確認 | EasyEDA 向けの MCP サーバーが第三者から出ている(確認。公式ではない) | 未確認 | 未確認 |
| バリアント(1 つの回路図から部品表を作り分ける) | なし(対象外) | 10 で対応(確認) | 未確認 | あり(知識) | 未確認 |
| ピン・ゲートの入れ替え | ロジック IC のゲートと入力どうしを、配置のあとにラッツネストが短くなるよう自動で入れ替え(`swap.rs`) | 10 で、制限なしのピン・ゲートの入れ替えと回路図への反映(確認。入れ替えてよい組の規則は今後) | 未確認 | 未確認 | 未確認 |
| 回路図の見た目と操作 | 暗い画面だけ。交差の飛び越し表示なし | 10 で、つながっていない線の交差を半円で描く(hop-over)、ドラッグ中の接続の変化の予告、回路図のグループ、投げ縄の選択、ツールバーの並べ替え、Windows の暗い画面(システムに合わせる)(確認) | 未確認 | 未確認 | 未確認 |
| 他のソフトの設計の取り込み | KiCad の回路図、LTspice の回路図(`.asc`)、KiCad の記号・フットプリント | 10 で Allegro・PADS・gEDA / Lepton の基板の取り込み(確認)。Altium・EAGLE などは以前から(知識) | 未確認 | 未確認 | 未確認 |

## 分かったこと

**circuitCad が強いところ**
- 回路図・シミュレーション・基板が1本で、解析が多い(過渡・AC・ノイズ・極零・ループゲイン・最悪ケース・モンテカルロ・故障・自己発熱)。KiCad は ngspice を統合しているが、解析ごとの専用画面や測定は circuitCad のほうが多い、という見立て(KiCad 側の細部は未確認)。
- 学習機能と日本語UI。調べた範囲で、公式の機能として持っているものは見つからなかった。
- CLI と MCP で、AI が回路を作って検証できる。
- 試作(ブレッドボードで動かす)と、届いた基板を組み立てて動かすまでの手順(実機のタブ)。調べた範囲で、他のソフトは基板の設計と製造データで終わる。「回路図 → 試作 → 基板 → 発注 → 組み立てて動かす」が1本でつながるのは circuitCad だけ(2026-11-19 時点、他のソフトの列は調べ直していない)。
- 熱の解析(基板の熱の地図・ケースの中の温度まで)とデータシートから部品を作る流れ(グラフ・OCR・型番違い)は、調べた範囲では他のソフトの公式の機能に見当たらない(他のソフトの細部は未確認)。

**埋まった差(2026-11-16〜19)**
- 3D と STEP: 曲面のままの STEP の書き出しと部品ごとの組み立て(E3b)、ケースの STEP を重ねてぶつかりを見る(E4)、ねじ柱と壁の穴(E2)。残るのは Altium の MCAD CoDesigner のような機械設計ツールとの同期。
- マルチボード: Altium の「接続の照合 → 3D で嵌合 → システム全体の部品表」の3本立てに、全体のシミュレーションまで持つ(E5)。
- 階層設計の向きの照合: ピンの電気的な種類を持ち、ERC がピンの入出力を使うようになった。

**差が出ているところ**
- KiCad 10 で入ったもの(バリアント・交差の飛び越し表示・時間での等長・画面でのルール作成・ピンの入れ替えの反映)のうち、初学者向けに効きそうなのは交差の飛び越し表示(回路図が読みやすい)。候補は `candidates.md` の J23。
- 発注との一体感。EasyEDA は部品店と基板製造まで同じ会社グループ。circuitCad は購入リストまで(A2)。
- 共同作業。EasyEDA は権限・ロック・チェックイン/アウトまである。circuitCad は個人利用の前提。
- 高速信号のルール(並走・スタブ・戻り経路)と、ルールの対象を式で書く自由度・順位付け(Altium・KiCad)。初学者向けの2層基板ではほぼ要らない。
- Windows だけ(KiCad・EasyEDA は Mac・Linux でも動く)。
- 回路図の階層設計とバス。階層設計は 2026-11 に入った(シート記号・ポート・同じ子シートの繰り返し)。Altium(公式ページで確認)と比べて足りないのは、記号から子シートを作る機能とポートの再同期・クロスリファレンスとジャンプ・階層の木の一覧・ネット名の範囲の切り替え。バスは、ラベルと線と点検だけを実装した(2026-11-04)。バスの Altium 側は知識によるもので、公式ページでは未確認。

**ここから言える次の手**
- 3D・ケース・マルチボードの大きな差は埋まった(2026-11-19)。「初学者が基板を発注して動かすまで」で残る差は **発注との一体感(A2 の型番確認、J5 の在庫・値段)** と、回路図の読みやすさ(J23 の交差の飛び越し)。
- 共同作業は、個人向けの方針とは別の話なので、今は候補にしない。

## 配線ルール: Altium との対応(2026-11-05、公式ドキュメントで確認)
Altium の分類ごとに、circuitCad で近いものを挙げる。「なし」は、コードと `docs/candidates.md` の記載で見つからなかったもの。

| Altium のルール(分類) | circuitCad |
|---|---|
| 配線: Width(最小・推奨・最大。層ごとの値、インピーダンスの設定から) | ネットクラスの `track_width` と `impedance`(層構成から線幅)。最小・最大の幅は基板全体の `min_track_width` のみ |
| 配線: Routing Neck-Down(細く引いてよい合計の長さの上限。BGA の引き出しなど) | `width_exceptions`(太さの例外の区間)と、細い配線の合計の長さの上限 `max_neck`(2026-11-05) |
| 配線: Routing Topology(最短・デイジーチェーン・スター など) | なし(ネットは最短でつなぐ) |
| 配線: Routing Priority・Routing Layers・Routing Via Style | 順番 `order`・使う層 `layers`・ビアの径とドリルと可否 |
| 配線: Routing Corners・Fanout Control・Differential Pairs Routing | 仕上げ(角丸め・涙型。`smooth.rs`)・出口の線 `breakout.rs`・差動ペア `diffpair.rs` |
| 電気: Clearance | 基板全体の最小間隔とクラスごとの間隔。2つのクラスの間は、大きいほう(`clearance_between`)。条件式で「AとBの間だけ」とは書けない |
| 電気: Short-Circuit・Un-Routed Net・Un-Connected Pin | DRC のショート・ラッツネスト(未配線)・ERC の未接続 |
| 電気: Creepage Distance・Z-Axis Clearance・Modified Polygon | なし |
| 製造: Hole Size・Minimum Annular Ring・Board Outline Clearance・Acute Angle・Silk To Silk | 最小ドリル・アニュラリング・基板端・鋭角 `acute.rs`・シルクの点検あり。Net Antennae・Layer Pairs はなし |
| 高速信号: Length・Matched Lengths | 長さの上限と蛇行、差動ペアの長さ合わせ |
| 高速信号: Maximum Via Count | ビア数の上限 `max_vias`(2026-11-05) |
| 高速信号: Parallel Segment・Daisy Chain Stub・Max Via Stub・Return Path・Vias Under SMD | なし |
| 信号の完全性: Impedance | 目標インピーダンスの点検あり(発注前チェック)。Overshoot・Undershoot・Flight Time・Supply Nets はなし |
| プレーン: Polygon Connect Style・Power Plane Clearance・Power Plane Connect | ベタの接続・島・細いかけらの点検あり(`drc/pour.rs`)。内層のプレーン専用のルールはなし |
| SMT・マスク・テストポイント・配置の分類 | マスク拡張・サーマルビアの点検あり。それ以外の SMT・テストポイントの専用ルールはなし。配置は、設計の要件の「近くに置く組」「離す組」で近い |

**見立て**
- 値の種類は、Altium の「線幅・間隔・ビア・層・長さ・インピーダンス」の中心部分は持っている。差が大きいのは、**高速信号の専用ルール(並走・スタブ・ビア数・戻り経路)** と、**ルールの対象を式で書く自由度と順位付け** の 2 点。
- 初学者向けの目的(2層の小さな基板)では、前者はほぼ要らない。後者は、「クラスの値」+「役割」で足りる範囲に収めている。
- 次に足すなら、小さくて効果がありそうなのは、Parallel Segment(並走の長さの上限。`quality.rs` の数字をルールにする)、Routing Topology のうちデイジーチェーン。優先順位とルールセットは大きく、今の使い方では必要になっていない。

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

## ファイル互換性(2026-11-08、コードと `docs/library.md` による。他のソフトで実際に開いて確かめたものではない)

circuitCad の列だけを書く。他のソフトが何を読み書きできるかは、今回は調べていない。

| 形式 | 取り込み | 書き出し |
|---|---|---|
| KiCad のシンボル(`.kicad_sym`、旧形式 `.lib`) | あり(部品作成フォーム。ピンだけ取り、記号は四角い形) | なし |
| KiCad のフットプリント(`.kicad_mod`) | あり(パッド・穴・長穴) | なし |
| Ultra Librarian・SnapEDA の zip(KiCad 形式) | あり(シンボル・フットプリント・STEP を自動で拾う) | - |
| メーカーの SPICE モデル(村田など) | 一部(読める素子と書き方に制限。`.include`・XSPICE の多くは不可) | - |
| SPICE ネットリスト(`.cir`) | なし | あり(ngspice の方言) |
| STEP(3D) | 部品1個ずつ(SolidCraft があればその三角形で)と、ケースの STEP(基板に重ねてぶつかりを見る。`pcb-core/case_model.rs`) | あり(基板と部品、曲面のまま・部品ごとの組み立て。`pcb-core/step_out.rs`) |
| OBJ(3D の三角形、色の MTL) | 部品1個ずつ | あり(`pcb-core/mesh_io.rs`) |
| Gerber・Excellon ドリル・BOM・部品配置(CSV) | - | あり |
| 回路図の SVG | - | あり(2026-11-08) |
| KiCad の回路図(`.kicad_sch`) | あり(部品とネットを取り出して組み立てる。`part-lib/kicad_sch.rs`・`kicad_import.rs`) | なし |
| LTspice の回路図(`.asc`) | あり(2026-11-09、`part-lib/ltspice*.rs`) | なし |
| KiCad の基板(`.kicad_pcb`)、EAGLE、Altium、EasyEDA | なし | なし |
| DXF | あり(基板の外形と穴をケースの図面から取る。`pcb-core/dxf_in.rs`) | あり(外形・穴・パッドのドリル・部品の箱。`pcb-core/dxf.rs`)。どちらもケースの CAD で実際に開く確認はまだ |
| 基板の SVG | - | あり(1つの面を Gerber から描く。`pcb-core/svg.rs`) |
| ODB++・Specctra(Freerouting 用) | なし | なし |

- 部品(ライブラリ)の互換性は、KiCad と SnapEDA・Ultra Librarian の資産を使える範囲にある。ただし記号の絵は取り込めない。
- 設計データ(回路図・基板)は、自前の形式(`.schjson` / `.pcbjson` / `.ccproj`)に閉じている。外へ出るのは製造データ・SPICE ネットリスト・SVG・DXF・STEP・OBJ だけ。入ってくるのは KiCad と LTspice の回路図とケースの DXF。
- 候補は `docs/candidates.md` の「ファイル互換性」。

## 未確認として残したもの(次に調べるなら)
- 配線ルール: KiCad の Board Setup の Constraints の各項目の細部、KiCad のカスタムルールの条件に使える性質の一覧、EasyEDA の差動ペアのルールの細部と優先順位、EasyEDA Standard 版のルール、Quadcept のクラス間クリアランスや配線長の設定の細部。
- KiCad: 配線機能の公式の説明(今回は公式マニュアルの PCB エディタの章が取れず、掲示板・第三者の解説・古い講演に頼った)、STEP の取り込み、DFM に近いチェック。
- Altium: SPICE の解析の種類、ActiveRoute の使い方の細部(ビアを置けない、電源ネットの戦略が無い、という制限は ActiveRoute の改善点の説明より)。
- EasyEDA: シミュレーションの種類と深さ(Pro の公式ドキュメント)。
- Quadcept: シミュレーション、STEP の入出力(公式の機能一覧)。

## 出典
- KiCad 9.0 紹介: https://docs.kicad.org/9.0/en/introduction/introduction.html
- KiCad 10.0 のリリース(2026-03、2026-11-16 に読んだ): https://www.kicad.org/blog/2026/03/Version-10.0.0-Released/
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
- Altium のマルチシートおよび階層設計(日本語版): https://www.altium.com/jp/documentation/altium-designer/schematic/multi-sheet-hierarchical-designs
