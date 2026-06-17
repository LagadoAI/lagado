import json, urllib.request, collections
GOAL="Click the Applications menu in the top panel"; N=12
# LEAN, memory-free selector prompt — dedicated to selection, NOT the full agent SYS prompt, NO Board.
def lean_prompt(labels):
    rows="".join(f'  el_{i}  {chr(34)+l+chr(34) if l else "<no label>"}  [a11y]\n' for i,l in enumerate(labels))
    return ("You select the single on-screen element that accomplishes the current step, and act on it.\n\n"
            f"Step: {GOAL}\n\n"
            "On-screen elements:\n"+rows+"\n"
            'Reply with exactly one action: click(selector="el_N") for the element that accomplishes the step, '
            'or click(selector="none") if none fit.')
def grammar(n):
    t=" | ".join([f'"el_{i}"' for i in range(n)]+['"none"'])
    return "\n".join(["root ::= click | type | key | wait | done",'click ::= "click(selector=\\"" target "\\")"',
        'type ::= "type(selector=\\"" target "\\", text=\\"" freetext "\\")"','key ::= "key(key=\\"" freetext "\\")"',
        'wait ::= "wait(ms=" [0-9]+ ")"','done ::= "done(reason=\\"" freetext "\\")"',f"target ::= {t}",'freetext ::= [^"\\\\]*'])+"\n"
def ask(labels):
    g=grammar(len(labels)); p=lean_prompt(labels); c=collections.Counter()
    for _ in range(N):
        req=urllib.request.Request("http://127.0.0.1:8080/completion",
            data=json.dumps({"prompt":p,"grammar":g,"n_predict":40,"temperature":0.2}).encode(),
            headers={"Content-Type":"application/json"})
        try:
            o=json.loads(urllib.request.urlopen(req,timeout=60).read()).get("content","")
            tok=o.split('selector="')[1].split('"')[0] if 'selector="' in o else o.strip()[:10]; c[tok]+=1
        except Exception as e: c[f"ERR:{type(e).__name__}"]+=1
    return c
def idx(l,n): return f"el_{l.index(n)}"
def show(c): return ", ".join(f"{k}={v}" for k,v in c.most_common())
print(f"=== GATE: lean memory-free selector, POSITION SWEEP (N={N}) — does lean + late-band hold TOGETHER? ===\n")
SWEEP={
 "P1 target FIRST (el_0), decoy last":      ["Applications","","","2026-06-17","Show Desktop","Directory Menu"],
 "P2 target row5 (el_4), decoy last [=C1]": ["Show Desktop","Trash","Files","Volume","Applications","Directory Menu"],
 "P3 target row4 (el_3), decoy last [=C3]": ["Show Desktop","Trash","Files","Applications","Volume","Directory Menu"],
 "P4 target LAST (el_5), decoy el_0 [=I]":  ["Directory Menu","","","2026-06-17","Show Desktop","Applications"],
}
for name,labs in SWEEP.items():
    t=idx(labs,"Applications"); r=ask(labs); hit=r.get(t,0)
    print(f"{name}: target={t} -> {hit}/{N}   [{show(r)}]")
