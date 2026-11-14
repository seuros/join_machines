# join-machines

> **AMS Widom**, NCC-4232, *Widom*-class stream tender.

Engine-room flight manual: log which hulls on two lanes passed abeam of each other.

Allocation-free `no_std` two-input interval join over record metadata. Each
record is an identity, a `u64` key and an event time; a left record at `t`
matches right records with the same key at `t - before ..= t + after`. The join
holds at most `L` left and `R` right records and stores no payloads.

Inserting a record owes one pair per matching held opposite record. `drain`
writes owed pairs into a caller buffer of any size and resumes where it stopped,
so a full buffer never loses or repeats a pair. While pairs are owed, `insert`
and `retire` are refused with `Pending`; that is the only backpressure the core
needs to stay bounded.

Each input has its own `window_machines::Watermark`. A record at or below its
own side's watermark is `Late`; the opposite watermark never makes a record
late. A held record becomes retirable when both watermarks have finished it:
the opposite one has reached the end of its reach, the last time it can match,
and its own has reached its timestamp. `retire` removes such records into a
bounded buffer so the caller can release their payloads. So a record whose
partner is still held is always admitted and paired, and a retired record
offered again is `Late`. Reaches saturate at the `u64` edges. The exact
inequalities are in the Contract below.

Refusals (`Mismatch`, `Pending`, `Late`, `Full`, `WatermarkRegressed`) are one
`Refusal` enum and change nothing. A full side is never evicted.

`insert` returns `Result<Inserting, Refusal>`. A record whose identity is held
on that side is answered from the held record, before any other check:

| `insert(side, record)` | Answer |
| --- | --- |
| identity not held, admitted | `Ok(Inserting::Accepted)`: held, its pairs owed |
| identity held with the same key and timestamp | `Ok(Inserting::Duplicate)`: nothing changes, no pair is owed twice |
| identity held with another key or timestamp | `Err(Refusal::Mismatch)` |
| identity not held, pairs still owed | `Err(Refusal::Pending)` |
| identity not held, at or below its own side's watermark | `Err(Refusal::Late)` |
| identity not held, no free slot | `Err(Refusal::Full)` |

Held means live right now. A record that has matched and been drained is still
held, so replaying it is `Duplicate`, also while its pairs are still owed and
after its own watermark has passed it. A retired record is no longer held, and
the join keeps no history of identities, yet the same record can never be
admitted again: it was retired only once its own watermark had reached it, and
that watermark never moves back, so a replay is `Late`. The identity itself is
not remembered: other contents under it are judged as any new record is
(`Accepted` if they are above the side's watermark, never `Mismatch`).

`advance_watermark` returns `Result<Advancing, Refusal>`: `Advancing::Advanced`
when the side moved forward, `Advancing::Repeated` when it already held exactly
that value (an idempotent repeat that changes nothing), and
`Err(Refusal::WatermarkRegressed)` for a value behind it. `Advancing` is
`window_machines::Advancing`, re-exported. The join asks the side's
`Watermark` where it stands and then sets it with `Watermark::at`, so it never
has to translate sibling refusals (`Overflow`, `Undersized`, `Late`, `Full`)
that `Watermark::advance` can never give.

The sibling seam is public: `Watermark` and `Advancing` are re-exported, `side_watermark(side)`
returns a side's `Watermark` by value (next to `watermark(side)`, which still
reads the bare `Option<u64>`), and `advance_watermark_from(side, &Watermark)`
advances a side from a watermark the caller owns, with the same answers as
`advance_watermark`. A source watermark with no value yet is
`Ok(Advancing::Repeated)` while the side is also uninitialized (they already
agree) and `Err(Refusal::WatermarkRegressed)` once the side has a value; it
never clears a side.

`Reach` keeps its fields private and is valid or does not exist:
`Reach::new(first, last)` is `None` unless `first <= last`, and `Bounds::reach`
always answers one (it saturates at both edges). `Record`, `Pair`, `Retired`
and `Bounds` hold any value of their field types, so their fields stay public
(`Bounds` behind its infallible `new`).

`Pair` and `Retired` implement `Default` so an output buffer is
`[Pair::default(); N]`. The default is a placeholder, not a
sentinel: a real zero-valued output equals it, so read only the entries reported.

The Ruby gem mirrors `Bounds`, `Reach`, `Record`, `Pair` and `Retired` as
`Data` values of the same names and fields, checked by the core when built, and takes `Bounds` and `Record`
where this core does. `side_watermark` answers a `Data` with the one reader
`value`, which is what its `advance_watermark_from` reads.

This is join bookkeeping only: no payload buffering, transport, execution,
persistence, key hashing or clocks. Key collisions, watermark generation and
recovery belong to the caller. Initial, unstable API.

