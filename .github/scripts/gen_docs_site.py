#!/usr/bin/env python3
"""Generate _site/index.html — user-facing project homepage for Oris."""
import os

VERSION = os.environ.get("VERSION", "?")

PAGE = f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Oris — AI Self-Evolution Framework</title>
<link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/highlight.js@11.9.0/styles/github.min.css">
<script src="https://cdn.jsdelivr.net/npm/highlight.js@11.9.0/lib/highlight.min.js"></script>
<script>document.addEventListener('DOMContentLoaded',()=>hljs.highlightAll());</script>
<style>
*,*::before,*::after{{box-sizing:border-box;margin:0;padding:0}}
:root{{
  --bg:#ffffff;--fg:#111827;--muted:#6b7280;--border:#e5e7eb;
  --accent:#4f46e5;--accent-light:#ede9fe;--accent-dark:#3730a3;
  --green:#065f46;--green-bg:#d1fae5;
  --code-bg:#f6f8fa;--radius:8px;
  --max:960px;
}}
html{{scroll-behavior:smooth}}
body{{font-family:system-ui,-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;color:var(--fg);background:var(--bg);line-height:1.6;font-size:16px}}

/* NAV */
nav{{position:sticky;top:0;z-index:100;background:rgba(255,255,255,.95);backdrop-filter:blur(8px);border-bottom:1px solid var(--border);padding:0 24px}}
.nav-inner{{max-width:var(--max);margin:0 auto;display:flex;align-items:center;height:52px;gap:24px}}
.nav-logo{{font-weight:700;font-size:1.1rem;color:var(--fg);text-decoration:none;letter-spacing:-.02em}}
.nav-logo span{{color:var(--accent)}}
.nav-links{{display:flex;gap:20px;margin-left:auto}}
.nav-links a{{color:var(--muted);text-decoration:none;font-size:.88rem;font-weight:500;transition:color .15s}}
.nav-links a:hover{{color:var(--fg)}}

