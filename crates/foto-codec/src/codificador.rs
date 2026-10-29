//! O codificador de JPEG por dentro: **o mesmo arquivo que o `image` 0.25
//! escreve, byte a byte**, em faixas de linhas divididas entre os núcleos.
//!
//! # Por que um codificador nosso — 2026-10-28
//!
//! 🚨 **O do `image` levava 0,27 s numa foto de 24 MP** (M2 Pro), e é ele que
//! sai em toda exportação, em todo envio ao pós-venda e em toda foto do
//! fotolivro. Ele faz tudo numa thread só, lê cada pixel por `get_pixel` com
//! conferência de limite e escreve o arquivo um byte por chamada.
//!
//! 🔑 **O arquivo não pode mudar**: a foto que já foi exportada, a que o site
//! converte e a que vai ao laboratório têm de continuar as mesmas. Por isso
//! cada conta é a do `image`, na mesma ordem e com os mesmos tipos — a cor em
//! ponto fixo, a DCT inteira do libjpeg, a quantização em `f32` com
//! `round` — e só muda **como** ela é feita:
//!
//! - a DCT anda as oito linhas (e depois as oito colunas) de um bloco juntas,
//!   o que o compilador transforma em instruções vetoriais;
//! - cada faixa de linhas de blocos vira bits numa thread própria, e as faixas
//!   se juntam no fim, deslocadas até o bit em que a anterior parou (o JPEG
//!   sem marcas de reinício é um fluxo de bits só, sem fronteira de byte entre
//!   blocos);
//! - o `0xFF` do fluxo ganha o `0x00` de enchimento na junção, uma vez.
//!
//! O teste `o_arquivo_e_o_do_image_byte_a_byte` compara com o `image` em todo
//! tamanho de borda, toda qualidade e os dois tipos de pixel, e a régua de
//! fotos reais compara a foto inteira.
//!
//! A DCT é a do `jfdctint.c` do libjpeg 9a, traduzida pelo `image` — este
//! software é baseado em parte no trabalho do Independent JPEG Group.

/// A densidade do cabeçalho JFIF: a unidade (0 = só proporção, 1 = pontos por
/// polegada, 2 = por centímetro) e os valores na horizontal e na vertical.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Densidade {
    pub unidade: u8,
    pub x: u16,
    pub y: u16,
}

/// Até aqui de pixels, uma thread só: dividir não paga as threads.
#[cfg(not(target_arch = "wasm32"))]
const POUCOS_PIXELS: usize = 1 << 18;

/// O JPEG de `largura × altura` pixels de 8 bits com `canais` bytes cada (3,
/// RGB; 4, RGBA, com o alfa descartado), linha a linha e sem enchimento.
///
/// Os lados vêm já conferidos (de 1 a 65 535) e `pixels` com o tamanho certo.
pub(crate) fn codificar(
    pixels: &[u8],
    canais: usize,
    largura: u16,
    altura: u16,
    qualidade: u8,
    densidade: Densidade,
) -> Vec<u8> {
    let foto = Foto {
        pixels,
        canais,
        largura: usize::from(largura),
        altura: usize::from(altura),
    };
    let tabelas = Tabelas::da_qualidade(qualidade);

    let mut saida = Saida::com_capacidade(foto.largura * foto.altura / 2 + 1024);
    cabecalho(&mut saida.bytes, largura, altura, &tabelas, densidade);

    let linhas_de_blocos = foto.altura.div_ceil(8);
    for (bytes, bits) in faixas_em_bits(&foto, &tabelas, linhas_de_blocos) {
        saida.juntar(&bytes, bits);
    }
    saida.fechar();
    saida.bytes
}

/// Os pixels e as medidas.
struct Foto<'a> {
    pixels: &'a [u8],
    canais: usize,
    largura: usize,
    altura: usize,
}

/// Os bits de cada faixa de linhas de blocos, na ordem da foto.
fn faixas_em_bits(foto: &Foto, tabelas: &Tabelas, linhas: usize) -> Vec<(Vec<u8>, u64)> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let nucleos = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .min(16);
        if nucleos > 1 && foto.largura * foto.altura > POUCOS_PIXELS {
            // Mais faixas que núcleos: os núcleos de eficiência terminam a
            // deles depois, e quem acaba antes pega a próxima.
            let por_faixa = linhas.div_ceil(nucleos * 4).max(1);
            let faixas = linhas.div_ceil(por_faixa);
            let proxima = std::sync::atomic::AtomicUsize::new(0);
            let mut prontas: Vec<(usize, (Vec<u8>, u64))> = std::thread::scope(|escopo| {
                let trabalhadores: Vec<_> = (0..nucleos.min(faixas))
                    .map(|_| {
                        escopo.spawn(|| {
                            let mut minhas = Vec::new();
                            loop {
                                let k = proxima.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                if k >= faixas {
                                    break minhas;
                                }
                                let de = k * por_faixa;
                                let ate = (de + por_faixa).min(linhas);
                                minhas.push((k, faixa_em_bits(foto, tabelas, de, ate)));
                            }
                        })
                    })
                    .collect();
                trabalhadores
                    .into_iter()
                    .flat_map(|t| t.join().expect("a faixa do JPEG não entra em pânico"))
                    .collect()
            });
            prontas.sort_unstable_by_key(|(k, _)| *k);
            return prontas.into_iter().map(|(_, f)| f).collect();
        }
    }
    vec![faixa_em_bits(foto, tabelas, 0, linhas)]
}

