---
name: test-ai-chain
description: Manually run Mine Mail's billable DeepSeek Chat Completions smoke suite for draft optimization, the three Agent modes, and bidirectional translation of one explicitly selected cached real email. Use only when the user explicitly invokes `$test-ai-chain`; never infer, suggest, schedule, or automatically run this skill from ordinary debugging, testing, build, CI, startup, or release work.
---

# Test Mine Mail AI Chain

Run the repository's ignored, billable AI integration suite only after an explicit
`$test-ai-chain` invocation. Never add it to a routine test command or automation.

## Required input

- Require the user to open the exact target email in Mine Mail immediately before
  invoking this Skill. Use the locally recorded most-recent body access only.
- Require `DEEPSEEK_API_KEY` in the current process environment.
- Default to model `deepseek-v4-pro` and `https://api.deepseek.com`; accept an explicit
  model or compatible DeepSeek base URL override from the user.
- Explain that one manual run performs six API-backed cases and can incur charges.
- Treat the explicit Skill invocation plus the user's confirmation that the target
  email is currently open as authority for that one run. Ask for that confirmation
  before running; do not choose another message.

The selected message must already have a cached plain-text body and recent local access
time. The suite never fetches mail, sends mail,
marks mail read, saves a draft, applies a proposal, or changes the product AI config.

## Run

From the repository root, run exactly:

```powershell
& .\.agents\skills\test-ai-chain\scripts\run-ai-chain.ps1 `
  -ConfirmBillableRun
```

Pass `-Model` or `-BaseUrl` only when the user explicitly overrides the defaults.
Never echo, inspect, or persist `DEEPSEEK_API_KEY`.

The suite covers:

1. one mock-draft optimization with a mandatory real change;
2. Auto mode comprehensive read-only analysis;
3. Chat mode explicit generation authorization and rewrite proposal;
4. Generate mode contact search, recipient/subject/body edits, stationery, and
   language/paragraph preservation in one proposal;
5. one cached real email translated to Simplified Chinese;
6. the same cached real email translated to English.

## Report

Report passed/total cases, provider, protocol, model, per-case duration, completed
tool count, changed fields, optimization decision, byte count, and short SHA-256
digests. Do not report subjects, addresses, bodies, raw model output, API keys,
database paths, or full local paths. If a case fails, identify its name and the
privacy-safe error; do not retry automatically because retries can duplicate cost.
