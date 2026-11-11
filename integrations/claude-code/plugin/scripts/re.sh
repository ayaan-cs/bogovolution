#!/bin/sh
# Wrapper the slash commands call: finds the rules-evolve binary and runs it.
#
#   re.sh <rules-evolve arguments...>
#
# Free only: --provider must be mock or replay. Anything else is refused here
# before the binary runs, on top of the binary's own check.
. "$(cd "$(dirname "$0")" && pwd)/lib.sh"

prev=""
for arg in "$@"; do
  case "$prev" in
    --provider)
      case "$arg" in
        mock | replay) ;;
        *)
          echo "rules-evolve plugin: refused. Only --provider mock or --provider replay are allowed from the plugin (got '$arg')." >&2
          exit 64
          ;;
      esac
      ;;
  esac
  case "$arg" in
    --provider=mock | --provider=replay) ;;
    --provider=*)
      echo "rules-evolve plugin: refused. Only --provider mock or --provider replay are allowed from the plugin (got '${arg#--provider=}')." >&2
      exit 64
      ;;
  esac
  prev=$arg
done

if ! BIN=$(re_find_bin); then
  cat >&2 <<'MSG'
rules-evolve plugin: the rules-evolve binary was not found.
Build it in the rules-evolve repo with `cargo build --release`, then do one of:
  - put target/release on PATH,
  - or set RULES_EVOLVE_BIN to the full path of the binary.
MSG
  exit 127
fi
exec "$BIN" "$@"
