import json, urllib.request, collections
SYS = open("/home/alucard/projects/lagado/lagado-agent/prompts/system_prompt.txt").read()
GOAL = "Click the Applications menu in the top panel"
N = 12

def grammar(n):
    t = " | ".join([f'"el_{i}"' for i in range(n)] + ['"none"'])
    return "\n".join(["root ::= click | type | key | wait | done",
        'click ::= "click(selector=\\"" target "\\")"','type ::= "type(selector=\\"" target "\\", text=\\"" freetext "\\")"',
        'key ::= "key(key=\\"" freetext "\\")"','wait ::= "wait(ms=" [0-9]+ ")"','done ::= "done(reason=\\"" freetext "\\")"',
        f"target ::= {t}",'freetext ::= [^"\\\\]*'])+"\n"

def rows(labels):
    out='On-screen elements (choose one token, or "none" if none fit the goal):\n'
    for i,l in enumerate(labels): out+=f'  el_{i}  {chr(34)+l+chr(34) if l else "<no label>"}  [a11y]\n'
    return out

def mem(m):  # neutral, semantically-irrelevant filler; vary LENGTH only
    return "" if m==0 else "Archived (not relevant to the current task):\n"+("- [archived log entry]\n"*m)+"\n"

# Two trailer layouts. TRAILER bytes are IDENTICAL across memory lengths within each layout.
def prompt_L(labels,m):  # candidates, then goal, then action  (goal-last)
    return f"{SYS}\n\n{mem(m)}{rows(labels)}\nGoal: {GOAL}\n\nWhat is your next action?"
def prompt_T(labels,m):  # goal, then candidates, then action  (candidates-last = at the very end)
    return f"{SYS}\n\n{mem(m)}Goal: {GOAL}\n\n{rows(labels)}\nWhat is your next action?"

def ask(prompt,n):
    g=grammar(n); c=collections.Counter()
    for _ in range(N):
        req=urllib.request.Request("http://127.0.0.1:8080/completion",
            data=json.dumps({"prompt":prompt,"grammar":g,"n_predict":40,"temperature":0.2}).encode(),
            headers={"Content-Type":"application/json"})
        try:
            o=json.loads(urllib.request.urlopen(req,timeout=60).read()).get("content","")
            tok=o.split('selector="')[1].split('"')[0] if 'selector="' in o else o.strip()[:10]
            c[tok]+=1
        except Exception as e: c[f"ERR:{type(e).__name__}"]+=1
    return c

def idx(labels,name): return f"el_{labels.index(name)}"
def show(c): return ", ".join(f"{k}={v}" for k,v in c.most_common())

print(f"=== FIXED-TRAILER × MEMORY-ABOVE discrimination (N={N}) ===")
print("    stable target-hit across mem => layout fix (position-from-start). flips => global volume => model floor.\n")

ARR = {
 "C2 target LAST (el_5), decoy el_4": ["Show Desktop","Trash","Files","Volume","Directory Menu","Applications"],
 "C1 target el_4 (in-band, not last), decoy LAST el_5": ["Show Desktop","Trash","Files","Volume","Applications","Directory Menu"],
}
for aname,labs in ARR.items():
    tgt=idx(labs,"Applications")
    print(f"[{aname}]  target={tgt}")
    for lname,fn in (("L goal-last ",prompt_L),("T cands-last",prompt_T)):
        cells=[]
        for m in (0,15,30):
            c=fn(labs,m); r=ask(c,len(labs)); cells.append((m,r.get(tgt,0),r))
        line="  ".join(f"mem={m}:{h}/{N}" for m,h,_ in cells)
        print(f"   {lname}: {line}")
        for m,h,r in cells:
            print(f"        mem={m:<2} -> [{show(r)}]")
    print()
