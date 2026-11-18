"""Fix the replay of Claude run 1 after the resumed run (see NOTES).

The first session was stopped by an API read timeout during run 0 of child
282aecf2. The recorder kept the 8 answers of that discarded attempt, and when
the run was redone it skipped recording the new answers (same keys). The log
uses the new answers. This appends the 8 new answers, taken from the API
transcript, as entries with the same keys; with rules-evolve after this fix a
later entry for a key replaces the earlier one.
"""
import json, sys

def lines_of(s):
    parts = s.split('\n')
    if parts and parts[-1] == '':
        parts.pop()
    return [p[:-1] if p.endswith('\r') else p for p in parts]

def strip_markers(line):
    return line.strip().lstrip('#*`>- ').rstrip('*` ')

def parse_edits(reply):
    lines = lines_of(reply); edits = []; i = 0
    while i < len(lines):
        st = strip_markers(lines[i])
        path = st[5:].strip().strip('`*"\' ') if st.lower().startswith('file:') else None
        if not path:
            i += 1; continue
        j = i + 1
        while j < len(lines) and j <= i + 3 and not lines[j].lstrip().startswith('```'):
            if lines[j].strip():
                break
            j += 1
        if j < len(lines) and lines[j].lstrip().startswith('```'):
            k = j + 1; body = []
            while k < len(lines) and lines[k].strip() != '```':
                body.append(lines[k]); k += 1
            content = '\n'.join(body) + '\n'
            if not any(e['path'] == path for e in edits):
                edits.append({'content': content, 'path': path})
            i = k + 1
        else:
            i += 1
    return edits

tr = [json.loads(l) for l in open('transcript.jsonl')]
raw = open('claude-haiku-5-5-replay.jsonl').read().splitlines()
rp = [json.loads(l) for l in raw[1:]]
assert len(tr) == 147 and len(rp) == 139
# check the port against every call that was recorded once
pairs = [(rp[i], tr[i]) for i in range(56)] + [(rp[64 + j], tr[72 + j]) for j in range(75)]
bad = 0
for e, t in pairs:
    assert e['seed'] == t['seed']
    if e['kind'] == 'task':
        if [ (x['path'], x['content']) for x in e['response']['edits']] != [(x['path'], x['content']) for x in parse_edits(t['reply'])]:
            bad += 1
        assert e['response']['tokens_used'] == t['tokens'], (e['task_id'], e['response']['tokens_used'], t['tokens'])
print('task calls checked', sum(e['kind'] == 'task' for e, _ in pairs), 'mismatches', bad)
if bad or '--write' not in sys.argv:
    sys.exit(0 if not bad else 1)
out = []
for i in range(8):
    e = dict(rp[56 + i]); t = tr[64 + i]
    assert e['kind'] == 'task' and e['rules_hash'].startswith('282aecf2') and e['seed'] == t['seed']
    e['response'] = {'edits': parse_edits(t['reply']), 'tokens_used': t['tokens']}
    out.append(json.dumps(e, separators=(',', ':')))
with open('claude-haiku-5-5-replay.jsonl', 'a') as f:
    for l in out:
        f.write(l + '\n')
print('appended', len(out))
