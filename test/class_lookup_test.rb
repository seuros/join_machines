# frozen_string_literal: true

require 'minitest/autorun'
require 'open3'
require 'rbconfig'

# The class of the value the extension wraps (Join) is fixed when it loads.
# Nothing a caller does later, redefining the constant or
# `Object.const_get`, reaches a join being built. Each case runs in a fresh
# process: the class is fixed once per process, at load.
class ClassLookupTest < Minitest::Test
  LIB = File.expand_path('../lib', __dir__)

  PRELUDE = <<~RUBY
    require 'join_machines'
    J = JoinMachines
    Join = J::Join
    bounds = J::Bounds.new(2, 5)
    record = J::Record.new(1, 7, 10)
  RUBY

  # Runs `body` after PRELUDE in a new Ruby; answers its last line of output.
  def isolated(body)
    out, err, status = Open3.capture3(RbConfig.ruby, '-I', LIB, '-e', PRELUDE + body)
    assert status.success?, "exited #{status.exitstatus}: #{err}"
    out.lines.last&.chomp
  end

  # Bug: a Join constant redefined before the first join made it an instance
  # of the new class, which has none of the join's methods.
  def test_a_redefined_join_constant_does_not_change_the_class
    assert_equal 'true true 1', isolated(<<~RUBY)
      J.send(:remove_const, :Join)
      J.const_set(:Join, Class.new)
      join = Join.new(bounds)
      join.insert(:left, record)
      puts [join.instance_of?(Join), join.bounds == bounds, join.size(:left)].join(' ')
    RUBY
  end

  # Bug: building the first join ran `Object.const_get`, Ruby code in the
  # middle of a native call. No Ruby runs while one is built now.
  def test_building_a_join_runs_no_const_get
    assert_equal '0 Join', isolated(<<~RUBY)
      hits = 0
      Object.define_singleton_method(:const_get) do |*args|
        hits += 1
        super(*args)
      end
      join = Join.new(bounds)
      puts [hits, join.class.name.split('::').last].join(' ')
    RUBY
  end

  # Bug: a redefined `Object.const_get` that raised hit an `unwrap` in the
  # extension: an uncatchable fatal, and the process exited.
  def test_a_raising_const_get_does_not_kill_the_process
    assert_equal '1', isolated(<<~RUBY)
      Object.define_singleton_method(:const_get) { |*| raise IOError, 'lookup ran in a native call' }
      join = Join.new(bounds)
      join.insert(:left, record)
      puts join.size(:left)
    RUBY
  end
end
