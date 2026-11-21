# ADC の等価モデル

ライブラリの ADC の、データシートから作った等価モデルの記録 (`docs/model-policy.md` の 3 章)。

- 変換 (入力の電圧をコードに変えること) は扱わない。電源の電流と、入力・基準電圧の入力に流れる電流だけ。回路図の DC と、試作のタブの「組む順と通電」・電源の予算表で、ADC の分の電流が 0 にならないようにするため (候補 L1)。
- 設定 (PGA の利得・基準バッファ・内部基準・IDAC) で電流が変わる部品は、**リセット直後の設定で変換しているとき**の値にした。違う設定の値は `sim.limits` に並べた。

## ADS124S08

データシート: TI ADS124S0x、SBAS660C (2016-08、2017-06 改訂)。7.5 節 Electrical Characteristics と 9.6.1.6 節 (Reference Control Register、リセット値 10h)。条件は AVDD = 3.3 V、DVDD = IOVDD = 3.3 V、外部基準、内部発振、25 °C。

| モデルの部分 | 値 | どこから |
|---|---|---|
| AVDD の電流 | 155 µA | 7.5 節「ANALOG SUPPLY CURRENT」の Conversion mode, PGA enabled, gain = 1, 2 が typ 120 µA。9.6.1.6 節で REFP_BUF のリセット値が 0 (有効)、REFN_BUF が 1 (無効) なので、「ADDITIONAL ANALOG SUPPLY CURRENTS」の Positive reference buffer typ 35 µA を足した |
| AVDD の電流の立ち上がり | 2 V を中心に幅 0.5 V | データシートは 2.7 V より下を書いていない。電源の入っていないレールを負にしないための形 (値の根拠ではない) |
| DVDD と IOVDD の電流 | 225 µA | 7.5 節「DIGITAL SUPPLY CURRENT」の Conversion mode, internal oscillator が typ 225 µA (IDVDD と IIOVDD をまとめた行)。DVDD 側に置き、IOVDD は 1 GΩ |
| アナログ入力 (AIN0〜11・AINCOM) | 20 GΩ を AVSS へ | 7.5 節「ANALOG INPUTS」の Absolute input current, PGA enabled が typ 0.1 nA (最大 ±2 nA)。中点 (3.3 V の半分くらい) で 0.1 nA になる抵抗。電流の向きは表に無い |
| REFP0 | 500 MΩ を AVSS へ | 7.5 節「VOLTAGE REFERENCE INPUTS」の Reference buffers enabled, VREF = 2.5 V が typ 5 nA。2.5 V で 5 nA |
| REFN0 | 250 kΩ を AVSS へ | 同じ表の Reference buffers disabled が typ 4 µA/V。REFN0 はふつう AVSS のそばにあるので、この抵抗にはほとんど電流が流れない |
| REFOUT | 1 GΩ | 9.6.1.6 節で REFCON のリセット値が 00 (内部基準オフ) |

### 誤差の表

| 項目 | データシート (typ) | モデル | 差 |
|---|---|---|---|
| AVDD の電流 (変換中・PGA 利得 1〜2・正の基準バッファ有効) | 120 + 35 µA | 155 µA | 0 |
| DVDD + IOVDD の電流 (変換中・内部発振) | 225 µA | 225 µA | 0 |
| REFP0 の入力電流 (バッファ有効、2.5 V) | 5 nA | 5 nA | 0 |
| アナログ入力の電流 (PGA 有効、1.65 V) | 0.1 nA | 0.08 nA | 向きは扱わない |

値はテスト `sim-core/src/tests_parts_power.rs` の `the_ads124s08_draws_its_supply_currents` で確かめている。

### 再現していないこと

変換、SPI、内部の 2.5 V 基準 (リセット時はオフ)、IDAC と VBIAS、利得ごとの電流の違い (`sim.limits` に値を並べた)、入力電流の向き・温度による変化、AVDD が 2.7 V より下の振る舞い。

## まだモデルの無い部品 (2026-11-21)

- Nexperia BAV199 (低リークの 2 連ダイオード) と Panasonic TQ2-L2-5V (2 コイルのラッチリレー): データシートの PDF が自動では取れなかった (Nexperia・Panasonic のサイトが拒否する)。製品のページから分かったのは BAV199 の漏れ電流 typ 3 pA・逆電圧 最大 75 V・切り替え typ 0.8 µs だけで、モデルに要る順方向の電圧の表 (Vf と If) と、TQ2-L2-5V のコイルの電流・抵抗が無い。データシートの PDF が手に入ったら作る。