/// As linhas de blocos `de..ate` em bits, sem o enchimento do `0xFF` (que
/// depende de onde a faixa cai no arquivo), e quantos bits valem.
fn faixa_em_bits(foto: &Foto, tabelas: &Tabelas, de: usize, ate: usize) -> (Vec<u8>, u64) {
    let colunas = foto.largura.div_ceil(8);
    let mut bits = Bits::com_capacidade((ate - de) * colunas * 64);
    let mut blocos = Blocos::default();

    // A predição do DC continua de onde a faixa anterior parou: o último bloco
    // da linha de cima.
    let mut anterior = [0i32; 3];
    if de > 0 {
        blocos.ler(foto, (colunas - 1) * 8, (de - 1) * 8);
        for (c, dc) in anterior.iter_mut().enumerate() {
            *dc = blocos.quantizar(c, tabelas)[0];
        }
    }

    for linha in de..ate {
        for coluna in 0..colunas {
            blocos.ler(foto, coluna * 8, linha * 8);
            for (c, dc) in anterior.iter_mut().enumerate() {
                let bloco = blocos.quantizar(c, tabelas);
                let huffman = if c == 0 {
                    &tabelas.luma
                } else {
                    &tabelas.croma
                };
                *dc = bits.bloco(&bloco, *dc, huffman);
            }
        }
    }
    bits.terminar()
}

/// Os três canais de um bloco 8×8 (Y, Cb, Cr) e a DCT de um deles.
#[derive(Default)]
struct Blocos {
    amostras: [[[i32; 8]; 8]; 3],
}

impl Blocos {
    /// O bloco que começa em `(x0, y0)`, em YCbCr. Fora da foto vale o pixel
    /// mais perto — a borda repetida, como no `image`.
    fn ler(&mut self, foto: &Foto, x0: usize, y0: usize) {
        let [y, cb, cr] = &mut self.amostras;
        let dentro = x0 + 8 <= foto.largura;
        for linha in 0..8 {
            let sy = (y0 + linha).min(foto.altura - 1);
            let fila =
                &foto.pixels[sy * foto.largura * foto.canais..][..foto.largura * foto.canais];
            for coluna in 0..8 {
                let sx = if dentro {
                    x0 + coluna
                } else {
                    (x0 + coluna).min(foto.largura - 1)
                };
                let p = &fila[sx * foto.canais..sx * foto.canais + 3];
                let (yc, b, r) = rgb_para_ycbcr(p[0], p[1], p[2]);
                y[linha][coluna] = yc;
                cb[linha][coluna] = b;
                cr[linha][coluna] = r;
            }
        }
    }

    /// O canal `c` transformado e quantizado, em zigue-zague.
    fn quantizar(&self, c: usize, tabelas: &Tabelas) -> [i32; 64] {
        let dct = fdct(&self.amostras[c]);
        let divisores = if c == 0 {
            &tabelas.quant_luma
        } else {
            &tabelas.quant_croma
        };
        let mut natural = [0i32; 64];
        for ((q, &d), &divisor) in natural.iter_mut().zip(dct.as_flattened()).zip(divisores) {
            // A conta do `image`: a DCT vem escalada por 8, e a divisão é
            // em `f32`, arredondada para longe do zero.
            *q = ((d / 8) as f32 / divisor).round() as i32;
        }
        let mut zz = [0i32; 64];
        for (z, &i) in zz.iter_mut().zip(&ZIGUEZAGUE) {
            *z = natural[usize::from(i)];
        }
        zz
    }
}

