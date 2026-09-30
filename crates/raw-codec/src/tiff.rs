//! Um leitor de TIFF do tamanho da pergunta: achar três etiquetas num DNG.
//!
//! O rawler lê o TIFF inteiro, mas não devolve nem o XMP nem a `OpcodeList2`
//! (os `dng_tags` do `RawImage` chegam vazios num DNG do Lightroom — medido em
//! 30/set/2026). As duas coisas estão a poucos bytes do começo do arquivo, e
//! ler só isso é mais simples do que abrir o rawler por dentro.

/// XMP embutido (`XMLPacket`).
pub const XMP: u16 = 0x02BC;
/// A orientação EXIF (1–8).
pub const ORIENTACAO: u16 = 0x0112;
/// As IFDs filhas: num DNG, é numa delas que mora a imagem do sensor.
const SUB_IFDS: u16 = 0x014A;
/// 0 = a imagem inteira; 1 = a prévia reduzida.
const NEW_SUBFILE_TYPE: u16 = 0x00FE;
/// Os opcodes aplicados depois da linearização (DNG 1.3, §6).
pub const OPCODE_LIST_2: u16 = 0xC741; // 51009

pub struct Tiff<'a> {
    bytes: &'a [u8],
    little: bool,
}

/// Uma entrada de IFD já resolvida para os bytes do valor.
struct Entrada<'a> {
    tag: u16,
    tipo: u16,
    quantos: u32,
    valor: &'a [u8],
}

impl<'a> Tiff<'a> {
    pub fn novo(bytes: &'a [u8]) -> Option<Self> {
        let little = match bytes.get(..4)? {
            [b'I', b'I', 42, 0] => true,
            [b'M', b'M', 0, 42] => false,
            _ => return None,
        };
        Some(Self { bytes, little })
    }

    fn u16(&self, em: usize) -> Option<u16> {
        let b: [u8; 2] = self.bytes.get(em..em + 2)?.try_into().ok()?;
        Some(if self.little {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    }

    fn u32(&self, em: usize) -> Option<u32> {
        let b: [u8; 4] = self.bytes.get(em..em + 4)?.try_into().ok()?;
        Some(if self.little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    }

    fn ifd0(&self) -> Option<usize> {
        self.u32(4).map(|o| o as usize)
    }

    fn entradas(&self, ifd: usize) -> Vec<Entrada<'a>> {
        let Some(n) = self.u16(ifd) else {
            return Vec::new();
        };
        (0..n as usize)
            .filter_map(|i| {
                let e = ifd + 2 + i * 12;
                let tag = self.u16(e)?;
                let tipo = self.u16(e + 2)?;
                let quantos = self.u32(e + 4)?;
                let tamanho = tamanho_do_tipo(tipo)?.checked_mul(quantos as usize)?;
                let inicio = if tamanho <= 4 {
                    e + 8
                } else {
                    self.u32(e + 8)? as usize
                };
                let valor = self.bytes.get(inicio..inicio.checked_add(tamanho)?)?;
                Some(Entrada {
                    tag,
                    tipo,
                    quantos,
                    valor,
                })
            })
            .collect()
    }

    fn valor(&self, ifd: usize, tag: u16) -> Option<&'a [u8]> {
        self.entradas(ifd)
            .into_iter()
            .find(|e| e.tag == tag)
            .map(|e| e.valor)
    }

    fn numero(&self, entrada: &Entrada, i: usize) -> Option<u32> {
        match entrada.tipo {
            3 => {
                let b: [u8; 2] = entrada.valor.get(i * 2..i * 2 + 2)?.try_into().ok()?;
                Some(if self.little {
                    u16::from_le_bytes(b)
                } else {
                    u16::from_be_bytes(b)
                } as u32)
            }
            4 | 13 => {
                let b: [u8; 4] = entrada.valor.get(i * 4..i * 4 + 4)?.try_into().ok()?;
                Some(if self.little {
                    u32::from_le_bytes(b)
                } else {
                    u32::from_be_bytes(b)
                })
            }
            _ => None,
        }
    }

    /// Uma etiqueta da IFD0 — onde o DNG guarda o XMP.
    pub fn da_ifd0(&self, tag: u16) -> Option<&'a [u8]> {
        self.valor(self.ifd0()?, tag)
    }

    /// Um número curto (SHORT/LONG) da IFD0.
    pub fn numero_da_ifd0(&self, tag: u16) -> Option<u32> {
        let entrada = self
            .entradas(self.ifd0()?)
            .into_iter()
            .find(|e| e.tag == tag)?;
        self.numero(&entrada, 0)
    }

