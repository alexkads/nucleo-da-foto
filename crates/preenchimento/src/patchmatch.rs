//! O motor tradicional: a síntese por patches do `revelacao-core`, na
//! qualidade medida como melhor para o editor (`Qualidade::recomendada`).

use std::sync::atomic::AtomicBool;

use revelacao_core::preenchimento::{self as motor, Etapa, Falha, Pedido, Qualidade};

use crate::{Controle, Entrada, Erro, Metodo, Motor, Progresso, Saida};

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
