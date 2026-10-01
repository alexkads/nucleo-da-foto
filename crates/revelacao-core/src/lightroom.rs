//! As tabelas medidas no Lightroom Classic — o processo 1 do motor.
//!
//! # Por que tabelas
//!
//! Em 1/out/2026 a régua (`ferramentas/lightroom/`) exportou do Lightroom
//! Classic 15.5 a mesma foto sintética com cada slider em cada valor, e mostrou
//! que os do motor eram **outra coisa**: o Contraste −100 virava um cinza liso
//! (o do Lightroom guarda o preto e o branco), a Exposição +1 estourava tudo
//! acima do meio, os Pretos eram fracos, e a vinheta pós-corte começava longe
//! demais do centro e parava em 78% no canto onde a do Lightroom chega a 47%.
//! Em vez de adivinhar a conta da Adobe, o motor lê **o que o Lightroom fez**:
//! a curva de cada valor, medida.
//!
//! 🔑 **São medidas, não arquivos da Adobe.** O binário saiu de fotos sintéticas
//! (rampas e cinzas lisos) reveladas pelo Lightroom e lidas pixel a pixel, por
//! `examples/tabelas-do-lightroom.rs`; nenhuma tabela de perfil da Adobe entra
//! aqui.
//!
//! # O desenho
//!
//! Uma textura `R32Float` de [`LARGURA`] × [`ALTURA`]. Cada linha é uma curva de
//! 256 pontos:
//!
//! | linhas | o quê | a coluna é |
//! |---|---|---|
//! | [`LINHA_EXPOSICAO`] + 0..17 | Exposição de −2 a +2 EV, de 0,25 em 0,25 | o valor de entrada (0–255) |
//! | [`LINHA_CONTRASTE`] … [`LINHA_PRETOS`] + 0..21 | cada slider de −100 a +100, de 10 em 10 | o valor de entrada |
//! | [`LINHA_VINHETA`] + 0..21 | a vinheta nos estilos 1 e 2, de −100 a +100 | o valor de entrada |
//! | [`LINHA_SOBREPOSICAO`] + 0..21 | a vinheta no estilo 3 (sobreposição) | o valor de entrada |
//! | [`LINHA_MASCARA`] + 0..81 | a máscara da vinheta, ponto médio × difusão (9 × 9, de 12,5 em 12,5) | a distância elíptica, de 0 a [`DISTANCIA_MAXIMA`] |
//!
//! O valor 0 de cada slider é a identidade, exata. O WGSL repete estas
//! constantes (`corpo.wgsl`), e `o_wgsl_usa_o_mesmo_desenho_das_tabelas` prende
//! as duas cópias.

/// Pontos por curva.
pub const LARGURA: u32 = 256;
/// Exposição: 17 linhas, de −2 a +2 EV.
pub const LINHA_EXPOSICAO: u32 = 0;
pub const LINHAS_DE_EXPOSICAO: u32 = 17;
/// Os cinco sliders de −100 a +100: 21 linhas cada.
pub const LINHA_CONTRASTE: u32 = 17;
pub const LINHA_REALCES: u32 = 38;
pub const LINHA_SOMBRAS: u32 = 59;
pub const LINHA_BRANCOS: u32 = 80;
pub const LINHA_PRETOS: u32 = 101;
/// A força da vinheta: estilos 1 e 2 (iguais no Lightroom, até em cor) e o 3.
pub const LINHA_VINHETA: u32 = 122;
pub const LINHA_SOBREPOSICAO: u32 = 143;
/// A máscara: 9 pontos médios × 9 difusões.
pub const LINHA_MASCARA: u32 = 164;
pub const PASSOS_DA_MASCARA: u32 = 9;
pub const ALTURA: u32 = 245;
/// A distância elíptica da última coluna da máscara: além do canto (√2).
pub const DISTANCIA_MAXIMA: f32 = 1.45;

const BRUTO: &[u8] = include_bytes!("tabelas_lightroom.bin");

