# Changelog

## [0.5.0](https://github.com/swibrow/agent-ops/compare/v0.4.0...v0.5.0) (2026-07-20)


### Features

* multi-dir Claude support, completed state, and smart notifications ([a41db74](https://github.com/swibrow/agent-ops/commit/a41db741b4db5e4356fa1d933003604ea07a356a))
* pane actions, live colored preview, and tmux integration fixes ([482b461](https://github.com/swibrow/agent-ops/commit/482b4618a02003324d022522485d218cb3e03a6c))
* web UI, transcript ingest, and activity review ([faa82bf](https://github.com/swibrow/agent-ops/commit/faa82bfe4bfcddd7f7e899ec5314ceead6ddfe0a))

## [0.4.0](https://github.com/swibrow/agent-ops/compare/v0.3.0...v0.4.0) (2026-03-21)


### Features

* add CLI with clap (--version, --poll-interval, --no-notifications, --reset-db) ([92e1525](https://github.com/swibrow/agent-ops/commit/92e1525399443a97e1c5f44d3e39f352d16e7434))
* tmux window icons for agent activity and accurate last_activity tracking ([21a4c45](https://github.com/swibrow/agent-ops/commit/21a4c45f1e754563a57ddec0335309ba322c591b))


### Bug Fixes

* resolve clippy and fmt warnings from CI ([fff0640](https://github.com/swibrow/agent-ops/commit/fff0640663ae3419c7e0fe13df6244559a48ab4e))

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
