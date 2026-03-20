# Changelog

## [0.3.0](https://github.com/swibrow/agent-ops/compare/v0.2.0...v0.3.0) (2026-03-20)


### Features

* multi-agent support (Claude, Codex, OpenCode, Gemini, Aider) ([16ff15a](https://github.com/swibrow/agent-ops/commit/16ff15ac69af40cdfd7d08a3cc1295ed9e174020))
* multi-agent support with trait-based provider system ([8ee6250](https://github.com/swibrow/agent-ops/commit/8ee62503763c026a6bf66df18b965687741ed8f8))

## [0.2.0](https://github.com/swibrow/agent-ops/compare/v0.1.0...v0.2.0) (2026-03-19)


### Features

* add quit confirmation dialog ([e09cead](https://github.com/swibrow/agent-ops/commit/e09ceadaed528cbc0cd6a59deb40ca67ee91107e))
* integrate tab bar into ASCII header to save vertical space ([0f15152](https://github.com/swibrow/agent-ops/commit/0f151521700d5a4dc35a99b815540704055188af))
* restore subtitle with tabs and agent count on same line ([3c6a74d](https://github.com/swibrow/agent-ops/commit/3c6a74da6acefe24671c39606d44a706b069363c))
* switch to notify-rust for native macOS notifications ([c2afe3f](https://github.com/swibrow/agent-ops/commit/c2afe3fe5ea49d6b8736fc5c2a94b016764d578e))


### Bug Fixes

* filter out stale panes where Claude exited but title remains ([4240591](https://github.com/swibrow/agent-ops/commit/42405919bd293c65000daf56b25e3890fd72382a))
* sort live sessions by activity state then project name ([e0ed69e](https://github.com/swibrow/agent-ops/commit/e0ed69eb54693f7a297e43ec9bfe32cba27a8b58))
