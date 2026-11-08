# 配布の手順

依存クレートを変えたら `python tools/gen_third_party_licenses.py` で `THIRD_PARTY_LICENSES.txt` を作り直す(CIが `--check` で古さを検査、リリースのzipに同梱)。`.github/workflows/release.yml` がタグ `v*` でWindows用のzip(`circuitCad.exe` と `ccad.exe` と docs)を作り、プレリリースとして公開する(本文は `docs/release-notes/<タグ>.md` を読むので、タグを打つ前に必ず置く)(macOS/Linuxは動作確認ができるまでmatrixでコメントアウト中)。日本語フォントは `startup.rs` の `install_japanese_font` がOSごとの候補パスから読み、無ければ `assets/fonts/BIZUDGothic-Regular.ttf`(OFL)を使う。

READMEの画像(`docs/images/*.png`)は `powershell -File tools/gen_screenshots.ps1` で作り直す(`cargo build -p app` のあと。1枚だけなら `-Only waveform`。画像を増やすときはスクリプトの `$shots` に1行)。READMEの操作録画 `docs/images/demo-live.gif` は `powershell -File tools/gen_demo_live.ps1` で作り直す(リリース版・1920×1080・実際のマウスを使う。台本の座標はパネルの配置を変えるとずれる。仕組みは `tools/screenrec.ps1`)。
