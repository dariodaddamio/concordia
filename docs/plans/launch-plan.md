# Concordia: implementation-gated X and LinkedIn launch

Status: draft strategy and copy, 2026-11-01. No media has been recorded for this plan and nothing has been posted. The [eight-part series](social-series.md) becomes follow-up material after a strong lead demonstration.

## Implement first

The owner's priority is to finish the improvements before the campaign. This document does not turn unfinished roadmap items into shipped features. A polished TUI alone does not complete the memory-manager vision.

For the full memory/orchestration launch described in the plan, these gates stay open until there is implementation and evidence:

1. Whole-history native indexing/retrieval with bounded reads and complete pagination; Cursor's exact-readback route must be implemented or explicitly outside the announced support.
2. Related-chat retrieval, provenance/lineage, and goals/decisions that remain correct across handoffs. Current recent source chains and connected Harness pins are only initial slices.
3. Proactive context rollover using verified provider/version capacity signals, with explicit compaction policy. A terminal capacity-error retry is not proactive rollover.
4. Durable worker/run ownership and uncertain-completion reconciliation. Existing explicit workers do not establish recovered multi-worker orchestration.
5. Reliable repeated workflows and release gates: usage freshness, original request preservation, cancellation, drafts/queues, file references, Markdown, independent input scrolling, one update status, supported-host behavior and provider approval/session parity.

Internal editing and managed worktrees remain later workspace stages. They do not have to be claimed by this memory-focused launch. Live provider verification must be separately scoped and budgeted; the implementation tests use fixtures, not subscription-consuming model turns. Publishing remains a separate action.

Use three states for each claim: implemented, demonstrated, releasable. Only publish an availability claim when all three are true for its supported providers. No percentage or completion date is inferred from feature count. The full launch remains held; these drafts can be revised to a narrower preview only if the owner chooses that scope.

## Lead with one result

Recommended hook: **Changing models shouldn't mean re-explaining your project.**

The audience is developers who already work across coding CLIs. The concrete value is carrying a task and its sources through a session/account change while keeping unfinished work and drafts separate. Avoid a feature menu in the opening seconds. Explain implementation details in follow-ups for people who ask how it works.

Make one 25-35 second master recording of the actual app. That length is an editorial recommendation, not a platform rule. The central story:

- 0-3s: show a concrete task and its existing decision/file reference; overlay the hook.
- 3-8s: show the interruption or explicit switch. If a quota/capacity event is synthetic, label it visibly.
- 8-18s: show continuation with the original request and source identity, then an exact source excerpt. Claim the model used an excerpt only if the recording/journal actually proves that read.
- 18-26s: show the relevant file beside the chat and a distinct unsent draft preserved.
- Final beat: show the result, product name, exact preview/support status, and one working next action.

Keep text large enough to read at phone width, crop to the relevant UI rather than shrinking a whole desktop, and use short captions so the result makes sense without sound. Use synthetic personal/account data. Separate a fixture-driven UI demonstration from later real-provider evidence. Do not fabricate speed, cost or task-success comparisons.

## X: free account

Use one lead post and the strongest 20-30 second native MP4; retain short GIF cuts for the eight feature follow-ups. MP4 is the editorial choice for text clarity and compression, not a claim that GIF is unsupported. The free account supports 280-character standard posts and videos up to 140 seconds / 512 MB. Attachments do not consume the text character allowance. Check the composer after adding a URL.

