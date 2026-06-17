import json, urllib.request, collections
GOAL = "Click the Applications menu in the top panel"
N = 16
GRAMMAR = 'root ::= "act" | "skip"\n'

def mem_block(lines):
    return "" if lines == 0 else "Recent activity:\n" + ("- the user edited unrelated documents and spreadsheets.\n" * lines) + "\n"

# DEDICATED verify prompt — NOT the agent action-loop system prompt.
def vp(label, mem_lines):
    return ("You judge whether clicking one on-screen UI element accomplishes a task.\n\n"
            f"{mem_block(mem_lines)}"
            f"Task: {GOAL}\n"
            f'Element label: "{label}"\n\n'
            'Reply with exactly one word: "act" if clicking this element accomplishes the task, '
            'or "skip" if it does not.')

def ask(prompt):
    req = urllib.request.Request("http://127.0.0.1:8080/completion",
        data=json.dumps({"prompt": prompt, "grammar": GRAMMAR, "n_predict": 4, "temperature": 0.2}).encode(),
        headers={"Content-Type": "application/json"})
    out = json.loads(urllib.request.urlopen(req, timeout=60).read()).get("content", "").strip().lower()
    return "act" if out.startswith("act") else ("skip" if out.startswith("skip") else f"?{out[:6]}")

print(f"=== VERIFY-MODE v2: DEDICATED verify prompt × memory-above  (N={N}) ===")
print(f"    goal='{GOAL}'  want: Applications=act, Directory Menu=skip(attractor), Trash=skip\n")
CANDS = [("Trash", "clearly-wrong floor"),
         ("Directory Menu", "ATTRACTOR (= false-act-on-top-1 probe)"),
         ("Show Desktop", "neutral wrong"),
         ("Applications", "exact-correct (must act)")]
print(f"{'candidate':<16}{'tag':<40}{'mem=0':>9}{'mem=15':>9}{'mem=30':>9}")
for label, tag in CANDS:
    row = []
    for m in (0, 15, 30):
        c = collections.Counter(ask(vp(label, m)) for _ in range(N))
        row.append(f"{c.get('act',0)}/{N}")
    print(f"{label:<16}{tag:<40}{row[0]:>9}{row[1]:>9}{row[2]:>9}")

print(f"\n=== SEQUENCE SIM (dedicated prompt, memory=30): judge top-1, widen on skip ===")
def seq(order, m):
    for i, lab in enumerate(order):
        if ask(vp(lab, m)) == "act":
            return i, lab
    return None, None
ORDERS = {
    "GOOD ranker (Applications top-1)": ["Applications","Directory Menu","Show Desktop","Trash","Volume"],
    "BAD ranker (Directory Menu top-1, Applications last)": ["Directory Menu","Show Desktop","Trash","Volume","Applications"],
}
for name, order in ORDERS.items():
    res = collections.Counter(); fa = 0
    for _ in range(N):
        i, lab = seq(order, 30)
        res[lab if lab else "ESCAPE(all-skip)"] += 1
        if i == 0 and order[0] != "Applications": fa += 1
    print(f"{name}\n   pick: [{', '.join(f'{k}={v}' for k,v in res.most_common())}]   FALSE-ACT-ON-TOP-1: {fa}/{N}")
