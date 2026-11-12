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
