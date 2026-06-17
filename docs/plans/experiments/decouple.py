import json, urllib.request, collections
SYS = open("/home/alucard/projects/lagado/lagado-agent/prompts/system_prompt.txt").read()
GOAL = "Click the Applications menu in the top panel"
N = 12

def run(rows):  # rows = list of (token, label) in DISPLAY order
    toks = sorted(set(t for t, _ in rows), key=lambda s: int(s.split("_")[1]))
    targets = " | ".join([f'"{t}"' for t in toks] + ['"none"'])
    g = "\n".join([
        "root ::= click | type | key | wait | done",
        'click ::= "click(selector=\\"" target "\\")"',
        'type ::= "type(selector=\\"" target "\\", text=\\"" freetext "\\")"',
        'key ::= "key(key=\\"" freetext "\\")"',
        'wait ::= "wait(ms=" [0-9]+ ")"',
        'done ::= "done(reason=\\"" freetext "\\")"',
        f"target ::= {targets}",
        'freetext ::= [^"\\\\]*',
    ]) + "\n"
    body = 'On-screen elements (choose one token, or "none" if none fit the goal):\n'
    for t, lab in rows:
        body += f'  {t}  {chr(34)+lab+chr(34) if lab else "<no label>"}  [a11y]\n'
    prompt = f"{SYS}\n\n{body}\nGoal: {GOAL}\n\nWhat is your next action?"
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

# F: Applications in the FIRST ROW, but its token is el_1 (NOT the first grammar alternative).
#    The first grammar alternative el_0 is assigned to the LAST row (Directory Menu).
rows = [("el_1","Applications"), ("el_2",""), ("el_3",""),
        ("el_4","2026-06-17"), ("el_5","Show Desktop"), ("el_0","Directory Menu")]
p = run(rows)
print("F  Apps = FIRST ROW but token el_1 (first grammar alt el_0 = last row 'Directory Menu')")
print(f"   picks: {', '.join(f'{k}={v}' for k,v in p.most_common())}")
print(f"   Apps(el_1)={p.get('el_1',0)}/{N}   el_0/DirMenu={p.get('el_0',0)}/{N}")
print("   -> picks el_1: aversion is to the FIRST GRAMMAR ALTERNATIVE (sampler), not the row. trivial fix.")
print("   -> picks el_0 or avoids el_1: aversion is to the VISUAL FIRST ROW.")
