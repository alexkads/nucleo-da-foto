//! JPEG: andar pelos segmentos até o `EOI` verdadeiro.
//!
//! 🚨 **O primeiro `FF D9` não é o fim.** A foto da câmera leva uma miniatura
//! JPEG inteira dentro do APP1 (o EXIF), com `FF D8` e `FF D9` próprios. Quem
//! procura o primeiro `FF D9` devolve a miniatura de 160×120 e descarta a foto.
//! Aqui os segmentos são pulados pelo comprimento declarado, e a miniatura
//! passa sem ser lida.
//!
//! Depois do `SOS` vêm os dados comprimidos, que não têm comprimento. Neles o
//! `FF` aparece sempre como `FF 00` (byte de enchimento) ou `FF D0`–`FF D7`
//! (reinício); qualquer outro `FF xx` é o próximo marcador. No JPEG progressivo
//! há vários `SOS`, e o laço só termina no `EOI`.

use std::io::{self, Read, Seek};

use crate::leitor::Leitor;

use super::MAIOR_FOTO;

/// O menor JPEG que vale a pena devolver. Abaixo disto é miniatura solta ou
/// lixo com cara de JPEG.
const MENOR_JPEG: u64 = 1024;

pub fn parece(cabeca: &[u8]) -> bool {
    // FF D8 FF e um marcador que começa arquivo de verdade: APPn, DQT, DHT,
    // SOF0 ou COM.
    cabeca[0] == 0xFF
        && cabeca[1] == 0xD8
        && cabeca[2] == 0xFF
        && matches!(cabeca[3], 0xE0..=0xEF | 0xDB | 0xC4 | 0xC0 | 0xFE)
}

fn e_sof(marcador: u8) -> bool {
    matches!(marcador, 0xC0..=0xCF) && !matches!(marcador, 0xC4 | 0xC8 | 0xCC)
}

pub fn medir<R: Read + Seek>(leitor: &mut Leitor<R>, inicio: u64) -> io::Result<Option<u64>> {
    let limite = inicio + MAIOR_FOTO;
    let mut pos = inicio + 2;
    let mut viu_sof = false;
    let mut viu_sos = false;

    loop {
        if pos >= limite {
            return Ok(None);
        }
        let Some(ff) = leitor.u8_em(pos)? else {
            return Ok(None);
        };
        if ff != 0xFF {
            return Ok(None);
        }
        let Some(marcador) = leitor.u8_em(pos + 1)? else {
            return Ok(None);
        };
        match marcador {
            // Enchimento entre marcadores.
            0xFF => {
                pos += 1;
                continue;
            }
            0xD9 => {
                let tamanho = pos + 2 - inicio;
                let completo = viu_sof && viu_sos && tamanho >= MENOR_JPEG;
                return Ok(completo.then_some(tamanho));
            }
            // Marcadores sem comprimento.
            0x01 | 0xD0..=0xD7 => {
                pos += 2;
                continue;
            }
            // Um segundo SOI no nível de cima é o começo de outro arquivo.
            0xD8 | 0x00 => return Ok(None),
            _ => {}
        }
        let Some(comprimento) = leitor.u16_be(pos + 2)? else {
            return Ok(None);
        };
        if comprimento < 2 {
            return Ok(None);
        }
        if e_sof(marcador) {
            viu_sof = true;
        }
        pos += 2 + comprimento as u64;
        if marcador == 0xDA {
            if !viu_sof {
                return Ok(None);
            }
            viu_sos = true;
            match fim_dos_dados(leitor, pos, limite)? {
                Some(proximo) => pos = proximo,
                None => return Ok(None),
            }
        }
    }
}

