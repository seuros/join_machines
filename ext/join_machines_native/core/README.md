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
own side's watermark is `Late`. A held record becomes retirable when the
opposite watermark reaches the last time it can match, and `retire` removes such
records into a bounded buffer so the caller can release their payloads. Because
lateness uses the record's own side and retirement the other, an admitted record
always finds every partner still held. Reaches saturate at the `u64` edges.

Refusals (`Pending`, `Late`, `Duplicate`, `Full`) change nothing. A full side is
never evicted. Watermark regressions return `false`.

This is join bookkeeping only: no payload buffering, transport, execution,
persistence, key hashing or clocks. Key collisions, watermark generation and
recovery belong to the caller. Initial, unstable API.

```rust
use join_machines::{Bounds, Join, Pair, Record, Refusal, Side};
let mut join = Join::<4, 4>::new(Bounds::new(2, 5));
let record = |id, timestamp| Record { id, key: 9, timestamp };
assert_eq!(join.insert(Side::Right, record(1, 8)), Ok(()));
assert_eq!(join.insert(Side::Right, record(2, 15)), Ok(()));
assert_eq!(join.insert(Side::Left, record(100, 10)), Ok(()));
assert_eq!(join.insert(Side::Left, record(101, 10)), Err(Refusal::Pending));

let mut out = [Pair { key: 0, left: 0, right: 0 }; 1];
assert_eq!(join.drain(&mut out), 1);
assert_eq!(out[0], Pair { key: 9, left: 100, right: 1 });
assert_eq!(join.drain(&mut out), 1);
assert_eq!(out[0].right, 2);
assert!(!join.pending());

assert!(join.advance_watermark(Side::Right, 15));
assert_eq!(join.retirable(), 1);
```
