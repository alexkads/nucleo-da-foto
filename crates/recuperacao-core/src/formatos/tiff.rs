//! TIFF e os RAW que moram nele: CR2, NEF, ARW, DNG, ORF, RW2.
//!
//! O TIFF não diz o próprio tamanho. Diz onde está cada coisa: cada IFD aponta
//! para os seus dados, para as faixas (strips) ou ladrilhos (tiles) da imagem,
//! para a prévia JPEG e para outros IFDs (SubIFDs, EXIF, GPS). O fim do arquivo
//! é **o ponto mais distante que alguém aponta**. Nos RAW é quase sempre o fim
//! dos dados do sensor, que ficam por último.
//!
//! 🔑 A marca sai da assinatura quando ela é própria (`CR` no byte 8 do CR2,
//! `IIRO` do ORF, `IIU` do RW2) e, nos demais, da tag `Make` ou da
//! `DNGVersion`.

use std::collections::HashSet;
use std::io::{self, Read, Seek};

use crate::leitor::Leitor;

use super::{Formato, MAIOR_FOTO};

/// Um TIFF menor que isto não é foto de câmera.
const MENOR_TIFF: u64 = 4096;
/// Quantos IFDs se visitam no máximo. Um RAW de verdade tem menos de dez;
/// o limite protege de um ciclo que o `HashSet` não pegue.
const MAIS_IFDS: usize = 64;
/// Quantas entradas um IFD pode ter. As câmeras usam algumas dezenas.
const MAIS_ENTRADAS: u16 = 1000;

const STRIP_OFFSETS: u16 = 273;
const STRIP_BYTE_COUNTS: u16 = 279;
const TILE_OFFSETS: u16 = 324;
const TILE_BYTE_COUNTS: u16 = 325;
const SUB_IFDS: u16 = 330;
const JPEG_OFFSET: u16 = 513;
const JPEG_LENGTH: u16 = 514;
const MAKE: u16 = 271;
const EXIF_IFD: u16 = 34665;
const GPS_IFD: u16 = 34853;
const DNG_VERSION: u16 = 50706;

pub fn parece(cabeca: &[u8]) -> bool {
    matches!(
        &cabeca[..4],
        b"II*\0" | b"MM\0*" | b"IIRO" | b"IIRS" | b"MMOR" | b"IIU\0"
    )
}

#[derive(Clone, Copy)]
struct Ordem(bool); // true = little endian (II)

impl Ordem {
    fn u16(self, b: [u8; 2]) -> u16 {
        if self.0 {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        }
    }
    fn u32(self, b: [u8; 4]) -> u32 {
        if self.0 {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        }
    }
}

/// Quantos bytes ocupa um valor de cada tipo TIFF.
fn tamanho_do_tipo(tipo: u16) -> Option<u64> {
    Some(match tipo {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 | 13 => 4,
        5 | 10 | 12 => 8,
        16..=18 => 8,
        _ => return None,
    })
}

struct Entrada {
    tag: u16,
    tipo: u16,
    contagem: u32,
    /// Os 4 bytes do campo de valor, crus.
    campo: [u8; 4],
}

struct Leitura<'a, R> {
    leitor: &'a mut Leitor<R>,
    inicio: u64,
    ordem: Ordem,
}

