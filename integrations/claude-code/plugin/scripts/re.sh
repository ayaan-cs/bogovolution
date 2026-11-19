#!/bin/sh
# Wrapper the slash commands call: finds the bogo binary and runs it.
#
#   re.sh <bogo arguments...>
#
# Free only: --provider must be mock or replay, and --allow-spend is refused.
# Anything else is refused here before the binary runs, on top of the
# binary's own checks.
. "$(cd "$(dirname "$0")" && pwd)/lib.sh"

prev=""
for arg in "$@"; do
  case "$prev" in
    --provider)
      case "$arg" in
        mock | replay) ;;
        *)
          echo "bogo plugin: refused. Only --provider mock or --provider replay are allowed from the plugin (got '$arg')." >&2
          exit 64
          ;;
      esac
      ;;
  esac
  case "$arg" in
    --allow-spend | --allow-spend=*)
      echo "bogo plugin: refused. --allow-spend (paid providers) is never allowed from the plugin." >&2
      exit 64
      ;;
    --provider=mock | --provider=replay) ;;
    --provider=*)
      echo "bogo plugin: refused. Only --provider mock or --provider replay are allowed from the plugin (got '${arg#--provider=}')." >&2
      exit 64
      ;;
  esac
  prev=$arg
done

if ! BIN=$(re_find_bin); then
  cat >&2 <<'MSG'
bogo plugin: the bogo binary was not found.
Build it in the bogo repo with `cargo build --release`, then do one of:
  - put target/release on PATH,
  - or set BOGO_BIN to the full path of the binary.
MSG
  exit 127
fi
exec "$BIN" "$@"
