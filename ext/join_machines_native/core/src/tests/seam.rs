use super::*;

#[test]
fn caller_owned_watermark_in_lockstep_equals_the_joins_own() {
    let mut join = Join::<4, 4>::new(Bounds::new(2, 5));
    let mut owned = [Watermark::default(); 2];
    for (index, side) in [Side::Left, Side::Right].into_iter().enumerate() {
        assert_eq!(join.side_watermark(side), owned[index]);
        // Both uninitialized: nothing to report, nothing to refuse.
        assert_eq!(
            join.advance_watermark_from(side, &owned[index]),
            Ok(Advancing::Repeated)
        );
        assert_eq!(join.watermark(side), None);
    }
    for (index, value) in [(0, 3), (1, 9), (0, 3), (0, 7), (1, u64::MAX), (0, 8)] {
        let side = [Side::Left, Side::Right][index];
        let expected = if owned[index].value() == Some(value) {
            Advancing::Repeated
        } else {
            Advancing::Advanced
        };
        assert_eq!(owned[index].advance(value), Ok(expected));
        let held = join.clone();
        assert_eq!(
            join.advance_watermark_from(side, &owned[index]),
            Ok(expected)
        );
        assert_eq!(join == held, expected == Advancing::Repeated);
        assert_eq!(join.side_watermark(Side::Left), owned[0]);
        assert_eq!(join.side_watermark(Side::Right), owned[1]);
        assert_eq!(join.watermark(side), owned[index].value());
    }
    // The integer path and the sibling path drive the same state.
    assert_eq!(
        join.advance_watermark(Side::Left, 20),
        Ok(Advancing::Advanced)
    );
    assert_eq!(owned[0].advance(20), Ok(Advancing::Advanced));
    assert_eq!(join.side_watermark(Side::Left), owned[0]);
    assert_eq!(join.insert(Side::Left, rec(1, 1, 20)), Err(Refusal::Late));
    // Lateness is judged on the record's own side only: the right watermark
    // at `u64::MAX` refuses every right record and no left one.
    assert_eq!(join.insert(Side::Left, rec(1, 1, 21)), ACCEPTED);
    assert_eq!(join.insert(Side::Right, rec(1, 1, u64::MAX)), LATE);
    assert_eq!((join.len(Side::Left), join.len(Side::Right)), (1, 0));
    // No right record can ever match it, but it leaves only once its own
    // watermark reaches it and a replay would be late.
    assert_eq!(join.retirable(), 0);
    advance(&mut join, Side::Left, 21);
    assert_eq!(join.retirable(), 1);
}

#[test]
fn regression_through_a_caller_watermark_is_refused_without_mutation() {
    let mut join = Join::<4, 4>::new(Bounds::new(2, 5));
    assert_eq!(
        join.insert(Side::Left, rec(1, 9, 10)),
        Ok(Inserting::Accepted)
    );
    assert_eq!(
        join.insert(Side::Right, rec(2, 9, 30)),
        Ok(Inserting::Accepted)
    );
    assert_eq!(
        join.advance_watermark(Side::Right, 12),
        Ok(Advancing::Advanced)
    );
    let before = join.clone();

    let behind = Watermark::at(11);
    assert_eq!(
        join.advance_watermark_from(Side::Right, &behind),
        Err(Refusal::WatermarkRegressed)
    );
    assert_eq!(join, before);
    // An uninitialized source is behind an initialized side: refused too.
    let unset = Watermark::default();
    assert_eq!(
        join.advance_watermark_from(Side::Right, &unset),
        Err(Refusal::WatermarkRegressed)
    );
    assert_eq!(join, before);
    assert_eq!(join.watermark(Side::Right), Some(12));
    // The source is only read.
    assert_eq!(behind.value(), Some(11));
    assert_eq!(unset.value(), None);
    // The untouched side still accepts the same source.
    assert_eq!(
        join.advance_watermark_from(Side::Left, &unset),
        Ok(Advancing::Repeated)
    );
    assert_eq!(
        join.advance_watermark_from(Side::Left, &behind),
        Ok(Advancing::Advanced)
    );
    let held = join.clone();
    assert_eq!(
        join.advance_watermark_from(Side::Left, &behind),
        Ok(Advancing::Repeated)
    );
    assert_eq!(join, held);
    assert_eq!(join.watermark(Side::Left), Some(11));
}

#[test]
fn output_element_defaults_are_overwritten_placeholders() {
    assert_eq!(Pair::default(), BLANK);
    assert_eq!(Retired::default(), GONE);
    let mut join = Join::<4, 4>::new(Bounds::new(0, 0));
    assert_eq!(
        join.insert(Side::Right, rec(7, 3, 5)),
        Ok(Inserting::Accepted)
    );
    assert_eq!(
        join.insert(Side::Left, rec(8, 3, 5)),
        Ok(Inserting::Accepted)
    );
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
    advance(&mut join, Side::Left, 5);
    advance(&mut join, Side::Right, 5);
    let mut gone = [Retired::default(); 3];
    assert_eq!(join.retire(&mut gone), Ok(2));
    assert_eq!((gone[0].side, gone[0].record), (Side::Left, rec(8, 3, 5)));
    assert_eq!((gone[1].side, gone[1].record), (Side::Right, rec(7, 3, 5)));
    assert_eq!(gone[2], Retired::default());
}
