# frozen_string_literal: true
require 'minitest/autorun'
require 'join_machines'

class JoinTest < Minitest::Test
  MAX = 2**64 - 1

  J = JoinMachines

  def join(before: 2, after: 5) = J::Join.new(J::Bounds.new(before, after))

  def rec(id, key, timestamp) = J::Record.new(id, key, timestamp)

  def test_construction_and_capacity
    j = join
    assert_equal J::Bounds.new(before: 2, after: 5), j.bounds
    assert_predicate j.bounds, :frozen?
    assert_equal %i[before after], J::Bounds.members
    assert_equal 64, JoinMachines::CAPACITY
    assert_equal [64, 64], [j.capacity(:left), j.capacity(:right)]
    assert j.empty?
    assert_equal [0, 0], [j.size(:left), j.size(:right)]
    assert_raises(ArgumentError) { JoinMachines::Join.new }
    assert_raises(ArgumentError) { JoinMachines::Join.new(1, 2) }
    assert_raises(ArgumentError) { j.capacity }
    assert_raises(ArgumentError) { j.capacity(:middle) }
    assert_raises(TypeError) { JoinMachines::Join.new(before: 1, after: 2) }
    assert_raises(TypeError) { JoinMachines::Join.new([1, 2]) }
    refute_respond_to j, :before
    assert_raises(TypeError) { JoinMachines::Join.allocate }
    assert_match(/\A\d+\.\d+\.\d+\z/, JoinMachines::VERSION)
  end

  def test_inclusive_asymmetric_matching_from_either_side
    j = join
    [[1, 7], [2, 8], [3, 15], [4, 16]].each do |id, at|
      assert_equal :accepted, j.insert(:right, rec(id, 9, at))
      refute j.pending?
    end
    assert_equal :accepted, j.insert(:right, rec(5, 1, 10))
    assert_equal :accepted, j.insert(:left, rec(100, 9, 10))
    assert j.pending?
    assert_equal 2, j.pending_pairs
    pairs = j.drain(10)
    assert_equal [JoinMachines::Pair.new(key: 9, left: 100, right: 2),
                  JoinMachines::Pair.new(key: 9, left: 100, right: 3)], pairs
    assert pairs.all?(&:frozen?)
    refute j.pending?
    assert_equal [], j.drain(10)
    assert_equal :accepted, j.insert(:right, rec(6, 9, 12))
    assert_equal [JoinMachines::Pair.new(9, 100, 6)], j.drain(1)
    assert j.include?(:right, 6)
    refute j.include?(:left, 6)
    refute j.empty?
  end

  def test_bounded_drain_resumes_and_blocks_other_mutations
    j = join(before: 100, after: 100)
    40.times { |i| assert_equal :accepted, j.insert(:right, rec(i, 1, 50 + i)) }
    assert_equal :accepted, j.insert(:left, rec(500, 1, 60))
    assert_equal 40, j.pending_pairs
    assert_equal [], j.drain(0)
    assert_equal :pending, j.insert(:left, rec(501, 1, 60))
    assert_equal :pending, j.retire(10)
    assert_equal :advanced, j.advance_watermark(:right, 1_000)
    assert_equal 1, j.size(:left)
    seen = j.drain(3) + j.drain(20) + j.drain(MAX)
    assert_equal (0...40).to_a, seen.map(&:right)
    assert_equal [500], seen.map(&:left).uniq
    refute j.pending?
    assert_equal 0, j.pending_pairs
  end

  def test_a_replayed_record_is_honoured_and_other_contents_are_a_mismatch
    j = join
    assert_equal :accepted, j.insert(:right, rec(7, 9, 10))
    assert_equal :accepted, j.insert(:left, rec(1, 9, 10))
    # A held id is answered first, even while pairs are owed.
    assert_equal :duplicate, j.insert(:left, rec(1, 9, 10))
    assert_equal :mismatch, j.insert(:left, rec(1, 9, 11))
    assert_equal :mismatch, j.insert(:left, rec(1, 8, 10))
    assert_equal :pending, j.insert(:left, rec(2, 9, 10))
    assert_equal [1, 1], [j.size(:left), j.pending_pairs]
    assert_equal [JoinMachines::Pair.new(9, 1, 7)], j.drain(8)
    # Matched and drained is still held: nothing is emitted twice.
    assert_equal :duplicate, j.insert(:left, rec(1, 9, 10))
    assert_equal :duplicate, j.insert(:right, rec(7, 9, 10))
    refute j.pending?
    assert_empty j.drain(8)
  end

  def test_a_replay_after_retirement_is_late_and_emits_no_pair_twice
    j = join(before: 2, after: 2)
    left = rec(1, 7, 10)
    assert_equal :accepted, j.insert(:left, left)
    assert_equal :accepted, j.insert(:right, rec(2, 7, 11))
    assert_equal [J::Pair.new(7, 1, 2)], j.drain(1)
    # Only the right watermark moves: nothing can match the left record any
    # more, but a replay of it would not be late, so it is not retired.
    assert_equal :advanced, j.advance_watermark(:right, 12)
    assert_equal 0, j.retirable
    assert_empty j.retire(8)
    assert_equal :duplicate, j.insert(:left, left)
    refute j.pending?
    assert_equal :advanced, j.advance_watermark(:left, 9)
    assert_equal 0, j.retirable
    assert_equal :advanced, j.advance_watermark(:left, 10)
    assert_equal [J::Retired.new(:left, left)], j.retire(8)
    2.times { assert_equal :late, j.insert(:left, left) }
    refute j.pending?
    assert_empty j.drain(8)
    assert_equal 0, j.size(:left)
  end

  def test_a_matchable_record_is_never_late_because_of_the_other_side
    # The right watermark is past everything left 10 can match, the left one
    # is unset, and the partner is still held.
    j = join(before: 2, after: 2)
    assert_equal :accepted, j.insert(:right, rec(2, 7, 11))
    assert_equal :advanced, j.advance_watermark(:right, 12)
    assert_equal :accepted, j.insert(:left, rec(1, 7, 10))
    assert_equal [J::Pair.new(7, 1, 2)], j.drain(8)
    # after: 0, with the right watermark at the record's time and far past it.
    j = join(before: 5, after: 0)
    assert_equal :accepted, j.insert(:right, rec(2, 7, 10))
    assert_equal :advanced, j.advance_watermark(:right, 10)
    assert_equal :accepted, j.insert(:left, rec(1, 7, 10))
    assert_equal [J::Pair.new(7, 1, 2)], j.drain(8)
    assert_equal :advanced, j.advance_watermark(:right, MAX)
    assert_equal :accepted, j.insert(:left, rec(3, 7, 15))
    assert_equal [J::Pair.new(7, 3, 2)], j.drain(8)
    assert_equal :late, j.insert(:right, rec(4, 7, MAX))
  end

  def test_a_lagging_side_keeps_its_records_until_full
    j = join(before: 0, after: 0)
    64.times { |id| assert_equal :accepted, j.insert(:left, rec(id, 0, 5 + id)) }
    assert_equal :advanced, j.advance_watermark(:right, MAX)
    assert_equal 0, j.retirable
    assert_equal :full, j.insert(:left, rec(64, 0, 500))
    assert_equal :advanced, j.advance_watermark(:left, 6)
    assert_equal [0, 1], j.retire(8).map { it.record.id }
    assert_equal :accepted, j.insert(:left, rec(64, 0, 500))
  end

  def test_a_retired_identity_is_not_remembered
    j = join(before: 0, after: 0)
    assert_equal :accepted, j.insert(:left, rec(1, 0, 5))
    assert_equal :advanced, j.advance_watermark(:right, 5)
    assert_empty j.retire(8)
    assert_equal :advanced, j.advance_watermark(:left, 5)
    assert_equal 1, j.retire(8).size
    assert_equal :late, j.insert(:left, rec(1, 0, 5))
    assert_equal :accepted, j.insert(:left, rec(1, 3, 9))
    assert_equal :duplicate, j.insert(:left, rec(1, 3, 9))
    assert_equal :advanced, j.advance_watermark(:right, 9)
    assert_equal :advanced, j.advance_watermark(:left, 9)
    assert_equal 1, j.retire(8).size
    assert_equal :late, j.insert(:left, rec(1, 3, 9))
  end

  def test_late_mismatch_and_full_are_symbols
    j = join
    assert_equal :accepted, j.insert(:left, rec(1, 0, 5))
    assert_equal :mismatch, j.insert(:left, rec(1, 3, 9))
    assert_equal :accepted, j.insert(:right, rec(1, 7, 50))
    assert_equal :advanced, j.advance_watermark(:left, 6)
    assert_equal :late, j.insert(:left, rec(2, 0, 6))
    assert_equal :watermark_regressed, j.advance_watermark(:left, 5)
    assert_equal :repeated, j.advance_watermark(:left, 6)
    assert_equal 6, j.watermark(:left)
    assert_nil j.watermark(:right)
    (2..64).each { |id| assert_equal :accepted, j.insert(:left, rec(id, 0, 1_000 + id)) }
    assert_equal :full, j.insert(:left, rec(65, 0, 2_000))
    assert_equal 64, j.size(:left)
  end

  def test_watermark_driven_retirement_is_bounded
    j = join
    [[1, 10], [2, 11], [3, 12]].each { |id, at| j.insert(:left, rec(id, 0, at)) }
    j.insert(:right, rec(7, 1, 10))
    assert_equal 0, j.retirable
    assert_equal [], j.retire(10)
    assert_equal :advanced, j.advance_watermark(:right, 14)
    assert_equal 0, j.retirable
    assert_equal :advanced, j.advance_watermark(:right, 16)
    # The left watermark has reached neither record: both stay.
    assert_equal 0, j.retirable
    assert_equal :advanced, j.advance_watermark(:left, 10)
    assert_equal 1, j.retirable
    assert_equal :advanced, j.advance_watermark(:left, 11)
    assert_equal 2, j.retirable
    assert_equal [], j.retire(0)
    first = JoinMachines::Record.new(id: 1, key: 0, timestamp: 10)
    assert_equal [JoinMachines::Retired.new(side: :left, record: first)], j.retire(1)
    assert_equal [2], j.retire(MAX).map { _1.record.id }
    assert_equal :advanced, j.advance_watermark(:left, 12)
    gone = j.retire(5)
    assert_equal [JoinMachines::Retired.new(:right, JoinMachines::Record.new(7, 1, 10))], gone
    assert_predicate gone.first, :frozen?
    assert_predicate gone.first.record, :frozen?
    assert_equal %i[side record], JoinMachines::Retired.members
    assert_equal %i[id key timestamp], JoinMachines::Record.members
    assert_equal %i[key left right], JoinMachines::Pair.members
    assert_equal :accepted, j.insert(:right, rec(7, 0, 17))
    assert_equal [JoinMachines::Pair.new(0, 3, 7)], j.drain(5)
  end

  def test_u64_limits
    j = join(before: MAX, after: MAX)
    assert_equal :accepted, j.insert(:left, rec(MAX, MAX, 0))
    assert_equal :accepted, j.insert(:right, rec(MAX, MAX, MAX))
    assert_equal [JoinMachines::Pair.new(MAX, MAX, MAX)], j.drain(1)
    assert_equal :advanced, j.advance_watermark(:right, MAX)
    assert_equal :late, j.insert(:right, rec(1, 1, MAX))
    # The right input is closed for good; the left one is not late for it.
    assert_equal :accepted, j.insert(:left, rec(2, MAX, 5))
    assert_equal [JoinMachines::Pair.new(MAX, 2, MAX)], j.drain(1)
    assert_empty j.retire(5)
    assert_equal :advanced, j.advance_watermark(:left, 0)
    assert_equal [rec(MAX, MAX, 0)], j.retire(5).map(&:record)
    assert_equal :advanced, j.advance_watermark(:left, MAX)
    assert_equal %i[left right], j.retire(5).map(&:side)
    assert_predicate j, :empty?
  end

  def test_strict_inputs_before_mutation
    j = join
    [1.0, nil, '1', Object.new].each do |bad|
      assert_raises(TypeError) { JoinMachines::Join.new(J::Bounds.new(bad, 1)) }
      assert_raises(TypeError) { JoinMachines::Join.new(bad) }
      assert_raises(TypeError) { j.insert(:left, bad) }
      assert_raises(TypeError) { j.side_watermark(bad) }
      assert_raises(TypeError) { j.capacity(bad) }
      assert_raises(TypeError) { j.insert(:left, rec(bad, 1, 1)) }
      assert_raises(TypeError) { j.insert(:left, rec(1, bad, 1)) }
      assert_raises(TypeError) { j.insert(:left, rec(1, 1, bad)) }
      assert_raises(TypeError) { j.insert(bad, rec(1, 1, 1)) }
      assert_raises(TypeError) { j.advance_watermark(:left, bad) }
      assert_raises(TypeError) { j.drain(bad) }
      assert_raises(TypeError) { j.retire(bad) }
      assert_raises(TypeError) { j.include?(:left, bad) }
      assert_raises(TypeError) { j.size(bad) }
      assert_raises(TypeError) { j.watermark(bad) }
    end
    [-1, MAX + 1].each do |bad|
      assert_raises(RangeError) { JoinMachines::Join.new(J::Bounds.new(1, bad)) }
      assert_raises(RangeError) { j.insert(:left, rec(bad, 1, 1)) }
      assert_raises(RangeError) { j.insert(:left, rec(1, 1, bad)) }
      assert_raises(RangeError) { j.advance_watermark(:right, bad) }
      assert_raises(RangeError) { j.drain(bad) }
      assert_raises(RangeError) { j.retire(bad) }
    end
    assert_raises(ArgumentError) { j.insert(:middle, rec(1, 1, 1)) }
    assert_raises(ArgumentError) { j.size(:middle) }
    assert_raises(ArgumentError) { j.insert(:left, 1, 1, 1) }
    assert_raises(TypeError) { j.insert(:left, [1, 1, 1]) }
    assert j.empty?
    assert_nil j.watermark(:left)
  end

  # Reads like WindowMachines::Watermark without depending on that gem.
  class Mark
    attr_reader :value

    def advance(value)
      return :regressed if @value && value < @value
      return :repeated if @value == value

      @value = value
      :advanced
    end
  end

  def test_caller_owned_watermark_stays_in_lockstep
    j = join
    marks = { left: Mark.new, right: Mark.new }
    marks.each do |side, mark|
      assert_equal :repeated, j.advance_watermark_from(side, mark)
      assert_nil j.watermark(side)
    end
    steps = [[:left, 3, :advanced], [:right, 9, :advanced], [:left, 3, :repeated], [:left, 7, :advanced],
             [:right, MAX, :advanced]]
    steps.each do |side, value, answer|
      assert_equal answer, marks[side].advance(value)
      assert_equal answer, j.advance_watermark_from(side, marks[side])
      assert_equal marks.transform_values(&:value), { left: j.watermark(:left), right: j.watermark(:right) }
    end
    assert_equal :late, j.insert(:left, rec(1, 1, 7))
    # Lateness is the record's own side's: the right watermark at MAX refuses
    # every right record and no left one.
    assert_equal :accepted, j.insert(:left, rec(1, 1, 8))
    assert_equal :late, j.insert(:right, rec(1, 1, MAX))
    assert_equal [1, 0, 0], [j.size(:left), j.size(:right), j.retirable]
    assert_equal :advanced, j.advance_watermark(:left, 8)
    assert_equal 1, j.retirable
  end

  def test_regressing_caller_watermark_is_refused_without_mutation
    j = join
    j.insert(:left, rec(1, 9, 10))
    assert_equal :advanced, j.advance_watermark(:right, 12)
    behind = Mark.new
    behind.advance(11)
    assert_equal :watermark_regressed, j.advance_watermark_from(:right, behind)
    assert_equal :watermark_regressed, j.advance_watermark_from(:right, Mark.new)
    assert_equal 12, j.watermark(:right)
    assert_nil j.watermark(:left)
    assert_equal [1, 0, 0], [j.size(:left), j.size(:right), j.retirable]
    assert_equal 11, behind.value
  end

  def test_side_watermark_reads_like_a_watermark
    j = join
    other = join
    mark = j.side_watermark(:left)
    assert_equal J::Watermark.new(nil), mark
    assert_kind_of Data, mark
    assert_predicate mark, :frozen?
    assert_equal %i[value], J::Watermark.members
    assert_equal :repeated, other.advance_watermark_from(:left, mark)
    assert_equal :advanced, j.advance_watermark(:left, 9)
    assert_equal [J::Watermark.new(9), J::Watermark.new(nil)], [j.side_watermark(:left), j.side_watermark(:right)]
    assert_equal j.watermark(:left), j.side_watermark(:left).value
    assert_nil mark.value # a copy, not a view
    assert_equal :advanced, other.advance_watermark_from(:right, j.side_watermark(:left))
    assert_equal :repeated, other.advance_watermark_from(:right, j.side_watermark(:left))
    assert_equal :watermark_regressed, other.advance_watermark_from(:right, j.side_watermark(:right))
    assert_equal 9, other.watermark(:right)
    assert_raises(ArgumentError) { j.side_watermark(:middle) }
  end

  def test_bounds_reach_and_other_side
    bounds = J::Bounds.new(2, 5)
    assert_equal J::Reach.new(first: 8, last: 15), bounds.reach(:left, 10)
    assert_equal J::Reach.new(5, 12), bounds.reach(:right, 10)
    assert_equal J::Reach.new(0, MAX), J::Bounds.new(MAX, MAX).reach(:left, 7)
    assert_equal %i[first last], J::Reach.members
    reach = bounds.reach(:left, 10)
    assert_equal [false, true, true, false], [7, 8, 15, 16].map { reach.include?(it) }
    assert_raises(TypeError) { reach.include?(8.0) }
    assert_raises(TypeError) { bounds.reach(:left, nil) }
    assert_raises(RangeError) { bounds.reach(:left, -1) }
    assert_raises(ArgumentError) { bounds.reach(:middle, 1) }
    assert_raises(TypeError) { J::Bounds.new(1.0, 2).reach(:left, 1) }
    assert_raises(NoMethodError) { bounds.native_reach(:left, 1) }
    assert_equal %i[right left], [J.other(:left), J.other(:right)]
    assert_raises(ArgumentError) { J.other(:middle) }
    assert_raises(TypeError) { J.other('left') }
  end

  def test_advance_watermark_from_is_strict
    j = join
    mark = Mark.new
    [1, nil, '1', Object.new].each do |bad|
      assert_raises(TypeError) { j.advance_watermark_from(:left, bad) }
    end
    [1.0, '1', Object.new].each do |bad|
      assert_raises(TypeError) { j.advance_watermark_from(:left, Data.define(:value).new(bad)) }
    end
    [-1, MAX + 1].each do |bad|
      assert_raises(RangeError) { j.advance_watermark_from(:left, Data.define(:value).new(bad)) }
    end
    assert_raises(TypeError) { j.advance_watermark_from(nil, mark) }
    assert_raises(ArgumentError) { j.advance_watermark_from(:middle, mark) }
    mark.advance(4)
    assert_raises(TypeError) { j.advance_watermark_from(nil, mark) }
    assert_raises(ArgumentError) { j.advance_watermark_from(:middle, mark) }
    assert_nil j.watermark(:left)
    j.freeze
    assert_raises(FrozenError) { j.advance_watermark_from(:left, mark) }
    assert_raises(FrozenError) { j.advance_watermark_from(:left, Mark.new) }
    assert_nil j.watermark(:left)
  end

  def test_frozen_join_refuses_mutation
    j = join
    j.insert(:left, rec(1, 1, 1))
    j.freeze
    assert_raises(FrozenError) { j.insert(:right, rec(1, 1, 1)) }
    assert_raises(FrozenError) { j.advance_watermark(:left, 1) }
    assert_raises(FrozenError) { j.drain(1) }
    assert_raises(FrozenError) { j.retire(1) }
    assert_equal 1, j.size(:left)
  end
end
