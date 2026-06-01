#!/usr/bin/env bash
set -euo pipefail

ledger="docs/upstream-parity.md"
estimates="docs/package-progress-estimates.tsv"
strict_inventory=""
portable_only=0
output=""
title="AI SDK Rust Package Progress"

usage() {
  cat <<'USAGE'
Usage: scripts/package-progress-table.sh [--ledger PATH] [--estimates PATH] [--strict-inventory PATH] [--portable-only] [--output PATH] [--title TITLE]

Emits a Markdown package-completion report from docs/upstream-parity.md.
For in-progress package rows, estimates come from docs/package-progress-estimates.tsv.
Verified and JavaScript-only rows are always 100%; not-started rows are always 0%.
When a strict test inventory is available, package rows with unmapped portable
upstream test cases are treated as in-progress regardless of their ledger status.
--title overrides the report's top-level heading (default: "AI SDK Rust Package Progress").
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
    --strict-inventory)
      strict_inventory="$2"
      shift 2
      ;;
    --portable-only)
      portable_only=1
      shift
      ;;
    --output)
      output="$2"
      shift 2
      ;;
    --title)
      title="$2"
      shift 2
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

if [ -z "$strict_inventory" ] && [ "$ledger" = "docs/upstream-parity.md" ] && [ -f "docs/ai-strict-test-inventory.md" ]; then
  strict_inventory="docs/ai-strict-test-inventory.md"
fi

ruby - "$ledger" "$estimates" "$strict_inventory" "$portable_only" "$output" "$title" <<'RUBY'
ledger_path, estimates_path, strict_inventory_path, portable_only_arg, output_path, title = ARGV
portable_only = portable_only_arg == "1"
title = "AI SDK Rust Package Progress" if title.nil? || title.empty?

Estimate = Struct.new(:percent, :basis)
StrictInventory = Struct.new(:cases, :portable_mapped, :portable_unmapped, :js_only, :type_system, :sample_ids)

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

def strip_code(value)
  value.to_s.strip.sub(/\A`/, "").sub(/`\z/, "")
end

def split_markdown_row(line)
  stripped = line.strip
  return nil unless stripped.start_with?("|") && stripped.end_with?("|")

  cells = []
  current = +""
  escaped = false
  stripped[1...-1].each_char do |char|
    if escaped
      current << char
      escaped = false
    elsif char == "\\"
      current << char
      escaped = true
    elsif char == "|"
      cells << current.strip.gsub('\|', "|").gsub('\\\\', "\\")
      current = +""
    else
      current << char
    end
  end
  cells << current.strip.gsub('\|', "|").gsub('\\\\', "\\")
  cells
end

def separator_row?(cells)
  cells && cells.all? { |cell| cell.match?(/\A:?-{3,}:?\z/) }
end

def status_label(status)
  case status
  when "verified"
    "Verified"
  when "js-only-documented"
    "JavaScript-only"
  when "in-progress"
    "In progress"
  when "not-started"
    "Not started"
  when "ported"
    "Ported"
  else
    status
  end
end

def package_table(rows, show_basis:)
  output = []
  if show_basis
    output << "| Package | Est. completion | Status | Kind | Basis / remaining work |"
    output << "| --- | ---: | --- | --- | --- |"
    rows.each do |row|
      output << "| `#{escape_markdown(row[:display_name])}` | #{row[:percent]}% | #{status_label(row[:status])} | #{escape_markdown(row[:kind])} | #{short_text(row[:basis])} |"
    end
  else
    output << "| Package | Completion | Status | Kind |"
    output << "| --- | ---: | --- | --- |"
    rows.each do |row|
      output << "| `#{escape_markdown(row[:display_name])}` | #{row[:percent]}% | #{status_label(row[:status])} | #{escape_markdown(row[:kind])} |"
    end
  end
  output
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

