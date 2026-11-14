//! Retirement: both watermarks, their boundaries, and replays afterwards.
use super::*;

#[test]
fn a_replay_after_retirement_is_late_and_emits_no_pair_twice() {
    let mut join = Join::<2, 2>::new(Bounds::new(2, 2));
    let left = rec(1, 7, 10);
    assert_eq!(join.insert(Side::Left, left), ACCEPTED);
    assert_eq!(join.insert(Side::Right, rec(2, 7, 11)), ACCEPTED);
    assert_eq!(drain_all(&mut join, 1), [pair(7, 1, 2)]);
    // Only the right watermark moves. No right record can match the left one
    // any more, but the left watermark has not reached it: offered again it
    // would not be late, so it is not retired. It is held, and says so.
    advance(&mut join, Side::Right, 12);
    let mut gone = [Retired::default(); 2];
    let kept = join.clone();
    assert_eq!(join.retirable(), 0);
    assert_eq!(join.retire(&mut gone), Ok(0));
    assert_eq!(join.insert(Side::Left, left), DUPLICATE);
    assert_eq!(join, kept);
    assert!(!join.pending(), "the pair was owed a second time");
    // One short of the record on its own side: still held.
    advance(&mut join, Side::Left, 9);
    assert_eq!(join.retirable(), 0);
    advance(&mut join, Side::Left, 10);
    assert_eq!(join.retirable(), 1);
    assert_eq!(join.retire(&mut gone), Ok(1));
    assert_eq!((gone[0].side, gone[0].record), (Side::Left, left));

    let retired = join.clone();
    assert_eq!(join.insert(Side::Left, left), LATE);
    assert_eq!(join, retired);
    assert!(!join.pending(), "the pair was owed a second time");
    assert_eq!(drain_all(&mut join, 1), []);
}

#[test]
fn a_matchable_record_is_never_late_because_of_the_other_side() {
    // The watermarks drift apart: the right one is past everything the left
    // record can match, the left one is unset, the partner is still held.
    let mut join = Join::<4, 4>::new(Bounds::new(2, 2));
    assert_eq!(join.insert(Side::Right, rec(2, 7, 11)), ACCEPTED);
    advance(&mut join, Side::Right, 12);
    assert_eq!(join.watermark(Side::Left), None);
    assert_eq!(join.insert(Side::Left, rec(1, 7, 10)), ACCEPTED);
    assert_eq!(drain_all(&mut join, 1), [pair(7, 1, 2)]);

    // `after: 0`: a left record arriving once the right watermark has reached
    // its time, or run far past it, still finds the right record held.
    let mut join = Join::<4, 4>::new(Bounds::new(5, 0));
    assert_eq!(join.insert(Side::Right, rec(2, 7, 10)), ACCEPTED);
    advance(&mut join, Side::Right, 10);
    assert_eq!(join.insert(Side::Left, rec(1, 7, 10)), ACCEPTED);
    assert_eq!(drain_all(&mut join, 1), [pair(7, 1, 2)]);
    advance(&mut join, Side::Right, 500);
    assert_eq!(join.insert(Side::Left, rec(3, 7, 15)), ACCEPTED);
    assert_eq!(drain_all(&mut join, 1), [pair(7, 3, 2)]);
    advance(&mut join, Side::Right, u64::MAX);
    assert_eq!(join.insert(Side::Left, rec(4, 7, 12)), ACCEPTED);
    assert_eq!(drain_all(&mut join, 1), [pair(7, 4, 2)]);
    // Its own side is another matter: the right input is closed for good.
    assert_eq!(join.insert(Side::Right, rec(5, 7, u64::MAX)), LATE);

    // The mirrored drift: the left watermark runs far ahead of a right record
    // whose partner is still held.
    let mut join = Join::<4, 4>::new(Bounds::new(0, 5));
    assert_eq!(join.insert(Side::Left, rec(1, 7, 10)), ACCEPTED);
    advance(&mut join, Side::Left, 400);
    assert_eq!(join.insert(Side::Right, rec(2, 7, 10)), ACCEPTED);
    assert_eq!(drain_all(&mut join, 1), [pair(7, 1, 2)]);
}

