import json, urllib.request, collections
SYS = open("/home/alucard/projects/lagado/lagado-agent/prompts/system_prompt.txt").read()
GOAL = "Click the Applications menu in the top panel"
N = 16

GRAMMAR = 'root ::= "act" | "skip"\n'

def mem_block(lines):
    if lines == 0:
        return ""
    return "Past sessions:\n" + ("- earlier the user worked on unrelated documents and spreadsheets.\n" * lines) + "\n"

def verify_prompt(label, mem_lines):
    return (f"{SYS}\n\n{mem_block(mem_lines)}"
            f"Goal: {GOAL}\n\n"
            f"Evaluate ONE on-screen element and decide if clicking it accomplishes the goal.\n"
            f'Element: "{label}"\n\n'
            f'Answer with one word — "act" to click it, or "skip" if it does not match the goal.')

def ask(prompt):
    req = urllib.request.Request("http://127.0.0.1:8080/completion",
        data=json.dumps({"prompt": prompt, "grammar": GRAMMAR, "n_predict": 4, "temperature": 0.2}).encode(),
        headers={"Content-Type": "application/json"})
    out = json.loads(urllib.request.urlopen(req, timeout=60).read()).get("content", "").strip().lower()
    return "act" if out.startswith("act") else ("skip" if out.startswith("skip") else f"?{out[:6]}")

def act_rate(label, mem_lines):
    c = collections.Counter(ask(verify_prompt(label, mem_lines)) for _ in range(N))
    return c.get("act", 0), c

# ── (2)+(1) ACQUIESCENCE GRADIENT under MEMORY-ABOVE (0/15/30 lines) ──
print(f"=== VERIFY-MODE acquiescence gradient × memory-above  (N={N}, goal='{GOAL}') ===")
print("    act-rate per candidate. want: Applications=high, Directory Menu=0 (ATTRACTOR), Trash=0\n")
CANDS = [("Trash", "clearly-wrong (floor)"),
         ("Directory Menu", "ATTRACTOR plausible-wrong — THE test / = false-act-on-top-1"),
         ("Applications", "exact-correct (must act)")]
print(f"{'candidate':<18}{'tag':<48}{'mem=0':>8}{'mem=15':>8}{'mem=30':>8}")
grad = {}
for label, tag in CANDS:
    row = []
    for m in (0, 15, 30):
        a, _ = act_rate(label, m)
        row.append(a); grad[(label, m)] = a
    print(f'{label:<18}{tag:<48}{row[0]:>6}/{N}{row[1]:>6}/{N}{row[2]:>6}/{N}')

# ── (3) FALSE-ACT-ON-TOP-1 + sequence sim (judge top-1, widen on skip) ──
print(f"\n=== SEQUENCE SIM: judge top-1, widen on skip (memory=30 lines) ===")
def run_sequence(order, mem_lines):
    # returns (acted_index, acted_label) for first 'act', else (None,None)=all-skip->escape
    for i, label in enumerate(order):
        if ask(verify_prompt(label, mem_lines)) == "act":
            return i, label
    return None, None

ORDERS = {
    "GOOD ranker (Applications top-1)":
        ["Applications", "Directory Menu", "Show Desktop", "Trash", "Volume"],
    "BAD ranker (attractor 'Directory Menu' top-1, Applications last)":
        ["Directory Menu", "Show Desktop", "Trash", "Volume", "Applications"],
}
for name, order in ORDERS.items():
    res = collections.Counter()
    falseact_top1 = 0
    for _ in range(N):
        idx, lab = run_sequence(order, 30)
        res[lab if lab else "ESCAPE(all-skip)"] += 1
        if idx == 0 and order[0] != "Applications":
            falseact_top1 += 1
    print(f"{name}")
    print(f"   final pick: [{', '.join(f'{k}={v}' for k,v in res.most_common())}]")
    print(f"   FALSE-ACT-ON-TOP-1 (terminal, committed-wrong): {falseact_top1}/{N}\n")
