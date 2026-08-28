# Desloppify backlog

Project: kie-mcp
Scope: current catalog-refresh diff and directly related tests
Mode: quick
Scan started: 2026-07-22T19:26:41+02:00
Scan status: complete
Baseline: 079112d184c5a5cf58aa6b8f70462205f11e62c5
Working tree: dirty; implementation changes in `src/kie/catalog.rs`, `src/kie/catalog/models.rs`, and `src/kie/jobs.rs`; pre-existing untracked `output/`
Product-code changes: none during this audit
Checks run: `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`, `cargo build --release`, focused debug-catalog queries, exact identifier search, and `git diff --check` passed

## Top next actions

None. No confirmed cleanup remains in the reviewed diff.

## Active backlog

### Critical

None.

### Medium cleanup

None.

### Nice-to-have polish

None.

## Needs confirmation

None.

## Completed

- DS-20260722-001 — Confirmed all ten requested identifiers are present exactly once and resolve to their intended media kind.
- DS-20260722-002 — Confirmed the parameterized first/last-frame binding replaces the existing hard-coded pair without adding overlapping behavior.
- DS-20260722-003 — Confirmed focused tests cover `image_size`, `quality`, and both new first/last-frame field pairs.

## Rejected / noise

None.

---

# Desloppify backlog — deep media-catalog contract audit

Project: kie-mcp
Scope: complete image/video catalog, request assembly, catalog-facing tests, and current KIE model documentation
Mode: deep
Scan started: 2026-07-22T20:56:37+02:00
Scan status: complete
Baseline: 506f46a90b34ca76514d1b0decae6de347797014
Working tree: dirty; pre-existing untracked `DESLOPPIFY.md` and `output/`
Product-code changes: corrected all confirmed catalog-contract defects, made prompt handling model-aware, removed uncataloged-model field guessing, added exhaustive catalog and request-assembly regression coverage, and bumped the crate to 0.2.0
Checks run: fetched `llms.txt` (68,849 bytes; SHA-256 `a943c9a51df2cef8fe980af1f6e0216e4da9d85a9eb6d6308c73052c1d8f8000`), parsed 116 official KIE media pages, matched and checked all 112 catalog entries, then passed `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test` (78 tests), `cargo build --release`, the 112-model contract snapshot, and `git diff --check`
Fix pass started: 2026-07-22T21:22:43+02:00
Fix completed: 2026-07-22T21:36:16+02:00
Fix status: complete

## Top next actions

None. All confirmed findings were corrected and verified. One upstream KIE documentation ambiguity remains listed under Needs confirmation.

## Active backlog

None.

## Fix-pass completed

### Critical

None.

### Medium cleanup

- [x] DS-20260722-004 — Four image-to-video profiles silently lose the documented terminal-frame convenience
  - Status: completed
  - Confidence: high
  - Blast radius: subsystem
  - Location: `src/kie/catalog/models.rs:66`, `:80`, `:85`, and `:87`
  - Evidence: Kling v2.1 Pro documents `image_url` plus `tail_image_url`; Bytedance V1 Lite and both Hailuo 02 image-to-video profiles document `image_url` plus optional `end_image_url`. The catalog declares all four as scalar `image_url` bindings.
  - Impact: `input_urls: [first, last]` is rejected even though KIE supports the pair. Supplying both fields manually in `input` still works.
  - Recommendation: use named pair bindings for all four profiles and add a table-driven payload test covering every terminal-frame naming convention.
  - Safe to fix now: yes — metadata-only changes using the existing named-pair implementation.
  - Verification: assert one URL populates the first field and two URLs populate both documented fields.

- [x] DS-20260722-005 — Four array bindings have incorrect or missing upper bounds
  - Status: completed
  - Confidence: high
  - Blast radius: subsystem
  - Location: `src/kie/catalog/models.rs:42`, `:57`, `:70`, and `:118`
  - Evidence: Grok Imagine Video 1.5 is capped at 1 locally but KIE publishes `maxItems: 7`; Ideogram Character is unlimited locally while KIE says only one image is supported; Kling 3.0 is unlimited locally while KIE describes at most first and last frames (one frame in multi-shot mode); Gemini Omni Video is unlimited locally while KIE states a maximum of 7 images.
  - Impact: the MCP rejects valid Grok requests containing 2–7 images and permits excess inputs for the other three models that KIE may reject or ignore.
  - Recommendation: set static caps to 7, 1, 2, and 7 respectively; optionally add mode-aware validation for Kling's stricter multi-shot limit.
  - Safe to fix now: yes for the static caps; mode-aware Kling validation is a separate decision.
  - Verification: boundary tests at max and max + 1 for each profile.

- [x] DS-20260722-006 — Two models with simple required media fields reject the top-level media shortcut
  - Status: completed
  - Confidence: high
  - Blast radius: local
  - Location: `src/kie/catalog/models.rs:102` and `:112`
  - Evidence: Wan 2.7 Video Edit requires scalar `video_url` but uses `UrlBinding::None`. HappyHorse Image to Video requires a one-element `image_urls` array but also uses `None`; its KIE schema has a trailing-space typo while the request example confirms the unspaced key.
  - Impact: `input_urls` and `local_input_paths` are rejected for these two models; callers must know and populate raw `input` fields.
  - Recommendation: bind Wan to `video_url` and HappyHorse to `image_urls` with a maximum of 1.
  - Safe to fix now: yes.
  - Verification: request-assembly tests for remote and uploaded local inputs.

- [x] DS-20260722-007 — Nine safe common-field mappings are missing across seven image profiles
  - Status: completed
  - Confidence: high
  - Blast radius: subsystem
  - Location: `src/kie/catalog/models.rs:12-15`, `:33-34`, and `:49`
  - Evidence: six profiles expose KIE `quality` but reject the MCP's top-level `resolution`; both Seedream 5 Lite profiles expose `output_format: png|jpeg` but declare no output-format style; Qwen2 Image Edit exposes ratio-valued `image_size` but has no aspect-ratio mapping.
  - Impact: the documented MCP conveniences fail for supported KIE fields, although callers can still place the exact fields in raw `input`.
  - Recommendation: map the six `quality` fields, enable JPEG/PNG normalization on Seedream 5 Lite, and map Qwen2 Image Edit aspect ratio to `image_size`.
  - Safe to fix now: yes — all target fields and enums are explicit in KIE's schemas.
  - Verification: payload tests for each distinct mapping/value style.

