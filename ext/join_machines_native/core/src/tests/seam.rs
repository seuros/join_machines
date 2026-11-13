use super::*;

#[test]
fn caller_owned_watermark_in_lockstep_equals_the_joins_own() {
    let mut join = Join::<4, 4>::new(Bounds::new(2, 5));
    let mut owned = [Watermark::default(); 2];
    for (index, side) in [Side::Left, Side::Right].into_iter().enumerate() {
        assert_eq!(join.side_watermark(side), owned[index]);
        // Both uninitialized: nothing to report, nothing to refuse.
        assert!(join.advance_watermark_from(side, &owned[index]));
        assert_eq!(join.watermark(side), None);
    }
    for (index, value) in [(0, 3), (1, 9), (0, 3), (0, 7), (1, u64::MAX), (0, 8)] {
        let side = [Side::Left, Side::Right][index];
        assert!(owned[index].advance(value));
        assert!(join.advance_watermark_from(side, &owned[index]));
        assert_eq!(join.side_watermark(Side::Left), owned[0]);
        assert_eq!(join.side_watermark(Side::Right), owned[1]);
        assert_eq!(join.watermark(side), owned[index].value());
    }
    // The integer path and the sibling path drive the same state.
    assert!(join.advance_watermark(Side::Left, 20));
    assert!(owned[0].advance(20));
    assert_eq!(join.side_watermark(Side::Left), owned[0]);
    assert_eq!(join.insert(Side::Left, rec(1, 1, 20)), Err(Refusal::Late));
    assert_eq!(join.insert(Side::Left, rec(1, 1, 21)), Ok(()));
}

#[test]
fn regression_through_a_caller_watermark_is_refused_without_mutation() {
    let mut join = Join::<4, 4>::new(Bounds::new(2, 5));
    assert_eq!(join.insert(Side::Left, rec(1, 9, 10)), Ok(()));
    assert_eq!(join.insert(Side::Right, rec(2, 9, 30)), Ok(()));
    assert!(join.advance_watermark(Side::Right, 12));
    let before = join.clone();

    let mut behind = Watermark::default();
    assert!(behind.advance(11));
    assert!(!join.advance_watermark_from(Side::Right, &behind));
    assert_eq!(join, before);
    // An uninitialized source is behind an initialized side: refused too.
    let unset = Watermark::default();
    assert!(!join.advance_watermark_from(Side::Right, &unset));
    assert_eq!(join, before);
    assert_eq!(join.watermark(Side::Right), Some(12));
    // The source is only read.
    assert_eq!(behind.value(), Some(11));
    assert_eq!(unset.value(), None);
    // The untouched side still accepts the same source.
    assert!(join.advance_watermark_from(Side::Left, &unset));
    assert!(join.advance_watermark_from(Side::Left, &behind));
    assert_eq!(join.watermark(Side::Left), Some(11));
}

#[test]
fn output_element_defaults_are_overwritten_placeholders() {
    assert_eq!(Pair::default(), BLANK);
    assert_eq!(Retired::default(), GONE);
    let mut join = Join::<4, 4>::new(Bounds::new(0, 0));
    assert_eq!(join.insert(Side::Right, rec(7, 3, 5)), Ok(()));
    assert_eq!(join.insert(Side::Left, rec(8, 3, 5)), Ok(()));
    let mut pairs = [Pair::default(); 3];
    assert_eq!(join.drain(&mut pairs), 1);
    assert_eq!(
        pairs,
        [
            Pair {
                key: 3,
                left: 8,
                right: 7
            },
            Pair::default(),
            Pair::default()
        ]
    );
    assert!(join.advance_watermark(Side::Left, 5));
    let mut gone = [Retired::default(); 3];
    assert_eq!(join.retire(&mut gone), Ok(1));
    assert_eq!(gone[0].record, rec(7, 3, 5));
    assert_eq!(gone[0].side, Side::Right);
    assert_eq!(gone[1..], [Retired::default(); 2]);
}
