# Changelog

この文書はRust再実装系列の変更を記録します。旧Python GUI系列の履歴は[docs/legacy/CHANGELOG.txt](docs/legacy/CHANGELOG.txt)に保持しています。

## 0.1.0 - Unreleased

### Added

- Rust所有のserial、camera、controller、settings、HTTP、WebSocket、WebRTC runtime
- SvelteKit 2／Svelte 5 Web UIと、single-instance、tray、native dialogを持つTauri shell
- profile単位CPython 3.14 workerと、永続Python／Lua dynamic worker
- OpenAPI、TypeScript、Python、Luaへ生成する正準contract registry
- fixed 3 baseline、103 scriptsをmanaged workerで検証する互換性runner
- Linux package、Windows NSIS installerのrelease pipelineとSHA-256 manifest
- 同梱CPython 3.14、managed uv、offline wheelhouse、Linux udev access ruleを検証するclean-install package gate
- PTY serialとV4L2 loopback cameraをnative backend経由で検証するLinux virtual I/O gate

### Changed

- ハードウェアresourceの所有権をRustへ集約し、script操作を型付きIPCへ移行
- user setting、profile、generated typings、managed venvをOS標準のConfig／Data／Cache／Stateへ分離
- legacy Tk、dialog、image、network、MQTT、LINE／Discord helperを閉じた互換surfaceとして再実装
- SvelteKit versionとDebian archive metadataを固定し、Linux release artifactをバイト単位で再現可能化
- 開発toolingとtestをacceptance、compatibility、quality、release、integrationの責務別directoryへ整理
- Linux camera captureをMMAP優先・USERPTR fallbackにし、driverが報告したframe byte数だけを復号

### Security

- wildcard bind、path traversal、late generation mutation、secretのdiagnostic出力を拒否
- worker crash、transport切断、profile切替、app終了時にcontrollerとdialogを強制cleanup
- bundled uvと互換性corpusをSHA-256で検証し、candidate昇格履歴をhash chainで保護
