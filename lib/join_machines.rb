# frozen_string_literal: true

require_relative 'join_machines/version'

module JoinMachines
  # A matched pair of record ids sharing a join key.
  Pair = Data.define(:key, :left, :right)
  # A record the join dropped; its payload can be released.
  Retired = Data.define(:side, :id, :key, :timestamp)
end

# Native is required: no algorithm fallback.
require 'join_machines_native/join_machines_native'

module JoinMachines
  # Operations layered over the native join.
  class Join
    # Advance one side from a watermark object the caller owns: anything that
    # answers +value+ with an Integer or nil, such as WindowMachines::Watermark.
    # Returns true exactly when the side now equals that value; a regression
    # returns false and changes nothing. A nil value (nothing observed yet) is
    # an accepted no-op while the side is nil too, and a refused regression
    # once the side holds a value.
    def advance_watermark_from(side, watermark)
      raise TypeError, 'watermark must respond to #value' unless watermark.respond_to?(:value)

      value = watermark.value
      return advance_watermark(side, value) unless value.nil?
      raise FrozenError, "can't modify frozen #{self.class}: #{inspect}" if frozen?

      watermark(side).nil?
    end
  end
end
