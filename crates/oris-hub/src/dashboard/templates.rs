use crate::registry::{NodeInfo, NodeStatus};
use crate::subscription::Subscription;

const TAILWIND_CDN: &str = "https://cdn.tailwindcss.com";

/// Escape user-supplied strings for safe HTML interpolation (prevents XSS).
fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

fn layout(title: &str, content: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{title} — Oris Hub</title>
<script src="{TAILWIND_CDN}"></script>
</head>
<body class="bg-gray-50 text-gray-900 min-h-screen">
<nav class="bg-white border-b border-gray-200 px-6 py-4">
  <div class="max-w-7xl mx-auto flex items-center justify-between">
    <a href="/dashboard" class="text-xl font-bold text-indigo-600">Oris Hub</a>
    <div class="flex gap-6 text-sm font-medium text-gray-600">
      <a href="/dashboard" class="hover:text-indigo-600">Overview</a>
      <a href="/dashboard/nodes" class="hover:text-indigo-600">Nodes</a>
      <a href="/dashboard/subscriptions" class="hover:text-indigo-600">Subscriptions</a>
    </div>
  </div>
</nav>
<main class="max-w-7xl mx-auto px-6 py-8">
{content}
</main>
<footer class="text-center text-xs text-gray-400 py-4">Oris Experience Repository Hub</footer>
</body>
</html>"#
    )
}

