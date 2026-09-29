//! CR3: o RAW da Canon desde 2018, em caixas ISO BMFF (o formato do MP4).
//!
//! Cada caixa começa com o próprio tamanho (4 bytes, ou 8 quando o campo curto
//! vale 1) e o tipo. O arquivo é a soma das caixas do nível de cima: `ftyp`,
//! `moov`, `uuid`… e o `mdat` com o sensor, que a Canon grava por último. A
//! soma para na primeira caixa com cara de lixo, depois de o `mdat` ter
//! aparecido.

use std::io::{self, Read, Seek};

use crate::leitor::Leitor;

use super::MAIOR_FOTO;

const CAIXAS_DE_CIMA: [&[u8; 4]; 8] = [
    b"ftyp", b"moov", b"uuid", b"mdat", b"free", b"skip", b"meta", b"wide",
];

pub fn parece(cabeca: &[u8]) -> bool {
    &cabeca[4..8] == b"ftyp" && &cabeca[8..12] == b"crx "
}

pub fn medir<R: Read + Seek>(leitor: &mut Leitor<R>, inicio: u64) -> io::Result<Option<u64>> {
    let mut pos = inicio;
    let mut viu_moov = false;
    let mut viu_mdat = false;
    loop {
        let mut cabeca = [0u8; 8];
        if leitor.ler_exato(pos, &mut cabeca)?.is_none() {
            break;
        }
        let tipo: [u8; 4] = cabeca[4..8].try_into().expect("4 bytes");
        if !CAIXAS_DE_CIMA.contains(&&tipo) {
            break;
        }
        let curto = u32::from_be_bytes(cabeca[..4].try_into().expect("4 bytes")) as u64;
        let tamanho = match curto {
            // "Até o fim do arquivo": sem tabela não há fim a que ir.
            0 => return Ok(None),
            1 => match leitor.u64_be(pos + 8)? {
                Some(t) => t,
                None => return Ok(None),
            },
            t => t,
        };
        if tamanho < 8 || pos - inicio + tamanho > MAIOR_FOTO {
            return Ok(None);
        }
        match &tipo {
            b"moov" => viu_moov = true,
            b"mdat" => viu_mdat = true,
            _ => {}
        }
        pos += tamanho;
        if viu_mdat && viu_moov {
            // Depois do sensor, só continua se vier outra caixa conhecida.
            let mut seguinte = [0u8; 8];
            if leitor.ler_exato(pos, &mut seguinte)?.is_none()
                || !CAIXAS_DE_CIMA.contains(&&<[u8; 4]>::try_from(&seguinte[4..8]).expect("4"))
                || &seguinte[4..8] == b"ftyp"
            {
                break;
            }
        }
    }
    Ok((viu_moov && viu_mdat).then_some(pos - inicio))
}

#[cfg(test)]
pub(crate) mod testes {
    use super::*;
    use std::io::Cursor;

    fn caixa(tipo: &[u8; 4], corpo: &[u8]) -> Vec<u8> {
        let mut c = ((corpo.len() + 8) as u32).to_be_bytes().to_vec();
        c.extend(tipo);
        c.extend(corpo);
        c
    }

    pub fn cr3(sensor: usize) -> Vec<u8> {
        let mut f = caixa(b"ftyp", b"crx \0\0\0\x01crx isom");
        f.extend(caixa(b"moov", &[0x11; 600]));
        f.extend(caixa(b"uuid", &[0x22; 300]));
        f.extend(caixa(
            b"mdat",
            &(0..sensor).map(|i| (i % 241) as u8).collect::<Vec<_>>(),
        ));
        f
    }

    #[test]
    fn soma_as_caixas_ate_o_sensor() {
        let arquivo = cr3(40_000);
        let tamanho = arquivo.len() as u64;
        let mut cartao = arquivo;
        cartao.extend([0u8; 4096]);
        let total = cartao.len() as u64;
        let mut leitor = Leitor::novo(Cursor::new(cartao), Some(total));
        assert_eq!(medir(&mut leitor, 0).unwrap(), Some(tamanho));
    }

    #[test]
    fn sem_o_sensor_nao_e_foto() {
        let mut f = caixa(b"ftyp", b"crx \0\0\0\x01crx isom");
        f.extend(caixa(b"moov", &[0x11; 600]));
        f.extend([0u8; 4096]);
        let total = f.len() as u64;
        let mut leitor = Leitor::novo(Cursor::new(f), Some(total));
        assert_eq!(medir(&mut leitor, 0).unwrap(), None);
    }
}