/// RGB para YCbCr de 8 bits, BT.601 em ponto fixo — a do `image`.
#[inline(always)]
fn rgb_para_ycbcr(r: u8, g: u8, b: u8) -> (i32, i32, i32) {
    const C_YR: i32 = 19595;
    const C_YG: i32 = 38469;
    const C_YB: i32 = 7471;
    const Y_ARREDONDA: i32 = (1 << 15) - 1;
    const C_UR: i32 = 11059;
    const C_UG: i32 = 21709;
    const C_UB: i32 = 32768;
    const UV_ARREDONDA: i32 = (128 * (1 << 16)) + ((1 << 15) - 1);
    const C_VR: i32 = C_UB;
    const C_VG: i32 = 27439;
    const C_VB: i32 = 5329;
    let (r, g, b) = (i32::from(r), i32::from(g), i32::from(b));
    let y = (C_YR * r + C_YG * g + C_YB * b + Y_ARREDONDA) >> 16;
    let cb = (-C_UR * r - C_UG * g + C_UB * b + UV_ARREDONDA) >> 16;
    let cr = (C_VR * r - C_VG * g - C_VB * b + UV_ARREDONDA) >> 16;
    // O `image` guarda em `u8`: o mesmo corte.
    (i32::from(y as u8), i32::from(cb as u8), i32::from(cr as u8))
}

const CONST_BITS: i32 = 13;
const PASS1_BITS: i32 = 2;
const FIX_0_298631336: i32 = 2446;
const FIX_0_390180644: i32 = 3196;
const FIX_0_541196100: i32 = 4433;
const FIX_0_765366865: i32 = 6270;
const FIX_0_899976223: i32 = 7373;
const FIX_1_175875602: i32 = 9633;
const FIX_1_501321110: i32 = 12_299;
const FIX_1_847759065: i32 = 15_137;
const FIX_1_961570560: i32 = 16_069;
const FIX_2_053119869: i32 = 16_819;
const FIX_2_562915447: i32 = 20_995;
const FIX_3_072711026: i32 = 25_172;

/// Oito valores de uma vez: a mesma conta em cada via.
type Vias = [i32; 8];

#[inline(always)]
fn vias(f: impl Fn(usize) -> i32) -> Vias {
    std::array::from_fn(f)
}

/// A DCT inteira do libjpeg (`jfdctint.c`), a mesma do `image`: as linhas,
/// depois as colunas, com os mesmos arredondamentos — mas as oito linhas (e
/// depois as oito colunas) juntas, uma por via.
///
/// `s[linha][coluna]` são as amostras de 0 a 255; a saída, `[linha][coluna]`,
/// vem escalada por 8, como a do `image`.
fn fdct(s: &[[i32; 8]; 8]) -> [[i32; 8]; 8] {
    // As colunas de cada linha, transpostas: `e[c][linha]`.
    let e: [Vias; 8] = std::array::from_fn(|c| vias(|l| s[l][c]));
    let linhas = passada(&e, 1);
    // `linhas[k][linha]` é o coeficiente `k` de cada linha; a segunda passada
    // anda as colunas, então as vias passam a ser as colunas.
    let t: [Vias; 8] = std::array::from_fn(|l| vias(|k| linhas[k][l]));
    let colunas = passada(&t, 2);
    // `colunas[k][coluna]`: o coeficiente `k` (na vertical) de cada coluna.
    colunas
}

/// Uma passada da DCT sobre oito vias: `e[i]` é a i-ésima entrada de cada
/// via, e a saída `[k]` o coeficiente `k` de cada via. A primeira passada (as
/// linhas) desloca o nível e sobe `PASS1_BITS`; a segunda os tira.
#[inline(always)]
fn passada(e: &[Vias; 8], qual: u8) -> [Vias; 8] {
    let soma = |a: usize, b: usize| vias(|v| e[a][v] + e[b][v]);
    let dif = |a: usize, b: usize| vias(|v| e[a][v] - e[b][v]);
    let t0 = soma(0, 7);
    let t1 = soma(1, 6);
    let t2 = soma(2, 5);
    let t3 = soma(3, 4);
    let d0 = dif(0, 7);
    let d1 = dif(1, 6);
    let d2 = dif(2, 5);
    let d3 = dif(3, 4);

    let mut s = [[0i32; 8]; 8];
    if qual == 1 {
        let desloca = (CONST_BITS - PASS1_BITS) as u32;
        let meio = 1 << (CONST_BITS - PASS1_BITS - 1);
        for v in 0..8 {
            let t10 = t0[v] + t3[v];
            let t12 = t0[v] - t3[v];
            let t11 = t1[v] + t2[v];
            let t13 = t1[v] - t2[v];
            s[0][v] = (t10 + t11 - 8 * 128) << PASS1_BITS;
            s[4][v] = (t10 - t11) << PASS1_BITS;
            let z1 = (t12 + t13) * FIX_0_541196100 + meio;
            s[2][v] = (z1 + t12 * FIX_0_765366865) >> desloca;
            s[6][v] = (z1 - t13 * FIX_1_847759065) >> desloca;
            impares(&mut s, v, [d0[v], d1[v], d2[v], d3[v]], desloca);
        }
    } else {
        let desloca = (CONST_BITS + PASS1_BITS) as u32;
        let meio = 1 << (CONST_BITS + PASS1_BITS - 1);
        for v in 0..8 {
            let t10 = t0[v] + t3[v] + (1 << (PASS1_BITS - 1));
            let t12 = t0[v] - t3[v];
            let t11 = t1[v] + t2[v];
            let t13 = t1[v] - t2[v];
            s[0][v] = (t10 + t11) >> PASS1_BITS;
            s[4][v] = (t10 - t11) >> PASS1_BITS;
            let z1 = (t12 + t13) * FIX_0_541196100 + meio;
            s[2][v] = (z1 + t12 * FIX_0_765366865) >> desloca;
            s[6][v] = (z1 - t13 * FIX_1_847759065) >> desloca;
            impares(&mut s, v, [d0[v], d1[v], d2[v], d3[v]], desloca);
        }
    }
    s
}

