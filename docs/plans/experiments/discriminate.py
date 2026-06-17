import json, urllib.request, collections
SYS=open("/home/alucard/projects/lagado/lagado-agent/prompts/system_prompt.txt").read()
GOAL="Click the Applications menu in the top panel"; N=12

def grammar(n):
    targets=" | ".join([f'"el_{i}"' for i in range(n)]+['"none"'])
    return "\n".join(["root ::= click | type | key | wait | done",
        'click ::= "click(selector=\\"" target "\\")"','type ::= "type(selector=\\"" target "\\", text=\\"" freetext "\\")"',
        'key ::= "key(key=\\"" freetext "\\")"','wait ::= "wait(ms=" [0-9]+ ")"','done ::= "done(reason=\\"" freetext "\\")"',
        f"target ::= {targets}",'freetext ::= [^"\\\\]*'])+"\n"

def render(labels):
    out='On-screen elements (choose one token, or "none" if none fit the goal):\n'
    for i,lab in enumerate(labels): out+=f'  el_{i}  {chr(34)+lab+chr(34) if lab else "<no label>"}  [a11y]\n'
    return out

def ask(prompt, n):
    g=grammar(n); picks=collections.Counter()
    for _ in range(N):
        req=urllib.request.Request("http://127.0.0.1:8080/completion",
            data=json.dumps({"prompt":prompt,"grammar":g,"n_predict":40,"temperature":0.2}).encode(),
            headers={"Content-Type":"application/json"})
        try:
            out=json.loads(urllib.request.urlopen(req,timeout=60).read()).get("content","")
            tok=out.split('selector="')[1].split('"')[0] if 'selector="' in out else out.strip()[:12]
            picks[tok]+=1
        except Exception as e: picks[f"ERR:{type(e).__name__}"]+=1
    return picks

def std_prompt(labels):  # list ABOVE goal (production layout)
    return f"{SYS}\n\n{render(labels)}\nGoal: {GOAL}\n\nWhat is your next action?"

def idx(labels,name):  # token index of a label
    return f"el_{labels.index(name)}"

print(f"=== DISCRIMINATION: does label beat position WITHIN the late band? (N={N}) ===")
print("    target='Applications' (exact substring of goal), decoy='Directory Menu' (shares 'menu')\n")
FILL=["Show Desktop","Trash","Files","Volume"]
disc={
 "C1 target row5, decoy row6(last)  [unconfounded]": ["Show Desktop","Trash","Files","Volume","Applications","Directory Menu"],
 "C2 decoy row5,  target row6(last) [confounded ctrl]":["Show Desktop","Trash","Files","Volume","Directory Menu","Applications"],
 "C3 target row4, filler row5, decoy row6(last) [hard]":["Show Desktop","Trash","Files","Applications","Volume","Directory Menu"],
}
for name,labs in disc.items():
    p=ask(std_prompt(labs),len(labs)); t=idx(labs,"Applications"); d=idx(labs,"Directory Menu")
    win="TARGET" if p.get(t,0)>=N//2 else ("DECOY" if p.get(d,0)>=N//2 else "other")
    print(f"{name}")
    print(f"   target={t} decoy={d}  -> {win}  [{', '.join(f'{k}={v}' for k,v in p.most_common())}]\n")

print(f"=== LAYOUT PROBE: intrinsic 'lateness' vs proximity-to-decision-point? (N={N}) ===")
print("    failing config = Applications FIRST (el_0), decoy 'Directory Menu' last\n")
FAIL=["Applications","Show Desktop","Trash","Files","Volume","Directory Menu"]
n=len(FAIL); t=idx(FAIL,"Applications")
# baseline
pb=ask(std_prompt(FAIL),n)
print(f"LP-base   list-above-goal           target={t} -> [{', '.join(f'{k}={v}' for k,v in pb.most_common())}]")
# goal repeated immediately before action token
gr=f"{SYS}\n\n{render(FAIL)}\nGoal: {GOAL}\n\nReminder — your goal: {GOAL}\nWhat is your next action?"
pg=ask(gr,n)
print(f"LP-goalrep goal repeated pre-action target={t} -> [{', '.join(f'{k}={v}' for k,v in pg.most_common())}]")
# list BELOW goal (candidates closest to the decision point)
lb=f"{SYS}\n\nGoal: {GOAL}\n\n{render(FAIL)}\nWhat is your next action?"
pl=ask(lb,n)
print(f"LP-below   list-below-goal          target={t} -> [{', '.join(f'{k}={v}' for k,v in pl.most_common())}]")
# big memory block ABOVE the list (production: variable memory sits above the list)
mem="Past sessions:\n"+("- earlier the user worked on unrelated documents and spreadsheets.\n"*30)
pm=ask(f"{SYS}\n\n{mem}\n{render(['Show Desktop','Trash','Files','Volume','Directory Menu','Applications'])}\nGoal: {GOAL}\n\nWhat is your next action?",6)
print(f"LP-memabove target LAST + 30-line memory block above list -> [{', '.join(f'{k}={v}' for k,v in pm.most_common())}]  (target=el_5)")
print("   (target-last survives memory-above => safe zone is END-OF-LIST/stable; breaks => absolute-position/rots)")
