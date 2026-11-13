#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]
extern crate std;
use super::*;
use std::vec::Vec;

const fn rec(id: u64, key: u64, timestamp: u64) -> Record {
    Record { id, key, timestamp }
}
const BLANK: Pair = Pair {
    key: 0,
    left: 0,
    right: 0,
};
const GONE: Retired = Retired {
    side: Side::Left,
    record: rec(0, 0, 0),
};

fn drain_all<const L: usize, const R: usize>(join: &mut Join<L, R>, chunk: usize) -> Vec<Pair> {
    let mut pairs = Vec::new();
    let mut buffer = [BLANK; 8];
    while join.pending() {
        let written = join.drain(&mut buffer[..chunk]);
        assert!(written > 0, "a pending join must make progress");
        pairs.extend_from_slice(&buffer[..written]);
    }
    assert_eq!(join.drain(&mut buffer), 0);
    pairs
}

#[test]
fn interval_bounds_are_inclusive_and_asymmetric() {
    let mut join = Join::<8, 8>::new(Bounds::new(2, 5));
    for (id, timestamp) in [(1, 7), (2, 8), (3, 15), (4, 16)] {
        assert_eq!(join.insert(Side::Right, rec(id, 9, timestamp)), Ok(()));
        assert!(!join.pending());
    }
    assert_eq!(join.insert(Side::Right, rec(5, 1, 10)), Ok(()));
    assert_eq!(join.insert(Side::Left, rec(100, 9, 10)), Ok(()));
    assert_eq!(join.pending_pairs(), 2);
    let pairs = drain_all(&mut join, 8);
    let pair = |right| Pair {
        key: 9,
        left: 100,
        right,
    };
    assert_eq!(pairs, [pair(2), pair(3)]);
    // The mirrored probe: a right record finds earlier left records.
    assert_eq!(join.insert(Side::Left, rec(101, 9, 18)), Ok(()));
    assert_eq!(
        drain_all(&mut join, 8),
        [pair(4)].map(|p| Pair { left: 101, ..p })
    );
    assert_eq!(join.insert(Side::Right, rec(6, 9, 19)), Ok(()));
    assert_eq!(
        drain_all(&mut join, 8),
        [Pair {
            key: 9,
            left: 101,
            right: 6
        }]
    );
    assert_eq!(
        Bounds::new(2, 5).reach(Side::Left, 10),
        Reach { first: 8, last: 15 }
    );
    assert_eq!(
        Bounds::new(2, 5).reach(Side::Right, 10),
        Reach { first: 5, last: 12 }
    );
    assert_eq!((join.bounds().before(), join.bounds().after()), (2, 5));
}

#[test]
fn full_output_buffer_resumes_without_losing_or_repeating_pairs() {
    let mut join = Join::<2, 6>::new(Bounds::new(10, 10));
    for id in 0..6 {
        assert_eq!(join.insert(Side::Right, rec(id, 1, 10 + id)), Ok(()));
    }
    assert_eq!(join.insert(Side::Left, rec(50, 1, 12)), Ok(()));
    assert_eq!(join.pending_pairs(), 6);
    assert_eq!(join.drain(&mut []), 0);
    assert!(join.pending());

    let before = join.clone();
    assert_eq!(
        join.insert(Side::Left, rec(51, 1, 12)),
        Err(Refusal::Pending)
    );
    assert_eq!(
        join.insert(Side::Right, rec(9, 1, 12)),
        Err(Refusal::Pending)
    );
    assert_eq!(join.retire(&mut [GONE; 4]), Err(Refusal::Pending));
    assert_eq!(join, before);

    let mut seen = Vec::new();
    let mut buffer = [BLANK; 4];
    assert_eq!(join.drain(&mut buffer), 4);
    seen.extend_from_slice(&buffer);
    assert_eq!(join.pending_pairs(), 2);
    // Watermarks may move while pairs are owed; nothing is removed by that.
    assert!(join.advance_watermark(Side::Left, 1_000));
    assert!(join.advance_watermark(Side::Right, 1_000));
    assert_eq!(join.retire(&mut [GONE; 4]), Err(Refusal::Pending));
    assert_eq!(join.drain(&mut buffer), 2);
    seen.extend_from_slice(&buffer[..2]);
    assert!(!join.pending());
    let rights: Vec<u64> = seen.iter().map(|pair| pair.right).collect();
    assert_eq!(rights, [0, 1, 2, 3, 4, 5]);
    assert!(seen.iter().all(|pair| pair.left == 50 && pair.key == 1));
}