/// A parte ímpar da DCT (coeficientes 1, 3, 5 e 7), igual nas duas passadas
/// fora o deslocamento.
///
/// 🚨 O arredondamento da parte ímpar é o da primeira passada **nas duas** — é
/// como o `image` faz, e o arquivo é o dele.
#[inline(always)]
fn impares(s: &mut [Vias; 8], v: usize, [t0, t1, t2, t3]: [i32; 4], desloca: u32) {
    let t12 = t0 + t2;
    let t13 = t1 + t3;
    let z1 = (t12 + t13) * FIX_1_175875602 + (1 << (CONST_BITS - PASS1_BITS - 1));
    let t12 = t12 * (-FIX_0_390180644) + z1;
    let t13 = t13 * (-FIX_1_961570560) + z1;

    let z1 = (t0 + t3) * (-FIX_0_899976223);
    let t0 = t0 * FIX_1_501321110 + z1 + t12;
    let t3 = t3 * FIX_0_298631336 + z1 + t13;

    let z1 = (t1 + t2) * (-FIX_2_562915447);
    let t1 = t1 * FIX_3_072711026 + z1 + t13;
    let t2 = t2 * FIX_2_053119869 + z1 + t12;

    s[1][v] = t0 >> desloca;
    s[3][v] = t1 >> desloca;
    s[5][v] = t2 >> desloca;
    s[7][v] = t3 >> desloca;
}

/// A ordem do zigue-zague: a posição natural do k-ésimo coeficiente.
#[rustfmt::skip]
const ZIGUEZAGUE: [u8; 64] = [
     0,  1,  8, 16,  9,  2,  3, 10,
    17, 24, 32, 25, 18, 11,  4,  5,
    12, 19, 26, 33, 40, 48, 41, 34,
    27, 20, 13,  6,  7, 14, 21, 28,
    35, 42, 49, 56, 57, 50, 43, 36,
    29, 22, 15, 23, 30, 37, 44, 51,
    58, 59, 52, 45, 38, 31, 39, 46,
    53, 60, 61, 54, 47, 55, 62, 63,
];

#[rustfmt::skip]
const QUANT_LUMA: [u8; 64] = [
    16, 11, 10, 16,  24,  40,  51,  61,
    12, 12, 14, 19,  26,  58,  60,  55,
    14, 13, 16, 24,  40,  57,  69,  56,
    14, 17, 22, 29,  51,  87,  80,  62,
    18, 22, 37, 56,  68, 109, 103,  77,
    24, 35, 55, 64,  81, 104, 113,  92,
    49, 64, 78, 87, 103, 121, 120, 101,
    72, 92, 95, 98, 112, 100, 103,  99,
];

#[rustfmt::skip]
const QUANT_CROMA: [u8; 64] = [
    17, 18, 24, 47, 99, 99, 99, 99,
    18, 21, 26, 66, 99, 99, 99, 99,
    24, 26, 56, 99, 99, 99, 99, 99,
    47, 66, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99,
];

// As tabelas de Huffman do anexo K (K.3 a K.6), as que o `image` grava.
const LUMA_DC_TAMANHOS: [u8; 16] = [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0];
const CROMA_DC_TAMANHOS: [u8; 16] = [0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0];
const DC_VALORES: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
const LUMA_AC_TAMANHOS: [u8; 16] = [0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7D];
#[rustfmt::skip]
const LUMA_AC_VALORES: [u8; 162] = [
    0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07,
    0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xA1, 0x08, 0x23, 0x42, 0xB1, 0xC1, 0x15, 0x52, 0xD1, 0xF0,
    0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0A, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x25, 0x26, 0x27, 0x28,
    0x29, 0x2A, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49,
    0x4A, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69,
    0x6A, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89,
    0x8A, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7,
    0xA8, 0xA9, 0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA, 0xC2, 0xC3, 0xC4, 0xC5,
    0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA, 0xE1, 0xE2,
    0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA, 0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8,
    0xF9, 0xFA,
];
const CROMA_AC_TAMANHOS: [u8; 16] = [0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77];
#[rustfmt::skip]
const CROMA_AC_VALORES: [u8; 162] = [
    0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71,
    0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xA1, 0xB1, 0xC1, 0x09, 0x23, 0x33, 0x52, 0xF0,
    0x15, 0x62, 0x72, 0xD1, 0x0A, 0x16, 0x24, 0x34, 0xE1, 0x25, 0xF1, 0x17, 0x18, 0x19, 0x1A, 0x26,
    0x27, 0x28, 0x29, 0x2A, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48,
    0x49, 0x4A, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68,
    0x69, 0x6A, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87,
    0x88, 0x89, 0x8A, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0xA2, 0xA3, 0xA4, 0xA5,
    0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA, 0xC2, 0xC3,
    0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA,
    0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8,
    0xF9, 0xFA,
];

