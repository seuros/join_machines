# Changelog

## [0.7.0](https://github.com/seuros/join_machines/compare/join-machines-v0.6.0...join-machines-v0.7.0) (2026-11-14)


### ⚠ BREAKING CHANGES

* adopt matryona-machines 0.2 (exact_fields, symbols!, frozen-first)

### Features

* adopt matryona-machines 0.2 (exact_fields, symbols!, frozen-first) ([84d94d2](https://github.com/seuros/join_machines/commit/84d94d2fc841d1dcbc3d0cdd8da938dcd3547298))

## [0.6.0](https://github.com/seuros/join_machines/compare/join-machines-v0.5.0...join-machines-v0.6.0) (2026-11-14)


### ⚠ BREAKING CHANGES

* seal values, native Bounds#reach, exact-class reads, re-check frozen before keeping

### Features

* seal values, native Bounds#reach, exact-class reads, re-check frozen before keeping ([3432d10](https://github.com/seuros/join_machines/commit/3432d1010e389333e15a2adf9dd702f0f7adf64d))

## [0.5.0](https://github.com/seuros/join_machines/compare/join-machines-v0.4.0...join-machines-v0.5.0) (2026-11-13)


### ⚠ BREAKING CHANGES

* converted to the fleet conventions. One exhaustive Refusal per crate, Result from every operation that can refuse, a refusal changes nothing, provable idempotent repeats are honoured, Ruby mirrors the core. No compatibility aliases. Requires window-machines >= 0.3 and re-exports its Advancing. Refusal::Duplicate is gone; a retired record is Late on replay.

### Features

* honour identical re-inserts as Duplicate, refuse changed ones as Mismatch, retire only when both watermarks have passed ([e06e9fe](https://github.com/seuros/join_machines/commit/e06e9fe31e3e93b80dd1fa99bfb5bc60839b2bd2))

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
