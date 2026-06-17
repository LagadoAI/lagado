import json, urllib.request, collections
SYS=open("/home/alucard/projects/lagado/lagado-agent/prompts/system_prompt.txt").read()
GOAL="Click the Applications menu in the top panel"; N=12
def grammar(n):
    t=" | ".join([f'"el_{i}"' for i in range(n)]+['"none"'])
    return "\n".join(["root ::= click | type | key | wait | done",'click ::= "click(selector=\\"" target "\\")"',
        'type ::= "type(selector=\\"" target "\\", text=\\"" freetext "\\")"','key ::= "key(key=\\"" freetext "\\")"',
        'wait ::= "wait(ms=" [0-9]+ ")"','done ::= "done(reason=\\"" freetext "\\")"',f"target ::= {t}",'freetext ::= [^"\\\\]*'])+"\n"
def rows(labels):
    o='On-screen elements (choose one token, or "none" if none fit the goal):\n'
    for i,l in enumerate(labels): o+=f'  el_{i}  {chr(34)+l+chr(34) if l else "<no label>"}  [a11y]\n'
    return o
def prompt_T(labels,memtext): return f"{SYS}\n\n{memtext}Goal: {GOAL}\n\n{rows(labels)}\nWhat is your next action?"
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
LABS=["Show Desktop","Trash","Files","Volume","Directory Menu","Applications"]  # target el_5, decoy el_4
def show(c): return ", ".join(f"{k}={v}" for k,v in c.most_common())
MEMS={
 "neutral (control)":         "Archived (not relevant):\n"+("- [archived log entry]\n"*30)+"\n",
 "DECOY-priming (adversarial)":"Recent activity:\n"+("- you often use the Directory Menu in the panel to open folders.\n"*30)+"\n",
 "GOAL-priming (helpful?)":    "Recent activity:\n"+("- the Applications menu launches your programs.\n"*30)+"\n",
}
print(f"=== SEMANTIC memory interference, fixed trailer, target=el_5 (last), decoy=el_4 (N={N}) ===")
print("    does goal-RELATED prepended memory (the production case) corrupt selection?\n")
for name,mt in MEMS.items():
    r=ask(prompt_T(LABS,mt),len(LABS))
    print(f"{name:<30} target(el_5)={r.get('el_5',0)}/{N}  decoy(el_4)={r.get('el_4',0)}/{N}  -> [{show(r)}]")
