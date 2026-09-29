//! RAF: o RAW da Fujifilm.
//!
//! O cabeçalho é fixo e diz onde está cada parte: a prévia JPEG, o cabeçalho do
//! sensor e os dados do sensor, com deslocamento e tamanho de cada uma (inteiros
//! big-endian a partir do byte 84). O fim do arquivo é o fim da parte mais
//! distante.

use std::io::{self, Read, Seek};

use crate::leitor::Leitor;

use super::MAIOR_FOTO;

const ASSINATURA: &[u8; 16] = b"FUJIFILMCCD-RAW ";
/// Onde começam os três pares (deslocamento, tamanho).
const PARES: u64 = 84;

pub fn parece(cabeca: &[u8]) -> bool {
    &cabeca[..16] == ASSINATURA
}

pub fn medir<R: Read + Seek>(leitor: &mut Leitor<R>, inicio: u64) -> io::Result<Option<u64>> {
    let mut fim = 0u64;
    let mut partes = [(0u64, 0u64); 3];
    for (i, parte) in partes.iter_mut().enumerate() {
        let base = inicio + PARES + i as u64 * 8;
        let (Some(onde), Some(tamanho)) = (leitor.u32_be(base)?, leitor.u32_be(base + 4)?) else {
            return Ok(None);
        };
        *parte = (onde as u64, tamanho as u64);
    }
    for (onde, tamanho) in partes {
        if onde + tamanho > MAIOR_FOTO {
            return Ok(None);
        }
        fim = fim.max(onde + tamanho);
    }
    // Sem os dados do sensor, o que sobrou é só a prévia.
    let (sensor_em, sensor) = partes[2];
    if sensor == 0 || sensor_em < PARES + 24 {
        return Ok(None);
    }
    Ok(Some(fim))
}

#[cfg(test)]
pub(crate) mod testes {
    use super::*;
    use std::io::Cursor;

    pub fn raf(sensor: usize) -> Vec<u8> {
        let mut f = ASSINATURA.to_vec();
        f.extend(b"0201FF129502X-T3\0");
        f.resize(PARES as usize, 0);
        let previa_em = 256u32;
        let previa = 2000u32;
        let cab_em = previa_em + previa;
        let cab = 100u32;
        let sensor_em = cab_em + cab;
        for v in [previa_em, previa, cab_em, cab, sensor_em, sensor as u32] {
            f.extend(v.to_be_bytes());
        }
        f.resize(previa_em as usize, 0);
        f.extend(vec![0x44u8; previa as usize]);
        f.extend(vec![0x55u8; cab as usize]);
        f.extend((0..sensor).map(|i| (i % 239) as u8));
        f
    }

    #[test]
    fn o_fim_e_o_do_sensor() {
        let arquivo = raf(30_000);
        let tamanho = arquivo.len() as u64;
        let mut cartao = arquivo;
        cartao.extend([0u8; 1000]);
        let total = cartao.len() as u64;
        let mut leitor = Leitor::novo(Cursor::new(cartao), Some(total));
        assert_eq!(medir(&mut leitor, 0).unwrap(), Some(tamanho));
    }
}
