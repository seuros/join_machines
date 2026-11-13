# Changelog

## [0.4.0](https://github.com/seuros/join_machines/compare/join-machines-v0.3.0...join-machines-v0.4.0) (2026-11-13)


### ⚠ BREAKING CHANGES

* see CONVENTIONS: one exhaustive Refusal per crate, Result from every mutator, honoured idempotent repeats, Ruby mirrors the core. No compatibility aliases. advance_watermark and advance_watermark_from return Result<Advancing, Refusal>.

### Features

* watermark advances answer Advancing or WatermarkRegressed, Ruby takes Bounds and Record ([eb4f557](https://github.com/seuros/join_machines/commit/eb4f557d1449c4791204000ca818b4a7d8bdec55))

## [0.3.0](https://github.com/seuros/join_machines/compare/join-machines-v0.2.0...join-machines-v0.3.0) (2026-11-13)


### Features

* expose the window-machines Watermark seam and default output elements ([51df744](https://github.com/seuros/join_machines/commit/51df744004344e4679adf8ccfe308ad68337eca8))

## [0.2.0](https://github.com/seuros/join_machines/compare/join-machines-v0.1.0...join-machines-v0.2.0) (2026-11-12)


### Features

* initial extraction ([a045343](https://github.com/seuros/join_machines/commit/a04534332b17bd40113de9bdcb41c6a24c6eefa6))
