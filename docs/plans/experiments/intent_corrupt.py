import json, urllib.request, collections
SYS=open("/home/alucard/projects/lagado/lagado-agent/prompts/system_prompt.txt").read()
N=12
GOAL="Open the Applications menu"                     # correct target = Applications
CANDS=["Applications","Files","Directory Menu","Show Desktop","Trash","Volume"]  # 'Files' = decoy (memory-primed)
def grammar():
    tgt=" | ".join(f'"{l}"' for l in CANDS)
    return 'root ::= "complete" | t\n'+'t ::= "target(\\"" target "\\")"\n'+f"target ::= {tgt}\n"
def screen(): return "Screen elements:\n"+"\n".join(f'  "{l}"' for l in CANDS)
def mem(kind):
    if kind=="neutral": return "Recent activity:\n"+("- you edited a spreadsheet yesterday.\n"*15)
    if kind=="decoy":   return "Recent activity:\n"+("- you frequently open Files to manage your documents.\n"*15)
    return ""
ASK='Emit the next target to act on toward the goal: target("<label>") or complete'
def prompt_HIGH(memkind):   # built structure: SYS / memory HIGH / screen / goal+ask LOW
    return f"{SYS}\n\n{mem(memkind)}\n\n{screen()}\n\nGoal: {GOAL}\n\n{ASK}"
def prompt_ADJ(memkind):    # bad structure: memory ADJACENT to the ask (near decision point)
    return f"{SYS}\n\n{screen()}\n\nGoal: {GOAL}\n\n{mem(memkind)}\n\n{ASK}"
def ask(promptfn, memkind):
    g=grammar(); p=promptfn(memkind); c=collections.Counter()
    for _ in range(N):
        req=urllib.request.Request("http://127.0.0.1:8080/completion",
            data=json.dumps({"prompt":p,"grammar":g,"n_predict":16,"temperature":0.2}).encode(),
            headers={"Content-Type":"application/json"})
        try:
            o=json.loads(urllib.request.urlopen(req,timeout=60).read()).get("content","").strip()
            if o.startswith("complete"): c["complete"]+=1
            elif 'target("' in o: c["→"+o.split('target("')[1].split('"')[0]]+=1
            else: c["?"+o[:8]]+=1
        except Exception as e: c[f"ERR:{type(e).__name__}"]+=1
    return c
def show(c): return ", ".join(f"{k}={v}" for k,v in c.most_common())
print(f"=== PLANNER INTENT-CORRUPTIBILITY (N={N}, goal='{GOAL}', correct=Applications, decoy=Files) ===")
print("    does decoy Board memory drag the emitted SUB-GOAL? does memory-HIGH resist better than memory-ADJACENT?\n")
print(f"  memory-HIGH  + neutral (control): [{show(ask(prompt_HIGH,'neutral'))}]")
print(f"  memory-HIGH  + DECOY  (built)   : [{show(ask(prompt_HIGH,'decoy'))}]")
print(f"  memory-ADJ   + DECOY  (bad)     : [{show(ask(prompt_ADJ,'decoy'))}]")
