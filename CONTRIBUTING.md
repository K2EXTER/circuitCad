# Contributing / 貢献のしかた

**English summary.** Bug reports, ideas, documentation fixes, new parts and code changes are all welcome, in English or Japanese.
- **Issues:** use the templates (bug report, confusing screen, feature request). Screenshots and the circuit file (`.schjson` / `.pcbjson`) help a lot. Reports on confusing screens or wording are especially welcome: the user interface is the area we are improving most actively.
- **English wording:** the English UI was translated with AI help. Fixes go in `crates/i18n/en/*.tsv` (one `Japanese<TAB>English` line per text; see `docs/code-notes/i18n.md`).
- **Before a pull request:** `cargo test --workspace` and `cargo clippy --workspace --all-targets` must pass (CI runs the same). Keep refactoring and new features in separate commits.
- **Parts:** the format is in `docs/part-format.md`. Do not copy vendor SPICE models or datasheet text and figures; build approximate models yourself and mark them as approximate.
- **License:** `AGPL-3.0-or-later`. By opening a pull request you offer your change under the same license.

The code map is in `CLAUDE.md`, and most design notes are in Japanese (`docs/code-notes/`).

---

## 日本語

バグ報告、提案、ドキュメントの修正、部品の追加、コードの変更、どれも歓迎します。

## まずはここから
- 困ったことやアイデアは、Issue に書いてください。画面の写真や、再現できる回路ファイル (`.schjson`) があると助かります。
- コードの構成は [CLAUDE.md](CLAUDE.md) の「コードマップ」で、どのファイルに何があるかをつかめます。

## 変更を出す前に
```bash
cargo test --workspace
cargo clippy --workspace --all-targets
```
どちらも通る状態にしてください (CI も同じことをします)。新しい解析や部品には、結果を理論値や解析解と比べるテストを付けてもらえると助かります。

## 方針
- 初学者が触って分かるかどうかを大事にしています。ボタンや入力欄にはホバーの説明を付け、エラーメッセージは原因と直し方が分かる日本語にします。
- 新しい解析を足すときは、画面での表示と、サンプル回路 (必要なら「学ぶ」のレッスン) までをセットにします。
- 動作を変えない整理 (ファイル分割など) と、機能の追加は、コミットを分けてください。

## 部品ライブラリへの追加
- 形式は [docs/part-format.md](docs/part-format.md) です。データシートの値を読み取ったら、ピン番号と寸法を必ず見直してください。
- **メーカーが配布している SPICE モデルや、データシートの文章・図をそのまま写さないでください。** 再配布を禁じている場合があります。数値は概算のモデルとして自分で組み、「概算」と明記してください。
- 値の出どころ (データシートの名前や版) を、プルリクエストの説明に書いてください。

## ライセンス
このプロジェクトは `AGPL-3.0-or-later` です (初期の `v0.1.0-alpha.7` までは `MIT OR Apache-2.0`。全文は `legacy-licenses/`)。プルリクエストを出すと、あなたの変更は同じライセンスで提供されたものとして扱います。
他人が著作権を持つコード・データ・文章を、許可なく含めないでください。