/// Os códigos de Huffman de uma tabela (anexo C.2): `(tamanho, código)` por
/// símbolo; `(17, 0)` para o símbolo que a tabela não tem.
const fn codigos(tamanhos: &[u8; 16], valores: &[u8]) -> [(u8, u16); 256] {
    let mut por_simbolo = [(17u8, 0u16); 256];
    let mut codigo = 0u16;
    let mut k = 0;
    let mut tamanho = 1;
    while tamanho <= 16 {
        let mut j = 0;
        while j < tamanhos[tamanho - 1] {
            por_simbolo[valores[k] as usize] = (tamanho as u8, codigo);
            codigo += 1;
            k += 1;
            j += 1;
        }
        codigo <<= 1;
        tamanho += 1;
    }
    por_simbolo
}

const LUMA_DC: [(u8, u16); 256] = codigos(&LUMA_DC_TAMANHOS, &DC_VALORES);
const LUMA_AC: [(u8, u16); 256] = codigos(&LUMA_AC_TAMANHOS, &LUMA_AC_VALORES);
const CROMA_DC: [(u8, u16); 256] = codigos(&CROMA_DC_TAMANHOS, &DC_VALORES);
const CROMA_AC: [(u8, u16); 256] = codigos(&CROMA_AC_TAMANHOS, &CROMA_AC_VALORES);

/// As tabelas de Huffman de um canal.
struct Huffman {
    dc: &'static [(u8, u16); 256],
    ac: &'static [(u8, u16); 256],
}

/// As tabelas de uma qualidade: quantização (em `f32`, como o `image` divide)
/// e Huffman.
struct Tabelas {
    /// As tabelas de quantização escaladas, na ordem natural, como vão ao DQT.
    gravadas: [[u8; 64]; 2],
    quant_luma: [f32; 64],
    quant_croma: [f32; 64],
    luma: Huffman,
    croma: Huffman,
}

impl Tabelas {
    /// A escala do libjpeg, a do `image`: qualidade de 1 a 100.
    fn da_qualidade(qualidade: u8) -> Self {
        let q = u32::from(qualidade.clamp(1, 100));
        let escala = if q < 50 { 5000 / q } else { 200 - q * 2 };
        let escalar = |base: &[u8; 64]| -> [u8; 64] {
            std::array::from_fn(|i| ((u32::from(base[i]) * escala + 50) / 100).clamp(1, 255) as u8)
        };
        let gravadas = [escalar(&QUANT_LUMA), escalar(&QUANT_CROMA)];
        Tabelas {
            quant_luma: gravadas[0].map(f32::from),
            quant_croma: gravadas[1].map(f32::from),
            gravadas,
            luma: Huffman {
                dc: &LUMA_DC,
                ac: &LUMA_AC,
            },
            croma: Huffman {
                dc: &CROMA_DC,
                ac: &CROMA_AC,
            },
        }
    }
}

/// Do SOI ao SOS, na ordem e com os campos do `image`.
fn cabecalho(m: &mut Vec<u8>, largura: u16, altura: u16, t: &Tabelas, d: Densidade) {
    fn segmento(m: &mut Vec<u8>, marca: u8, dados: &[u8]) {
        m.extend_from_slice(&[0xFF, marca]);
        m.extend_from_slice(&(dados.len() as u16 + 2).to_be_bytes());
        m.extend_from_slice(dados);
    }
    m.extend_from_slice(&[0xFF, 0xD8]);

    let mut jfif = b"JFIF\0\x01\x02".to_vec();
    jfif.push(d.unidade);
    jfif.extend_from_slice(&d.x.to_be_bytes());
    jfif.extend_from_slice(&d.y.to_be_bytes());
    jfif.extend_from_slice(&[0, 0]);
    segmento(m, 0xE0, &jfif);

    let mut quadro = vec![8];
    quadro.extend_from_slice(&altura.to_be_bytes());
    quadro.extend_from_slice(&largura.to_be_bytes());
    quadro.extend_from_slice(&[3, 1, 0x11, 0, 2, 0x11, 1, 3, 0x11, 1]);
    segmento(m, 0xC0, &quadro);

    for (i, tabela) in t.gravadas.iter().enumerate() {
        let mut dqt = vec![i as u8];
        dqt.extend(ZIGUEZAGUE.iter().map(|&k| tabela[usize::from(k)]));
        segmento(m, 0xDB, &dqt);
    }

    for (classe_destino, tamanhos, valores) in [
        (0x00, &LUMA_DC_TAMANHOS, &DC_VALORES[..]),
        (0x10, &LUMA_AC_TAMANHOS, &LUMA_AC_VALORES[..]),
        (0x01, &CROMA_DC_TAMANHOS, &DC_VALORES[..]),
        (0x11, &CROMA_AC_TAMANHOS, &CROMA_AC_VALORES[..]),
    ] {
        let mut dht = vec![classe_destino];
        dht.extend_from_slice(tamanhos);
        dht.extend_from_slice(valores);
        segmento(m, 0xC4, &dht);
    }

    segmento(m, 0xDA, &[3, 1, 0x00, 2, 0x11, 3, 0x11, 0, 63, 0]);
}