#[test]
fn capacity_full_duplicate_and_late_are_refusals_without_mutation() {
    let mut join = Join::<2, 1>::new(Bounds::new(1, 1));
    assert_eq!(join.capacity(Side::Left), 2);
    assert_eq!(join.capacity(Side::Right), 1);
    assert!(join.is_empty());
    assert_eq!(join.insert(Side::Left, rec(1, 0, 5)), Ok(()));
    assert_eq!(
        join.insert(Side::Left, rec(1, 3, 9)),
        Err(Refusal::Duplicate)
    );
    // Identities are per side.
    assert_eq!(join.insert(Side::Right, rec(1, 7, 50)), Ok(()));
    assert_eq!(join.insert(Side::Left, rec(2, 0, 6)), Ok(()));
    let before = join.clone();
    assert_eq!(join.insert(Side::Left, rec(3, 0, 7)), Err(Refusal::Full));
    assert_eq!(join.insert(Side::Right, rec(2, 0, 7)), Err(Refusal::Full));
    assert!(join.advance_watermark(Side::Left, 6));
    assert_eq!(join.insert(Side::Left, rec(3, 0, 6)), Err(Refusal::Late));
    assert_eq!(join.insert(Side::Left, rec(3, 0, 0)), Err(Refusal::Late));
    assert!(join.advance_watermark(Side::Left, 5).eq(&false));
    assert_eq!(join.watermark(Side::Left), Some(6));
    assert_eq!(join.watermark(Side::Right), None);
    assert_eq!((join.len(Side::Left), join.len(Side::Right)), (2, 1));
    assert!(join.contains(Side::Left, 2) && !join.contains(Side::Right, 2));
    assert_eq!(join.left, before.left);
    assert_eq!(join.right, before.right);
    assert!(!join.is_empty());

    let mut zero = Join::<0, 0>::new(Bounds::new(0, 0));
    assert_eq!(zero.insert(Side::Left, rec(1, 1, 1)), Err(Refusal::Full));
    assert_eq!(zero.retire(&mut [GONE; 1]), Ok(0));
}

#[test]
fn retirement_follows_the_opposite_watermark_and_is_resumable() {
    let mut join = Join::<4, 4>::new(Bounds::new(2, 5));
    for (id, timestamp) in [(1, 10), (2, 11), (3, 12)] {
        assert_eq!(join.insert(Side::Left, rec(id, 0, timestamp)), Ok(()));
    }
    assert_eq!(join.insert(Side::Right, rec(7, 1, 10)), Ok(()));
    assert_eq!(join.retirable(), 0);
    assert_eq!(join.retire(&mut [GONE; 4]), Ok(0));
    // Left 10 reaches right times 8..=15; right watermark 14 keeps it alive.
    assert!(join.advance_watermark(Side::Right, 14));
    assert_eq!(join.retirable(), 0);
    assert!(join.advance_watermark(Side::Right, 16));
    assert_eq!(join.retirable(), 2);
    assert_eq!(join.retire(&mut []), Ok(0));
    let mut out = [GONE; 1];
    assert_eq!(join.retire(&mut out), Ok(1));
    assert_eq!(out[0].record.id, 1);
    assert_eq!(join.retirable(), 1);
    assert_eq!(join.retire(&mut out), Ok(1));
    assert_eq!(
        out[0],
        Retired {
            side: Side::Left,
            record: rec(2, 0, 11)
        }
    );
    assert_eq!(join.retire(&mut out), Ok(0));
    assert_eq!(join.len(Side::Left), 1);
    // Right 10 reaches left times 5..=12; the left watermark retires it.
    assert!(join.advance_watermark(Side::Left, 11));
    assert_eq!(join.retirable(), 0);
    assert!(join.advance_watermark(Side::Left, 12));
    assert_eq!(join.retire(&mut out), Ok(1));
    assert_eq!(
        out[0],
        Retired {
            side: Side::Right,
            record: rec(7, 1, 10)
        }
    );
    // A retired identity and its slot are reusable.
    assert_eq!(join.insert(Side::Right, rec(7, 0, 17)), Ok(()));
    assert_eq!(
        drain_all(&mut join, 1),
        [Pair {
            key: 0,
            left: 3,
            right: 7
        }]
    );
}