impl<R: Read + Seek> Leitura<'_, R> {
    fn bytes<const N: usize>(&mut self, deslocamento: u64) -> io::Result<Option<[u8; N]>> {
        let mut b = [0u8; N];
        Ok(self
            .leitor
            .ler_exato(self.inicio + deslocamento, &mut b)?
            .map(|_| b))
    }

    /// Os valores inteiros de uma entrada (SHORT ou LONG), onde quer que estejam.
    fn inteiros(&mut self, e: &Entrada) -> io::Result<Option<Vec<u64>>> {
        let largura = match e.tipo {
            3 => 2,
            4 | 13 => 4,
            _ => return Ok(None),
        };
        let n = e.contagem as usize;
        if n == 0 || n > 1 << 20 {
            return Ok(None);
        }
        let mut bruto = vec![0u8; n * largura];
        if bruto.len() <= 4 {
            let n = bruto.len();
            bruto.copy_from_slice(&e.campo[..n]);
        } else {
            let onde = self.ordem.u32(e.campo) as u64;
            if self
                .leitor
                .ler_exato(self.inicio + onde, &mut bruto)?
                .is_none()
            {
                return Ok(None);
            }
        }
        Ok(Some(
            bruto
                .chunks_exact(largura)
                .map(|c| {
                    if largura == 2 {
                        self.ordem.u16([c[0], c[1]]) as u64
                    } else {
                        self.ordem.u32([c[0], c[1], c[2], c[3]]) as u64
                    }
                })
                .collect(),
        ))
    }

    fn texto(&mut self, e: &Entrada) -> io::Result<String> {
        let n = (e.contagem as usize).min(64);
        let mut b = vec![0u8; n];
        if n <= 4 {
            b.copy_from_slice(&e.campo[..n]);
        } else {
            let onde = self.ordem.u32(e.campo) as u64;
            if self.leitor.ler_exato(self.inicio + onde, &mut b)?.is_none() {
                return Ok(String::new());
            }
        }
        Ok(String::from_utf8_lossy(&b)
            .trim_end_matches('\0')
            .trim()
            .to_string())
    }
}

