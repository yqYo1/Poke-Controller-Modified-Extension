# GPUI selected-closure ライセンス資料

対象読者は開発者および実装支援エージェント。利用者向け操作説明ではない。

## 収録物

- `gpui-selected-closure-license-report.json` — 生成物。`x86_64-unknown-linux-gnu`
  かつ `pokecon` の `gpui` feature で選択される依存閉包（870 package）の
  source-based inventory。schema `gpui-selected-closure-license-report` version 1。
  上流の union lockfile ではなく、現行 source に対する `cargo metadata --locked`
  から生成する。
- `NOTICE-GPUI` — 再配布 NOTICE（英語）。MPL-2.0 の未解決 7 件、デュアルライセンスの
  選択 2 件、許諾文同梱のない permissive crate 76 件の帰属表示、Lucide／Feather
  の条件付き扱いを記す。法的助言・頒布承認ではない。
- 生成器 `scripts/licenses/generate_gpui_closure_report.py` と
  検証器 `scripts/licenses/check_gpui_closure_report.py`。

## 再生成と検証（Nix 経由）

```text
nix develop -c python3 -I scripts/licenses/generate_gpui_closure_report.py
nix develop -c python3 -I scripts/licenses/check_gpui_closure_report.py
```

検証器は現行 source から再生成して tracked report とバイト比較し、
未解決項目と選択の NOTICE 反映を検査する。不一致は fail とする。

## 状態と残件

license／notice gate は未完了。MPL-2.0 7 件の source-offer 対応、
Nix system library（fontconfig、freetype、libxkbcommon、XCB、Mesa、
Vulkan loader、Tauri 経路の GTK／WebKitGTK）の runtime 閉包確認、
法的確認が残る。Phase 1 および Gate 1 の checkbox は変更しない。
計画側の pointer は `docs/GPUI_FRONTEND_PLAN.md` の候補依存ライセンス節にある。
