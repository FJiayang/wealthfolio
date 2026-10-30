# Initial Linux CI baseline review

Reviewed on 2026-10-30 by a separate source/pixel reviewer and the integration
coordinator. Architecture impact: snapshots only; no application behavior, test
assertions or comparison tolerances change.

- Candidate artifact: [12966610621](https://github.com/FJiayang/wealthfolio/actions/runs/41028644453/artifacts/12966610621), job 123973326770.
- Artifact ZIP SHA-256: `07b8abc406d0b5b6ba05aa59489d3870736c68320fd9b75870d1cab766d72f6a`.
- Captured source: `d65bc6883bda897c5a6a09c9e41fffcf598e1a8d`.
- PR head: `968cb86bbaca95cc49fd889d1c540f6963ba985c`.
- PR base: `a9d39b7554c6b2342e178f681d9bb4fb44149824`.
- Source and head have the same tree: `9eae6d743a343aae7ad412b50fea63a08d3c6839`.
- Image: `mcr.microsoft.com/playwright:v1.58.2-noble`, resolved digest
  `sha256:6446946a1d9fd62d9ae501312a2d76a43ee688542b21622056a372959b65d63d`.

The candidate ran 666 tests with one worker and zero retries. Generation passing
is not acceptance: the next ordinary CI run must compare these committed files
with snapshot updates disabled.

The 238 files are copied byte-for-byte from that artifact: each browser has
108 PNGs and 11 accessibility JSON summaries. All 216 images were reviewed in
contact sheets, with full-resolution gallery, mobile RTL, dialog/select and
cyber/newspaper/poster/terminal menu samples. Both engines, light/dark, classic
LTR/RTL desktop/mobile, and the additional theme variants are represented.
The 22 empty JSON objects mean zero violations in the existing axe rule scans;
they are not accessibility-tree snapshots or proof of complete accessibility.
RTL fixtures contain English text, not CJK or translated application content.

Against 38 independently captured upstream `2f234185` reference images, all
image dimensions match and all 19 WebKit images are pixel-identical. The 19
Chromium images have cross-environment raster differences; they are not claimed
pixel-equivalent. The upstream gallery fixture, theme/global styles and sampled
dialog/select implementations are unchanged. The upstream-added Change lines
section and new themes are intentional, not removals of earlier coverage.
No application-wide visual equivalence is inferred from this synthetic gallery.

Only PNG/JSON expectations and this record are committed. Artifact reports,
traces, credentials and machine-local snapshots are excluded.
