# LAPUTA — COMPLETE UI BUILDING GUIDE
## For a non-coder, visual-first. Build it tonight, see everything before it touches your real files.

**Your stack (already set up):** React + TypeScript + Vite + Tailwind. You have ~60 .tsx files already.
**Tools:** ShadCN (foundation), v0.dev (custom pages), Claude artifacts (visual iteration), your local Vite dev server (the real preview).
**Goal:** A complete, good-looking, fully-owned UI shell — disconnected from the backend, every page visible and testable, ready to wire up later.

---

## THE CORE IDEA (read this first)

You are not "coding" tonight. You are **assembling and previewing**. Three places you'll see your work:

1. **v0.dev / Claude artifacts** — a sandbox to *design and see* a page before it's real. Throwaway preview.
2. **Your local Vite server (`npm run dev`)** — the REAL preview. This shows your actual files, live, in a browser, updating as you save. This is your truth.
3. **The files themselves** — where the code lives. You paste finished code here.

The whole workflow is: **design in a sandbox → see it → copy real code → paste into your file → see it again live in Vite → commit.** You never guess. You always see it before and after.

---

## STEP 0 — SAFETY NET (do this once, takes 5 minutes)

Because you're a non-coder editing real files, you need an undo button bigger than Ctrl+Z.

1. Open a terminal in your project folder (`/home/d/laputa/laputa-ui/`).
2. Run these once:
   ```
   git add .
   git commit -m "ui: snapshot before tonight's build"
   ```
3. **After every page you finish, do this again** with a new message (e.g. `git commit -m "ui: chat page done"`).

Why: if anything breaks at 2am, you type `git reset --hard HEAD` and you're back to your last good commit. You lose one page, not the night. This is your seatbelt. Use it religiously.

---

## STEP 1 — START THE LIVE PREVIEW (your truth window)

1. In the terminal, in `/home/d/laputa/laputa-ui/`, run:
   ```
   npm run dev
   ```
2. It will print a local address, usually `http://localhost:5173`.
3. Open that in your browser. Leave it open all night on a second monitor or half your screen.

**This is your real preview.** Every time you save a file, this updates instantly. When you paste new code and the page looks right *here*, it's actually right. The sandboxes (v0, artifacts) are just for designing — Vite is for confirming.

If it shows errors (a red screen or blank page), that's normal while building — it means a file has a problem. We'll handle errors in Step 7.

---

## STEP 2 — INSTALL SHADCN (the foundation)

ShadCN gives you real, ownable component files. Not a black box — the actual code lands in your project and you own it.

1. **Check your path alias.** Open `tsconfig.json`. Look for something like:
   ```json
   "paths": { "@/*": ["./src/*"] }
   ```
   If it's there, good. If not, add it inside `compilerOptions`. (Also check `vite.config.ts` has a matching `resolve.alias` for `@`. If you're unsure, paste both files to Claude and ask "does this have the @ alias set up for ShadCN?")

2. **Initialize ShadCN.** In the terminal:
   ```
   npx shadcn@latest init
   ```
   It asks a few questions (style, base color). Pick a base color you like — this becomes your theme. You can change it later.

3. This creates a `globals.css` (or updates your CSS) with theme variables, and a `components/ui/` folder. That's where owned components will live.

