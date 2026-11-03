# L3: native binary/PDF inputs

Read the [common worker instructions](README.md), [input contracts](../../images-and-diagrams.md)
and the document phases in [workspace and inputs](../workspace-input-completion.md).

## Outcome

Deliver one verified native PDF transport, including attachment selection, durable snapshots,
queued recovery and provider dispatch. Images and text snapshots already work; binary documents
currently stop before dispatch. A new descriptor without a supported adapter is not delivery.

## Ownership

Own `crates/concordia/src/attachments.rs`, `attachment_documents.rs`, any agreed binary-input
helper and feature-specific documentation. Coordinate changes to shared provider payloads,
capability registration, TUI, queue and persistence with the coordinator. Preserve existing
image/text snapshot compatibility; describe any format migration before integration.

## Work sequence

1. Establish the selected provider/version's supported native document media schema, limits,
   model/session eligibility and local-byte versus upload requirements. Use official sources
   and read-only inspection. Do not assume image support implies PDF support.
2. Implement a bounded, versioned binary snapshot and one native adapter together. Preserve
   ordered mixed attachments and the existing four-attachment count. Propose explicit individual
   and aggregate byte limits before increasing existing limits.
3. Bind snapshots to their content hash and owner; validate bytes again on queued send and
   provider switch. Keep payloads out of ordinary conversation text/log metadata. Define cleanup
   and cancellation ownership without deleting a snapshot still referenced by a draft or queue.
4. Integrate selection and dispatch. Unsupported media/model/provider combinations pause before
   dispatch and retain the complete draft and queue. Show a precise actionable reason.

## Acceptance and handoff

Extend existing attachment/image fixture scenarios to cover a supported document, mixed ordering,
oversize/corrupt/changed input, provider switching, cancellation and restart recovery. Demonstrate
the exact serialized native payload through the fake provider and distinguish that from live proof.
Do not add OCR, prompt base64 conversion, remote conversion, new auth or parser dependencies merely
to bypass an unsupported transport. No automatic upload experiments or real model turns.

If no supported native contract exists, return the provider/version, sources examined and exact
missing contract as `blocked-vendor`; free the slot. Leave the capability open. Follow the common
delivery record, including migration/rollback and the shared integration patch.
