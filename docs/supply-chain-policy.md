# Supply-Chain Policy

Oris uses `cargo-deny` as the repository-owned baseline for dependency
advisories, license policy, duplicate version hygiene, and source provenance.

## Local command

Install the tool once:

```bash
cargo install cargo-deny --locked
```

Run the same check set as CI:

```bash
bash scripts/run_supply_chain_checks.sh
```

## Baseline rules

- RustSec vulnerabilities, unsound crates, and yanked releases fail the build.
- Unmaintained and notice advisories are surfaced as warnings and require human
  review.
- Unlicensed and copyleft dependencies are denied by default.
- Only crates from the default crates.io index are allowed.
- Wildcard dependency requirements are denied.

The canonical policy lives in [deny.toml](/Users/jiafan/Desktop/poc/Oris/deny.toml).

## Exception handling

Default policy is "no exceptions":

- `advisories.ignore = []`
- `licenses.exceptions = []`
- `bans.deny = []`, `bans.skip = []`, `bans.skip-tree = []`
- `sources.allow-git = []`

If an exception is unavoidable:

1. Keep the scope narrow. Prefer a single advisory or crate-specific exception.
2. Add the exception in `deny.toml` in the same PR that introduces it.
3. Include the upstream advisory/license identifier and the internal tracking
   issue or PR in a comment next to the exception.
4. Remove the exception as soon as the upstream fix or dependency replacement is
   available.

Silent waivers are not acceptable. Every exception must be reviewable in Git
history and visible in code review.