/// Os bits de uma faixa, do mais alto para o mais baixo, sem enchimento.
struct Bits {
    bytes: Vec<u8>,
    /// Os bits ainda não gravados, nos `n` bits de baixo.
    acumulado: u64,
    n: u32,
}

impl Bits {
    fn com_capacidade(bytes: usize) -> Self {
        Bits {
            bytes: Vec::with_capacity(bytes),
            acumulado: 0,
            n: 0,
        }
    }

    /// Os `tamanho` bits de baixo de `valor` (até 32).
    #[inline(always)]
    fn por(&mut self, valor: u32, tamanho: u32) {
        self.acumulado = (self.acumulado << tamanho) | u64::from(valor);
        self.n += tamanho;
        if self.n >= 32 {
            self.n -= 32;
            let palavra = (self.acumulado >> self.n) as u32;
            self.bytes.extend_from_slice(&palavra.to_be_bytes());
        }
    }

    /// O símbolo de Huffman seguido dos `tamanho` bits do valor, de uma vez.
    #[inline(always)]
    fn simbolo(&mut self, tabela: &[(u8, u16); 256], simbolo: u8, valor: u16, tamanho: u32) {
        let (n, codigo) = tabela[usize::from(simbolo)];
        // Um símbolo fora da tabela (`n = 17`) não acontece: o DC de 8 bits
        // cabe em 11 bits e o AC em 10, e as tabelas do anexo K têm todos.
        self.por(
            (u32::from(codigo) << tamanho) | u32::from(valor),
            u32::from(n) + tamanho,
        );
    }

    /// Um bloco em zigue-zague, com a predição do DC; devolve o DC dele, a
    /// predição do próximo. É o `write_block` do `image`.
    #[inline(always)]
    fn bloco(&mut self, zz: &[i32; 64], anterior: i32, huffman: &Huffman) -> i32 {
        let (tamanho, valor) = coeficiente(zz[0] - anterior);
        self.simbolo(huffman.dc, tamanho, valor, u32::from(tamanho));

        // Os coeficientes que não são zero, um bit cada (do 1 ao 63).
        let mut nao_zeros = 0u64;
        for (k, &c) in zz.iter().enumerate().skip(1) {
            nao_zeros |= u64::from(c != 0) << k;
        }
        let mut ultimo = 0;
        while nao_zeros != 0 {
            let k = nao_zeros.trailing_zeros() as usize;
            nao_zeros &= nao_zeros - 1;
            let mut corrida = k - ultimo - 1;
            while corrida > 15 {
                self.simbolo(huffman.ac, 0xF0, 0, 0);
                corrida -= 16;
            }
            let (tamanho, valor) = coeficiente(zz[k]);
            self.simbolo(
                huffman.ac,
                ((corrida as u8) << 4) | tamanho,
                valor,
                u32::from(tamanho),
            );
            ultimo = k;
        }
        if zz[63] == 0 {
            self.simbolo(huffman.ac, 0x00, 0, 0);
        }
        zz[0]
    }

    /// Os bytes (o último completado com zeros) e quantos bits valem.
    fn terminar(mut self) -> (Vec<u8>, u64) {
        let bits = self.bytes.len() as u64 * 8 + u64::from(self.n);
        while self.n > 0 {
            let tamanho = self.n.min(8);
            self.n -= tamanho;
            let byte = ((self.acumulado >> self.n) as u8) << (8 - tamanho);
            self.bytes.push(byte);
        }
        (self.bytes, bits)
    }
}

/// O tamanho em bits de um coeficiente e os bits dele — o `encode_coefficient`
/// do `image` (o negativo vai em complemento de um).
#[inline(always)]
fn coeficiente(c: i32) -> (u8, u16) {
    let magnitude = c.unsigned_abs() as u16;
    let tamanho = (16 - magnitude.leading_zeros()) as u8;
    let mascara = ((1u32 << tamanho) - 1) as u16;
    let valor = if c < 0 {
        (c - 1) as u16 & mascara
    } else {
        c as u16 & mascara
    };
    (tamanho, valor)
}