- [x] DS-20260722-008 — The global prompt contract does not represent promptless models
  - Status: completed
  - Confidence: high for the contract mismatch; medium for whether KIE rejects the extra field
  - Blast radius: subsystem
  - Location: `src/kie/jobs.rs:33`, `src/kie/jobs.rs:216`, and `src/kie/jobs.rs:243-246`
  - Evidence: the MCP requires a non-empty prompt and always inserts it. KIE publishes no `prompt` property for Topaz image/video upscale, both Recraft operations, Grok video upscale, both Wan Animate operations, and Volcengine lip sync.
  - Impact: callers must invent a meaningless prompt and every request sends an undocumented field. KIE may ignore it, but the MCP cannot express the published contract exactly.
  - Recommendation: add a compact per-model prompt policy and make `prompt` optional where KIE omits or permits it.
  - Safe to fix now: depends — changing the public MCP schema needs compatibility review.
  - Verification: schema tests plus payload tests for required, optional, and absent prompt policies.

- [x] DS-20260722-009 — Uncataloged raw models receive guessed field names despite heterogeneous KIE schemas
  - Status: completed
  - Confidence: high
  - Blast radius: subsystem
  - Location: `src/kie/jobs.rs:402-429` and the `spec == None` convenience fallbacks in `src/kie/jobs.rs`
  - Evidence: an unknown ID containing `image-to-video` is automatically assigned `first_frame_url`/`last_frame_url`; other unknown IDs receive guessed `image`, `input_urls`, `aspect_ratio`, `resolution`, and `output_format` fields. The audited KIE pages use multiple incompatible naming conventions.
  - Impact: future or lagging models can pass media-kind validation yet receive a syntactically valid request with the wrong input field names.
  - Recommendation: continue accepting inferable raw IDs, but require model-specific fields in raw `input` whenever no catalog profile exists; do not guess convenience bindings.
  - Safe to fix now: depends — safer behavior, but it intentionally rejects previously guessed raw-model shortcuts.
  - Verification: tests proving raw models accept explicit `input` and reject unmapped top-level conveniences.

- [x] DS-20260722-010 — Existing regression tests cannot detect catalog/document drift
  - Status: completed
  - Confidence: high
  - Blast radius: repository-wide
  - Location: `src/kie/catalog.rs:295-405` and `src/kie/jobs.rs:815-843`
  - Evidence: the suite checks key uniqueness, a minimum model count, and a few representative profiles. All 66 tests pass while the confirmed metadata defects above remain reachable. The earlier quick audit therefore produced an unjustifiably reassuring result.
  - Impact: wrong field names, missing second frames, and bad cardinalities can ship as long as the selected representative tests remain green.
  - Recommendation: add a repeatable official-doc comparison command and retain reviewed, table-driven assertions for every nontrivial media binding and common-field mapping.
  - Safe to fix now: yes, after the catalog corrections establish the intended contract.
  - Verification: intentionally perturb one field, one cap, and one model ID and confirm the audit gate fails.

### Nice-to-have polish

None.

## Needs confirmation

- Grok Imagine Image to Image: the same KIE schema says `maxItems: 5` but its description says “up to 1”. The catalog currently follows the machine-readable limit of 5; a non-billable authoritative clarification or explicitly approved live validation is needed.

## Completed

- DS-20260722-011 — Matched all 112 catalog entries (44 image, 68 video) to the 116 current KIE media pages and confirmed their media kinds and normalized display keys.
- DS-20260722-012 — Verified that every field currently declared by a scalar, array, pair, aspect-ratio, resolution, or output-format profile exists with the expected type in its matched KIE schema.
- DS-20260722-013 — Confirmed exact normalized catalog keys are unique and all catalog entries resolve to a single official page.
- DS-20260722-014 — Confirmed the unified task-result contract uses `resultUrls` for generated image/video media and that the current output normalizer also handles Seedance first/last-frame results.
- DS-20260722-015 — Ran the full local verification gates successfully without changing product code.

## Rejected / noise

- KIE's Qwen2 Text to Image page declares `qwen2/image-edit` in its enum/default/example while its title, URL, and operation ID identify text-to-image. The catalog's `qwen2/text-to-image` ID is the coherent interpretation.
- KIE's Kling V2.5 Turbo Image to Video Pro page retains the V2.1 Master model ID and operation ID. The catalog's V2.5 ID is coherent with the page title and URL.
- KIE schemas contain trailing spaces in `reference_video_urls ` (Seedance 2 Fast), `image_urls ` (HappyHorse Image to Video), and `reference_image ` (HappyHorse Video Edit); examples use the unspaced names where shown.
- OmniHuman human-identification and subject-detection are intentionally excluded: they return classification/mask objects rather than generated image/video media.
- Legacy token-valued `image_size` fields such as `landscape_16_9` were not labeled missing aspect-ratio bindings because correct convenience support would require value translation, not merely a field rename; raw `input` remains the appropriate path.

## Per-model matrix

The status below concerns the compact catalog contract, not every model-specific KIE option. “Conforme” means the current media binding and declared common mappings agree with the official page; complex, uncommon inputs remain intentionally available through raw `input`.

