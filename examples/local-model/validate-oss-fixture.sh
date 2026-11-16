#!/bin/sh
# Check every task make-oss-fixture.py knows: the hidden check must pass on the
# upstream code (--reference) and fail on the stub.
#
#   validate-oss-fixture.sh <more-itertools checkout at ceafee8ce96f4bc1eb74c528bc247c91d648a0bb>
#
# Prints one line per task and exits 1 if any task is not "ok".
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
SRC=${1:?usage: validate-oss-fixture.sh <more-itertools checkout>}
T=$(mktemp -d)
ALL=$(python3 -c "
import ast
t = ast.parse(open('$HERE/make-oss-fixture.py').read())
print(','.join(k for n in t.body if isinstance(n, ast.Assign)
      and getattr(n.targets[0], 'id', '') in ('FUNCS', 'MORE_FUNCS')
      for k in ast.literal_eval(n.value)))")
python3 "$HERE/make-oss-fixture.py" "$SRC" "$T/ref" "$ALL" --reference
python3 "$HERE/make-oss-fixture.py" "$SRC" "$T/stub" "$ALL"
bad=0
for f in $(echo "$ALL" | tr , ' '); do
  r=$(cd "$T/ref" && timeout 60 python3 -B -m unittest "checks.test_$f" >/dev/null 2>&1; echo $?)
  s=$(cd "$T/stub" && timeout 60 python3 -B -m unittest "checks.test_$f" >/dev/null 2>&1; echo $?)
  if [ "$r" = 0 ] && [ "$s" != 0 ]; then echo "ok   $f"; else echo "BAD  $f (reference exit $r, stub exit $s)"; bad=1; fi
done
rm -rf "$T"
exit $bad