/// O arquivo: o cabeçalho e as faixas juntadas, com o enchimento do `0xFF`.
struct Saida {
    bytes: Vec<u8>,
    /// Os bits que ainda não fecharam um byte, nos `n` de baixo (`n < 8`).
    pendente: u32,
    n: u32,
}

impl Saida {
    fn com_capacidade(bytes: usize) -> Self {
        Saida {
            bytes: Vec::with_capacity(bytes),
            pendente: 0,
            n: 0,
        }
    }

    /// Um byte do fluxo; o `0xFF` leva o `0x00` atrás, para não virar marca.
    #[inline(always)]
    fn byte(&mut self, b: u8) {
        self.bytes.push(b);
        if b == 0xFF {
            self.bytes.push(0);
        }
    }

    /// Os `bits` primeiros bits de `bytes`, a partir de onde o fluxo está.
    fn juntar(&mut self, bytes: &[u8], bits: u64) {
        let inteiros = (bits / 8) as usize;
        let resto = (bits % 8) as u32;
        if self.n == 0 {
            // Alinhado: os bytes vão como estão, com o enchimento.
            for pedaco in bytes[..inteiros].split_inclusive(|&b| b == 0xFF) {
                self.bytes.extend_from_slice(pedaco);
                if pedaco.last() == Some(&0xFF) {
                    self.bytes.push(0);
                }
            }
        } else {
            let n = self.n;
            for &b in &bytes[..inteiros] {
                let junto = (self.pendente << 8) | u32::from(b);
                self.byte((junto >> n) as u8);
                self.pendente = junto & ((1 << n) - 1);
            }
        }
        if resto > 0 {
            let alto = u32::from(bytes[inteiros] >> (8 - resto));
            self.pendente = (self.pendente << resto) | alto;
            self.n += resto;
            if self.n >= 8 {
                self.n -= 8;
                self.byte((self.pendente >> self.n) as u8);
                self.pendente &= (1 << self.n) - 1;
            }
        }
    }

    /// O último byte completado com uns (o `pad_byte` do `image`) e o EOI.
    fn fechar(&mut self) {
        if self.n > 0 {
            let completar = 8 - self.n;
            self.byte(((self.pendente << completar) | ((1 << completar) - 1)) as u8);
            self.n = 0;
        }
        self.bytes.extend_from_slice(&[0xFF, 0xD9]);
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use image::codecs::jpeg::{JpegEncoder, PixelDensity};
    use image::{ImageBuffer, Rgb, Rgba};

    /// O arquivo do `image`, o de antes desta mudança.
    fn do_image(pixels: &[u8], canais: usize, largura: u32, altura: u32, q: u8) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut codificador = JpegEncoder::new_with_quality(&mut bytes, q);
        codificador.set_pixel_density(PixelDensity::dpi(300));
        let feito = if canais == 3 {
            let foto = ImageBuffer::<Rgb<u8>, _>::from_raw(largura, altura, pixels).unwrap();
            codificador.encode_image(&foto)
        } else {
            let foto = ImageBuffer::<Rgba<u8>, _>::from_raw(largura, altura, pixels).unwrap();
            codificador.encode_image(&foto)
        };
        feito.expect("o image codifica");
        bytes
    }

    fn nosso(pixels: &[u8], canais: usize, largura: u32, altura: u32, q: u8) -> Vec<u8> {
        let d = Densidade {
            unidade: 1,
            x: 300,
            y: 300,
        };
        codificar(pixels, canais, largura as u16, altura as u16, q, d)
    }

    /// Pixels que o JPEG não comprime de graça: ruído de um gerador fixo, com
    /// faixas lisas e bordas duras no meio.
    fn pixels(largura: u32, altura: u32, canais: usize, semente: u64) -> Vec<u8> {
        let mut estado = semente.wrapping_mul(6364136223846793005).wrapping_add(1);
        let mut v = Vec::with_capacity((largura * altura) as usize * canais);
        for y in 0..altura {
            for x in 0..largura {
                for c in 0..canais {
                    estado = estado
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    let ruido = (estado >> 56) as u8;
                    let valor = match (x / 5 + y / 7) % 4 {
                        0 => ruido,
                        1 => (x * 9 + y * 3 + c as u32 * 50) as u8,
                        2 => {
                            if (x + y) % 2 == 0 {
                                255
                            } else {
                                0
                            }
                        }
                        _ => 255 - (y * 4) as u8,
                    };
                    v.push(valor);
                }
            }
        }
        v
    }

