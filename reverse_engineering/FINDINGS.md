# COSMOS firmware: what the decompilation shows

Sources: `COSMOS_M1.41.smf` (main), `COSMOS_Q1.41.smf` (quadro), `COSMOSPM1.61.smf`
(pitch), `COSMOS_R1.51.smf` (rhythmic). The images are **not encrypted**.

## 1. Container and platform

| Item | Value | How it was found |
|---|---|---|
| File header | `"SOMA...." "COSM"` + 4-byte checksum (16 bytes) | hexdump |
| Load address | `0x08020000` (bootloader below, 128 KB) | IRQ28 vector points to `0x080201C5`, i.e. the first byte after a `0x1C4`-byte vector table |
| MCU | STM32F446 (Cortex-M4F, 97 IRQs, `RCC_DCKCFGR`, PLLSAI) | vector count, RCC accesses |
| Toolchain | Keil / ARMCC (`SystemInit` → `__main` → scatter-load) | reset handler shape |
| Sound memory | 8 MB SDRAM at `0xC0000000` = **2^21 floats** | `if (i < 0x200000)` bounds checks |
| Sample rate | **46 875 Hz**, not 48 kHz | `HAL_I2S_Init(AudioFreq = 0xB71B)`; TIM2 `PSC = 7`, `ARR = 239` at 90 MHz |
| Memory length | 2^21 / 46 875 = **44.739 s** (the manual's "44.7 s") | |
| Audio entry | TIM2 IRQ (`0x080201C4`), one call per sample; reads I2S data latched by TIM7 | vector table, ISR body |

To reproduce: `scripts/` holds the Ghidra headless scripts (`SetupVectors.java` marks the
vector targets as Thumb functions and maps SRAM / peripherals / SDRAM; `DumpAll.java`
decompiles every function), `annotate.py` replaces each `DAT_` literal-pool reference
with its float / int / address value, `algconst.py` lists the wrap constants per
algorithm, `scan.py` and `dis.py` are quick capstone helpers.

```sh
analyzeHeadless proj cosmos -import COSMOS_M1.41.bin -loader BinaryLoader \
  -loader-baseAddr 0x08020000 -processor ARM:LE:32:Cortex \
  -scriptPath scripts -preScript SetupVectors.java -postScript DumpAll.java out.c
python3 annotate.py COSMOS_M1.41.bin out.c out_annotated.c
```
(`*.bin` = the `.smf` with its 16-byte header stripped.)

## 2. The program selector

The 12-way rotary switch is read as a **one-hot** code from GPIOD/GPIOG. Panel position 1
is bit `0x800`, position 12 is bit `0x001`. Dispatch:

| Pos | Bit | Routine | Algorithm |
|---|---|---|---|
| 1-3 | 0x800 0x400 0x200 | `FUN_08023230` `08023b7c` `08024494` | Two delays S/M/L |
| 4-6 | 0x100 0x080 0x040 | `FUN_08024d98` `08025c64` `08026b30` | Four delays S/M/L |
| 7-9 | 0x020 0x010 0x008 | `FUN_0802797c` `08028d4c` `0802a138` | Giant reverb S/M/L |
| 10-12 | 0x004 0x002 0x001 | `FUN_0802b520` `0802c0b0` `0802cc3c` | Granular S/M/L |

A new position must be stable for **47 000 samples (~1 s)** before anything happens.

## 3. Memory layouts (samples at 46 875 Hz)

Every layout is laid out from index 0; the remainder of the 2^21 floats is a **tail**
that keeps a running recording of the newest line (200-sample linear fades at both ends).
There is no copying when switching: the next algorithm reinterprets the same memory. This
is the "memory is transferred to the next algorithm" of the manual.

### Main

| Algorithm | Lines | Tail |
|---|---|---|
| 2D S | 100 003 · 123 113 | 1 874 036 |
| 2D M | 468 761 · 431 251 | 1 197 140 |
| 2D L | 1 000 003 · 1 097 101 | 48 |
| 4D S | 133 013 · 152 003 · 88 001 · 111 029 | 1 613 106 |
| 4D M | **332 838** · 234 383 · 604 711 · 454 709 | 470 511 |
| 4D L | 576 577 · 426 563 · 328 127 · 764 081 | 1 804 (zeros) |
| Reverb S | 43 156 · 59 661 · 89 175 · 65 182 · 100 559 · 64 189 · 97 482 · 68 677 | 1 509 071 |
| Reverb M | 201 119 · 119 321 · 178 349 · 130 363 · 86 311 · 128 377 · 194 963 · 137 353 | 920 996 |
| Reverb L | 341 911 · 202 859 · 146 743 · 221 621 · 303 217 · 218 249 · 331 447 · 233 509 | 97 596 |
| Gran S | 75 003 (grain 9 361) | 2 022 149 |
| Gran M | 234 376 (grain 20 361) | 1 862 776 |
| Gran L | 334 376 (grain 27 361 + central 12 456) | 1 762 776 |

All delay lines are prime, except 4D-M line A: the code wraps on `332837 < i`, giving
332 838 (an off-by-one; 332 837 is the prime). Reverb M and L lines are all prime; the
small reverb is not prime-based. Reverb order is AP1, D1, AP2, D2, AP3, D3, AP4, D4.

### Quadro (`Q1.41`)

| Algorithm | Lines |
|---|---|
| 2D S/M/L | 110 119 · 130 121 / 498 761 · 538 763 / 820 037 · 920 011 |
| 4D S | 143 063 · 112 019 · 60 013 · 92 003 |
| 4D M | 302 843 · 224 423 · 504 727 · 354 727 |
| 4D L | 526 583 · 376 573 · 288 137 · 804 091 |
| Reverb S | 87 180 · 61 667 · 42 157 · 67 187 · 80 567 · 66 191 · 107 507 · 69 678 |
| Reverb M | 231 131 · 159 337 · 89 317 · 170 363 · 208 367 · 126 397 · 224 969 · 167 381 |
| Reverb L | 301 913 · 232 861 · 116 747 · 181 639 · 263 227 · 258 253 · 311 447 · 193 513 |
| Granular | 65 003 / 274 376 / 374 376; grains 8 361 / 17 361 / 25 361 + central 16 456 |

## 4. Anti-click machinery ("more than 100 functions")

The FAQ's 3-180 ms fades are a handful of patterns repeated everywhere:

| Where | Curve | Length |
|---|---|---|
| Algorithm switch | global gain −0.001 / tick to < 0.001, glue, then +0.001 / tick | 1 000 ticks (21 ms) each way |
| **Glue** of every segment seam (old and new layouts) | `buf[start+i] *= i/120`, `buf[end−i] *= i/120` — linear | 120 samples (2.56 ms) per side |
| Glue at the old write heads | V notch `buf[c±i] *= i/120` (reverb: every head; others: tail head) | 240 samples |
| Reverse | `fade −= 0.001` to < 0.001, flip direction, `fade += 0.001` | 2 × 1 000 ticks; FB → 1 and writes cross-faded while it runs, so the memory freezes rather than mutes |
| Tail recording | `i/200`, `(N−1−i)/200` | 200 samples |
| RECORD groups | `g += (g − target)·(−0.002)` | τ ≈ 500 ticks (11 ms) |
| ERASE | `keep += keep·(−0.0005)` down, `keep += (keep − 1.1)·(−0.0002)` up | τ 43 ms / 107 ms |
| HPF / LPF on-off | `a += (a − 1)·(−5e-5)` | τ ≈ 0.43 s |
| Knobs | one-pole `k += (k − adc)·(−0.0003)` (BLUR, DRIFT) / `(−0.001)` (others; MIX twice) | 71 ms / 21 ms |

The 4D layouts cut their tail notch at `C_start + tail_ptr` and the main small granular
at `74 999 + tail_ptr` (tail starts at 75 003); the quad firmware fixes the latter. These
quirks are kept: they are part of the "mechanically damaged tape" sound of switching.

## 5. Per-sample signal path

**Input**: 24-bit / 8 388 607. LEDs: green > 0.3, red > 0.95, hold 23 000 samples.
**Noise gate** (per channel): `|x| ≤ thr → g += g·(−1e-5)`, else `g += (g − 1)·(−0.1)`;
`thr = 0.004` (sensitive mode 0.0004).

**FB**: `FB = 1.066·k·(2 − k)`, `k = adc / cal`, `cal` from flash (valid 3000-7000,
default 5200). Calibration stores `cal·k/0.75`, so unity is at `k = 0.75` → 95 % of the
knob travel with the default. Maximum ≈ 1.018.

**SUP|COM**: below the centre, suppressor amount `((0.5 − s)·2)²`; above, compressor
amount `((s − 0.5)·2)²` (and the suppressor detectors are zeroed). Suppressor: two
envelopes of `|4·x·rec|` with 0.1 attack and releases 0.00015 / 0.00004; gain
`max(0, ((1 − 5·E_fast)·E_slow − E_fast)·amount + 1)`, applied to what the memory plays
back. Compressor per line: target `1 + (0.8 − 1.9·(in + line)²)·amount`, gain falls at
0.01 / tick and recovers at 3e-5 / tick, applied to the line and to the dry signal.

**Tone** (in the loop): HPF `lp += (x − lp)·c; y = x − lp` ×gain with
`(c, gain) = (0.035, 1.035) / (0.06, 1.04) / (0.1, 1.063)` → 266 / 462 / 786 Hz.
LPF `lp += (y − lp)·c; z = G·lp − Q·lp²` (an even-order bend) with
`(c, G, Q) = (0.08, 1.033, 0.03) / (0.15, 1.01, 0.025) / (0.5, 1.001, 0.001)` → 622 Hz /
1.2 kHz / 5.2 kHz.

**Loop limiter**: target `0` below 0.8, `1.25·(|w| − 0.8)²` up to 1.2, `|w| − 1` beyond;
envelope attack 0.1, release 0.001; `w *= 1 − env`.
**DRIVE**: `k = knob³`, `y = ((1 + k)·x − k·x|x|)·(1 − 0.3k)`, in the loop and at the output.
**Output limiter**: identity to 0.9, then `x − 2.52·(x − 0.9)²`, clamped at 1.0984 (zero
slope at the top).

**Chaos**: xorshift32 (13, 17, 5), seed 1234, XOR'd every tick with 3 ADC LSBs. Two random
walks: `c1 += (hi16 − 32768)·5e-9`, `c2 += (lo16 − 32768)·1e-8`, wrapping 1 → −0.5 and
−0.5 → 1. **LFOs**: triangles 0..1, increments 3.1151e-6, 3.7667e-6 (× `c1 + 1`),
2.3159e-6, 2.9663e-6 (× `c2 + 1`): periods of 13.7 / 11.3 / 18.4 / 14.4 s at rest.

### Two delays
Read and write at the same index of each prime ring (delay = length; no fractional
modulation). BLUR: `a' = (1 − 0.49b)a + 0.51b·b_line` and symmetric. **DRIFT pans**:
`L = A(1 − p1·d) + B·p2·d`, `R = B(1 − p2·d) + A·p1·d`. Write `= line·FB + in·g`, then
limiter and DRIVE.

### Four delays
Cross-feed ring `A ← −0.6b·D`, `B ← 0.6b·C`, `C ← 0.55b·A`, `D ← 0.55b·B` (own gain
`1 − 0.46b`). Output: `L = 0.7·[A(1 − p1d) + B·p2d + C(0.7(1−d) + p3d) + D(0.4(1−d) + (1−p4)d)]`,
mirrored for R. A and C take the left input, B and D the right.

### Giant reverb
A ring of four Schroeder all-pass + delay stages: `AP[p] = v + g·r`, `D[q] ← r − g·v`,
`g = 0.55·(1 − FB/2)`, `0.91·FB` between stages; tone, limiter and DRIVE in every stage;
inputs L, R, L, R. AP1 and AP2 are read through linearly-interpolated taps swept by
triangles of 0..8300 samples at 0.002112 / tick and 0..11100 at 0.0018099 / tick (periods
of 167 s and 262 s). Output `L = 0.7·(D1 + 0.7·D2 + 0.4·D4)`, `R = 0.7·(D3 + 0.5·D4 + 0.4·D2)`.
BLUR and DRIFT do nothing here, as the manual says.

### Granular
One mono line (the buffer) with feedback; 4 L and 4 R triangular grains offset by a
quarter grain, plus (large only) 4 central grains ×0.85. At each grain start:
L offset `= int(rnd·N·BLUR) + int(p2·N·DRIFT·0.9)`, R offset `= int(p1·N·DRIFT·0.9) +
int(((rnd − 0.5)·BLUR + 0.5)·N)`. Grains read **absolute** buffer positions, so their
delay changes as the write head passes. Sum ×0.7 (large) / ×0.8.

### Half speed (RECORD held 250 001 samples ≈ 5.3 s)
2D/4D only: writes and pointer moves happen every other tick; reads repeat (no
interpolation).

## 6. Pitch firmware
DRIVE becomes speed: `TIM2->ARR = 1919 + (1 − knob)·1920`. The whole DSP runs at
`46 875 / (2 − knob)` Hz while the codec stays at 46 875 Hz: input decimated with no
anti-aliasing, output held. Sideral's CLOCK + GRIT reproduce this.

## 7. Rhythmic firmware (`R1.51`)

Dispatch (one-hot as in the main firmware): 0x800 looper, 0x400 8/7, 0x200 4/3,
0x100 / 0x080 / 0x040 random mangler ÷8 / ÷6 / ÷5, 0x020 / 0x010 crazy mangler ÷8 / ÷6,
0x008 fractal pitch shifter, then 4D-M, Reverb S and Granular L of the main set.

**Memory**: two 2^20 halves, L at 0 and R at 1 048 575. Loop state `@20010468`: 0 empty,
1 defining (length `@20010490` grows one sample per tick, capped at **898 000** = 19.16 s),
2 defined. Length and position survive program changes inside the family. When the
previous program was 8/7 or 4/3, a 200-sample fade is cut where the old right loop ended.

**Looper / 8:7 / 4:3**: the 2D signal path without BLUR cross-feed; the right channel wraps
at `7N/8` or `3N/4`.

**BLUR = "Halo" plate reverb** on the output, in a 16 384-float SRAM ring addressed by tap
offsets from a single moving index: AP 2011 (read swept 0..53 at ±0.002103/tick), D 1193,
AP 1783, D 2303, AP 863 (swept 0..71 at ±0.001214), D 2283, AP 2949, D 1373, carry
`−D4·d`. Decay `d = 0.5 + 0.4·blur`, diffusion `g = 0.55·(1.1 − d³)`, input `(L+R)/8`.
Taps L: 2213·0.8 + 5336·0.5 + 9201·0.58 + 15015·0.4d; R: 2709·0.6 + 6532·0.7 + 10187·0.4 +
16201·0.5d. Mix `((1−b)·x + 1.6·b·rev)·(1 + min(b, 1−b))`.

**Random mangler**: slice = N/div; every slice L and R each pick a random source slice
(3 bits of the xorshift: `(x & 0xffff) >> 13`, `x >> 29`); read **and write** at
`src·slice + q`, so the memory is shuffled in place; 200-sample linear fades at the slice
edges on reads and on the recorded input.

**Crazy mangler**: per slice, its own xorshift draws: source slices; a repeat length of
slice/1, /2, /4 or /8 (stutter); a ratchet (when `(evL ^ evR) & 0x3f) >> 3 == 6`, the repeat
shrinks by one sample per tick down to 400); reversal (`z >> 28 ≥ 14`, 2/16); octave up
(`(evL ^ evR) & 7 == 6`, read at 2r); octave down (DRIFT ≥ 0.5 and 12/256); and a 6-bit
event per channel: < 4 silence, 21–26 dissolve, 41–46 swell, ≥ 58 ×0.7.

**Fractal pitch shifter**: DRIFT ≥ 0.75 reads at `2p mod N`, < 0.25 at `p/2`, both ×0.75
and smoothed by a two-tap average; the result is written back at `p`, so the shift compounds
every pass (the manual's "content will gradually fade out"). With an impulse in the loop,
octave up produces a geometric accelerando of echoes.

`PR1.61` is the rhythmic firmware with the pitch firmware's TIM2 trick.