| Modèle | Type | Binding média du catalogue | Résultat de l’audit |
| --- | --- | --- | --- |
| [`bytedance/seedream`](https://docs.kie.ai/market/seedream/seedream.md) | image | — | Conforme. |
| [`bytedance/seedream-v4-text-to-image`](https://docs.kie.ai/market/seedream/seedream-v4-text-to-image.md) | image | — | Conforme. |
| [`bytedance/seedream-v4-edit`](https://docs.kie.ai/market/seedream/seedream-v4-edit.md) | image | `image_urls` [10] | Conforme. |
| [`seedream/4.5-text-to-image`](https://docs.kie.ai/market/seedream/4-5-text-to-image.md) | image | — | Conforme : `resolution` est reliée à `quality`. |
| [`seedream/4.5-edit`](https://docs.kie.ai/market/seedream/4-5-edit.md) | image | `image_urls` [14] | Conforme : `resolution` est reliée à `quality`. |
| [`seedream/5-lite-text-to-image`](https://docs.kie.ai/market/seedream/5-lite-text-to-image.md) | image | — | Conforme : `quality` et `output_format` (`png`/`jpeg`) sont reliés. |
| [`seedream/5-lite-image-to-image`](https://docs.kie.ai/market/seedream-5-lite-image-to-image.md) | image | `image_urls` [14] | Conforme : `quality` et `output_format` (`png`/`jpeg`) sont reliés. |
| [`seedream/5-pro-text-to-image`](https://docs.kie.ai/market/seedream/5-pro-text-to-image.md) | image | — | Conforme. |
| [`seedream/5-pro-image-to-image`](https://docs.kie.ai/market/seedream/5-pro-image-to-image.md) | image | `image_urls` [10] | Conforme. |
| [`z-image`](https://docs.kie.ai/market/z-image/z-image.md) | image | — | Conforme. |
| [`google/imagen4-fast`](https://docs.kie.ai/market/google/imagen4-fast.md) | image | — | Conforme. |
| [`google/imagen4-ultra`](https://docs.kie.ai/market/google/imagen4-ultra.md) | image | — | Conforme. |
| [`google/imagen4`](https://docs.kie.ai/market/google/imagen4.md) | image | — | Conforme. |
| [`google/nano-banana-edit`](https://docs.kie.ai/market/google/nano-banana-edit.md) | image | `image_urls` [10] | Conforme. |
| [`google/nano-banana`](https://docs.kie.ai/market/google/nano-banana.md) | image | — | Conforme. |
| [`nano-banana-pro`](https://docs.kie.ai/market/google/pro-image-to-image.md) | image | `image_input` [8] | Conforme. |
| [`nano-banana-2`](https://docs.kie.ai/market/google/nanobanana2.md) | image | `image_input` [14] | Conforme. |
| [`nano-banana-2-lite`](https://docs.kie.ai/market/google/nano-banana-2-lite.md) | image | `image_urls` [10] | Conforme. |
| [`flux-2/pro-image-to-image`](https://docs.kie.ai/market/flux2/pro-image-to-image.md) | image | `input_urls` [8] | Conforme. |
| [`flux-2/pro-text-to-image`](https://docs.kie.ai/market/flux2/pro-text-to-image.md) | image | — | Conforme. |
| [`flux-2/flex-image-to-image`](https://docs.kie.ai/market/flux2/flex-image-to-image.md) | image | `input_urls` [8] | Conforme. |
| [`flux-2/flex-text-to-image`](https://docs.kie.ai/market/flux2/flex-text-to-image.md) | image | — | Conforme. |
| [`grok-imagine/text-to-image`](https://docs.kie.ai/market/grok-imagine/text-to-image.md) | image | — | Conforme. |
| [`grok-imagine/image-to-image`](https://docs.kie.ai/market/grok-imagine/image-to-image.md) | image | `image_urls` [5] | **À confirmer :** KIE se contredit: `maxItems: 5`, description « up to 1 »; le catalogue suit le schéma à 5 |
| [`gpt-image/1.5-text-to-image`](https://docs.kie.ai/market/gpt-image/1-5-text-to-image.md) | image | — | Conforme : `resolution` est reliée à `quality`. |
| [`gpt-image/1.5-image-to-image`](https://docs.kie.ai/market/gpt-image/1-5-image-to-image.md) | image | `input_urls` [16] | Conforme : `resolution` est reliée à `quality`. |
| [`gpt-image-2-text-to-image`](https://docs.kie.ai/market/gpt/gpt-image-2-text-to-image.md) | image | — | Conforme. |
| [`gpt-image-2-image-to-image`](https://docs.kie.ai/market/gpt/gpt-image-2-image-to-image.md) | image | `input_urls` [16] | Conforme. |
| [`topaz/image-upscale`](https://docs.kie.ai/market/topaz/image-upscale.md) | image | `image_url` | Conforme : politique de prompt `none`. |
| [`recraft/remove-background`](https://docs.kie.ai/market/recraft/remove-background.md) | image | `image` | Conforme : politique de prompt `none`. |
| [`recraft/crisp-upscale`](https://docs.kie.ai/market/recraft/crisp-upscale.md) | image | `image` | Conforme : politique de prompt `none`. |
| [`ideogram/character-edit`](https://docs.kie.ai/market/ideogram/character-edit.md) | image | — | Conforme au périmètre compact : entrées média complexes laissées dans `input`. |
| [`ideogram/character-remix`](https://docs.kie.ai/market/ideogram/character-remix.md) | image | — | Conforme au périmètre compact : entrées média complexes laissées dans `input`. |
| [`ideogram/character`](https://docs.kie.ai/market/ideogram/character.md) | image | `reference_image_urls` [1] | Conforme. |
| [`ideogram/v3-text-to-image`](https://docs.kie.ai/market/ideogram/v3-text-to-image.md) | image | — | Conforme. |
| [`ideogram/v3-edit`](https://docs.kie.ai/market/ideogram/v3-edit.md) | image | `image_url` | Conforme. |
| [`ideogram/v3-remix`](https://docs.kie.ai/market/ideogram/v3-remix.md) | image | `image_url` | Conforme. |
| [`qwen/text-to-image`](https://docs.kie.ai/market/qwen/text-to-image.md) | image | — | Conforme. |
| [`qwen/image-to-image`](https://docs.kie.ai/market/qwen/image-to-image.md) | image | `image_url` | Conforme. |
| [`qwen/image-edit`](https://docs.kie.ai/market/qwen/image-edit.md) | image | `image_url` | Conforme. |
| [`qwen2/image-edit`](https://docs.kie.ai/market/qwen2/image-edit.md) | image | `image_url` | Conforme : `aspect_ratio` est relié à `image_size`. |
| [`qwen2/text-to-image`](https://docs.kie.ai/market/qwen2/text-to-image.md) | image | — | **Doc KIE :** la page KIE déclare par copier-coller l’ID `qwen2/image-edit`; titre, URL et operationId indiquent text-to-image |
| [`wan/2-7-image`](https://docs.kie.ai/market/wan/2-7-image.md) | image | `input_urls` [9] | Conforme. |
| [`wan/2-7-image-pro`](https://docs.kie.ai/market/wan/2-7-image-pro.md) | image | `input_urls` [9] | Conforme. |
| [`grok-imagine/text-to-video`](https://docs.kie.ai/market/grok-imagine/text-to-video.md) | video | — | Conforme. |
| [`grok-imagine/image-to-video`](https://docs.kie.ai/market/grok-imagine/image-to-video.md) | video | `image_urls` [7] | Conforme. |
| [`grok-imagine/upscale`](https://docs.kie.ai/market/grok-imagine/upscale.md) | video | — | Conforme : politique de prompt `none`. |
| [`grok-imagine/extend`](https://docs.kie.ai/market/grok-imagine/extend.md) | video | — | Conforme. |
| [`grok-imagine-video-1-5-preview`](https://docs.kie.ai/market/grok-imagine/1-5-preview.md) | video | `image_urls` [7] | Conforme. |
| [`kling-2.6/text-to-video`](https://docs.kie.ai/market/kling/text-to-video.md) | video | — | Conforme. |
| [`kling-2.6/image-to-video`](https://docs.kie.ai/market/kling/image-to-video.md) | video | `image_urls` [1] | Conforme. |
| [`kling/v2-1-master-image-to-video`](https://docs.kie.ai/market/kling/v2-1-master-image-to-video.md) | video | `image_url` | Conforme. |
| [`kling/v2-5-turbo-image-to-video-pro`](https://docs.kie.ai/market/kling/v25-turbo-image-to-video-pro.md) | video | `image_url` → `tail_image_url` | **Doc KIE :** la page KIE conserve l’ID et l’operationId de Kling v2.1 Master |
| [`kling/v2-5-turbo-text-to-video-pro`](https://docs.kie.ai/market/kling/v25-turbo-text-to-video-pro.md) | video | — | Conforme. |
| [`kling/ai-avatar-standard`](https://docs.kie.ai/market/kling/ai-avatar-standard.md) | video | — | Conforme au périmètre compact : entrées média complexes laissées dans `input`. |
| [`kling/ai-avatar-pro`](https://docs.kie.ai/market/kling/ai-avatar-pro.md) | video | — | Conforme au périmètre compact : entrées média complexes laissées dans `input`. |
| [`kling/v2-1-master-text-to-video`](https://docs.kie.ai/market/kling/v2-1-master-text-to-video.md) | video | — | Conforme. |
| [`kling/v2-1-pro`](https://docs.kie.ai/market/kling/v2-1-pro.md) | video | `image_url` / `tail_image_url` | Conforme. |
| [`kling/v2-1-standard`](https://docs.kie.ai/market/kling/v2-1-standard.md) | video | `image_url` | Conforme. |
| [`kling-2.6/motion-control`](https://docs.kie.ai/market/kling/motion-control.md) | video | — | Conforme au périmètre compact : entrées média complexes laissées dans `input`. |
| [`kling-3.0/motion-control`](https://docs.kie.ai/market/kling/motion-control-v3.md) | video | — | Conforme au périmètre compact : entrées média complexes laissées dans `input`. |
| [`kling-3.0/video`](https://docs.kie.ai/market/kling/kling-3-0.md) | video | `image_urls` [2] | Conforme au contrat compact; KIE valide la limite plus stricte du mode multi-shot. |
| [`kling/v3-turbo-text-to-video`](https://docs.kie.ai/market/kling/v3-turbo-text-to-video.md) | video | — | Conforme. |
| [`kling/v3-turbo-image-to-video`](https://docs.kie.ai/market/kling/v3-turbo-image-to-video.md) | video | `image_urls` [1] | Conforme : la page KIE du modèle indique explicitement un seul fichier maximum. |
| [`bytedance/seedance-2`](https://docs.kie.ai/market/bytedance/seedance-2.md) | video | `first_frame_url` → `last_frame_url` | Conforme. |
| [`bytedance/seedance-2-fast`](https://docs.kie.ai/market/bytedance/seedance-2-fast.md) | video | `first_frame_url` → `last_frame_url` | **Doc KIE :** le schéma KIE contient `reference_video_urls ` avec un espace final |
| [`bytedance/seedance-2-mini`](https://docs.kie.ai/market/bytedance/seedance-2-mini.md) | video | `first_frame_url` → `last_frame_url` | Conforme. |
| [`bytedance/seedance-1.5-pro`](https://docs.kie.ai/market/bytedance/seedance-1-5-pro.md) | video | `input_urls` [2] | Conforme. |
| [`bytedance/v1-pro-fast-image-to-video`](https://docs.kie.ai/market/bytedance/v1-pro-fast-image-to-video.md) | video | `image_url` | Conforme. |
| [`bytedance/v1-pro-image-to-video`](https://docs.kie.ai/market/bytedance/v1-pro-image-to-video.md) | video | `image_url` | Conforme. |
| [`bytedance/v1-pro-text-to-video`](https://docs.kie.ai/market/bytedance/v1-pro-text-to-video.md) | video | — | Conforme. |
| [`bytedance/v1-lite-image-to-video`](https://docs.kie.ai/market/bytedance/v1-lite-image-to-video.md) | video | `image_url` / `end_image_url` | Conforme. |
| [`bytedance/v1-lite-text-to-video`](https://docs.kie.ai/market/bytedance/v1-lite-text-to-video.md) | video | — | Conforme. |
| [`hailuo/2-3-image-to-video-pro`](https://docs.kie.ai/market/hailuo/2-3-image-to-video-pro.md) | video | `image_url` | Conforme. |
| [`hailuo/2-3-image-to-video-standard`](https://docs.kie.ai/market/hailuo/2-3-image-to-video-standard.md) | video | `image_url` | Conforme. |
| [`hailuo/02-text-to-video-pro`](https://docs.kie.ai/market/hailuo/02-text-to-video-pro.md) | video | — | Conforme. |
| [`hailuo/02-image-to-video-pro`](https://docs.kie.ai/market/hailuo/02-image-to-video-pro.md) | video | `image_url` / `end_image_url` | Conforme. |
| [`hailuo/02-text-to-video-standard`](https://docs.kie.ai/market/hailuo/02-text-to-video-standard.md) | video | — | Conforme. |
| [`hailuo/02-image-to-video-standard`](https://docs.kie.ai/market/hailuo/02-image-to-video-standard.md) | video | `image_url` / `end_image_url` | Conforme. |
| [`wan/2-2-a14b-image-to-video-turbo`](https://docs.kie.ai/market/wan/2-2-a14b-image-to-video-turbo.md) | video | `image_url` | Conforme. |
| [`wan/2-2-a14b-speech-to-video-turbo`](https://docs.kie.ai/market/wan/2-2-a14b-speech-to-video-turbo.md) | video | — | Conforme au périmètre compact : entrées média complexes laissées dans `input`. |
| [`wan/2-2-a14b-text-to-video-turbo`](https://docs.kie.ai/market/wan/2-2-a14b-text-to-video-turbo.md) | video | — | Conforme. |
| [`wan/2-2-animate-move`](https://docs.kie.ai/market/wan/2-2-animate-move.md) | video | — | Conforme : politique de prompt `none`. |
| [`wan/2-2-animate-replace`](https://docs.kie.ai/market/wan/2-2-animate-replace.md) | video | — | Conforme : politique de prompt `none`. |
| [`wan/2-6-image-to-video`](https://docs.kie.ai/market/wan/2-6-image-to-video.md) | video | `image_urls` [1] | Conforme. |
| [`wan/2-6-text-to-video`](https://docs.kie.ai/market/wan/2-6-text-to-video.md) | video | — | Conforme. |
| [`wan/2-6-video-to-video`](https://docs.kie.ai/market/wan/2-6-video-to-video.md) | video | `video_urls` [3] | Conforme. |
| [`wan/2-6-flash-image-to-video`](https://docs.kie.ai/market/wan/2-6-flash-image-to-video.md) | video | `image_urls` [1] | Conforme. |
| [`wan/2-6-flash-video-to-video`](https://docs.kie.ai/market/wan/2-6-flash-video-to-video.md) | video | `video_urls` [3] | Conforme. |
| [`wan/2-5-image-to-video`](https://docs.kie.ai/market/wan/2-5-image-to-video.md) | video | `image_url` | Conforme. |
| [`wan/2-5-text-to-video`](https://docs.kie.ai/market/wan/2-5-text-to-video.md) | video | — | Conforme. |
| [`wan/2-7-text-to-video`](https://docs.kie.ai/market/wan/2-7-text-to-video.md) | video | — | Conforme au périmètre compact : entrées média complexes laissées dans `input`. |
| [`wan/2-7-image-to-video`](https://docs.kie.ai/market/wan/2-7-image-to-video.md) | video | `first_frame_url` → `last_frame_url` | Conforme. |
| [`wan/2-7-videoedit`](https://docs.kie.ai/market/wan/2-7-videoedit.md) | video | `video_url` | Conforme. |
| [`wan/2-7-r2v`](https://docs.kie.ai/market/wan/2-7-r2v.md) | video | — | Conforme au périmètre compact : entrées média complexes laissées dans `input`. |
| [`topaz/video-upscale`](https://docs.kie.ai/market/topaz/video-upscale.md) | video | `video_url` | Conforme : politique de prompt `none`. |
| [`infinitalk/from-audio`](https://docs.kie.ai/market/infinitalk/from-audio.md) | video | — | Conforme au périmètre compact : entrées média complexes laissées dans `input`. |
| [`pixverse-v6/text-to-video`](https://docs.kie.ai/market/pixverse/text-to-video.md) | video | — | Conforme. |
| [`pixverse-v6/image-to-video`](https://docs.kie.ai/market/pixverse/image-to-video.md) | video | `image_urls` [2] | Conforme. |
| [`pixverse-v6/transition`](https://docs.kie.ai/market/pixverse/transition.md) | video | `first_frame_image_url` → `last_frame_image_url` | Conforme. |
| [`pixverse-v6/extend`](https://docs.kie.ai/market/pixverse/extend.md) | video | `video_url` | Conforme. |
| [`pixverse-v6/reference-to-video`](https://docs.kie.ai/market/pixverse/reference-to-video.md) | video | — | Conforme. |
| [`happyhorse/text-to-video`](https://docs.kie.ai/market/happyhorse/text-to-video.md) | video | — | Conforme. |
| [`happyhorse/image-to-video`](https://docs.kie.ai/market/happyhorse/image-to-video.md) | video | `image_urls` [1] | Conforme; le nom sans espace suit l’exemple KIE. |
| [`happyhorse/reference-to-video`](https://docs.kie.ai/market/happyhorse/reference-to-video.md) | video | `reference_image` [9] | Conforme. |
| [`happyhorse/video-edit`](https://docs.kie.ai/market/happyhorse/video-edit.md) | video | `video_url` | **Doc KIE :** le schéma KIE contient `reference_image ` avec un espace final |
| [`happyhorse-1-1/image-to-video`](https://docs.kie.ai/market/happyhorse-1-1/image-to-video.md) | video | `image_urls` [1] | Conforme. |
| [`happyhorse-1-1/text-to-video`](https://docs.kie.ai/market/happyhorse-1-1/text-to-video.md) | video | — | Conforme. |
| [`happyhorse-1-1/reference-to-video`](https://docs.kie.ai/market/happyhorse-1-1/reference-to-video.md) | video | `reference_image` [9] | Conforme. |
| [`gemini-omni-video`](https://docs.kie.ai/market/gemini-omni-video.md) | video | `image_urls` [7] | Conforme. |
| [`omnihuman-1-5`](https://docs.kie.ai/market/omnihuman-1-5.md) | video | — | Conforme au périmètre compact : entrées média complexes laissées dans `input`. |
| [`volcengine/video-to-video-lip-sync`](https://docs.kie.ai/market/volcengine/video-to-video-lip-sync.md) | video | — | Conforme : politique de prompt `none`. |

---

# Desloppify backlog — Kie structured operations and catalog refresh

Project: kie-mcp
Scope: current catalog refresh, five specialized Kie operations, related tests, and documentation
Mode: standard
Scan started: 2026-08-20T22:35:13+02:00
Scan completed: 2026-08-20T22:36:40+02:00
Scan status: complete
Baseline: eb03a55c121d319eeb0d8fbf37770a34b1f7d526
Working tree: dirty; intended implementation changes plus pre-existing untracked `DESLOPPIFY.md` and `output/`
Product-code changes: none during this audit
Checks run: `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test` (89 tests), `cargo build --release`, `git diff --check`, catalog ID uniqueness check, and focused searches for stale identifiers, TODOs, and unchecked production paths all passed

## Top next actions

1. Completed later in `c8e93ca`: add an MCP call test for resuming completed structured tasks through `kie_task_status`.

## Active backlog

### Critical

None confirmed.

### Medium cleanup

- [x] DS-20260820-001 — Structured task recovery has no public MCP regression test
  - Status: resolved in `c8e93ca`
  - Confidence: high
  - Blast radius: subsystem
  - Location: `src/mcp.rs:429` and `tests/mcp_stdio.rs:59`
  - Evidence: `kie_task_status` has a new success branch for the three structured models, but the stdio suite only lists tools and checks schemas. No test calls the tool or verifies typed results and optional mask downloads.
  - Impact: a later routing change could send a recovered Segment Map or OmniHuman task through media download, return `NoMedia`, or skip previews without failing the current suite.
  - Recommendation: run the stdio server against a local mock `recordInfo` response and call `kie_task_status` for one structured result with `download_if_complete` both false and true.
  - Safe to fix now: completed after explicit user approval.
  - Verification: `cargo test --test mcp_stdio` plus the full test suite.
  - Resolution: the stdio suite now calls `kie_task_status` against a local structured Segment Map record, checks the typed result without downloads, then enables preview download and verifies the absolute local mask path.

### Nice-to-have polish

None confirmed.

## Needs confirmation

None.

## Completed

- DS-20260820-002 — Confirmed the reviewed snapshot contains 127 unique final image/video model IDs and excludes the three structured preparation models.
- DS-20260820-003 — Confirmed the Gemini Audio and Character clients use their dedicated authenticated routes, documented request keys, response codes, and reusable IDs.
- DS-20260820-004 — Confirmed Grok and OmniHuman structured tasks reuse task creation and polling, parse typed `resultObject` data, preserve remote mask URLs, and treat local previews as best effort.
- DS-20260820-005 — Confirmed all 11 MCP tools publish the intended schemas and the README and maintainer notes match the implemented workflows.

## Rejected / noise

- Treating `subject_status` as a Boolean or success code was rejected because Kie's documentation does not define its integer values; the tool returns it unchanged.
- Flagging Gemini Omni Audio as standalone audio scope creep was rejected because the endpoint returns a reusable profile ID, not an audio file, and that ID is an input to Gemini Omni Video.
- Making preview download failures fail the whole structured task was rejected because the remote mask URLs remain the primary reusable API result.

---

# Desloppify backlog — full KIE contract and Segment Map investigation

Project: kie-mcp
Scope: full repository, all MCP tools, embedded model profiles, live catalog parser and current official KIE image/video route contracts
Mode: deep
Scan started: 2026-08-28T01:23:48+02:00
Scan completed: 2026-08-28T01:32:18+02:00
Scan status: complete
Baseline: 23dfb25e436d4a005195e49cabbb42ae1f92bd9b
Working tree: dirty; pre-existing untracked `DESLOPPIFY.md` and `output/`
Product-code changes: none during this scan
Checks run: repository instructions and prior audits read; exact identifier and cross-tool-description searches; Git and Codex task-history inspection; current `llms.txt` and route documents fetched read-only; all 130 live image/video contracts and all 127 embedded profiles inventoried; 125 embedded routes compared with authoritative live schemas; every live schema walked recursively; representative routes from all 27 model families checked manually; all five structured preparation tools and both Gemini reusable-ID tools compared with their dedicated KIE documents; configured release and worktree release provenance checked

## Evidence set and chronology

- On 2026-08-20, the retained Codex task record downloaded KIE's `llms.txt` with SHA-256 `43e0b581b8289c0498d44f31fa8740bd44f9d77eb28394840388a73247c2d911`. Its Grok Image 2 index contained Text to Image, Segment Map, and a page titled Image Edit at `/market/grok-imagine-image-2-0/image-edit.md`; it did not contain Segment Edit or `/image-to-image.md`. The route parser recorded that page's declared model as `grok-imagine-image-2-0/image-edit`. This proves the August 20 implementation followed the then-published canonical identifier.
- Commits `9562cf7` and `3a5369d`, both dated 2026-08-20, added the embedded Grok Image 2 profiles and the Segment Map instruction. The instruction correctly reflected that dated KIE snapshot, but it was handwritten and had no downstream-contract regression test.
- By 2026-08-27, retained documentation fetches show that KIE had repurposed `/image-edit.md` as **Segment Edit**, declaring `grok-imagine-image-2-0/segment-edit`, `prompt`, `task_id`, and `mask_indexs`, and had introduced `/image-to-image.md` for the separate `grok-imagine-image-2-0/image-edit` contract with `aspect_ratio` and `image_urls`.
- During development of the live catalog on 2026-08-27, an intermediate parser rejected the repurposed `/image-edit.md` page because its URL no longer contained the declared `segment-edit` identifier. Commit `23dfb25` (2026-08-28T00:46:41+02:00) fixed renamed-route acceptance generically when the page title confirms the model. The current live catalog therefore exposes Segment Edit correctly. The stale handwritten instruction and embedded Grok profiles were not updated in that commit.
- The 2026-08-28 read-only evidence set contains KIE `llms.txt` SHA-256 `1f2e479b07265e9bd5b422e00009aa929fe6a8b5f1ff8c74a02a1cd56668bd01`, Segment Map document SHA-256 `c89c1f5fe02c0d00bd2686a65e9f84bd0561d60a40ad46ec6ec2642f88e21285`, Segment Edit document SHA-256 `1637e6398c13bdc9da28ee0ceab99dd5d18eb29cbcae1565e33f39473d95609e`, and Image Edit document SHA-256 `c24287eaeebc100a20df809db2212c83a8fc31e9dec66786cd23b186f13e3cf7`.
- The configured executable is `/Users/pym/www/Git/GitHub/pym/kie-mcp/target/release/kie-mcp`. It was rebuilt from the main checkout at commit `23dfb25` on 2026-08-28T01:14:09+02:00. Its SHA-256 is `99c83960529e923646bee37991b91da850849730df294f8ef72f3ac3c47adf22`. The older worktree artifact at `/Users/pym/.codex/worktrees/dadb/kie-mcp/target/release/kie-mcp` is byte-identical, but the Codex configuration does not point to it. The current binary itself contains the stale Segment Map description, so binary selection cannot explain this defect.

## Top next actions

1. Completed in `4a25d19`: downgrade self-contradictory live schemas to informational (DS-20260828-002).
2. Completed in `c8e93ca`: add the public MCP regression for structured task recovery (DS-20260820-001).
3. The MCP policy now follows the machine-readable Grok and OmniHuman limits. Backend behavior outside those limits still requires a KIE answer.

## Findings

### Critical

- [x] DS-20260828-001 — Segment Map tells callers to use the unrelated Image Edit contract
  - Status: resolved after the audit
  - Confidence: high
  - Blast radius: subsystem
  - Location: `src/mcp.rs:375`, `README.md:127`, `src/kie/catalog/models.rs:34`, `tests/fixtures/kie_catalog_contract.json:335`, and the absent downstream test near `tests/kie_mock.rs:959`
  - Evidence: the tool and README direct `task_id` and `mask_indexs` to `grok-imagine-image-2-0/image-edit`. The current official Segment Edit OpenAPI declares `grok-imagine-image-2-0/segment-edit` with required `prompt` and `task_id`, plus `mask_indexs` as a non-empty integer array. The separate Image Edit OpenAPI declares `grok-imagine-image-2-0/image-edit` with required `aspect_ratio` and `image_urls` (at most five), while `prompt` is optional. The embedded table still describes the August 20 Image Edit contract as prompt-only and omits Segment Edit. The configured release reproduces the current split through its live catalog, but no test carries a Segment Map result into Segment Edit task creation.
  - Impact: a caller follows the MCP's own instruction, sends Segment Edit fields to another model contract, and can receive KIE's pre-task validation error `This field is required`.
  - Recommendation: name the canonical Segment Edit model everywhere; add its compatibility profile; update Image Edit's compatibility profile to the current optional-prompt, `image_urls`, and `aspect_ratio` contract; add a full mock regression from Segment Map output to validated Segment Edit task creation; assert the public tool description.
  - Safe to fix now: yes — the user explicitly requested this correction after the audit findings are recorded.
  - Verification: MCP tool-description assertion, mock two-step flow, full project gates, and read-only live schema lookup.
  - Resolution: `src/mcp.rs`, `README.md`, and `docs/DEVELOPMENT.md` now direct Segment Map output only to `grok-imagine-image-2-0/segment-edit`. The embedded catalog and reviewed snapshot now contain Segment Edit and the corrected, separate Image Edit profile. `tests/kie_mock.rs` carries a real parsed `task_id` and selected index `1` into Segment Edit, asserts the exact payload, and proves the authoritative fixture rejects a missing `task_id` before task creation. `tests/mcp_stdio.rs` locks the public instruction.

- [x] DS-20260828-002 — Internally implausible live schemas are still marked authoritative
  - Status: resolved in `4a25d19`
  - Confidence: high
  - Blast radius: subsystem
  - Location: `src/kie/catalog/live.rs:765`, `src/kie/catalog/live.rs:782`, and `src/kie/catalog/validation.rs:34`
  - Evidence: authority currently requires only an exact model ID and an object-shaped input. Both newly published Wan 3.0 routes pass those checks, but KIE's OpenAPI types `first_frame_url` and `last_frame_url` as objects and each item of `reference_image_urls`, `reference_video_urls`, and `reference_audio_urls` as an object. The field names, descriptions, and examples identify URL strings. Because the routes are live-only and authoritative, the local validator rejects those documented string URLs before any remote request.
  - Impact: `wan/3-0-video` and `wan/3-0-video-prime` can be exposed as authoritative while their normal media inputs are unusable through the MCP. The same trust rule can turn future upstream schema-generation mistakes into local false rejections.
  - Recommendation: add generic schema-coherence checks that downgrade suspect contracts to advisory status, retain a warning in `kie_models`, and cover malformed URL schemas with parser/validation fixtures. Do not special-case Wan.
  - Safe to fix now: completed after the user chose advisory schemas.
  - Verification: malformed-schema fixture, read-only live catalog query, and request-construction tests proving documented strings are not falsely blocked.
  - Resolution: the parser validates each published request example against the extracted input schema. A mismatch keeps the schema visible, records `schema_warning`, marks it informational, and skips local rejection. Wan 3 and Wan 3 Prime also have stable embedded request profiles without a guessed multimodal media binding.

### Medium cleanup

None confirmed beyond the test and compatibility-profile gaps grouped with DS-20260828-001.

### Nice-to-have polish

None confirmed.

## Needs confirmation from KIE

- Both current English and Chinese Segment Map documents return mask indexes `0..10`. Both current Segment Edit documents set `mask_indexs.items.minimum: 1` and use `[1, 2]` in their examples. KIE's public model form does not state whether index `0` is accepted. The contradiction is therefore proven, but the backend rule cannot be established without KIE confirmation or a prohibited remote task-creation attempt. The MCP continues to follow the machine-readable minimum `1`; no Grok-only override was added.
- Both current English and Chinese OmniHuman documents say "recommended 300, maximum 1000" while setting `maxLength: 300`. KIE's public model form only says "recommended <=300" and does not claim support for 301..1000 characters. The MCP conservatively keeps the machine-readable limit of 300. Whether the backend accepts more requires KIE confirmation or a prohibited remote task-creation attempt.

## Completed

- DS-20260828-003 — Walked all 128 live `input_schema` values recursively: 1,207 schema nodes, 938 properties, 325 required declarations, 261 enums, 377 defaults, 86 numeric-bound nodes, 160 string-bound nodes, 96 array-bound nodes, and 11 `oneOf`/`anyOf` alternatives remain represented. `kie_models` is not generically flattening these structures; descriptions/examples are only removed unless explicitly requested.
- DS-20260828-004 — Compared 125 embedded routes with authoritative live contracts and manually sampled every model family. Apart from the Grok split and the upstream issues recorded here, the generic media bindings, prompt policies, aliases, cardinalities, enums, defaults, resolutions, ratios, qualities, and nested schemas were preserved or deliberately left in raw `input` for complex model-specific fields.
- DS-20260828-005 — Verified Gemini Omni Audio/Character and the Grok/OmniHuman preparation tools against their dedicated current documents. Endpoint paths, required fields, payload keys, file limits, reusable identifiers, typed intermediate results, and downstream task relationships match. No other stale cross-tool route instruction was found.
- DS-20260828-006 — Verified the live parser discovered 135 indexed routes, loaded 131 schemas, and exposed 130 final image/video models. The live-only final models are Segment Edit and the two Wan 3.0 routes; no embedded final model is absent from the merged catalog.
- DS-20260828-007 — Verified the main-checkout release path, commit, timestamp, and checksum. The current stale Segment Map text is a source defect present in the fresh main build, not an obsolete-worktree loading artifact.
- DS-20260828-008 — Resolved the Ideogram naming question. The current English and Chinese contracts both type `reference_mask_urls` as one string, explain that only one mask is supported and extra masks are ignored, and show `reference_mask_urls: ""`. The plural field name is awkward, but the contract itself is consistent; no MCP change is needed.

## Rejected / noise

- KIE's Qwen2 Text to Image page currently declares the Image Edit model ID, and its Kling V2.5 Turbo Image to Video page declares Kling V2.1 Master. The parser rejects both identity conflicts and retains the embedded fallback, so these upstream copy errors do not corrupt the merged catalog.
- KIE's HappyHorse and Seedance pages contain a few property names with trailing spaces. The compatibility profiles follow the unspaced names used by the same pages' examples; no new MCP workaround is warranted without evidence that KIE accepts the spaced spelling.
- Seedance 2-family schemas omit root `required: [prompt]` while their property descriptions state that prompt is required. Keeping the embedded required-prompt policy was not treated as loss of contract data.

## Post-audit remediation verification

- Product files changed: `src/kie/catalog/models.rs`, `src/kie/catalog.rs`, `src/mcp.rs`, `README.md`, and `docs/DEVELOPMENT.md`.
- Regression files changed or added: `tests/catalog_contract.rs`, `tests/fixtures/kie_catalog_contract.json`, `tests/fixtures/kie_catalog_index.txt`, `tests/fixtures/kie_grok_segment_edit_route.md`, `tests/kie_mock.rs`, and `tests/mcp_stdio.rs`.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- `cargo test --all-targets --all-features`: passed — 103 tests total (66 library, 35 mock integration, one catalog snapshot, one MCP stdio).
- `git diff --check`: passed.
- `cargo build --release --offline`: passed from `/Users/pym/www/Git/GitHub/pym/kie-mcp`, whose HEAD is `23dfb25e436d4a005195e49cabbb42ae1f92bd9b`; the release includes the listed uncommitted remediation files.
- Main-checkout release: `/Users/pym/www/Git/GitHub/pym/kie-mcp/target/release/kie-mcp`, built 2026-08-28T01:37:34+02:00, 11,905,152 bytes, SHA-256 `d4e62ea3f813f642a9b990a64c84971f059eedebed61472fed7c6c685ce26266`.
- Temporary-worktree release remains 11,905,024 bytes with SHA-256 `99c83960529e923646bee37991b91da850849730df294f8ef72f3ac3c47adf22`; it is now demonstrably different from the corrected main build. Codex configuration points to the corrected main path.
- The corrected release's embedded fallback exposes both `grok-imagine-image-2-0/segment-edit` and the separate `grok-imagine-image-2-0/image-edit` profiles. Two final live-refresh attempts fell back after a request error even though a simultaneous `curl -I` returned HTTP 200; no paid endpoint was called. The same current live documents had already been captured and queried successfully before remediation, and the exact Segment Edit contract is exercised locally through the authoritative renamed-route fixture.

---

# Deep post-remediation audit

Project: kie-mcp
Scope: full repository and the Segment Edit, Wan 3, OmniHuman, and structured task recovery changes through `c8e93ca`, followed by approved remediation in `682a81d`
Mode: deep
Scan started: 2026-08-28T02:10:35+02:00
Scan completed: 2026-08-28T02:16:40+02:00
Scan status: complete
Baseline: c8e93caaaa6413479d3e1e904d11d3afef7033b9
Working tree: dirty only for this audit artifact and the preserved pre-existing untracked `output/`
Product-code changes: four approved audit fixes committed in `682a81dcf8449a558106ae5cebb203fce87792d4`
Checks run: `cargo fmt --all -- --check`, strict Clippy, all-target/all-feature tests, `git diff --check`, read-only full live catalog load, and offline release build all passed

## Top next actions

None. Every confirmed finding from this scan was fixed and verified in `682a81d`.

## Active backlog

### Critical

None confirmed.

### Medium cleanup

None.

### Nice-to-have polish

None.

## Needs confirmation

None.

## Completed

- [x] DS-20260828-009 — Segment Map now states the Segment Edit index floor
  - Status: resolved in `682a81d`
  - Confidence: high
  - Blast radius: subsystem
  - Location: `src/mcp.rs`, `src/kie/operations.rs`, `README.md`, and `docs/DEVELOPMENT.md`
  - Resolution: the public tool description, returned Segment Map Markdown, and workflow docs now say that the current Segment Edit schema accepts indexes of `1` or greater and rejects `0`, even though Segment Map can return it. Validation remains driven by the downloaded schema.
  - Verification: the structured-result unit test checks the warning, the MCP stdio test checks the tool description, and the full flow rejects index `0` before task creation.

- [x] DS-20260828-010 — `kie_models` text now exposes informational schema warnings
  - Status: resolved in `682a81d`
  - Confidence: high
  - Blast radius: subsystem
  - Location: `src/kie/catalog/live.rs`, `src/mcp.rs`, and `tests/mcp_stdio.rs`
  - Resolution: each text model line includes `authoritative`, `informational`, or `unavailable`; informational lines include the exact `schema_warning`. The MCP test checks both structured JSON and text against the Wan contradiction fixture.
  - Verification: `cargo test --test mcp_stdio` and the full suite passed.

- [x] DS-20260828-011 — Both OpenAPI request-example forms have local regressions
  - Status: resolved in `682a81d`
  - Confidence: high
  - Blast radius: subsystem
  - Location: `src/kie/catalog/live.rs` and `tests/kie_mock.rs`
  - Resolution: the parser unit suite covers direct `application/json.example`; the Wan integration covers named `application/json.examples` and proves a documented URL string reaches the mock create route.
  - Verification: both focused tests and the full suite passed.

- [x] DS-20260828-012 — Stdio test children are killed on drop
  - Status: resolved in `682a81d`
  - Confidence: high
  - Blast radius: local
  - Location: `tests/mcp_stdio.rs`
  - Resolution: both commands set `kill_on_drop(true)` and retain explicit cleanup on the successful path.
  - Verification: MCP stdio tests and strict Clippy passed.

- DS-20260828-002 is resolved by `4a25d19`: documented examples that fail their own input schema now downgrade the contract generically to informational, retain a warning, and skip false local rejection. No Wan-specific validator exception was added.
- DS-20260820-001 is resolved by `c8e93ca`: the public `kie_task_status` tool now has stdio coverage for typed Segment Map recovery with preview download both disabled and enabled.
- The current public catalog still has SHA-256 `1f2e479b07265e9bd5b422e00009aa929fe6a8b5f1ff8c74a02a1cd56668bd01`. The embedded snapshot therefore still points to the reviewed catalog version.
- A read-only full live load exposed 130 final models: 123 authoritative, five informational, and two embedded fallbacks for KIE pages whose declared model IDs conflict with their route identities. No paid endpoint was called.
- Both Wan 3 routes expose their contradictory URL schemas as informational with the exact failing example and accept documented URL strings locally. OmniHuman remains authoritative with `prompt.maxLength: 300`.
- Final gates passed: 68 library tests, 37 mock integration tests, two MCP stdio tests, and one catalog snapshot test, for 108 tests total. Formatting, strict Clippy, and `git diff --check` also passed.
- `cargo build --release --offline` completed from the main checkout at `682a81dcf8449a558106ae5cebb203fce87792d4`. `/Users/pym/www/Git/GitHub/pym/kie-mcp/target/release/kie-mcp` is 11,933,408 bytes, built at 2026-08-28T02:16:19+02:00, with SHA-256 `5cbbdfdf7b03ef4142e521e5aedabc731d978b6c5e3bda1a3abb3a1578b70f4a`.
- Codex configuration points to that main-checkout binary. The old temporary-worktree artifact remains 11,905,024 bytes with SHA-256 `99c83960529e923646bee37991b91da850849730df294f8ef72f3ac3c47adf22`, so it is neither selected nor byte-identical to the release just validated.

## Rejected / noise

- The three additional informational routes are not false positives. `grok-imagine/text-to-video` documents a string `duration` against a numeric schema; `grok-imagine/extend` documents a string `extend_times` against a numeric schema; and `happyhorse/image-to-video` uses `image_urls` in its request example while its schema requires the trailing-space property `image_urls `. Downgrading them is the intended generic safety behavior.
- Leaving Wan 3 media on raw `input` is deliberate. Its first/last-frame, multimodal reference, file, and link modes are mutually exclusive and cannot be represented truthfully by one top-level media shortcut.
- The backend behavior for Grok index `0` and OmniHuman prompts longer than 300 characters remains unknowable without a KIE answer or prohibited task creation. This no longer blocks the MCP policy: it follows the current machine-readable limits and explains them to callers.