    /// 🚨 **O arquivo é o do `image`, byte a byte**: todo resto de bloco nas
    /// bordas (1 a 17 px, e lados que não são múltiplos de 8), as cem
    /// qualidades e os dois tipos de pixel.
    #[test]
    fn o_arquivo_e_o_do_image_byte_a_byte() {
        for canais in [3, 4] {
            for (largura, altura) in [
                (1, 1),
                (8, 8),
                (9, 7),
                (17, 3),
                (23, 16),
                (16, 23),
                (65, 33),
            ] {
                let px = pixels(largura, altura, canais, u64::from(largura * 31 + altura));
                for q in 1..=100 {
                    assert_eq!(
                        nosso(&px, canais, largura, altura, q),
                        do_image(&px, canais, largura, altura, q),
                        "{largura}×{altura}, {canais} canais, q{q}"
                    );
                }
            }
            for largura in 1..=17 {
                for altura in [1, 2, 7, 8, 9, 15, 16, 17] {
                    let px = pixels(largura, altura, canais, 7);
                    for q in [1, 50, 90, 100] {
                        assert_eq!(
                            nosso(&px, canais, largura, altura, q),
                            do_image(&px, canais, largura, altura, q),
                            "{largura}×{altura}, {canais} canais, q{q}"
                        );
                    }
                }
            }
        }
    }

    /// O pior caso dos coeficientes: xadrez de 0 e 255 na qualidade 100, onde
    /// a quantização divide por 1 e o AC chega perto dos 10 bits.
    #[test]
    fn o_xadrez_na_qualidade_cem_sai_igual() {
        for passo in [1, 2, 4] {
            let (largura, altura) = (40, 24);
            let mut px = Vec::new();
            for y in 0..altura {
                for x in 0..largura {
                    let v = if (x / passo + y / passo) % 2 == 0 {
                        255
                    } else {
                        0
                    };
                    px.extend_from_slice(&[v, 255 - v, v]);
                }
            }
            for q in [95, 100] {
                assert_eq!(
                    nosso(&px, 3, largura, altura, q),
                    do_image(&px, 3, largura, altura, q),
                    "passo {passo}, q{q}"
                );
            }
        }
    }

    /// Uma foto grande o bastante para ir em faixas, uma por núcleo: as
    /// junções caem no meio de um byte e no meio de um `0xFF`.
    #[test]
    fn em_faixas_sai_o_mesmo_arquivo() {
        let (largura, altura) = (701, 413);
        assert!((largura * altura) as usize > POUCOS_PIXELS);
        for canais in [3, 4] {
            let px = pixels(largura, altura, canais, 99);
            for q in [10, 90, 100] {
                assert_eq!(
                    nosso(&px, canais, largura, altura, q),
                    do_image(&px, canais, largura, altura, q),
                    "{canais} canais, q{q}"
                );
            }
        }
    }

    /// A junção das faixas, contra a conta de um bit por vez: alinhada e
    /// desalinhada, com `0xFF` que precisa do enchimento.
    #[test]
    fn juntar_faixas_e_o_mesmo_que_escrever_bit_a_bit() {
        let faixas: [(&[u8], u64); 5] = [
            (&[0xFF, 0xF0], 12),
            (&[0xAB, 0xFF, 0x80], 17),
            (&[0x3F, 0xFF], 16),
            (&[0xFF], 3),
            (&[0x12, 0x34, 0xFE], 23),
        ];
        let mut saida = Saida::com_capacidade(16);
        let mut bits = Vec::new();
        for (bytes, n) in faixas {
            saida.juntar(bytes, n);
            for i in 0..n as usize {
                bits.push((bytes[i / 8] >> (7 - i % 8)) & 1);
            }
        }
        saida.fechar();
        while bits.len() % 8 != 0 {
            bits.push(1);
        }
        let mut esperado = Vec::new();
        for byte in bits.chunks(8) {
            let b = byte.iter().fold(0u8, |a, &bit| (a << 1) | bit);
            esperado.push(b);
            if b == 0xFF {
                esperado.push(0);
            }
        }
        esperado.extend_from_slice(&[0xFF, 0xD9]);
        assert_eq!(saida.bytes, esperado);
    }

    #[test]
    fn os_codigos_de_huffman_sao_os_do_anexo_k() {
        // K.3: o DC do luma de tamanho 0 é `00`, o de 11 é `111111110`.
        assert_eq!(LUMA_DC[0], (2, 0b00));
        assert_eq!(LUMA_DC[11], (9, 0b1_1111_1110));
        // K.5: o fim de bloco é `1010`, e o ZRL `11111111001`.
        assert_eq!(LUMA_AC[0x00], (4, 0b1010));
        assert_eq!(LUMA_AC[0xF0], (11, 0b111_1111_1001));
        assert_eq!(LUMA_AC[0x0B].0, 17, "AC de 11 bits não existe");
    }
}
