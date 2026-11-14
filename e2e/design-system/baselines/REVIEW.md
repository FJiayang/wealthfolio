# Initial Linux CI baseline review

## Upstream 3.13 and Playwright 1.65 — 2026-11-13

Independent reviewers `/root/baseline_review`, `/root/overlay_review`, and
`/root/gallery_review` were authorized by the user. The successful
[candidate run](https://github.com/FJiayang/wealthfolio/actions/runs/43238783694)
used source `7924d22df0506d59b2acb6156213971d1a6c0039` and image
`mcr.microsoft.com/playwright:v1.65.0-noble`. Its 216 PNGs and 22 accessibility
JSON files match the earlier candidate inspected by the reviewers byte for byte.

The reviewers inspected the gallery, themes, dialogs, sheets and overlays in
both browsers, light and dark modes, and LTR and RTL. They accepted 208 changed
PNGs; eight Chromium desktop light dialog/sheet images remain at their existing
bytes because ordinary CI did not report them as failing. All accessibility JSON
files remain unchanged. Mobile control height and bottom-sheet boundaries follow
upstream styles. A same-source comparison using the old and new Playwright
images reproduced the observed font and line-wrap differences; it does not
identify the specific browser or system-font component responsible.

Architecture impact: expectations only; assertions, tolerances and retries are
unchanged. Ordinary CI must verify the updated revision with updates disabled.

## Upstream 3.12 Signature buttons — 2026-11-04

Independent reviewer: `/root/baseline_review`, separately authorized by the
user. Candidate artifact:
[13305434678](https://github.com/FJiayang/wealthfolio/actions/runs/41795112488/artifacts/13305434678).
Source: `e184d732c53d938cc63904988d188b1871dbd012`. Image:
`mcr.microsoft.com/playwright:v1.58.2-noble`. The tested PR merge
`8f446114b9ef8a7647851390148db4d10039f19e` has the same tree as this source:
`95ec0da4e870f1b606380d4f27f0e957ffe929d3`.

The reviewer inspected all eight expected/actual/diff comparisons and verified
that the candidate contains the complete 238-file inventory. Only eight PNGs
change: `theme-signature-{dark,light}-1440.png` and
`theme-signature-menu-{dark,light}-1440.png`, in both Chromium and WebKit. All
other 230 files, including all 22 accessibility JSON summaries, remain
byte-identical. Each accepted candidate matches the independently reviewed CI
actual image byte-for-byte. Differences are limited to upstream's destructive
button fill, border, shadow and exposed edges behind the menu. The relevant
theme tokens and button implementation match upstream exactly.

Architecture impact: expectations only; assertions, tolerances and retries are
unchanged. Ordinary CI must compare the new revision with updates disabled.

## Original bootstrap review

Reviewed on 2026-10-30 by a separate source/pixel reviewer and the integration
coordinator. Architecture impact: snapshots only; no application behavior, test
assertions or comparison tolerances change.

- Candidate artifact:
  [12966610621](https://github.com/FJiayang/wealthfolio/actions/runs/41028644453/artifacts/12966610621),
  job 123973326770.
- Artifact ZIP SHA-256:
  `07b8abc406d0b5b6ba05aa59489d3870736c68320fd9b75870d1cab766d72f6a`.
- Captured source: `d65bc6883bda897c5a6a09c9e41fffcf598e1a8d`.
- PR head: `968cb86bbaca95cc49fd889d1c540f6963ba985c`.
- PR base: `a9d39b7554c6b2342e178f681d9bb4fb44149824`.
- Source and head have the same tree:
  `9eae6d743a343aae7ad412b50fea63a08d3c6839`.
- Image: `mcr.microsoft.com/playwright:v1.58.2-noble`, resolved digest
  `sha256:6446946a1d9fd62d9ae501312a2d76a43ee688542b21622056a372959b65d63d`.

The candidate ran 666 tests with one worker and zero retries. Generation passing
is not acceptance: the next ordinary CI run must compare these committed files
with snapshot updates disabled.

The 238 files are copied byte-for-byte from that artifact: each browser has 108
PNGs and 11 accessibility JSON summaries. All 216 images were reviewed in
contact sheets, with full-resolution gallery, mobile RTL, dialog/select and
cyber/newspaper/poster/terminal menu samples. Both engines, light/dark, classic
LTR/RTL desktop/mobile, and the additional theme variants are represented. The
22 empty JSON objects mean zero violations in the existing axe rule scans; they
are not accessibility-tree snapshots or proof of complete accessibility. RTL
fixtures contain English text, not CJK or translated application content.

Against 38 independently captured upstream `2f234185` reference images, all
image dimensions match and all 19 WebKit images are pixel-identical. The 19
Chromium images have cross-environment raster differences; they are not claimed
pixel-equivalent. The upstream gallery fixture, theme/global styles and sampled
dialog/select implementations are unchanged. The upstream-added Change lines
section and new themes are intentional, not removals of earlier coverage. No
application-wide visual equivalence is inferred from this synthetic gallery.

Only PNG/JSON expectations and this record are committed. Artifact reports,
traces, credentials and machine-local snapshots are excluded.
