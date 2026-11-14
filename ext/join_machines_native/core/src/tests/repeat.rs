//! Re-inserting a held record: honoured repeat, mismatch, and what "held" means.
use super::*;

#[test]
fn an_identical_reinsert_is_honoured_and_other_contents_are_a_mismatch() {
    let mut join = Join::<2, 2>::new(Bounds::new(1, 1));
    assert_eq!(join.insert(Side::Left, rec(1, 0, 5)), ACCEPTED);
    assert_eq!(
        unchanged(&mut join, |j| j.insert(Side::Left, rec(1, 0, 5))),
        DUPLICATE
    );
    for other in [rec(1, 1, 5), rec(1, 0, 6), rec(1, 9, 9)] {
        refuses(&mut join, Refusal::Mismatch, |j| {
            j.insert(Side::Left, other)
        });
    }
    // Identities are per side: the same record on the other input is new.
    assert_eq!(join.insert(Side::Right, rec(1, 0, 5)), ACCEPTED);
}

#[test]
fn a_repeat_owes_no_pair_twice_while_pending_or_after_draining() {
    let mut join = Join::<2, 2>::new(Bounds::new(5, 5));
    assert_eq!(join.insert(Side::Right, rec(7, 3, 10)), ACCEPTED);
    assert_eq!(join.insert(Side::Left, rec(1, 3, 10)), ACCEPTED);
    assert_eq!(join.pending_pairs(), 1);
    // Pairs still owed: the held record answers before `Pending` does, for
    // the probing record and for its partner alike.
    for (side, record) in [(Side::Left, rec(1, 3, 10)), (Side::Right, rec(7, 3, 10))] {
        assert_eq!(unchanged(&mut join, |j| j.insert(side, record)), DUPLICATE);
    }
    refuses(&mut join, Refusal::Mismatch, |j| {
        j.insert(Side::Left, rec(1, 3, 11))
    });
    refuses(&mut join, Refusal::Pending, |j| {
        j.insert(Side::Left, rec(2, 3, 10))
    });
    assert_eq!(drain_all(&mut join, 1), [pair(3, 1, 7)]);
    // Matched and drained is still held: a repeat emits nothing again.
    for (side, record) in [(Side::Left, rec(1, 3, 10)), (Side::Right, rec(7, 3, 10))] {
        assert_eq!(unchanged(&mut join, |j| j.insert(side, record)), DUPLICATE);
    }
    assert!(!join.pending());
}

#[test]
fn a_held_record_answers_before_lateness_and_capacity() {
    let mut join = Join::<1, 1>::new(Bounds::new(0, 50));
    assert_eq!(join.insert(Side::Left, rec(1, 0, 5)), ACCEPTED);
    // Its own watermark passes it; it stays held until the right one does too.
    assert_eq!(
        join.advance_watermark(Side::Left, 9),
        Ok(Advancing::Advanced)
    );
    assert_eq!(
        unchanged(&mut join, |j| j.insert(Side::Left, rec(1, 0, 5))),
        DUPLICATE
    );
    refuses(&mut join, Refusal::Mismatch, |j| {
        j.insert(Side::Left, rec(1, 0, 20))
    });
    // A new identity meets the usual checks: late, then full.
    refuses(&mut join, Refusal::Late, |j| {
        j.insert(Side::Left, rec(2, 0, 5))
    });
    refuses(&mut join, Refusal::Full, |j| {
        j.insert(Side::Left, rec(2, 0, 20))
    });
}