/* HERO */
.hero{{background:linear-gradient(135deg,#f8f9ff 0%,#ffffff 100%);border-bottom:1px solid var(--border);padding:72px 24px 80px}}
.hero-inner{{max-width:var(--max);margin:0 auto}}
.hero-badge{{display:inline-flex;align-items:center;gap:6px;background:var(--accent-light);color:var(--accent);font-size:.75rem;font-weight:600;padding:3px 12px;border-radius:20px;margin-bottom:20px;letter-spacing:.03em}}
.hero h1{{font-size:clamp(2rem,5vw,3rem);font-weight:800;letter-spacing:-.04em;line-height:1.1;margin-bottom:16px}}
.hero h1 span{{color:var(--accent)}}
.hero-sub{{font-size:1.1rem;color:var(--muted);max-width:600px;margin-bottom:28px;line-height:1.7}}
.hero-cta{{display:flex;gap:12px;flex-wrap:wrap;margin-bottom:32px}}
.btn{{display:inline-flex;align-items:center;gap:6px;padding:10px 20px;border-radius:6px;font-size:.9rem;font-weight:600;text-decoration:none;transition:all .15s}}
.btn-primary{{background:var(--accent);color:#fff}}
.btn-primary:hover{{background:var(--accent-dark)}}
.btn-ghost{{border:1px solid var(--border);color:var(--fg);background:#fff}}
.btn-ghost:hover{{background:var(--code-bg)}}
.badges{{display:flex;gap:8px;flex-wrap:wrap}}
.badges img{{height:20px}}

/* SECTIONS */
.section{{padding:64px 24px}}
.section:nth-child(even){{background:#fafafa}}
.section-inner{{max-width:var(--max);margin:0 auto}}
.section-label{{font-size:.75rem;font-weight:700;text-transform:uppercase;letter-spacing:.08em;color:var(--accent);margin-bottom:8px}}
.section h2{{font-size:1.75rem;font-weight:700;letter-spacing:-.03em;margin-bottom:12px}}
.section-desc{{color:var(--muted);max-width:600px;margin-bottom:40px;line-height:1.7}}

/* FEATURES grid */
.features{{display:grid;grid-template-columns:repeat(auto-fit,minmax(220px,1fr));gap:20px}}
.feature{{background:#fff;border:1px solid var(--border);border-radius:var(--radius);padding:20px;transition:box-shadow .15s}}
.feature:hover{{box-shadow:0 4px 16px rgba(0,0,0,.06)}}
.feature-icon{{font-size:1.5rem;margin-bottom:10px}}
.feature h3{{font-size:.95rem;font-weight:700;margin-bottom:6px}}
.feature p{{font-size:.85rem;color:var(--muted);line-height:1.6}}

/* EVOLUTION LOOP */
.loop-grid{{display:grid;grid-template-columns:repeat(auto-fit,minmax(200px,1fr));gap:16px}}
.loop-step{{background:#fff;border:1px solid var(--border);border-radius:var(--radius);padding:16px 20px;position:relative}}
.loop-num{{position:absolute;top:14px;right:16px;font-size:.75rem;font-weight:700;color:var(--border)}}
.loop-step h3{{font-size:.9rem;font-weight:700;margin-bottom:4px;display:flex;align-items:center;gap:8px}}
.loop-step p{{font-size:.82rem;color:var(--muted);line-height:1.5}}
.loop-arrow{{display:none}}

/* QUICKSTART */
.qs-tabs{{display:flex;gap:0;border:1px solid var(--border);border-radius:var(--radius);overflow:hidden;margin-bottom:0}}
.qs-tab{{padding:8px 18px;font-size:.82rem;font-weight:600;border:none;background:#fff;cursor:pointer;color:var(--muted);border-bottom:2px solid transparent}}
.qs-tab.active{{background:var(--accent-light);color:var(--accent);border-bottom-color:var(--accent)}}
.qs-panel{{display:none;background:var(--code-bg);border:1px solid var(--border);border-top:none;border-radius:0 0 var(--radius) var(--radius);overflow-x:auto}}
.qs-panel.active{{display:block}}
.qs-panel pre{{padding:20px;margin:0}}
.qs-panel pre code{{background:none;font-size:.84rem;line-height:1.65}}

/* ARCH */
.arch-cols{{display:grid;grid-template-columns:1fr 1fr;gap:32px}}
@media(max-width:640px){{.arch-cols{{grid-template-columns:1fr}}}}
.arch-diagram{{background:var(--code-bg);border:1px solid var(--border);border-radius:var(--radius);padding:20px;font-family:'SF Mono',Menlo,Monaco,Consolas,monospace;font-size:.8rem;line-height:1.8;color:var(--fg);overflow-x:auto;white-space:pre}}

/* CRATES TABLE */
table{{width:100%;border-collapse:collapse;font-size:.88rem}}
th{{text-align:left;padding:10px 14px;background:#f9fafb;border-bottom:2px solid var(--border);font-weight:600;color:var(--fg)}}
td{{padding:10px 14px;border-bottom:1px solid var(--border);vertical-align:top;line-height:1.5}}
tr:last-child td{{border-bottom:none}}
.maturity{{display:inline-block;padding:2px 8px;border-radius:10px;font-size:.75rem;font-weight:600}}
.m-stable{{background:#d1fae5;color:#065f46}}
.m-exp{{background:#fef3c7;color:#92400e}}
.m-scaffold{{background:#f3f4f6;color:#6b7280}}
code{{background:var(--code-bg);padding:1px 5px;border-radius:4px;font-size:.83em;font-family:'SF Mono',Menlo,Monaco,Consolas,monospace}}

/* FOOTER */
footer{{background:#111827;color:#9ca3af;padding:40px 24px;font-size:.85rem}}
.footer-inner{{max-width:var(--max);margin:0 auto;display:flex;flex-wrap:wrap;gap:32px;justify-content:space-between}}
.footer-col h4{{color:#fff;font-size:.82rem;font-weight:700;text-transform:uppercase;letter-spacing:.07em;margin-bottom:12px}}
.footer-col a{{display:block;color:#9ca3af;text-decoration:none;margin-bottom:6px;transition:color .15s}}
.footer-col a:hover{{color:#fff}}
.footer-bottom{{max-width:var(--max);margin:28px auto 0;padding-top:20px;border-top:1px solid #374151;display:flex;justify-content:space-between;flex-wrap:wrap;gap:8px}}
</style>
</head>
<body>

<!-- NAV -->
<nav>
  <div class="nav-inner">
    <a class="nav-logo" href="#"><span>Oris</span></a>
    <div class="nav-links">
      <a href="#why">Why Oris</a>
      <a href="#loop">How It Works</a>
      <a href="#quickstart">Quick Start</a>
      <a href="#hub">Hub</a>
      <a href="#architecture">Architecture</a>
      <a href="#crates">Crates</a>
      <a href="https://github.com/Colin4k1024/Oris" target="_blank">GitHub</a>
      <a href="https://docs.rs/oris-runtime" target="_blank">docs.rs</a>
    </div>
  </div>
</nav>

<!-- HERO -->
<div class="hero">
  <div class="hero-inner">
    <div class="hero-badge">&#x1F9EC; AI Self-Evolution Framework</div>
    <h1>Software that learns<br>from <span>every execution</span></h1>
    <p class="hero-sub">
      Oris is an AI self-evolution framework for supervised, bounded, closed-loop software improvement.
      Capture signals. Generate mutations. Validate. Promote. Reuse.
    </p>
    <div class="hero-cta">
      <a class="btn btn-primary" href="#quickstart">&#x25B6; Get Started</a>
      <a class="btn btn-ghost" href="https://github.com/Colin4k1024/Oris" target="_blank">&#x2B50; GitHub</a>
      <a class="btn btn-ghost" href="https://docs.rs/oris-runtime" target="_blank">&#x1F4D6; API Docs</a>
    </div>
    <div class="badges">
      <a href="https://crates.io/crates/oris-runtime" target="_blank"><img src="https://img.shields.io/crates/v/oris-runtime.svg" alt="crates.io"></a>
      <a href="https://docs.rs/oris-runtime" target="_blank"><img src="https://img.shields.io/docsrs/oris-runtime" alt="docs.rs"></a>
      <a href="https://codecov.io/gh/Colin4k1024/Oris" target="_blank"><img src="https://codecov.io/gh/Colin4k1024/Oris/graph/badge.svg" alt="coverage"></a>
      <img src="https://img.shields.io/badge/version-{VERSION}-6366f1" alt="version">
    </div>
  </div>
</div>

<!-- WHY ORIS -->
<div class="section" id="why">
  <div class="section-inner">
    <div class="section-label">Why Oris</div>
    <h2>Systems that improve themselves</h2>
    <p class="section-desc">Most systems execute tasks but never learn from them. Oris closes the loop.</p>
    <div class="features">
      <div class="feature">
        <div class="feature-icon">&#x1F4E1;</div>
        <h3>Capture Real Signals</h3>
        <p>Collect actionable signals from compiler failures, test regressions, and runtime outcomes — not synthetic benchmarks.</p>
      </div>
      <div class="feature">
        <div class="feature-icon">&#x1F9EA;</div>
        <h3>Safe Mutation Sandbox</h3>
        <p>Generate candidate changes from successful patterns and execute them in OS-level isolated sandboxes before any promotion.</p>
      </div>
      <div class="feature">
        <div class="feature-icon">&#x2705;</div>
        <h3>Validate Before Promoting</h3>
        <p>Two-phase quality evaluation — static analysis gates block bad mutations before the LLM critic even runs.</p>
      </div>
      <div class="feature">
        <div class="feature-icon">&#x267B;&#xFE0F;</div>
        <h3>Confidence-Aware Reuse</h3>
        <p>Proven solutions are promoted into durable genes. Future runs replay them with tracked confidence, reducing reasoning over time.</p>
      </div>
      <div class="feature">
        <div class="feature-icon">&#x1F512;</div>
        <h3>Supervised &amp; Bounded</h3>
        <p>Fail-closed policy enforcement. No autonomous merge or release without explicit gate passage. Auditable at every step.</p>
      </div>
      <div class="feature">
        <div class="feature-icon">&#x1F310;</div>
        <h3>Cross-Node Sharing</h3>
        <p>Publish and fetch genes over the Oris Evolution Network. Ed25519-verified envelopes. Rate-limited PKI key registry.</p>
      </div>
    </div>
  </div>
</div>

<!-- EVOLUTION LOOP -->
<div class="section" id="loop">
  <div class="section-inner">
    <div class="section-label">How It Works</div>
    <h2>The 8-Stage Self-Evolution Loop</h2>
    <p class="section-desc">Every improvement follows a deterministic, auditable pipeline from signal to reusable asset.</p>
    <div class="loop-grid">
      <div class="loop-step"><span class="loop-num">01</span><h3>&#x1F50D; Detect</h3><p>Collect actionable signals from compiler diagnostics, test failures, and runtime panics.</p></div>
      <div class="loop-step"><span class="loop-num">02</span><h3>&#x1F3AF; Select</h3><p>Choose the best candidate gene or strategy from the pool using confidence scores.</p></div>
      <div class="loop-step"><span class="loop-num">03</span><h3>&#x1F9EC; Mutate</h3><p>Generate candidate changes derived from prior successful patterns and gene history.</p></div>
      <div class="loop-step"><span class="loop-num">04</span><h3>&#x1F4E6; Execute</h3><p>Run mutations inside a controlled sandbox with OS-level resource isolation.</p></div>
      <div class="loop-step"><span class="loop-num">05</span><h3>&#x2705; Validate</h3><p>Verify correctness through static analysis gates and configurable safety policies.</p></div>
      <div class="loop-step"><span class="loop-num">06</span><h3>&#x1F4CA; Evaluate</h3><p>Compare improvement vs regression with two-phase quality scoring (static + LLM).</p></div>
      <div class="loop-step"><span class="loop-num">07</span><h3>&#x1F4BE; Solidify</h3><p>Promote successful mutations into durable, reusable genes in the SQLite gene pool.</p></div>
      <div class="loop-step"><span class="loop-num">08</span><h3>&#x267B;&#xFE0F; Reuse</h3><p>Replay proven genes with confidence tracking — fewer LLM calls on each cycle.</p></div>
    </div>
    <div style="margin-top:24px;padding:16px 20px;background:#f5f3ff;border-left:4px solid #6366f1;border-radius:0 8px 8px 0;font-size:.88rem;color:#4338ca;max-width:640px">
      <strong>North Star:</strong> Task → Detect → Replay if trusted → Mutate only when needed → Validate → Capture → Reuse → <em>reduce reasoning over time</em>
    </div>
  </div>
</div>

<!-- QUICK START -->
<div class="section" id="quickstart">
  <div class="section-inner">
    <div class="section-label">Quick Start</div>
    <h2>Up and running in minutes</h2>
    <p class="section-desc">Add the crate, set your API key, and run the canonical evolution scenario.</p>

    <div style="max-width:720px">
      <div class="qs-tabs">
        <button class="qs-tab active" onclick="showTab('install')">1. Install</button>
        <button class="qs-tab" onclick="showTab('server')">2. Run Server</button>
        <button class="qs-tab" onclick="showTab('evolve')">3. First Evolution</button>
        <button class="qs-tab" onclick="showTab('job')">4. Submit Job</button>
      </div>

      <div id="tab-install" class="qs-panel active">
        <pre><code class="language-toml"># Cargo.toml
[dependencies]
oris-runtime = {{ version = "*", features = ["full-evolution-experimental"] }}</code></pre>
        <pre style="margin-top:0;border-top:1px solid #e5e7eb"><code class="language-bash"># Set your LLM key
export OPENAI_API_KEY="sk-..."

# Or use Anthropic
export ANTHROPIC_API_KEY="sk-ant-..."</code></pre>
      </div>

      <div id="tab-server" class="qs-panel">
        <pre><code class="language-bash"># Build and start the execution server (HTTP API)
cargo run -p oris-runtime \\
  --example execution_server \\
  --features "sqlite-persistence,execution-server"

# Server starts at http://127.0.0.1:8080
# Override with: export ORIS_SERVER_ADDR=0.0.0.0:8080</code></pre>
      </div>

      <div id="tab-evolve" class="qs-panel">
        <pre><code class="language-bash"># Run the canonical evolution scenario
cargo run -p evo_oris_repo

# Or run with observable artifacts
bash scripts/evo_first_run.sh

# Expected outputs:
#   target/evo_first_run/summary.json
#   target/evo_first_run/run.log</code></pre>
        <pre style="margin-top:0;border-top:1px solid #e5e7eb"><code class="language-bash"># Other example binaries
cargo run -p evo_oris_repo --bin intake_webhook_demo
cargo run -p evo_oris_repo --bin confidence_lifecycle_demo
cargo run -p evo_oris_repo --bin network_exchange</code></pre>
      </div>

      <div id="tab-job" class="qs-panel">
        <pre><code class="language-bash"># Submit a job to the execution server
curl -X POST http://127.0.0.1:8080/jobs \\
  -H "Content-Type: application/json" \\
  -d '{{"graph_name":"test_graph","input":{{"task":"example"}}}}'

# Monitor job status
curl http://127.0.0.1:8080/jobs/&lt;job_id&gt;

# Stream events in real-time
curl -N http://127.0.0.1:8080/jobs/&lt;job_id&gt;/stream</code></pre>
      </div>
    </div>
  </div>
</div>

<!-- HUB -->
<div class="section" id="hub">
  <div class="section-inner">
    <div class="section-label">Experience Hub</div>
    <h2>Federated gene sharing at scale</h2>
    <p class="section-desc">The Hub connects multiple Experience Repository nodes — register, discover, federate queries, and subscribe to cross-node evolution events.</p>
    <div class="features">
      <div class="feature">
        <div class="feature-icon">&#x1F4CB;</div>
        <h3>Node Registry</h3>
        <p>Register nodes with Ed25519 public keys. Key substitution prevention, health checking, and automatic deregistration on key conflict.</p>
      </div>
      <div class="feature">
        <div class="feature-icon">&#x1F50E;</div>
        <h3>Discovery</h3>
        <p>Query registered nodes by capability, tag, or health status. Find the right experience repository for your evolution domain.</p>
      </div>
      <div class="feature">
        <div class="feature-icon">&#x1F30D;</div>
        <h3>Federated Queries</h3>
        <p>Fan out gene searches across all healthy nodes. Aggregate results with deduplication — one API call, all nodes searched.</p>
      </div>
      <div class="feature">
        <div class="feature-icon">&#x1F514;</div>
        <h3>Event Subscriptions</h3>
        <p>Subscribe to gene promotion events across the network. Get notified when a new proven gene appears on any connected node.</p>
      </div>
    </div>

    <h3 style="font-size:.95rem;font-weight:700;margin-top:40px;margin-bottom:16px">Hub Quick Start</h3>
    <div style="max-width:720px">
      <div class="qs-tabs">
        <button class="qs-tab active" onclick="showHubTab('hub-start')">Start Hub</button>
        <button class="qs-tab" onclick="showHubTab('hub-register')">Register Node</button>
        <button class="qs-tab" onclick="showHubTab('hub-query')">Federated Query</button>
        <button class="qs-tab" onclick="showHubTab('hub-subscribe')">Subscribe</button>
      </div>

      <div id="tab-hub-start" class="qs-panel active">
        <pre><code class="language-bash"># Start the Hub server
cargo run -p oris-hub

# Or with configuration
export HUB_ADDR=0.0.0.0:3000
export HUB_CORS_ORIGINS=https://your-app.example.com
cargo run -p oris-hub

# Dashboard available at http://localhost:3000/dashboard</code></pre>
      </div>

      <div id="tab-hub-register" class="qs-panel">
        <pre><code class="language-bash"># Register a node (Ed25519 public key required)
curl -X POST http://localhost:3000/api/v1/nodes \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer &lt;hub-token&gt;" \
  -d '{{
    "node_id": "node-alpha",
    "endpoint": "https://alpha.example.com",
    "public_key": "&lt;base64-ed25519-pubkey&gt;",
    "capabilities": ["gene-store", "capsule-store"]
  }}'

# List registered nodes
curl http://localhost:3000/api/v1/nodes</code></pre>
      </div>

      <div id="tab-hub-query" class="qs-panel">
        <pre><code class="language-bash"># Federated gene search across all nodes
curl "http://localhost:3000/api/v1/federation/genes?q=fix_timeout"

# Response aggregates results from all healthy nodes:
# {{ "results": [...], "nodes_queried": 3, "nodes_healthy": 3 }}

# Search by task class
curl "http://localhost:3000/api/v1/federation/genes?task_class=network_retry"</code></pre>
      </div>

      <div id="tab-hub-subscribe" class="qs-panel">
        <pre><code class="language-bash"># Subscribe to gene promotion events
curl -X POST http://localhost:3000/api/v1/subscriptions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer &lt;hub-token&gt;" \
  -d '{{
    "callback_url": "https://my-node.example.com/hooks/gene",
    "events": ["gene_promoted", "gene_revoked"],
    "filter": {{ "min_confidence": 0.8 }}
  }}'

# Hub pushes events to your callback URL when genes
# are promoted or revoked on any registered node</code></pre>
      </div>
    </div>
  </div>
</div>

<!-- ARCHITECTURE -->
<div class="section" id="architecture">
  <div class="section-inner">
    <div class="section-label">Architecture</div>
    <h2>Clean layered design</h2>
    <p class="section-desc">Oris is organized as a Cargo workspace of 23 library crates with a strict dependency DAG — no circular dependencies.</p>
    <div class="arch-cols">
      <div>
        <h3 style="font-size:.95rem;font-weight:700;margin-bottom:12px">Dependency Layers</h3>
        <div class="arch-diagram">Leaf (no workspace deps)
  oris-agent-contract · oris-economics
  oris-genestore · oris-kernel
  oris-mutation-evaluator

Layer 1 — builds on leaf crates
  oris-evolution  (depends on oris-kernel)
  oris-evokernel  (11 deps, highest fan-in)
  oris-governor   oris-intake
  oris-orchestrator

Layer 2 — network &amp; network-aware
  oris-evolution-network
  oris-experience-repo

Layer 3 — hub &amp; runtime facade
  oris-hub            (federation layer)
  oris-runtime        (re-exports evokernel)
  oris-execution-server</div>
      </div>
      <div>
        <h3 style="font-size:.95rem;font-weight:700;margin-bottom:12px">Key Abstractions</h3>
        <div style="display:flex;flex-direction:column;gap:10px;font-size:.85rem">
          <div style="padding:12px;background:#fff;border:1px solid var(--border);border-radius:6px"><strong>EvolutionPipeline</strong><br><span style="color:var(--muted)">8-stage detect→reuse orchestration</span></div>
          <div style="padding:12px;background:#fff;border:1px solid var(--border);border-radius:6px"><strong>Gene &amp; Capsule</strong><br><span style="color:var(--muted)">Durable evolution assets with confidence scores</span></div>
          <div style="padding:12px;background:#fff;border:1px solid var(--border);border-radius:6px"><strong>Kernel</strong><br><span style="color:var(--muted)">Deterministic execution: event log, replay, snapshot</span></div>
          <div style="padding:12px;background:#fff;border:1px solid var(--border);border-radius:6px"><strong>MutationEvaluator</strong><br><span style="color:var(--muted)">Two-phase quality gate (static + LLM critic)</span></div>
          <div style="padding:12px;background:#fff;border:1px solid var(--border);border-radius:6px"><strong>PluginRegistry</strong><br><span style="color:var(--muted)">9 categories, determinism contracts, version negotiation</span></div>
          <div style="padding:12px;background:#fff;border:1px solid var(--border);border-radius:6px"><strong>Hub</strong><br><span style="color:var(--muted)">Node registry, federated queries, event subscriptions</span></div>
        </div>
      </div>
    </div>
  </div>
</div>

<!-- CRATES -->
<div class="section" id="crates">
  <div class="section-inner">
    <div class="section-label">Crates</div>
    <h2>Component overview</h2>
    <p class="section-desc">Modular crates with feature flags — use only what you need.</p>
    <table>
      <thead>
        <tr><th>Crate</th><th>Purpose</th><th>Maturity</th><th>Feature Flag</th></tr>
      </thead>
      <tbody>
        <tr><td><code>oris-runtime</code></td><td>Main facade: agents, graphs, tools, RAG, multi-step execution</td><td><span class="maturity m-stable">stable</span></td><td>—</td></tr>
        <tr><td><code>oris-evolution</code></td><td>Core types: Gene, Capsule, EvolutionEvent, Pipeline, Confidence</td><td><span class="maturity m-stable">stable</span></td><td><code>evolution-experimental</code></td></tr>
        <tr><td><code>oris-evokernel</code></td><td>Self-evolving kernel orchestration (highest fan-in, 11 deps)</td><td><span class="maturity m-stable">stable</span></td><td><code>full-evolution-experimental</code></td></tr>
        <tr><td><code>oris-kernel</code></td><td>Deterministic execution: event log, replay, snapshot, K1–K5</td><td><span class="maturity m-stable">stable</span></td><td>—</td></tr>
        <tr><td><code>oris-sandbox</code></td><td>OS-level isolated mutation execution</td><td><span class="maturity m-stable">stable</span></td><td><code>evolution-experimental</code></td></tr>
        <tr><td><code>oris-mutation-evaluator</code></td><td>Two-phase quality evaluator (static analysis + LLM critic)</td><td><span class="maturity m-stable">stable</span></td><td><code>evolution-experimental</code></td></tr>
        <tr><td><code>oris-genestore</code></td><td>SQLite-based Gene and Capsule storage</td><td><span class="maturity m-stable">stable</span></td><td>—</td></tr>
        <tr><td><code>oris-governor</code></td><td>Promotion, cooldown, and revocation policies</td><td><span class="maturity m-stable">stable</span></td><td><code>governor-experimental</code></td></tr>
        <tr><td><code>oris-intake</code></td><td>Issue intake, deduplication, prioritization, CI failure parsing</td><td><span class="maturity m-stable">stable</span></td><td>—</td></tr>
        <tr><td><code>oris-evolution-network</code></td><td>OEN envelope, gossip sync, Ed25519 signing</td><td><span class="maturity m-exp">experimental</span></td><td><code>evolution-network-experimental</code></td></tr>
        <tr><td><code>oris-experience-repo</code></td><td>HTTP API: gene/capsule sharing, Ed25519 PKI, rate limiting</td><td><span class="maturity m-stable">v{VERSION}</span></td><td>standalone</td></tr>
        <tr><td><code>oris-hub</code></td><td>Experience Hub: node registry, discovery, federation, subscriptions, dashboard</td><td><span class="maturity m-stable">v0.1.0</span></td><td>standalone</td></tr>
        <tr><td><code>oris-hub-client</code></td><td>Typed Rust client for the Hub API</td><td><span class="maturity m-stable">v0.1.0</span></td><td>standalone</td></tr>
        <tr><td><code>oris-orchestrator</code></td><td>Autonomous loop, GitHub delivery, release automation</td><td><span class="maturity m-exp">experimental</span></td><td><code>release-automation-experimental</code></td></tr>
        <tr><td><code>oris-spec</code></td><td>OUSL YAML spec contracts and compilers</td><td><span class="maturity m-exp">experimental</span></td><td><code>spec-experimental</code></td></tr>
      </tbody>
    </table>
  </div>
</div>

<!-- WHAT YOU CAN BUILD -->
<div class="section" id="use-cases">
  <div class="section-inner">
    <div class="section-label">Use Cases</div>
    <h2>What you can build</h2>
    <p class="section-desc">Oris is a framework, not a product. You bring the domain; Oris handles the evolution infrastructure.</p>
    <div class="features">
      <div class="feature">
        <div class="feature-icon">&#x1F916;</div>
        <h3>Self-Improving AI Agents</h3>
        <p>Agents that learn from failed runs and promote successful strategies into reusable genes — without human intervention per cycle.</p>
      </div>
      <div class="feature">
        <div class="feature-icon">&#x1F527;</div>
        <h3>Supervised Dev Loops</h3>
        <p>Bounded, auditable repair loops for recurring issues. The governor enforces cooldowns and prevents runaway mutation.</p>
      </div>
      <div class="feature">
        <div class="feature-icon">&#x1F504;</div>
        <h3>Replay Pipelines</h3>
        <p>Confidence-aware replay: use a proven gene instead of re-reasoning from scratch. Confidence degrades gracefully over time.</p>
      </div>
      <div class="feature">
        <div class="feature-icon">&#x1F4E1;</div>
        <h3>Cross-Agent Knowledge Exchange</h3>
        <p>Publish promoted genes to the Evolution Network. Other nodes fetch and replay them to accelerate their own local evolution.</p>
      </div>
    </div>
  </div>
</div>

<!-- FOOTER -->
<footer>
  <div class="footer-inner">
    <div class="footer-col">
      <h4>Oris</h4>
      <a href="https://github.com/Colin4k1024/Oris" target="_blank">GitHub Repository</a>
      <a href="https://crates.io/crates/oris-runtime" target="_blank">crates.io</a>
      <a href="https://docs.rs/oris-runtime" target="_blank">docs.rs (API)</a>
      <a href="https://codecov.io/gh/Colin4k1024/Oris" target="_blank">Code Coverage</a>
    </div>
    <div class="footer-col">
      <h4>Documentation</h4>
      <a href="https://github.com/Colin4k1024/Oris/blob/main/docs/ARCHITECTURE.md" target="_blank">Architecture</a>
      <a href="https://github.com/Colin4k1024/Oris/blob/main/docs/kernel-api.md" target="_blank">Kernel API</a>
      <a href="https://github.com/Colin4k1024/Oris/blob/main/docs/plugin-authoring.md" target="_blank">Plugin Authoring</a>
      <a href="https://github.com/Colin4k1024/Oris/blob/main/docs/production-operations-guide.md" target="_blank">Operations Guide</a>
    </div>
    <div class="footer-col">
      <h4>Community</h4>
      <a href="https://github.com/Colin4k1024/Oris/blob/main/CONTRIBUTING.md" target="_blank">Contributing</a>
      <a href="https://github.com/Colin4k1024/Oris/blob/main/CODE_OF_CONDUCT.md" target="_blank">Code of Conduct</a>
      <a href="https://github.com/Colin4k1024/Oris/blob/main/SECURITY.md" target="_blank">Security Policy</a>
      <a href="https://github.com/Colin4k1024/Oris/blob/main/LICENSE" target="_blank">MIT License</a>
    </div>
    <div class="footer-col">
      <h4>Examples</h4>
      <a href="https://github.com/Colin4k1024/Oris/tree/main/examples/evo_oris_repo" target="_blank">Evolution Scenario</a>
      <a href="https://github.com/Colin4k1024/Oris/tree/main/examples/oris_starter_axum" target="_blank">Axum Starter</a>
      <a href="https://github.com/Colin4k1024/Oris/tree/main/examples/oris_operator_cli" target="_blank">Operator CLI</a>
      <a href="https://github.com/Colin4k1024/Oris/blob/main/docs/evokernel/examples.md" target="_blank">More Examples</a>
    </div>
  </div>
  <div class="footer-bottom">
    <span>&#169; 2026 Oris Contributors &mdash; MIT License</span>
    <span>oris-runtime v0.61.0 &middot; oris-hub v0.1.0</span>
  </div>
</footer>

<script>
function showTab(id) {{
  var parent = event.target.parentElement;
  parent.querySelectorAll('.qs-tab').forEach(function(t){{ t.classList.remove('active'); }});
  var panels = parent.parentElement.querySelectorAll('.qs-panel');
  panels.forEach(function(p){{ p.classList.remove('active'); }});
  event.target.classList.add('active');
  document.getElementById('tab-' + id).classList.add('active');
}}
function showHubTab(id) {{
  var parent = event.target.parentElement;
  parent.querySelectorAll('.qs-tab').forEach(function(t){{ t.classList.remove('active'); }});
  var panels = parent.parentElement.querySelectorAll('.qs-panel');
  panels.forEach(function(p){{ p.classList.remove('active'); }});
  event.target.classList.add('active');
  document.getElementById('tab-' + id).classList.add('active');
}}
</script>
</body>
</html>"""

os.makedirs("_site", exist_ok=True)
with open("_site/index.html", "w", encoding="utf-8") as f:
    f.write(PAGE)

print(f"Generated _site/index.html  ({len(PAGE):,} bytes)")