strict_rows = {}
strict_global = {}
if strict_inventory_path && !strict_inventory_path.empty?
  abort_with("strict inventory not found: #{strict_inventory_path}") unless File.exist?(strict_inventory_path)

  in_summary = false
  headers = nil
  File.readlines(strict_inventory_path, chomp: true).each do |line|
    cells = split_markdown_row(line)
    if cells && cells.length == 2 && !separator_row?(cells)
      strict_global[cells[0]] = strip_code(cells[1])
    end

    if line == "## Package Summary"
      in_summary = true
      headers = nil
      next
    end
    if in_summary && line.start_with?("## ")
      in_summary = false
    end
    next unless in_summary
    next unless line.start_with?("|")

    next if separator_row?(cells)
    if headers.nil?
      headers = cells
      next
    end
    next unless cells && cells.length == headers.length

    values = headers.zip(cells).to_h
    item = strip_code(values["Item"])
    next unless item.start_with?("packages/")

    package_dir = item.delete_prefix("packages/")
    strict_rows[package_dir] = StrictInventory.new(
      Integer(values["Cases"], 10),
      Integer(values["Portable mapped"], 10),
      Integer(values["Portable unmapped"], 10),
      Integer(values["JS-only"], 10),
      Integer(values["Type-system impossible"], 10),
      values["Sample failing IDs"],
    )
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
  ledger_status = status
  portable = status != "js-only-documented"
  estimate = estimates[package_dir]
  strict = strict_rows[package_dir]
  strict_forced = strict && portable && strict.portable_unmapped.positive?
  if strict_forced
    status = "in-progress" unless ledger_status == "not-started"
  end

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
  if strict_forced
    portable_total = strict.portable_mapped + strict.portable_unmapped
    percent = portable_total.zero? ? percent : (strict.portable_mapped * 100.0 / portable_total).floor
    percent = [percent, 99].min
  end

  basis =
    if strict_forced
      "strict test inventory: #{strict.portable_unmapped} portable upstream cases still need named Rust tests; sample failing IDs: #{strict.sample_ids}"
    else
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
    end

  rows << {
    package_dir: package_dir,
    display_name: display_name,
    kind: kind,
    status: status,
    ledger_status: ledger_status,
    portable: portable,
    percent: percent,
    basis: basis,
    strict_inventory: strict,
    strict_forced: strict_forced,
  }
end

abort_with("no package rows found in #{ledger_path}") if rows.empty?

in_progress_package_dirs = rows.select { |row| row[:status] == "in-progress" && !row[:strict_forced] }.map { |row| row[:package_dir] }
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
strict_display_rows = rows.select { |row| row[:strict_inventory] }
strict_portable_mapped = strict_display_rows.sum { |row| row[:strict_inventory].portable_mapped }
strict_portable_unmapped = strict_display_rows.sum { |row| row[:strict_inventory].portable_unmapped }
strict_portable_total = strict_portable_mapped + strict_portable_unmapped

closed = rows.select { |row| ["verified", "js-only-documented"].include?(row[:status]) }
in_progress = rows.select { |row| row[:status] == "in-progress" }
not_started = rows.select { |row| row[:status] == "not-started" }

document = []
document << "# #{title}"
document << ""
generated_from = "_Generated from `#{escape_markdown(ledger_path)}` and `#{escape_markdown(estimates_path)}`"
generated_from += " with strict test inventory `#{escape_markdown(strict_inventory_path)}`" if strict_inventory_path && !strict_inventory_path.empty?
document << "#{generated_from}._"
document << ""
document << "- Displayed package rows: #{rows.length}"
document << "- Average estimated completion: #{format('%.1f%%', average(rows.map { |row| row[:percent] }))}"
document << "- Portable package average: #{format('%.1f%%', average(portable_rows.map { |row| row[:percent] }))}"
document << "- Closed package rows: #{closed_rows} / #{rows.length}"
document << "- Strict portable verified rows: #{portable_verified_rows} / #{portable_rows.length}"
document << "- In-progress rows: #{in_progress_rows}"
document << "- Not-started rows: #{not_started_rows}"
if strict_portable_total.positive?
  if strict_global["Upstream cases scanned"]
    document << "- Strict inventory full upstream cases scanned: #{strict_global["Upstream cases scanned"]}"
  end
  if strict_global["Portable mapped denominator"]
    document << "- Strict inventory full portable cases mapped: #{strict_global["Portable mapped denominator"]}"
  end
  if strict_global["Portable cases still missing named Rust tests"]
    document << "- Strict inventory full portable cases unmapped: #{strict_global["Portable cases still missing named Rust tests"]}"
  end
  document << "- Displayed-row strict portable test cases mapped: #{strict_portable_mapped} / #{strict_portable_total}"
  document << "- Displayed-row strict portable test cases unmapped: #{strict_portable_unmapped}"
end
document << ""
document << "## 100% Closed"
document << ""
document.concat(package_table(closed, show_basis: false))
document << ""
document << "## In Progress"
document << ""
document.concat(package_table(in_progress, show_basis: true))
document << ""
document << "## Not Started"
document << ""
document.concat(package_table(not_started, show_basis: false))
document << ""

content = document.join("\n")
if output_path && !output_path.empty?
  File.write(output_path, content)
else
  puts content
end
RUBY
