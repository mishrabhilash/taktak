# Office Classic: sources

Everything in this pack comes from one CC0 recording set, cut and levelled as described below.
To rebuild it byte for byte (DOWNLOADS is any folder; `fetch.sh` puts the zip in it and checks
its SHA-256):

```
tools/pack-sources/office-classic/fetch.sh DOWNLOADS/unicaegames
python3 tools/pack-sources/office-classic/build.py DOWNLOADS/unicaegames/unicae_games_keyboard_soundpack_1_0.zip packs/office-classic
```

`build.py` uses only the Python standard library (it reads the WAVs straight from the zip)
and writes every file in this folder, including this one.

## Source

| | |
|---|---|
| Work | "Keyboard Soundpack #1 [Typing and Single Keystrokes]" |
| Author | unicaegames |
| Source page | https://opengameart.org/content/keyboard-soundpack-1-typing-and-single-keystrokes |
| Direct download | https://opengameart.org/sites/default/files/unicae_games_keyboard_soundpack_1_0.zip |
| Downloaded file | `unicae_games_keyboard_soundpack_1_0.zip`, 8,515,609 bytes |
| SHA-256 of the download | `935eae2fa5c3742eacdd38c4ea0e9047f3887faa0701996498492c256db1b351` |
| License | CC0-1.0 (Creative Commons Zero v1.0 Universal) |
| License proof | the source page's License(s) field reads "CC0" and links to https://creativecommons.org/publicdomain/zero/1.0/ |
| Date checked | 2026-10-02 |
| Recording, per the author | keyboard: Cherry KC 1000 (a rubber-dome / membrane office board); microphone: Shure SM7B; post-processed with iZotope Neutron 2 |

License evidence, checked on 2026-10-02:

- The OpenGameArt page lists the license as CC0 (Creative Commons Public Domain Zero 1.0).
  The page was submitted on 2021-02-12; the author's 2026-01-27 update only fixed a Patreon
  link in the description and readme.
- The zip's `readme.txt` (dated 2026-02-01 inside the zip) says the sounds are free to use
  however you like. It adds no conditions that conflict with CC0.
- CC0 needs no attribution. The pack credits the author anyway: "Keyboard sounds by
  unicaegames (CC0)".

The zip contains 32 single keystrokes (`Single Keys/`), 10 human typing takes
(`Human Typing/`), 7 generated typing takes (`Generated Typing/`), `preview.ogg` and
`readme.txt`. It contains no executables.

## What is used, and why

Only files in `Single Keys/` are used: 31 of its 32. Each is a 250 ms, 44.1 kHz, 16-bit
mono WAV that holds one complete tap: the press, then, 40–125 ms later, the quieter sound of
the key coming back up. The build splits every tap into a press sample and a release sample,
so key-down and key-up each get the sound the keyboard really made. No release is synthesized.

Not used:

- `Single Keys/keypress-021.wav`: the same recording as `keypress-016.wav`, placed 20.11 ms
  earlier in its file. The two files differ byte for byte, but their waveforms around the
  press peak correlate at 0.99999 (distinct keystrokes of this set stay below 0.6), so using
  both would play that one stroke twice as often as any other. `build.py` checks this on
  every build, and stops if two keystrokes it uses correlate at 0.95 or more.
- `Human Typing/`: continuous typing in which keystrokes overlap, so clean single strokes
  cannot be cut from it without bleed from neighbouring keys.
- `Generated Typing/`: sequences assembled from recorded keystrokes; nothing new.
- `preview.ogg`: lossy, and a sequence.

The source does not say which key each recording is, so there are no per-key, space or Enter
sounds. Every key uses the pool of 31 presses and 31 releases in
`groups.alphanumeric`. Pack files keep the number of their source file, so the numbers of the
keystrokes that are not used are missing.

## Processing

For each source file, in this order (all positions are sample indices at 44,100 Hz into the
original file):

1. Subtract the file's mean (DC), then a 2nd-order Butterworth high-pass at 30 Hz (causal).
2. Find the press onset and the release onset: coarse search on a 1 ms energy envelope of a
   150 Hz high-passed copy (detection only), then refined to the first sample that rises
   clearly above the noise (press) or above the decaying press tail (release).
