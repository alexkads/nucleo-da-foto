//! O motor tradicional: a síntese por patches do `revelacao-core`, na
//! qualidade medida como melhor para o editor (`Qualidade::recomendada`).

use std::sync::atomic::AtomicBool;

use revelacao_core::preenchimento::{self as motor, Etapa, Falha, Pedido, Qualidade};

use crate::{Controle, Entrada, Erro, Metodo, Motor, Progresso, Saida};

/// O motor PatchMatch. Sem estado: um só serve a qualquer número de
/// preenchimentos. Sempre roda na CPU e informa progresso por etapa.
pub struct PatchMatch;

impl Motor for PatchMatch {
    fn metodo(&self) -> Metodo {
        Metodo::PatchMatch
    }

    fn preencher(&self, e: &Entrada, controle: &Controle) -> Result<Saida, Erro> {
        let avisar = |p: motor::Progresso| {
            let etapa = match p.etapa {
                Etapa::Preparando => "Preparando".to_string(),
                Etapa::Nivel { n, de } => format!("Estrutura — nível {n} de {de}"),
                Etapa::Textura => "Textura".to_string(),
            };
            (controle.progresso)(Progresso {
                fracao: Some(p.fracao),
                etapa,
            });
        };
        let cancelado: &AtomicBool = controle.cancelado;
        let pedido = Pedido {
            rgba: e.rgba,
            largura: e.largura,
            altura: e.altura,
            destino: e.destino,
            amostragem: e.amostragem,
            qualidade: Qualidade {
                harmonizar: e.adaptar_cor,
                ..Qualidade::recomendada()
            },
            semente: e.semente,
        };
        let r = motor::sintetizar(
            &pedido,
            &motor::Controle {
                cancelado: Some(cancelado),
                progresso: Some(&avisar),
            },
        )
        .map_err(|f| match f {
            Falha::SemDestino => Erro::SemDestino,
            Falha::SemFontes => Erro::SemFontes,
            Falha::Cancelado => Erro::Cancelado,
        })?;
        Ok(Saida {
            x0: r.x0,
            y0: r.y0,
            largura: r.largura,
            altura: r.altura,
            rgba: r.rgba,
            reducao: 1.0,
            executado_em: "CPU".into(),
        })
    }
}

#[cfg(test)]
mod testes {
    use std::sync::atomic::AtomicBool;
    use std::sync::Mutex;

    use super::*;

    const W: u32 = 64;
    const H: u32 = 48;

    /// Um fundo liso de um cinza esverdeado com um quadrado vermelho — o
    /// "objeto" a remover — no meio.
    fn cena() -> (Vec<u8>, Vec<bool>) {
        let dentro = |x: u32, y: u32| (26..38).contains(&x) && (18..30).contains(&y);
        let rgba = (0..H)
            .flat_map(|y| {
                (0..W).flat_map(move |x| {
                    if dentro(x, y) {
                        [230, 20, 20, 255]
                    } else {
                        [90, 120, 100, 255]
                    }
                })
            })
            .collect();
        let destino = (0..W * H).map(|i| dentro(i % W, i / W)).collect();
        (rgba, destino)
    }

    fn entrada<'a>(rgba: &'a [u8], destino: &'a [bool]) -> Entrada<'a> {
        Entrada {
            rgba,
            largura: W,
            altura: H,
            destino,
            amostragem: None,
            semente: 7,
            contexto: 0.5,
            adaptar_cor: true,
        }
    }

    fn rodar(e: &Entrada, cancelado: bool) -> (Result<Saida, Erro>, Vec<Progresso>) {
        let cancelado = AtomicBool::new(cancelado);
        let avisos = Mutex::new(Vec::new());
        let progresso = |p: Progresso| avisos.lock().unwrap().push(p);
        let r = PatchMatch.preencher(
            e,
            &Controle {
                cancelado: &cancelado,
                progresso: &progresso,
            },
        );
        (r, avisos.into_inner().unwrap())
    }

    #[test]
    fn remove_o_objeto_com_o_fundo_em_volta_e_na_caixa_do_destino() {
        let (rgba, destino) = cena();
        let (r, avisos) = rodar(&entrada(&rgba, &destino), false);
        let s = r.expect("um remendo");
        assert_eq!((s.x0, s.y0, s.largura, s.altura), (26, 18, 12, 12));
        assert_eq!(s.rgba.len(), (s.largura * s.altura * 4) as usize);
        assert_eq!(s.reducao, 1.0, "na resolução da foto");
        assert_eq!(s.executado_em, "CPU");
        // O fundo é liso: o miolo reconstruído tem a cor dele, não o vermelho.
        for px in s.rgba.as_chunks::<4>().0 {
            let [r, g, b, _] = [px[0], px[1], px[2], px[3]];
            assert!(
                r.abs_diff(90) <= 6 && g.abs_diff(120) <= 6 && b.abs_diff(100) <= 6,
                "{px:?}"
            );
        }
        // Progresso por etapa, crescente, terminando cheio.
        assert!(!avisos.is_empty());
        assert!(avisos
            .iter()
            .all(|p| p.fracao.is_some() && !p.etapa.is_empty()));
        let fracoes: Vec<f32> = avisos.iter().map(|p| p.fracao.unwrap()).collect();
        assert!(fracoes.windows(2).all(|j| j[0] <= j[1]), "{fracoes:?}");
        assert!(avisos.iter().any(|p| p.etapa == "Preparando"));
    }

    #[test]
    fn a_mesma_semente_da_o_mesmo_remendo() {
        let (mut rgba, destino) = cena();
        // Uma textura, para a síntese ter escolhas a fazer.
        for (i, px) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            if !destino[i] {
                px[0] = px[0].wrapping_add(((i * 37) % 23) as u8);
            }
        }
        let e = entrada(&rgba, &destino);
        let a = rodar(&e, false).0.unwrap();
        let b = rodar(&e, false).0.unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn sem_destino_sem_fontes_e_cancelado_viram_o_erro_do_contrato() {
        let (rgba, destino) = cena();

        let nada = vec![false; destino.len()];
        assert_eq!(
            rodar(&entrada(&rgba, &nada), false).0,
            Err(Erro::SemDestino)
        );

        let sem_amostragem = vec![false; destino.len()];
        let e = Entrada {
            amostragem: Some(&sem_amostragem),
            ..entrada(&rgba, &destino)
        };
        assert_eq!(rodar(&e, false).0, Err(Erro::SemFontes));

        assert_eq!(
            rodar(&entrada(&rgba, &destino), true).0,
            Err(Erro::Cancelado)
        );
    }

    #[test]
    fn o_metodo_e_o_patchmatch_e_as_capacidades_sao_as_dele() {
        assert_eq!(PatchMatch.metodo(), Metodo::PatchMatch);
        assert_eq!(PatchMatch.capacidades(), Metodo::PatchMatch.capacidades());
    }
}