pub fn medir<R: Read + Seek>(
    leitor: &mut Leitor<R>,
    inicio: u64,
) -> io::Result<Option<(u64, Formato)>> {
    let mut cabeca = [0u8; 16];
    if leitor.ler_exato(inicio, &mut cabeca)?.is_none() {
        return Ok(None);
    }
    let ordem = Ordem(&cabeca[..2] == b"II");
    let magica = ordem.u16([cabeca[2], cabeca[3]]);
    let formato = match (&cabeca[..4], magica) {
        (b"IIRO" | b"IIRS" | b"MMOR", _) => Some(Formato::Orf),
        (_, 0x55) => Some(Formato::Rw2),
        (_, 42) if &cabeca[8..10] == b"CR" => Some(Formato::Cr2),
        (_, 42) => None,
        _ => return Ok(None),
    };

    let primeiro = ordem.u32([cabeca[4], cabeca[5], cabeca[6], cabeca[7]]) as u64;
    let mut leitura = Leitura {
        leitor,
        inicio,
        ordem,
    };

    let mut fim: u64 = 16;
    let mut tem_imagem = false;
    let mut make = String::new();
    let mut dng = false;
    let mut pendentes = vec![primeiro];
    let mut vistos = HashSet::new();

    while let Some(ifd) = pendentes.pop() {
        if !(8..MAIOR_FOTO).contains(&ifd) || !vistos.insert(ifd) {
            continue;
        }
        if vistos.len() > MAIS_IFDS {
            return Ok(None);
        }
        let Some(b) = leitura.bytes::<2>(ifd)? else {
            return Ok(None);
        };
        let n = ordem.u16(b);
        if n == 0 || n > MAIS_ENTRADAS {
            // O primeiro IFD vazio é estrutura quebrada; um encadeado vazio é
            // só o fim da corrente escrito de um jeito diferente.
            if ifd == primeiro {
                return Ok(None);
            }
            continue;
        }
        let fim_do_ifd = ifd + 2 + n as u64 * 12 + 4;
        fim = fim.max(fim_do_ifd);

        let mut faixas: (Option<Vec<u64>>, Option<Vec<u64>>) = (None, None);
        let mut ladrilhos: (Option<Vec<u64>>, Option<Vec<u64>>) = (None, None);
        let mut jpeg: (Option<u64>, Option<u64>) = (None, None);

        for k in 0..n as u64 {
            let Some(b) = leitura.bytes::<12>(ifd + 2 + k * 12)? else {
                return Ok(None);
            };
            let e = Entrada {
                tag: ordem.u16([b[0], b[1]]),
                tipo: ordem.u16([b[2], b[3]]),
                contagem: ordem.u32([b[4], b[5], b[6], b[7]]),
                campo: [b[8], b[9], b[10], b[11]],
            };
            // Os dados da própria entrada, quando não cabem no campo.
            if let Some(largura) = tamanho_do_tipo(e.tipo) {
                let tamanho = largura * e.contagem as u64;
                if tamanho > 4 {
                    let onde = ordem.u32(e.campo) as u64;
                    if onde + tamanho <= MAIOR_FOTO {
                        fim = fim.max(onde + tamanho);
                    }
                }
            }
            match e.tag {
                STRIP_OFFSETS => faixas.0 = leitura.inteiros(&e)?,
                STRIP_BYTE_COUNTS => faixas.1 = leitura.inteiros(&e)?,
                TILE_OFFSETS => ladrilhos.0 = leitura.inteiros(&e)?,
                TILE_BYTE_COUNTS => ladrilhos.1 = leitura.inteiros(&e)?,
                JPEG_OFFSET => jpeg.0 = leitura.inteiros(&e)?.and_then(|v| v.first().copied()),
                JPEG_LENGTH => jpeg.1 = leitura.inteiros(&e)?.and_then(|v| v.first().copied()),
                SUB_IFDS | EXIF_IFD | GPS_IFD => {
                    // O SubIFD às vezes vem com o tipo 13 (IFD), às vezes LONG.
                    if let Some(v) = leitura.inteiros(&e)? {
                        pendentes.extend(v);
                    }
                }
                MAKE if make.is_empty() => make = leitura.texto(&e)?,
                DNG_VERSION => dng = true,
                _ => {}
            }
        }

        for (offsets, contagens) in [faixas, ladrilhos] {
            if let (Some(offsets), Some(contagens)) = (offsets, contagens) {
                for (o, c) in offsets.iter().zip(contagens.iter()) {
                    if *c > 0 && o + c <= MAIOR_FOTO {
                        fim = fim.max(o + c);
                        tem_imagem = true;
                    }
                }
            }
        }
        if let (Some(o), Some(c)) = jpeg {
            if c > 0 && o + c <= MAIOR_FOTO {
                fim = fim.max(o + c);
                tem_imagem = true;
            }
        }

        // O próximo IFD da corrente.
        if let Some(b) = leitura.bytes::<4>(ifd + 2 + n as u64 * 12)? {
            let proximo = ordem.u32(b) as u64;
            if proximo != 0 {
                pendentes.push(proximo);
            }
        }
    }

    if !tem_imagem || fim < MENOR_TIFF {
        return Ok(None);
    }
    let formato = formato.unwrap_or_else(|| marca(&make, dng));
    Ok(Some((fim, formato)))
}

fn marca(make: &str, dng: bool) -> Formato {
    if dng {
        return Formato::Dng;
    }
    let make = make.to_ascii_uppercase();
    if make.starts_with("NIKON") {
        Formato::Nef
    } else if make.starts_with("SONY") {
        Formato::Arw
    } else if make.starts_with("CANON") {
        Formato::Cr2
    } else if make.starts_with("OLYMPUS") || make.starts_with("OM DIGITAL") {
        Formato::Orf
    } else if make.starts_with("PANASONIC") || make.starts_with("LEICA") {
        Formato::Rw2
    } else {
        Formato::Tiff
    }
}

#[cfg(test)]
pub(crate) mod testes {
    use super::*;
    use std::io::Cursor;