#[test]
fn u64_limits_saturate_exactly() {
    let max = u64::MAX;
    assert_eq!(
        Bounds::new(max, max).reach(Side::Left, 5),
        Reach {
            first: 0,
            last: max
        }
    );
    assert_eq!(
        Bounds::new(3, 3).reach(Side::Right, max),
        Reach {
            first: max - 3,
            last: max
        }
    );
    let mut join = Join::<2, 2>::new(Bounds::new(3, 3));
    assert_eq!(join.insert(Side::Left, rec(max, max, max)), Ok(()));
    assert_eq!(join.insert(Side::Left, rec(0, max, 0)), Ok(()));
    assert_eq!(join.insert(Side::Right, rec(max, max, max - 3)), Ok(()));
    assert_eq!(
        drain_all(&mut join, 2),
        [Pair {
            key: max,
            left: max,
            right: max
        }]
    );
    assert!(join.advance_watermark(Side::Right, max - 1));
    let mut out = [GONE; 4];
    assert_eq!(join.retire(&mut out), Ok(1));
    assert_eq!(out[0].record.timestamp, 0);
    assert_eq!(join.insert(Side::Right, rec(1, max, max)), Ok(()));
    assert_eq!(join.pending_pairs(), 1);
    assert_eq!(drain_all(&mut join, 1).len(), 1);
    // Only the last representable watermark ends a record at the domain edge.
    assert!(join.advance_watermark(Side::Right, max));
    assert!(join.advance_watermark(Side::Left, max));
    assert!(!join.advance_watermark(Side::Left, max - 1));
    assert_eq!(join.insert(Side::Left, rec(9, 0, max)), Err(Refusal::Late));
    assert_eq!(join.retire(&mut out), Ok(3));
    assert!(join.is_empty());
}

struct Draws(u64);
impl Draws {
    const fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    const fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }
}

/// Naive model: remembers every accepted record forever and compares with
/// wide signed arithmetic, so it shares no reach or retirement logic.
#[derive(Default)]
struct Model {
    seen: [Vec<(Record, bool)>; 2],
    watermark: [Option<u64>; 2],
    owed: Vec<Pair>,
}
impl Model {
    fn matches(before: u64, after: u64, left: Record, right: Record) -> bool {
        let delta = i128::from(right.timestamp) - i128::from(left.timestamp);
        left.key == right.key && delta >= -i128::from(before) && delta <= i128::from(after)
    }
    fn live(&self, side: usize) -> usize {
        self.seen[side].iter().filter(|(_, live)| *live).count()
    }
    /// Dead when no admissible (above-watermark, representable) opposite
    /// timestamp is still within the record's bounds.
    fn dead(&self, before: u64, after: u64, side: usize, record: Record) -> bool {
        let ahead = if side == 0 { after } else { before };
        let last = (i128::from(record.timestamp) + i128::from(ahead)).min(i128::from(u64::MAX));
        self.watermark[1 - side].is_some_and(|mark| i128::from(mark) >= last)
    }
}

const SIDES: [Side; 2] = [Side::Left, Side::Right];

