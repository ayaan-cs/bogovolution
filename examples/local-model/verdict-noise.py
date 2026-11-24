"""How often the verdict rule accepts, simulated from a root's measured runs.

  verdict-noise.py <project>/.graft/log.jsonl <root hash prefix> <held out ids, comma separated>

Each task passes with the share of root runs it passed. A simulated child is
either the root again, or the root with K train (and H held out) tasks that the
root never solved turned into tasks it always solves. The rule is applied to
20,000 simulated pairs per case. Tasks are treated as independent."""
import json, random, statistics, sys
log = sys.argv[1]
runs = {}
for l in open(log):
    e = json.loads(l)
    if e["kind"] == "run_recorded" and e["payload"]["node"].startswith(sys.argv[2]):
        p = e["payload"]; runs[p["run_index"]] = p
tasks = {}
for r in runs.values():
    for t in r["tasks"]:
        tasks.setdefault(t["id"], []).append(t["status"] == "passed")
ho = set(sys.argv[3].split(","))
tr = [k for k in tasks if k not in ho]; he = [k for k in tasks if k in ho]
p = {k: sum(v) / len(v) for k, v in tasks.items()}
def rate(ids, pr): return 100 * sum(random.random() < pr[k] for k in ids) / len(ids)
def sim(n, thr, wins, hthr, hwins, lift=0, hlift=0, trials=20000):
    acc = 0
    pc = dict(p)
    zero = [k for k in tr if p[k] == 0][:lift]
    for k in zero: pc[k] = 1.0
    for k in [k for k in he if p[k] == 0][:hlift]: pc[k] = 1.0
    for _ in range(trials):
        a = [rate(tr, p) for _ in range(n)]; b = [rate(tr, pc) for _ in range(n)]
        ha = [rate(he, p) for _ in range(n)]; hb = [rate(he, pc) for _ in range(n)]
        g = statistics.median(b) - statistics.median(a)
        w = sum(y > x for x, y in zip(a, b))
        if g >= thr and w >= wins:
            hg = statistics.median(hb) - statistics.median(ha)
            hw = sum(y > x for x, y in zip(ha, hb))
            if hg >= hthr and hw >= hwins: acc += 1
    return 100 * acc / trials
print("root per run train %", [round(100*sum(t['status']=='passed' for t in r['tasks'] if t['id'] in tr)/len(tr),1) for r in runs.values()])
print("root per run held %", [round(100*sum(t['status']=='passed' for t in r['tasks'] if t['id'] in he)/len(he),1) for r in runs.values()])
print("tasks never passed: train", sum(p[k]==0 for k in tr), "held", sum(p[k]==0 for k in he), "always passed: train", sum(p[k]==1 for k in tr), "held", sum(p[k]==1 for k in he))
random.seed(1)
for name, args in [("default 5 runs, 10pp, 4/5, held 10pp 3/5", (5,10,4,10,3)),
                   ("2 runs, 10pp, 2/2, held 10pp 2/2", (2,10,2,10,2)),
                   ("proposal 5 runs, 10pp, 4/5, held 5pp 3/5", (5,10,4,5,3))]:
    print(name, "| false accept % (child = root):", sim(*args, lift=0),
          "| overfit, fixes 3 train tasks only:", sim(*args, lift=3),
          "| general, fixes 3 train and 3 held out:", sim(*args, lift=3, hlift=3),
          "| general, fixes 3 train and 2 held out:", sim(*args, lift=3, hlift=2))
