# frozen_string_literal: true

require 'minitest/autorun'
require 'join_machines'

# Values are valid or do not exist, and the binding never loses work.
class ValuesTest < Minitest::Test
  J = JoinMachines
  MAX = 2**64 - 1
  BAD_TYPES = [1.2, Float::NAN, '2', nil, :one, Object.new].freeze
  BAD_RANGES = [-1, 2**64].freeze

  # Replaces `klass.new` with `body`, handed the original `new`, for the length
  # of the block. (A Data class defines `new` on its own singleton class, so
  # the original is set aside and put back rather than removed.)
  def redefining(klass, body)
    singleton = klass.singleton_class
    singleton.alias_method(:__original_new, :new)
    singleton.remove_method(:new)
    original = klass.method(:__original_new)
    klass.define_singleton_method(:new) { |*args| body.call(original, *args) }
    yield
  ensure
    singleton.remove_method(:new)
    singleton.alias_method(:new, :__original_new)
    singleton.remove_method(:__original_new)
  end

  def failing(klass, &) = redefining(klass, ->(*) { raise IOError, 'the answer could not be built' }, &)

  # Every Integer field of `klass` refuses a wrong type and an out-of-range value.
  def assert_integer_fields(klass, valid)
    valid.each_index do |index|
      BAD_TYPES.each do |bad|
        args = valid.dup.tap { _1[index] = bad }
        assert_raises(TypeError) { klass.new(*args) }
      end
      BAD_RANGES.each do |bad|
        args = valid.dup.tap { _1[index] = bad }
        assert_raises(RangeError) { klass.new(*args) }
      end
    end
  end

  def test_a_reach_is_held_to_the_core_constructor
    assert_equal [3, 5], J::Reach.new(3, 5).deconstruct
    assert_equal J::Reach.new(7, 7), J::Reach.new(first: 7, last: 7)
    assert_equal J::Reach.new(0, MAX), J::Bounds.new(MAX, MAX).reach(:left, 3)
    assert_raises(RangeError) { J::Reach.new(5, 3) }
    assert_raises(RangeError) { J::Reach.new(first: 5, last: 3) }
    assert_raises(RangeError) { J::Reach.new(3, 5).with(first: 6) }
    assert_integer_fields(J::Reach, [3, 5])
    reach = J::Reach.new(3, 5)
    assert reach.include?(3)
    refute reach.include?(6)
    BAD_TYPES.each { |bad| assert_raises(TypeError) { reach.include?(bad) } }
    BAD_RANGES.each { |bad| assert_raises(RangeError) { reach.include?(bad) } }
    assert_raises(ArgumentError) { J::Reach.new(1) }
  end

  def test_integer_values_hold_only_unsigned_64_bit_integers
    assert_equal [MAX, 0], J::Bounds.new(MAX, 0).deconstruct
    assert_equal [1, 2, 3], J::Pair.new(1, 2, 3).deconstruct
    assert_equal J::Record.new(1, 2, 3), J::Record.new(id: 1, key: 2, timestamp: 3)
    assert_integer_fields(J::Bounds, [1, 2])
    assert_integer_fields(J::Pair, [1, 2, 3])
    assert_integer_fields(J::Record, [1, 2, 3])
    assert_raises(RangeError) { J::Record.new(1, 2, 3).with(timestamp: -1) }
    assert_raises(ArgumentError) { J::Record.new(1, 2) }
  end

  def test_retired_holds_a_side_and_a_record
    record = J::Record.new(1, 2, 3)
    assert_equal record, J::Retired.new(:right, record).record
    assert_raises(RangeError) { J::Retired.new(:middle, record) }
    [nil, 'left', 1].each { |bad| assert_raises(TypeError) { J::Retired.new(bad, record) } }
    [nil, [1, 2, 3], Data.define(:id, :key, :timestamp).new(1, 2, 3)].each do |bad|
      assert_raises(TypeError) { J::Retired.new(:left, bad) }
    end
  end

  def test_a_watermark_value_is_nil_or_an_unsigned_64_bit_integer
    assert_nil J::Watermark.new(nil).value
    assert_equal MAX, J::Watermark.new(MAX).value
    [1.2, '2', :one].each { |bad| assert_raises(TypeError) { J::Watermark.new(bad) } }
    BAD_RANGES.each { |bad| assert_raises(RangeError) { J::Watermark.new(bad) } }
  end

  def test_the_hooks_are_private
    %i[__reach __bounds __pair __record __retired __watermark].each { refute_respond_to J, _1 }
    refute_includes J.constants, :Checked
  end

  def test_a_failed_drain_loses_no_pair
    j = J::Join.new(J::Bounds.new(10, 10))
    j.insert(:right, J::Record.new(1, 7, 5))
    j.insert(:right, J::Record.new(2, 7, 6))
    j.insert(:left, J::Record.new(9, 7, 5))
    failing(J::Pair) { assert_raises(IOError) { j.drain(10) } }
    assert_equal 2, j.pending_pairs
    assert_equal [J::Pair.new(7, 9, 1), J::Pair.new(7, 9, 2)], j.drain(10)
    refute_predicate j, :pending?
  end

  def test_a_failed_retire_keeps_every_record
    j = J::Join.new(J::Bounds.new(0, 0))
    j.insert(:left, J::Record.new(1, 7, 5))
    j.insert(:right, J::Record.new(2, 8, 5))
    j.advance_watermark(:left, 10)
    j.advance_watermark(:right, 10)
    [J::Retired, J::Record].each do |klass|
      failing(klass) { assert_raises(IOError) { j.retire(10) } }
      assert_equal [1, 1, 2], [j.size(:left), j.size(:right), j.retirable]
    end
    expected = [J::Retired.new(:left, J::Record.new(1, 7, 5)), J::Retired.new(:right, J::Record.new(2, 8, 5))]
    assert_equal expected, j.retire(10)
    assert_predicate j, :empty?
  end

  def test_a_reentrant_change_while_building_raises_and_the_outer_call_is_not_applied
    j = J::Join.new(J::Bounds.new(0, 0))
    advance = ->(side, to) { ->(original, *args) { j.advance_watermark(side, to).then { original.call(*args) } } }
    j.insert(:right, J::Record.new(1, 7, 5))
    j.insert(:left, J::Record.new(9, 7, 5))
    redefining(J::Pair, advance.(:right, 5)) { assert_raises(RuntimeError) { j.drain(10) } }
    assert_equal [1, 5], [j.pending_pairs, j.watermark(:right)]
    assert_equal [J::Pair.new(7, 9, 1)], j.drain(10)
    j.advance_watermark(:left, 5)
    redefining(J::Retired, advance.(:left, 6)) { assert_raises(RuntimeError) { j.retire(10) } }
    assert_equal [2, 6], [j.retirable, j.watermark(:left)]
    assert_equal 2, j.retire(10).size
    assert_predicate j, :empty?
  end

  def test_a_freeze_while_building_raises_and_the_call_is_not_applied
    j = J::Join.new(J::Bounds.new(0, 0))
    j.insert(:right, J::Record.new(1, 7, 5))
    j.insert(:left, J::Record.new(9, 7, 5))
    freeze = ->(original, *args) { j.freeze && original.call(*args) }
    redefining(J::Pair, freeze) { assert_raises(FrozenError) { j.drain(10) } }
    assert_equal [1, true], [j.pending_pairs, j.frozen?]
    k = J::Join.new(J::Bounds.new(0, 0))
    k.insert(:left, J::Record.new(1, 7, 5))
    k.advance_watermark(:left, 10)
    k.advance_watermark(:right, 10)
    assert_equal 1, k.retirable
    freeze = ->(original, *args) { k.freeze && original.call(*args) }
    redefining(J::Retired, freeze) { assert_raises(FrozenError) { k.retire(10) } }
    assert_equal [1, 1], [k.size(:left), k.retirable]
  end

  # A sibling's watermark is read through `#value`, Ruby code that runs
  # before the core: one that freezes the join is FrozenError, not applied.
  def test_a_sibling_watermark_that_freezes_the_join_is_not_applied
    j = J::Join.new(J::Bounds.new(0, 0))
    sibling = Object.new
    sibling.define_singleton_method(:value) { j.freeze && 9 }
    assert_raises(FrozenError) { j.advance_watermark_from(:left, sibling) }
    assert_nil j.watermark(:left)
  end

  def test_value_classes_are_resolved_at_load
    original = J.send(:remove_const, :Pair)
    J.const_set(:Pair, Data.define(:key, :left, :right))
    GC.compact # the resolved classes are pinned
    j = J::Join.new(J::Bounds.new(0, 0))
    j.insert(:right, J::Record.new(1, 7, 5))
    j.insert(:left, J::Record.new(9, 7, 5))
    assert_instance_of original, j.drain(1).first
  ensure
    J.send(:remove_const, :Pair)
    J.const_set(:Pair, original)
  end

  VALUES = [J::Reach, J::Bounds, J::Pair, J::Record, J::Retired, J::Watermark].freeze

  def test_values_are_sealed
    VALUES.each do |value|
      assert_raises(NoMethodError) { value.allocate }
      assert_raises(TypeError) { Class.new(value) { def initialize(**) = nil } }
    end
    # A named subclass raises too, but Ruby binds the constant before
    # `inherited` runs, so the constant leaks. The native side refuses it.
    assert_raises(TypeError) { self.class.class_eval('class LeakedPair < JoinMachines::Pair; end') }
    assert self.class.const_defined?(:LeakedPair, false)
  end

  # The leaked constant of a named subclass, with `initialize` bypassed.
  def forged(name, parent)
    assert_raises(TypeError) { self.class.class_eval("class #{name} < #{parent}; end", __FILE__, __LINE__) }
    self.class.const_get(name).tap do |leaked|
      leaked.class_eval { def initialize(**kw) = Data.instance_method(:initialize).bind_call(self, **kw) }
    end
  end

  # Sealing stops accidents, not reflection: a leaked subclass binding
  # `Data#initialize` directly, or a Marshal stream with edited members,
  # builds an invalid value. The native boundary refuses both, before the core.
  def test_forged_values_never_reach_the_core
    j = J::Join.new(J::Bounds.new(0, 0))
    record = forged(:ForgedRecord, 'JoinMachines::Record').new(id: -1, key: 'k', timestamp: nil)
    assert_raises(TypeError) { j.insert(:left, record) }
    assert_raises(TypeError) { J::Retired.new(:left, record) }
    bounds = forged(:ForgedBounds, 'JoinMachines::Bounds').new(before: 1, after: 2)
    assert_raises(TypeError) { J::Join.new(bounds) }
    assert_raises(TypeError) { bounds.reach(:left, 1) }
    reach = forged(:ForgedReach, 'JoinMachines::Reach').new(first: 9, last: 3)
    assert_raises(TypeError) { reach.include?(4) }
    # A subclass of the gem's own Watermark is not duck-typed as a sibling's.
    mark = forged(:ForgedWatermark, 'JoinMachines::Watermark').new(value: 4)
    assert_raises(TypeError) { j.advance_watermark_from(:left, mark) }
    assert_equal [0, nil], [j.size(:left), j.watermark(:left)]
  end

  def test_marshalled_values_with_edited_members_are_refused
    j = J::Join.new(J::Bounds.new(0, 0))
    record = edited(J::Record.new(1, 7, 10), "idi\x06", "idi\xFA") # id -1
    assert_raises(RangeError) { j.insert(:left, record) }
    reach = edited(J::Reach.new(3, 9), "firsti\b", "firsti\x0E")
    reach = Marshal.load(Marshal.dump(reach).sub("lasti\x0E".b, "lasti\b")) # rubocop:disable Security/MarshalLoad
    assert_equal [9, 3], reach.deconstruct
    assert_raises(RangeError) { reach.include?(4) }
    mark = edited(J::Watermark.new(9), "valuei\x0E", "valuei\xFA") # value -1
    assert_raises(RangeError) { j.advance_watermark_from(:left, mark) }
    assert_equal [0, nil], [j.size(:left), j.watermark(:left)]
  end

  def edited(value, from, to)
    Marshal.load(Marshal.dump(value).sub(from.b, to.b)) # rubocop:disable Security/MarshalLoad
  end
end