#[allow(clippy::too_many_lines)]
fn run_model(seed: u64, before: u64, after: u64, base: u64) {
    const L: usize = 5;
    const R: usize = 7;
    let caps = [L, R];
    let mut draws = Draws(seed);
    let mut join = Join::<L, R>::new(Bounds::new(before, after));
    let mut model = Model::default();
    let mut clock = [base; 2];
    for _ in 0..4_000 {
        match draws.below(10) {
            0..=4 => {
                let side = usize::try_from(draws.below(2)).unwrap();
                let jitter = draws.below(12);
                let timestamp = (clock[side].saturating_sub(4)).saturating_add(jitter);
                let record = rec(draws.below(24), draws.below(3), timestamp);
                let expected = if !model.owed.is_empty() {
                    Err(Refusal::Pending)
                } else if model.watermark[side].is_some_and(|mark| timestamp <= mark) {
                    Err(Refusal::Late)
                } else if model.seen[side]
                    .iter()
                    .any(|(held, live)| *live && held.id == record.id)
                {
                    Err(Refusal::Duplicate)
                } else if model.live(side) == caps[side] {
                    Err(Refusal::Full)
                } else {
                    Ok(())
                };
                let snapshot = join.clone();
                assert_eq!(join.insert(SIDES[side], record), expected);
                if expected.is_err() {
                    assert_eq!(join, snapshot);
                    continue;
                }
                // Every record ever accepted on the other side, retired or not.
                for (other, _) in &model.seen[1 - side] {
                    let (left, right) = if side == 0 {
                        (record, *other)
                    } else {
                        (*other, record)
                    };
                    if Model::matches(before, after, left, right) {
                        model.owed.push(Pair {
                            key: record.key,
                            left: left.id,
                            right: right.id,
                        });
                    }
                }
                model.seen[side].push((record, true));
                assert_eq!(join.pending_pairs(), model.owed.len());
            }
            5 | 6 => {
                let mut buffer = [BLANK; 3];
                let room = usize::try_from(draws.below(4)).unwrap();
                let written = join.drain(&mut buffer[..room]);
                assert_eq!(written, room.min(model.owed.len()));
                for pair in &buffer[..written] {
                    let index = model.owed.iter().position(|owed| owed == pair);
                    model
                        .owed
                        .swap_remove(index.expect("emitted pair must be owed"));
                }
                assert_eq!(join.pending(), !model.owed.is_empty());
            }
            7 => {
                let side = usize::try_from(draws.below(2)).unwrap();
                let value = (clock[side].saturating_sub(2)).saturating_add(draws.below(6));
                let ok = model.watermark[side].is_none_or(|mark| value >= mark);
                let snapshot = join.clone();
                assert_eq!(join.advance_watermark(SIDES[side], value), ok);
                if ok {
                    model.watermark[side] = Some(value);
                    clock[side] = clock[side].max(value);
                } else {
                    assert_eq!(join, snapshot);
                }
                assert_eq!(join.watermark(SIDES[side]), model.watermark[side]);
            }
            _ => {
                let mut buffer = [GONE; 3];
                let room = usize::try_from(draws.below(4)).unwrap();
                let snapshot = join.clone();
                let result = join.retire(&mut buffer[..room]);
                if !model.owed.is_empty() {
                    assert_eq!(result, Err(Refusal::Pending));
                    assert_eq!(join, snapshot);
                    continue;
                }
                let due: usize = (0..2)
                    .map(|side| {
                        model.seen[side]
                            .iter()
                            .filter(|(held, live)| *live && model.dead(before, after, side, *held))
                            .count()
                    })
                    .sum();
                assert_eq!(snapshot.retirable(), due);
                assert_eq!(result, Ok(room.min(due)));
                for gone in &buffer[..room.min(due)] {
                    let side = usize::from(gone.side == Side::Right);
                    assert!(model.dead(before, after, side, gone.record));
                    let entry = model.seen[side]
                        .iter_mut()
                        .find(|(held, live)| *live && *held == gone.record)
                        .expect("retired record must be live");
                    entry.1 = false;
                }
            }
        }
        for (index, side) in SIDES.into_iter().enumerate() {
            assert_eq!(join.len(side), model.live(index));
        }
    }
}

#[test]
fn randomized_runs_match_the_naive_model() {
    for seed in 1..=40_u64 {
        let mut draws = Draws(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let (before, after) = (draws.below(6), draws.below(6));
        run_model(seed, before, after, 0);
        run_model(seed + 1_000, before, after, u64::MAX - 40);
    }
    run_model(7, u64::MAX, u64::MAX, 0);
    run_model(8, 0, 0, 0);
}

mod seam;
