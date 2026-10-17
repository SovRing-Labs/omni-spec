# jpeg — JPEG decoder (ITU-T T.81), baseline and progressive Huffman

- **Spec:** ITU-T T.81 (ISO/IEC 10918-1) — B.2 markers and segments (DQT, DHT, SOF0/1/2, SOS, DRI, RSTn, EOI); F.2.2 sequential coding and the Huffman decoding procedure; G.1.2 progressive coding (spectral selection, successive approximation: DC first/refine, AC first/refine with EOBRUN); A.3.3 inverse DCT (realised as the integer "islow" factorisation); JFIF YCbCr → RGB.
- **Code:** `src/jpeg.rs` — `decode(&[u8]) -> Result<Image>` (RGB8). Restart intervals; 1 or 3 components; sampling factors 1–4; Adobe transform 0 and 'RGB' component ids treated as direct RGB.
- **Conformance** (Pillow / libjpeg-turbo oracle, `tools/oracle_jpeg.py` → `tests/data/jpeg/*.jpg|.rgb`, index `tests/data/jpeg-oracle.tsv`; synthetic content, encoder run as a separate process):
  - **Gate: 8 / 8** images within ±1 per sample: baseline and progressive 4:4:4 (odd sizes 33×17, 40×23; 64×64, 64×48), grayscale baseline and progressive (37×19, 40×40), restart-marker baseline 4:4:4 (70×33) and grayscale (50×25).
  - **Report: 2 / 2** 4:2:0 images (48×32 baseline, 35×21 progressive): mean deviation 2.3 and 3.5 on smooth content, max 11 and 15. Test gate: mean ≤ 4.
- **Known gaps:**
  - Chroma upsampling is nearest-neighbour; libjpeg uses fancy (triangle) upsampling for 4:2:0 and 4:2:2, which is why the 4:2:0 images are report-only.
  - Arithmetic coding, lossless, 12-bit precision, hierarchical and CMYK are `Unsupported`; DNL (height 0) is rejected.
  - Colour conversion is libjpeg-style fixed point; the integer IDCT is the islow method (not bit-proven against libjpeg's source; agreement is measured by the ±1 gate).
  - No corpus JPEGs from /usr/share were used (none in the system decoded set); the set is synthetic.
- **Patent screen:** JPEG baseline (ITU-T T.81 / ISO/IEC 10918-1) — all relevant patents have expired. No known patent encumbrances.