3. Press sample: from the press onset to 0.5 ms before the release onset, or earlier if the
   press has already decayed into the noise floor.
4. Release sample: from the release onset to the end of the last 1 ms block that is more than
   3 dB above the file's noise floor (measured before the press).
5. One gain for the whole pack: **+1.48 dB**. The median press peak was
   -10.1 dBFS and the loudest sample in the pack -2.5 dBFS. Bringing
   the median press to -6 dBFS would have clipped the hardest taps, so the gain stops where the
   loudest sample reaches -1 dBFS. There is no per-file normalization: the natural
   differences between soft and hard taps (about 18 dB of press peak level)
   are kept.
6. If an onset is still below -50 dBFS after the gain (a few releases swell in gently), it
   moves to the first sample that reaches -50 dBFS, since anything before that is leading
   silence that the loader would trim anyway. Each sample then starts 0.5 ms (22 samples)
   before its onset.
7. Fades: a raised-cosine fade-in over the 0.5 ms pre-roll, and an 8 ms raised-cosine fade-out
   that ends on an exact zero.
8. Written as 16-bit mono WAV at 44,100 Hz, the source rate (no resampling, no dither).
9. Loudness: `volume` in `pack.json` is **0.94** (-0.5 dB), a playback gain the
   app applies to the whole pack, so no audio file changes. It matches typing loudness across
   the bundled packs, so switching packs does not jump in volume; the reference is the
   synthesized packs' -25.8 LK. Measured with
   `cargo run -p synth-packs --release -- loudness packs/office-classic`
   (`tools/synth-packs/src/loudness.rs`): the K-weighted (ITU-R BS.1770) energy of the first
   100 ms of the sample each alphanumeric key plays on press by default, power-averaged over
   the keys, on the samples as the app loads them at 48 kHz. The pack reads -25.6 LK as built and -26.2 LK
   at this volume. This volume also keeps the loudest true peak (4x oversampled, as resampling
   to the device rate can peak between samples) at or below -1 dBFS at the top of the ±5 %
   volume variation (the mixer clips hard at 0 dBFS). `build.py` writes the volume (`VOLUME`),
   so a rebuild keeps it.

