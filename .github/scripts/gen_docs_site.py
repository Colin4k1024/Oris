#!/usr/bin/env python3
"""Generate _site/index.html — Oris documentation landing page."""
import os, html as _html

_VERSION = os.environ.get("VERSION", "?")
VERSION = _html.escape(_VERSION)

PAGE = f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Oris — Self-Evolving AI Execution Runtime</title>
<meta name="description" content="Oris is a Rust framework for supervised, bounded, closed-loop AI self-evolution. Capture signals, mutate safely, validate, promote, reuse.">
<link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/highlight.js@11.9.0/styles/github-dark.min.css">
<script src="https://cdn.jsdelivr.net/npm/highlight.js@11.9.0/lib/highlight.min.js"></script>
<script>document.addEventListener('DOMContentLoaded',()=>hljs.highlightAll())</script>
<style>
*,*::before,*::after{{box-sizing:border-box;margin:0;padding:0}}
:root{{
  --bg:#0d0d12;--s1:#13131a;--s2:#1a1a24;--s3:#22222e;
  --fg:#e8e8f0;--fg2:#a0a0bc;--fg3:#646480;
  --bd:#28283c;--bd2:#38384e;
  --ac:#818cf8;--ac2:#6366f1;
  --ac-bg:rgba(129,140,248,.1);--ac-bg2:rgba(129,140,248,.18);
  --grn:#34d399;--grn-bg:rgba(52,211,153,.12);
  --amb:#fbbf24;--amb-bg:rgba(251,191,36,.12);
  --red:#f87171;--red-bg:rgba(248,113,113,.12);
  --r:8px;--r2:12px;--max:1200px;--nav-h:60px;
}}
html{{scroll-behavior:smooth}}
body{{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI','Inter',sans-serif;background:var(--bg);color:var(--fg);line-height:1.6;font-size:16px}}
a{{color:var(--ac);text-decoration:none}}
a:hover{{color:#a5b4fc}}
::-webkit-scrollbar{{width:5px;height:5px}}
::-webkit-scrollbar-track{{background:var(--s1)}}
::-webkit-scrollbar-thumb{{background:var(--bd2);border-radius:3px}}

/* ── INLINE CODE ── */
code{{font-family:'SF Mono',Menlo,Monaco,Consolas,'Fira Code',monospace;font-size:.82em;background:var(--s2);padding:2px 6px;border-radius:4px;border:1px solid var(--bd);color:#c4b5fd}}

/* ── NAV ── */
#nav{{position:sticky;top:0;z-index:200;background:rgba(13,13,18,.9);backdrop-filter:blur(16px);border-bottom:1px solid var(--bd);height:var(--nav-h)}}
.nav-w{{max-width:var(--max);margin:0 auto;padding:0 24px;height:100%;display:flex;align-items:center;gap:8px}}
.logo{{font-weight:800;font-size:1.15rem;letter-spacing:-.04em;color:var(--fg);display:flex;align-items:center;gap:8px;flex-shrink:0}}
.logo-dot{{width:8px;height:8px;border-radius:50%;background:var(--ac);box-shadow:0 0 10px var(--ac);animation:blink 3s ease-in-out infinite}}
@keyframes blink{{0%,100%{{opacity:1}}50%{{opacity:.4}}}}
.nav-links{{display:flex;gap:2px;margin-left:20px}}
.nav-links a{{color:var(--fg2);font-size:.82rem;font-weight:500;padding:5px 10px;border-radius:5px;transition:all .15s}}
.nav-links a:hover,.nav-links a.spy-active{{color:var(--fg);background:var(--s2)}}
.nav-right{{margin-left:auto;display:flex;gap:8px;align-items:center}}
.gh-btn{{display:inline-flex;align-items:center;gap:6px;padding:6px 14px;border-radius:6px;font-size:.8rem;font-weight:600;background:var(--s2);border:1px solid var(--bd2);color:var(--fg);transition:all .15s}}
.gh-btn:hover{{background:var(--s3);color:var(--fg)}}
@media(max-width:900px){{.nav-links{{display:none}}}}

/* ── HERO ── */
#hero{{position:relative;overflow:hidden;padding:96px 24px 88px;background:var(--bg)}}
.hero-grid{{position:absolute;inset:0;background-image:linear-gradient(rgba(129,140,248,.04) 1px,transparent 1px),linear-gradient(90deg,rgba(129,140,248,.04) 1px,transparent 1px);background-size:60px 60px;mask-image:radial-gradient(ellipse 80% 60% at 50% 0%,black 30%,transparent 100%)}}
.hero-glow{{position:absolute;top:-150px;left:50%;transform:translateX(-50%);width:900px;height:500px;background:radial-gradient(ellipse,rgba(99,102,241,.2) 0%,transparent 65%);pointer-events:none}}
.hero-w{{max-width:var(--max);margin:0 auto;position:relative}}
.hero-pill{{display:inline-flex;align-items:center;gap:8px;background:var(--ac-bg);border:1px solid rgba(129,140,248,.3);color:var(--ac);font-size:.75rem;font-weight:700;padding:4px 14px;border-radius:20px;margin-bottom:24px;letter-spacing:.05em;text-transform:uppercase}}
.pill-dot{{width:6px;height:6px;border-radius:50%;background:var(--ac);animation:blink 2s infinite}}
h1.htitle{{font-size:clamp(2.4rem,6vw,3.8rem);font-weight:800;letter-spacing:-.05em;line-height:1.06;margin-bottom:20px;background:linear-gradient(140deg,#ffffff 30%,#a5b4fc 100%);-webkit-background-clip:text;-webkit-text-fill-color:transparent;background-clip:text}}
.hero-sub{{font-size:1.05rem;color:var(--fg2);max-width:580px;margin-bottom:36px;line-height:1.75}}
.hero-cta{{display:flex;gap:12px;flex-wrap:wrap;margin-bottom:40px}}
.btn{{display:inline-flex;align-items:center;gap:7px;padding:10px 22px;border-radius:7px;font-size:.88rem;font-weight:600;transition:all .15s;cursor:pointer;border:none;text-decoration:none}}
.btn-primary{{background:var(--ac2);color:#fff;box-shadow:0 0 24px rgba(99,102,241,.35)}}
.btn-primary:hover{{background:#5254e0;color:#fff;box-shadow:0 0 36px rgba(99,102,241,.5)}}
.btn-outline{{background:transparent;color:var(--fg);border:1px solid var(--bd2)}}
.btn-outline:hover{{background:var(--s2);color:var(--fg)}}
.hero-badges{{display:flex;gap:8px;flex-wrap:wrap}}
.hero-badges img{{height:20px;border-radius:3px}}

/* ── STATS BAR ── */
.stats-bar{{background:var(--s1);border-top:1px solid var(--bd);border-bottom:1px solid var(--bd)}}
.stats-w{{max-width:var(--max);margin:0 auto;display:flex;flex-wrap:wrap}}
.stat{{flex:1;min-width:140px;padding:20px 28px;border-right:1px solid var(--bd)}}
.stat:last-child{{border-right:none}}
.stat-n{{font-size:1.6rem;font-weight:800;letter-spacing:-.04em;color:var(--fg)}}
.stat-l{{font-size:.72rem;font-weight:600;text-transform:uppercase;letter-spacing:.07em;color:var(--fg3);margin-top:2px}}

/* ── GENERIC SECTION ── */
.sect{{padding:80px 24px}}
.sect.alt{{background:var(--s1)}}
.sect-w{{max-width:var(--max);margin:0 auto}}
.lbl{{font-size:.72rem;font-weight:700;text-transform:uppercase;letter-spacing:.1em;color:var(--ac);margin-bottom:10px}}
.stitle{{font-size:clamp(1.5rem,3vw,2rem);font-weight:700;letter-spacing:-.03em;line-height:1.2;margin-bottom:12px}}
.sdesc{{font-size:.93rem;color:var(--fg2);max-width:580px;margin-bottom:48px;line-height:1.75}}

/* ── FEATURE GRID ── */
.feat-grid{{display:grid;grid-template-columns:repeat(auto-fit,minmax(270px,1fr));gap:1px;background:var(--bd);border:1px solid var(--bd);border-radius:var(--r2);overflow:hidden}}
.feat-card{{background:var(--bg);padding:28px;transition:background .15s}}
.feat-card:hover{{background:var(--s2)}}
.f-icon{{width:38px;height:38px;border-radius:8px;background:var(--ac-bg);border:1px solid rgba(129,140,248,.2);display:flex;align-items:center;justify-content:center;font-size:1.1rem;margin-bottom:16px}}
.feat-card h3{{font-size:.93rem;font-weight:700;margin-bottom:8px}}
.feat-card p{{font-size:.83rem;color:var(--fg2);line-height:1.65}}

/* ── EVOLUTION LOOP ── */
.loop-grid{{display:grid;grid-template-columns:repeat(4,1fr);gap:1px;background:var(--bd);border:1px solid var(--bd);border-radius:var(--r2);overflow:hidden}}
@media(max-width:900px){{.loop-grid{{grid-template-columns:repeat(2,1fr)}}}}
@media(max-width:480px){{.loop-grid{{grid-template-columns:1fr}}}}
.loop-step{{background:var(--s1);padding:24px 20px;transition:background .15s}}
.loop-step:hover{{background:var(--s2)}}
.step-n{{font-size:.68rem;font-weight:800;letter-spacing:.1em;color:var(--fg3);text-transform:uppercase;margin-bottom:6px}}
.step-ico{{font-size:1.3rem;margin-bottom:6px}}
.loop-step h3{{font-size:.88rem;font-weight:700;margin-bottom:5px}}
.loop-step p{{font-size:.79rem;color:var(--fg2);line-height:1.55}}
.northstar{{margin-top:28px;padding:18px 24px;background:var(--ac-bg);border:1px solid rgba(129,140,248,.22);border-radius:var(--r);font-size:.87rem;color:var(--fg2);line-height:1.8}}
.northstar strong{{color:var(--ac)}}

/* ── TABS ── */
.tabs-wrap{{max-width:780px}}
.tab-bar{{display:flex;border-bottom:1px solid var(--bd)}}
.tab-btn{{padding:9px 16px;font-size:.81rem;font-weight:600;border:none;background:transparent;cursor:pointer;color:var(--fg2);border-bottom:2px solid transparent;margin-bottom:-1px;transition:all .15s}}
.tab-btn.active{{color:var(--ac);border-bottom-color:var(--ac)}}
.tab-btn:hover{{color:var(--fg)}}
.tab-panel{{display:none}}
.tab-panel.active{{display:block}}
.tab-panel pre{{background:var(--s2)!important;border:1px solid var(--bd);border-top:none;border-radius:0 0 var(--r) var(--r);overflow-x:auto;margin:0}}
.tab-panel pre code{{background:none!important;border:none;padding:0;color:inherit;font-size:.82rem;line-height:1.72}}
pre.hljs{{padding:20px 24px}}

/* ── CONCEPT TABS ── */
.ctabs{{display:grid;grid-template-columns:210px 1fr;border:1px solid var(--bd);border-radius:var(--r2);overflow:hidden}}
@media(max-width:680px){{.ctabs{{grid-template-columns:1fr}}}}
.cnav{{background:var(--s2);padding:10px 0;border-right:1px solid var(--bd)}}
.cnav-btn{{display:block;width:100%;text-align:left;padding:10px 18px;font-size:.84rem;font-weight:600;border:none;background:transparent;cursor:pointer;color:var(--fg2);transition:all .15s;border-left:3px solid transparent}}
.cnav-btn:hover{{color:var(--fg);background:var(--s3)}}
.cnav-btn.active{{color:var(--ac);background:var(--ac-bg);border-left-color:var(--ac)}}
.ccon{{background:var(--s1);padding:32px;overflow-y:auto;max-height:640px}}
.cpanel{{display:none}}
.cpanel.active{{display:block}}
.cpanel h3{{font-size:1.05rem;font-weight:700;margin-bottom:10px}}
.cpanel>.pdesc{{font-size:.87rem;color:var(--fg2);line-height:1.75;margin-bottom:20px}}
.cpanel pre{{background:var(--s2)!important;border:1px solid var(--bd);border-radius:var(--r);overflow-x:auto;margin-bottom:20px}}
.cpanel pre code{{background:none!important;border:none;padding:0;font-size:.8rem;line-height:1.68}}
.cpanel pre.hljs{{padding:16px 20px}}
.phase-list{{display:flex;flex-direction:column;gap:10px;margin-top:12px}}
.phase{{display:flex;gap:14px;padding:14px 16px;background:var(--s2);border:1px solid var(--bd);border-radius:var(--r)}}
.phase-badge{{flex-shrink:0;min-width:36px;height:22px;border-radius:4px;background:var(--ac-bg);border:1px solid rgba(129,140,248,.28);font-size:.7rem;font-weight:800;color:var(--ac);display:flex;align-items:center;justify-content:center;letter-spacing:.04em}}
.phase-info h4{{font-size:.84rem;font-weight:700;margin-bottom:3px}}
.phase-info p{{font-size:.79rem;color:var(--fg2);line-height:1.55}}
.pgrid{{display:grid;grid-template-columns:repeat(3,1fr);gap:8px;margin-top:14px}}
@media(max-width:500px){{.pgrid{{grid-template-columns:repeat(2,1fr)}}}}
.pcard{{padding:10px 14px;background:var(--s2);border:1px solid var(--bd);border-radius:6px}}
.pcard strong{{font-size:.82rem;font-weight:700;display:block;color:var(--fg)}}
.pcard span{{font-size:.72rem;color:var(--fg3)}}

/* ── ARCHITECTURE ── */
.arch-two{{display:grid;grid-template-columns:1fr 1fr;gap:24px}}
@media(max-width:720px){{.arch-two{{grid-template-columns:1fr}}}}
.arch-label{{font-size:.75rem;font-weight:700;text-transform:uppercase;letter-spacing:.07em;color:var(--fg3);margin-bottom:10px}}
.arch-diag{{background:var(--s2);border:1px solid var(--bd);border-radius:var(--r);padding:20px;font-family:'SF Mono',Menlo,Monaco,Consolas,monospace;font-size:.76rem;line-height:1.9;color:var(--fg);overflow-x:auto;white-space:pre}}
.abs-list{{display:flex;flex-direction:column;gap:8px}}
.abs{{padding:14px 16px;background:var(--s2);border:1px solid var(--bd);border-radius:var(--r)}}
.abs h4{{font-size:.85rem;font-weight:700;margin-bottom:4px;color:var(--fg)}}
.abs p{{font-size:.79rem;color:var(--fg2);line-height:1.55}}

/* ── TABLE ── */
.tbl-wrap{{overflow-x:auto;border:1px solid var(--bd);border-radius:var(--r2);overflow:hidden}}
table{{width:100%;border-collapse:collapse;font-size:.84rem}}
thead th{{text-align:left;padding:11px 14px;background:var(--s2);border-bottom:1px solid var(--bd2);font-size:.76rem;font-weight:700;text-transform:uppercase;letter-spacing:.05em;color:var(--fg2)}}
tbody td{{padding:11px 14px;border-bottom:1px solid var(--bd);vertical-align:top;line-height:1.55}}
tbody tr:last-child td{{border-bottom:none}}
tbody tr:hover td{{background:var(--s2)}}
.tag{{display:inline-block;padding:2px 8px;border-radius:10px;font-size:.72rem;font-weight:700}}
.t-s{{background:var(--grn-bg);color:var(--grn)}}
.t-e{{background:var(--amb-bg);color:var(--amb)}}
.t-v{{background:var(--ac-bg);color:var(--ac)}}

/* ── FEATURE FLAGS ── */
.flags-grp{{margin-bottom:36px}}
.flags-grp h3{{font-size:.78rem;font-weight:700;text-transform:uppercase;letter-spacing:.07em;color:var(--fg3);padding-bottom:8px;border-bottom:1px solid var(--bd);margin-bottom:0}}
.flag-row{{display:flex;gap:16px;padding:10px 0;border-bottom:1px solid var(--bd);font-size:.84rem;align-items:flex-start}}
.flag-row:last-child{{border-bottom:none}}
.flag-name{{flex-shrink:0;width:280px}}

/* ── FOOTER ── */
footer{{background:var(--s1);border-top:1px solid var(--bd);padding:56px 24px 36px}}
.footer-w{{max-width:var(--max);margin:0 auto}}
.footer-top{{display:grid;grid-template-columns:1.8fr 1fr 1fr 1fr;gap:40px;margin-bottom:44px}}
@media(max-width:860px){{.footer-top{{grid-template-columns:1fr 1fr}}}}
@media(max-width:480px){{.footer-top{{grid-template-columns:1fr}}}}
.footer-brand p{{font-size:.84rem;color:var(--fg2);line-height:1.65;max-width:260px;margin-top:12px}}
.footer-col h4{{font-size:.73rem;font-weight:700;text-transform:uppercase;letter-spacing:.08em;color:var(--fg3);margin-bottom:14px}}
.footer-col a{{display:block;color:var(--fg2);font-size:.84rem;margin-bottom:8px;transition:color .15s}}
.footer-col a:hover{{color:var(--fg)}}
.footer-btm{{padding-top:22px;border-top:1px solid var(--bd);display:flex;justify-content:space-between;flex-wrap:wrap;gap:8px;font-size:.78rem;color:var(--fg3)}}
</style>
</head>
<body>

<!-- NAV -->
<nav id="nav">
<div class="nav-w">
  <div class="logo"><div class="logo-dot"></div>Oris</div>
  <div class="nav-links">
    <a href="#why">Why Oris</a>
    <a href="#loop">How It Works</a>
    <a href="#quickstart">Quick Start</a>
    <a href="#concepts">Core Concepts</a>
    <a href="#hub">Hub</a>
    <a href="#architecture">Architecture</a>
    <a href="#crates">Crates</a>
    <a href="#flags">Feature Flags</a>
  </div>
  <div class="nav-right">
    <a class="gh-btn" href="https://github.com/Colin4k1024/Oris" target="_blank">
      <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor"><path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"/></svg>
      GitHub
    </a>
    <a class="btn btn-primary" href="https://docs.rs/oris-runtime" target="_blank" style="padding:6px 14px;font-size:.8rem">docs.rs</a>
  </div>
</div>
</nav>

<!-- HERO -->
<section id="hero">
  <div class="hero-grid"></div>
  <div class="hero-glow"></div>
  <div class="hero-w">
    <div class="hero-pill"><div class="pill-dot"></div>Self-Evolving AI Execution Runtime</div>
    <h1 class="htitle">Software that learns from<br>every execution</h1>
    <p class="hero-sub">Oris is a Rust framework for supervised, bounded, closed-loop software improvement. Capture signals. Generate mutations. Validate. Promote. Reuse. Reduce reasoning over time.</p>
    <div class="hero-cta">
      <a class="btn btn-primary" href="#quickstart">&#x25B6;&#xFE0F; Get Started</a>
      <a class="btn btn-outline" href="https://github.com/Colin4k1024/Oris" target="_blank">&#x2B50; Star on GitHub</a>
      <a class="btn btn-outline" href="https://docs.rs/oris-runtime" target="_blank">&#x1F4D6; API Docs</a>
    </div>
    <div class="hero-badges">
      <a href="https://crates.io/crates/oris-runtime" target="_blank"><img src="https://img.shields.io/crates/v/oris-runtime.svg?style=flat-square" alt="crates.io"></a>
      <a href="https://docs.rs/oris-runtime" target="_blank"><img src="https://img.shields.io/docsrs/oris-runtime?style=flat-square" alt="docs.rs"></a>
      <a href="https://codecov.io/gh/Colin4k1024/Oris" target="_blank"><img src="https://img.shields.io/codecov/c/github/Colin4k1024/Oris?style=flat-square" alt="coverage"></a>
      <img src="https://img.shields.io/badge/version-{VERSION}-818cf8?style=flat-square" alt="version">
      <img src="https://img.shields.io/badge/license-MIT-34d399?style=flat-square" alt="MIT">
    </div>
  </div>
</section>

<!-- STATS -->
<div class="stats-bar">
  <div class="stats-w">
    <div class="stat"><div class="stat-n">23</div><div class="stat-l">Library Crates</div></div>
    <div class="stat"><div class="stat-n">185K</div><div class="stat-l">Lines of Rust</div></div>
    <div class="stat"><div class="stat-n">295</div><div class="stat-l">Unit Tests</div></div>
    <div class="stat"><div class="stat-n">K1&ndash;K5</div><div class="stat-l">Kernel Phases</div></div>
    <div class="stat"><div class="stat-n">8</div><div class="stat-l">Evolution Stages</div></div>
    <div class="stat"><div class="stat-n">50+</div><div class="stat-l">Feature Flags</div></div>
  </div>
</div>

<!-- WHY ORIS -->
<section class="sect" id="why">
  <div class="sect-w">
    <div class="lbl">Why Oris</div>
    <h2 class="stitle">Systems that improve themselves</h2>
    <p class="sdesc">Most AI systems execute tasks without learning from them. Oris closes the loop — every execution is an opportunity to improve.</p>
    <div class="feat-grid">
      <div class="feat-card"><div class="f-icon">&#x1F4E1;</div><h3>Capture Real Signals</h3><p>Collect actionable signals from compiler failures, test regressions, panics, and runtime outcomes — not synthetic benchmarks.</p></div>
      <div class="feat-card"><div class="f-icon">&#x1F9EA;</div><h3>Safe Mutation Sandbox</h3><p>Generate candidate patches from successful patterns, executed in OS-level isolated child processes before touching production.</p></div>
      <div class="feat-card"><div class="f-icon">&#x2705;</div><h3>Two-Phase Validation</h3><p>Static analysis gates block bad mutations cheaply before the LLM critic runs — fast rejection first, expensive evaluation second.</p></div>
      <div class="feat-card"><div class="f-icon">&#x267B;&#xFE0F;</div><h3>Confidence-Aware Reuse</h3><p>Proven solutions become durable genes with confidence scores. Future runs replay them — fewer LLM calls every cycle.</p></div>
      <div class="feat-card"><div class="f-icon">&#x1F512;</div><h3>Supervised &amp; Bounded</h3><p>Fail-closed policy enforcement. No autonomous promotion without gate passage. Every decision is recorded and auditable.</p></div>
      <div class="feat-card"><div class="f-icon">&#x1F310;</div><h3>Cross-Node Gene Sharing</h3><p>Publish and fetch genes via the Oris Evolution Network with Ed25519-signed envelopes and federated queries.</p></div>
    </div>
  </div>
</section>

<!-- EVOLUTION LOOP -->
<section class="sect alt" id="loop">
  <div class="sect-w">
    <div class="lbl">How It Works</div>
    <h2 class="stitle">The 8-Stage Self-Evolution Loop</h2>
    <p class="sdesc">Every improvement follows a deterministic, auditable pipeline from signal intake to a reusable gene asset.</p>
    <div class="loop-grid">
      <div class="loop-step"><div class="step-n">Stage 01</div><div class="step-ico">&#x1F50D;</div><h3>Detect</h3><p>Collect actionable signals from compiler diagnostics, test failures, panics, and runtime outcomes.</p></div>
      <div class="loop-step"><div class="step-n">Stage 02</div><div class="step-ico">&#x1F3AF;</div><h3>Select</h3><p>Choose the best candidate gene or strategy from the gene pool using confidence scores and task-class matching.</p></div>
      <div class="loop-step"><div class="step-n">Stage 03</div><div class="step-ico">&#x1F9EC;</div><h3>Mutate</h3><p>Generate candidate patches derived from prior successful patterns and gene history — not random mutation.</p></div>
      <div class="loop-step"><div class="step-n">Stage 04</div><div class="step-ico">&#x1F4E6;</div><h3>Execute</h3><p>Run mutations inside a sandboxed child process with OS-level resource limits and budget enforcement.</p></div>
      <div class="loop-step"><div class="step-n">Stage 05</div><div class="step-ico">&#x2705;</div><h3>Validate</h3><p>Verify correctness through static analysis gates and configurable safety policies before any quality scoring.</p></div>
      <div class="loop-step"><div class="step-n">Stage 06</div><div class="step-ico">&#x1F4CA;</div><h3>Evaluate</h3><p>Two-phase quality scoring: static analysis score first, LLM critic only on candidates that pass the gate.</p></div>
      <div class="loop-step"><div class="step-n">Stage 07</div><div class="step-ico">&#x1F4BE;</div><h3>Solidify</h3><p>Promote successful mutations into durable, reusable genes in the SQLite gene store with full metadata.</p></div>
      <div class="loop-step"><div class="step-n">Stage 08</div><div class="step-ico">&#x267B;&#xFE0F;</div><h3>Reuse</h3><p>Replay proven genes with confidence tracking. Stale confidence triggers re-mutation automatically.</p></div>
    </div>
    <div class="northstar"><strong>North-Star Outcome:</strong> Task &rarr; Detect &rarr; Replay if trusted &rarr; Mutate only when needed &rarr; Validate &rarr; Capture &rarr; Reuse &rarr; <em>reduce reasoning over time</em></div>
  </div>
</section>

<!-- QUICK START -->
<section class="sect" id="quickstart">
  <div class="sect-w">
    <div class="lbl">Quick Start</div>
    <h2 class="stitle">Up and running in minutes</h2>
    <p class="sdesc">Add the crate, set your LLM API key, and run the canonical evolution scenario.</p>
    <div class="tabs-wrap">
      <div class="tab-bar" id="qs-bar">
        <button class="tab-btn active" onclick="switchTab('qs','install',this)">1. Add Dependency</button>
        <button class="tab-btn" onclick="switchTab('qs','server',this)">2. Start Server</button>
        <button class="tab-btn" onclick="switchTab('qs','evolve',this)">3. First Evolution</button>
        <button class="tab-btn" onclick="switchTab('qs','job',this)">4. Submit a Job</button>
      </div>
      <div id="qs-install" class="tab-panel active">
        <pre><code class="language-toml"># Cargo.toml
[dependencies]
oris-runtime = {{ version = "*", features = ["full-evolution-experimental"] }}

# Minimal setup — no evolution, just graphs + agents
# oris-runtime = {{ version = "*" }}</code></pre>
        <pre style="margin-top:0;border-top:1px solid #2d2d3d"><code class="language-bash"># Set your LLM provider key
export OPENAI_API_KEY="sk-..."        # OpenAI
export ANTHROPIC_API_KEY="sk-ant-..." # Anthropic (alternative)

cargo build</code></pre>
      </div>
      <div id="qs-server" class="tab-panel">
        <pre><code class="language-bash"># Start the HTTP execution server
cargo run -p oris-runtime \
  --example execution_server \
  --features "sqlite-persistence,execution-server"

# Defaults:
#   Listen:  http://127.0.0.1:8080
#   DB:      in-memory SQLite

# Override via env vars:
export ORIS_SERVER_ADDR=0.0.0.0:8080
export ORIS_SQLITE_DB=oris.db
export ORIS_RUNTIME_BACKEND=sqlite  # or postgres</code></pre>
      </div>
      <div id="qs-evolve" class="tab-panel">
        <pre><code class="language-bash"># Run the canonical evolution scenario
cargo run -p evo_oris_repo

# Run with observable artifacts
bash scripts/evo_first_run.sh
#   → target/evo_first_run/summary.json
#   → target/evo_first_run/run.log

# Focused example binaries
cargo run -p evo_oris_repo --bin intake_webhook_demo
cargo run -p evo_oris_repo --bin confidence_lifecycle_demo
cargo run -p evo_oris_repo --bin network_exchange</code></pre>
      </div>
      <div id="qs-job" class="tab-panel">
        <pre><code class="language-bash"># Submit a graph job to the execution server
curl -X POST http://127.0.0.1:8080/jobs \
  -H "Content-Type: application/json" \
  -d '{{"graph_name":"my_graph","input":{{"task":"example"}}}}'

# Response: {{"job_id":"abc123", "status":"pending"}}

# Poll status
curl http://127.0.0.1:8080/jobs/abc123

# Stream SSE events in real-time
curl -N http://127.0.0.1:8080/jobs/abc123/stream</code></pre>
      </div>
    </div>
  </div>
</section>

<!-- CORE CONCEPTS -->
<section class="sect alt" id="concepts">
  <div class="sect-w">
    <div class="lbl">Core Concepts</div>
    <h2 class="stitle">Understand the internals</h2>
    <p class="sdesc">Deep dives on the key abstractions — from the deterministic kernel to the plugin system.</p>
    <div class="ctabs">
      <div class="cnav">
        <button class="cnav-btn active" onclick="switchConcept('gene',this)">Gene &amp; Capsule</button>
        <button class="cnav-btn" onclick="switchConcept('kernel',this)">Deterministic Kernel</button>
        <button class="cnav-btn" onclick="switchConcept('pipeline',this)">Evolution Pipeline</button>
        <button class="cnav-btn" onclick="switchConcept('sandbox',this)">Mutation Sandbox</button>
        <button class="cnav-btn" onclick="switchConcept('governor',this)">Governor &amp; Policies</button>
        <button class="cnav-btn" onclick="switchConcept('plugins',this)">Plugin System (K4)</button>
      </div>
      <div class="ccon">

        <!-- Gene & Capsule -->
        <div id="concept-gene" class="cpanel active">
          <h3>Gene &amp; Capsule — Evolution Assets</h3>
          <p class="pdesc">A <strong>Gene</strong> is a proven, reusable solution to a class of problem. When a mutation succeeds, it is promoted and stored in the SQLite gene store with a confidence score, task-class label, and usage history. A <strong>Capsule</strong> bundles multiple related Genes for cross-node sharing via the Experience Repository.</p>
          <pre><code class="language-rust">// Gene lifecycle — from signal to reuse
let signal = IssueSignal {{
    source: SignalSource::CompilerDiagnostic,
    message: "cannot borrow `x` as mutable".into(),
    task_class: "borrow_checker_fix".into(),
}};

let result = evokernel.process(signal).await?;

match result {{
    EvolutionResult::Promoted(gene) => {{
        // gene.confidence starts at 1.0, decays over time
        // next identical signal replays this gene directly
        println!("Promoted gene {{}} (conf={{:.2}})", gene.id, gene.confidence);
    }}
    EvolutionResult::Replayed(gene) => {{
        println!("Replayed trusted gene {{}}", gene.id);
    }}
    EvolutionResult::Rejected => println!("No valid mutation found"),
}}</code></pre>
          <p class="pdesc">Confidence decays over time. When it falls below the configured threshold, the gene triggers a fresh mutation cycle instead of blind replay — ensuring stale knowledge is refreshed.</p>
        </div>

        <!-- Deterministic Kernel -->
        <div id="concept-kernel" class="cpanel">
          <h3>Deterministic Kernel — Phases K1 through K5</h3>
          <p class="pdesc">The <code>oris-kernel</code> crate provides a deterministic execution substrate: every action is recorded in an append-only event log, enabling replay, branch exploration, and zero-data-loss crash recovery.</p>
          <div class="phase-list">
            <div class="phase"><div class="phase-badge">K1</div><div class="phase-info"><h4>ExecutionStep contract + effect capture</h4><p>Freeze the <code>ExecutionStep</code> API. All side effects pass through <code>EffectSink</code>. Determinism guard enforces reproducible output.</p></div></div>
            <div class="phase"><div class="phase-badge">K2</div><div class="phase-info"><h4>Canonical log store + replay cursor</h4><p>Append-only <code>EventStore</code> records every action. <code>ReplayCursor</code> walks the log deterministically; <code>ReplayVerifier</code> detects any divergence.</p></div></div>
            <div class="phase"><div class="phase-badge">K3</div><div class="phase-info"><h4>Interrupt object + suspension state machine</h4><p>Interrupts are first-class objects driving a <code>KernelInterrupt</code> state machine. Suspended runs resume deterministically through replay — no lost state.</p></div></div>
            <div class="phase"><div class="phase-badge">K4</div><div class="phase-info"><h4>Plugin categories + execution sandbox</h4><p>9 plugin categories with determinism declarations, resource limits, version negotiation. External crates extend the runtime without forking.</p></div></div>
            <div class="phase"><div class="phase-badge">K5</div><div class="phase-info"><h4>Lease-based finalization + context-aware scheduler</h4><p>Zero-data-loss recovery via lease manager. Weighted priority dispatch, backpressure signaling, and circuit breaker pattern built-in.</p></div></div>
          </div>
        </div>

        <!-- Evolution Pipeline -->
        <div id="concept-pipeline" class="cpanel">
          <h3>EvolutionPipeline — Stage Internals</h3>
          <p class="pdesc">The pipeline in <code>oris-evolution/pipeline.rs</code> orchestrates all 8 stages. Each stage is a composable trait object. The pipeline short-circuits on rejection and emits per-stage telemetry at every boundary.</p>
          <pre><code class="language-rust">// Compose a custom pipeline
let pipeline = EvolutionPipeline::builder()
    .detector(CompilerSignalDetector::new())
    .selector(ConfidenceSelector::with_threshold(0.7))
    .mutator(LlmMutator::with_model("gpt-4o"))
    .sandbox(ProcessSandbox::with_limits(ResourceLimits::default()))
    .validator(StaticAnalysisValidator::new())
    .evaluator(TwoPhaseEvaluator::new(static_gate, llm_critic))
    .solidifier(SqliteGeneStore::open("genes.db")?)
    .build()?;

let report = pipeline.run(signal).await?;
// report.stage_durations — per-stage timing
// report.rejection_reason — why it failed (if any)</code></pre>
          <p class="pdesc">If a trusted gene exists with <code>confidence &gt;= threshold</code>, the pipeline enters <strong>replay mode</strong> — Mutate, Execute, Validate, and Evaluate are all skipped. This is how reasoning cost reduces over time.</p>
        </div>

        <!-- Mutation Sandbox -->
        <div id="concept-sandbox" class="cpanel">
          <h3>Mutation Sandbox — OS-level Isolation</h3>
          <p class="pdesc">The <code>oris-sandbox</code> crate runs candidate mutations in a controlled child process. Failed or runaway mutations cannot affect the host runtime. Enable fine-grained limits with the <code>resource-limits</code> feature flag.</p>
          <pre><code class="language-rust">let limits = ResourceLimits {{
    max_memory_bytes: 256 * 1024 * 1024,  // 256 MB
    max_cpu_seconds:  30,
    max_output_bytes: 10 * 1024 * 1024,   // 10 MB stdout
    network_access:   false,
}};

let sandbox = ProcessSandbox::new(limits);
let outcome = sandbox.execute(&candidate_patch).await?;

match outcome {{
    SandboxOutcome::Success(output)   => /* forward to validator */
    SandboxOutcome::Timeout           => /* penalize gene confidence */
    SandboxOutcome::MemoryExceeded    => /* reject immediately */
    SandboxOutcome::CompileError(err) => /* feed back to mutator */
}}</code></pre>
        </div>

        <!-- Governor -->
        <div id="concept-governor" class="cpanel">
          <h3>Governor &amp; Policies</h3>
          <p class="pdesc">The <code>oris-governor</code> crate enforces promotion, cooldown, and revocation policies — preventing runaway mutation cycles and ensuring no gene is promoted without meeting quality thresholds.</p>
          <pre><code class="language-rust">let policy = PromotionPolicy::builder()
    .min_confidence(0.75)
    .min_passes(2)                // must pass validator twice
    .cooldown(Duration::hours(1)) // prevent promotion thrash
    .max_revocations(3)           // archive after 3 regressions
    .build();

let governor = Governor::new(policy, gene_store.clone());

// Called automatically before the Solidify stage
if governor.allow_promotion(&candidate)? {{
    gene_store.save(candidate.into_gene())?;
}}</code></pre>
          <p class="pdesc">Revocation triggers automatically when a promoted gene causes a regression. After <code>max_revocations</code>, the gene is archived and a forced re-mutation cycle begins with a clean slate.</p>
        </div>

        <!-- Plugins -->
        <div id="concept-plugins" class="cpanel">
          <h3>Plugin System — 9 Categories (K4)</h3>
          <p class="pdesc">The K4 plugin system lets external crates extend Oris without forking the runtime. Plugins declare determinism contracts; the registry enforces version negotiation and resource limits at load time.</p>
          <div class="pgrid">
            <div class="pcard"><strong>Node</strong><span>Graph execution node</span></div>
            <div class="pcard"><strong>Tool</strong><span>Agent-callable tool</span></div>
            <div class="pcard"><strong>Memory</strong><span>Long-term memory backend</span></div>
            <div class="pcard"><strong>LLMAdapter</strong><span>LLM provider bridge</span></div>
            <div class="pcard"><strong>Scheduler</strong><span>Task dispatch strategy</span></div>
            <div class="pcard"><strong>Checkpoint</strong><span>State persistence</span></div>
            <div class="pcard"><strong>Effect</strong><span>Side-effect sink</span></div>
            <div class="pcard"><strong>Observer</strong><span>Telemetry / tracing</span></div>
            <div class="pcard"><strong>Governor</strong><span>Promotion policy</span></div>
          </div>
          <pre style="margin-top:16px"><code class="language-rust">// Implement a custom Tool plugin
#[derive(Plugin)]
#[plugin(category = "Tool", deterministic = false)]
struct MySearchTool;

impl ToolPlugin for MySearchTool {{
    fn name(&self) -> &str {{ "web_search" }}
    async fn call(&self, input: ToolInput) -> anyhow::Result&lt;ToolOutput&gt; {{
        // call your search API
        todo!()
    }}
}}

// Register at runtime startup
registry.register(Box::new(MySearchTool))?;</code></pre>
        </div>

      </div>
    </div>
  </div>
</section>

<!-- ARCHITECTURE -->
<section class="sect" id="architecture">
  <div class="sect-w">
    <div class="lbl">Architecture</div>
    <h2 class="stitle">Clean layered dependency DAG</h2>
    <p class="sdesc">23 library crates with a strict no-cycles dependency graph — each layer only reaches downward.</p>
    <div class="arch-two">
      <div>
        <div class="arch-label">Dependency Layers</div>
        <div class="arch-diag">Leaf  (no workspace deps)
  oris-agent-contract
  oris-economics
  oris-genestore
  oris-kernel
  oris-mutation-evaluator

Layer 1  (builds on leaf crates)
  oris-evolution       &rarr; oris-kernel
  oris-execution-runtime &rarr; oris-kernel
  oris-governor        &rarr; oris-evolution
  oris-intake          &rarr; oris-agent-contract, oris-evolution
  oris-sandbox         &rarr; oris-evolution
  oris-spec            &rarr; oris-evolution

Layer 2  (network-aware)
  oris-evolution-network &rarr; oris-evolution
  oris-orchestrator    &rarr; oris-agent-contract,
                          oris-evolution, oris-intake

Layer 3  (highest fan-in)
  oris-evokernel       &rarr; 11 crates
  oris-runtime         &rarr; oris-evokernel (opt),
                          oris-execution-runtime,
                          oris-kernel

Layer 4  (facades / standalone)
  oris-execution-server &rarr; oris-runtime
  oris-experience-repo  (standalone HTTP service)
  oris-hub              (standalone HTTP service)</div>
      </div>
      <div>
        <div class="arch-label">Key Abstractions</div>
        <div class="abs-list">
          <div class="abs"><h4>StateGraph / CompiledGraph</h4><p>Builder for stateful graphs with typed nodes, conditional edges, and persistence. <code>invoke()</code>, <code>stream()</code>, <code>step_once()</code>.</p></div>
          <div class="abs"><h4>Kernel (oris-kernel)</h4><p>Deterministic substrate: append-only event log, replay cursor, snapshot store. K1–K5 phases all complete.</p></div>
          <div class="abs"><h4>EvolutionPipeline</h4><p>Composable 8-stage Detect&rarr;Reuse orchestration. Per-stage telemetry, short-circuit on rejection, replay bypass.</p></div>
          <div class="abs"><h4>MutationEvaluator</h4><p>Two-phase quality gate: static analysis score first, LLM critic only on candidates that pass the gate.</p></div>
          <div class="abs"><h4>PluginRegistry (K4)</h4><p>9 plugin categories, determinism declarations, resource limits, semantic version negotiation at load time.</p></div>
          <div class="abs"><h4>SkeletonScheduler (K5)</h4><p>Context-aware weighted priority dispatch with backpressure signaling and circuit breaker pattern.</p></div>
        </div>
      </div>
    </div>
  </div>
</section>

<!-- HUB -->
<section class="sect alt" id="hub">
  <div class="sect-w">
    <div class="lbl">Experience Hub</div>
    <h2 class="stitle">Federated gene sharing at scale</h2>
    <p class="sdesc">The Hub connects multiple Experience Repository nodes — register, discover, federate queries, and subscribe to cross-node evolution events.</p>
    <div class="feat-grid" style="margin-bottom:48px">
      <div class="feat-card"><div class="f-icon">&#x1F4CB;</div><h3>Node Registry</h3><p>Register nodes with Ed25519 public keys. Key substitution prevention, health checking, and automatic deregistration on conflict.</p></div>
      <div class="feat-card"><div class="f-icon">&#x1F50E;</div><h3>Federated Discovery</h3><p>Fan-out gene searches across all healthy nodes. One API call returns aggregated, deduplicated results from the entire network.</p></div>
      <div class="feat-card"><div class="f-icon">&#x1F514;</div><h3>Event Subscriptions</h3><p>Subscribe to gene promotion events via webhook callbacks. Get notified when a proven gene appears on any connected node.</p></div>
      <div class="feat-card"><div class="f-icon">&#x1F6E1;&#xFE0F;</div><h3>Signed Envelopes</h3><p>All inter-node traffic uses OEN envelopes with Ed25519 signatures. Built-in rate limiting and PKI key registry.</p></div>
    </div>
    <p style="font-size:.8rem;font-weight:700;text-transform:uppercase;letter-spacing:.07em;color:var(--fg3);margin-bottom:14px">Hub Quick Start</p>
    <div class="tabs-wrap">
      <div class="tab-bar" id="hub-bar">
        <button class="tab-btn active" onclick="switchTab('hub','start',this)">Start Hub</button>
        <button class="tab-btn" onclick="switchTab('hub','register',this)">Register Node</button>
        <button class="tab-btn" onclick="switchTab('hub','query',this)">Federated Query</button>
        <button class="tab-btn" onclick="switchTab('hub','subscribe',this)">Subscribe</button>
      </div>
      <div id="hub-start" class="tab-panel active">
        <pre><code class="language-bash">cargo run -p oris-hub

# Configuration via environment variables
export HUB_ADDR=127.0.0.1:3000   # default: 0.0.0.0:3000
export HUB_DB_PATH=hub.db         # default: hub.db

# Dashboard:  http://localhost:3000/dashboard
# API base:   http://localhost:3000/api/v1</code></pre>
      </div>
      <div id="hub-register" class="tab-panel">
        <pre><code class="language-bash">curl -X POST http://localhost:3000/api/v1/nodes \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer &lt;token&gt;" \
  -d '{{
    "node_id":      "node-alpha",
    "endpoint":     "https://alpha.example.com",
    "public_key":   "&lt;base64-ed25519-pubkey&gt;",
    "capabilities": ["gene-store", "capsule-store"]
  }}'

curl http://localhost:3000/api/v1/nodes  # list all nodes</code></pre>
      </div>
      <div id="hub-query" class="tab-panel">
        <pre><code class="language-bash"># Fan-out gene search across all healthy nodes
curl "http://localhost:3000/api/v1/federation/genes?q=fix_timeout"

# Filter by task class and minimum confidence
curl "http://localhost:3000/api/v1/federation/genes?task_class=network_retry&min_confidence=0.8"

# Response aggregates results from every healthy node:
# {{"results":[...],"nodes_queried":3,"nodes_healthy":3}}</code></pre>
      </div>
      <div id="hub-subscribe" class="tab-panel">
        <pre><code class="language-bash">curl -X POST http://localhost:3000/api/v1/subscriptions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer &lt;token&gt;" \
  -d '{{
    "callback_url": "https://my-node.example.com/hooks/gene",
    "events":       ["gene_promoted","gene_revoked"],
    "filter":       {{"min_confidence":0.8}}
  }}'

# Hub delivers a signed POST to your callback_url
# when any registered node promotes or revokes a matching gene.</code></pre>
      </div>
    </div>
  </div>
</section>

<!-- CRATES REFERENCE -->
<section class="sect" id="crates">
  <div class="sect-w">
    <div class="lbl">Crates Reference</div>
    <h2 class="stitle">Component overview</h2>
    <p class="sdesc">23 library crates — include only what your project needs via feature flags.</p>
    <div class="tbl-wrap">
      <table>
        <thead><tr><th>Crate</th><th>Purpose</th><th>Maturity</th><th>Feature Flag</th></tr></thead>
        <tbody>
          <tr><td><code>oris-runtime</code></td><td>Main facade: agents, graphs, tools, RAG, multi-step execution</td><td><span class="tag t-s">stable</span></td><td>—</td></tr>
          <tr><td><code>oris-kernel</code></td><td>Deterministic execution: event log, replay, snapshot, K1–K5</td><td><span class="tag t-s">stable</span></td><td>—</td></tr>
          <tr><td><code>oris-evolution</code></td><td>Core types: Gene, Capsule, EvolutionEvent, Pipeline, Confidence, Task Classes</td><td><span class="tag t-s">stable</span></td><td><code>evolution-experimental</code></td></tr>
          <tr><td><code>oris-evokernel</code></td><td>Self-evolving kernel orchestration — highest fan-in, 11 workspace deps</td><td><span class="tag t-s">stable</span></td><td><code>full-evolution-experimental</code></td></tr>
          <tr><td><code>oris-execution-runtime</code></td><td>Control plane: scheduler, lease manager, repositories, circuit breaker, crash recovery</td><td><span class="tag t-s">stable</span></td><td>—</td></tr>
          <tr><td><code>oris-execution-server</code></td><td>Graph-aware HTTP execution server (Axum)</td><td><span class="tag t-s">stable</span></td><td><code>execution-server</code></td></tr>
          <tr><td><code>oris-sandbox</code></td><td>OS-level isolated mutation execution with resource budgets</td><td><span class="tag t-s">stable</span></td><td><code>evolution-experimental</code></td></tr>
          <tr><td><code>oris-mutation-evaluator</code></td><td>Two-phase quality evaluator: static analysis gate + LLM critic</td><td><span class="tag t-s">stable</span></td><td><code>evolution-experimental</code></td></tr>
          <tr><td><code>oris-genestore</code></td><td>SQLite-based Gene and Capsule persistence with full metadata</td><td><span class="tag t-s">stable</span></td><td>—</td></tr>
          <tr><td><code>oris-governor</code></td><td>Promotion, cooldown, and revocation policies</td><td><span class="tag t-s">stable</span></td><td><code>governor-experimental</code></td></tr>
          <tr><td><code>oris-intake</code></td><td>Issue intake, deduplication, prioritization, webhook support, CI failure parsing</td><td><span class="tag t-s">stable</span></td><td>—</td></tr>
          <tr><td><code>oris-evolution-network</code></td><td>OEN envelope, gossip sync, Ed25519 signing, rate-limited PKI registry</td><td><span class="tag t-e">experimental</span></td><td><code>evolution-network-experimental</code></td></tr>
          <tr><td><code>oris-experience-repo</code></td><td>HTTP API: gene/capsule sharing, Ed25519 OEN verification, PKI key service</td><td><span class="tag t-v">v{VERSION}</span></td><td>standalone</td></tr>
          <tr><td><code>oris-hub</code></td><td>Experience Hub: node registry, discovery, federation, subscriptions, dashboard</td><td><span class="tag t-s">stable</span></td><td>standalone</td></tr>
          <tr><td><code>oris-orchestrator</code></td><td>Autonomous loop, release automation, GitHub delivery, task planning</td><td><span class="tag t-e">experimental</span></td><td><code>release-automation-experimental</code></td></tr>
          <tr><td><code>oris-spec</code></td><td>OUSL YAML spec contracts and compilers</td><td><span class="tag t-e">experimental</span></td><td><code>spec-experimental</code></td></tr>
          <tr><td><code>oris-agent-contract</code></td><td>External agent proposal contracts (proposal-only interface)</td><td><span class="tag t-s">stable</span></td><td><code>agent-contract-experimental</code></td></tr>
          <tr><td><code>oris-economics</code></td><td>Local EVU ledger and reputation accounting</td><td><span class="tag t-e">experimental</span></td><td><code>economics-experimental</code></td></tr>
        </tbody>
      </table>
    </div>
  </div>
</section>

<!-- FEATURE FLAGS -->
<section class="sect alt" id="flags">
  <div class="sect-w">
    <div class="lbl">Feature Flags</div>
    <h2 class="stitle">Include only what you need</h2>
    <p class="sdesc">All flags are on <code>oris-runtime</code> unless otherwise noted. Use <code>full-evolution-experimental</code> to enable all evolution capabilities.</p>

    <div class="flags-grp">
      <h3>Evolution (Self-Improvement)</h3>
      <div class="flag-row"><div class="flag-name"><code>evolution-experimental</code></div><div>Core Gene, Capsule, Pipeline, Selector, Sandbox, MutationEvaluator</div></div>
      <div class="flag-row"><div class="flag-name"><code>evokernel-facade</code></div><div>Re-exports <code>oris-evokernel</code> — base for the full evolution stack</div></div>
      <div class="flag-row"><div class="flag-name"><code>governor-experimental</code></div><div>Promotion cooldown, revocation policies, budget enforcement</div></div>
      <div class="flag-row"><div class="flag-name"><code>evolution-network-experimental</code></div><div>OEN gossip sync, Ed25519 envelopes, rate-limited PKI registry</div></div>
      <div class="flag-row"><div class="flag-name"><code>economics-experimental</code></div><div>Local EVU ledger and reputation scoring</div></div>
      <div class="flag-row"><div class="flag-name"><code>spec-experimental</code></div><div>OUSL YAML spec contracts and compilers</div></div>
      <div class="flag-row"><div class="flag-name"><code>agent-contract-experimental</code></div><div>External agent proposal interface</div></div>
      <div class="flag-row"><div class="flag-name"><code>full-evolution-experimental</code></div><div>Aggregate — enables all of the above evolution flags</div></div>
    </div>

    <div class="flags-grp">
      <h3>Persistence &amp; Database</h3>
      <div class="flag-row"><div class="flag-name"><code>sqlite-persistence</code></div><div>SQLite checkpointing for graphs and kernel snapshots via rusqlite</div></div>
      <div class="flag-row"><div class="flag-name"><code>postgres</code></div><div>PostgreSQL vector store via pgvector + sqlx</div></div>
      <div class="flag-row"><div class="flag-name"><code>kernel-postgres</code></div><div>PostgreSQL event log backend for the deterministic kernel</div></div>
    </div>

    <div class="flags-grp">
      <h3>Vector Stores</h3>
      <div class="flag-row"><div class="flag-name"><code>surrealdb</code> &middot; <code>qdrant</code></div><div>SurrealDB and Qdrant vector store backends</div></div>
      <div class="flag-row"><div class="flag-name"><code>sqlite-vss</code> &middot; <code>sqlite-vec</code></div><div>SQLite-based vector search (two implementation variants)</div></div>
      <div class="flag-row"><div class="flag-name"><code>in-memory</code></div><div>In-memory vector store — for development and testing</div></div>
      <div class="flag-row"><div class="flag-name"><code>opensearch</code> &middot; <code>pinecone</code> &middot; <code>weaviate</code></div><div>Cloud-managed vector store backends</div></div>
      <div class="flag-row"><div class="flag-name"><code>chroma</code> &middot; <code>mongodb</code> &middot; <code>milvus</code> &middot; <code>faiss</code></div><div>Additional vector store backends</div></div>
    </div>

    <div class="flags-grp">
      <h3>LLM Providers</h3>
      <div class="flag-row"><div class="flag-name"><em>(built-in)</em></div><div>OpenAI and Anthropic are included by default</div></div>
      <div class="flag-row"><div class="flag-name"><code>ollama</code></div><div>Ollama local LLM — default host <code>http://localhost:11434</code></div></div>
      <div class="flag-row"><div class="flag-name"><code>gemini</code></div><div>Google Gemini</div></div>
      <div class="flag-row"><div class="flag-name"><code>mistralai</code></div><div>Mistral AI</div></div>
      <div class="flag-row"><div class="flag-name"><code>bedrock</code></div><div>AWS Bedrock</div></div>
    </div>

    <div class="flags-grp">
      <h3>Document Loaders &amp; Retrieval</h3>
      <div class="flag-row"><div class="flag-name"><code>lopdf</code> &middot; <code>pdf-extract</code></div><div>PDF document loaders (two variants)</div></div>
      <div class="flag-row"><div class="flag-name"><code>git</code> &middot; <code>aws-s3</code> &middot; <code>github</code></div><div>Git repository, S3, and GitHub loaders</div></div>
      <div class="flag-row"><div class="flag-name"><code>fastembed</code></div><div>FastEmbed local embedding model</div></div>
      <div class="flag-row"><div class="flag-name"><code>flashrank</code></div><div>FlashRank cross-encoder reranker</div></div>
      <div class="flag-row"><div class="flag-name"><code>tree-sitter</code></div><div>Code-aware text splitter (11 language parsers)</div></div>
    </div>

    <div class="flags-grp">
      <h3>Server &amp; Protocol</h3>
      <div class="flag-row"><div class="flag-name"><code>execution-server</code></div><div>Axum HTTP API server for job submission and SSE streaming</div></div>
      <div class="flag-row"><div class="flag-name"><code>mcp-experimental</code></div><div>MCP bootstrap endpoint (requires <code>execution-server</code>)</div></div>
      <div class="flag-row"><div class="flag-name"><code>a2a-production</code></div><div>Production A2A protocol boundary</div></div>
      <div class="flag-row"><div class="flag-name"><code>browser-use</code></div><div>Headless Chrome browser automation tool</div></div>
    </div>
  </div>
</section>

<!-- FOOTER -->
<footer>
  <div class="footer-w">
    <div class="footer-top">
      <div class="footer-brand">
        <div class="logo"><div class="logo-dot"></div>Oris</div>
        <p>A Rust framework for supervised, bounded, closed-loop AI self-evolution. Built for systems that need to improve themselves — safely and auditably.</p>
      </div>
      <div class="footer-col">
        <h4>Project</h4>
        <a href="https://github.com/Colin4k1024/Oris" target="_blank">GitHub Repository</a>
        <a href="https://crates.io/crates/oris-runtime" target="_blank">crates.io</a>
        <a href="https://docs.rs/oris-runtime" target="_blank">docs.rs (API)</a>
        <a href="https://codecov.io/gh/Colin4k1024/Oris" target="_blank">Code Coverage</a>
        <a href="https://github.com/Colin4k1024/Oris/blob/main/RELEASE.md" target="_blank">Release History</a>
      </div>
      <div class="footer-col">
        <h4>Documentation</h4>
        <a href="https://github.com/Colin4k1024/Oris/blob/main/docs/ARCHITECTURE.md" target="_blank">Architecture</a>
        <a href="https://github.com/Colin4k1024/Oris/blob/main/docs/kernel-api.md" target="_blank">Kernel API</a>
        <a href="https://github.com/Colin4k1024/Oris/blob/main/docs/plugin-authoring.md" target="_blank">Plugin Authoring</a>
        <a href="https://github.com/Colin4k1024/Oris/blob/main/docs/production-operations-guide.md" target="_blank">Operations Guide</a>
        <a href="https://github.com/Colin4k1024/Oris/blob/main/docs/ORIS_2.0_STRATEGY.md" target="_blank">2.0 Strategy</a>
      </div>
      <div class="footer-col">
        <h4>Community</h4>
        <a href="https://github.com/Colin4k1024/Oris/blob/main/CONTRIBUTING.md" target="_blank">Contributing</a>
        <a href="https://github.com/Colin4k1024/Oris/blob/main/CODE_OF_CONDUCT.md" target="_blank">Code of Conduct</a>
        <a href="https://github.com/Colin4k1024/Oris/blob/main/SECURITY.md" target="_blank">Security Policy</a>
        <a href="https://github.com/Colin4k1024/Oris/blob/main/LICENSE" target="_blank">MIT License</a>
      </div>
    </div>
    <div class="footer-btm">
      <span>&copy; 2026 Oris Contributors &mdash; MIT License</span>
      <span>oris-runtime v0.61.0 &middot; oris-experience-repo v{VERSION} &middot; oris-hub v0.1.0</span>
    </div>
  </div>
</footer>

<script>
// Tab switching — pass button explicitly, no reliance on global `event`
function switchTab(group, id, btn) {{
  var bar = btn.parentElement;
  var wrap = bar.parentElement;
  bar.querySelectorAll('.tab-btn').forEach(function(b) {{ b.classList.remove('active'); }});
  wrap.querySelectorAll('.tab-panel').forEach(function(p) {{ p.classList.remove('active'); }});
  btn.classList.add('active');
  var panel = document.getElementById(group + '-' + id);
  if (panel) panel.classList.add('active');
}}

// Concept panel switching
function switchConcept(id, btn) {{
  var nav = btn.parentElement;
  nav.querySelectorAll('.cnav-btn').forEach(function(b) {{ b.classList.remove('active'); }});
  document.querySelectorAll('.cpanel').forEach(function(p) {{ p.classList.remove('active'); }});
  btn.classList.add('active');
  var panel = document.getElementById('concept-' + id);
  if (panel) panel.classList.add('active');
}}

// Scroll-spy
(function() {{
  var links = document.querySelectorAll('.nav-links a[href^="#"]');
  var pairs = [];
  links.forEach(function(a) {{
    var id = a.getAttribute('href').slice(1);
    var el = document.getElementById(id);
    if (el) pairs.push({{ el: el, a: a }});
  }});
  function spy() {{
    var y = window.scrollY + 80;
    var active = null;
    pairs.forEach(function(p) {{ if (p.el.offsetTop <= y) active = p; }});
    links.forEach(function(a) {{ a.classList.remove('spy-active'); }});
    if (active) active.a.classList.add('spy-active');
  }}
  window.addEventListener('scroll', spy, {{passive: true}});
  spy();
}})();
</script>
</body>
</html>"""

os.makedirs("_site", exist_ok=True)
with open("_site/index.html", "w", encoding="utf-8") as f:
    f.write(PAGE)

print(f"Generated _site/index.html  ({len(PAGE):,} bytes)")