**Security rule from here on:** ONLY use `npx shadcn@latest add [name]` from the official command. Never add from a random URL someone posts. Read each file after it lands (they're short). This matters because Laputa is a security product — you can't pull blind code into it.

---

## STEP 3 — LOCK YOUR THEME (do this ONCE, everything inherits it)

This is the secret to a consistent look without fussing over every page.

**Option A — visual (recommended for non-coders):**
1. Go to **tweakcn.com** (a free visual theme editor for ShadCN).
2. Play with colors, fonts, radius, spacing until it feels like Laputa — think sovereign, dark, fortress, calm-but-powerful.
3. It generates CSS variables. Copy them.
4. Paste them into your `globals.css`, replacing the default `:root` variables.
5. Save. Watch your Vite preview shift to the new look.

**Option B — by hand:** open `globals.css`, edit the color variables directly. Slower, more control.

Do this NOW, before building pages. Every component you add will automatically use these colors. Lock it once, and your whole app is consistent for free.

---

## STEP 4 — PULL ALL YOUR PRIMITIVES (one sitting)

Primitives are the small building blocks (buttons, inputs, cards). Get them all at once so you never stop to fetch one mid-page.

In the terminal, run this (it pulls the common set Laputa needs):
```
npx shadcn@latest add button input card dialog tabs badge select checkbox radio-group slider alert progress separator scroll-area tooltip dropdown-menu switch textarea
```

Each lands in `src/components/ui/`. **Open a few and read them** — they're ~30-80 lines each, mostly Tailwind classes. You'll see there's no mystery. Commit when done:
```
git add . && git commit -m "ui: shadcn primitives"
```

Now you have a complete kit. Every page is built by *composing* these.

---

## STEP 5 — THE MOCK-DATA RULE (the most important habit tonight)

Since the backend isn't connected, every page needs **fake data** to display. Do NOT build empty shells — build pages that render fake data that *looks* real.

At the top of each page file, hardcode a fake data object:
```tsx
const MOCK_MESSAGES = [
  { id: 1, role: "user", text: "open my email", brain: null },
  { id: 2, role: "agent", text: "I'll do that now.", brain: "Liquid" },
  { id: 3, role: "agent", text: "Found 3 unread. Want me to summarize?", brain: "Reasoning" },
];
```
Then the page displays `MOCK_MESSAGES`. You'll see a real-looking chat tonight.

**Why this is the key trick:** when you wire the backend later, you delete the mock object and point the page at real WebSocket data. **The visual part of the page never changes.** Backend day becomes "swap the data source," not "rebuild the screen." This single habit saves you the most pain.

Do the same for every page: fake files for Vault, fake VM list for VMManager, fake terminal output for Terminal, fake settings values for Settings.

---

## STEP 6 — THE PAGE-BUILDING LOOP (repeat for each page)

This is the core loop you'll run ~12 times tonight. Each page, same steps:

**6a. Decide what the page shows.** Look at your existing version of the page in your files and in the Vite preview. What's on it? What data does it display? Write your MOCK_DATA object for it.

**6b. Design it in a sandbox.**
- For standard pages (Settings, Vault, Chat, Terminal, MCP, Server, VM): use **v0.dev**. Tell it:
  > "React + TypeScript + Tailwind, using ShadCN components. No localStorage. Build a [page name] that renders from this data shape: [paste your MOCK_DATA]. It should have [describe: a sidebar, a list, tabs, etc.]."
  v0 outputs ShadCN-based code, so it drops into your project cleanly.
- For custom/special pages (Immersive live canvas, the Awakening sequence): use **Claude artifacts** here in the app — describe it, see it render live, iterate until it's right.

**6c. SEE it in the sandbox.** v0 and artifacts both show a live preview. Tweak the prompt until it looks right. This is where you test *before* touching your files.

**6d. Copy the real code.** Both tools give you actual code. Copy it.

**6e. Paste into your real file.** Open the page file in your editor. Replace its contents (or the relevant part) with the new code. Fix the imports at the top so they point to your real paths:
- ShadCN components: `import { Button } from "@/components/ui/button"`
- Make sure any icon library it used is installed (`lucide-react` is the ShadCN default — likely already there).

**6f. SEE it live in Vite.** Save the file. Look at your `localhost:5173` preview. Navigate to that page. Does it render? Does it look like the sandbox? If yes — done.

**6g. Commit.** `git add . && git commit -m "ui: [page name]"`

That's one page. Repeat.

---

## THE BUILD ORDER (dependency-safe — nothing breaks what came before)

Build in this order so every page only depends on things already finished:

1. **Theme** (Step 3) — done first, everything inherits it.
2. **Primitives** (Step 4) — done in one sitting.
3. **Fake `useAgentSocket` hook** — make your WebSocket hook return mock data and just `console.log` when something is "sent." Every page imports the real hook name from day one; later you swap its *insides*, not every page. (Ask Claude: "write me a fake useAgentSocket hook that returns mock data and logs sends, matching this interface: [paste your current hook's function signatures].")
4. **Layout shell** — Header, Layout, sidebar/nav. The frame every page sits in.
5. **Immersive page** — hardest one (live canvas, agent-running states, the gate). Build it FIRST while you're fresh, around 9pm, not 3am.
6. **Chat page** — message list, input, brain-indicator badges, inline confirmation.
7. **Settings** — tabs/sub-pages, toggles, model roster, the Chronos timeline view.
8. **Vault** — file list, preview, encrypted-status indicators.
9. **Terminal** — output area, tabs, input line.
10. **Code page** — editor area + run output (Monaco can be a styled placeholder box tonight).
11. **MCP / Server / VM managers** — lists with toggle/start/stop controls.
12. **Login** — auth form.
13. **Awakening** — the first-launch sequence (use artifacts; this is custom and emotional — the five truths, "Today, I begin."). Animations can be rough tonight; you'll perfect them later.
14. **Dialogs last** — ConfirmationDialog, PermissionBrowser, SessionRestore, etc. They sit on top of finished pages.
15. **Final polish pass** — go through all pages once, fix spacing/consistency. Because your theme is locked, this is quick.

---

## STEP 7 — WHEN SOMETHING BREAKS (it will, that's normal)

You're not a coder, so here's how to handle the inevitable red screen without panic:

1. **Read the error in the browser or terminal.** It usually names a file and a line. Often it's a typo or a missing import.
2. **Most common fix:** an import path is wrong. The error says something like `Cannot find module '@/components/ui/button'`. Check the path matches where the file actually is.
3. **If you can't fix it in 2 minutes:** copy the WHOLE error message + the file it's complaining about, and save it to paste to Claude later (you said you won't ask until you're done — so keep a "problems.txt" file and dump errors there as you go).
4. **If a page totally breaks and you're stuck:** `git reset --hard HEAD` reverts to your last commit. You lose only that one unfinished page. This is why you commit after every page.
5. **Keep going.** A broken page doesn't stop the others. Comment out its route if needed and move on; fix it in the audit pass with Claude.

---

## STEP 8 — EDITING WHAT YOU ALREADY HAVE (your real question)

You already built ~60 .tsx files. You don't throw them away — you upgrade them in place. Two approaches:

**Approach A — Side-by-side replace (recommended):**
1. Open your existing page in the Vite preview. See what it currently does.
2. Note what data it shows and what's on it.
3. Rebuild that page (better-looking, ShadCN-based) in v0/artifacts using the SAME data shape.
4. Replace the file's contents with the new version.
5. Compare old vs new in Vite. Keep the better one (git lets you go back if the old was better).

**Approach B — Improve a piece at a time:**
If a page is mostly fine but ugly, don't rebuild it — just swap its raw HTML elements for ShadCN ones:
- Find `<button>` → replace with ShadCN `<Button>`
- Find a plain `<input>` → replace with ShadCN `<Input>`
- Wrap sections in ShadCN `<Card>`
Paste the file to v0 or Claude with: "upgrade this to use ShadCN components, keep the same behavior and data, just make it look polished." Then preview in Vite.

**For a non-coder, Approach A is safer** — you get a clean, known-good page rather than half-editing something you don't fully understand. Let the tool give you the whole file, you paste it, you verify it visually.

---

## SECURITY DISCIPLINE (because this is Laputa)

You're building the face of a sovereignty product. Hold these rules:

1. **Official ShadCN registry only.** Never `add` from a random URL.
2. **Read each component file** before committing it. They're short. You'll understand your own UI better, too.
3. **Skip the flashy ecosystem add-ons** (Magic UI 3D globe, random "blocks" registries) unless you read every line. A sovereign agent doesn't need an animated globe.
4. **Pin versions; commit your lockfile.** Run `npm audit` at the end of the night and save anything it flags for the Claude audit.
5. **No `postinstall` surprises.** If a package wants to run scripts on install, that's a flag — note it.

When you finish, you'll paste your code (lockfile + package.json first, then any third-party/v0 code) and Claude will run a security audit on it.

---

## WHAT "DONE TONIGHT" LOOKS LIKE

- Theme locked; every page consistent.
- All primitives owned in `components/ui/`.
- A fake `useAgentSocket` so pages have data without a backend.
- Every page renders in Vite with real-looking mock data.
- You can click through the whole app and it *looks* alive — fake chat, fake vault, fake VM list, the Awakening sequence.
- Committed page-by-page, so nothing is lost.
- A `problems.txt` with any errors you couldn't fix, ready for the audit.

When the backend comes, you delete the mock objects, point pages at the real hook, and watch it come alive. The visual work you do tonight does not get redone — it just gets connected.

---

## QUICK REFERENCE — THE LOOP, IN ONE LINE

**Design in v0/artifacts → see it → copy real code → paste into file → fix imports → see it live in Vite → commit → next page.**

Theme once. Mock data always. Read before commit. Commit after every page. `git reset --hard HEAD` is your undo. You see everything before it's real, and again after.

Go build the fortress's face. It's the part you can finish alone — and now you have the exact path.

---

**— End of Laputa UI Building Guide.**
