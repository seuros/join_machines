# Changelog

## [0.6.0](https://github.com/seuros/join_machines/compare/join_machines/v0.5.1...join_machines/v0.6.0) (2026-11-14)


### ⚠ BREAKING CHANGES

* read sibling watermarks through matryona own_or_sibling, one refusal message

### Features

* read sibling watermarks through matryona own_or_sibling, one refusal message ([6634897](https://github.com/seuros/join_machines/commit/6634897f7e7684feb52c3cf0ddb7c805467f4991))

## [0.5.1](https://github.com/seuros/join_machines/compare/join_machines/v0.5.0...join_machines/v0.5.1) (2026-11-14)


### Bug Fixes

* **deps:** require matryona-machines &gt;= 0.3 with symbols! tables for refusals, advancing, inserting ([a287139](https://github.com/seuros/join_machines/commit/a287139080c0b42d4ea0435f9cecaf5213924b85))

## [0.5.0](https://github.com/seuros/join_machines/compare/join_machines/v0.4.0...join_machines/v0.5.0) (2026-11-14)


### ⚠ BREAKING CHANGES

* adopt matryona-machines 0.2 (exact_fields, symbols!, frozen-first)

### Features

* adopt matryona-machines 0.2 (exact_fields, symbols!, frozen-first) ([84d94d2](https://github.com/seuros/join_machines/commit/84d94d2fc841d1dcbc3d0cdd8da938dcd3547298))

## [0.4.0](https://github.com/seuros/join_machines/compare/join_machines/v0.3.0...join_machines/v0.4.0) (2026-11-14)


### ⚠ BREAKING CHANGES

* seal values, native Bounds#reach, exact-class reads, re-check frozen before keeping

### Features

* seal values, native Bounds#reach, exact-class reads, re-check frozen before keeping ([3432d10](https://github.com/seuros/join_machines/commit/3432d1010e389333e15a2adf9dd702f0f7adf64d))

## [0.3.0](https://github.com/seuros/join_machines/compare/join_machines/v0.2.0...join_machines/v0.3.0) (2026-11-13)


### ⚠ BREAKING CHANGES

* converted to the fleet conventions. One exhaustive Refusal per crate, Result from every operation that can refuse, a refusal changes nothing, provable idempotent repeats are honoured, Ruby mirrors the core. No compatibility aliases. Requires window-machines >= 0.3 and re-exports its Advancing. Refusal::Duplicate is gone; a retired record is Late on replay.

### Features

* honour identical re-inserts as Duplicate, refuse changed ones as Mismatch, retire only when both watermarks have passed ([e06e9fe](https://github.com/seuros/join_machines/commit/e06e9fe31e3e93b80dd1fa99bfb5bc60839b2bd2))

## [0.2.0](https://github.com/seuros/join_machines/compare/join_machines/v0.1.2...join_machines/v0.2.0) (2026-11-13)


### ⚠ BREAKING CHANGES

* see CONVENTIONS: one exhaustive Refusal per crate, Result from every mutator, honoured idempotent repeats, Ruby mirrors the core. No compatibility aliases. advance_watermark and advance_watermark_from return Result<Advancing, Refusal>.

### Features

* watermark advances answer Advancing or WatermarkRegressed, Ruby takes Bounds and Record ([eb4f557](https://github.com/seuros/join_machines/commit/eb4f557d1449c4791204000ca818b4a7d8bdec55))

## [0.1.2](https://github.com/seuros/join_machines/compare/join_machines/v0.1.1...join_machines/v0.1.2) (2026-11-13)


### Features

* expose the window-machines Watermark seam and default output elements ([51df744](https://github.com/seuros/join_machines/commit/51df744004344e4679adf8ccfe308ad68337eca8))

## [0.1.1](https://github.com/seuros/join_machines/compare/join_machines-v0.1.0...join_machines/v0.1.1) (2026-11-12)


### Features

* initial extraction ([a045343](https://github.com/seuros/join_machines/commit/a04534332b17bd40113de9bdcb41c6a24c6eefa6))
