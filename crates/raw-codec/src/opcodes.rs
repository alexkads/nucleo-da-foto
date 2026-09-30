//! A `OpcodeList2` do DNG: o que o rawler 0.8.0 não aplica.
//!
//! # 🚨 Por que o DNG com perdas do Lightroom saía rosado
//!
//! O DNG com perdas (`Compression = 34892`) guarda a imagem já demosaicada em
//! blocos JPEG de **8 bits**, e 8 bits lineares não bastam para uma foto. O
//! Lightroom resolve gravando os valores numa curva e deixando no arquivo, como
//! opcode, o polinômio que os devolve ao linear: três `MapPolynomial` de grau 3,
//! um por canal (medido no `_CSF7953.dng` do acervo em 30/set/2026).
//!
//! O rawler decodifica os blocos e ignora os opcodes, e aí o motor recebia a
//! curva como se fosse luz: foto lavada e rosada. Aplicado o polinômio, a cor
//! bate com a da LibRaw do sistema.
//!
//! Só os dois mapeamentos de valor são tratados (`MapTable` e `MapPolynomial`):
//! são eles que mudam o que cada número **significa**. Os outros da lista 2
//! (`GainMap`, a sombra da lente em DNG de sensor) corrigem a imagem mas não a
//! tornam errada quando faltam.

use rawler::rawimage::{BlackLevel, RawImage, RawImageData, WhiteLevel};

const MAP_TABLE: u32 = 7;
const MAP_POLYNOMIAL: u32 = 8;

/// A área e os planos onde um opcode age (DNG 1.3, §6.2.2).
#[derive(Debug, Clone, PartialEq)]
struct Area {
    topo: usize,
    esquerda: usize,
    baixo: usize,
    direita: usize,
    plano: usize,
    planos: usize,
    passo_linha: usize,
    passo_coluna: usize,
}

#[derive(Debug, Clone, PartialEq)]
enum Mapa {
    /// Coeficientes do grau 0 em diante, sobre o valor normalizado em 0..1.
    Polinomio(Vec<f64>),
    /// Tabela de 16 bits indexada pelo valor normalizado para 0..65535.
    Tabela(Vec<u16>),
}

#[derive(Debug, Clone, PartialEq)]
struct Opcode {
    area: Area,
    mapa: Mapa,
}

/// Os opcodes são sempre big-endian, qualquer que seja o TIFF.
fn u32_em(b: &[u8], em: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(em..em + 4)?.try_into().ok()?))
}

fn f64_em(b: &[u8], em: usize) -> Option<f64> {
    Some(f64::from_be_bytes(b.get(em..em + 8)?.try_into().ok()?))
}

fn ler(lista: &[u8]) -> Vec<Opcode> {
    let Some(quantos) = u32_em(lista, 0) else {
        return Vec::new();
    };
    let mut em = 4;
    let mut saida = Vec::new();
    for _ in 0..quantos {
        let (Some(id), Some(tamanho)) = (u32_em(lista, em), u32_em(lista, em + 12)) else {
            break;
        };
        let p = em + 16;
        em = p + tamanho as usize;
        if id != MAP_TABLE && id != MAP_POLYNOMIAL {
            continue;
        }
        let campo = |i: usize| u32_em(lista, p + i * 4).map(|v| v as usize);
        let area = (|| {
            Some(Area {
                topo: campo(0)?,
                esquerda: campo(1)?,
                baixo: campo(2)?,
                direita: campo(3)?,
                plano: campo(4)?,
                planos: campo(5)?.max(1),
                passo_linha: campo(6)?.max(1),
                passo_coluna: campo(7)?.max(1),
            })
        })();
        let Some(area) = area else { break };
        let mapa = if id == MAP_POLYNOMIAL {
            let Some(grau) = campo(8) else { break };
            let coeficientes: Option<Vec<f64>> =
                (0..=grau).map(|k| f64_em(lista, p + 36 + 8 * k)).collect();
            match coeficientes {
                Some(c) => Mapa::Polinomio(c),
                None => break,
            }
        } else {
            let Some(n) = campo(8) else { break };
            let tabela: Option<Vec<u16>> = (0..n)
                .map(|k| {
                    lista
                        .get(p + 36 + 2 * k..p + 38 + 2 * k)
                        .map(|b| u16::from_be_bytes([b[0], b[1]]))
                })
                .collect();
            match tabela {
                Some(t) if !t.is_empty() => Mapa::Tabela(t),
                _ => break,
            }
        };
        saida.push(Opcode { area, mapa });
    }
    saida
}

