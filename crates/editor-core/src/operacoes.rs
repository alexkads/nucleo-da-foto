//! O que se faz de uma vez numa camada: apagar e preencher a seleção, mesclar
//! uma camada na de baixo e a miniatura do painel.
//!
//! 🔑 **Todas devolvem uma [`Mudanca`]** — o mesmo passo do desfazer que o
//! traço do pincel produz, com o tile de antes e o de depois. O histórico, a
//! gravação e a coleta já sabem lidar com ele.

use std::sync::Arc;

use crate::documento::{Camada, Mascara, MascaraNoTile};
use crate::mesclagem::mesclar_em_camada;
use crate::pincel::Mudanca;
use crate::retangulo::Retangulo;
use crate::selecao::Selecao;
use crate::tiles::{
    indice, retangulo_do_tile, CamadaDePixels, Posicao, Tile, BYTES_DO_TILE, LADO_DO_TILE,
};

/// Refaz cada tile de `posicoes` com `fazer(posicao, tile_de_antes)` e devolve
/// o que mudou. `fazer` devolve o tile novo (ou `None` para "não muda").
fn refazer_tiles(
    camada: &mut CamadaDePixels,
    posicoes: impl IntoIterator<Item = Posicao>,
    mut fazer: impl FnMut(Posicao, Option<&Tile>) -> Option<Vec<u8>>,
) -> Option<Mudanca> {
    let mut antes = Vec::new();
    let mut depois = Vec::new();
    for posicao in posicoes {
        let velho = camada.tile(posicao).cloned();
        let Some(novo) = fazer(posicao, velho.as_ref()) else {
            continue;
        };
        let novo: Option<Tile> = novo
            .iter()
            .skip(3)
            .step_by(4)
            .any(|a| *a != 0)
            .then(|| Arc::new(novo));
        let igual = match (&velho, &novo) {
            (None, None) => true,
            (Some(a), Some(b)) => a == b,
            _ => false,
        };
        if igual {
            continue;
        }
        camada.definir(posicao, novo.clone());
        antes.push((posicao, velho));
        depois.push((posicao, novo));
    }
    (!antes.is_empty()).then_some(Mudanca { antes, depois })
}

/// A máscara do pixel `(lx, ly)` de um tile.
#[inline]
fn mascara(m: &Result<&[u8], u8>, lx: u32, ly: u32) -> u8 {
    match m {
        Ok(t) => t[(ly * LADO_DO_TILE + lx) as usize],
        Err(v) => *v,
    }
}

/// Apaga a camada onde a seleção está (Delete): o alfa cai na proporção da
/// máscara, a cor fica — como a borracha.
pub fn apagar(camada: &mut CamadaDePixels, selecao: &Selecao) -> Option<Mudanca> {
    let posicoes: Vec<Posicao> = camada.existentes().map(|(p, _)| *p).collect();
    refazer_tiles(camada, posicoes, |posicao, velho| {
        let velho = velho?;
        let m = selecao.do_tile(posicao);
        if let Err(0) = m {
            return None;
        }
        let mut novo = velho.as_ref().clone();
        for ly in 0..LADO_DO_TILE {
            for lx in 0..LADO_DO_TILE {
                let v = mascara(&m, lx, ly) as u32;
                if v == 0 {
                    continue;
                }
                let i = indice(lx, ly) + 3;
                novo[i] = ((novo[i] as u32 * (255 - v) + 127) / 255) as u8;
            }
        }
        Some(novo)
    })
}

/// "Bloquear pixels transparentes" depois de um gesto que mexeu nos pixels
/// (preencher, degradê, lata, remendo): cada pixel volta ao alfa de antes, e
/// o que era transparente volta como estava — só a cor muda. Devolve a mudança
/// que sobrou (`None` quando nada mudou de fato).
pub fn travar_alfa(camada: &mut CamadaDePixels, mudanca: Mudanca) -> Option<Mudanca> {
    let mut antes = Vec::new();
    let mut depois = Vec::new();
    for ((posicao, velho), (_, novo)) in mudanca.antes.into_iter().zip(mudanca.depois) {
        let corrigido: Option<Tile> = match (&velho, &novo) {
            (None, _) => None,
            (Some(v), None) => Some(v.clone()),
            (Some(v), Some(n)) => {
                let mut t = n.as_ref().clone();
                for (px, pv) in t
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(v.as_chunks::<4>().0)
                {
                    if pv[3] == 0 {
                        *px = *pv;
                    } else {
                        px[3] = pv[3];
                    }
                }
                Some(Arc::new(t))
            }
        };
        camada.definir(posicao, corrigido.clone());
        let igual = match (&velho, &corrigido) {
            (None, None) => true,
            (Some(a), Some(b)) => a == b,
            _ => false,
        };
        if !igual {
            antes.push((posicao, velho));
            depois.push((posicao, corrigido));
        }
    }
    (!antes.is_empty()).then_some(Mudanca { antes, depois })
}

