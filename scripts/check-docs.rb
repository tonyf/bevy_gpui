#!/usr/bin/env ruby
# frozen_string_literal: true

require "pathname"

ROOT = Pathname.new(__dir__).parent.expand_path
FILES = [ROOT / "README.md", ROOT / "CONTRIBUTING.md"] +
        Pathname.glob(ROOT / "docs/*.md") +
        [ROOT / "vendor/gpui-ce/BEVY_GPUI_PATCH.md"]

def heading_slug(title)
  title
    .downcase
    .gsub(/[`*_~]/, "")
    .gsub(/[^\p{Alnum}\s-]/, "")
    .strip
    .gsub(/\s+/, "-")
end

headings = {}
FILES.each do |file|
  counts = Hash.new(0)
  headings[file] = file.readlines.map do |line|
    match = line.match(/^\#{1,6}\s+(.+?)\s*$/)
    next unless match

    base = heading_slug(match[1])
    suffix = counts[base]
    counts[base] += 1
    suffix.zero? ? base : "#{base}-#{suffix}"
  end.compact
end

errors = []
links = Hash.new { |hash, key| hash[key] = [] }

FILES.each do |file|
  file.read.scan(/\[[^\]]*\]\(([^)]+)\)/).flatten.each do |raw|
    link = raw.split(/\s+/, 2).first.to_s.sub(/^</, "").sub(/>$/, "")
    next if link.match?(/\A(?:https?:|mailto:)/)

    path, anchor = link.split("#", 2)
    target = path.nil? || path.empty? ? file : (file.dirname / path).cleanpath

    unless target.exist?
      errors << "#{file.relative_path_from(ROOT)}: missing target #{link}"
      next
    end

    links[file] << target if target.extname == ".md"
    if anchor && headings.key?(target) && !headings[target].include?(anchor)
      errors << "#{file.relative_path_from(ROOT)}: missing anchor #{link}"
    end
  end
end

reachable = { ROOT / "README.md" => 0 }
2.times do |depth|
  reachable.select { |_, value| value == depth }.keys.each do |file|
    links[file].each { |target| reachable[target] ||= depth + 1 }
  end
end

Pathname.glob(ROOT / "docs/*.md").each do |file|
  next if reachable.key?(file)

  errors << "#{file.relative_path_from(ROOT)}: not reachable within two links from README.md"
end

if errors.empty?
  puts "documentation links, anchors, and README discoverability are valid"
else
  warn errors.join("\n")
  exit 1
end
