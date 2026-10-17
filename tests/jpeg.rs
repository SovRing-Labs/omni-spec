//! JPEG conformance against Pillow / libjpeg-turbo (tests/data/jpeg/, index tests/data/jpeg-oracle.tsv,
//! tools/oracle_jpeg.py). 4:4:4 and grayscale ("gate") must match the reference RGB8 within ±1 per sample;
//! 4:2:0 ("report") is decoded and its maximum deviation is reported (libjpeg's fancy upsampling is not reproduced).

use omni_spec::jpeg::decode;

#[test]
fn pillow_reference_within_one_sample() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/jpeg");
    let index = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/jpeg-oracle.tsv")).expect("index");
    let mut failures = Vec::new();
    let mut gated = 0usize;
    for line in index.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let name = f[0];
        let (w, h): (usize, usize) = (f[1].parse().unwrap(), f[2].parse().unwrap());
        let gate = f[4] == "gate";
        let jpg = std::fs::read(format!("{dir}/{name}")).unwrap();
        let reference = std::fs::read(format!("{dir}/{}.rgb", name.trim_end_matches(".jpg"))).unwrap();
        match decode(&jpg) {
            Ok(img) => {
                assert_eq!((img.width, img.height), (w, h), "{name}");
                let diffs: Vec<i32> = img.rgb.iter().zip(&reference).map(|(a, b)| (*a as i32 - *b as i32).abs()).collect();
                let max = diffs.iter().copied().max().unwrap_or(0);
                let mean = diffs.iter().sum::<i32>() as f64 / diffs.len() as f64;
                if gate {
                    gated += 1;
                    if max > 1 {
                        failures.push(format!("{name}: max sample deviation {max}"));
                    }
                } else {
                    // 4:2:0 report-only: nearest-neighbour chroma differs from libjpeg fancy upsampling; mean must stay under 4.
                    eprintln!("report {name}: max sample deviation {max}, mean {mean:.2}");
                    if mean > 4.0 {
                        failures.push(format!("{name}: mean deviation {mean:.2}"));
                    }
                }
            }
            Err(e) => failures.push(format!("{name}: {e:?}")),
        }
    }
    eprintln!("jpeg oracle: {gated} gated images within ±1");
    assert!(failures.is_empty(), "{failures:?}");
}