| Original file | SHA-256 of original | Press file | Press slice, samples (ms) | Release file | Release slice, samples (ms) | Noise floor dBFS (RMS) |
|---|---|---|---|---|---|---|
| `Single Keys/keypress-001.wav` | `7b873548c399b9aa0efbe63052fe1344c5317a9b23455b8be3523f47d434daf1` | `press-01.wav` | 1565–5195 (35.49–117.80 ms) | `release-01.wav` | 5686–10328 (128.93–234.20 ms) | -57.6 |
| `Single Keys/keypress-002.wav` | `f58f93430c1ad434feb341a89b89740ccad75e7a775d778d91b7f206e0acbe8e` | `press-02.wav` | 1668–4994 (37.82–113.24 ms) | `release-02.wav` | 5001–8976 (113.40–203.54 ms) | -57.9 |
| `Single Keys/keypress-003.wav` | `d67cf9547d99fc65c815c4c68fa9181e0af71419761cbb1d1f0a10eb8491a8d2` | `press-03.wav` | 521–3267 (11.81–74.08 ms) | `release-03.wav` | 3267–8349 (74.08–189.32 ms) | -57.0 |
| `Single Keys/keypress-004.wav` | `29e1aea1fb841a0663ce75e9d83e675ddc7ae5cef00c623bdda65a72604fde7e` | `press-04.wav` | 423–4229 (9.59–95.90 ms) | `release-04.wav` | 4246–8184 (96.28–185.58 ms) | -55.3 |
| `Single Keys/keypress-005.wav` | `619eb7e26571657b76fc9e1e3d99e85c3fc2259fa91c4c45f57384a85774ac24` | `press-05.wav` | 1111–5790 (25.19–131.29 ms) | `release-05.wav` | 5790–8188 (131.29–185.67 ms) | -56.8 |
| `Single Keys/keypress-006.wav` | `419bde001e7ed1cae63b84f60c803fb024a0254e1afe65e59d8c6d8ad4196eb7` | `press-06.wav` | 345–3762 (7.82–85.31 ms) | `release-06.wav` | 3762–7656 (85.31–173.61 ms) | -52.2 |
| `Single Keys/keypress-007.wav` | `522834f471e6c2935fd1e6a28042c0864925556efbec32c36d7c7a98026d4bfc` | `press-07.wav` | 169–2949 (3.83–66.87 ms) | `release-07.wav` | 2949–5875 (66.87–133.22 ms) | -53.8 |
| `Single Keys/keypress-008.wav` | `c2ab8722a81a2f1cab66cd863a09a9ec00392e2ede61362b3707a7bcc0eee506` | `press-08.wav` | 2333–5428 (52.90–123.08 ms) | `release-08.wav` | 5428–9234 (123.08–209.39 ms) | -55.4 |
| `Single Keys/keypress-009.wav` | `25595c1d1e0e18aada04dce3a651dc2c499f1fbc2647c1ab0b3c4e718275b16b` | `press-09.wav` | 2055–5949 (46.60–134.90 ms) | `release-09.wav` | 6139–10692 (139.21–242.45 ms) | -55.1 |
| `Single Keys/keypress-010.wav` | `3a65bf93ec554b068b0a64d11d2bb521cd54e7a9e7ef3362a670efb1e410f0dc` | `press-10.wav` | 2393–5830 (54.26–132.20 ms) | `release-10.wav` | 5830–10296 (132.20–233.47 ms) | -58.1 |
| `Single Keys/keypress-011.wav` | `72e79b249040bff166be863dd75f5c50d6e22bf904bd7e0adab1b30941e19968` | `press-11.wav` | 1985–5614 (45.01–127.30 ms) | `release-11.wav` | 5614–8980 (127.30–203.63 ms) | -57.0 |
| `Single Keys/keypress-012.wav` | `79de7a9cc035ca4204600a52281129be9f258b1d903c88279b1e7128e57ba86c` | `press-12.wav` | 662–3676 (15.01–83.36 ms) | `release-12.wav` | 4108–9636 (93.15–218.50 ms) | -55.6 |
| `Single Keys/keypress-013.wav` | `a73d12a22403fe7b63a3da41ff07f13f956e3a30b397196288262d467f1e28c3` | `press-13.wav` | 522–3910 (11.84–88.66 ms) | `release-13.wav` | 3910–7496 (88.66–169.98 ms) | -55.6 |
| `Single Keys/keypress-014.wav` | `b2069e29be488a7e55b151ef458e3314ec8f138f6d5a34fe19189f4810cba098` | `press-14.wav` | 510–3520 (11.56–79.82 ms) | `release-14.wav` | 3520–8558 (79.82–194.06 ms) | -57.2 |
| `Single Keys/keypress-015.wav` | `39df76e16d6daa76619f17b724ed4fdcfacc3d0c311103f6d45dc23ec3634678` | `press-15.wav` | 907–5065 (20.57–114.85 ms) | `release-15.wav` | 5229–9651 (118.57–218.84 ms) | -56.2 |
| `Single Keys/keypress-016.wav` | `181e3c5d28d2a1ed0e953f5668391ef1f9b7cd577d33058fb62bd18473b45791` | `press-16.wav` | 2899–7057 (65.74–160.02 ms) | `release-16.wav` | 8431–10345 (191.18–234.58 ms) | -58.6 |
| `Single Keys/keypress-017.wav` | `0f8d80d3f64328936bb699eade15cc89de12b7464833459e539bcf001101e431` | `press-17.wav` | 296–3728 (6.71–84.54 ms) | `release-17.wav` | 3728–7622 (84.54–172.83 ms) | -54.0 |
| `Single Keys/keypress-018.wav` | `784622e91e8797605fa76c43c0e4745663688c9faf455ff2a951d79f5f4dac65` | `press-18.wav` | 769–5394 (17.44–122.31 ms) | `release-18.wav` | 5394–7836 (122.31–177.69 ms) | -56.2 |
| `Single Keys/keypress-019.wav` | `9cdb32c0074eaa114f8740e80bdfc28b1b44b4c6789371e1ec872e185e92d49e` | `press-19.wav` | 1148–4172 (26.03–94.60 ms) | `release-19.wav` | 4172–7890 (94.60–178.91 ms) | -55.6 |
| `Single Keys/keypress-020.wav` | `056b14dea2cc590e817851d93f910b0ba918cd9179bbb4d2b39b4db92c0d505f` | `press-20.wav` | 706–3720 (16.01–84.35 ms) | `release-20.wav` | 5809–8295 (131.72–188.10 ms) | -51.8 |
| `Single Keys/keypress-022.wav` | `8cf602c150e338a94e9c19dcf68b1d80af5adb311c84ec20590a6ab0359b91c5` | `press-22.wav` | 1573–6215 (35.67–140.93 ms) | `release-22.wav` | 6552–9874 (148.57–223.90 ms) | -56.8 |
| `Single Keys/keypress-023.wav` | `b7395018288bc4842af0983a97d790e9db84ab65dc8ae57629dc6a9ba31ff62c` | `press-23.wav` | 2757–4642 (62.52–105.26 ms) | `release-23.wav` | 4642–11000 (105.26–249.43 ms) | -57.6 |
| `Single Keys/keypress-024.wav` | `39f3a508a9be3bbf31f630ccf9a9413f02068965babd3885771d6060f58b388e` | `press-24.wav` | 2698–5522 (61.18–125.22 ms) | `release-24.wav` | 5534–11000 (125.49–249.43 ms) | -58.8 |
| `Single Keys/keypress-025.wav` | `bd3f9c7f28df223592fd6b3ae3c8c6f6f0fa5d899e66492825ee7b5dd4a3a9dc` | `press-25.wav` | 2008–4582 (45.53–103.90 ms) | `release-25.wav` | 4583–9401 (103.92–213.17 ms) | -55.7 |
| `Single Keys/keypress-026.wav` | `a7016b0c501bcc73a190bb3783626ecc6c3086e9a935e13c3828d70854e4774e` | `press-26.wav` | 2449–5302 (55.53–120.23 ms) | `release-26.wav` | 5308–9856 (120.36–223.49 ms) | -57.4 |
| `Single Keys/keypress-027.wav` | `c0376d2249a2c6d4e79d166445bb83ca6efbdcc09518b8323499644e393dd5ab` | `press-27.wav` | 2061–5449 (46.73–123.56 ms) | `release-27.wav` | 5449–10179 (123.56–230.82 ms) | -58.0 |
| `Single Keys/keypress-028.wav` | `a324dd279d5422bc2328e49122cdd39d071faccaf109332602f6ede13686ada7` | `press-28.wav` | 715–4036 (16.21–91.52 ms) | `release-28.wav` | 4036–8106 (91.52–183.81 ms) | -55.5 |
| `Single Keys/keypress-029.wav` | `248b459e92dce812858aab2498546887957828007978fa29f481ac16d799a14b` | `press-29.wav` | 2370–4950 (53.74–112.24 ms) | `release-29.wav` | 4952–9108 (112.29–206.53 ms) | -58.4 |
| `Single Keys/keypress-030.wav` | `355fb80cc1a40165de17c7653eed643da30afbb87f444d74c2eb0119adeec4f5` | `press-30.wav` | 823–4862 (18.66–110.25 ms) | `release-30.wav` | 4862–9416 (110.25–213.51 ms) | -56.5 |
| `Single Keys/keypress-031.wav` | `df5718d60529e81769597226c5bce593b3a6fd6ae7aeb9b7249754059d0c464a` | `press-31.wav` | 1886–4290 (42.77–97.28 ms) | `release-31.wav` | 4290–10252 (97.28–232.47 ms) | -57.5 |
| `Single Keys/keypress-032.wav` | `569648b47017747070b6d2a6d640449030b1b70da717c1f774c76a32dd75fb6c` | `press-32.wav` | 2478–5390 (56.19–122.22 ms) | `release-32.wav` | 5390–10340 (122.22–234.47 ms) | -57.3 |

