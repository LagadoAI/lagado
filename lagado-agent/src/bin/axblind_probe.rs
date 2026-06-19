//! axblind_probe (v2) — the REAL a11y-coverage discriminator, not a meter.
//!
//! THE QUESTION (2026-06-19): captioned CV/vision only wins where the AT-SPI2 spine is BLIND to real
//! content. Before building Phase-2 captioning (expensive), measure how common that blindness is on the
//! agent's real surfaces. Cheap measurement gates expensive capability. See memory
//! `lagado-capture-mode-knob-parked`.
//!
//! v1 was a broken meter: box-count + sum-of-areas saturated to "100% blind everywhere" (an artifact —
//! a11y-rich GTK apps the agent clicks today CANNOT be 100% blind), the window-crop failed so the
//! textured wallpaper polluted every surface, and it compared CV against INTERACTIVE LEAVES only, which
//! makes every app look blind. The eyeball caught it and named the fix. v2 is that fix, built as the
//! signal Lagado was missing, not a throwaday.
//!
//! WHAT v2 MEASURES (the honest general quantity, classification done AFTER):
//!   Within the focused window rect (from xdotool, NOT perceive's bootstrap line), rasterize two masks:
//!     - CV-structure mask: where the classical-CV proposer sees visual structure.
//!     - a11y mask: where the FULL a11y tree (--focused-all: ALL non-noise elements incl. containers,
//!       minus the window-spanning root) reports an element. SMALL elements (<10% window) = widget
//!       granularity; LARGE elements (10–85%) = coarse container.
//!   Then for the CV-structure cells, the TRIAD (not a conflated scalar):
//!     - RICH      : CV cell covered by a SMALL a11y element  → a11y itemizes the content.
//!     - DEGRADED  : CV cell covered ONLY by a LARGE a11y element → a11y present but COARSE/lying
//!                   (one blob over many widgets) → wants geometry, not captions.
//!     - BLIND     : CV cell covered by NO a11y element → a11y ABSENT → captioning could add a target.
//!   Plus cv_coverage (how much structure exists at all → SPARSE when ~none).
//!
//! FAIL-CLOSED ON ITS OWN PRECONDITIONS: a surface is MEASURED only if (a) a real app window is focused
//! (active-window class is not the desktop) with valid geometry, and (b) the QMP frame decodes. Else it
//! is SKIPPED with a reason — never emitted as a polluted data point.
//!
//! HONESTY RAILS (unchanged from v1, still load-bearing):
//!   - BLIND is an UPPER BOUND on captioning payoff: Canny+CC can't tell a widget from a photo. So a LOW
//!     blind fraction is a STRONG no; a HIGH one is INCONCLUSIVE until the saved frames are eyeballed
//!     (CV boxes are colored by class: green=rich, yellow=degraded, red=blind; a11y boxes drawn blue).
//!   - TUIs are MEASURED but EXCLUDED from the captioning distribution: terminal content is genuinely
//!     a11y-blind but the agent already handles it via the CLI command channel, not screen captioning.
//!   - Output is a DISTRIBUTION (this fraction rich / canvas-blind / degraded, TUIs excluded, N skipped),
//!     NOT "X% blind". The workload weighting (how often the agent faces each surface) is the user's.
//!
//! Usage: axblind_probe          (boots the VM, walks the curated surfaces, shuts down). No llama needed.

#[cfg(not(unix))]
fn main() {
    eprintln!("[axblind_probe] Unix required");
}