#[test]
fn a_retired_record_is_late_for_good_and_its_identity_is_not_remembered() {
    let mut join = Join::<2, 2>::new(Bounds::new(0, 0));
    assert_eq!(join.insert(Side::Left, rec(1, 0, 5)), ACCEPTED);
    assert_eq!(join.insert(Side::Left, rec(2, 0, 6)), ACCEPTED);
    advance(&mut join, Side::Right, 6);
    advance(&mut join, Side::Left, 5);
    let mut gone = [Retired::default(); 2];
    assert_eq!(join.retire(&mut gone), Ok(1));
    assert_eq!(gone[0].record, rec(1, 0, 5));
    // Its own watermark, which let it go, refuses it and keeps refusing it.
    let retired = join.clone();
    assert_eq!(join.insert(Side::Left, rec(1, 0, 5)), LATE);
    assert_eq!(
        join.advance_watermark(Side::Left, 5),
        Ok(Advancing::Repeated)
    );
    assert_eq!(join.insert(Side::Left, rec(1, 0, 5)), LATE);
    assert_eq!(join, retired);
    // The identity itself left no trace: other contents under it are judged
    // as any new record is, not as a mismatch.
    assert_eq!(join.insert(Side::Left, rec(1, 4, 3)), LATE);
    assert_eq!(join, retired);
    assert_eq!(join.insert(Side::Left, rec(1, 4, 9)), ACCEPTED);
    // The record still held keeps answering as held.
    let held = join.clone();
    assert_eq!(join.insert(Side::Left, rec(2, 0, 6)), DUPLICATE);
    assert_eq!(join, held);
}

/// A record at `t` leaves when the opposite watermark is at or past the end
/// of its reach (`t + after` for a left record, `t + before` for a right one)
/// and its own watermark is at or past `t`. One short on either: it stays.
#[test]
fn retirement_is_inclusive_on_both_watermarks_in_either_order() {
    let cases = [
        (Side::Left, 3, Bounds::new(1, 3)),
        (Side::Right, 1, Bounds::new(1, 3)),
        (Side::Left, 0, Bounds::new(0, 0)),
        (Side::Right, 0, Bounds::new(0, 0)),
    ];
    for (side, ahead, bounds) in cases {
        for own_first in [true, false] {
            let mut join = Join::<2, 2>::new(bounds);
            assert_eq!(join.insert(side, rec(1, 0, 10)), ACCEPTED);
            if own_first {
                advance(&mut join, side, 10);
                assert_eq!(join.retirable(), 0);
                advance(&mut join, side.other(), 9 + ahead);
                assert_eq!(join.retirable(), 0);
                advance(&mut join, side.other(), 10 + ahead);
            } else {
                advance(&mut join, side.other(), 10 + ahead);
                assert_eq!(join.retirable(), 0);
                // The other side's watermark makes nothing late here.
                assert_eq!(join.insert(side, rec(2, 0, 10)), ACCEPTED);
                advance(&mut join, side, 9);
                assert_eq!(join.retirable(), 0);
                advance(&mut join, side, 10);
            }
            let held = join.len(side);
            assert_eq!(join.retirable(), held);
            let mut gone = [Retired::default(); 2];
            assert_eq!(join.retire(&mut gone), Ok(held));
            let retired = join.clone();
            for record in gone.iter().take(held).map(|gone| gone.record) {
                assert_eq!(join.insert(side, record), LATE);
            }
            assert_eq!(join, retired);
            assert_eq!(join.insert(side, rec(4, 0, 11)), ACCEPTED);
        }
    }
}

/// A reach that would overflow saturates at `u64::MAX`: nothing can match the
/// record only when the opposite watermark is `u64::MAX` itself, where no
/// opposite record can arrive at all.
#[test]
fn a_saturated_reach_ends_only_at_the_domain_edge() {
    let max = u64::MAX;
    for (side, bounds) in [
        (Side::Left, Bounds::new(0, max)),
        (Side::Right, Bounds::new(max, 0)),
        (Side::Left, Bounds::new(0, 8)),
    ] {
        let mut join = Join::<2, 2>::new(bounds);
        assert_eq!(join.insert(side, rec(1, 0, max - 3)), ACCEPTED);
        advance(&mut join, side, max - 3);
        advance(&mut join, side.other(), max - 1);
        assert_eq!(join.retirable(), 0);
        advance(&mut join, side.other(), max);
        assert_eq!(join.retirable(), 1);
        // The other side is finished for good; this side is not late for it.
        assert_eq!(join.insert(side, rec(2, 0, max)), ACCEPTED);
        assert_eq!(join.retirable(), 1);
        advance(&mut join, side, max - 1);
        assert_eq!(join.retirable(), 1);
        advance(&mut join, side, max);
        let mut gone = [Retired::default(); 2];
        assert_eq!(join.retire(&mut gone), Ok(2));
        let retired = join.clone();
        assert_eq!(join.insert(side, rec(1, 0, max - 3)), LATE);
        assert_eq!(join.insert(side, rec(2, 0, max)), LATE);
        assert_eq!(join, retired);
    }
}