/// Inverter (⌘I numa camada de pixels): cada cor vira o negativo dela, na
/// proporção da seleção (sem seleção, a camada inteira). O alfa fica.
pub fn inverter_cores(camada: &mut CamadaDePixels, selecao: Option<&Selecao>) -> Option<Mudanca> {
    let posicoes: Vec<Posicao> = camada.existentes().map(|(p, _)| *p).collect();
    refazer_tiles(camada, posicoes, |posicao, velho| {
        let velho = velho?;
        let m = selecao.map_or(Err(255), |s| s.do_tile(posicao));
        if let Err(0) = m {
            return None;
        }
        let mut novo = velho.as_ref().clone();
        for ly in 0..LADO_DO_TILE {
            for lx in 0..LADO_DO_TILE {
                let v = mascara(&m, lx, ly) as u32;
                let i = indice(lx, ly);
                if v == 0 || novo[i + 3] == 0 {
                    continue;
                }
                for c in &mut novo[i..i + 3] {
                    let a = *c as u32;
                    *c = ((a * (255 - v) + (255 - a) * v + 127) / 255) as u8;
                }
            }
        }
        Some(novo)
    })
}

/// "Aplicar máscara": o alfa de cada pixel da camada passa a ser o alfa vezes
/// o valor da máscara (com a densidade e a difusão) — a camada fica com a
/// mesma cara sem a máscara. Também o que está fora da foto (a máscara vale
/// o fundo lá).
pub fn aplicar_mascara(camada: &mut CamadaDePixels, mascara: &Mascara) -> Option<Mudanca> {
    let leitor = mascara.leitor();
    let posicoes: Vec<Posicao> = camada.todos().map(|(p, _)| *p).collect();
    let dentro: Vec<bool> = posicoes.iter().map(|p| camada.dentro(*p)).collect();
    let mut k = 0;
    refazer_tiles(camada, posicoes, |posicao, velho| {
        let de_dentro = dentro[k];
        k += 1;
        let velho = velho?;
        let m = if de_dentro {
            leitor.no_tile(posicao)
        } else {
            MascaraNoTile::Constante(mascara.tabela_da_densidade()[mascara.fundo as usize])
        };
        if let MascaraNoTile::Constante(255) = m {
            return None;
        }
        let mut novo = velho.as_ref().clone();
        for j in (0..BYTES_DO_TILE).step_by(4) {
            if novo[j + 3] == 0 {
                continue;
            }
            let v = m.valor(j) as u32;
            novo[j + 3] = ((novo[j + 3] as u32 * v + 127) / 255) as u8;
        }
        Some(novo)
    })
}

/// Preenche a seleção com uma cor (⌥Delete), por cima do que a camada tem —
/// sem seleção, a camada inteira.
pub fn preencher(
    camada: &mut CamadaDePixels,
    selecao: Option<&Selecao>,
    cor: [u8; 3],
) -> Option<Mudanca> {
    let (largura, altura) = (camada.largura(), camada.altura());
    let area = selecao.map_or(Retangulo::inteiro(largura, altura), Selecao::limites);
    let posicoes = camada.tiles_do_retangulo(&area);
    let pincel = crate::pincel::Pincel {
        cor,
        opacidade: 1.0,
        ..Default::default()
    };
    refazer_tiles(camada, posicoes, |posicao, velho| {
        let m = selecao.map_or(Err(255), |s| s.do_tile(posicao));
        if let Err(0) = m {
            return None;
        }
        let pedaco = retangulo_do_tile(posicao, largura, altura);
        let mut novo = velho.map_or_else(|| vec![0; BYTES_DO_TILE], |t| t.as_ref().clone());
        for ly in 0..pedaco.altura {
            for lx in 0..pedaco.largura {
                let v = mascara(&m, lx, ly);
                if v == 0 {
                    continue;
                }
                let i = indice(lx, ly);
                let antes = [novo[i], novo[i + 1], novo[i + 2], novo[i + 3]];
                novo[i..i + 4].copy_from_slice(&crate::pincel::aplicar(&pincel, antes, v));
            }
        }
        Some(novo)
    })
}

/// Mescla `cima` em `baixo` (⌘E), com o modo, a opacidade e a máscara de
/// `cima` (a máscara é aplicada: o escondido não desce). A de baixo guarda o
/// modo, a opacidade e a máscara dela, como no Photoshop.
///
/// O que a de cima tem **fora da foto** (Mover/⌘T, etapa 14) desce também:
/// mesclar não perde conteúdo que ainda pode voltar.
pub fn mesclar_na_de_baixo(baixo: &mut CamadaDePixels, cima: &Camada) -> Option<Mudanca> {
    let posicoes: Vec<Posicao> = cima.pixels.todos().map(|(p, _)| *p).collect();
    let leitor = cima.leitor_das_mascaras();
    refazer_tiles(baixo, posicoes, |posicao, velho| {
        let de_cima = cima.pixels.tile(posicao)?;
        let m = leitor.as_ref().map(|l| l.no_tile(posicao));
        if let Some(MascaraNoTile::Constante(0)) = m {
            return None;
        }
        let mut novo = velho.map_or_else(|| vec![0; BYTES_DO_TILE], |t| t.as_ref().clone());
        for k in (0..BYTES_DO_TILE).step_by(4) {
            let c = [de_cima[k], de_cima[k + 1], de_cima[k + 2], de_cima[k + 3]];
            if c[3] == 0 {
                continue;
            }
            let opacidade = match &m {
                None => cima.opacidade,
                Some(m) => cima.opacidade * m.valor(k) as f32 / 255.0,
            };
            if opacidade <= 0.0 {
                continue;
            }
            let b = [novo[k], novo[k + 1], novo[k + 2], novo[k + 3]];
            novo[k..k + 4].copy_from_slice(&mesclar_em_camada(b, c, opacidade, cima.modo));
        }
        Some(novo)
    })
}

