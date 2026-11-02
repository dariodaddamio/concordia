# Browser automation security audit

## Follow-up on 2026-10-31

The installed Python Playwright package was updated from 1.58.0 to the current 1.64.1.
Its official installer selected Chromium and headless shell 156.0.8078.4 and removed
unused older shared browser builds. EcoClaim's Playwright test dependency was updated
from 1.63.0 to 1.64.1; its workspace package audit now reports zero known advisories
after compatible dependency updates. Typechecking and the frontend production build pass.

The host scanner still raises two identification errors: Python Playwright 1.64.1's
bundled Node driver identifies itself as `1.64.0-beta-1791568591000`, and headless shell
has no PE product-version string. The Python package version and browser manifest were
checked separately; running the headless executable with `--version` reports
`Google Chrome for Testing 156.0.8078.4`. Neither error establishes an old browser.
Five extension manifests declare browser test packages that are not installed; those
remain unverified declarations. This follow-up does not certify all extensions or MCPs.

## Original findings

Checked 2026-10-31 on Windows after the report about CVE-2026-106378.

Concordia's Rust crate, lockfile and provider-launch code contain no Playwright,
Puppeteer, Chromium or embedded browser dependency or browser-automation launch path.
Its Markdown preview renders in the terminal. A GitHub release asset's
`browser_download_url` is a download field, not an embedded browser.

This does not certify provider tools. Codex, Claude Code, Cursor and their MCP servers
can launch browsers under their own configuration. Concordia preserves those native
tools and approvals; it does not update or sandbox their browser installations.

The local audit found old shared Windows Chromium caches (145, 147, 151 and 153), a
Python Playwright 1.58.0 driver, and multiple project Playwright installations below
1.64.1. Harness's main checkout was updated to Playwright 1.64.1 and its Chromium
156.0.8078.4 browser was downloaded. Other callers and old caches remain flagged.

The Harness source/build now offers `browser-security --root PATH --host --json`,
also registered as a public tool for clients. Its `doctor` checks project packages,
and its own craft capture refuses old packages and old/unknown browser runtimes before
navigation, with Chromium sandboxing requested. These changes are in Harness source;
the globally installed Harness 0.10.0 has not been replaced or published.

No macOS host or TCC/Accessibility permissions were available to inspect. Cursor's
built-in Browser Automation extension has not exposed a verified browser engine via
its package metadata. External MCP services, remote browsers, WSL/container images and
all virtual environments have not been certified. No provider inference or exploit
was run. Updating regular Chrome or Concordia alone is not remediation for those tools.

The [original research](https://pwn.ai/blog/chrome-escape) assumes existing native code
execution in the GPU helper and describes inherited macOS permissions. The fix shipped
in [Chrome 155.0.8059.39](https://chromereleases.googleblog.com/2026/10/stable-channel-update-for-desktop_086471744.html).
Harness uses the higher maintenance floor Playwright 1.64.1 / Chromium 156.0.8078.4;
that is not a claim of universal browser safety.
