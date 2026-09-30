//! Os arquivos de verdade do acervo do dono. `#[ignore]` porque não estão no
//! repositório: `cargo test -p raw-codec -- --ignored` na máquina que os tem.

use std::path::Path;

const DNG_DO_LIGHTROOM: &str = "/Users/alexkads/Documents/FotosParaSite/_CSF7953.dng";
const NEF: &str = "/Users/alexkads/Downloads/arquivos_2026_2026-01-04_DSC_4645.NEF";

fn media(img: &image::RgbImage) -> [f64; 3] {
    let mut soma = [0f64; 3];
    for p in img.pixels() {
        for c in 0..3 {
            soma[c] += p[c] as f64;
        }
    }
    let n = (img.width() * img.height()) as f64;
    soma.map(|s| s / n)
}

#[test]
#[ignore]
fn o_dng_com_perdas_do_lightroom_abre_de_pe_e_sem_rosa() {
    if !Path::new(DNG_DO_LIGHTROOM).exists() {
        return;
    }
    let bytes = std::fs::read(DNG_DO_LIGHTROOM).unwrap();
    let img = raw_codec::decodificar(&bytes).unwrap();
    // Retrato: a foto é 2560×1707 deitada com `Orientation = 8`.
    assert_eq!((img.width(), img.height()), (1707, 2560));
    // 🚨 Sem a `OpcodeList2` a média saía rosa e clara: R≈G+60. Com ela, a
    // parede de madeira e o vestido branco dão um marrom neutro.
    let [r, g, b] = media(&img);
    assert!(
        r - g < 40.0 && g > b - 10.0,
        "cor fora: {r:.0} {g:.0} {b:.0}"
    );
    assert!(g < 150.0, "clara demais (curva lida como luz?): {g:.0}");
}

#[test]
#[ignore]
fn o_xmp_do_dng_do_lightroom_traz_a_revelacao() {
    if !Path::new(DNG_DO_LIGHTROOM).exists() {
        return;
    }
    let xmp = raw_codec::xmp(&std::fs::read(DNG_DO_LIGHTROOM).unwrap()).unwrap();
    assert!(xmp.contains(r#"crs:Exposure2012="+0.46""#));
    assert!(xmp.contains("crs:SplitToningShadowHue"));
}

#[test]
#[ignore]
fn o_nef_abre() {
    if !Path::new(NEF).exists() {
        return;
    }
    let img = raw_codec::decodificar(&std::fs::read(NEF).unwrap()).unwrap();
    assert!(img.width() > 4000, "{}x{}", img.width(), img.height());
}

#[test]
#[ignore]
fn o_dng_do_lightroom_diz_que_esta_em_retrato() {
    if !Path::new(DNG_DO_LIGHTROOM).exists() {
        return;
    }
    // `Rotate 270 CW` no exiftool = 8.
    assert_eq!(
        raw_codec::orientacao_exif(&std::fs::read(DNG_DO_LIGHTROOM).unwrap()),
        Some(8)
    );
}