Primary references, checked 2026-11-01: [standard posts](https://help.x.com/en/using-x/types-of-posts), [video limits](https://help.x.com/en/using-x/x-videos), [GIF attachments](https://help.x.com/en/using-x/posting-gifs-and-pictures).

Lead draft (245 ASCII characters, without a URL; held for the implementation/demo gates):

Changing models shouldn't mean re-explaining your project.

I'm building Concordia: a local Windows workspace that keeps tasks, source history and unfinished work connected across AI coding sessions.

One task. A new session. The work continues.

First reply: the precise supported providers, one limitation readers would otherwise misunderstand, and the release/demo link when it actually exists. Follow with 2-3 proof replies only if each adds new evidence. Then publish the existing feature clips as useful standalone posts rather than requiring everyone to read an eight-post thread first. Pin the lead post once the promised destination is available.

Alternate hook to test in a later post: **The task should survive the session.** Compare qualified replies and actual demo/download interest rather than assuming a hook will go viral. There is no account-specific evidence for a universal best posting time; choose a window when the owner can respond and adjust using the account's own analytics.

## LinkedIn: engineering story plus native video

Use the same master footage in a separate 30-45 second edit, with a brief founder explanation or captions connecting the workflow to engineering work. A 4:5 or square export is a candidate only if the terminal remains readable; a cropped landscape video is better than illegible portrait text. Upload the video directly, select a useful result frame for the thumbnail, and add/review captions. These are editorial recommendations; LinkedIn supports a wider range of formats.

LinkedIn's current native upload interface supports thumbnails and captions. Its own creative guidance recommends a fast opening, clear production and captions. This supports using the task/result first, then explaining the engineering problem; it is not evidence of guaranteed reach or a specific algorithm trick. [Video upload](https://www.linkedin.com/help/linkedin/answer/a7486279), [creative guidance](https://www.linkedin.com/business/marketing/blog/marketing-collective/how-to-create-high-impact-video-with-a-linkedin-expert), [writing from experience](https://www.linkedin.com/business/talent/blog/talent-acquisition/how-to-write-engaging-post-in-linkedin-feed).

Lead draft (held until the recording proves each described behavior):

Changing AI coding tools can make you the handoff system.

You carry the task, decisions, file references and unfinished work from one session to the next. A model switch becomes another round of explaining the project.

I'm building Concordia to keep that continuity in a local Windows workspace for coding CLIs.

The demo follows one task through a session change: the original request stays available, the next session gets explicit source pointers, and the relevant file opens beside the conversation. A newer draft stays separate from the unfinished request.

The engineering work behind that is source identity, bounded retrieval and recovery that stops when a tool's outcome is uncertain. Those details matter because a convincing summary is not proof of what actually happened.

Before launch, I'm finishing and validating the memory and orchestration behavior shown here. I'll publish the supported providers and remaining limits with the build.

Where does your coding workflow lose the most context: a session restart, an account limit, or handing work to another agent?

This is a draft in the founder's voice, not an invented user testimonial. If the publication happens after a public release, replace the prelaunch paragraph with the exact availability and link. Use one real invitation to discuss the workflow; avoid engagement bait or unrelated mentions.

Suggested follow-ups, each standing alone with a proof clip:

1. Source identity: explain why exact originals and stale-source rejection matter; show one read and one refusal after source replacement.
2. Recovery: demonstrate a pending request, newer draft and queued work remaining separate after restart. Explain why automatic replay can be wrong.
3. Explicit delegation: show task ownership and result handoff, and distinguish native provider subagents from Concordia workers.
4. Lessons from building the UI: tables/links, useful file previews, independent input scrolling, one status per operation. Connect each fix to a real workflow interruption.

Post follow-ups when there is new evidence to share. Do not copy an eight-post X thread verbatim into LinkedIn. Start with the pain/result, provide enough engineering detail to assess it, and answer substantive questions. Use account analytics and the quality of developer feedback to adjust cadence.

## Recording and publication readiness

The release/demo link must resolve to the version in the recording. The demo must be repeatable, the code must be the installed artifact, and the supported-host/provider limits must be visible. Known stress-test failures and unverified live behavior must not be hidden by a passing isolated demo. Cursor requests on the installed WezTerm 2024 host are not working visible custom pointers; see [host evidence](../chat-rendering.md).

The best immediate work is closing these implementation/evidence gaps. Copy, storyboards and exports can be prepared in parallel, but final launch claims and media stay gated on the demonstrated product.


## Session evidence before posts

The owner's [reference post](https://x.com/EXM7777/status/2116952915168616521) describes building
and researching firsthand, capturing the session, recording challenges/failures, then turning
those notes into posts. Use that workflow for Concordia: record the problem, change, proof and
remaining limitations before drafting copy. The reference text was retrieved through a public
syndication mirror when X could not be fetched; it is publishing advice, not an orchestration spec.
The clipboard/worker pass is an example: PowerShell blocked local actions, worker model selection
was absent, and result handback was manual. Synthetic tests must be labelled as such. Do not turn
these into live-provider speed, cost or task-success claims before measuring them.
