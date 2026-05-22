#!/usr/bin/env bash
set -euo pipefail

ledger="docs/upstream-parity.md"
estimates="docs/package-progress-estimates.tsv"
portable_only=0

usage() {
  cat <<'USAGE'
Usage: scripts/package-progress-table.sh [--ledger PATH] [--estimates PATH] [--portable-only]

Emits a Markdown package-completion table from docs/upstream-parity.md.
For in-progress package rows, estimates come from docs/package-progress-estimates.tsv.
Verified and JavaScript-only rows are always 100%; not-started rows are always 0%.
USAGE
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --ledger)
      ledger="$2"
      shift 2
      ;;
    --estimates)
      estimates="$2"
      shift 2
      ;;
    --portable-only)
      portable_only=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [ ! -f "$ledger" ]; then
  echo "ledger not found: $ledger" >&2
  exit 1
fi

ruby - "$ledger" "$estimates" "$portable_only" <<'RUBY'
ledger_path, estimates_path, portable_only_arg = ARGV
portable_only = portable_only_arg == "1"

Estimate = Struct.new(:percent, :basis)

def abort_with(message)
  warn(message)
  exit(1)
end

def escape_markdown(value)
  value.to_s.gsub("|", "\\|").gsub(/\s+/, " ").strip
end

def short_text(value, limit = 120)
  text = escape_markdown(value)
  return text if text.length <= limit

  text[0, limit - 1].sub(/\s+\S*\z/, "") + "..."
end

estimates = {}
if File.exist?(estimates_path)
  File.readlines(estimates_path, chomp: true).each_with_index do |line, index|
    stripped = line.strip
    next if stripped.empty? || stripped.start_with?("#")

    package_dir, percent_text, basis = line.split("\t", 3)
    abort_with("#{estimates_path}:#{index + 1}: expected package, percent, and basis columns") unless basis

    begin
      percent = Integer(percent_text, 10)
    rescue ArgumentError
      abort_with("#{estimates_path}:#{index + 1}: invalid percentage #{percent_text.inspect}")
    end
    abort_with("#{estimates_path}:#{index + 1}: percentage must be 0..100") unless percent.between?(0, 100)

    estimates[package_dir] = Estimate.new(percent, basis)
  end
end

rows = []
in_package_inventory = false
File.readlines(ledger_path, chomp: true).each_with_index do |line, index|
  if line.start_with?("## Package And Provider Inventory")
    in_package_inventory = true
    next
  end

  if in_package_inventory && line.start_with?("## ")
    in_package_inventory = false
  end

  next unless in_package_inventory
  next unless line.start_with?("| `packages/")
  next if line.include?("| --- |")

  cells = line.split("|")[1..-2].map(&:strip)
  item, kind, status, rust_path, evidence, notes = cells
  package_dir = item[/`packages\/([^`]+)`/, 1]
  abort_with("#{ledger_path}:#{index + 1}: cannot parse package directory") unless package_dir

  display_name = item[/\((`[^`]+`)\)/, 1]&.delete("`") || package_dir
  status = status.delete("`")
  portable = status != "js-only-documented"
  estimate = estimates[package_dir]
  percent =
    case status
    when "verified", "js-only-documented"
      100
    when "not-started"
      0
    when "ported"
      estimate&.percent || 90
    when "in-progress"
      estimate&.percent || (rust_path == "none" ? 10 : 50)
    else
      abort_with("#{ledger_path}:#{index + 1}: unknown status #{status.inspect}")
    end

  basis =
    case status
    when "verified"
      "verified"
    when "js-only-documented"
      "intentionally JavaScript-only"
    when "not-started"
      "not started"
    else
      estimate&.basis || notes[/Remaining work:\s*(.+)\z/, 1] || "in progress"
    end

  rows << {
    package_dir: package_dir,
    display_name: display_name,
    kind: kind,
    status: status,
    portable: portable,
    percent: percent,
    basis: basis,
  }
end

abort_with("no package rows found in #{ledger_path}") if rows.empty?

in_progress_package_dirs = rows.select { |row| row[:status] == "in-progress" }.map { |row| row[:package_dir] }
missing_estimates = in_progress_package_dirs - estimates.keys
abort_with("missing package progress estimates for in-progress rows: #{missing_estimates.join(", ")}") unless missing_estimates.empty?

stale_estimates = estimates.keys - in_progress_package_dirs
abort_with("package progress estimates are stale for non-in-progress rows: #{stale_estimates.join(", ")}") unless stale_estimates.empty?

rows = rows.select { |row| row[:portable] } if portable_only

def average(values)
  return 0.0 if values.empty?
  values.sum.to_f / values.length
end

portable_rows = rows.select { |row| row[:status] != "js-only-documented" }
closed_rows = rows.count { |row| ["verified", "js-only-documented"].include?(row[:status]) }
portable_verified_rows = portable_rows.count { |row| row[:status] == "verified" }
in_progress_rows = rows.count { |row| row[:status] == "in-progress" }
not_started_rows = rows.count { |row| row[:status] == "not-started" }

puts "_Generated from `#{escape_markdown(ledger_path)}` and `#{escape_markdown(estimates_path)}`._"
puts
puts "| Metric | Value |"
puts "| --- | ---: |"
puts "| Displayed package rows | #{rows.length} |"
puts "| Average estimated completion | #{format('%.1f%%', average(rows.map { |row| row[:percent] }))} |"
puts "| Portable package average | #{format('%.1f%%', average(portable_rows.map { |row| row[:percent] }))} |"
puts "| Closed package rows | #{closed_rows} / #{rows.length} |"
puts "| Strict portable verified rows | #{portable_verified_rows} / #{portable_rows.length} |"
puts "| In-progress rows | #{in_progress_rows} |"
puts "| Not-started rows | #{not_started_rows} |"
puts
puts "| Package | Est. completion | Status | Kind | Basis / remaining work |"
puts "| --- | ---: | --- | --- | --- |"
rows.each do |row|
  puts "| `#{escape_markdown(row[:display_name])}` | #{row[:percent]}% | `#{escape_markdown(row[:status])}` | #{escape_markdown(row[:kind])} | #{short_text(row[:basis])} |"
end
RUBY