### preview.wav

Rendered by `build.py` from this pack's own samples after the gain, with a fixed rhythm
(random seed 20261002): 3 keys, space, 2 keys, space, 1 key, a short pause, Enter. Dwell
60–100 ms, 80–130 ms from each release to the next press, mixed by simple addition, 16-bit
mono at 44,100 Hz, 1702.38 ms long. Since the pack has no dedicated space or Enter
sounds, those strokes use the same pool as every other key.

Timeline: press-06 @ 0.00 ms, release-31 @ 95.90 ms, press-23 @ 214.04 ms, release-03 @ 274.58 ms, press-05 @ 398.48 ms, release-22 @ 476.46 ms, press-29 @ 566.87 ms, release-16 @ 642.02 ms, press-18 @ 726.17 ms, release-11 @ 811.54 ms, press-12 @ 892.36 ms, release-09 @ 960.91 ms, press-13 @ 1046.39 ms, release-29 @ 1142.06 ms, press-24 @ 1228.48 ms, release-20 @ 1291.47 ms, press-27 @ 1466.26 ms, release-25 @ 1563.13 ms.

## Statistics

| Group | Count | Mean duration ms | Peak dBFS (max) | Peak dBFS (median) | RMS dBFS (mean) | Leading silence ms (max) |
|---|---|---|---|---|---|---|
| alphanumeric.press | 31 | 75.8 | -1.0 | -8.6 | -27.7 | 0.50 |
| alphanumeric.release | 31 | 94.4 | -5.8 | -15.8 | -35.8 | 0.50 |
| preview | 1 | 1702.4 | -2.4 | -2.4 | -29.6 | 0.48 |

