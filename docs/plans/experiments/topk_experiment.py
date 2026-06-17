import json, urllib.request, collections

SYS = open("/home/alucard/projects/lagado/lagado-agent/prompts/system_prompt.txt").read()
GOAL = "Click the Applications menu in the top panel"
N = 12  # trials per condition

def grammar(n):
    targets = " | ".join([f'"el_{i}"' for i in range(n)] + ['"none"'])
    return "\n".join([
        "root ::= click | type | key | wait | done",
        'click ::= "click(selector=\\"" target "\\")"',
        'type ::= "type(selector=\\"" target "\\", text=\\"" freetext "\\")"',
        'key ::= "key(key=\\"" freetext "\\")"',
        'wait ::= "wait(ms=" [0-9]+ ")"',
        'done ::= "done(reason=\\"" freetext "\\")"',
        f"target ::= {targets}",
        'freetext ::= [^"\\\\]*',
    ]) + "\n"

def render(labels):
    out = 'On-screen elements (choose one token, or "none" if none fit the goal):\n'
    for i, lab in enumerate(labels):
        shown = f'"{lab}"' if lab else "<no label>"
        out += f"  el_{i}  {shown}  [a11y]\n"
    return out

def run(labels):
    g = grammar(len(labels))
    prompt = f"{SYS}\n\n{render(labels)}\nGoal: {GOAL}\n\nWhat is your next action?"
    picks = collections.Counter()
    for _ in range(N):
        req = urllib.request.Request(
            "http://127.0.0.1:8080/completion",
            data=json.dumps({"prompt": prompt, "grammar": g, "n_predict": 40, "temperature": 0.2}).encode(),
            headers={"Content-Type": "application/json"},
        )
        try:
            out = json.loads(urllib.request.urlopen(req, timeout=60).read()).get("content", "")
            tok = out.split('selector="')[1].split('"')[0] if 'selector="' in out else out.strip()[:12]
            picks[tok] += 1
        except Exception as e:
            picks[f"ERR:{type(e).__name__}"] += 1
    return picks

# Apps index per condition -> "correct" token
CONDITIONS = [
    ("A FULL6  Apps@el_0 (LIVE REPRO, spatial order)",
     ["Applications", "", "", "2026-06-17", "Show Desktop", "Directory Menu"], "el_0"),
    ("B FULL6  Apps@el_4 (same 6 items, Apps moved to middle)",
     ["", "", "2026-06-17", "Show Desktop", "Applications", "Directory Menu"], "el_4"),
    ("C TOP3   Apps@el_0 (short+clean, Apps first)",
     ["Applications", "Show Desktop", "Directory Menu"], "el_0"),
    ("D TOP3   Apps@el_2 (short+clean, Apps LAST)",
     ["Show Desktop", "Directory Menu", "Applications"], "el_2"),
]

print(f"=== top-k / position experiment (N={N} per condition, temp=0.2, goal='{GOAL}') ===\n")
for name, labels, correct in CONDITIONS:
    picks = run(labels)
    hit = picks.get(correct, 0)
    dist = ", ".join(f"{k}={v}" for k, v in picks.most_common())
    print(f"{name}")
    print(f"   correct={correct}  ->  {hit}/{N} correct   [{dist}]\n")

# ── Disambiguator: is the dead slot the TOKEN "el_0" or the FIRST ROW? ──
def grammar_off(n, off):
    targets = " | ".join([f'"el_{i+off}"' for i in range(n)] + ['"none"'])
    return "\n".join([
        "root ::= click | type | key | wait | done",
        'click ::= "click(selector=\\"" target "\\")"',
        'type ::= "type(selector=\\"" target "\\", text=\\"" freetext "\\")"',
        'key ::= "key(key=\\"" freetext "\\")"',
        'wait ::= "wait(ms=" [0-9]+ ")"',
        'done ::= "done(reason=\\"" freetext "\\")"',
        f"target ::= {targets}",
        'freetext ::= [^"\\\\]*',
    ]) + "\n"
def render_off(labels, off):
    out = 'On-screen elements (choose one token, or "none" if none fit the goal):\n'
    for i, lab in enumerate(labels):
        shown = f'"{lab}"' if lab else "<no label>"
        out += f"  el_{i+off}  {shown}  [a11y]\n"
    return out
def run_off(labels, off):
    g = grammar_off(len(labels), off)
    prompt = f"{SYS}\n\n{render_off(labels, off)}\nGoal: {GOAL}\n\nWhat is your next action?"
    picks = collections.Counter()
    for _ in range(N):
        req = urllib.request.Request("http://127.0.0.1:8080/completion",
            data=json.dumps({"prompt": prompt, "grammar": g, "n_predict": 40, "temperature": 0.2}).encode(),
            headers={"Content-Type": "application/json"})
        try:
            out = json.loads(urllib.request.urlopen(req, timeout=60).read()).get("content","")
            tok = out.split('selector="')[1].split('"')[0] if 'selector="' in out else out.strip()[:12]
            picks[tok]+=1
        except Exception as e: picks[f"ERR:{type(e).__name__}"]+=1
    return picks

print("=== disambiguator: Applications in the FIRST ROW, 1-indexed (token el_1) ===")
labs = ["Applications", "", "", "2026-06-17", "Show Desktop", "Directory Menu"]
p = run_off(labs, 1)
print(f"E FULL6 1-indexed, Apps first row = el_1  ->  el_1: {p.get('el_1',0)}/{N}   [{', '.join(f'{k}={v}' for k,v in p.most_common())}]")
print("   (picks el_1 => aversion was to the TOKEN el_0, fix=1-index. avoids el_1 => aversion is to the FIRST ROW.)")
