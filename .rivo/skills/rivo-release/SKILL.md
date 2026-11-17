---
name: rivo-release
description: Prepare a Rivo release — run tests, check the headless scenes and summarise changes.
---

# Rivo release

Use this playbook when asked to prepare or verify a release of Rivo.

1. Run `cargo test` from the workspace root. Everything must pass.
2. Render the three headless scenes and check for duplicate-key warnings:
   - `cargo run -p rivo-app --bin rivo -- --scene demo --snap /tmp/rivo-demo.png`
   - `cargo run -p rivo-app --bin rivo -- --scene streaming --tree`
   - `cargo run -p rivo-app --bin rivo -- --scene settings --snap /tmp/rivo-settings.png`
3. Use `git_diff` to review what changed. Keep the diff focused.
4. Summarise: features added, tests run, any remaining risks.

Do not bump versions or push anything without an explicit request.
