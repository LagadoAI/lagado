import json, urllib.request, collections
SYS=open("/home/alucard/projects/lagado/lagado-agent/prompts/system_prompt.txt").read()
GOAL="Click the Applications menu in the top panel"; N=12
def rows(labels): return "".join(f'  el_{i}  {chr(34)+l+chr(34) if l else "<no label>"}  [a11y]\n' for i,l in enumerate(labels))
def full_sys(labels):  # full agent SYS, no memory  (the C1/C3 config)
    return f"{SYS}\n\nOn-screen elements (choose one token, or \"none\" if none fit the goal):\n{rows(labels)}\nGoal: {GOAL}\n\nWhat is your next action?"
def grammar(n):
    t=" | ".join([f'"el_{i}"' for i in range(n)]+['"none"'])
    return "\n".join(["root ::= click | type | key | wait | done",'click ::= "click(selector=\\"" target "\\")"',
        'type ::= "type(selector=\\"" target "\\", text=\\"" freetext "\\")"','key ::= "key(key=\\"" freetext "\\")"',
        'wait ::= "wait(ms=" [0-9]+ ")"','done ::= "done(reason=\\"" freetext "\\")"',f"target ::= {t}",'freetext ::= [^"\\\\]*'])+"\n"
def ask(prompt,n):
    g=grammar(n); c=collections.Counter()
    for _ in range(N):
        req=urllib.request.Request("http://127.0.0.1:8080/completion",
            data=json.dumps({"prompt":prompt,"grammar":g,"n_predict":40,"temperature":0.2}).encode(),
            headers={"Content-Type":"application/json"})
        try:
            o=json.loads(urllib.request.urlopen(req,timeout=60).read()).get("content","")
            tok=o.split('selector="')[1].split('"')[0] if 'selector="' in o else o.strip()[:10]; c[tok]+=1
        except Exception as e: c[f"ERR:{type(e).__name__}"]+=1
    return c
def idx(l,n): return f"el_{l.index(n)}"
def show(c): return ", ".join(f"{k}={v}" for k,v in c.most_common())
print(f"=== HEAD-TO-HEAD: FULL-SYS (no mem) reproduce C1/C3/I? (N={N}) — vs the lean gate that just failed ===\n")
SWEEP={
 "P1 target FIRST (el_0)":      ["Applications","","","2026-06-17","Show Desktop","Directory Menu"],
 "P2 target row5 (el_4) [C1]":  ["Show Desktop","Trash","Files","Volume","Applications","Directory Menu"],
 "P3 target row4 (el_3) [C3]":  ["Show Desktop","Trash","Files","Applications","Volume","Directory Menu"],
 "P4 target LAST (el_5) [I]":   ["Directory Menu","","","2026-06-17","Show Desktop","Applications"],
}
for name,labs in SWEEP.items():
    t=idx(labs,"Applications"); r=ask(full_sys(labs),len(labs))
    print(f"{name}: target={t} -> {r.get(t,0)}/{N}   [{show(r)}]")
