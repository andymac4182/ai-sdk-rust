#!/usr/bin/env bash
set -euo pipefail

# Enforce the repo convention from scripts/codex-goal/port-ai-sdk.md:
# avoid vague bucket names in source paths, modules, crate names, public APIs,
# and documented identifiers.
#
# Explicit upstream-mirroring exceptions:
# - provider_utils mirrors the upstream @ai-sdk/provider-utils package boundary.
# - util mirrors the existing upstream packages/ai utility surface already
#   exposed by this crate.
# - SharedV4ProviderReference is an upstream provider-v4 type name mentioned in
#   docs for the Rust ProviderReference wrapper.

failures=()

add_failure() {
  failures+=("$1")
}

is_banned_token() {
  case "$1" in
    helper | helpers | util | utils | common | misc | stuff | shared)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

allowed_path() {
  case "$1" in
    src/provider_utils.rs | src/util.rs)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

allowed_identifier_token() {
  local name="$1"
  local token="$2"

  if [[ "$token" == "utils" && "$name" == *provider_utils* ]]; then
    return 0
  fi

  if [[ "$token" == "util" && "$name" == "util" ]]; then
    return 0
  fi

  if [[ "$token" == "shared" && "$name" == SharedV4ProviderReference ]]; then
    return 0
  fi

  return 1
}

identifier_tokens() {
  local name="$1"

  perl -CS -e '
    my $name = shift;
    $name =~ s/([a-z0-9])([A-Z])/$1_$2/g;
    $name = lc $name;
    $name =~ s/[^a-z0-9]+/\n/g;
    print "$_\n" for grep { length } split /\n/, $name;
  ' "$name"
}

check_identifier() {
  local origin="$1"
  local name="$2"

  while IFS= read -r token; do
    if is_banned_token "$token" && ! allowed_identifier_token "$name" "$token"; then
      add_failure "$origin: '$name' uses vague token '$token'"
    fi
  done < <(identifier_tokens "$name")
}

check_path() {
  local path="$1"

  allowed_path "$path" && return 0

  local component
  IFS='/' read -ra components <<< "$path"
  for component in "${components[@]}"; do
    local stem="${component%.*}"
    check_identifier "$path path component" "$stem"
  done
}

while IFS= read -r path; do
  check_path "$path"
done < <(git ls-files)

while IFS= read -r cargo_manifest; do
  while IFS= read -r line; do
    value="${line#*=}"
    value="${value%\"}"
    value="${value#*\"}"
    check_identifier "$cargo_manifest crate name" "$value"
  done < <(grep -E '^[[:space:]]*name[[:space:]]*=' "$cargo_manifest")
done < <(git ls-files 'Cargo.toml' '*/Cargo.toml')

while IFS= read -r path; do
  case "$path" in
    scripts/codex-goal/* | scripts/run-gnhf-port.sh)
      continue
      ;;
  esac

  while IFS=: read -r line_number name; do
    [[ -z "${name:-}" ]] && continue
    check_identifier "$path:$line_number" "$name"
  done < <(
    perl -ne '
      if (/^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\b/) {
        print "$.:$1\n";
      }
      if (/^\s*pub(?:\([^)]*\))?\s+(?:async\s+)?(?:fn|struct|enum|trait|type|const|static|mod)\s+([A-Za-z_][A-Za-z0-9_]*)\b/) {
        print "$.:$1\n";
      }
      if (/^\s*pub\s+use\s+([A-Za-z_][A-Za-z0-9_]*)::/) {
        print "$.:$1\n";
      }
    ' "$path"
  )
done < <(git ls-files '*.rs')

while IFS= read -r path; do
  case "$path" in
    scripts/codex-goal/* | scripts/run-gnhf-port.sh)
      continue
      ;;
  esac

  while IFS=: read -r line_number name; do
    [[ -z "${name:-}" ]] && continue
    check_identifier "$path:$line_number documented identifier" "$name"
  done < <(
    perl -ne '
      while (/`([^`]+)`/g) {
        my $identifier = $1;
        next if $identifier =~ /@ai-sdk\/provider-utils/;
        next if $identifier =~ /provider[_-]utils/;
        next unless $identifier =~ /^[A-Za-z_][A-Za-z0-9_-]*$/;
        print "$.:$identifier\n";
      }
    ' "$path"
  )
done < <(git ls-files '*.md' '*.rs')

if (( ${#failures[@]} > 0 )); then
  printf 'Naming convention check failed:\n' >&2
  printf '  - %s\n' "${failures[@]}" >&2
  exit 1
fi

printf 'Naming convention check passed.\n'