/// ⌘E de uma camada **recortada** na base do conjunto: a recortada se mescla
/// na cor da base com a transparência dela travada — a cor muda, o alfa da
/// base fica. É a mesma conta da composição (`composicao.rs`, máscara de
/// corte), com a opacidade e a máscara da recortada; a base guarda o modo, a
/// opacidade e a máscara dela. Por isso a foto é a mesma, byte a byte, antes e
/// depois de mesclar. Vale para recortada de pixels e de ajuste.
pub fn mesclar_recortada_na_base(base: &mut CamadaDePixels, cima: &Camada) -> Option<Mudanca> {
    let preparado = cima.ajuste.as_ref().map(|a| a.preparar());
    let posicoes: Vec<Posicao> = base.todos().map(|(p, _)| *p).collect();
    let leitor = cima.leitor_das_mascaras();
    refazer_tiles(base, posicoes, |posicao, velho| {
        let velho = velho?;
        let de_cima = match preparado {
            Some(_) => None,
            None => Some(cima.pixels.tile(posicao)?),
        };
        let m = leitor.as_ref().map(|l| l.no_tile(posicao));
        if let Some(MascaraNoTile::Constante(0)) = m {
            return None;
        }
        let mut novo = velho.as_ref().clone();
        for k in (0..BYTES_DO_TILE).step_by(4) {
            if novo[k + 3] == 0 {
                continue;
            }
            let opacidade = match &m {
                None => cima.opacidade,
                Some(m) => cima.opacidade * m.valor(k) as f32 / 255.0,
            };
            if opacidade <= 0.0 {
                continue;
            }
            let g = [novo[k], novo[k + 1], novo[k + 2]];
            let c = match (de_cima, &preparado) {
                (Some(t), _) => [t[k], t[k + 1], t[k + 2], t[k + 3]],
                (None, Some(p)) => {
                    let [r, gg, b] = p.aplicar(g);
                    [r, gg, b, 255]
                }
                (None, None) => continue,
            };
            let cor = crate::mesclagem::mesclar(g, c, opacidade, cima.modo);
            novo[k..k + 3].copy_from_slice(&cor);
        }
        Some(novo)
    })
}

/// ⌘E de uma camada de ajuste: o ajuste entra nos pixels da de baixo, com a
/// opacidade, o modo e a máscara dele — onde a de baixo é transparente, não há
/// o que ajustar.
pub fn ajustar_a_de_baixo(baixo: &mut CamadaDePixels, cima: &Camada) -> Option<Mudanca> {
    let preparado = cima.ajuste.as_ref()?.preparar();
    let posicoes: Vec<Posicao> = baixo.existentes().map(|(p, _)| *p).collect();
    let leitor = cima.leitor_das_mascaras();
    refazer_tiles(baixo, posicoes, |posicao, velho| {
        let velho = velho?;
        let m = leitor.as_ref().map(|l| l.no_tile(posicao));
        if let Some(MascaraNoTile::Constante(0)) = m {
            return None;
        }
        let mut novo = velho.as_ref().clone();
        for k in (0..BYTES_DO_TILE).step_by(4) {
            if novo[k + 3] == 0 {
                continue;
            }
            let opacidade = match &m {
                None => cima.opacidade,
                Some(m) => cima.opacidade * m.valor(k) as f32 / 255.0,
            };
            if opacidade <= 0.0 {
                continue;
            }
            let [r, g, b] = preparado.aplicar([novo[k], novo[k + 1], novo[k + 2]]);
            let cor = crate::mesclagem::mesclar(
                [novo[k], novo[k + 1], novo[k + 2]],
                [r, g, b, 255],
                opacidade,
                cima.modo,
            );
            novo[k..k + 3].copy_from_slice(&cor);
        }
        Some(novo)
    })
}

