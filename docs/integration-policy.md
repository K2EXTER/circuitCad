# 外部のソフトとの連携の方針

2026-11-15 に決めた。FPGA・ファームウェア・RF・高速デジタル・ASIC の候補(`docs/candidates.md` の R・S・F・W 節と「対象外にしたもの」)を書いたときの議論から。

## 何を自分で持ち、何を外に任せるか

**外にすでに良いものがあり、作り直しても勝てないものは、連携に任せる。** circuitCad の価値は、初学者が回路図 → シミュレーション → 基板 → 発注を 1 本の流れでたどれることと、その途中の日本語の説明・ヒント・点検にある。そこは自分で持ち、専門の道具の中身は作らない。

| 自分で持つ | 連携に任せる |
|---|---|
| 回路図・シミュレーション・基板・発注までの流れと、日本語の説明・ヒント・サンプル | FPGA の合成・配置配線(yosys・nextpnr・メーカーのツール) |
| 正本のデータ(部品・ピン・ネット・基板)と、それを使う点検(ERC・DRC・回路図とファームの照合 W2) | ファームのビルドと書き込み(arduino-cli・Pico SDK・openFPGALoader) |
| よく使う解析(DC・過渡・AC・ノイズ・自己発熱 …)を、外部のソフトなしで動かすこと | HDL・マイコンの命令のシミュレーション(Verilator・Icarus Verilog・simavr)。波形を読み込んで重ねる |
| 外部のソフトに渡すファイルを作ることと、戻ってきたファイルを読むこと | ASIC 全体(OpenROAD・Magic・KLayout、オープンな PDK) |
| | 3 次元の電磁界の解析(アンテナ・放射・ビアやコネクタの細かいモデル) |
| | 部品の 3D の形を作る CAD(押し出し・回転・面取り・ブーリアン)。SolidCraft など |
| | 検算(ngspice) |

初学者に「まず○○を入れてください」と言うと、そこで止まる。**よく使う機能は、外部のソフトを入れなくても動くこと**を守る。外部のソフトが要るのは、中級者以上が使う機能と、検算だけにする。

## 連携のしかたの決まり

1. **ファイルでつなぐのを先にする。** 制約ファイル(.pcf・.xdc・.qsf・.lpf)、VCD、Touchstone(.sNp)、IBIS(.ibs)、SPICE のネットリスト、Gerber・Excellon・DXF・STEP のような決まった形式でつなぐ。外部のソフトの版が変わっても壊れにくく、外部のソフトなしでテストできる(ファイルの読み書きの試験は、手で書いた小さなファイルで行う)。
2. **外部のソフトを呼ぶときは、無くても困らないようにする。** 見つからなければ、その機能だけ使えなくし、何が無くてどう入れるかを日本語で出す(原因と対処が分かるエラーメッセージの規約どおり)。ほかの機能は止めない。外部のソフトの場所は、設定で指定でき、無ければ PATH から探す。開発者の手元のパスを決め打ちしない。
3. **同梱はライセンスを確かめてから。** GPL などのツールは、別の実行ファイルとして呼ぶだけにして、circuitCad の中に組み込まない(リンクしない)。同梱するものは `THIRD_PARTY_LICENSES.txt` に入れる(`docs/release-process.md`)。メーカーのモデルの扱いは `docs/model-policy.md`。
4. **CLI と MCP で同じことができるようにする。** 外部のツールを呼ぶ処理も `ccad` の命令にし、画面のボタンはそれを呼ぶ。スクリプトや GitHub Actions で同じ流れを回せる。
5. **正本は circuitCad の側に 1 つ。** ピンの割り当てなど、両方で持つ情報は回路図と部品のピンの表を正本にし、外部のソフト向けのファイルはそこから作る生成物にする。生成するのは専用のファイルか、目印で囲んだ区画だけで、手で書いた部分は消さない。外で直されたら、読み込んで食い違いを指摘する(上書きで黙って揃えない)。
6. **外部のソフトの結果を、こちらの画面の言葉で見せる。** 読み込んだ波形は今の波形表示に重ね、外部のツールのエラーは窓に出しつつ、よくある原因はヒントに言い換える。

## 新しいソフトに分けるかどうか

- データの形が違う、または使う人が違うものは、別のソフトにする(ASIC がこれ。作るとしても circuitCad の中には入れない。今は作らない)。
- 同じデータ(ピン・ネット・基板)を使うものは、circuitCad の中に作る(FPGA の外側: F 節、ファームとの連携: W 節)。
- 迷ったら、まずこのリポジトリの中に新しいクレートとして作る(例: `crates/fpga`)。部品・シミュレータ・データシートのクレートを共有でき、大きくなってから別のリポジトリに切り出せる。

## 今の連携