/// The price of the rule: a side whose own watermark lags keeps its records,
/// however far the other side has gone, and the bound on that is `Full`.
#[test]
fn a_lagging_own_watermark_keeps_records_until_the_side_is_full() {
    let mut join = Join::<2, 2>::new(Bounds::new(0, 0));
    assert_eq!(join.insert(Side::Left, rec(1, 0, 5)), ACCEPTED);
    assert_eq!(join.insert(Side::Left, rec(2, 0, 6)), ACCEPTED);
    advance(&mut join, Side::Right, u64::MAX);
    let lagging = join.clone();
    assert_eq!(join.retirable(), 0);
    assert_eq!(join.retire(&mut [Retired::default(); 2]), Ok(0));
    assert_eq!(join.insert(Side::Left, rec(3, 0, 7)), Err(Refusal::Full));
    assert_eq!(join, lagging);
    advance(&mut join, Side::Left, 5);
    assert_eq!(join.retire(&mut [Retired::default(); 2]), Ok(1));
    assert_eq!(join.insert(Side::Left, rec(3, 0, 7)), ACCEPTED);
}

/// Every boundary at once, against wide arithmetic that shares nothing with
/// the core: a record is retirable exactly when both conditions hold; when it
/// is, no opposite record `insert` would still accept matches it and a replay
/// of it is late; when the first condition fails, a partner can still arrive.
#[test]
fn retirable_means_unmatchable_and_late_on_replay_at_every_boundary() {
    let max = u64::MAX;
    let times = [0, 1, 2, 3, 4, max - 4, max - 3, max - 2, max - 1, max];
    let spans = [0, 1, 2, max - 1, max];
    let mut marks = std::vec![None];
    marks.extend(times.map(Some));
    let wide = i128::from;
    let (mut retired, mut kept) = (0_u32, 0_u32);
    for side in [Side::Left, Side::Right] {
        for (before, after, at) in spans.iter().flat_map(|before| {
            let pairs = spans.iter().map(move |after| (*before, *after));
            pairs.flat_map(|(before, after)| times.map(move |at| (before, after, at)))
        }) {
            for (own, other) in marks
                .iter()
                .flat_map(|own| marks.iter().map(move |other| (*own, *other)))
            {
                let mut join = Join::<1, 1>::new(Bounds::new(before, after));
                let record = rec(1, 0, at);
                assert_eq!(join.insert(side, record), ACCEPTED);
                for (side, mark) in [(side, own), (side.other(), other)] {
                    if let Some(value) = mark {
                        advance(&mut join, side, value);
                    }
                }
                let ahead = if side == Side::Left { after } else { before };
                let last = (wide(at) + wide(ahead)).min(wide(max));
                let unmatchable = other.is_some_and(|mark| wide(mark) >= last);
                let late_again = own.is_some_and(|mark| mark >= at);
                assert_eq!(join.retirable() == 1, unmatchable && late_again);

                let partner = |join: &Join<1, 1>, timestamp| {
                    let mut probe = join.clone();
                    let answer = probe.insert(side.other(), rec(2, 0, timestamp));
                    (answer, probe.pending_pairs())
                };
                if !unmatchable {
                    let last = u64::try_from(last).unwrap();
                    assert_eq!(partner(&join, last), (ACCEPTED, 1));
                }
                let held = join.clone();
                if join.retirable() == 0 {
                    assert_eq!(join.retire(&mut [Retired::default(); 1]), Ok(0));
                    assert_eq!(join.insert(side, record), DUPLICATE);
                    assert_eq!(join, held);
                    kept += 1;
                    continue;
                }
                for timestamp in times {
                    let (answer, owed) = partner(&join, timestamp);
                    assert!(answer == LATE || (answer, owed) == (ACCEPTED, 0));
                }
                assert_eq!(join.retire(&mut [Retired::default(); 1]), Ok(1));
                let gone = join.clone();
                assert_eq!(join.insert(side, record), LATE);
                assert_eq!(join, gone);
                retired += 1;
            }
        }
    }
    assert!(retired > 5_000 && kept > 5_000, "{retired} {kept}");
}
