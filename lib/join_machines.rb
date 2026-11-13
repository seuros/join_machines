# frozen_string_literal: true

require_relative 'join_machines/version'

module JoinMachines
  # Inclusive range of opposite-side event times a record can match.
  Reach = Data.define(:first, :last) do
    # Whether an opposite-side timestamp is inside the range (the core's `contains`).
    def include?(timestamp)
      raise TypeError, 'expected an Integer (unsigned 64-bit)' unless timestamp.is_a?(Integer)

      first <= timestamp && timestamp <= last
    end
  end

  # Interval bounds: a pair matches when `left - before <= right <= left + after`
  # in event time. `reach` is native.
  Bounds = Data.define(:before, :after) do
    # Opposite-side timestamps reachable from a record on `side`, saturating
    # at the edges of 0..2**64-1.
    def reach(side, timestamp) = Reach.new(*native_reach(side, timestamp))
  end

  # A matched pair of record ids sharing a join key.
  Pair = Data.define(:key, :left, :right)
  # Join metadata for one externally stored record.
  Record = Data.define(:id, :key, :timestamp)
  # A record the join dropped; its payload can be released.
  Retired = Data.define(:side, :record)

  # One input's event-time progress, as `Join#side_watermark` answers it: a
  # frozen copy whose `value` is an Integer, or nil before the first watermark.
  # It reads the way `Join#advance_watermark_from` expects, so it can be handed
  # to another join; it does not advance.
  Watermark = Data.define(:value)
end

# Native is required: no algorithm fallback. It builds the Data values above
# and adds `Bounds#reach`, so they must exist first.
require 'join_machines_native/join_machines_native'
