#!/usr/bin/env python3
"""Generate _site/index.html — a single-page documentation portal for Oris."""
import os
import re
import html as html_mod

DOCS_DIR = "_site/docs"
VERSION = os.environ.get("VERSION", "?")


# ── helpers ────────────────────────────────────────────────────────────────


def h1_from_file(path):
    try:
        with open(path, encoding="utf-8") as f:
            for line in f:
                s = line.strip()
                if s.startswith("# "):
                    return s[2:].strip()
    except Exception:
        pass
    return None


def name_from_path(rel_path):
    basename = os.path.basename(rel_path)
    name = basename.replace(".md", "")
    # strip date prefix like 2026-03-05-
    name = re.sub(r"^\d{4}-\d{2}-\d{2}-", "", name)
    name = name.replace("-", " ").replace("_", " ")
    LOWER = {"a", "an", "the", "and", "or", "in", "of", "to", "for", "with", "on", "at", "from", "by", "as"}
    words = name.split()
    return " ".join(w if (i > 0 and w.lower() in LOWER) else w.capitalize() for i, w in enumerate(words))


def best_name(rel_path):
    full = os.path.join(DOCS_DIR, rel_path)
    return h1_from_file(full) or name_from_path(rel_path)


# ── nav collectors ─────────────────────────────────────────────────────────


def collect_dir(subdir):
    base = os.path.join(DOCS_DIR, subdir)
    if not os.path.isdir(base):
        return []
    files = [os.path.join(subdir, f) for f in os.listdir(base) if f.endswith(".md")]
    files.sort(key=lambda p: os.path.basename(p).lower())
    return files


def collect_dir_recursive(subdir):
    base = os.path.join(DOCS_DIR, subdir)
    if not os.path.isdir(base):
        return []
    result = []
    # loose files at top level
    loose = [
        os.path.join(subdir, f)
        for f in sorted(os.listdir(base))
        if f.endswith(".md") and os.path.isfile(os.path.join(base, f))
    ]
    result.extend(loose)
    # subdirectories as sub-groups
    for d in sorted(os.listdir(base)):
        full_sub = os.path.join(base, d)
        if os.path.isdir(full_sub):
            sub_files = [
                os.path.join(subdir, d, f)
                for f in sorted(os.listdir(full_sub))
                if f.endswith(".md")
            ]
            if sub_files:
                label = re.sub(r"^\d{4}-\d{2}-\d{2}-?", "", d).replace("-", " ").capitalize() or d
                result.append({"label": label, "items": sub_files})
    return result


ROOT_ORDER = [
    "_README.md",
    "ARCHITECTURE.md",
    "ORIS_2.0_STRATEGY.md",
    "PROJECT_AUDIT_2026_Q1.md",
    "oris-v1-os-architecture.md",
    "open-source-onboarding-zh.md",
    "evolution.md",
    "evolution-boundary.md",
    "durable-execution.md",
    "evokernel-v0.1.md",
    "evolution-network-protocol.md",
    "interrupt-resume-invariants.md",
    "replay-lifecycle-invariants.md",
    "kernel-api.md",
    "plugin-authoring.md",
    "mcp-bootstrap.md",
    "rust-ecosystem-integration.md",
    "production-operations-guide.md",
    "incident-response-runbook.md",
    "runtime-schema-migrations.md",
    "postgres-backup-restore-runbook.md",
    "runtime-benchmark-policy.md",
    "scheduler-stress-baseline.md",
    "supply-chain-policy.md",
    "v100-operator-quickstart.md",
    "v070-milestone-proof-artifacts.md",
    "v100-release-proof-artifacts.md",
    "v100-bounded-autonomous-intake-baseline.md",
    "v100-confidence-lifecycle-baseline.md",
    "v100-governed-evolution-baseline.md",
    "v100-proposal-to-pr-baseline.md",
    "v100-reliability-gate-baseline.md",
    "v100-runtime-hardening-baseline.md",
    "evomap-vs-oris-comparison.md",
    "evomap-gap-unified-alignment.md",
    "evomap-test-cases.md",
    "evomap-semantic-alignment-issue-index-2026-03-09.md",
]


def collect_root():
    existing = set(os.listdir(DOCS_DIR))
    result = []
    seen = set()
    for f in ROOT_ORDER:
        if f in existing and os.path.isfile(os.path.join(DOCS_DIR, f)):
            result.append(f)
            seen.add(f)
    for f in sorted(existing):
        if f.endswith(".md") and f not in seen and os.path.isfile(os.path.join(DOCS_DIR, f)):
            result.append(f)
    return result


SECTIONS = [
    ("📖", "Overview",         collect_root()),
    ("🧬", "EvoKernel",        collect_dir("evokernel")),
    ("🔬", "Evolution",        collect_dir("evolution")),
    ("📋", "Plans",            collect_dir("plans")),
    ("🚀", "Sprint Artifacts", collect_dir_recursive("artifacts")),
    ("💾", "Memory",           collect_dir_recursive("memory")),
    ("📊", "Observability",    collect_dir("observability")),
]