```rust
use join_machines::{
    Advancing, Bounds, Inserting, Join, Pair, Record, Refusal, Side, Watermark,
};
let mut join = Join::<4, 4>::new(Bounds::new(2, 5));
let record = |id, timestamp| Record { id, key: 9, timestamp };
assert_eq!(join.insert(Side::Right, record(1, 8)), Ok(Inserting::Accepted));
assert_eq!(join.insert(Side::Right, record(2, 15)), Ok(Inserting::Accepted));
assert_eq!(join.insert(Side::Left, record(100, 10)), Ok(Inserting::Accepted));
assert_eq!(join.insert(Side::Left, record(101, 10)), Err(Refusal::Pending));
// A replay of a held record is honoured; the same id with other contents is not.
assert_eq!(join.insert(Side::Left, record(100, 10)), Ok(Inserting::Duplicate));
assert_eq!(join.insert(Side::Left, record(100, 11)), Err(Refusal::Mismatch));

let mut out = [Pair::default(); 1];
assert_eq!(join.drain(&mut out), 1);
assert_eq!(out[0], Pair { key: 9, left: 100, right: 1 });
assert_eq!(join.drain(&mut out), 1);
assert_eq!(out[0].right, 2);
assert!(!join.pending());

// The right input's progress is tracked by a watermark the caller owns.
let mut right = Watermark::default();
assert_eq!(right.advance(15), Ok(Advancing::Advanced));
assert_eq!(join.advance_watermark_from(Side::Right, &right), Ok(Advancing::Advanced));
assert_eq!(join.advance_watermark(Side::Right, 15), Ok(Advancing::Repeated));
assert_eq!(join.advance_watermark(Side::Right, 14), Err(Refusal::WatermarkRegressed));
assert_eq!(join.side_watermark(Side::Right), right);
assert_eq!(join.watermark(Side::Right), Some(15));
// Right 15 ends left 100 (it reaches 8..=15), but a replay of it would not be
// late yet: it stays until the left watermark reaches it too.
assert_eq!(join.retirable(), 0);
assert_eq!(join.advance_watermark(Side::Left, 10), Ok(Advancing::Advanced));
// Left 100 goes, and so does right 1: it reaches left times 3..=10.
assert_eq!(join.retirable(), 2);
```

## Contract

Write `wl` and `wr` for the left and right watermarks (unset until first
advanced) and `⊕` for addition saturating at `u64::MAX`.

- **Match.** Left `l` and right `r` match when the keys are equal and
  `l.timestamp - before <= r.timestamp <= l.timestamp + after`.
- **Late.** `insert` of a record at `t` that is not held answers `Late` when
  its own side's watermark is set and `t <= w_own`, and on no other count. The
  opposite watermark never refuses a record. Watermarks never move back, so a
  record that is `Late` once is `Late` for good.
- **Nothing matchable is refused.** A record whose partner is held is admitted
  and paired however far the two watermarks have drifted apart, unless its own
  side has declared it late (or the side is `Full`, or pairs are `Pending`).
- **Retire.** A held record at `t` is retirable when both hold:
  1. *Nothing can match it any more.* The opposite watermark has reached the
     end of its reach: `wr >= t ⊕ after` for a left record, `wl >= t ⊕ before`
     for a right one. An opposite record is admitted only strictly above its
     own watermark, so every one still to come lies past the reach. The
     comparison is inclusive for that reason, and exact: one short, a record
     at the end of the reach is still admitted and matches. A reach that
     saturates is reached only by a watermark of `u64::MAX`, where no opposite
     record can be admitted at all.
  2. *A replay of it is late.* Its own watermark has reached it: `wl >= t`
     for a left record, `wr >= t` for a right one. This is `insert`'s `Late`
     test itself, so the two cannot disagree.

  `retire` and `retirable` use this one test. With `Bounds::new(0, 0)` a
  record at `t` is retirable once both watermarks are at or past `t`.
- **A lagging side keeps its records.** A record is held until its *own*
  side's watermark reaches it, whatever the opposite one says: a side whose
  watermark is never advanced never retires anything. Memory stays bounded all
  the same: the side fills and `insert` answers `Full`. Advance both
  watermarks to release records.
- **Exactly once, and complete.** As long as an identity names one record
  (same side, key and timestamp) for the life of the join: no `(left, right)`
  pair is drained twice, because a pair is owed only when one of its records
  is `Accepted`, a held record replayed is `Duplicate`, and a retired one is
  `Late`; and every two accepted records that match are drained as a pair,
  because the earlier one is still held when the later one is accepted
  (rule 1 kept it).
- **No clock.** Every time here is event time; the core reads and records no
  monotonic clock. An honoured `advance_watermark`/`advance_watermark_from`
  records the side's watermark (progress, not a clock) on `Advanced` and
  nothing on `Repeated`.
- **No unreachable arms.** No `panic!`, `assert!`, `unreachable!` or `expect`
  outside tests. The owed pair carries the opposite record it names, so
  `drain` never looks a slot up again.
- **Refusals change nothing**, and none is remembered: each is an answer about
  the join as it is now. `Late` stays `Late` only because watermarks, which
  honoured calls record, never regress.
