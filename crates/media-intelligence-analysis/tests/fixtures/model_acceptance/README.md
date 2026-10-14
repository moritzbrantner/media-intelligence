# Model-backed acceptance fixture (#34) — attribution

`wcc-excerpt.webm` is a short, re-encoded excerpt of a Creative Commons video. It is the
input of the opt-in model-backed acceptance test `tests/model_backed_acceptance.rs` and of
the cheap fixture checks in `tests/model_acceptance_fixture.rs`.

## Source and licence

| | |
|---|---|
| Title | Wikimedia Core Curriculum - Initial steps |
| Author | Aespinoza09 |
| Source | <https://commons.wikimedia.org/wiki/File:Wikimedia_Core_Curriculum_-_Initial_steps.webm> |
| File | <https://upload.wikimedia.org/wikipedia/commons/7/76/Wikimedia_Core_Curriculum_-_Initial_steps.webm> |
| Licence | Creative Commons Attribution 4.0 International (CC BY 4.0), <https://creativecommons.org/licenses/by/4.0/> |
| Original | 139.683 s, 1920x1080 VP8 25 fps + mono Vorbis 44.1 kHz, 7 599 961 bytes |
| Original sha256 | `29af9da70d0dee89b1f606dd4cd32e1c074c1a2e1aa0dda45c9bb6e6bbbadec4` |

The source description states that the narration is an AI-generated voice reading the
full-screen slides.

## Modifications

**Excerpt, re-encoded.** Seconds 7.9 to 39.9 of the original were cut out, scaled to
960x540 and re-encoded (VP9 video, mono Vorbis 22.05 kHz audio); container metadata was
dropped. Content was not otherwise altered. Command (ffmpeg 8.0.1, Ubuntu build
`8.0.1-3ubuntu2`; re-encoding is not bit-reproducible across ffmpeg/libvpx versions, so the
committed bytes and the hash below are authoritative):

```sh
ffmpeg -hide_banner -loglevel error -y -ss 7.9 -i wcc.webm -t 32 \
  -map 0:v:0 -map 0:a:0 -vf scale=960:540 -r 25 \
  -c:v libvpx-vp9 -crf 38 -b:v 0 -row-mt 1 -threads 2 -g 250 \
  -c:a libvorbis -q:a 2 -ac 1 -ar 22050 -map_metadata -1 wcc-excerpt.webm
```

| | |
|---|---|
| Excerpt | `wcc-excerpt.webm`, 465 062 bytes, 32.012 s (container), 800 video frames at 25 fps |
| Excerpt sha256 | `c954e099a777f09b95ecd071146567672c4e25ea83448600adf6f2ea7240db8d` |

## What is on screen (observed by the acceptance author)

Observed from 1 fps contact sheets and `ffmpeg select='gt(scene,0.05)'` change points of the
excerpt (excerpt time, seconds). There is no reference transcript; the narration was not
listened to. Audio RMS is above -25 dBFS in every second of the excerpt except second 11
(-35 dBFS), i.e. near-continuous narration.

| Excerpt time | Slide | On-screen text |
|---|---|---|
| 0.00 – 4.77 | Benefits | "Benefits of Creating an Account"; "Having an account allows you to save your work and join the global Wikipedia community." |
| 4.77 – 12.17 | Step 1 | "Step 1: Go to Wikipedia & Click 'Create Account'"; address bar "en.wikipedia.org" (4.77 – 8.1); then a Wikipedia main-page screenshot (transition 8.09 – 8.41) |
| 12.17 – 19.89 | Step 2 | "Step 2: Fill out the Form"; screenshot of the "Create account" form (scrolls at about 17.17) |
| 19.89 – 32.01 | Step 3 | "Step 3: Choosing the right username"; "Create account", "Username" screenshot (transition 19.89 – 20.41); animated illustrations; "Dos" appears at about 29.81 |

Every slide also carries a small "WIKIMEDIA FOUNDATION" footer logo.