Leading silence is measured as in `docs/pack-format.md`: time before the first sample at or
above -50 dBFS.

## Shipped files

SHA-256 of every audio file as shipped. `press-NN.wav` and `release-NN.wav` both come from
`Single Keys/keypress-0NN.wav` (table above); `preview.wav` is mixed from them. A rebuild
with `build.py` from the same zip reproduces them byte for byte, and removes any other WAV in
`sounds/`.

| File | Bytes | SHA-256 |
|---|---|---|
| `sounds/press-01.wav` | 7304 | `749fa0913d9d9be1a7882dee6a2cd21d7b2e9a804e31be9876798def59a3e714` |
| `sounds/press-02.wav` | 6696 | `9d94f6eeb8d077193a7ecd5e993e7e4740b4876ae687fcbe189d010bea3e5439` |
| `sounds/press-03.wav` | 5536 | `c0e666a3095cf94ef40b21e403bca63617483c38ee02aedebac2d064b0193e8a` |
| `sounds/press-04.wav` | 7656 | `848aadb1e185b073981294556d6e579f33fa243c1d31339718a4f6171a5724c0` |
| `sounds/press-05.wav` | 9402 | `565edbd55f31ce9d8c9e2fdf02cfda0616b4488f47d14cd9f6e9e7d861970575` |
| `sounds/press-06.wav` | 6878 | `d00210db247d460495fd43e5a835d4276a26803c8e7b712b361da5f9ba568bed` |
| `sounds/press-07.wav` | 5604 | `a7d7f7ce61bb767950b5027fb9f1280303d91b8dd7098917aa89a21b820783d6` |
| `sounds/press-08.wav` | 6234 | `a03fb2df1b63b9c162645e1a314941a91bd20c53eb466f318a55d771150e1f1a` |
| `sounds/press-09.wav` | 7832 | `2653b3ad618a10d2ef5727d3997faeece4b414f64ab21bf6fd4a8f9078c29035` |
| `sounds/press-10.wav` | 6918 | `acb8794961894b7d45147cf76186160d93e98407eb2edd6b0d384a4ed0fe7159` |
| `sounds/press-11.wav` | 7302 | `33b86b10601c5d765eeb3cd91a20f3b4c1b948e5282467b44018d2c03b2beb6d` |
| `sounds/press-12.wav` | 6072 | `2ee62a07cfe803194626d7083e80c60a481c916f3f76f7e95da56f3bd0ff0be7` |
| `sounds/press-13.wav` | 6820 | `bf623731079dd71f6877668e5a049f13b8e04353fbc4411325b7ca1e94d5d46b` |
| `sounds/press-14.wav` | 6064 | `c7fd959d4edb1fc94cf9c8185635bc56494180de379386e07a0f88abd537ce56` |
| `sounds/press-15.wav` | 8360 | `9b497e196544800875368791b749423832bb2fedc1e6a03b1ea4d54509de6e86` |
| `sounds/press-16.wav` | 8360 | `e9da894e968bf738a62e6e93e8a308028e4bf24b8e99841853eee1310ab0f61f` |
| `sounds/press-17.wav` | 6908 | `01c79f7bf876023384187b4abb9b6cb189561027e1d5b7529cf678c1f947fe33` |
| `sounds/press-18.wav` | 9294 | `50bd106c2493e4fc79df94eb666ded3bc37ce35cbf8476f1a7e8df0256aa0002` |
| `sounds/press-19.wav` | 6092 | `516f7213ec36bb2415bb5d60f56769fc4dccb3245a3a6a2325fb66ae97c1318c` |
| `sounds/press-20.wav` | 6072 | `3b98dfb6003187ae889a5d052bcbb9f3f53871c069365caae147dc021e85a705` |
| `sounds/press-22.wav` | 9328 | `9e74709b72e29a3c59c8bb4dfc02a47cbdf31380c6d3af15a0dcc1bfb6865172` |
| `sounds/press-23.wav` | 3814 | `621747d9f52807c02c77faa58eeb9c4bc9809afccf566f3dd153b292d3cc8c75` |
| `sounds/press-24.wav` | 5692 | `a724feee21de8af7fbfaad63e833ce7ffa83dcfe85a38e00476d6d774f290a4f` |
| `sounds/press-25.wav` | 5192 | `cddd11bceef69331eef6164ada18ea98f7792473e146449ec1a23dd5b7953383` |
| `sounds/press-26.wav` | 5750 | `73b29efa3d1aa9df67e1e19cba818b7d7f0497178d70e8ba7771c38d1adcd8e8` |
| `sounds/press-27.wav` | 6820 | `a9add99379de2fddd21b0318736965fed6a65456f6c94d69b5528698f8b89db3` |
| `sounds/press-28.wav` | 6686 | `03f641d39d7569cb8043497c2451e86447395687cd6d154f7c38116c2b512645` |
| `sounds/press-29.wav` | 5204 | `d68111858b6f63987ec85a98618e325cd37296e801aa1298860fadf185b9e424` |
| `sounds/press-30.wav` | 8122 | `fecc55884205221e42fa57c778d9677feba7194cb97b43d4470145d0a305b73d` |
| `sounds/press-31.wav` | 4852 | `3255d4fd89424556f608e096aca145c5bdb41ef58bd1857ed6ca69234847f86a` |
| `sounds/press-32.wav` | 5868 | `b5735f7ee37112f9f4ed1296c4b911d4b812cf1104499b0c051037556c506572` |
| `sounds/release-01.wav` | 9328 | `27b30da2cb4f6a451b8ec12718e8a9716a9c078bc4d69681844f9401d198e909` |
| `sounds/release-02.wav` | 7994 | `22b88977f6cc417bb193152835b91cf8b35477db6acc9d79e269dc40e9821f01` |
| `sounds/release-03.wav` | 10208 | `f580a6b70ef6e18b930f63499435b5e229147cc5d22d7a09e7d8d188a5625aeb` |
| `sounds/release-04.wav` | 7920 | `4958f0728f0436ccf0c4558e9350d43957dcad1c6b2a248963812c548fefa97a` |
| `sounds/release-05.wav` | 4840 | `1a8ed223a5da00135a5ca3d31f0b73b6532c172bc1adf177dde4c76893c4a4fb` |
| `sounds/release-06.wav` | 7832 | `04fd7c0c1363815943c9263a3a6a37e4c754deca296cf28c3667b10f7c8c7020` |
| `sounds/release-07.wav` | 5896 | `903ff36021aef15d3139467a5db334a675c4f8c978d4d175b433ca057cfb13cc` |
| `sounds/release-08.wav` | 7656 | `3c043ad80fabade639144805b25fb60a38f06f12399d21a78e83eda459f88b16` |
| `sounds/release-09.wav` | 9150 | `6049e5e5bfc5bf2f96924fc9a641510d3ef8b0bf7bebd2568e4750ad322e8523` |
| `sounds/release-10.wav` | 8976 | `e3027ba42a11398eea23b603fa9b803e926c0959aaab57607fa3add97206b223` |
| `sounds/release-11.wav` | 6776 | `f4d78b13d794db1e01e0275e5a7cf5aab15e1ccc6b8de606828a3b650c491fc6` |
| `sounds/release-12.wav` | 11100 | `cdca0bdee07c0c3377f49b2c88a26354ae0be6fc19e4ea97739f4a58e6a2601c` |
| `sounds/release-13.wav` | 7216 | `3858bbdf8d093cfa98dedb1d7ad7dc03995b19460bb9c5eefa5dc80fb4483ff6` |
| `sounds/release-14.wav` | 10120 | `8a279039577cf09167797e145f3845adc99277cf107b8ea97729bf9cba235e0d` |
| `sounds/release-15.wav` | 8888 | `ee766781eeb5b6875f63ac5761f53834290474d19fc28a90950d2fc2f77cb944` |
| `sounds/release-16.wav` | 3872 | `dc3612a30bb5259a303c68f8aa335f920b3e94b20a5d713f156d807947d8f3e7` |
| `sounds/release-17.wav` | 7832 | `b6acb4028947572b2d7596c9193f516606fa41415698d1cdf1feb04acd31eec1` |
| `sounds/release-18.wav` | 4928 | `d491e054a3ac291c0c02e0bb6a3c4c512d0b008aa3c26e4733dfbde7e42ab44d` |
| `sounds/release-19.wav` | 7480 | `39d5c6ef947e452fc6552e87a8dacc671cdfc61bb79f6f36e0a8d4bfe7e5e095` |
| `sounds/release-20.wav` | 5016 | `a16b4441406440ff9bd52499954b9fb4aabdceea996df1e3412a06a5100b332f` |
| `sounds/release-22.wav` | 6688 | `d004cd395959abccc299bd6afe67a76d15567629d225d38defdaa9c39291ab29` |
| `sounds/release-23.wav` | 12760 | `fb2062b2fd5ff241a79bb727c92b464a0b491deb0c5ea45347d6c613bbb0c45e` |
| `sounds/release-24.wav` | 10976 | `6602ed9c1b95235b3374e9cf7d5b6df53f590e4cab246949431b0587bb779440` |
| `sounds/release-25.wav` | 9680 | `a57b502949c071ba3aaa3d60aaa3ba96a2289cc08b4e7a58c2dadd9d3d66382a` |
| `sounds/release-26.wav` | 9140 | `8a7dee8be3e84e4260a030b2f25d7e575042372680261fe06152442d9fc8297a` |
| `sounds/release-27.wav` | 9504 | `c7aa484f9c80fe66f18d022c2934054afd227f2dd5df0d9180e0b4d5e1a1e678` |
| `sounds/release-28.wav` | 8184 | `b27dece50728f9177de009acee94e9d9c8c2c92dfb8387845381b94ffed279e6` |
| `sounds/release-29.wav` | 8356 | `d44ffb89f9bfeb32e1696dd2806009c4684ab947697ccdafdbd6cd3e0547c517` |
| `sounds/release-30.wav` | 9152 | `2084294363dd8f31b0eebf89e5587d9d1711452451e318c0d8b174a12c4ce0d6` |
| `sounds/release-31.wav` | 11968 | `2ab0e5caa13ddce3d79f4f049f91d67949a9973d2fb0f507972087dc45845c83` |
| `sounds/release-32.wav` | 9944 | `6ef208d073e8559e621247e10c06b67cb33c901734e140ad92a7ddaf023d8b25` |
| `preview.wav` | 150194 | `a5e051f770c4a27fb7a6506cc5bb2e9882533b4d8eb20ca6fb216eb35ab285c2` |
