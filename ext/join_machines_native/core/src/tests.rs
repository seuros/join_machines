extern crate std;
use super::*;
use std::vec::Vec;

const fn rec(id: u64, key: u64, timestamp: u64) -> Record {
    Record { id, key, timestamp }
}
const fn pair(key: u64, left: u64, right: u64) -> Pair {
    Pair { key, left, right }
}
const ACCEPTED: Result<Inserting, Refusal> = Ok(Inserting::Accepted);
const DUPLICATE: Result<Inserting, Refusal> = Ok(Inserting::Duplicate);
const MISMATCH: Result<Inserting, Refusal> = Err(Refusal::Mismatch);
const LATE: Result<Inserting, Refusal> = Err(Refusal::Late);

fn advance<const L: usize, const R: usize>(join: &mut Join<L, R>, side: Side, value: u64) {
    assert_eq!(join.advance_watermark(side, value), Ok(Advancing::Advanced));
}

fn drain_all<const L: usize, const R: usize>(join: &mut Join<L, R>, chunk: usize) -> Vec<Pair> {
    let mut pairs = Vec::new();
    let mut buffer = [Pair::default(); 8];
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
        assert_eq!(join.insert(Side::Right, rec(id, 9, timestamp)), ACCEPTED);
        assert!(!join.pending());
    }
    assert_eq!(join.insert(Side::Right, rec(5, 1, 10)), ACCEPTED);
    assert_eq!(join.insert(Side::Left, rec(100, 9, 10)), ACCEPTED);
    assert_eq!(join.pending_pairs(), 2);
    let pairs = drain_all(&mut join, 8);
    assert_eq!(pairs, [pair(9, 100, 2), pair(9, 100, 3)]);
    // The mirrored probe: a right record finds earlier left records.
    assert_eq!(join.insert(Side::Left, rec(101, 9, 18)), ACCEPTED);
    assert_eq!(drain_all(&mut join, 8), [pair(9, 101, 4)]);
    assert_eq!(join.insert(Side::Right, rec(6, 9, 19)), ACCEPTED);
    assert_eq!(drain_all(&mut join, 8), [pair(9, 101, 6)]);
    assert_eq!(
        Bounds::new(2, 5).reach(Side::Left, 10),
        Reach::new(8, 15).expect("8 <= 15")
    );
    assert_eq!(
        Bounds::new(2, 5).reach(Side::Right, 10),
        Reach::new(5, 12).expect("5 <= 12")
    );
    assert_eq!((join.bounds().before(), join.bounds().after()), (2, 5));
}

#[test]
fn a_reach_is_valid_or_does_not_exist() {
    assert_eq!(Reach::new(5, 3), None);
    let one = Reach::new(7, 7).expect("7 <= 7");
    assert_eq!((one.first(), one.last()), (7, 7));
    assert!(one.contains(7) && !one.contains(6) && !one.contains(8));
    // `reach` saturates at both edges and stays a valid reach.
    let edge = Bounds::new(u64::MAX, u64::MAX).reach(Side::Left, 3);
    assert_eq!(Some(edge), Reach::new(0, u64::MAX));
}

#[test]
fn full_output_buffer_resumes_without_losing_or_repeating_pairs() {
    let mut join = Join::<2, 6>::new(Bounds::new(10, 10));
    for id in 0..6 {
        assert_eq!(join.insert(Side::Right, rec(id, 1, 10 + id)), ACCEPTED);
    }
    assert_eq!(join.insert(Side::Left, rec(50, 1, 12)), ACCEPTED);
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
    assert_eq!(
        join.retire(&mut [Retired::default(); 4]),
        Err(Refusal::Pending)
    );
    assert_eq!(join, before);

    let mut seen = Vec::new();
    let mut buffer = [Pair::default(); 4];
    assert_eq!(join.drain(&mut buffer), 4);
    seen.extend_from_slice(&buffer);
    assert_eq!(join.pending_pairs(), 2);
    // Watermarks may move while pairs are owed; nothing is removed by that.
    assert_eq!(
        join.advance_watermark(Side::Left, 1_000),
        Ok(Advancing::Advanced)
    );
    assert_eq!(
        join.advance_watermark(Side::Right, 1_000),
        Ok(Advancing::Advanced)
    );
    assert_eq!(
        join.retire(&mut [Retired::default(); 4]),
        Err(Refusal::Pending)
    );
    assert_eq!(join.drain(&mut buffer), 2);
    seen.extend_from_slice(&buffer[..2]);
    assert!(!join.pending());
    let rights: Vec<u64> = seen.iter().map(|pair| pair.right).collect();
    assert_eq!(rights, [0, 1, 2, 3, 4, 5]);
    assert!(seen.iter().all(|pair| pair.left == 50 && pair.key == 1));
}