    /// Um RAW em TIFF little-endian: IFD0 com Make e a prévia JPEG, um SubIFD
    /// com a imagem do sensor em faixas, e o sensor por último, como nas
    /// câmeras.
    pub fn raw_tiff(make: &str, cr2: bool, sensor: usize) -> Vec<u8> {
        let mut f = vec![0u8; 16];
        f[..4].copy_from_slice(b"II*\0");
        f[4..8].copy_from_slice(&16u32.to_le_bytes());
        if cr2 {
            f[8..12].copy_from_slice(b"CR\x02\0");
        }
        // IFD0 em 16: Make, JPEGOffset, JPEGLength, SubIFDs. 4 entradas.
        let ifd0 = 16u32;
        let n = 4u16;
        let depois_do_ifd = ifd0 + 2 + n as u32 * 12 + 4;
        let make_em = depois_do_ifd;
        let mut make_bytes = make.as_bytes().to_vec();
        make_bytes.push(0);
        let previa_em = make_em + make_bytes.len() as u32;
        let previa = vec![0x77u8; 3000];
        let subifd_em = previa_em + previa.len() as u32;
        let sub_n = 2u16;
        let sensor_em = subifd_em + 2 + sub_n as u32 * 12 + 4;

        let entrada = |tag: u16, tipo: u16, contagem: u32, valor: u32| {
            let mut e = Vec::new();
            e.extend(tag.to_le_bytes());
            e.extend(tipo.to_le_bytes());
            e.extend(contagem.to_le_bytes());
            e.extend(valor.to_le_bytes());
            e
        };
        f.extend(n.to_le_bytes());
        f.extend(entrada(MAKE, 2, make_bytes.len() as u32, make_em));
        f.extend(entrada(JPEG_OFFSET, 4, 1, previa_em));
        f.extend(entrada(JPEG_LENGTH, 4, 1, previa.len() as u32));
        f.extend(entrada(SUB_IFDS, 4, 1, subifd_em));
        f.extend(0u32.to_le_bytes());
        f.extend(&make_bytes);
        f.extend(&previa);
        f.extend(sub_n.to_le_bytes());
        f.extend(entrada(STRIP_OFFSETS, 4, 1, sensor_em));
        f.extend(entrada(STRIP_BYTE_COUNTS, 4, 1, sensor as u32));
        f.extend(0u32.to_le_bytes());
        assert_eq!(f.len() as u32, sensor_em);
        f.extend((0..sensor).map(|i| (i % 253) as u8));
        f
    }

    fn medir_em(bytes: Vec<u8>) -> Option<(u64, Formato)> {
        let total = bytes.len() as u64;
        let mut leitor = Leitor::novo(Cursor::new(bytes), Some(total));
        // Pelo `formatos::medir`, que é quem confere o fim contra o cartão.
        crate::formatos::medir(&mut leitor, 0, crate::formatos::Familia::Tiff).unwrap()
    }

    #[test]
    fn o_fim_e_o_fim_do_sensor() {
        let raw = raw_tiff("NIKON CORPORATION", false, 50_000);
        let tamanho = raw.len() as u64;
        let mut cartao = raw;
        cartao.extend([0u8; 8192]);
        assert_eq!(medir_em(cartao), Some((tamanho, Formato::Nef)));
    }

    #[test]
    fn a_marca_decide_a_extensao() {
        let raw = raw_tiff("SONY", false, 10_000);
        assert_eq!(medir_em(raw).unwrap().1, Formato::Arw);
        let raw = raw_tiff("Canon", true, 10_000);
        assert_eq!(medir_em(raw).unwrap().1, Formato::Cr2);
    }

    #[test]
    fn sensor_cortado_e_descartado() {
        let mut raw = raw_tiff("SONY", false, 50_000);
        raw.truncate(raw.len() - 1000);
        assert_eq!(medir_em(raw), None);
    }

    #[test]
    fn cabecalho_solto_e_descartado() {
        let mut lixo = b"II*\0\x08\0\0\0".to_vec();
        lixo.extend([0u8; 9000]);
        assert_eq!(medir_em(lixo), None);
    }
}