/// O degradê (G), de `de` até `ate` em pixels da foto, dentro da seleção (ou
/// na camada inteira).
///
/// - `ate_a_cor = None` — **da cor para o transparente**, por cima do que a
///   camada tem: escurecer um céu numa camada vazia.
/// - `ate_a_cor = Some(c)` — **opaco, da cor até `c`**, no lugar do que havia:
///   o degradê da máscara (preto → branco) refeito do zero, como o do
///   Photoshop com as cores de frente e de fundo.
pub fn degrade(
    camada: &mut CamadaDePixels,
    selecao: Option<&Selecao>,
    cor: [u8; 3],
    ate_a_cor: Option<[u8; 3]>,
    de: (f32, f32),
    ate: (f32, f32),
) -> Option<Mudanca> {
    let (largura, altura) = (camada.largura(), camada.altura());
    let (vx, vy) = (ate.0 - de.0, ate.1 - de.1);
    let comprimento2 = vx * vx + vy * vy;
    if comprimento2 < 1.0 {
        return None;
    }
    // A posição ao longo do degradê: 0 no começo, 1 no fim.
    let t = |x: f32, y: f32| ((x - de.0) * vx + (y - de.1) * vy) / comprimento2;
    let area = selecao.map_or(Retangulo::inteiro(largura, altura), Selecao::limites);
    let posicoes: Vec<Posicao> = camada
        .tiles_do_retangulo(&area)
        .into_iter()
        .filter(|p| {
            // Da cor para o transparente, o tile todo depois do fim não muda.
            let r = retangulo_do_tile(*p, largura, altura);
            ate_a_cor.is_some()
                || [
                    (r.x, r.y),
                    (r.direita(), r.y),
                    (r.x, r.baixo()),
                    (r.direita(), r.baixo()),
                ]
                .iter()
                .any(|&(x, y)| t(x as f32, y as f32) < 1.0)
        })
        .collect();
    let pincel = crate::pincel::Pincel {
        cor,
        opacidade: 1.0,
        ..Default::default()
    };
    refazer_tiles(camada, posicoes, |posicao, velho| {
        let m = selecao.map_or(Err(255), |s| s.do_tile(posicao));
        if let Err(0) = m {
            return None;
        }
        let pedaco = retangulo_do_tile(posicao, largura, altura);
        let mut novo = velho.map_or_else(|| vec![0; BYTES_DO_TILE], |t| t.as_ref().clone());
        for ly in 0..pedaco.altura {
            for lx in 0..pedaco.largura {
                let v = mascara(&m, lx, ly) as f32 / 255.0;
                if v <= 0.0 {
                    continue;
                }
                let k =
                    t((pedaco.x + lx) as f32 + 0.5, (pedaco.y + ly) as f32 + 0.5).clamp(0.0, 1.0);
                let i = indice(lx, ly);
                let antes = [novo[i], novo[i + 1], novo[i + 2], novo[i + 3]];
                let depois = match ate_a_cor {
                    None => {
                        let cobertura = ((1.0 - k) * v * 255.0).round() as u8;
                        if cobertura == 0 {
                            continue;
                        }
                        crate::pincel::aplicar(&pincel, antes, cobertura)
                    }
                    Some(fim) => {
                        let c = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * k).round() as u8;
                        let cheio = [c(cor[0], fim[0]), c(cor[1], fim[1]), c(cor[2], fim[2]), 255];
                        if v >= 1.0 {
                            cheio
                        } else {
                            mesclar_em_camada(antes, cheio, v, crate::mesclagem::Modo::Normal)
                        }
                    }
                };
                novo[i..i + 4].copy_from_slice(&depois);
            }
        }
        Some(novo)
    })
}