# ── render nav HTML ────────────────────────────────────────────────────────


def render_items(items, depth=0):
    out = []
    for item in items:
        if isinstance(item, dict):
            label = html_mod.escape(item["label"])
            out.append(f'<div class="nav-sub-label">{label}</div>')
            out.extend(render_items(item["items"], depth + 1))
        else:
            name = html_mod.escape(best_name(item))
            epath = html_mod.escape(item)
            jpath = item.replace("'", "\\'")
            cls = ' class="indent"' if depth > 0 else ""
            out.append(
                f'<a{cls} href="#" data-path="{epath}" '
                f"onclick=\"loadDoc('{jpath}');return false;\">{name}</a>"
            )
    return out


nav_parts = []
for icon, label, items in SECTIONS:
    if not items:
        continue
    nav_parts.append(f'<div class="nav-section">{icon} {html_mod.escape(label)}</div>')
    nav_parts.extend(render_items(items))

NAV_HTML = "\n".join(nav_parts)

# first doc to pre-load
FIRST_DOC = "_README.md" if os.path.isfile(os.path.join(DOCS_DIR, "_README.md")) else "ARCHITECTURE.md"

# ── HTML template ──────────────────────────────────────────────────────────

PAGE = (
    """<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Oris — Documentation</title>
<link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/highlight.js@11.9.0/styles/github.min.css">
<script src="https://cdn.jsdelivr.net/npm/marked@12.0.0/marked.min.js"></script>
<script src="https://cdn.jsdelivr.net/npm/highlight.js@11.9.0/lib/highlight.min.js"></script>
<style>
*,*::before,*::after{box-sizing:border-box;margin:0;padding:0}
html,body{height:100%;font-family:system-ui,-apple-system,sans-serif;font-size:15px;color:#1a1a1a;background:#fff}
#layout{display:flex;height:100vh;overflow:hidden}
/* sidebar */
#sidebar{width:270px;min-width:270px;border-right:1px solid #e5e7eb;display:flex;flex-direction:column;background:#fafafa}
#sidebar-header{padding:16px 16px 12px;border-bottom:1px solid #e5e7eb}
#sidebar-header h1{font-size:1rem;font-weight:700;letter-spacing:-.01em}
#sidebar-header .meta{font-size:.75rem;color:#6b7280;margin-top:2px}
#sidebar-search{padding:8px 12px;border-bottom:1px solid #e5e7eb}
#sidebar-search input{width:100%;padding:5px 8px;font-size:.8rem;border:1px solid #d1d5db;border-radius:5px;outline:none;background:#fff}
#sidebar-search input:focus{border-color:#6366f1}
#nav{flex:1;overflow-y:auto;padding:8px 0 24px}
.nav-section{padding:10px 16px 3px;font-size:.7rem;font-weight:700;text-transform:uppercase;letter-spacing:.07em;color:#9ca3af}
#nav a{display:block;padding:4px 16px;font-size:.83rem;color:#374151;text-decoration:none;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;border-left:2px solid transparent}
#nav a:hover{background:#f0f0f0;color:#111}
#nav a.active{background:#ede9fe;color:#4f46e5;border-left-color:#4f46e5;font-weight:600}
#nav a.indent{padding-left:28px;font-size:.78rem;color:#6b7280}
#nav a.indent:hover{color:#111}
#nav a.indent.active{color:#4f46e5}
.nav-sub-label{padding:6px 28px 2px;font-size:.72rem;font-weight:600;color:#9ca3af;text-transform:uppercase;letter-spacing:.05em}
/* content */
#content-wrap{flex:1;display:flex;flex-direction:column;overflow:hidden}
#topbar{padding:10px 24px;border-bottom:1px solid #e5e7eb;display:flex;align-items:center;gap:12px;background:#fff;font-size:.8rem;color:#6b7280;flex-shrink:0}
#breadcrumb{flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.badge{background:#d1fae5;color:#065f46;padding:1px 8px;border-radius:10px;font-size:.72rem;font-weight:600;flex-shrink:0}
#content{flex:1;overflow-y:auto;padding:32px 48px 64px;max-width:960px}
#content h1{font-size:1.6rem;font-weight:700;margin-bottom:.5rem;padding-bottom:.4rem;border-bottom:1px solid #e5e7eb}
#content h2{font-size:1.2rem;font-weight:600;margin:1.8rem 0 .5rem;padding-bottom:.3rem;border-bottom:1px solid #f3f4f6}
#content h3{font-size:1rem;font-weight:600;margin:1.4rem 0 .4rem}
#content h4{font-size:.9rem;font-weight:600;margin:1.2rem 0 .3rem}
#content p{line-height:1.7;margin:.8rem 0;color:#374151}
#content ul,#content ol{padding-left:1.5rem;margin:.6rem 0;line-height:1.7}
#content li{margin:.2rem 0;color:#374151}
#content a{color:#4f46e5;text-decoration:none}
#content a:hover{text-decoration:underline}
#content code{background:#f3f4f6;padding:1px 5px;border-radius:4px;font-size:.85em;font-family:'SF Mono',Menlo,Monaco,Consolas,monospace}
#content pre{background:#f6f8fa;border:1px solid #e5e7eb;border-radius:8px;padding:16px;overflow-x:auto;margin:1rem 0}
#content pre code{background:none;padding:0;font-size:.82rem;line-height:1.6}
#content table{width:100%;border-collapse:collapse;margin:1rem 0;font-size:.88rem}
#content th{text-align:left;padding:8px 12px;background:#f9fafb;border:1px solid #e5e7eb;font-weight:600}
#content td{padding:8px 12px;border:1px solid #e5e7eb;vertical-align:top;line-height:1.5}
#content tr:nth-child(even) td{background:#fafafa}
#content blockquote{border-left:4px solid #6366f1;background:#f5f3ff;margin:1rem 0;padding:10px 16px;border-radius:0 6px 6px 0}
#content blockquote p{color:#4338ca;margin:0}
#content hr{border:none;border-top:1px solid #e5e7eb;margin:1.5rem 0}
#content img{max-width:100%;border-radius:6px}
</style>
</head>
<body>
<div id="layout">
  <div id="sidebar">
    <div id="sidebar-header">
      <h1>&#x1F4DA; Oris Docs</h1>
      <div class="meta">"""
    + f"v{VERSION}"
    + """ &nbsp;&middot;&nbsp; <a href="https://github.com/Colin4k1024/Oris" target="_blank" style="color:#6366f1;text-decoration:none">GitHub &#x2197;</a></div>
    </div>
    <div id="sidebar-search">
      <input id="search-input" type="text" placeholder="Filter docs&hellip;" oninput="filterNav(this.value)">
    </div>
    <div id="nav">
"""
    + NAV_HTML
    + """
    </div>
  </div>
  <div id="content-wrap">
    <div id="topbar">
      <span id="breadcrumb">Select a document from the sidebar</span>
      <span class="badge">OK</span>
      <a href="https://crates.io/crates/oris-experience-repo" target="_blank" style="color:#6366f1;text-decoration:none;font-size:.75rem">crates.io &#x2197;</a>
      <a href="https://docs.rs/oris-experience-repo" target="_blank" style="color:#6366f1;text-decoration:none;font-size:.75rem">docs.rs &#x2197;</a>
    </div>
    <div id="content"></div>
  </div>
</div>
<script>
marked.setOptions({
  gfm: true,
  breaks: false
});
const renderer = new marked.Renderer();
const origLink = renderer.link.bind(renderer);
renderer.link = function(href, title, text) {
  const out = origLink(href, title, text);
  if (href && (href.startsWith('http') || href.startsWith('//'))) {
    return out.replace('<a ', '<a target="_blank" ');
  }
  return out;
};
marked.use({ renderer });

function loadDoc(path) {
  document.querySelectorAll('#nav a').forEach(function(a) { a.classList.remove('active'); });
  const link = document.querySelector('#nav a[data-path="' + path + '"]');
  if (link) { link.classList.add('active'); link.scrollIntoView({block:'nearest'}); }
  const crumb = path.replace('_README.md','README.md').replace(/^.*\\//, '').replace('.md','');
  document.getElementById('breadcrumb').textContent = crumb;
  const content = document.getElementById('content');
  content.innerHTML = '<p style="color:#9ca3af;padding:32px 0">Loading…</p>';
  fetch('docs/' + path)
    .then(function(r) { if (!r.ok) throw new Error(r.status); return r.text(); })
    .then(function(md) {
      content.innerHTML = marked.parse(md);
      content.querySelectorAll('pre code').forEach(function(el) { hljs.highlightElement(el); });
      content.scrollTop = 0;
    })
    .catch(function(e) {
      content.innerHTML = '<p style="color:#ef4444">Failed to load (' + e.message + ')</p>';
    });
}

function filterNav(q) {
  const term = q.toLowerCase();
  let lastSection = null;
  let lastSectionVisible = false;
  document.querySelectorAll('#nav > *').forEach(function(el) {
    if (el.classList.contains('nav-section')) {
      if (lastSection) lastSection.style.display = lastSectionVisible ? '' : 'none';
      lastSection = el;
      lastSectionVisible = false;
    } else if (el.tagName === 'A') {
      const show = !term || el.textContent.toLowerCase().includes(term);
      el.style.display = show ? '' : 'none';
      if (show) lastSectionVisible = true;
    } else {
      el.style.display = '';
      lastSectionVisible = true;
    }
  });
  if (lastSection) lastSection.style.display = lastSectionVisible ? '' : 'none';
}
"""
    + f"loadDoc('{FIRST_DOC}');\n"
    + """</script>
</body>
</html>"""
)

os.makedirs("_site", exist_ok=True)
with open("_site/index.html", "w", encoding="utf-8") as f:
    f.write(PAGE)

total = sum(
    sum(len(i["items"]) if isinstance(i, dict) else 1 for i in items)
    for _, _, items in SECTIONS
    if items
)
print(f"Generated _site/index.html  ({len(PAGE):,} bytes,  {total} nav entries)")
