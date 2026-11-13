//! Re-inserting a held record: honoured repeat, mismatch, and what "held" means.
use super::*;

#[test]
fn an_identical_reinsert_of_a_held_record_is_honoured() {
    let mut join = Join::<2, 2>::new(Bounds::new(1, 1));
    assert_eq!(join.insert(Side::Left, rec(1, 0, 5)), ACCEPTED);
    let held = join.clone();
    assert_eq!(join.insert(Side::Left, rec(1, 0, 5)), DUPLICATE);
    assert_eq!(join, held);
    // Identities are per side: the same record on the other input is new.
    assert_eq!(join.insert(Side::Right, rec(1, 0, 5)), ACCEPTED);
}

#[test]
fn a_held_identity_with_other_contents_is_a_mismatch() {
    let mut join = Join::<2, 2>::new(Bounds::new(1, 1));
    assert_eq!(join.insert(Side::Left, rec(1, 0, 5)), ACCEPTED);
    let held = join.clone();
    for other in [rec(1, 1, 5), rec(1, 0, 6), rec(1, 9, 9)] {
        assert_eq!(join.insert(Side::Left, other), MISMATCH);
        assert_eq!(join, held);
    }
}

#[test]
fn a_repeat_owes_no_pair_twice_while_pending_or_after_draining() {
    let mut join = Join::<2, 2>::new(Bounds::new(5, 5));
    assert_eq!(join.insert(Side::Right, rec(7, 3, 10)), ACCEPTED);
    assert_eq!(join.insert(Side::Left, rec(1, 3, 10)), ACCEPTED);
    assert_eq!(join.pending_pairs(), 1);
    // Pairs still owed: the held record answers before `Pending` does, for
    // the probing record and for its partner alike.
    let owing = join.clone();
    assert_eq!(join.insert(Side::Left, rec(1, 3, 10)), DUPLICATE);
    assert_eq!(join.insert(Side::Right, rec(7, 3, 10)), DUPLICATE);
    assert_eq!(join.insert(Side::Left, rec(1, 3, 11)), MISMATCH);
    assert_eq!(
        join.insert(Side::Left, rec(2, 3, 10)),
        Err(Refusal::Pending)
    );
    assert_eq!(join, owing);
    assert_eq!(
        drain_all(&mut join, 1),
        [Pair {
            key: 3,
            left: 1,
            right: 7
        }]
    );
    // Matched and drained is still held: a repeat emits nothing again.
    let drained = join.clone();
    assert_eq!(join.insert(Side::Left, rec(1, 3, 10)), DUPLICATE);
    assert_eq!(join.insert(Side::Right, rec(7, 3, 10)), DUPLICATE);
    assert_eq!(join, drained);
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
    let held = join.clone();
    assert_eq!(join.insert(Side::Left, rec(1, 0, 5)), DUPLICATE);
    assert_eq!(join.insert(Side::Left, rec(1, 0, 20)), MISMATCH);
    // A new identity meets the usual checks: late, then full.
    assert_eq!(join.insert(Side::Left, rec(2, 0, 5)), Err(Refusal::Late));
    assert_eq!(join.insert(Side::Left, rec(2, 0, 20)), Err(Refusal::Full));
    assert_eq!(join, held);
}