| 相手 | 形 | 場所 |
|---|---|---|
| ngspice | 呼ぶ(検算。無ければ検算だけ使えない) | `app/src/xcheck*.rs`、`tools/sim_compare.py`。探す順は `sch_core::find_ngspice`: `--ngspice EXE` → 環境変数 `NGSPICE` → PATH の `ngspice_con`・`ngspice` → `C:\Spice64\bin\ngspice_con.exe`(Windows の zip を展開する既定の場所)。見つからないときの案内は `NGSPICE_MISSING`。ngspice の例のファイルを読む試験は `ngspice_examples()`(無ければ飛ばす) |
| KiCad(記号・フットプリント・回路図) | ファイルを読む | `part-lib/src/kicad*.rs` |
| LTspice(回路図 `.asc`) | ファイルを読む | `part-lib/src/ltspice*.rs` |
| メーカーの SPICE モデル | ファイルを読む(zip の索引も) | `sim-core/src/spice_in.rs`・`spice_flat.rs`、`docs/model-policy.md` |
| 基板の製造 | ファイルを書く(Gerber・Excellon・BOM・部品の位置) | `pcb-core/src/gerber.rs` ほか |
| ケースの CAD | ファイルを読み書きする(DXF、STEP の書き出し) | `pcb-core/src/dxf*.rs`・`step.rs`・`step_out.rs` |
| SolidCraft(Rust 製のパラメトリック CAD、MIT / Apache-2.0、0.42 で確かめた) | STEP でつなぐ。部品の 3D の形を作る道具として使う(下の「部品の 3D の形」) | 呼び出しのコードは無い。`docs/library.md` の「3D の形を自分で作る」 |
| データシート(PDF) | 自前で読む(`hayro`) | `crates/datasheet` |
| AI のエージェント | MCP サーバー(`ccad mcp`) | `crates/cli` |

## これから(候補の節との対応)

- FPGA: 制約ファイルの読み書き(F1・W1・W2)、yosys・nextpnr・iceprog を呼ぶ(W4)、HDL のシミュレータの VCD を読む(F8)
- ファーム: ピン定義のヘッダ・devicetree を書く(W1)、照合(W2)、ビルドと書き込み(W4)、命令のシミュレータとの連携(W5)
- RF: Touchstone の読み書き(R3)
- 高速デジタル: IBIS を読む(S3)

## 部品の 3D の形(2026-11-15)

メーカーの STEP は再配布できないことが多く、同梱できない。STEP が無い部品の形は、利用者が作れる必要がある。そのための CAD を circuitCad の中に作るのはやめた: 置く・押し出す・回転体までは中くらいで作れるが、ブーリアン(形の足し算・引き算)・後からの角の丸め・拘束は大掛かりで、外にもう良いものがある。代わりに **SolidCraft** と STEP でつなぐ。

- **確かめたこと(2026-11-15、SolidCraft 0.42)**: circuitCad が書いた基板の STEP(`board step`、サンプル `rlc_board`)を SolidCraft の形状カーネル(NVIDIA の SMLib を Rust に移した SolidCore)で読むと、13 個の立体が名前つきで、全部「閉じた立体」、警告なし、基板の体積 1575.5 mm³(40 × 25 × 1.6 mm から穴を引いた値)。逆に SolidCraft の STEP(本物の円柱を含む)を circuitCad の `step::read_step` で読むと、体積の差 0.7 %(円柱を三角形に分けた分)。電解コンデンサの形を SolidCraft で作り、部品作成フォームの「3D の形」で読み込めることも確かめた。
- **台本で使える**: `solidcraft-cli run 台本.json --out 部品.step` で、コマンドを並べた JSON(`[{"command": "solid.cylinder", "params": {...}}, ...]`)から STEP ができる。MCP(`solidcraft-cli mcp`)をつなげば画面を見ながら作れるが、無くても AI がデータシートの寸法から台本を書ける。`solidcraft-cli commands` がコマンドと引数の一覧(JSON)、`solidcraft-cli eval --in 部品.step` が立体ごとの体積・閉じているか、`snapshot` が絵。
- **同梱はしない**: SolidCraft は別の実行ファイルとして使う(決まり 2・3)。無くても circuitCad は困らない(メーカーの STEP を読む、パッドからの推測の箱、のまま)。
- **部品作成フォームからつなぐ(2026-11-15)**: 「STEP が無いとき: SolidCraft で形を作る」で、データシートの外形図を切り出して縮尺を合わせ、`solidcraft-cli run 台本.json --save part.sc` で下絵(`canvas.insert`。上から見た図は XY、横から見た図は x = +X・y = +Z の面を部品の後ろに立てる。XZ の面は 2 つ目の軸が −Z で絵が逆さになるので使わない)とたたき台の箱の設計を作り、`solidcraft.exe part.sc` で開く。作業フォルダ(一時フォルダの `circuitcad_solidcraft/部品名/`)の `part.step` が書かれたら自動で読み込む。SolidCraft の場所は `SOLIDCRAFT_DIR` → PATH → `C:\Program Files\SolidCraft`。
- **SolidCraft が無くなったとき(版が若く、先行きは分からない)の備え**: つなぎ目を細く保つ。SolidCraft に特有なのは台本を作る関数 `part_form/solidcraft.rs` の `script` と、2 つの実行ファイルの名前だけ。切り出した図は普通の PNG で、作業フォルダに残るので、FreeCAD や Fusion のキャンバスにもそのまま使える。戻ってくるのは標準の STEP で、どの CAD で作っても同じ読み込み(`step::read_step`)を通る。SolidCraft の独自形式(`.sc`)は circuitCad に保存しない。止まったら、台本の部分だけを別の CAD(FreeCAD の Python マクロなど)に書き換えればよい。
- 版が若い(本人たちの説明では「簡単な部品なら使える」)。カーネルをライブラリとして組み込む(STEP の書き出しを本当の曲面にする E3b など)のは、単独のクレートとして安定してから考える。

