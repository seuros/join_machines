# frozen_string_literal: true
require 'minitest/autorun'
require 'join_machines'

class JoinTest < Minitest::Test
  MAX = 2**64 - 1

  def join(before: 2, after: 5) = JoinMachines::Join.new(before:, after:)

  def test_construction_and_capacity
    j = join
    assert_equal [2, 5], [j.before, j.after]
    assert_equal 64, JoinMachines::CAPACITY
    assert_equal JoinMachines::CAPACITY, j.capacity
    assert j.empty?
    assert_equal [0, 0], [j.size(:left), j.size(:right)]
    assert_raises(ArgumentError) { JoinMachines::Join.new }
    assert_raises(ArgumentError) { JoinMachines::Join.new(before: 1) }
    assert_raises(ArgumentError) { JoinMachines::Join.new(1, 2) }
    assert_raises(TypeError) { JoinMachines::Join.allocate }
    assert_match(/\A\d+\.\d+\.\d+\z/, JoinMachines::VERSION)
  end

  def test_inclusive_asymmetric_matching_from_either_side
    j = join
    [[1, 7], [2, 8], [3, 15], [4, 16]].each do |id, at|
      assert_equal :accepted, j.insert(:right, id, 9, at)
      refute j.pending?
    end
    assert_equal :accepted, j.insert(:right, 5, 1, 10)
    assert_equal :accepted, j.insert(:left, 100, 9, 10)
    assert j.pending?
    assert_equal 2, j.pending_pairs
    pairs = j.drain(10)
    assert_equal [JoinMachines::Pair.new(key: 9, left: 100, right: 2),
                  JoinMachines::Pair.new(key: 9, left: 100, right: 3)], pairs
    assert pairs.all?(&:frozen?)
    refute j.pending?
    assert_equal [], j.drain(10)
    assert_equal :accepted, j.insert(:right, 6, 9, 12)
    assert_equal [JoinMachines::Pair.new(9, 100, 6)], j.drain(1)
    assert j.include?(:right, 6)
    refute j.include?(:left, 6)
    refute j.empty?
  end

  def test_bounded_drain_resumes_and_blocks_other_mutations
    j = join(before: 100, after: 100)
    40.times { |i| assert_equal :accepted, j.insert(:right, i, 1, 50 + i) }
    assert_equal :accepted, j.insert(:left, 500, 1, 60)
    assert_equal 40, j.pending_pairs
    assert_equal [], j.drain(0)
    assert_equal :pending, j.insert(:left, 501, 1, 60)
    assert_equal :pending, j.retire(10)
    assert j.advance_watermark(:right, 1_000)
    assert_equal 1, j.size(:left)
    seen = j.drain(3) + j.drain(20) + j.drain(MAX)
    assert_equal (0...40).to_a, seen.map(&:right)
    assert_equal [500], seen.map(&:left).uniq
    refute j.pending?
    assert_equal 0, j.pending_pairs
  end

  def test_late_duplicate_and_full_are_symbols
    j = join
    assert_equal :accepted, j.insert(:left, 1, 0, 5)
    assert_equal :duplicate, j.insert(:left, 1, 3, 9)
    assert_equal :accepted, j.insert(:right, 1, 7, 50)
    assert j.advance_watermark(:left, 6)
    assert_equal :late, j.insert(:left, 2, 0, 6)
    refute j.advance_watermark(:left, 5)
    assert_equal 6, j.watermark(:left)
    assert_nil j.watermark(:right)
    (2..64).each { |id| assert_equal :accepted, j.insert(:left, id, 0, 1_000 + id) }
    assert_equal :full, j.insert(:left, 65, 0, 2_000)
    assert_equal 64, j.size(:left)
  end

  def test_watermark_driven_retirement_is_bounded
    j = join
    [[1, 10], [2, 11], [3, 12]].each { |id, at| j.insert(:left, id, 0, at) }
    j.insert(:right, 7, 1, 10)
    assert_equal 0, j.retirable
    assert_equal [], j.retire(10)
    assert j.advance_watermark(:right, 14)
    assert_equal 0, j.retirable
    assert j.advance_watermark(:right, 16)
    assert_equal 2, j.retirable
    assert_equal [], j.retire(0)
    assert_equal [JoinMachines::Retired.new(side: :left, id: 1, key: 0, timestamp: 10)], j.retire(1)
    assert_equal [2], j.retire(MAX).map(&:id)
    assert j.advance_watermark(:left, 12)
    assert_equal [JoinMachines::Retired.new(:right, 7, 1, 10)], j.retire(5)
    assert_equal :accepted, j.insert(:right, 7, 0, 17)
    assert_equal [JoinMachines::Pair.new(0, 3, 7)], j.drain(5)
  end

  def test_u64_limits
    j = join(before: MAX, after: MAX)
    assert_equal :accepted, j.insert(:left, MAX, MAX, 0)
    assert_equal :accepted, j.insert(:right, MAX, MAX, MAX)
    assert_equal [JoinMachines::Pair.new(MAX, MAX, MAX)], j.drain(1)
    assert j.advance_watermark(:right, MAX)
    assert_equal :late, j.insert(:right, 1, 1, MAX)
    assert_equal [:left], j.retire(5).map(&:side)
  end

  def test_strict_inputs_before_mutation
    j = join
    [1.0, nil, '1', Object.new].each do |bad|
      assert_raises(TypeError) { JoinMachines::Join.new(before: bad, after: 1) }
      assert_raises(TypeError) { j.insert(:left, bad, 1, 1) }
      assert_raises(TypeError) { j.insert(:left, 1, bad, 1) }
      assert_raises(TypeError) { j.insert(:left, 1, 1, bad) }
      assert_raises(TypeError) { j.insert(bad, 1, 1, 1) }
      assert_raises(TypeError) { j.advance_watermark(:left, bad) }
      assert_raises(TypeError) { j.drain(bad) }
      assert_raises(TypeError) { j.retire(bad) }
      assert_raises(TypeError) { j.include?(:left, bad) }
      assert_raises(TypeError) { j.size(bad) }
      assert_raises(TypeError) { j.watermark(bad) }
    end
    [-1, MAX + 1].each do |bad|
      assert_raises(RangeError) { JoinMachines::Join.new(before: 1, after: bad) }
      assert_raises(RangeError) { j.insert(:left, bad, 1, 1) }
      assert_raises(RangeError) { j.insert(:left, 1, 1, bad) }
      assert_raises(RangeError) { j.advance_watermark(:right, bad) }
      assert_raises(RangeError) { j.drain(bad) }
      assert_raises(RangeError) { j.retire(bad) }
    end
    assert_raises(ArgumentError) { j.insert(:middle, 1, 1, 1) }
    assert_raises(ArgumentError) { j.size(:middle) }
    assert j.empty?
    assert_nil j.watermark(:left)
  end

  # Reads like WindowMachines::Watermark without depending on that gem.
  class Mark
    attr_reader :value

    def advance(value)
      return false if @value && value < @value

      @value = value
      true
    end
  end

  def test_caller_owned_watermark_stays_in_lockstep
    j = join
    marks = { left: Mark.new, right: Mark.new }
    marks.each do |side, mark|
      assert j.advance_watermark_from(side, mark)
      assert_nil j.watermark(side)
    end
    [[:left, 3], [:right, 9], [:left, 3], [:left, 7], [:right, MAX]].each do |side, value|
      assert marks[side].advance(value)
      assert j.advance_watermark_from(side, marks[side])
      assert_equal marks.transform_values(&:value), { left: j.watermark(:left), right: j.watermark(:right) }
    end
    assert_equal :late, j.insert(:left, 1, 1, 7)
    assert_equal :accepted, j.insert(:left, 1, 1, 8)
  end

  def test_regressing_caller_watermark_is_refused_without_mutation
    j = join
    j.insert(:left, 1, 9, 10)
    assert j.advance_watermark(:right, 12)
    behind = Mark.new
    behind.advance(11)
    refute j.advance_watermark_from(:right, behind)
    refute j.advance_watermark_from(:right, Mark.new)
    assert_equal 12, j.watermark(:right)
    assert_nil j.watermark(:left)
    assert_equal [1, 0, 0], [j.size(:left), j.size(:right), j.retirable]
    assert_equal 11, behind.value
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
    j.insert(:left, 1, 1, 1)
    j.freeze
    assert_raises(FrozenError) { j.insert(:right, 1, 1, 1) }
    assert_raises(FrozenError) { j.advance_watermark(:left, 1) }
    assert_raises(FrozenError) { j.drain(1) }
    assert_raises(FrozenError) { j.retire(1) }
    assert_equal 1, j.size(:left)
  end
end