pub fn overview(
    active_nodes: usize,
    total_subscriptions: usize,
    capabilities: &[String],
) -> String {
    let caps_html: String = capabilities
        .iter()
        .map(|c| {
            let escaped = escape_html(c);
            format!(r#"<span class="inline-block bg-indigo-100 text-indigo-700 px-2 py-1 rounded text-xs mr-1 mb-1">{escaped}</span>"#)
        })
        .collect();

    let content = format!(
        r#"<h1 class="text-2xl font-bold mb-6">Dashboard Overview</h1>
<div class="grid grid-cols-1 md:grid-cols-3 gap-6 mb-8">
  <div class="bg-white rounded-lg shadow p-6">
    <div class="text-sm text-gray-500">Active Nodes</div>
    <div class="text-3xl font-bold text-indigo-600">{active_nodes}</div>
  </div>
  <div class="bg-white rounded-lg shadow p-6">
    <div class="text-sm text-gray-500">Subscriptions</div>
    <div class="text-3xl font-bold text-green-600">{total_subscriptions}</div>
  </div>
  <div class="bg-white rounded-lg shadow p-6">
    <div class="text-sm text-gray-500">Capabilities</div>
    <div class="text-3xl font-bold text-amber-600">{cap_count}</div>
  </div>
</div>
<div class="bg-white rounded-lg shadow p-6">
  <h2 class="text-lg font-semibold mb-3">Network Capabilities</h2>
  <div>{caps_html}</div>
</div>"#,
        cap_count = capabilities.len(),
    );

    layout("Overview", &content)
}

pub fn nodes_page(nodes: &[NodeInfo]) -> String {
    let rows: String = nodes
        .iter()
        .map(|n| {
            let caps = escape_html(&n.capabilities.join(", "));
            let region = escape_html(n.region.as_deref().unwrap_or("—"));
            let last_seen =
                escape_html(&n.last_heartbeat.format("%Y-%m-%d %H:%M:%S UTC").to_string());
            format!(
                r#"<tr class="border-b border-gray-100 hover:bg-gray-50">
  <td class="px-4 py-3 font-medium">{node_id}</td>
  <td class="px-4 py-3 text-sm text-gray-600">{endpoint}</td>
  <td class="px-4 py-3 text-sm">{caps}</td>
  <td class="px-4 py-3 text-sm">{region}</td>
  <td class="px-4 py-3 text-sm">{version}</td>
  <td class="px-4 py-3 text-sm text-gray-500">{last_seen}</td>
</tr>"#,
                node_id = escape_html(&n.node_id),
                endpoint = escape_html(&n.endpoint),
                version = escape_html(&n.version),
            )
        })
        .collect();

    let content = format!(
        r#"<h1 class="text-2xl font-bold mb-6">Active Nodes</h1>
<div class="bg-white rounded-lg shadow overflow-hidden">
<table class="w-full text-left">
<thead class="bg-gray-50 border-b">
  <tr>
    <th class="px-4 py-3 text-xs font-semibold text-gray-500 uppercase">Node ID</th>
    <th class="px-4 py-3 text-xs font-semibold text-gray-500 uppercase">Endpoint</th>
    <th class="px-4 py-3 text-xs font-semibold text-gray-500 uppercase">Capabilities</th>
    <th class="px-4 py-3 text-xs font-semibold text-gray-500 uppercase">Region</th>
    <th class="px-4 py-3 text-xs font-semibold text-gray-500 uppercase">Version</th>
    <th class="px-4 py-3 text-xs font-semibold text-gray-500 uppercase">Last Seen</th>
  </tr>
</thead>
<tbody>
{rows}
</tbody>
</table>
{empty_state}
</div>"#,
        empty_state = if nodes.is_empty() {
            r#"<div class="text-center text-gray-400 py-8">No active nodes registered</div>"#
        } else {
            ""
        }
    );

    layout("Nodes", &content)
}

pub fn subscriptions_page(subs: &[Subscription]) -> String {
    let rows: String = subs
        .iter()
        .map(|s| {
            let task_class = escape_html(s.filter.task_class.as_deref().unwrap_or("*"));
            let min_conf = escape_html(
                &s.filter
                    .min_confidence
                    .map(|c| format!("{c:.2}"))
                    .unwrap_or_else(|| "—".to_string()),
            );
            let source_nodes = escape_html(
                &s.filter
                    .source_nodes
                    .as_ref()
                    .map(|v| v.join(", "))
                    .unwrap_or_else(|| "—".to_string()),
            );

            format!(
                r#"<tr class="border-b border-gray-100 hover:bg-gray-50">
  <td class="px-4 py-3 font-mono text-xs">{id}</td>
  <td class="px-4 py-3 text-sm">{subscriber}</td>
  <td class="px-4 py-3 text-sm text-gray-600">{callback}</td>
  <td class="px-4 py-3 text-sm">{task_class}</td>
  <td class="px-4 py-3 text-sm">{min_conf}</td>
  <td class="px-4 py-3 text-sm text-gray-500">{source_nodes}</td>
</tr>"#,
                id = escape_html(&s.id[..8]),
                subscriber = escape_html(&s.subscriber_node_id),
                callback = escape_html(&s.callback_url),
            )
        })
        .collect();

    let content = format!(
        r#"<h1 class="text-2xl font-bold mb-6">Subscriptions</h1>
<div class="bg-white rounded-lg shadow overflow-hidden">
<table class="w-full text-left">
<thead class="bg-gray-50 border-b">
  <tr>
    <th class="px-4 py-3 text-xs font-semibold text-gray-500 uppercase">ID</th>
    <th class="px-4 py-3 text-xs font-semibold text-gray-500 uppercase">Subscriber</th>
    <th class="px-4 py-3 text-xs font-semibold text-gray-500 uppercase">Callback</th>
    <th class="px-4 py-3 text-xs font-semibold text-gray-500 uppercase">Task Class</th>
    <th class="px-4 py-3 text-xs font-semibold text-gray-500 uppercase">Min Confidence</th>
    <th class="px-4 py-3 text-xs font-semibold text-gray-500 uppercase">Source Nodes</th>
  </tr>
</thead>
<tbody>
{rows}
</tbody>
</table>
{empty_state}
</div>"#,
        empty_state = if subs.is_empty() {
            r#"<div class="text-center text-gray-400 py-8">No active subscriptions</div>"#
        } else {
            ""
        }
    );

    layout("Subscriptions", &content)
}

pub fn node_detail(node: &NodeInfo) -> String {
    let status_badge = match node.status {
        NodeStatus::Active => {
            r#"<span class="bg-green-100 text-green-700 px-2 py-1 rounded text-xs font-medium">Active</span>"#
        }
        NodeStatus::Degraded => {
            r#"<span class="bg-yellow-100 text-yellow-700 px-2 py-1 rounded text-xs font-medium">Degraded</span>"#
        }
        NodeStatus::Offline => {
            r#"<span class="bg-red-100 text-red-700 px-2 py-1 rounded text-xs font-medium">Offline</span>"#
        }
    };

    let caps_html: String = node
        .capabilities
        .iter()
        .map(|c| {
            let escaped = escape_html(c);
            format!(r#"<span class="inline-block bg-indigo-100 text-indigo-700 px-2 py-1 rounded text-xs mr-1 mb-1">{escaped}</span>"#)
        })
        .collect();

    let region = escape_html(node.region.as_deref().unwrap_or("—"));

    let content = format!(
        r#"<div class="mb-4"><a href="/dashboard/nodes" class="text-indigo-600 text-sm hover:underline">&larr; Back to Nodes</a></div>
<div class="bg-white rounded-lg shadow p-6">
  <div class="flex items-center gap-4 mb-6">
    <h1 class="text-2xl font-bold">{node_id}</h1>
    {status_badge}
  </div>
  <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
    <div>
      <div class="text-xs text-gray-500 uppercase mb-1">Endpoint</div>
      <div class="font-mono text-sm">{endpoint}</div>
    </div>
    <div>
      <div class="text-xs text-gray-500 uppercase mb-1">Region</div>
      <div class="text-sm">{region}</div>
    </div>
    <div>
      <div class="text-xs text-gray-500 uppercase mb-1">Version</div>
      <div class="text-sm">{version}</div>
    </div>
    <div>
      <div class="text-xs text-gray-500 uppercase mb-1">TTL</div>
      <div class="text-sm">{ttl}s</div>
    </div>
    <div>
      <div class="text-xs text-gray-500 uppercase mb-1">Registered</div>
      <div class="text-sm">{registered}</div>
    </div>
    <div>
      <div class="text-xs text-gray-500 uppercase mb-1">Last Heartbeat</div>
      <div class="text-sm">{last_hb}</div>
    </div>
  </div>
  <div class="mt-6">
    <div class="text-xs text-gray-500 uppercase mb-2">Capabilities</div>
    <div>{caps_html}</div>
  </div>
</div>"#,
        node_id = escape_html(&node.node_id),
        endpoint = escape_html(&node.endpoint),
        version = escape_html(&node.version),
        ttl = node.ttl_seconds,
        registered = node.registered_at.format("%Y-%m-%d %H:%M:%S UTC"),
        last_hb = node.last_heartbeat.format("%Y-%m-%d %H:%M:%S UTC"),
    );

    layout(&format!("Node: {}", escape_html(&node.node_id)), &content)
}

pub fn node_not_found(node_id: &str) -> String {
    let safe_id = escape_html(node_id);
    let content = format!(
        r#"<div class="text-center py-16">
  <div class="text-6xl mb-4">🔍</div>
  <h1 class="text-2xl font-bold text-gray-700 mb-2">Node Not Found</h1>
  <p class="text-gray-500 mb-4">No node with ID <code class="bg-gray-100 px-2 py-1 rounded">{safe_id}</code> is registered.</p>
  <a href="/dashboard/nodes" class="text-indigo-600 hover:underline">&larr; Back to Nodes</a>
</div>"#
    );
    layout("Not Found", &content)
}

pub fn search_page(results: Option<&str>) -> String {
    let results_section = results.unwrap_or("");

    let content = format!(
        r#"<h1 class="text-2xl font-bold mb-6">Federated Search</h1>
<div class="bg-white rounded-lg shadow p-6 mb-6">
  <form method="GET" action="/dashboard/search" class="flex gap-4">
    <input type="text" name="q" placeholder="Search genes across network..."
      class="flex-1 px-4 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-indigo-500"
      value="">
    <select name="task_class" class="px-4 py-2 border border-gray-300 rounded-lg">
      <option value="">All Task Classes</option>
      <option value="build-fix">build-fix</option>
      <option value="test-fix">test-fix</option>
      <option value="perf-opt">perf-opt</option>
    </select>
    <button type="submit" class="px-6 py-2 bg-indigo-600 text-white rounded-lg hover:bg-indigo-700">Search</button>
  </form>
</div>
{results_section}"#
    );

    layout("Search", &content)
}

pub fn search_results(query: &str, total: usize, items_html: &str) -> String {
    let safe_query = escape_html(query);
    format!(
        r#"<div class="bg-white rounded-lg shadow p-6">
  <div class="flex items-center justify-between mb-4">
    <h2 class="text-lg font-semibold">Results for "{safe_query}"</h2>
    <span class="text-sm text-gray-500">{total} results</span>
  </div>
  {items_html}
  {empty}
</div>"#,
        empty = if total == 0 {
            r#"<div class="text-center text-gray-400 py-4">No results found</div>"#
        } else {
            ""
        }
    )
}