/// A lata de tinta (⇧G): pinta com `cor` a área contínua em volta de `(x, y)`
/// cuja amostra difere da do ponto no máximo `tolerancia` em cada canal (32,
/// como no Photoshop). A amostra é a da própria camada — numa camada vazia,
/// o transparente todo; na máscara, o valor dela (`fundo` = o de onde não se
/// pintou). Com seleção, não passa da borda dela.
pub fn lata_de_tinta(
    camada: &mut CamadaDePixels,
    selecao: Option<&Selecao>,
    cor: [u8; 3],
    (x, y): (u32, u32),
    tolerancia: u8,
    fundo_da_mascara: Option<u8>,
) -> Option<Mudanca> {
    let (largura, altura) = (camada.largura(), camada.altura());
    if x >= largura || y >= altura || selecao.is_some_and(|s| s.valor(x, y) == 0) {
        return None;
    }
    let amostra = |p: [u8; 4]| -> [u8; 4] {
        match fundo_da_mascara {
            Some(f) => {
                let v = crate::documento::Mascara::valor_do_pixel(f, p);
                [v, v, v, 255]
            }
            // O transparente é um só, seja qual for a cor guardada nele.
            None if p[3] == 0 => [0; 4],
            None => p,
        }
    };
    let alvo = amostra(camada.pixel(x, y));
    let tol = tolerancia as i16;
    let parecido = |p: [u8; 4]| (0..4).all(|i| (p[i] as i16 - alvo[i] as i16).abs() <= tol);
    let (l, a) = (largura as usize, altura as usize);
    // 🔑 **Onde a tinta pode entrar, montado tile a tile** — e não perguntando
    // ao mapa de tiles por cada um dos 24 milhões de pixels. Tile que não
    // existe é uma amostra só.
    let mut livre = vec![false; l * a];
    let vazio_serve = parecido(amostra([0; 4]));
    for posicao in camada.tiles_do_retangulo(&Retangulo::inteiro(largura, altura)) {
        let pedaco = retangulo_do_tile(posicao, largura, altura);
        let m = selecao.map_or(Err(255), |s| s.do_tile(posicao));
        if let Err(0) = m {
            continue;
        }
        let tile = camada.tile(posicao);
        if tile.is_none() && !vazio_serve {
            continue;
        }
        for ly in 0..pedaco.altura {
            let linha = (pedaco.y + ly) as usize * l + pedaco.x as usize;
            for lx in 0..pedaco.largura {
                if mascara(&m, lx, ly) == 0 {
                    continue;
                }
                livre[linha + lx as usize] = match tile {
                    None => true,
                    Some(t) => {
                        let i = indice(lx, ly);
                        parecido(amostra([t[i], t[i + 1], t[i + 2], t[i + 3]]))
                    }
                };
            }
        }
    }
    // Por linhas: cada trecho contínuo entra de uma vez, e as linhas de cima e
    // de baixo dele vão para a pilha.
    let mut cheio = vec![false; l * a];
    let mut pilha = vec![(x as usize, y as usize)];
    let pode = |cheio: &[bool], x: usize, y: usize| livre[y * l + x] && !cheio[y * l + x];
    let mut limites = Retangulo::default();
    while let Some((px, py)) = pilha.pop() {
        if !pode(&cheio, px, py) {
            continue;
        }
        let mut x0 = px;
        while x0 > 0 && pode(&cheio, x0 - 1, py) {
            x0 -= 1;
        }
        let mut x1 = px;
        while x1 + 1 < l && pode(&cheio, x1 + 1, py) {
            x1 += 1;
        }
        cheio[py * l + x0..=py * l + x1].fill(true);
        limites = limites.uniao(&Retangulo::novo(
            x0 as u32,
            py as u32,
            (x1 - x0 + 1) as u32,
            1,
        ));
        for ny in [py.wrapping_sub(1), py + 1] {
            if ny >= a {
                continue;
            }
            let mut xx = x0;
            while xx <= x1 {
                if pode(&cheio, xx, ny) {
                    pilha.push((xx, ny));
                    // Um por trecho: pula o resto do trecho contínuo.
                    while xx <= x1 && pode(&cheio, xx, ny) {
                        xx += 1;
                    }
                } else {
                    xx += 1;
                }
            }
        }
    }
    let posicoes = camada.tiles_do_retangulo(&limites);
    let pincel = crate::pincel::Pincel {
        cor,
        opacidade: 1.0,
        ..Default::default()
    };
    refazer_tiles(camada, posicoes, |posicao, velho| {
        let pedaco = retangulo_do_tile(posicao, largura, altura);
        let m = selecao.map_or(Err(255), |s| s.do_tile(posicao));
        let mut novo = velho.map_or_else(|| vec![0; BYTES_DO_TILE], |t| t.as_ref().clone());
        let mut mexeu = false;
        for ly in 0..pedaco.altura {
            for lx in 0..pedaco.largura {
                let (fx, fy) = ((pedaco.x + lx) as usize, (pedaco.y + ly) as usize);
                if !cheio[fy * l + fx] {
                    continue;
                }
                let v = mascara(&m, lx, ly);
                let i = indice(lx, ly);
                let antes = [novo[i], novo[i + 1], novo[i + 2], novo[i + 3]];
                novo[i..i + 4].copy_from_slice(&crate::pincel::aplicar(&pincel, antes, v));
                mexeu = true;
            }
        }
        mexeu.then_some(novo)
    })
}

/// A camada deslocada `(dx, dy)` pixels — o que a ferramenta Mover (V) mostra
/// durante o arrasto. O que sai da foto se perde, como numa camada do tamanho
/// da tela.
///
/// 🔑 **Linha por linha, e não pixel por pixel**: cada linha de um tile vai,
/// inteira, para no máximo dois tiles de destino. Uma camada cheia de 24 MP
/// são ~96 MB de cópia — dá para refazer a cada movimento do ponteiro.
pub fn deslocada(camada: &CamadaDePixels, dx: i64, dy: i64) -> CamadaDePixels {
    // 🔑 Nada se perde fora da foto (etapa 14): o que sai vira tile de fora, e
    // volta inteiro quando anda de novo para dentro.
    let mut saida = CamadaDePixels::nova(camada.largura(), camada.altura());
    let lado = LADO_DO_TILE as i64;
    for (posicao, tile) in camada.todos() {
        let (tx, ty) = crate::tiles::origem_do_tile(*posicao);
        for ly in 0..lado {
            let y = ty + ly + dy;
            let (x0, x1) = (tx + dx, tx + lado + dx);
            let mut x = x0;
            while x < x1 {
                let destino = (x.div_euclid(lado) as i32, y.div_euclid(lado) as i32);
                let fim = ((x.div_euclid(lado) + 1) * lado).min(x1);
                let n = (fim - x) as usize * 4;
                let de = indice((x - dx - tx) as u32, ly as u32);
                let para = indice(x.rem_euclid(lado) as u32, y.rem_euclid(lado) as u32);
                saida.tile_mut(destino)[para..para + n].copy_from_slice(&tile[de..de + n]);
                x = fim;
            }
        }
    }
    let posicoes: Vec<Posicao> = saida.todos().map(|(p, _)| *p).collect();
    for p in posicoes {
        saida.enxugar(p);
    }
    saida
}

