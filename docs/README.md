# Shift documentation

Start with the user guide, then choose the engineering document that matches the
work.

| Document | Audience | Contents |
| --- | --- | --- |
| [`../README.md`](../README.md) / [`../README.zh-CN.md`](../README.zh-CN.md) | Users | Features, formats, shortcuts, privacy, known limitations |
| [`INSTALL.md`](INSTALL.md) | Users and support | Installation, first run, permissions, optional components, troubleshooting, uninstall |
| [`../CHANGELOG.md`](../CHANGELOG.md) | Users and maintainers | What changed in each version |
| [`ARCHITECTURE.md`](ARCHITECTURE.md) | Contributors | Process/window model, module ownership, events, data flows |
| [`SPECIFICATION.md`](SPECIFICATION.md) | Product and QA | Observable behaviour, naming, limits, acceptance criteria |
| [`SECURITY.md`](SECURITY.md) | Security reviewers and maintainers | Trust boundaries, update and LAN risk, residual gaps |
| [`ENGINEERING-NOTES.md`](ENGINEERING-NOTES.md) | Contributors | Regression ledger and non-negotiable implementation constraints |
| [`RELEASE.md`](RELEASE.md) | Release maintainers | Channel layout, signing, update handoff and verification |
| [`BUILD_WINDOWS.md`](BUILD_WINDOWS.md) | Builders | Windows prerequisites, cross-build and installer notes |
| [`ROADMAP.md`](ROADMAP.md) | Contributors | Completed milestones and next desktop work |
| [`MOBILE.md`](MOBILE.md) | Product planning | Mobile feasibility and architecture plan |
| [`../CONTRIBUTING.md`](../CONTRIBUTING.md) | Contributors | Setup, checks, change rules and manual matrix |

## Source of truth

- Current code and tests define implemented behavior.
- The conversion/tool tables in `README` describe the user-visible catalogue;
  the runtime catalogue lives in `src/lib/conversionFormats.ts` and
  `src/lib/radialOrder.ts`.
- Platform/runtime limitations must be stated as unverified when no real
  hardware run exists.
- Do not claim a CI run without linking its log.