impl Mapa {
    fn aplicar(&self, x: f64) -> f64 {
        match self {
            Mapa::Polinomio(c) => c.iter().rev().fold(0.0, |acc, k| acc * x + k),
            Mapa::Tabela(t) => {
                let i = (x * 65535.0).round().clamp(0.0, (t.len() - 1) as f64) as usize;
                t[i] as f64 / 65535.0
            }
        }
    }
}

/// Aplica os mapeamentos da `OpcodeList2` ao RAW decodificado, que sai em
/// 16 bits com preto 0 e branco 65535.
///
/// Devolve quantos opcodes agiram. ⚠️ Só age sobre dados inteiros com o mesmo
/// preto em todos os canais — o caso do DNG do Lightroom; o resto passa como
/// veio, que é o que acontecia antes.
pub fn aplicar(raw: &mut RawImage, lista: &[u8]) -> usize {
    let opcodes = ler(lista);
    if opcodes.is_empty() {
        return 0;
    }
    let pretos = raw.blacklevel.as_vec();
    let preto = pretos.first().copied().unwrap_or(0.0);
    if pretos.iter().any(|p| *p != preto) {
        return 0;
    }
    let brancos = raw.whitelevel.as_vec();
    let (largura, altura, cpp) = (raw.width, raw.height, raw.cpp);
    let RawImageData::Integer(dados) = &mut raw.data else {
        return 0;
    };

    // Para 0..1 uma vez só, e os opcodes em sequência sobre o mesmo buffer.
    let mut normal: Vec<f64> = dados
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let branco = brancos
                .get(i % cpp)
                .or(brancos.first())
                .copied()
                .unwrap_or(65535.0) as f64;
            ((*v as f64 - preto as f64) / (branco - preto as f64).max(1.0)).clamp(0.0, 1.0)
        })
        .collect();

    for op in &opcodes {
        let a = &op.area;
        let baixo = a.baixo.min(altura);
        let direita = a.direita.min(largura);
        for y in (a.topo..baixo).step_by(a.passo_linha) {
            for x in (a.esquerda..direita).step_by(a.passo_coluna) {
                for plano in a.plano..(a.plano + a.planos).min(cpp) {
                    let i = (y * largura + x) * cpp + plano;
                    normal[i] = op.mapa.aplicar(normal[i]).clamp(0.0, 1.0);
                }
            }
        }
    }

    for (d, n) in dados.iter_mut().zip(normal) {
        *d = (n * 65535.0).round() as u16;
    }
    raw.whitelevel = WhiteLevel::new(vec![65535; cpp]);
    raw.blacklevel = BlackLevel::zero(1, 1, cpp);
    opcodes.len()
}

#[cfg(test)]
pub(crate) mod testes {
    use super::*;

    /// Uma `OpcodeList2` com um `MapPolynomial` por plano, sobre a foto toda.
    pub fn lista_de_polinomios(altura: u32, largura: u32, por_plano: &[[f64; 4]]) -> Vec<u8> {
        let mut b = (por_plano.len() as u32).to_be_bytes().to_vec();
        for (plano, c) in por_plano.iter().enumerate() {
            b.extend(MAP_POLYNOMIAL.to_be_bytes());
            b.extend(0x0103_0000u32.to_be_bytes());
            b.extend(0u32.to_be_bytes());
            b.extend((36u32 + 8 * 4).to_be_bytes());
            for v in [0, 0, altura, largura, plano as u32, 1, 1, 1, 3] {
                b.extend(v.to_be_bytes());
            }
            for k in c {
                b.extend(k.to_be_bytes());
            }
        }
        b
    }

    #[test]
    fn le_os_tres_polinomios_do_dng_do_lightroom() {
        // Os coeficientes do `_CSF7953.dng`, lidos pelo exiftool.
        let c = [
            [0.000656, 0.001716, 0.0, 0.217892],
            [0.002045, 0.004499, 0.0, 0.571407],
            [0.000809, 0.002932, 0.0, 0.372318],
        ];
        let lista = lista_de_polinomios(1707, 2560, &c);
        let ops = ler(&lista);
        assert_eq!(ops.len(), 3);
        assert_eq!(ops[1].area.plano, 1);
        assert_eq!(ops[1].area.baixo, 1707);
        assert_eq!(ops[1].mapa, Mapa::Polinomio(c[1].to_vec()));
    }

    #[test]
    fn o_polinomio_devolve_o_linear() {
        let m = Mapa::Polinomio(vec![0.0, 0.0, 0.0, 1.0]);
        assert!((m.aplicar(0.5) - 0.125).abs() < 1e-9);
    }

    #[test]
    fn lista_truncada_nao_quebra() {
        let lista = lista_de_polinomios(10, 10, &[[0.0, 1.0, 0.0, 0.0]]);
        assert!(ler(&lista[..lista.len() - 3]).is_empty());
        assert!(ler(&[]).is_empty());
    }
}