    /// Uma etiqueta da IFD da imagem inteira (`NewSubfileType = 0`): a IFD0,
    /// se for ela, ou a primeira SubIFD que for.
    pub fn da_imagem_inteira(&self, tag: u16) -> Option<&'a [u8]> {
        let ifd0 = self.ifd0()?;
        let mut candidatas = vec![ifd0];
        if let Some(sub) = self.entradas(ifd0).into_iter().find(|e| e.tag == SUB_IFDS) {
            candidatas.extend(
                (0..sub.quantos as usize)
                    .filter_map(|i| self.numero(&sub, i))
                    .map(|o| o as usize),
            );
        }
        candidatas.into_iter().find_map(|ifd| {
            let entradas = self.entradas(ifd);
            let tipo = entradas
                .iter()
                .find(|e| e.tag == NEW_SUBFILE_TYPE)
                .and_then(|e| self.numero(e, 0))
                .unwrap_or(0);
            if tipo != 0 {
                return None;
            }
            entradas.into_iter().find(|e| e.tag == tag).map(|e| e.valor)
        })
    }
}

fn tamanho_do_tipo(tipo: u16) -> Option<usize> {
    Some(match tipo {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 | 13 => 4,
        5 | 10 | 12 => 8,
        _ => return None,
    })
}

#[cfg(test)]
pub(crate) mod testes {
    use super::*;

    /// Um TIFF little-endian mínimo: IFD0 com as etiquetas dadas (valores
    /// sempre fora da entrada) e, se `sub` vier, uma SubIFD com as dela.
    pub fn tiff_com(ifd0: &[(u16, u16, &[u8])], sub: &[(u16, u16, &[u8])]) -> Vec<u8> {
        fn ifd(saida: &mut Vec<u8>, etiquetas: &[(u16, u16, Vec<u8>)]) -> usize {
            let inicio = saida.len();
            let dados_em = inicio + 2 + etiquetas.len() * 12 + 4;
            let mut dados: Vec<u8> = Vec::new();
            saida.extend((etiquetas.len() as u16).to_le_bytes());
            for (tag, tipo, valor) in etiquetas {
                let unidade = tamanho_do_tipo(*tipo).unwrap();
                saida.extend(tag.to_le_bytes());
                saida.extend(tipo.to_le_bytes());
                saida.extend(((valor.len() / unidade) as u32).to_le_bytes());
                if valor.len() <= 4 {
                    let mut v = valor.clone();
                    v.resize(4, 0);
                    saida.extend(v);
                } else {
                    saida.extend(((dados_em + dados.len()) as u32).to_le_bytes());
                    dados.extend(valor);
                }
            }
            saida.extend(0u32.to_le_bytes());
            saida.extend(dados);
            inicio
        }
        let mut saida = b"II*\0\x08\0\0\0".to_vec();
        let mut do_ifd0: Vec<(u16, u16, Vec<u8>)> = ifd0
            .iter()
            .map(|(t, ti, v)| (*t, *ti, v.to_vec()))
            .collect();
        if sub.is_empty() {
            ifd(&mut saida, &do_ifd0);
            return saida;
        }
        // A SubIFD vai depois da IFD0; o deslocamento dela é calculado
        // montando a IFD0 uma vez com um valor provisório do mesmo tamanho.
        do_ifd0.push((SUB_IFDS, 4, 0u32.to_le_bytes().to_vec()));
        let mut prova = saida.clone();
        ifd(&mut prova, &do_ifd0);
        let sub_em = prova.len() as u32;
        do_ifd0.last_mut().unwrap().2 = sub_em.to_le_bytes().to_vec();
        ifd(&mut saida, &do_ifd0);
        let do_sub: Vec<(u16, u16, Vec<u8>)> =
            sub.iter().map(|(t, ti, v)| (*t, *ti, v.to_vec())).collect();
        assert_eq!(saida.len() as u32, sub_em);
        ifd(&mut saida, &do_sub);
        saida
    }

    #[test]
    fn acha_a_etiqueta_na_ifd0_e_na_imagem_inteira() {
        let bytes = tiff_com(
            &[
                (NEW_SUBFILE_TYPE, 4, &1u32.to_le_bytes()),
                (XMP, 1, b"<x:xmpmeta/>"),
            ],
            &[
                (NEW_SUBFILE_TYPE, 4, &0u32.to_le_bytes()),
                (OPCODE_LIST_2, 7, b"opcodes!"),
            ],
        );
        let tiff = Tiff::novo(&bytes).unwrap();
        assert_eq!(tiff.da_ifd0(XMP), Some(&b"<x:xmpmeta/>"[..]));
        assert_eq!(
            tiff.da_imagem_inteira(OPCODE_LIST_2),
            Some(&b"opcodes!"[..])
        );
        assert_eq!(tiff.numero_da_ifd0(NEW_SUBFILE_TYPE), Some(1));
    }

    #[test]
    fn nao_e_tiff_nao_e_nada() {
        assert!(Tiff::novo(b"\xFF\xD8\xFF\xE0").is_none());
    }
}