#[test]
fn capacity_full_mismatch_and_late_are_refusals_without_mutation() {
    let mut join = Join::<2, 1>::new(Bounds::new(1, 1));
    assert_eq!(join.capacity(Side::Left), 2);
    assert_eq!(join.capacity(Side::Right), 1);
    assert!(join.is_empty());
    assert_eq!(join.insert(Side::Left, rec(1, 0, 5)), ACCEPTED);
    let single = join.clone();
    assert_eq!(
        join.insert(Side::Left, rec(1, 3, 9)),
        Err(Refusal::Mismatch)
    );
    assert_eq!(join, single);
    // Identities are per side.
    assert_eq!(join.insert(Side::Right, rec(1, 7, 50)), ACCEPTED);
    assert_eq!(join.insert(Side::Left, rec(2, 0, 6)), ACCEPTED);
    let before = join.clone();
    assert_eq!(join.insert(Side::Left, rec(3, 0, 7)), Err(Refusal::Full));
    assert_eq!(join.insert(Side::Right, rec(2, 0, 7)), Err(Refusal::Full));
    assert_eq!(
        join.advance_watermark(Side::Left, 6),
        Ok(Advancing::Advanced)
    );
    assert_eq!(join.insert(Side::Left, rec(3, 0, 6)), LATE);
    assert_eq!(join.insert(Side::Left, rec(3, 0, 0)), LATE);
    let held = join.clone();
    assert_eq!(
        join.advance_watermark(Side::Left, 5),
        Err(Refusal::WatermarkRegressed)
    );
    assert_eq!(join, held);
    assert_eq!(
        join.advance_watermark(Side::Left, 6),
        Ok(Advancing::Repeated)
    );
    assert_eq!(join, held);
    assert_eq!(join.watermark(Side::Left), Some(6));
    assert_eq!(join.watermark(Side::Right), None);
    assert_eq!((join.len(Side::Left), join.len(Side::Right)), (2, 1));
    assert!(join.contains(Side::Left, 2) && !join.contains(Side::Right, 2));
    assert_eq!(join.left, before.left);
    assert_eq!(join.right, before.right);
    assert!(!join.is_empty());

    let mut zero = Join::<0, 0>::new(Bounds::new(0, 0));
    assert_eq!(zero.insert(Side::Left, rec(1, 1, 1)), Err(Refusal::Full));
    assert_eq!(zero.retire(&mut [Retired::default(); 1]), Ok(0));
}

#[test]
fn retirement_needs_both_watermarks_and_is_resumable() {
    let mut join = Join::<4, 4>::new(Bounds::new(2, 5));
    for (id, timestamp) in [(1, 10), (2, 11), (3, 12)] {
        assert_eq!(join.insert(Side::Left, rec(id, 0, timestamp)), ACCEPTED);
    }
    assert_eq!(join.insert(Side::Right, rec(7, 1, 10)), ACCEPTED);
    assert_eq!(join.retirable(), 0);
    assert_eq!(join.retire(&mut [Retired::default(); 4]), Ok(0));
    // Left 10 reaches right times 8..=15; right watermark 14 keeps it alive.
    advance(&mut join, Side::Right, 14);
    assert_eq!(join.retirable(), 0);
    // Right 16 ends left 10 and left 11, but the left watermark has not
    // reached either: offered again they would not be late yet, so they stay.
    advance(&mut join, Side::Right, 16);
    assert_eq!(join.retirable(), 0);
    assert_eq!(join.retire(&mut [Retired::default(); 4]), Ok(0));
    advance(&mut join, Side::Left, 10);
    assert_eq!(join.retirable(), 1);
    advance(&mut join, Side::Left, 11);
    assert_eq!(join.retirable(), 2);
    assert_eq!(join.retire(&mut []), Ok(0));
    let mut out = [Retired::default(); 1];
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
    // Right 10 reaches left times 5..=12: left watermark 12 ends it, and the
    // right watermark is long past it.
    assert_eq!(join.retirable(), 0);
    advance(&mut join, Side::Left, 12);
    assert_eq!(join.retire(&mut out), Ok(1));
    assert_eq!(
        out[0],
        Retired {
            side: Side::Right,
            record: rec(7, 1, 10)
        }
    );
    // Left 12 is late on replay now, but right times up to 17 can still
    // match it: it stays, and the right record at 17 finds it.
    assert_eq!(join.retirable(), 0);
    assert_eq!(join.insert(Side::Right, rec(8, 0, 17)), ACCEPTED);
    assert_eq!(drain_all(&mut join, 1), [pair(0, 3, 8)]);
}

#[test]
fn u64_limits_saturate_exactly() {
    let max = u64::MAX;
    assert_eq!(
        Bounds::new(3, 3).reach(Side::Right, max),
        Reach {
            first: max - 3,
            last: max
        }
    );
    let mut join = Join::<2, 2>::new(Bounds::new(3, 3));
    assert_eq!(join.insert(Side::Left, rec(max, max, max)), ACCEPTED);
    assert_eq!(join.insert(Side::Left, rec(0, max, 0)), ACCEPTED);
    assert_eq!(join.insert(Side::Right, rec(max, max, max - 3)), ACCEPTED);
    assert_eq!(drain_all(&mut join, 2), [pair(max, max, max)]);
    // Left 0 can match nothing above right watermark `max - 1`, and its own
    // watermark reaching 0 makes a replay of it late: only then it retires.
    advance(&mut join, Side::Right, max - 1);
    let mut out = [Retired::default(); 4];
    assert_eq!(join.retire(&mut out), Ok(0));
    advance(&mut join, Side::Left, 0);
    assert_eq!(join.retire(&mut out), Ok(1));
    assert_eq!(out[0].record.timestamp, 0);
    assert_eq!(join.insert(Side::Right, rec(1, max, max)), ACCEPTED);
    assert_eq!(join.pending_pairs(), 1);
    assert_eq!(drain_all(&mut join, 1).len(), 1);
    // Only the last representable watermark ends a record at the domain edge.
    advance(&mut join, Side::Right, max);
    assert_eq!(join.retirable(), 0);
    advance(&mut join, Side::Left, max - 1);
    assert_eq!(join.retirable(), 0);
    advance(&mut join, Side::Left, max);
    assert_eq!(join.retirable(), 3);
    assert_eq!(
        join.advance_watermark(Side::Left, max - 1),
        Err(Refusal::WatermarkRegressed)
    );
    assert_eq!(join.insert(Side::Left, rec(9, 0, max)), LATE);
    assert_eq!(join.retire(&mut out), Ok(3));
    assert!(join.is_empty());
}

mod model;
mod repeat;
mod retire;
mod seam;
