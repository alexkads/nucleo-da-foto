//! O laço: do primeiro setor ao último, atrás de começos de foto.

use std::io::{self, Read, Seek, Write};

use crate::formatos::{self, Formato, CABECA};
use crate::leitor::Leitor;

/// A FAT e a exFAT começam cada arquivo num cluster, e todo cluster começa num
/// setor de 512. Testar só aí poupa 511 em cada 512 comparações sem perder
/// nenhuma foto.
pub const SETOR: u64 = 512;
/// Quanto se lê de cada vez na varredura.
const BLOCO: usize = 4 << 20;
/// Quantos blocos ilegíveis seguidos se toleram antes de desistir. Um setor
/// ruim no cartão não pode encerrar a varredura; um cartão que saiu do leitor,
/// sim.
const FALHAS_SEGUIDAS: u32 = 16;

/// Uma foto achada: onde começa, quanto mede e de que formato é.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Achado {
    pub inicio: u64,
    pub tamanho: u64,
    pub formato: Formato,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Andamento {
    pub lidos: u64,
    pub total: Option<u64>,
    pub achadas: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Resumo {
    pub achadas: usize,
    pub lidos: u64,
    /// Blocos de 4 MiB que o cartão não deixou ler.
    pub ilegiveis: u32,
    pub interrompida: bool,
}

/// Varre o cartão inteiro.
///
/// - `parar` é perguntado a cada bloco;
/// - `andamento` recebe onde a varredura está, a cada bloco;
/// - `achou` recebe cada foto, com o leitor, para copiar dali. Um erro dele
///   (disco de destino cheio, por exemplo) encerra a varredura: continuar
///   varrendo sem ter onde gravar seria só perder tempo.
///
/// 🔑 **Depois de uma foto, a varredura pula para o fim dela.** A prévia JPEG
/// dentro de um RAW não vira uma segunda foto, e os setores do sensor não são
/// testados à toa.
pub fn varrer<R: Read + Seek>(
    leitor: &mut Leitor<R>,
    parar: &dyn Fn() -> bool,
    andamento: &mut dyn FnMut(Andamento),
    achou: &mut dyn FnMut(&mut Leitor<R>, &Achado) -> io::Result<()>,
) -> io::Result<Resumo> {
    let total = leitor.total();
    let mut bloco = vec![0u8; BLOCO];
    let mut pos = 0u64;
    let mut resumo = Resumo::default();
    let mut falhas_seguidas = 0;

    loop {
        if parar() {
            resumo.interrompida = true;
            break;
        }
        if total.is_some_and(|t| pos >= t) {
            break;
        }
        let n = match leitor.ler_em(pos, &mut bloco) {
            Ok(n) => {
                falhas_seguidas = 0;
                n
            }
            Err(_) => {
                resumo.ilegiveis += 1;
                falhas_seguidas += 1;
                if falhas_seguidas >= FALHAS_SEGUIDAS {
                    return Err(io::Error::other(
                        "o cartão parou de responder: confira se ele continua no leitor",
                    ));
                }
                pos += BLOCO as u64;
                continue;
            }
        };
        if n == 0 {
            break;
        }

        let mut salto = None;
        let mut setor = 0usize;
        while setor + CABECA <= n {
            if let Some(familia) = formatos::assinatura(&bloco[setor..setor + CABECA]) {
                let inicio = pos + setor as u64;
                if let Some((tamanho, formato)) = formatos::medir(leitor, inicio, familia)? {
                    let achado = Achado {
                        inicio,
                        tamanho,
                        formato,
                    };
                    achou(leitor, &achado)?;
                    resumo.achadas += 1;
                    salto = Some(alinhar(inicio + tamanho));
                    break;
                }
            }
            setor += SETOR as usize;
        }

        pos = salto.unwrap_or(pos + n as u64);
        resumo.lidos = pos;
        andamento(Andamento {
            lidos: total.map_or(pos, |t| pos.min(t)),
            total,
            achadas: resumo.achadas,
        });
    }
    resumo.lidos = total.map_or(resumo.lidos, |t| resumo.lidos.min(t));
    Ok(resumo)
}

/// O próximo começo de setor a partir de `pos`.
fn alinhar(pos: u64) -> u64 {
    pos.div_ceil(SETOR) * SETOR
}

/// Copia a foto achada para `destino`.
pub fn copiar<R: Read + Seek, W: Write>(
    leitor: &mut Leitor<R>,
    achado: &Achado,
    destino: &mut W,
) -> io::Result<()> {
    let mut buf = vec![0u8; 1 << 20];
    let mut feito = 0u64;
    while feito < achado.tamanho {
        let quanto = (achado.tamanho - feito).min(buf.len() as u64) as usize;
        if leitor
            .ler_exato(achado.inicio + feito, &mut buf[..quanto])?
            .is_none()
        {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "o cartão acabou no meio da foto",
            ));
        }
        destino.write_all(&buf[..quanto])?;
        feito += quanto as u64;
    }
    Ok(())
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::formatos::{cr3, jpeg, raf, tiff};
    use std::io::Cursor;

    const CLUSTER: usize = 32 * 1024;

    /// Um cartão formatado: o começo com cara de FAT32 recém-gravada (tabela
    /// zerada), e as fotos da câmera onde estavam, cada uma num cluster.
    fn cartao(fotos: &[Vec<u8>], lixo_entre: bool) -> (Vec<u8>, Vec<u64>) {
        let mut c = vec![0u8; 4 * CLUSTER];
        c[0..3].copy_from_slice(&[0xEB, 0x58, 0x90]);
        c[3..11].copy_from_slice(b"MSDOS5.0");
        c[510] = 0x55;
        c[511] = 0xAA;
        let mut onde = Vec::new();
        for foto in fotos {
            onde.push(c.len() as u64);
            c.extend(foto);
            let resto = c.len() % CLUSTER;
            if resto != 0 {
                let preenchimento = if lixo_entre { 0xA5 } else { 0 };
                c.extend(vec![preenchimento; CLUSTER - resto]);
            }
        }
        c.extend(vec![0u8; 2 * CLUSTER]);
        (c, onde)
    }

    fn recuperar(c: Vec<u8>) -> (Vec<(Achado, Vec<u8>)>, Resumo) {
        let total = c.len() as u64;
        let mut leitor = Leitor::novo(Cursor::new(c), Some(total));
        let mut achadas = Vec::new();
        let resumo = varrer(
            &mut leitor,
            &|| false,
            &mut |_| {},
            &mut |leitor, achado| {
                let mut bytes = Vec::new();
                copiar(leitor, achado, &mut bytes)?;
                achadas.push((*achado, bytes));
                Ok(())
            },
        )
        .unwrap();
        (achadas, resumo)
    }

    #[test]
    fn devolve_cada_foto_byte_a_byte() {
        let fotos = vec![
            jpeg::testes::jpeg_de_camera(1, 200_000),
            tiff::testes::raw_tiff("Canon", true, 300_000),
            jpeg::testes::jpeg_de_camera(2, 5_000_000),
            cr3::testes::cr3(250_000),
            raf::testes::raf(150_000),
            tiff::testes::raw_tiff("NIKON CORPORATION", false, 120_000),
        ];
        let (c, onde) = cartao(&fotos, true);
        let (achadas, resumo) = recuperar(c);

        assert_eq!(resumo.achadas, fotos.len());
        let formatos: Vec<_> = achadas.iter().map(|(a, _)| a.formato).collect();
        assert_eq!(
            formatos,
            [
                Formato::Jpeg,
                Formato::Cr2,
                Formato::Jpeg,
                Formato::Cr3,
                Formato::Raf,
                Formato::Nef
            ]
        );
        for (((achado, bytes), original), inicio) in achadas.iter().zip(&fotos).zip(&onde) {
            assert_eq!(achado.inicio, *inicio);
            assert!(bytes == original, "a foto em {inicio} não saiu igual");
        }
    }

    #[test]
    fn a_previa_dentro_do_raw_nao_vira_outra_foto() {
        // Uma prévia JPEG de verdade, alinhada a setor dentro do RAW, é o pior
        // caso: a assinatura bate, e só o salto até o fim do RAW a esconde.
        let mut raw = tiff::testes::raw_tiff("SONY", false, 200_000);
        let previa = jpeg::testes::jpeg_de_camera(5, 20_000);
        let em = 8192;
        raw[em..em + previa.len()].copy_from_slice(&previa);
        let (c, _) = cartao(&[raw], false);
        let (achadas, _) = recuperar(c);
        assert_eq!(achadas.len(), 1);
        assert_eq!(achadas[0].0.formato, Formato::Arw);
    }

    #[test]
    fn a_foto_cortada_no_fim_do_cartao_fica_de_fora() {
        let inteira = jpeg::testes::jpeg_de_camera(1, 50_000);
        let cortada = jpeg::testes::jpeg_de_camera(2, 80_000);
        let (mut c, _) = cartao(std::slice::from_ref(&inteira), false);
        c.extend(&cortada[..40_000]);
        let (achadas, _) = recuperar(c);
        assert_eq!(achadas.len(), 1);
        assert!(achadas[0].1 == inteira);
    }

    #[test]
    fn parar_interrompe() {
        let (c, _) = cartao(&[jpeg::testes::jpeg_de_camera(1, 50_000)], false);
        let total = c.len() as u64;
        let mut leitor = Leitor::novo(Cursor::new(c), Some(total));
        let resumo = varrer(&mut leitor, &|| true, &mut |_| {}, &mut |_, _| Ok(())).unwrap();
        assert!(resumo.interrompida);
        assert_eq!(resumo.achadas, 0);
    }

    #[test]
    fn o_andamento_chega_ao_fim() {
        let (c, _) = cartao(&[jpeg::testes::jpeg_de_camera(1, 50_000)], false);
        let total = c.len() as u64;
        let mut leitor = Leitor::novo(Cursor::new(c), Some(total));
        let mut ultimo = Andamento::default();
        varrer(&mut leitor, &|| false, &mut |a| ultimo = a, &mut |_, _| {
            Ok(())
        })
        .unwrap();
        assert_eq!(ultimo.lidos, total);
        assert_eq!(ultimo.achadas, 1);
    }
}
