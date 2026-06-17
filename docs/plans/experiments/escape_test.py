import json, urllib.request, collections
SYS=open("/home/alucard/projects/lagado/lagado-agent/prompts/system_prompt.txt").read()
N=12
def rows(labels): return "".join(f'  el_{i}  {chr(34)+l+chr(34) if l else "<no label>"}  [a11y]\n' for i,l in enumerate(labels))
def prompt(labels, goal):
    return f"{SYS}\n\nOn-screen elements (choose one token, or \"none\" if none fit the goal):\n{rows(labels)}\nGoal: {goal}\n\nWhat is your next action?"
# SELECTOR-ONLY grammar: click(el_N) or click(none). No done/wait/key bail.
def grammar(n):
    t=" | ".join([f'"el_{i}"' for i in range(n)]+['"none"'])
    return 'root ::= "click(selector=\\"" target "\\")"\n'+f"target ::= {t}\n"
def ask(labels, goal):
    g=grammar(len(labels)); p=prompt(labels,goal); c=collections.Counter()
    for _ in range(N):
        req=urllib.request.Request("http://127.0.0.1:8080/completion",
            data=json.dumps({"prompt":p,"grammar":g,"n_predict":12,"temperature":0.2}).encode(),
            headers={"Content-Type":"application/json"})
        try:
            o=json.loads(urllib.request.urlopen(req,timeout=60).read()).get("content","")
            tok=o.split('selector="')[1].split('"')[0] if 'selector="' in o else o.strip()[:8]; c[tok]+=1
        except Exception as e: c[f"ERR:{type(e).__name__}"]+=1
    return c
def annot(r,labs):
    return ", ".join((f"{k}('{labs[int(k.split('_')[1])]}')={v}" if k.startswith('el_') and int(k.split('_')[1])<len(labs) else f"{k}={v}") for k,v in r.most_common())
PERMS={
 "perm A": ["Applications","Directory Menu","Show Desktop","Trash","Files","Volume"],
 "perm B (rev)": ["Volume","Files","Trash","Show Desktop","Directory Menu","Applications"],
 "perm C (Apps mid)": ["Show Desktop","Trash","Applications","Directory Menu","Files","Volume"],
}
print(f"=== SELECTOR-ONLY grammar (click el_N | click none), NO-MATCH goal — does 'none' fire cleanly? (N={N}) ===\n")
for goal in ["Water the office plants","Adjust the thermostat temperature"]:
    print(f"goal='{goal}'")
    for name,labs in PERMS.items():
        r=ask(labs,goal); none=r.get("none",0)
        print(f"  {name:<18} none={none}/{N}   [{annot(r,labs)}]")
    print()
print("=== control: selector-only grammar, MATCHING goal 'Click the Applications menu', target in late band ===")
labs=["Show Desktop","Trash","Files","Volume","Applications","Directory Menu"]
r=ask(labs,"Click the Applications menu in the top panel")
print(f"  target=el_4(Applications) -> {r.get('el_4',0)}/{N}   [{annot(r,labs)}]")
