# `.github/` — CI workflows and contribution templates

GitHub configuration for this repository: the workflows that build and test the
project, and the **mandatory** templates for issues and pull requests.

---

## Read this first if you are opening a PR or issue

[`AGENTS.md`](../AGENTS.md) states that reading the relevant template **before**
opening any PR, issue or discussion is mandatory, and that every section is
required — including ones that look ceremonial, such as checklists, the
**AI Assistance disclosure**, and any **human-written description**. A generic
Summary/Test-plan layout is not acceptable.

- **PRs:** read [`PULL_REQUEST_TEMPLATE.md`](PULL_REQUEST_TEMPLATE.md) and fill in
  every section. Where a section needs a human-written paragraph, leave an explicit
  TODO for the human contributor — do not write it in their voice.
- **Issues:** [`ISSUE_TEMPLATE/`](ISSUE_TEMPLATE/). Blank issues are disabled. Use
  `bug_report.md` for bugs.
- **Feature requests do not belong in issues.** They go to
  [Discussions](https://github.com/cjpais/Handy/discussions), per
  [`ISSUE_TEMPLATE/config.yml`](ISSUE_TEMPLATE/config.yml).

Upstream is under a **feature freeze**: features require demonstrated community
support in Discussions before a PR is opened. See
[`../CONTRIBUTING.md`](../CONTRIBUTING.md). These rules govern *upstream*
submissions — they do not restrict work on this private fork.

AI-assisted PRs are welcome upstream, but the extent of AI use must be disclosed.

---

## Templates

| File | Purpose |
| --- | --- |
| `PULL_REQUEST_TEMPLATE.md` | Mandatory PR structure, including AI disclosure and community feedback |
| `ISSUE_TEMPLATE/bug_report.md` | Bug report structure (system info, reproduction, logs) |
| `ISSUE_TEMPLATE/config.yml` | Disables blank issues; routes features to Discussions |
| `FUNDING.yml` | Sponsor links shown on the repository page |

---

## Workflows

| Workflow | Trigger | What it does |
| --- | --- | --- |
| `code-quality.yml` | push, PR, manual | Frontend lint/format plus repository checks |
| `test.yml` | push, PR, manual | Rust tests (`cargo test`) |
| `playwright.yml` | PR, manual | Playwright specs. **Not run on push to `main`** |
| `nix-check.yml` | push, PR, manual | Nix build check |
| `build.yml` | `workflow_call` | Reusable multi-platform build |
| `build-test.yml` | manual | Manual build test |
| `main-build.yml` | push | Build on main |
| `pr-test-build.yml` | manual | Build a PR and comment the result |
| `release.yml` | manual | Create a release and publish Tauri bundles |

### Notes for contributors

- Version/build toolchain is pinned in these workflows, so **CI is the source of
  truth for what must build** — a local success on a different toolchain can still
  fail CI.
- Release and Windows installer signing use **CI-only secrets and tooling**
  (upstream's Azure Trusted Signing account). Local signed builds will not work;
  use `bun run tauri build --no-bundle` for a local binary.
- `scripts/check-translations.ts` and `scripts/check-model-language-coverage.ts`
  are the data checks run by the code-quality path. Run them locally before pushing
  — see [`../scripts/README.md`](../scripts/README.md).
- The Playwright workflow deliberately excludes pushes to `main`, so a red
  Playwright run may only surface on your PR.

---

## Forking note

This repository is a fork used for local work. Workflow behaviour inherited from
upstream assumes upstream's secrets are present; for a private fork, release and
signing workflows will fail without equivalent configuration, while the test and
code-quality paths remain useful as-is.
