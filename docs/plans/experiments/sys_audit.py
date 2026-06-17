import json, urllib.request, collections
SYS=open("/home/alucard/projects/lagado/lagado-agent/prompts/system_prompt.txt").read()
N=12
def rows(labels): return "".join(f'  el_{i}  {chr(34)+l+chr(34) if l else "<no label>"}  [a11y]\n' for i,l in enumerate(labels))
def prompt(labels, goal):  # full SYS minus Board (the executor candidate config)
    return f"{SYS}\n\nOn-screen elements (choose one token, or \"none\" if none fit the goal):\n{rows(labels)}\nGoal: {goal}\n\nWhat is your next action?"
def grammar(n):
    t=" | ".join([f'"el_{i}"' for i in range(n)]+['"none"'])
    return "\n".join(["root ::= click | type | key | wait | done",'click ::= "click(selector=\\"" target "\\")"',
        'type ::= "type(selector=\\"" target "\\", text=\\"" freetext "\\")"','key ::= "key(key=\\"" freetext "\\")"',
        'wait ::= "wait(ms=" [0-9]+ ")"','done ::= "done(reason=\\"" freetext "\\")"',f"target ::= {t}",'freetext ::= [^"\\\\]*'])+"\n"
def ask(labels, goal):
    g=grammar(len(labels)); p=prompt(labels,goal); c=collections.Counter()
    for _ in range(N):
        req=urllib.request.Request("http://127.0.0.1:8080/completion",
            data=json.dumps({"prompt":p,"grammar":g,"n_predict":40,"temperature":0.2}).encode(),
            headers={"Content-Type":"application/json"})
        try:
            o=json.loads(urllib.request.urlopen(req,timeout=60).read()).get("content","")
            if 'selector="' in o: tok=o.split('selector="')[1].split('"')[0]
            else: tok=o.strip()[:8]
            c[tok]+=1
        except Exception as e: c[f"ERR:{type(e).__name__}"]+=1
    return c
# Fixed label SET, PERMUTED order, NO-MATCH goal. Track whether pick tracks POSITION or LABEL.
BASE=["Applications","Directory Menu","Show Desktop","Trash","Files","Volume"]
PERMS={
 "perm A (as-is)":        ["Applications","Directory Menu","Show Desktop","Trash","Files","Volume"],
 "perm B (reversed)":     ["Volume","Files","Trash","Show Desktop","Directory Menu","Applications"],
 "perm C (Apps middle)":  ["Show Desktop","Trash","Applications","Directory Menu","Files","Volume"],
 "perm D (Apps first)":   ["Applications","Show Desktop","Trash","Files","Volume","Directory Menu"],
}
for goal in ["Water the office plants", "Adjust the thermostat temperature"]:
    print(f"\n=== SYS-content audit | NO-MATCH goal='{goal}' (N={N}) ===")
    print("    pick tracks POSITION across perms => positional (content-neutral). tracks a LABEL => SYS content offset. 'none' => escape.\n")
    for name,labs in PERMS.items():
        r=ask(labs,goal)
        # annotate top pick with its label
        top=r.most_common(1)[0][0]
        lbl = ("<escape none>" if top=="none" else
               (labs[int(top.split("_")[1])] if top.startswith("el_") and top.split("_")[1].isdigit() and int(top.split("_")[1])<len(labs) else top))
        dist=", ".join(f"{k}({labs[int(k.split('_')[1])] if k.startswith('el_') and k.split('_')[1].isdigit() and int(k.split('_')[1])<len(labs) else '?'})={v}" if k.startswith('el_') else f"{k}={v}" for k,v in r.most_common())
        print(f"  {name:<22} top={top}->'{lbl}'   [{dist}]")
