# Observability Assets

This folder contains the default observability assets for the runtime metrics exposed at `GET /metrics`.

## Included files

- `runtime-dashboard.json`: Grafana dashboard covering runtime queue/backpressure/latency plus compatibility A2A queue, claim latency, lease expiry reclaim, and report-to-capture latency.
- `prometheus-alert-rules.yml`: Prometheus alert thresholds for elevated terminal error rate, high recovery latency, sustained backpressure, compatibility A2A queue stalling, and compatibility lease churn.
- `sample-runtime-workload.prom`: A sample scrape from the regression workload used to validate that the dashboard and alert rules reference real exported metrics.

## Validation

The repository regression `observability_assets_reference_metrics_present_in_sample_workload` checks that:

- every metric used by the dashboard exists in the sample workload scrape
- every metric used by the alert rules exists in the sample workload scrape

The runtime metrics endpoint regression `metrics_endpoint_is_scrape_ready_and_exposes_runtime_metrics` verifies that the live `/metrics` endpoint exports the same metric family names in Prometheus text format.
