# frozen_string_literal: true

require_relative 'join_machines/version'

module JoinMachines
  # Every value below is checked by the core when it is built, by hand or by
  # the gem, `with` included: TypeError for a wrong type, RangeError for a
  # value the core rejects. `hook` names the private native check.
  module Checked
    def self.by(hook)
      Module.new do
        define_method(:initialize) do |**fields|
          super(**fields)
          JoinMachines.__send__(hook, *deconstruct)
        end
      end
    end
  end

  # Inclusive range of opposite-side event times a record can match. Like the
  # core's `Reach::new` it holds Integers in 0..2**64-1 with `first <= last`.
  # `include?(timestamp)` is native (the core's `contains`).
  Reach = Data.define(:first, :last) { include Checked.by(:__reach) }

  # Interval bounds: a pair matches when `left - before <= right <= left + after`
  # in event time. Both are Integers in 0..2**64-1. `reach(side, timestamp)` is
  # native: the opposite-side timestamps reachable from a record on `side`, a
  # Reach saturating at the edges of 0..2**64-1.
  Bounds = Data.define(:before, :after) { include Checked.by(:__bounds) }

  # A matched pair of record ids sharing a join key; Integers in 0..2**64-1.
  Pair = Data.define(:key, :left, :right) { include Checked.by(:__pair) }
  # Join metadata for one externally stored record; Integers in 0..2**64-1.
  Record = Data.define(:id, :key, :timestamp) { include Checked.by(:__record) }
  # A record the join dropped; its payload can be released. `side` is :left or
  # :right, `record` a Record.
  Retired = Data.define(:side, :record) { include Checked.by(:__retired) }

  # One input's event-time progress, as `Join#side_watermark` answers it: a
  # frozen copy whose `value` is an Integer in 0..2**64-1, or nil before the
  # first watermark. It reads the way `Join#advance_watermark_from` expects, so
  # it can be handed to another join; it does not advance.
  Watermark = Data.define(:value) { include Checked.by(:__watermark) }
end

# Native is required: no algorithm fallback. It validates and builds the Data
# values above, resolves their classes once at load and adds `Bounds#reach`
# and `Reach#include?`, so they must exist first.
require 'join_machines_native/join_machines_native'

module JoinMachines
  private_class_method :__reach, :__bounds, :__pair, :__record, :__retired, :__watermark
  private_constant :Checked

  # Sealed: no subclass can skip the check by overriding `initialize`, and no
  # bare `allocate` builds a value that never ran it. Sealing stops
  # accidents, not reflection, so the native side reads only instances of
  # exactly these classes and re-validates their members through the core.
  [Reach, Bounds, Pair, Record, Retired, Watermark].each do |value|
    value.singleton_class.undef_method(:allocate)
    value.define_singleton_method(:inherited) { |_| raise TypeError, "#{value} cannot be subclassed" }
  end
end
