#!/usr/bin/env ruby
# frozen_string_literal: true

require "json"
require "open3"

stdout, stderr, status = Open3.capture3("cargo", "metadata", "--format-version", "1")
unless status.success?
  warn stderr
  exit status.exitstatus || 1
end

packages = JSON.parse(stdout).fetch("packages")
wgpu = packages.select { |package| package.fetch("name") == "wgpu" }
identities = wgpu.map { |package| package.fetch("id") }.uniq

if identities.length != 1
  warn "expected exactly one wgpu package identity, found #{identities.length}:"
  warn identities.join("\n")
  exit 1
end

puts "single wgpu package identity: #{identities.first}"