/// Anda pelos dados comprimidos e devolve a posição do próximo marcador.
fn fim_dos_dados<R: Read + Seek>(
    leitor: &mut Leitor<R>,
    mut pos: u64,
    limite: u64,
) -> io::Result<Option<u64>> {
    let mut buf = vec![0u8; 64 * 1024];
    // Se o bloco anterior terminou num FF, o par se completa no seguinte.
    let mut ff_pendente = false;
    while pos < limite {
        let n = leitor.ler_em(pos, &mut buf)?;
        if n == 0 {
            return Ok(None);
        }
        let mut i = 0;
        if ff_pendente {
            ff_pendente = false;
            if !matches!(buf[0], 0x00 | 0xD0..=0xD7) {
                return Ok(Some(pos - 1));
            }
            i = 1;
        }
        while i < n {
            if buf[i] == 0xFF {
                if i + 1 == n {
                    ff_pendente = true;
                    break;
                }
                let seguinte = buf[i + 1];
                if !matches!(seguinte, 0x00 | 0xD0..=0xD7) {
                    return Ok(Some(pos + i as u64));
                }
                i += 2;
            } else {
                i += 1;
            }
        }
        pos += n as u64;
    }
    Ok(None)
}

#[cfg(test)]
pub(crate) mod testes {
    use super::*;
    use std::io::Cursor;

    fn segmento(marcador: u8, corpo: &[u8]) -> Vec<u8> {
        let mut s = vec![0xFF, marcador];
        s.extend_from_slice(&((corpo.len() + 2) as u16).to_be_bytes());
        s.extend_from_slice(corpo);
        s
    }

    /// Um JPEG com a estrutura de uma câmera: APP1 com miniatura JPEG dentro,
    /// DQT, SOF0, DHT, SOS, dados com `FF 00` e reinícios, e `EOI`.
    pub fn jpeg_de_camera(semente: u8, dados: usize) -> Vec<u8> {
        let mut miniatura = vec![0xFF, 0xD8];
        miniatura.extend(segmento(0xDB, &[0; 65]));
        miniatura.extend(segmento(0xC0, &[8, 0, 16, 0, 16, 1, 1, 0x11, 0]));
        miniatura.extend(segmento(0xDA, &[1, 1, 0, 0, 63, 0]));
        miniatura.extend([0x12, 0x34, 0xFF, 0x00, 0x56]);
        miniatura.extend([0xFF, 0xD9]);

        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend(&miniatura);

        let mut j = vec![0xFF, 0xD8];
        j.extend(segmento(0xE1, &app1));
        j.extend(segmento(0xDB, &[semente; 65]));
        j.extend(segmento(0xC0, &[8, 1, 0, 1, 0, 1, 1, 0x11, 0]));
        j.extend(segmento(0xC4, &[0; 30]));
        j.extend(segmento(0xDA, &[1, 1, 0, 0, 63, 0]));
        for i in 0..dados {
            let b = (i as u8).wrapping_mul(31).wrapping_add(semente);
            j.push(b);
            if b == 0xFF {
                j.push(0x00);
            }
            if i % 4096 == 4095 {
                j.extend([0xFF, 0xD0 + ((i / 4096) % 8) as u8]);
            }
        }
        j.extend([0xFF, 0xD9]);
        j
    }

    fn medir_em(bytes: Vec<u8>) -> Option<u64> {
        let total = bytes.len() as u64;
        let mut leitor = Leitor::novo(Cursor::new(bytes), Some(total));
        medir(&mut leitor, 0).unwrap()
    }

    #[test]
    fn a_miniatura_embutida_nao_corta_a_foto() {
        let j = jpeg_de_camera(3, 20_000);
        let tamanho = j.len() as u64;
        let mut cartao = j;
        cartao.extend([0xAB; 4000]);
        assert_eq!(medir_em(cartao), Some(tamanho));
    }

    #[test]
    fn o_fim_atravessa_blocos_de_leitura() {
        let j = jpeg_de_camera(9, 3 * 1024 * 1024);
        let tamanho = j.len() as u64;
        assert_eq!(medir_em(j), Some(tamanho));
    }

    #[test]
    fn jpeg_cortado_e_descartado() {
        let mut j = jpeg_de_camera(1, 20_000);
        j.truncate(j.len() - 500);
        assert_eq!(medir_em(j), None);
    }

    #[test]
    fn assinatura_solta_e_descartada() {
        let mut lixo = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        lixo.extend([0x55; 5000]);
        assert_eq!(medir_em(lixo), None);
    }
}