/// As tabelas, linha a linha (`ALTURA × LARGURA` valores).
pub fn tabelas() -> Vec<f32> {
    let (palavras, _) = BRUTO.as_chunks::<4>();
    palavras.iter().map(|b| f32::from_le_bytes(*b)).collect()
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn o_binario_tem_o_tamanho_do_desenho() {
        assert_eq!(tabelas().len(), (LARGURA * ALTURA) as usize);
    }

    /// O valor 0 de cada slider tem de ser a identidade **exata**: no processo
    /// 1 com tudo no neutro, a foto sai como entrou.
    #[test]
    fn o_zero_de_cada_slider_e_a_identidade() {
        let t = tabelas();
        let linha = |l: u32| &t[(l * LARGURA) as usize..((l + 1) * LARGURA) as usize];
        for (rotulo, l) in [
            ("exposição", LINHA_EXPOSICAO + 8),
            ("contraste", LINHA_CONTRASTE + 10),
            ("realces", LINHA_REALCES + 10),
            ("sombras", LINHA_SOMBRAS + 10),
            ("brancos", LINHA_BRANCOS + 10),
            ("pretos", LINHA_PRETOS + 10),
            ("vinheta", LINHA_VINHETA + 10),
            ("sobreposição", LINHA_SOBREPOSICAO + 10),
        ] {
            for (i, v) in linha(l).iter().enumerate() {
                assert_eq!(
                    *v, i as f32,
                    "{rotulo}: a coluna {i} do zero não é a identidade"
                );
            }
        }
    }

    /// 🔑 **O preto e o branco ficam onde estão** no Contraste, nas duas
    /// pontas — é o que separa a curva do Lightroom da reta que o motor tinha
    /// (Contraste −100 virava cinza 128 em tudo).
    #[test]
    fn o_contraste_do_lightroom_guarda_o_preto_e_o_branco() {
        let t = tabelas();
        for passo in [0u32, 4, 16, 20] {
            let l = ((LINHA_CONTRASTE + passo) * LARGURA) as usize;
            assert!(t[l] < 3.0, "contraste {passo}: o preto foi a {}", t[l]);
            assert!(
                t[l + 255] > 252.0,
                "contraste {passo}: o branco foi a {}",
                t[l + 255]
            );
        }
    }

    /// Toda curva de tom sobe: um valor maior na entrada nunca sai menor.
    #[test]
    fn as_curvas_de_tom_nunca_descem() {
        let t = tabelas();
        for l in LINHA_EXPOSICAO..LINHA_VINHETA {
            let c = &t[(l * LARGURA) as usize..((l + 1) * LARGURA) as usize];
            for i in 1..256 {
                assert!(
                    c[i] + 0.01 >= c[i - 1],
                    "linha {l}: desce em {i} ({} → {})",
                    c[i - 1],
                    c[i]
                );
            }
        }
    }

    /// O WGSL repete o desenho das linhas; as duas cópias têm de dizer o mesmo,
    /// senão o shader leria a curva do Contraste no lugar da dos Realces.
    #[test]
    fn o_wgsl_usa_o_mesmo_desenho_das_tabelas() {
        let shader = include_str!("shaders/corpo.wgsl");
        let constante = |nome: &str| -> f32 {
            let linha = shader
                .lines()
                .find(|l| l.trim_start().starts_with(&format!("const {nome}:")))
                .unwrap_or_else(|| panic!("o WGSL não declara `{nome}`"));
            linha
                .split('=')
                .nth(1)
                .unwrap()
                .trim()
                .trim_end_matches(';')
                .parse()
                .unwrap()
        };
        for (nome, valor) in [
            ("LR_LINHA_EXPOSICAO", LINHA_EXPOSICAO),
            ("LR_LINHA_CONTRASTE", LINHA_CONTRASTE),
            ("LR_LINHA_REALCES", LINHA_REALCES),
            ("LR_LINHA_SOMBRAS", LINHA_SOMBRAS),
            ("LR_LINHA_BRANCOS", LINHA_BRANCOS),
            ("LR_LINHA_PRETOS", LINHA_PRETOS),
            ("LR_LINHA_VINHETA", LINHA_VINHETA),
            ("LR_LINHA_SOBREPOSICAO", LINHA_SOBREPOSICAO),
            ("LR_LINHA_MASCARA", LINHA_MASCARA),
        ] {
            assert_eq!(
                constante(nome),
                valor as f32,
                "{nome} diverge de `lightroom.rs`"
            );
        }
        assert_eq!(constante("LR_DISTANCIA_MAXIMA"), DISTANCIA_MAXIMA);
    }

    /// A máscara vai de 0 no centro a 1 no canto, sem descer no caminho.
    #[test]
    fn a_mascara_vai_de_zero_a_um() {
        let t = tabelas();
        for l in LINHA_MASCARA..ALTURA {
            let c = &t[(l * LARGURA) as usize..((l + 1) * LARGURA) as usize];
            assert!(c[0] < 0.02, "máscara {l}: o centro já tem {}", c[0]);
            assert!(
                *c.last().unwrap() > 0.95,
                "máscara {l}: o canto só tem {}",
                c.last().unwrap()
            );
            for i in 1..256 {
                assert!(c[i] + 1e-4 >= c[i - 1], "máscara {l}: desce em {i}");
            }
        }
    }
}