#[cfg(unix)]
#[tokio::main]
async fn main() {
    use std::process::Command;
    use std::time::{Duration, Instant};

    use image::Rgb;
    use imageproc::drawing::draw_hollow_rect_mut;
    use imageproc::rect::Rect;

    use lagado_agent::perception::cv_proposer::propose_frame;
    use lagado_agent::perception::parse_ref_bboxes;
    use lagado_agent::vm::{QemuDesktopBackend, QmpClient, VmBackend, VmConfig};

    const FRAME: &str = "/dev/shm/axblind_frame.png";
    const SCALE: i32 = 4; // mask downscale: 1 grid cell = 4×4 px (≈ widget-edge resolution)
    let out_dir = "/tmp/axblind";
    let _ = std::fs::create_dir_all(out_dir);

    // ── ssh helpers (key auth, fail-fast) ──
    fn ssh(port: u16, cmd: &str) -> Option<String> {
        let out = Command::new("ssh")
            .args(["-o","StrictHostKeyChecking=no","-o","ConnectTimeout=5","-o","BatchMode=yes",
                   "-p",&port.to_string(),"laputa@127.0.0.1",cmd]).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }
    // Launch a GUI app detached. setsid puts it in a NEW session so it survives the ssh command
    // returning. MOZ_DISABLE_AUTO_SAFE_MODE stops firefox's post-crash "Troubleshoot Mode?" prompt
    // (harmless for non-firefox apps), so a -9'd firefox relaunches straight into the page.
    fn launch(port: u16, cmd: &str) {
        let _ = ssh(port, &format!("DISPLAY=:0 MOZ_DISABLE_AUTO_SAFE_MODE=1 MOZ_CRASHREPORTER_DISABLE=1 setsid nohup {cmd} >/dev/null 2>&1 < /dev/null &"));
    }
    fn intersect_clamp(b: (i32,i32,i32,i32), win: (i32,i32,i32,i32)) -> Option<(i32,i32,i32,i32)> {
        let (bx,by,bw,bh)=b; let (wx,wy,ww,wh)=win;
        let x0=bx.max(wx); let y0=by.max(wy);
        let x1=(bx+bw).min(wx+ww); let y1=(by+bh).min(wy+wh);
        (x1>x0 && y1>y0).then_some((x0,y0,x1-x0,y1-y0))
    }

    println!("══ AX-BLIND PROBE v2 (coverage discriminator) ══════════");
    println!("measuring: of the focused window, what fraction of visual structure does a11y represent —");
    println!("rich (itemized) / degraded (coarse blob) / blind (absent). TUIs excluded from the verdict.\n");

    // ── Boot VM (no llama needed) ──
    let t0 = Instant::now();
    let backend = QemuDesktopBackend::default();
    let cfg = VmConfig::default();
    let port = cfg.ssh_port;
    println!("[vm] booting…");
    let handle = match backend.boot(&cfg) { Ok(h)=>h, Err(e)=>{ eprintln!("[FAIL] boot: {e}"); std::process::exit(1);} };
    let deadline = Instant::now()+Duration::from_secs(240);
    let mut up=false;
    while Instant::now()<deadline { if let Some(w)=ssh(port,"whoami"){ if w.contains("laputa"){up=true;break;} } tokio::time::sleep(Duration::from_secs(3)).await; }
    if !up { eprintln!("[FAIL] ssh never up"); let _=backend.shutdown(handle); std::process::exit(1); }
    println!("[vm] ssh up after {:?}", t0.elapsed());
    let xdl=Instant::now()+Duration::from_secs(90);
    while Instant::now()<xdl { if ssh(port,"DISPLAY=:0 xdotool getdisplaygeometry 2>/dev/null").map(|g|!g.is_empty()).unwrap_or(false){break;} tokio::time::sleep(Duration::from_secs(3)).await; }
    println!("[vm] X up");

    // Ship the updated perceive.py (now with --focused-all).
    let _=Command::new("scp").args(["-o","StrictHostKeyChecking=no","-o","BatchMode=yes","-P",&port.to_string(),
        "perceive.py","laputa@127.0.0.1:/home/laputa/perceive.py"]).status();

    // ── Curated surfaces: (name, kind, launch_cmd) ──
    // kind: "control"=expect a11y-rich, "adversarial"=suspect blind, "tui"=blind-but-CLI-handled (excluded).
    let have=|c:&str| ssh(port,&format!("command -v {c} >/dev/null 2>&1 && echo y")).map(|s|s=="y").unwrap_or(false);
    let term = if have("xfce4-terminal"){Some("xfce4-terminal")} else if have("xterm"){Some("xterm")} else {None};
    let tui  = if have("htop"){Some("htop")} else if have("top"){Some("top")} else {None};
    let browser=["firefox","firefox-esr","chromium","chromium-browser","epiphany-browser","midori"].into_iter().find(|b|have(b));
    // ship a canvas page with drawn widgets a11y cannot see.
    {
        let html=r#"<!doctype html><html><body style="margin:0;background:#222"><canvas id=c width=900 height=650></canvas><script>var x=document.getElementById('c').getContext('2d');x.fillStyle='#2b2b2b';x.fillRect(0,0,900,650);var L=['Open','Save','Export','Delete','Run','Stop','Zoom+','Zoom-','Layer','Filter','Undo','Redo'];for(var i=0;i<L.length;i++){var cx=40+(i%4)*210,cy=40+((i/4)|0)*160;x.fillStyle='#3b82f6';x.fillRect(cx,cy,180,90);x.strokeStyle='#fff';x.strokeRect(cx,cy,180,90);x.fillStyle='#fff';x.font='20px sans-serif';x.fillText(L[i],cx+20,cy+50);}</script></body></html>"#;
        let local=format!("{out_dir}/canvas.html"); let _=std::fs::write(&local,html);
        let _=Command::new("scp").args(["-o","StrictHostKeyChecking=no","-o","BatchMode=yes","-P",&port.to_string(),&local,"laputa@127.0.0.1:/home/laputa/canvas.html"]).status();
    }
    let mut surfaces: Vec<(String,&'static str,String)>=Vec::new();
    if have("thunar"){surfaces.push(("thunar(file-mgr)".into(),"control","thunar /home/laputa".into()));}
    if have("mousepad"){surfaces.push(("mousepad(editor)".into(),"control","mousepad".into()));}
    if have("xfce4-appfinder"){surfaces.push(("appfinder".into(),"control","xfce4-appfinder".into()));}
    // (xfce4-settings-manager dropped: its AT-SPI app name ≠ its binary, so tine can't scope to it →
    //  empty a11y → a polluting false-blind point. thunar+mousepad+appfinder cover the GTK-rich case.)
    // Plain launch; MOZ_DISABLE_AUTO_SAFE_MODE (in launch()) + sessionstore wipe (in cleanup) keep the
    // relaunch dialog-free. Default profile (the path-based --profile gave "Profile Missing").
    if let Some(b)=browser{surfaces.push(("browser-chrome".into(),"control",format!("{b} --new-window about:blank")));}
    if let Some(b)=browser{surfaces.push(("canvas(drawn)".into(),"adversarial",format!("{b} --new-window file:///home/laputa/canvas.html")));}
    if let (Some(t),Some(tu))=(term,tui){
        let c=if t=="xfce4-terminal"{format!("xfce4-terminal --command={tu}")}else{format!("xterm -e {tu}")};
        surfaces.push((format!("terminal+{tu}(TUI)"),"tui",c));
    }
    println!("[probe] apps: term={:?} tui={:?} browser={:?}", term,tui,browser);
    if browser.is_none(){ println!("[probe] NOTE: no browser → canvas-WIDGET surface NOT tested (lower bound on coverage)."); }
    println!();

    // ── Per-surface measurement ──
    struct Row{name:String,kind:&'static str,status:String,a11y_n:usize,cv_cov:f32,rich:f32,degraded:f32,blind:f32,blob:f32,class:&'static str}
    let mut rows:Vec<Row>=Vec::new();

    // Enumerate managed top-level windows via EWMH _NET_CLIENT_LIST (canonical; focus-independent).
    // Close every one that isn't the panel/desktop → clean slate between surfaces.
    let cleanup = "DISPLAY=:0 bash -c 'for w in $(xprop -root _NET_CLIENT_LIST 2>/dev/null | sed \"s/.*# //; s/,//g\"); do c=$(xdotool getwindowclassname $w 2>/dev/null); case \"${c,,}\" in *desktop*|*panel*|*whisker*) ;; *) xdotool windowclose $w 2>/dev/null ;; esac; done'; true";
    // List managed windows as `wid class x y w h` (same getwindowgeometry perceive.py uses → coord-consistent).
    let lister = "DISPLAY=:0 bash -c 'for w in $(xprop -root _NET_CLIENT_LIST 2>/dev/null | sed \"s/.*# //; s/,//g\"); do c=$(xdotool getwindowclassname $w 2>/dev/null); eval $(xdotool getwindowgeometry --shell $w 2>/dev/null); echo \"$w ${c:-none} ${X:-0} ${Y:-0} ${WIDTH:-0} ${HEIGHT:-0}\"; done'";

    println!("{:<20} {:<6} {:>6} {:>7} {:>6} {:>9} {:>6} {:>6}  {}","surface","kind","a11yN","cvCov%","rich%","degraded%","blind%","unrch%","class");
    println!("{:-<98}","");

    for (name,kind,cmd) in &surfaces {
        let _=ssh(port,cleanup);
        // kill known probe apps explicitly (NOT a dynamic `pkill -f bash`, which would hit shells/ssh).
        let _=ssh(port,"for a in firefox thunar mousepad xfce4-appfinder xfce4-settings-manager htop xfce4-terminal; do pkill -9 $a 2>/dev/null; done; true");
        // wipe firefox crash/session state so a relaunch never shows the restore/troubleshoot dialog.
        let _=ssh(port,"rm -rf ~/.mozilla/firefox/*/sessionstore* ~/.mozilla/firefox/*/sessionCheckpoints.json ~/.mozilla/firefox/Crash\\ Reports ~/.mozilla/firefox/*/.parentlock 2>/dev/null; true");
        tokio::time::sleep(Duration::from_millis(900)).await;

        launch(port,cmd);

        // PRECONDITION (focus-independent): find the app window (largest non-panel/desktop), then
        // force-activate it so perceive.py --focused-all (which keys off getactivewindow) reads THIS one.
        let mut win:Option<(i32,i32,i32,i32)>=None; let mut wclass=String::new(); let mut wid=String::new();
        let wait=Instant::now()+Duration::from_secs(if *kind!="control"{13}else{10});
        while Instant::now()<wait {
            let list=ssh(port,lister).unwrap_or_default();
            let mut best:Option<(String,String,(i32,i32,i32,i32))>=None;
            for ln in list.lines(){
                let f:Vec<&str>=ln.split_whitespace().collect();
                if f.len()<6 {continue;}
                let cls=f[1].to_lowercase();
                // class via xdotool is unreliable here (often empty→"none"); exclude desktop/panel by NAME
                // when known AND by GEOMETRY (full-screen-at-origin = the root desktop), keep real apps.
                if cls.contains("desktop")||cls.contains("panel")||cls.contains("whisker"){continue;}
                let (x,y,w,h)=(f[2].parse().unwrap_or(0),f[3].parse().unwrap_or(0),f[4].parse().unwrap_or(0),f[5].parse().unwrap_or(0));
                if w<60||h<60 {continue;}
                if x<=0&&y<=0&&w>=1270&&h>=798 {continue;} // the root/desktop window
                let area=(w as i64)*(h as i64);
                if best.as_ref().map(|(_,_,bb)|((bb.2 as i64)*(bb.3 as i64))<area).unwrap_or(true){ best=Some((f[0].to_string(),f[1].to_string(),(x,y,w,h))); }
            }
            if let Some((id,cls,geo))=best { wid=id; wclass=cls; win=Some(geo); break; }
            tokio::time::sleep(Duration::from_millis(600)).await;
        }
        let mut win = match win {
            Some(w)=>w,
            None=>{ let dbg=ssh(port,lister).unwrap_or_default();
                    println!("{:<20} {:<6} SKIP (app window never appeared). window list →",name,kind);
                    for l in dbg.lines().take(12){ println!("        {l}"); }
                    if dbg.trim().is_empty(){ println!("        (empty — xprop/_NET_CLIENT_LIST returned nothing)"); }
                    rows.push(Row{name:name.clone(),kind,status:"SKIP:no-window".into(),a11y_n:0,cv_cov:0.0,rich:0.0,degraded:0.0,blind:0.0,blob:0.0,class:"-"}); continue; }
        };
        // Force the discovered window frontmost+focused (timeout-guarded so a stubborn --sync can't hang).
        let _=ssh(port,&format!("DISPLAY=:0 timeout 3 xdotool windowactivate --sync {wid} 2>/dev/null; DISPLAY=:0 xdotool windowfocus {wid} 2>/dev/null; true"));
        tokio::time::sleep(Duration::from_millis(900)).await;
        // Re-read THIS window's geometry (firefox & co. resize after first paint).
        if let Some(g)=ssh(port,&format!("DISPLAY=:0 xdotool getwindowgeometry --shell {wid} 2>/dev/null")){
            let gk=|k:&str| g.lines().find_map(|l| l.strip_prefix(k)?.strip_prefix('=')?.trim().parse::<i32>().ok());
            if let (Some(x),Some(y),Some(w),Some(h))=(gk("X"),gk("Y"),gk("WIDTH"),gk("HEIGHT")){ if w>60&&h>60 { win=(x,y,w,h); } }
        }

        // a11y FULL tree (containers included), screen-absolute.
        let screen=ssh(port,"DISPLAY=:0 python3 ~/perceive.py --focused-all 2>/dev/null").unwrap_or_default();
        let a11y=parse_ref_bboxes(&screen);
        if a11y.is_empty() {
            let active=ssh(port,"DISPLAY=:0 xdotool getactivewindow 2>&1").unwrap_or_default();
            let nc=ssh(port,"DISPLAY=:0 xdotool getactivewindow getwindowname getwindowclassname 2>&1").unwrap_or_default();
            let foc=ssh(port,"DISPLAY=:0 python3 ~/perceive.py --focused 2>&1").unwrap_or_default();
            println!("   [diag {name}] chosen wid={wid} ({win:?}); getactivewindow={active}");
            println!("   [diag {name}] active name|class: {:?}", nc.replace('\n'," | "));
            println!("   [diag {name}] --focused-all head: {:?}", screen.lines().take(4).collect::<Vec<_>>());
            println!("   [diag {name}] --focused      head: {:?}", foc.lines().take(4).collect::<Vec<_>>());
        }

        // CV frame via QMP, cropped to the window.
        let frame_ok = QmpClient::connect(&cfg.qmp_socket).map(|mut q| q.screendump(FRAME).is_ok()).unwrap_or(false);
        let img = if frame_ok { std::fs::read(FRAME).ok().and_then(|b| image::load_from_memory(&b).ok()) } else { None };
        let mut rgb = match img { Some(i)=>i.to_rgb8(), None=>{
            println!("{:<20} {:<6} SKIP  (no frame)",name,kind);
            rows.push(Row{name:name.clone(),kind,status:"SKIP:no-frame".into(),a11y_n:0,cv_cov:0.0,rich:0.0,degraded:0.0,blind:0.0,blob:0.0,class:"-"}); continue; } };
        // CROP the image to the window BEFORE running CV — guarantees CV cannot see the wallpaper
        // (box-filtering depended on `win` being exact; cropping the pixels is unconditional).
        let (wx,wy,ww,wh)=win;
        let iw=rgb.width() as i32; let ih=rgb.height() as i32;
        let cx=wx.clamp(0,iw-1); let cy=wy.clamp(0,ih-1);
        let cw=ww.min(iw-cx).max(1); let ch=wh.min(ih-cy).max(1);
        let sub=image::imageops::crop_imm(&rgb,cx as u32,cy as u32,cw as u32,ch as u32).to_image();
        let cv:Vec<(i32,i32,i32,i32)>=propose_frame(sub.as_raw(),sub.width(),sub.height())
            .iter().map(|b|(b.x+cx,b.y+cy,b.w,b.h)).collect();
        println!("    [geom {name}] win=({wx},{wy},{ww},{wh}) crop=({cx},{cy},{cw},{ch}) cv_boxes={} a11y={}", cv.len(), a11y.len());

        // ── rasterize masks over the window grid (wx,wy,ww,wh from the crop block above) ──
        let gw=((ww/SCALE).max(1)) as usize; let gh=((wh/SCALE).max(1)) as usize;
        let mut cv_g=vec![false;gw*gh]; let mut sm_g=vec![false;gw*gh]; let mut lg_g=vec![false;gw*gh];
        let mut paint=|grid:&mut Vec<bool>, b:(i32,i32,i32,i32)| {
            let (bx,by,bw_,bh_)=b;
            let gx0=(((bx-wx)/SCALE).max(0) as usize).min(gw.saturating_sub(1));
            let gy0=(((by-wy)/SCALE).max(0) as usize).min(gh.saturating_sub(1));
            let gx1=((((bx-wx+bw_)/SCALE)).max(0) as usize).min(gw.saturating_sub(1));
            let gy1=((((by-wy+bh_)/SCALE)).max(0) as usize).min(gh.saturating_sub(1));
            for gy in gy0..=gy1 { for gx in gx0..=gx1 { grid[gy*gw+gx]=true; } }
        };
        let win_area=(ww as f32*wh as f32).max(1.0);
        // a11y: split SMALL (<10% win) vs LARGE (10–85%); drop root (≥85%).
        for &b in a11y.values() {
            if let Some(ib)=intersect_clamp(b,win){
                let frac=(b.2 as f32*b.3 as f32)/win_area;
                if frac>=0.85 { continue; }
                if frac>=0.10 { paint(&mut lg_g,ib); } else { paint(&mut sm_g,ib); }
            }
        }
        for &b in &cv { paint(&mut cv_g,b); }

        // ── the triad over CV-structure cells ──
        let total=(gw*gh) as f32;
        let mut cvc=0usize; let (mut rich,mut deg,mut blind)=(0usize,0usize,0usize);
        for i in 0..gw*gh { if cv_g[i] { cvc+=1;
            if sm_g[i]{rich+=1;} else if lg_g[i]{deg+=1;} else {blind+=1;} } }
        let cv_cov=cvc as f32/total;
        let (rich_f,deg_f,blind_f)= if cvc>0 {(rich as f32/cvc as f32, deg as f32/cvc as f32, blind as f32/cvc as f32)} else {(0.0,0.0,0.0)};

        // ── a11y PROXIMITY (the real absence signal) ──
        // The frames showed why per-cell / contiguity / zone all failed: in an a11y-rich app CV
        // over-fragments AROUND the widgets, so fragments land just OUTSIDE (but adjacent to) the a11y
        // boxes; on a canvas the structure sits FAR from any a11y (which lives only in the chrome). So
        // the test is proximity: dilate the a11y mask by a widget-margin R, then a CV cell is truly
        // BLIND only if NO a11y lies within R of it. Fragments hugging an icon → covered; a button
        // 40px from the nearest a11y → blind.
        let r:i32=5; // 5 grid cells × 4px = 20px widget margin
        let mut a_dil=vec![false;gw*gh];
        for gy in 0..gh as i32 { for gx in 0..gw as i32 {
            if sm_g[gy as usize*gw+gx as usize] || lg_g[gy as usize*gw+gx as usize] {
                for dy in -r..=r { for dx in -r..=r {
                    let (nx,ny)=(gx+dx,gy+dy);
                    if nx>=0&&ny>=0&&(nx as usize)<gw&&(ny as usize)<gh { a_dil[ny as usize*gw+nx as usize]=true; }
                }}
            }
        }}
        let mut cvn=0usize; let mut farblind=0usize;
        for i in 0..gw*gh { if cv_g[i] { cvn+=1; if !a_dil[i] { farblind+=1; } } }
        let unreached = if cvn>0 { farblind as f32/cvn as f32 } else {0.0}; // CV structure with NO a11y within R

        // ── classify AFTER measuring (raw fractions above are the truth) ──
        // CANVAS-BLIND = most structure has NO a11y anywhere near it (canvas), vs fragments hugging
        // a11y widgets (file mgr → low).
        let class = if cv_cov<0.03 {"SPARSE"}
            else if unreached>=0.50 {"CANVAS-BLIND"}
            else if deg_f>=0.40 {"DEGRADED"}
            else {"RICH"};

        // ── annotate (CV boxes colored by class; a11y boxes blue) for the eyeball ──
        let cell_class=|cx:i32,cy:i32|->u8{ // 0 rich,1 deg,2 blind
            let gx=(((cx-wx)/SCALE).max(0) as usize).min(gw.saturating_sub(1));
            let gy=(((cy-wy)/SCALE).max(0) as usize).min(gh.saturating_sub(1));
            let i=gy*gw+gx; if sm_g[i]{0}else if lg_g[i]{1}else{2} };
        for &b in a11y.values(){ if b.2>0&&b.3>0 { draw_hollow_rect_mut(&mut rgb,Rect::at(b.0,b.1).of_size(b.2 as u32,b.3 as u32),Rgb([60,120,255])); } }
        for &(bx,by,bw_,bh_) in &cv { if bw_>0&&bh_>0 {
            let col=match cell_class(bx+bw_/2,by+bh_/2){0=>Rgb([40,220,40]),1=>Rgb([235,200,20]),_=>Rgb([255,40,40])};
            draw_hollow_rect_mut(&mut rgb,Rect::at(bx,by).of_size(bw_ as u32,bh_ as u32),col); } }
        let safe=name.replace(['(',')','/','+',' '],"_");
        let _=rgb.save(format!("{out_dir}/{safe}.png"));

        println!("{:<20} {:<6} {:>6} {:>6.1} {:>6.1} {:>9.1} {:>6.1} {:>6.1}  {}",
            name,kind,a11y.len(),cv_cov*100.0,rich_f*100.0,deg_f*100.0,blind_f*100.0,unreached*100.0,class);
        rows.push(Row{name:name.clone(),kind,status:"OK".into(),a11y_n:a11y.len(),cv_cov,rich:rich_f,degraded:deg_f,blind:blind_f,blob:unreached,class});
    }
    let _=ssh(port,cleanup);

    // ── DISTRIBUTION (TUIs excluded; skips reported separately) ──
    println!("\n── distribution (captioning-relevant surfaces; TUIs excluded, skips separate) ──");
    let relevant:Vec<&Row>=rows.iter().filter(|r|r.status=="OK" && r.kind!="tui").collect();
    let skipped=rows.iter().filter(|r|r.status.starts_with("SKIP")).count();
    let tui_n=rows.iter().filter(|r|r.kind=="tui"&&r.status=="OK").count();
    let n=relevant.len().max(1);
    let cnt=|c:&str| relevant.iter().filter(|r|r.class==c).count();
    let (rich,blindc,degc,sparse)=(cnt("RICH"),cnt("CANVAS-BLIND"),cnt("DEGRADED"),cnt("SPARSE"));
    println!("  measured (non-TUI): {}", relevant.len());
    println!("    RICH         {rich}/{}  ({:.0}%)  — a11y itemizes the content", relevant.len(), rich as f32*100.0/n as f32);
    println!("    CANVAS-BLIND {blindc}/{}  ({:.0}%)  — a11y ABSENT where structure is → captioning could win", relevant.len(), blindc as f32*100.0/n as f32);
    println!("    DEGRADED     {degc}/{}  ({:.0}%)  — a11y present but COARSE → wants geometry, not captions", relevant.len(), degc as f32*100.0/n as f32);
    println!("    SPARSE       {sparse}/{}  ({:.0}%)  — little structure; blindness moot", relevant.len(), sparse as f32*100.0/n as f32);
    println!("  excluded: {tui_n} TUI (CLI-handled), {skipped} skipped (precondition fail)");
    println!("\n  Annotated frames in {out_dir}/ — CV boxes: GREEN=rich YELLOW=degraded RED=blind; a11y=blue.");
    println!("  BLIND is an UPPER BOUND (CV can't tell widget from photo). HIGH blind ⇒ eyeball the red boxes.");
    println!("  NOT workload-weighted — re-weight CANVAS-BLIND by how often the agent faces canvas/custom-drawn apps.");

    let _=backend.shutdown(handle);
    println!("\n[axblind_probe v2] done — total {:?}", t0.elapsed());
}