/// O passo do desfazer entre duas versões de uma camada: os tiles que mudaram.
pub fn diferenca(antes: &CamadaDePixels, depois: &CamadaDePixels) -> Option<Mudanca> {
    // Com os tiles de fora da foto: o desfazer do Mover devolve o que saiu.
    let posicoes: std::collections::BTreeSet<Posicao> = antes
        .todos()
        .chain(depois.todos())
        .map(|(p, _)| *p)
        .collect();
    let mut m = Mudanca {
        antes: Vec::new(),
        depois: Vec::new(),
    };
    for p in posicoes {
        let (a, d) = (antes.tile(p).cloned(), depois.tile(p).cloned());
        let igual = match (&a, &d) {
            (None, None) => true,
            (Some(x), Some(y)) => Arc::ptr_eq(x, y) || x == y,
            _ => false,
        };
        if !igual {
            m.antes.push((p, a));
            m.depois.push((p, d));
        }
    }
    (!m.antes.is_empty()).then_some(m)
}

/// Cola um remendo RGB (o preenchimento por conteúdo) por cima da camada, no
/// retângulo `ret`, com a força de cada pixel dada por `peso` (0 = não toca,
/// 255 = substitui).
pub fn colar(
    camada: &mut CamadaDePixels,
    ret: &Retangulo,
    rgba: &[u8],
    peso: &dyn Fn(u32, u32) -> u8,
) -> Option<Mudanca> {
    let (largura, altura) = (camada.largura(), camada.altura());
    let posicoes = camada.tiles_do_retangulo(ret);
    refazer_tiles(camada, posicoes, |posicao, velho| {
        let pedaco = retangulo_do_tile(posicao, largura, altura);
        let mut novo = velho.map_or_else(|| vec![0; BYTES_DO_TILE], |t| t.as_ref().clone());
        let mut mexeu = false;
        for y in pedaco.y.max(ret.y)..pedaco.baixo().min(ret.baixo()) {
            for x in pedaco.x.max(ret.x)..pedaco.direita().min(ret.direita()) {
                let p = peso(x, y);
                if p == 0 {
                    continue;
                }
                let k = (((y - ret.y) * ret.largura + (x - ret.x)) * 4) as usize;
                let i = indice(x - pedaco.x, y - pedaco.y);
                let antes = [novo[i], novo[i + 1], novo[i + 2], novo[i + 3]];
                let cima = [rgba[k], rgba[k + 1], rgba[k + 2], p];
                novo[i..i + 4].copy_from_slice(&mesclar_em_camada(
                    antes,
                    cima,
                    1.0,
                    crate::mesclagem::Modo::Normal,
                ));
                mexeu = true;
            }
        }
        mexeu.then_some(novo)
    })
}

/// A miniatura da camada para o painel, em RGBA de `largura × altura`, pelo
/// pixel mais próximo (são umas mil leituras). O transparente fica
/// transparente: quem desenha põe o xadrez por baixo.
pub fn miniatura(camada: &CamadaDePixels, largura: u32, altura: u32) -> Vec<u8> {
    let (lf, af) = (camada.largura().max(1), camada.altura().max(1));
    let mut saida = Vec::with_capacity((largura * altura * 4) as usize);
    for y in 0..altura {
        for x in 0..largura {
            let fx = ((x as u64 * 2 + 1) * lf as u64 / (largura as u64 * 2)) as u32;
            let fy = ((y as u64 * 2 + 1) * af as u64 / (altura as u64 * 2)) as u32;
            saida.extend_from_slice(&camada.pixel(fx.min(lf - 1), fy.min(af - 1)));
        }
    }
    saida
}

