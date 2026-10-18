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

    use rawler::decoders::Camera;
    use rawler::pixarray::PixU16;
    use rawler::rawimage::RawPhotometricInterpretation;

    /// Um opcode qualquer: o cabeçalho de 16 bytes, a área (8 campos) e o
    /// corpo, que começa pelo grau ou pelo tamanho da tabela.
    fn bloco(id: u32, area: [u32; 8], corpo: &[u8]) -> Vec<u8> {
        let mut b = id.to_be_bytes().to_vec();
        b.extend(0x0103_0000u32.to_be_bytes());
        b.extend(0u32.to_be_bytes());
        b.extend((32 + corpo.len() as u32).to_be_bytes());
        for v in area {
            b.extend(v.to_be_bytes());
        }
        b.extend(corpo);
        b
    }

    fn lista(blocos: &[Vec<u8>]) -> Vec<u8> {
        let mut b = (blocos.len() as u32).to_be_bytes().to_vec();
        for bl in blocos {
            b.extend(bl);
        }
        b
    }

    fn polinomio(c: &[f64]) -> Vec<u8> {
        let mut b = ((c.len() - 1) as u32).to_be_bytes().to_vec();
        for k in c {
            b.extend(k.to_be_bytes());
        }
        b
    }

    fn tabela(t: &[u16]) -> Vec<u8> {
        let mut b = (t.len() as u32).to_be_bytes().to_vec();
        for v in t {
            b.extend(v.to_be_bytes());
        }
        b
    }

    /// A foto toda, todos os planos a partir de `plano`.
    fn toda(altura: u32, largura: u32, plano: u32, planos: u32) -> [u32; 8] {
        [0, 0, altura, largura, plano, planos, 1, 1]
    }

    /// Um RAW de dados inteiros, `largura × altura` pixels de `cpp` amostras.
    fn raw(
        cpp: usize,
        largura: usize,
        altura: usize,
        dados: Vec<u16>,
        pretos: &[u16],
        branco: u32,
    ) -> RawImage {
        RawImage::new(
            Camera::default(),
            PixU16::new_with(dados, largura * cpp, altura),
            cpp,
            [1.0; 4],
            RawPhotometricInterpretation::LinearRaw,
            Some(BlackLevel::new(pretos, 1, 1, cpp)),
            Some(WhiteLevel::new(vec![branco; cpp])),
            false,
        )
    }

    fn inteiros(raw: &RawImage) -> Vec<u16> {
        match &raw.data {
            RawImageData::Integer(d) => d.clone(),
            RawImageData::Float(_) => panic!("devia ser inteiro"),
        }
    }

    #[test]
    fn o_polinomio_age_sobre_o_valor_normalizado_e_volta_a_16_bits() {
        let mut r = raw(1, 2, 2, vec![0, 32768, 65535, 16384], &[0], 65535);
        let l = lista(&[bloco(
            MAP_POLYNOMIAL,
            toda(2, 2, 0, 1),
            &polinomio(&[0.0, 0.0, 0.0, 1.0]),
        )]);
        assert_eq!(aplicar(&mut r, &l), 1);
        let cubo = |v: f64| ((v / 65535.0).powi(3) * 65535.0).round() as u16;
        assert_eq!(inteiros(&r), vec![0, cubo(32768.0), 65535, cubo(16384.0)]);
        assert_eq!(r.whitelevel.as_vec(), vec![65535.0]);
        assert_eq!(r.blacklevel.as_vec(), vec![0.0]);
    }

    #[test]
    fn o_preto_e_o_branco_do_arquivo_entram_na_normalizacao() {
        // Preto 1000, branco 5000: abaixo do preto é 0, acima do branco é cheio.
        let mut r = raw(1, 4, 1, vec![500, 1000, 3000, 6000], &[1000], 5000);
        let l = lista(&[bloco(
            MAP_POLYNOMIAL,
            toda(1, 4, 0, 1),
            &polinomio(&[0.0, 1.0]),
        )]);
        assert_eq!(aplicar(&mut r, &l), 1);
        assert_eq!(inteiros(&r), vec![0, 0, 32768, 65535]);
        assert_eq!(
            r.whitelevel.as_vec(),
            vec![65535.0],
            "agora em 16 bits cheios"
        );
        assert_eq!(r.blacklevel.as_vec(), vec![0.0]);
    }

    #[test]
    fn a_tabela_e_indexada_pelo_valor_e_presa_no_ultimo_degrau() {
        let mut r = raw(1, 4, 1, vec![0, 1, 2, 3], &[0], 65535);
        let l = lista(&[bloco(MAP_TABLE, toda(1, 4, 0, 1), &tabela(&[10, 20, 30]))]);
        assert_eq!(ler(&l)[0].mapa, Mapa::Tabela(vec![10, 20, 30]));
        assert_eq!(aplicar(&mut r, &l), 1);
        assert_eq!(inteiros(&r), vec![10, 20, 30, 30]);
    }

    #[test]
    fn so_a_area_o_passo_e_os_planos_pedidos_mudam() {
        // 3×2 pixels RGB, todos 30000. Zera o verde (plano 1) da primeira
        // linha, de dois em dois pixels: (0,0) e (2,0).
        let mut r = raw(3, 3, 2, vec![30000; 18], &[0, 0, 0], 65535);
        let l = lista(&[bloco(
            MAP_POLYNOMIAL,
            [0, 0, 1, 3, 1, 1, 1, 2],
            &polinomio(&[0.0]),
        )]);
        assert_eq!(aplicar(&mut r, &l), 1);
        let d = inteiros(&r);
        let zerados: Vec<usize> = (0..18).filter(|i| d[*i] == 0).collect();
        assert_eq!(zerados, vec![1, 7], "o verde de (0,0) e de (2,0)");
        assert!(d.iter().all(|v| *v == 0 || *v == 30000));
    }

    #[test]
    fn os_opcodes_agem_em_sequencia_sobre_o_mesmo_buffer() {
        // Dobra e depois eleva ao quadrado: (2x)² = 4x², preso em 1.
        let mut r = raw(1, 2, 1, vec![6553, 40000], &[0], 65535);
        let l = lista(&[
            bloco(MAP_POLYNOMIAL, toda(1, 2, 0, 1), &polinomio(&[0.0, 2.0])),
            bloco(
                MAP_POLYNOMIAL,
                toda(1, 2, 0, 1),
                &polinomio(&[0.0, 0.0, 1.0]),
            ),
        ]);
        assert_eq!(aplicar(&mut r, &l), 2);
        let x = 6553.0 / 65535.0;
        assert_eq!(inteiros(&r)[0], (4.0 * x * x * 65535.0_f64).round() as u16);
        assert_eq!(
            inteiros(&r)[1],
            65535,
            "2·0,61 passa de 1, fica em 1, e 1² = 1"
        );
    }

    #[test]
    fn a_area_maior_que_a_foto_e_cortada_na_borda() {
        let mut r = raw(1, 2, 2, vec![100; 4], &[0], 65535);
        let l = lista(&[bloco(
            MAP_POLYNOMIAL,
            toda(9000, 9000, 0, 1),
            &polinomio(&[1.0]),
        )]);
        assert_eq!(aplicar(&mut r, &l), 1);
        assert_eq!(inteiros(&r), vec![65535; 4]);
    }

    #[test]
    fn os_outros_opcodes_da_lista_sao_pulados() {
        // WarpRectilinear (1) e GainMap (9) antes do polinômio.
        let l = lista(&[
            bloco(1, toda(1, 1, 0, 1), &[0; 12]),
            bloco(9, toda(1, 1, 0, 1), &[0; 40]),
            bloco(MAP_POLYNOMIAL, toda(1, 1, 0, 1), &polinomio(&[0.5])),
        ]);
        let ops = ler(&l);
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].mapa, Mapa::Polinomio(vec![0.5]));
    }

    #[test]
    fn passos_e_planos_zerados_valem_um() {
        let l = lista(&[bloco(
            MAP_POLYNOMIAL,
            [0, 0, 1, 1, 0, 0, 0, 0],
            &polinomio(&[0.0]),
        )]);
        let a = &ler(&l)[0].area;
        assert_eq!((a.planos, a.passo_linha, a.passo_coluna), (1, 1, 1));
    }

    #[test]
    fn tabela_vazia_ou_truncada_encerra_a_leitura() {
        let vazia = lista(&[bloco(MAP_TABLE, toda(1, 1, 0, 1), &tabela(&[]))]);
        assert!(ler(&vazia).is_empty());
        let mut curta = tabela(&[1, 2, 3]);
        curta.truncate(curta.len() - 1);
        assert!(ler(&lista(&[bloco(MAP_TABLE, toda(1, 1, 0, 1), &curta)])).is_empty());
    }

    #[test]
    fn preto_diferente_por_canal_ou_dado_em_ponto_flutuante_passa_como_veio() {
        let l = lista(&[bloco(MAP_POLYNOMIAL, toda(1, 1, 0, 3), &polinomio(&[0.0]))]);

        let mut r = raw(3, 1, 1, vec![500, 600, 700], &[0, 100, 0], 65535);
        assert_eq!(aplicar(&mut r, &l), 0);
        assert_eq!(inteiros(&r), vec![500, 600, 700]);
        assert_eq!(r.whitelevel.as_vec(), vec![65535.0; 3]);

        let mut r = raw(3, 1, 1, vec![500, 600, 700], &[0, 0, 0], 65535);
        r.data = RawImageData::Float(vec![0.1, 0.2, 0.3]);
        assert_eq!(aplicar(&mut r, &l), 0);
        assert!(matches!(&r.data, RawImageData::Float(f) if f == &vec![0.1, 0.2, 0.3]));
    }

    #[test]
    fn lista_sem_mapeamento_nao_toca_no_raw() {
        let mut r = raw(1, 1, 1, vec![4000], &[100], 8000);
        assert_eq!(aplicar(&mut r, &[]), 0);
        assert_eq!(
            aplicar(&mut r, &lista(&[bloco(1, toda(1, 1, 0, 1), &[])])),
            0
        );
        assert_eq!(inteiros(&r), vec![4000]);
        assert_eq!(
            r.whitelevel.as_vec(),
            vec![8000.0],
            "o branco do arquivo fica"
        );
    }

    #[test]
    fn lista_truncada_nao_quebra() {
        let lista = lista_de_polinomios(10, 10, &[[0.0, 1.0, 0.0, 0.0]]);
        assert!(ler(&lista[..lista.len() - 3]).is_empty());
        assert!(ler(&[]).is_empty());
    }
}