/// A miniatura da máscara: o valor dela em cinza, opaco.
pub fn miniatura_da_mascara(mascara: &Mascara, largura: u32, altura: u32) -> Vec<u8> {
    let mut saida = miniatura(&mascara.pixels, largura, altura);
    for p in saida.as_chunks_mut::<4>().0 {
        let v = Mascara::valor_do_pixel(mascara.fundo, *p);
        *p = [v, v, v, 255];
    }
    saida
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::mesclagem::Modo;
    use crate::selecao::Forma;

    fn cheia(largura: u32, altura: u32, cor: [u8; 4]) -> CamadaDePixels {
        let mut c = CamadaDePixels::nova(largura, altura);
        for p in c.tiles_do_retangulo(&Retangulo::inteiro(largura, altura)) {
            let t = c.tile_mut(p);
            for k in (0..t.len()).step_by(4) {
                t[k..k + 4].copy_from_slice(&cor);
            }
        }
        c
    }

    #[test]
    fn apagar_tira_so_o_que_esta_selecionado() {
        let mut c = cheia(600, 400, [10, 20, 30, 255]);
        let s = Selecao::da_forma(
            600,
            400,
            &Forma::Retangulo(Retangulo::novo(100, 100, 50, 50)),
        );
        let m = apagar(&mut c, &s).unwrap();
        assert_eq!(c.pixel(120, 120)[3], 0);
        assert_eq!(c.pixel(99, 120), [10, 20, 30, 255]);
        assert_eq!(m.antes.len(), 1, "um tile mexido");
        // Apagar de novo não muda nada.
        assert!(apagar(&mut c, &s).is_none());
    }

    #[test]
    fn preencher_pinta_a_selecao_e_sem_selecao_a_camada_toda() {
        let mut c = CamadaDePixels::nova(600, 400);
        let s = Selecao::da_forma(
            600,
            400,
            &Forma::Elipse(Retangulo::novo(300, 100, 200, 200)),
        );
        preencher(&mut c, Some(&s), [255, 0, 0]).unwrap();
        assert_eq!(c.pixel(400, 200), [255, 0, 0, 255]);
        assert_eq!(c.pixel(305, 105)[3], 0, "fora da elipse");
        preencher(&mut c, None, [0, 0, 255]).unwrap();
        assert_eq!(c.pixel(5, 5), [0, 0, 255, 255]);
        assert_eq!(c.pixel(599, 399), [0, 0, 255, 255]);
    }

    #[test]
    fn mesclar_na_de_baixo_compoe_igual_as_duas_separadas() {
        use crate::composicao::compor;
        use crate::documento::{BaseRef, Documento};
        let base = image::RgbImage::from_fn(300, 300, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 77])
        });
        let mut doc = Documento::novo(BaseRef::da_imagem(&base));
        doc.camadas[0].pixels = cheia(300, 300, [200, 100, 50, 255]);
        let mut cima = Camada::nova("Camada 1", 300, 300);
        cima.pixels = cheia(300, 300, [40, 200, 90, 180]);
        cima.modo = Modo::Multiplicacao;
        cima.opacidade = 0.6;
        doc.camadas.push(cima.clone());
        let separadas = compor(&base, &doc);

        let mut juntas = doc.clone();
        let de_cima = juntas.camadas.pop().unwrap();
        mesclar_na_de_baixo(&mut juntas.camadas[0].pixels, &de_cima).unwrap();
        let mescladas = compor(&base, &juntas);
        for (a, b) in separadas.as_raw().iter().zip(mescladas.as_raw()) {
            assert!((*a as i32 - *b as i32).abs() <= 1, "{a} × {b}");
        }
    }

    #[test]
    fn deslocar_leva_cada_pixel_e_guarda_o_que_sai() {
        let mut c = CamadaDePixels::nova(600, 400);
        c.tile_mut((0, 0))[indice(10, 20)..indice(10, 20) + 4].copy_from_slice(&[1, 2, 3, 255]);
        c.tile_mut((1, 1))[indice(250, 0)..indice(250, 0) + 4].copy_from_slice(&[4, 5, 6, 255]);
        let d = deslocada(&c, 300, -10);
        assert_eq!(d.pixel(310, 10), [1, 2, 3, 255]);
        assert_eq!(d.pixel(10, 20)[3], 0);
        // (506, 256) + (300, −10) = (806, 246): fora da foto de 600 — não
        // aparece, mas fica guardado (etapa 14).
        assert_eq!(d.existentes().count(), 1);
        assert_eq!(d.quantos(), 2);
        assert_eq!(d.pixel_em(806, 246), [4, 5, 6, 255]);
        let volta = deslocada(&d, -300, 10);
        assert_eq!(volta.pixel(10, 20), [1, 2, 3, 255]);
        assert_eq!(volta.pixel(506, 256), [4, 5, 6, 255], "voltou inteiro");
        let m = diferenca(&c, &d).unwrap();
        assert!(m.antes.len() >= 2);
        assert!(diferenca(&c, &c.clone()).is_none());
        // Um deslocamento que cruza a emenda de dois tiles.
        let e = deslocada(&c, 250, 0);
        assert_eq!(e.pixel(260, 20), [1, 2, 3, 255]);
        let f = deslocada(&c, 0, 0);
        assert_eq!(f, c);
    }

    #[test]
    fn colar_respeita_o_peso() {
        let mut c = CamadaDePixels::nova(600, 400);
        let ret = Retangulo::novo(250, 10, 20, 10);
        let rgba = vec![9u8; 20 * 10 * 4];
        let m = colar(&mut c, &ret, &rgba, &|x, _| if x < 260 { 255 } else { 0 }).unwrap();
        assert_eq!(c.pixel(255, 15), [9, 9, 9, 255]);
        assert_eq!(c.pixel(265, 15)[3], 0);
        assert_eq!(m.antes.len(), 2, "dois tiles: o remendo cruza a emenda");
    }

    #[test]
    fn a_miniatura_pega_o_pixel_mais_proximo() {
        let mut c = CamadaDePixels::nova(400, 200);
        c.tile_mut((1, 0))[..4].copy_from_slice(&[1, 2, 3, 4]);
        let m = miniatura(&c, 4, 2);
        assert_eq!(m.len(), 4 * 2 * 4);
        assert_eq!(&m[..4], &[0, 0, 0, 0]);
    }

    #[test]
    fn o_degrade_vai_da_cor_ao_transparente_por_cima_e_opaco_na_mascara() {
        let mut c = CamadaDePixels::nova(600, 100);
        let m = degrade(
            &mut c,
            None,
            [0, 0, 255],
            None,
            (100.0, 50.0),
            (500.0, 50.0),
        )
        .unwrap();
        assert_eq!(
            c.pixel(50, 10),
            [0, 0, 255, 255],
            "antes do começo, a cor cheia"
        );
        let meio = c.pixel(300, 10)[3];
        assert!(
            (120..=135).contains(&meio),
            "no meio, meio transparente ({meio})"
        );
        assert_eq!(c.pixel(550, 10)[3], 0, "depois do fim, nada");
        assert!(
            m.depois.iter().all(|(p, _)| p.0 < 2),
            "o tile todo depois do fim nem é tocado"
        );
        // Opaco (o da máscara): preto → branco, refeito por cima do que havia.
        let mut mascara = cheia(600, 100, [0, 255, 0, 255]);
        degrade(
            &mut mascara,
            None,
            [0; 3],
            Some([255; 3]),
            (0.0, 0.0),
            (600.0, 0.0),
        )
        .unwrap();
        assert_eq!(mascara.pixel(0, 5), [0, 0, 0, 255]);
        assert_eq!(mascara.pixel(599, 5), [255, 255, 255, 255]);
        assert!((120..=135).contains(&mascara.pixel(300, 5)[0]));
        // Sem comprimento, nada.
        assert!(degrade(&mut c, None, [1; 3], None, (5.0, 5.0), (5.2, 5.0)).is_none());
    }

    #[test]
    fn a_lata_pinta_so_a_area_parecida_e_continua() {
        // Um quadrado vermelho numa camada vazia, e um vermelho solto longe.
        let mut c = CamadaDePixels::nova(600, 400);
        preencher(
            &mut c,
            Some(&Selecao::da_forma(
                600,
                400,
                &Forma::Retangulo(Retangulo::novo(100, 100, 100, 100)),
            )),
            [255, 0, 0],
        );
        preencher(
            &mut c,
            Some(&Selecao::da_forma(
                600,
                400,
                &Forma::Retangulo(Retangulo::novo(400, 300, 20, 20)),
            )),
            [250, 5, 0],
        );
        // Dentro do quadrado: só ele (o solto é parecido, mas não encosta).
        lata_de_tinta(&mut c, None, [0, 255, 0], (150, 150), 32, None).unwrap();
        assert_eq!(c.pixel(150, 150), [0, 255, 0, 255]);
        assert_eq!(c.pixel(410, 310), [250, 5, 0, 255], "o solto não foi");
        assert_eq!(c.pixel(50, 50)[3], 0, "nem o transparente em volta");
        // No transparente: tudo menos os dois quadrados.
        lata_de_tinta(&mut c, None, [0, 0, 255], (10, 10), 32, None).unwrap();
        assert_eq!(c.pixel(599, 399), [0, 0, 255, 255]);
        assert_eq!(c.pixel(150, 150), [0, 255, 0, 255]);
        // A seleção é parede.
        let mut d = CamadaDePixels::nova(600, 400);
        let s = Selecao::da_forma(600, 400, &Forma::Retangulo(Retangulo::novo(0, 0, 300, 400)));
        lata_de_tinta(&mut d, Some(&s), [9, 9, 9], (10, 10), 32, None).unwrap();
        assert_eq!(d.pixel(299, 10)[3], 255);
        assert_eq!(d.pixel(300, 10)[3], 0);
        assert!(lata_de_tinta(&mut d, Some(&s), [9, 9, 9], (500, 10), 32, None).is_none());
    }

    #[test]
    fn a_mesclagem_aplica_a_mascara_de_quem_desce() {
        let mut cima = Camada::nova("c", 300, 10);
        cima.pixels = cheia(300, 10, [255, 0, 0, 255]);
        let mut m = Mascara::nova(255, 300, 10);
        m.pixels = cheia(300, 10, [0, 0, 0, 255]);
        // Revela de volta só a metade de cima do tile 1.
        preencher(
            &mut m.pixels,
            Some(&Selecao::da_forma(
                300,
                10,
                &Forma::Retangulo(Retangulo::novo(256, 0, 44, 10)),
            )),
            [255; 3],
        );
        cima.mascara = Some(m);
        let mut baixo = CamadaDePixels::nova(300, 10);
        mesclar_na_de_baixo(&mut baixo, &cima).unwrap();
        assert_eq!(baixo.pixel(10, 5)[3], 0, "o escondido não desceu");
        assert_eq!(baixo.pixel(280, 5), [255, 0, 0, 255]);
        // Desligada, a máscara não conta.
        cima.mascara.as_mut().unwrap().ativa = false;
        let mut baixo = CamadaDePixels::nova(300, 10);
        mesclar_na_de_baixo(&mut baixo, &cima).unwrap();
        assert_eq!(baixo.pixel(10, 5), [255, 0, 0, 255]);
    }

    #[test]
    fn o_valor_da_mascara_e_o_cinza_sobre_o_fundo() {
        assert_eq!(Mascara::valor_do_pixel(255, [0, 0, 0, 0]), 255);
        assert_eq!(Mascara::valor_do_pixel(0, [0, 0, 0, 0]), 0);
        assert_eq!(Mascara::valor_do_pixel(255, [0, 0, 0, 255]), 0);
        assert_eq!(Mascara::valor_do_pixel(0, [255, 255, 255, 255]), 255);
        assert_eq!(Mascara::valor_do_pixel(255, [0, 0, 0, 128]), 127);
        // Uma cor vira o cinza dela.
        assert_eq!(Mascara::valor_do_pixel(0, [255, 0, 0, 255]), 77);
    }
}
