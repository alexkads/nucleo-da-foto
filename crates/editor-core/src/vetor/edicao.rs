//! As edições do caminho, sem gesto: acrescentar, inserir e excluir âncoras,
//! converter pontos, mover âncoras e componentes, fechar, duplicar.
//!
//! Cada função muda o caminho no lugar e devolve se mudou; quem chama (a
//! [`super::caneta::Caneta`]) junta o antes e o depois num passo do
//! histórico.

use super::geometria::dividir;
use super::{
    Ancora, Caminho, Lado, Ligacao, OperacaoDoComponente, Ponto, RefAncora, Subcaminho, ALCA_NULA,
};

/// A ponta de um subcaminho aberto por onde ele cresce.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Extremo {
    /// A primeira âncora — as novas entram antes dela.
    Inicio,
    /// A última — as novas entram depois.
    Fim,
}

impl Extremo {
    /// A alça que **aponta para a próxima âncora desenhada** a partir desta
    /// ponta: crescendo pelo fim é a saída; pelo início, a entrada (o
    /// segmento novo vai da âncora nova até a primeira).
    pub fn alca_para_frente(self) -> Lado {
        match self {
            Extremo::Fim => Lado::Saida,
            Extremo::Inicio => Lado::Entrada,
        }
    }
}

/// Um subcaminho novo com a primeira âncora, de canto.
pub fn novo_subcaminho(c: &mut Caminho, ponto: Ponto, operacao: OperacaoDoComponente) -> RefAncora {
    let sub = c.gerar_id();
    let ancora = c.gerar_id();
    let mut s = Subcaminho::novo(sub, operacao);
    s.ancoras.push(Ancora::de_canto(ancora, ponto));
    c.subcaminhos.push(s);
    RefAncora { sub, ancora }
}

/// Uma âncora de canto nova na ponta `extremo` do subcaminho aberto.
pub fn acrescentar(c: &mut Caminho, sub: u64, extremo: Extremo, ponto: Ponto) -> Option<RefAncora> {
    let id = c.gerar_id();
    let s = c.subcaminho_mut(sub)?;
    if s.fechado {
        return None;
    }
    let a = Ancora::de_canto(id, ponto);
    match extremo {
        Extremo::Fim => s.ancoras.push(a),
        Extremo::Inicio => s.ancoras.insert(0, a),
    }
    Some(RefAncora { sub, ancora: id })
}

/// A âncora da ponta do subcaminho aberto.
pub fn ancora_do_extremo(c: &Caminho, sub: u64, extremo: Extremo) -> Option<RefAncora> {
    let s = c.subcaminho(sub)?;
    let a = match extremo {
        Extremo::Inicio => s.ancoras.first()?,
        Extremo::Fim => s.ancoras.last()?,
    };
    Some(RefAncora { sub, ancora: a.id })
}

/// Se `r` é uma ponta de um subcaminho aberto (com 1+ âncoras), qual.
pub fn extremo_de(c: &Caminho, r: RefAncora) -> Option<Extremo> {
    let s = c.subcaminho(r.sub)?;
    if s.fechado {
        return None;
    }
    let i = s.indice_da_ancora(r.ancora)?;
    if i + 1 == s.ancoras.len() {
        Some(Extremo::Fim)
    } else if i == 0 {
        Some(Extremo::Inicio)
    } else {
        None
    }
}

/// Puxa as alças da âncora com o gesto do arrasto: a alça `lado` vai a
/// `alvo` e a oposta, espelhada, do mesmo tamanho — e a âncora fica
/// [`Ligacao::Suave`] (a simetria é do gesto, não uma regra para depois).
pub fn puxar_alcas(c: &mut Caminho, r: RefAncora, lado: Lado, alvo: Ponto) -> bool {
    let Some(a) = c.ancora_mut(r) else {
        return false;
    };
    if alvo.distancia(a.ponto) < ALCA_NULA {
        a.entrada = None;
        a.saida = None;
        a.ligacao = Ligacao::Canto;
        return true;
    }
    *a.alca_mut(lado) = Some(alvo);
    *a.alca_mut(lado.oposto()) = Some(alvo.espelhado_em(a.ponto));
    a.ligacao = Ligacao::Suave;
    a.automatica = false;
    true
}

/// Só uma alça (o ⌥ no meio do arrasto, ou ao fechar quebrando): a oposta
/// fica como está e a âncora vira canto.
pub fn puxar_uma_alca(c: &mut Caminho, r: RefAncora, lado: Lado, alvo: Option<Ponto>) -> bool {
    let Some(a) = c.ancora_mut(r) else {
        return false;
    };
    *a.alca_mut(lado) = alvo;
    a.ligacao = Ligacao::Canto;
    a.automatica = false;
    a.normalizar();
    true
}

/// Move a alça com a ligação da âncora (ou sem, com `independente`; aí a
/// âncora vira canto, como o ⌥ + arrasto da alça no Photoshop).
pub fn mover_alca(
    c: &mut Caminho,
    r: RefAncora,
    lado: Lado,
    alvo: Ponto,
    independente: bool,
) -> bool {
    let Some(a) = c.ancora_mut(r) else {
        return false;
    };
    if independente {
        a.ligacao = Ligacao::Canto;
    }
    a.automatica = false;
    a.mover_alca(lado, alvo, independente);
    true
}

/// A âncora vira canto sem alças (o clique da Converter ponto numa âncora
/// suave, ou o ⌥ + clique da Caneta).
pub fn converter_em_canto(c: &mut Caminho, r: RefAncora) -> bool {
    let Some(a) = c.ancora_mut(r) else {
        return false;
    };
    if a.entrada.is_none() && a.saida.is_none() && a.ligacao == Ligacao::Canto {
        return false;
    }
    a.entrada = None;
    a.saida = None;
    a.ligacao = Ligacao::Canto;
    a.automatica = false;
    true
}

/// Tira uma alça só (o ⌥ + clique na última âncora durante a construção:
/// o próximo segmento sai reto e o de entrada fica como está).
pub fn tirar_alca(c: &mut Caminho, r: RefAncora, lado: Lado) -> bool {
    let Some(a) = c.ancora_mut(r) else {
        return false;
    };
    if a.alca(lado).is_none() {
        return false;
    }
    *a.alca_mut(lado) = None;
    a.ligacao = Ligacao::Canto;
    a.automatica = false;
    true
}

/// Troca a regra de edição das alças da âncora e acerta a forma a ela:
/// suave alinha a entrada à saída (cada uma com o seu comprimento),
/// simétrica também iguala os comprimentos (à média), canto só solta.
pub fn definir_ligacao(c: &mut Caminho, r: RefAncora, ligacao: Ligacao) -> bool {
    let Some(a) = c.ancora_mut(r) else {
        return false;
    };
    let antes = a.clone();
    a.ligacao = ligacao;
    a.automatica = false;
    match ligacao {
        Ligacao::Canto => {}
        Ligacao::Suave => a.aplicar_ligacao_a_partir_de(Lado::Saida),
        Ligacao::Simetrico => {
            if let (Some(e), Some(s)) = (a.entrada, a.saida) {
                let media = (e.distancia(a.ponto) + s.distancia(a.ponto)) / 2.0;
                let (dx, dy) = s.menos(a.ponto);
                let l = dx.hypot(dy).max(ALCA_NULA);
                a.saida = Some(a.ponto.mais((dx / l * media, dy / l * media)));
            }
            a.aplicar_ligacao_a_partir_de(Lado::Saida);
        }
    }
    *a != antes
}

/// Insere uma âncora no segmento `indice` do subcaminho, em `t`, dividindo
/// a cúbica por de Casteljau: **a curva não muda**. Num segmento reto, a
/// âncora nova é de canto e sem alças (a reta continua reta); numa curva,
/// nasce suave, com as alças da divisão.
pub fn inserir_ancora(c: &mut Caminho, sub: u64, indice: usize, t: f64) -> Option<RefAncora> {
    let id = c.gerar_id();
    let s = c.subcaminho_mut(sub)?;
    let seg = s.segmento(indice)?;
    let t = t.clamp(1e-4, 1.0 - 1e-4);
    let nova = if seg.reto() {
        let p = super::geometria::avaliar(&seg.p, t);
        Ancora::de_canto(id, p)
    } else {
        let (a, b) = dividir(&seg.p, t);
        s.ancoras[seg.de].saida = Some(a[1]);
        s.ancoras[seg.ate].entrada = Some(b[2]);
        s.ancoras[seg.de].normalizar();
        s.ancoras[seg.ate].normalizar();
        let mut nova = Ancora {
            id,
            ponto: a[3],
            entrada: Some(a[2]),
            saida: Some(b[1]),
            ligacao: Ligacao::Suave,
            automatica: false,
        };
        nova.normalizar();
        if nova.entrada.is_none() || nova.saida.is_none() {
            nova.ligacao = Ligacao::Canto;
        }
        nova
    };
    // O segmento de fechamento termina na primeira: a nova vai para o fim.
    let posicao = if seg.ate == 0 {
        s.ancoras.len()
    } else {
        seg.ate
    };
    s.ancoras.insert(posicao, nova);
    Some(RefAncora { sub, ancora: id })
}

/// Exclui a âncora e liga as vizinhas com um segmento só, feito **da alça
/// de saída da anterior e da alça de entrada da seguinte, como estavam**.
/// É previsível, mas não preserva a curva: duas cúbicas em geral não cabem
/// numa. A última âncora de um subcaminho leva o subcaminho junto; o fechado
/// que fica com uma âncora só passa a aberto.
pub fn excluir_ancora(c: &mut Caminho, r: RefAncora) -> bool {
    let Some(i_sub) = c.indice_do_subcaminho(r.sub) else {
        return false;
    };
    let s = &mut c.subcaminhos[i_sub];
    let Some(i) = s.indice_da_ancora(r.ancora) else {
        return false;
    };
    s.ancoras.remove(i);
    if s.ancoras.is_empty() {
        c.subcaminhos.remove(i_sub);
    } else if s.ancoras.len() < 2 {
        s.fechado = false;
    }
    true
}

/// Exclui várias âncoras (o Delete da Seleção direta). Num subcaminho
/// **fechado**, tirar âncoras abre o caminho onde elas estavam; num aberto,
/// tirar uma do meio o parte em dois — os segmentos que tocavam a âncora
/// somem com ela, como no Photoshop.
pub fn excluir_ancoras_partindo(c: &mut Caminho, refs: &[RefAncora]) -> bool {
    let mut mudou = false;
    let subs: Vec<u64> = c.subcaminhos.iter().map(|s| s.id).collect();
    for sub in subs {
        let Some(i_sub) = c.indice_do_subcaminho(sub) else {
            continue;
        };
        let s = &c.subcaminhos[i_sub];
        let tirar: Vec<bool> = s
            .ancoras
            .iter()
            .map(|a| refs.contains(&RefAncora { sub, ancora: a.id }))
            .collect();
        if !tirar.iter().any(|t| *t) {
            continue;
        }
        mudou = true;
        let s = c.subcaminhos.remove(i_sub);
        let n = s.ancoras.len();
        // Os trechos que sobram, em ordem; no fechado, a volta começa logo
        // depois de uma âncora tirada.
        let mut trechos: Vec<Vec<Ancora>> = Vec::new();
        let inicio = if s.fechado {
            (0..n).find(|&k| tirar[k]).map_or(0, |k| (k + 1) % n)
        } else {
            0
        };
        let mut atual: Vec<Ancora> = Vec::new();
        for passo in 0..n {
            let k = (inicio + passo) % n;
            if tirar[k] {
                if !atual.is_empty() {
                    trechos.push(std::mem::take(&mut atual));
                }
            } else {
                atual.push(s.ancoras[k].clone());
            }
        }
        if !atual.is_empty() {
            trechos.push(atual);
        }
        let mut novos = Vec::new();
        for (j, mut ancoras) in trechos.into_iter().enumerate() {
            // As pontas soltas perdem a alça que ia para o segmento que sumiu.
            if let Some(a) = ancoras.first_mut() {
                a.entrada = None;
            }
            if let Some(a) = ancoras.last_mut() {
                a.saida = None;
            }
            for a in ancoras
                .iter_mut()
                .filter(|a| a.entrada.is_none() || a.saida.is_none())
            {
                if a.ligacao != Ligacao::Canto {
                    a.ligacao = Ligacao::Canto;
                }
            }
            let id = if j == 0 { s.id } else { 0 };
            novos.push(Subcaminho {
                id,
                ancoras,
                fechado: false,
                operacao: s.operacao,
            });
        }
        for sub in novos.iter_mut().filter(|n| n.id == 0) {
            sub.id = c.gerar_id();
        }
        for (k, n) in novos.into_iter().enumerate() {
            c.subcaminhos.insert(i_sub + k, n);
        }
    }
    mudou
}

/// Fecha o subcaminho (o clique na primeira âncora).
pub fn fechar(c: &mut Caminho, sub: u64) -> bool {
    match c.subcaminho_mut(sub) {
        Some(s) if !s.fechado && s.ancoras.len() >= 2 => {
            s.fechado = true;
            true
        }
        _ => false,
    }
}

/// Liga a ponta `de` de um subcaminho aberto à ponta `ate` de **outro**
/// subcaminho aberto do mesmo caminho (clicar na ponta de outro componente
/// enquanto se desenha): os dois viram um só. Devolve o subcaminho que
/// ficou e a ponta dele por onde se continua — nenhuma, porque o clique
/// termina ali (o desenho segue da ponta livre do outro, se quiser).
pub fn ligar_subcaminhos(c: &mut Caminho, de: RefAncora, ate: RefAncora) -> Option<u64> {
    if de.sub == ate.sub {
        return None;
    }
    let (ed, ea) = (extremo_de(c, de)?, extremo_de(c, ate)?);
    let i_ate = c.indice_do_subcaminho(ate.sub)?;
    let mut outro = c.subcaminhos.remove(i_ate);
    // O outro, orientado para começar pela ponta clicada.
    if ea == Extremo::Fim {
        inverter(&mut outro);
    }
    let s = c.subcaminho_mut(de.sub)?;
    match ed {
        Extremo::Fim => s.ancoras.extend(outro.ancoras),
        Extremo::Inicio => {
            inverter(s);
            s.ancoras.extend(outro.ancoras);
        }
    }
    Some(de.sub)
}

/// O subcaminho no sentido contrário: a ordem das âncoras e o papel das
/// alças (entrada ↔ saída) se invertem; a curva é a mesma.
pub fn inverter(s: &mut Subcaminho) {
    s.ancoras.reverse();
    for a in &mut s.ancoras {
        std::mem::swap(&mut a.entrada, &mut a.saida);
    }
}

/// Dobra o segmento `indice` arrastando o ponto da curva em `t` por `d`: as
/// duas alças do segmento mudam e as pontas ficam (a conta do "arrastar a
/// curva" do Inkscape — o peso de cada alça segue o `t`, então agarrar perto
/// de uma ponta mexe sobretudo na alça dela). Depois, as âncoras suaves e
/// simétricas acertam a alça do outro lado, para a emenda continuar lisa.
pub fn dobrar_segmento(c: &mut Caminho, sub: u64, indice: usize, t: f64, d: (f64, f64)) -> bool {
    let Some(s) = c.subcaminho_mut(sub) else {
        return false;
    };
    let Some(seg) = s.segmento(indice) else {
        return false;
    };
    let t = t.clamp(0.02, 0.98);
    let peso = if t <= 1.0 / 6.0 {
        0.0
    } else if t <= 0.5 {
        ((6.0 * t - 1.0) / 2.0).powi(3) / 2.0
    } else if t <= 5.0 / 6.0 {
        1.0 - ((6.0 * (1.0 - t) - 1.0) / 2.0).powi(3) / 2.0
    } else {
        1.0
    };
    let u = 1.0 - t;
    let a0 = (1.0 - peso) / (3.0 * t * u * u);
    let a1 = peso / (3.0 * t * t * u);
    let p1 = seg.p[1].mais((d.0 * a0, d.1 * a0));
    let p2 = seg.p[2].mais((d.0 * a1, d.1 * a1));
    s.ancoras[seg.de].saida = Some(p1);
    s.ancoras[seg.ate].entrada = Some(p2);
    s.ancoras[seg.de].automatica = false;
    s.ancoras[seg.ate].automatica = false;
    s.ancoras[seg.de].aplicar_ligacao_a_partir_de(Lado::Saida);
    s.ancoras[seg.ate].aplicar_ligacao_a_partir_de(Lado::Entrada);
    s.ancoras[seg.de].normalizar();
    s.ancoras[seg.ate].normalizar();
    true
}

// ------------------------------------------------- a Caneta de curvatura

/// Quanto da distância ao vizinho vira alça na âncora automática — um
/// terço, a proporção da Catmull-Rom (`(próximo − anterior) / 6` nas âncoras
/// igualmente espaçadas).
const PUXO_DA_CURVATURA: f64 = 1.0 / 3.0;

/// Refaz as alças das âncoras automáticas (a Caneta de curvatura): numa
/// suave, a tangente é a direção do vizinho de trás ao da frente e cada alça
/// tem um terço da distância ao vizinho do lado dela — a curva passa lisa
/// pelos pontos; numa de canto, sem alças. A ponta de um aberto (um vizinho
/// só) fica sem alça. As âncoras feitas à mão não mudam. Devolve se mudou.
pub fn recalcular_automaticas(c: &mut Caminho) -> bool {
    let mut mudou = false;
    for s in &mut c.subcaminhos {
        let n = s.ancoras.len();
        if !s.ancoras.iter().any(|a| a.automatica) {
            continue;
        }
        let pontos: Vec<Ponto> = s.ancoras.iter().map(|a| a.ponto).collect();
        for i in 0..n {
            if !s.ancoras[i].automatica {
                continue;
            }
            let anterior = (i > 0 || s.fechado).then(|| pontos[(i + n - 1) % n]);
            let proximo = (i + 1 < n || s.fechado).then(|| pontos[(i + 1) % n]);
            let a = &mut s.ancoras[i];
            let (entrada, saida) = match (a.ligacao, anterior, proximo) {
                (Ligacao::Canto, _, _) | (_, None, _) | (_, _, None) => (None, None),
                (_, Some(ant), Some(prox)) if n >= 2 && ant != prox => {
                    let (dx, dy) = prox.menos(ant);
                    let l = dx.hypot(dy);
                    if l < ALCA_NULA {
                        (None, None)
                    } else {
                        let (ux, uy) = (dx / l, dy / l);
                        let li = a.ponto.distancia(ant) * PUXO_DA_CURVATURA;
                        let lo = a.ponto.distancia(prox) * PUXO_DA_CURVATURA;
                        (
                            Some(a.ponto.mais((-ux * li, -uy * li))),
                            Some(a.ponto.mais((ux * lo, uy * lo))),
                        )
                    }
                }
                _ => (None, None),
            };
            if a.entrada != entrada || a.saida != saida {
                a.entrada = entrada;
                a.saida = saida;
                mudou = true;
            }
        }
    }
    mudou
}

/// Marca a âncora como automática (a Caneta de curvatura): suave, ou canto
/// com `canto`. As alças saem no próximo [`recalcular_automaticas`].
pub fn tornar_automatica(c: &mut Caminho, r: RefAncora, canto: bool) -> bool {
    let Some(a) = c.ancora_mut(r) else {
        return false;
    };
    a.automatica = true;
    a.ligacao = if canto {
        Ligacao::Canto
    } else {
        Ligacao::Suave
    };
    true
}

/// As âncoras andam `d` (com as alças).
pub fn mover_ancoras(c: &mut Caminho, refs: &[RefAncora], d: (f64, f64)) -> bool {
    let mut mudou = false;
    for r in refs {
        if let Some(a) = c.ancora_mut(*r) {
            a.transladar(d);
            mudou = true;
        }
    }
    mudou
}

/// Os componentes andam `d`.
pub fn mover_subcaminhos(c: &mut Caminho, subs: &[u64], d: (f64, f64)) -> bool {
    let mut mudou = false;
    for s in c.subcaminhos.iter_mut().filter(|s| subs.contains(&s.id)) {
        for a in &mut s.ancoras {
            a.transladar(d);
        }
        mudou = true;
    }
    mudou
}

/// Duplica os componentes (logo acima de cada um), com ids novos. Devolve
/// os ids das cópias, na ordem.
pub fn duplicar_subcaminhos(c: &mut Caminho, subs: &[u64]) -> Vec<u64> {
    let mut novos = Vec::new();
    let mut i = 0;
    while i < c.subcaminhos.len() {
        if subs.contains(&c.subcaminhos[i].id) {
            let mut copia = c.subcaminhos[i].clone();
            copia.id = c.gerar_id();
            for a in &mut copia.ancoras {
                a.id = c.gerar_id();
            }
            novos.push(copia.id);
            c.subcaminhos.insert(i + 1, copia);
            i += 1;
        }
        i += 1;
    }
    novos
}

/// Exclui os componentes.
pub fn excluir_subcaminhos(c: &mut Caminho, subs: &[u64]) -> bool {
    let antes = c.subcaminhos.len();
    c.subcaminhos.retain(|s| !subs.contains(&s.id));
    c.subcaminhos.len() != antes
}

/// A operação dos componentes.
pub fn definir_operacao(c: &mut Caminho, subs: &[u64], op: OperacaoDoComponente) -> bool {
    let mut mudou = false;
    for s in c.subcaminhos.iter_mut().filter(|s| subs.contains(&s.id)) {
        if s.operacao != op {
            s.operacao = op;
            mudou = true;
        }
    }
    mudou
}

/// O caminho inteiro pela afim `p ↦ (a·x + b·y + c, d·x + e·y + f)` — a
/// imagem de uma Bézier por uma afim é a Bézier dos pontos transformados,
/// então a forma acompanha a transformação exata.
pub fn transformar(c: &mut Caminho, m: [f64; 6]) {
    let f = |p: Ponto| {
        Ponto::novo(
            m[0] * p.x + m[1] * p.y + m[2],
            m[3] * p.x + m[4] * p.y + m[5],
        )
    };
    for s in &mut c.subcaminhos {
        for a in &mut s.ancoras {
            a.ponto = f(a.ponto);
            a.entrada = a.entrada.map(f);
            a.saida = a.saida.map(f);
        }
    }
}

/// Só os componentes `subs` pela afim (o ⌘T de componentes escolhidos).
pub fn transformar_subcaminhos(c: &mut Caminho, subs: &[u64], m: [f64; 6]) {
    let f = |p: Ponto| {
        Ponto::novo(
            m[0] * p.x + m[1] * p.y + m[2],
            m[3] * p.x + m[4] * p.y + m[5],
        )
    };
    for s in c.subcaminhos.iter_mut().filter(|s| subs.contains(&s.id)) {
        for a in &mut s.ancoras {
            a.ponto = f(a.ponto);
            a.entrada = a.entrada.map(f);
            a.saida = a.saida.map(f);
        }
    }
}

// ------------------------------------------------- formas prontas (testes, comandos)

/// Um retângulo fechado, no sentido horário, num caminho novo.
pub fn retangulo_em_caminho(x: f64, y: f64, l: f64, a: f64) -> Caminho {
    poligono_em_caminho(&[(x, y), (x + l, y), (x + l, y + a), (x, y + a)])
}

/// Um polígono fechado de cantos, num caminho novo.
pub fn poligono_em_caminho(pontos: &[(f64, f64)]) -> Caminho {
    let mut c = Caminho::novo(1, super::NOME_DO_TRABALHO);
    let Some(&(x0, y0)) = pontos.first() else {
        return c;
    };
    let r = novo_subcaminho(&mut c, Ponto::novo(x0, y0), OperacaoDoComponente::Somar);
    for &(x, y) in &pontos[1..] {
        acrescentar(&mut c, r.sub, Extremo::Fim, Ponto::novo(x, y));
    }
    fechar(&mut c, r.sub);
    c
}

/// Uma elipse de quatro cúbicas (a constante de 0,5523 do círculo), fechada.
pub fn elipse_em_caminho(cx: f64, cy: f64, rx: f64, ry: f64) -> Caminho {
    const K: f64 = 0.552_284_749_830_793_4;
    let mut c = Caminho::novo(1, super::NOME_DO_TRABALHO);
    let pontos = [
        (cx + rx, cy, (0.0, ry * K)),
        (cx, cy + ry, (-rx * K, 0.0)),
        (cx - rx, cy, (0.0, -ry * K)),
        (cx, cy - ry, (rx * K, 0.0)),
    ];
    let sub = c.gerar_id();
    let mut s = Subcaminho::novo(sub, OperacaoDoComponente::Somar);
    for (x, y, (dx, dy)) in pontos {
        let id = c.gerar_id();
        let p = Ponto::novo(x, y);
        s.ancoras.push(Ancora {
            id,
            ponto: p,
            entrada: Some(p.mais((-dx, -dy))),
            saida: Some(p.mais((dx, dy))),
            ligacao: Ligacao::Simetrico,
            automatica: false,
        });
    }
    s.fechado = true;
    c.subcaminhos.push(s);
    c
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::vetor::geometria::avaliar;

    fn curva_simples() -> (Caminho, u64) {
        let mut c = Caminho::novo(1, "t");
        let r = novo_subcaminho(&mut c, Ponto::novo(0.0, 100.0), OperacaoDoComponente::Somar);
        let b = acrescentar(&mut c, r.sub, Extremo::Fim, Ponto::novo(200.0, 100.0)).unwrap();
        c.ancora_mut(r).unwrap().saida = Some(Ponto::novo(40.0, 0.0));
        c.ancora_mut(b).unwrap().entrada = Some(Ponto::novo(170.0, 10.0));
        (c, r.sub)
    }

    fn amostras(c: &Caminho, sub: u64) -> Vec<Ponto> {
        let s = c.subcaminho(sub).unwrap();
        let mut v = Vec::new();
        for seg in s.segmentos() {
            for k in 0..=50 {
                v.push(avaliar(&seg.p, k as f64 / 50.0));
            }
        }
        v
    }

    /// A distância de `q` à curva (densa).
    fn distancia_a(c: &Caminho, sub: u64, q: Ponto) -> f64 {
        let s = c.subcaminho(sub).unwrap();
        s.segmentos()
            .map(|seg| crate::vetor::geometria::mais_proximo(&seg, q).1)
            .fold(f64::MAX, f64::min)
    }

    #[test]
    fn inserir_ancora_preserva_a_curva() {
        let (mut c, sub) = curva_simples();
        let antes = c.clone();
        let nova = inserir_ancora(&mut c, sub, 0, 0.3).unwrap();
        assert_eq!(c.subcaminho(sub).unwrap().ancoras.len(), 3);
        assert_eq!(c.subcaminho(sub).unwrap().ancoras[1].id, nova.ancora);
        // Todo ponto da curva de antes está na de depois, e vice-versa.
        for q in amostras(&antes, sub) {
            assert!(distancia_a(&c, sub, q) < 1e-6, "{q:?}");
        }
        for q in amostras(&c, sub) {
            assert!(distancia_a(&antes, sub, q) < 1e-6, "{q:?}");
        }
        // A âncora nova é suave e as alças dela são colineares.
        let a = c.ancora(nova).unwrap();
        assert_eq!(a.ligacao, Ligacao::Suave);
        assert!(a.alcas_colineares());
    }

    #[test]
    fn inserir_numa_reta_deixa_reta_e_sem_alcas() {
        let mut c = retangulo_em_caminho(0.0, 0.0, 100.0, 50.0);
        let sub = c.subcaminhos[0].id;
        // O segmento de fechamento (da última à primeira).
        let nova = inserir_ancora(&mut c, sub, 3, 0.5).unwrap();
        let s = c.subcaminho(sub).unwrap();
        assert_eq!(s.ancoras.len(), 5);
        assert_eq!(s.ancoras.last().unwrap().id, nova.ancora);
        let a = c.ancora(nova).unwrap();
        assert_eq!(a.ponto, Ponto::novo(0.0, 25.0));
        assert!(a.entrada.is_none() && a.saida.is_none());
        assert!(s.segmentos().all(|seg| seg.reto()));
    }

    #[test]
    fn a_alca_suave_gira_a_oposta_sem_igualar_o_comprimento() {
        let mut c = Caminho::novo(1, "t");
        let r = novo_subcaminho(&mut c, Ponto::novo(50.0, 50.0), OperacaoDoComponente::Somar);
        puxar_alcas(&mut c, r, Lado::Saida, Ponto::novo(80.0, 50.0));
        assert_eq!(c.ancora(r).unwrap().ligacao, Ligacao::Suave);
        // Encurta a entrada (alça independente de tamanho): a saída fica onde está.
        mover_alca(&mut c, r, Lado::Entrada, Ponto::novo(40.0, 50.0), false);
        let a = c.ancora(r).unwrap();
        assert_eq!(a.saida, Some(Ponto::novo(80.0, 50.0)));
        assert!((a.entrada.unwrap().distancia(a.ponto) - 10.0).abs() < 1e-9);
        // Gira a entrada para cima: a saída gira junto, colinear, com 30.
        mover_alca(&mut c, r, Lado::Entrada, Ponto::novo(50.0, 30.0), false);
        let a = c.ancora(r).unwrap();
        assert!(a.alcas_colineares());
        let s = a.saida.unwrap();
        assert!((s.distancia(a.ponto) - 30.0).abs() < 1e-9);
        assert!((s.x - 50.0).abs() < 1e-9 && (s.y - 80.0).abs() < 1e-9);
        assert!((a.entrada.unwrap().distancia(a.ponto) - 20.0).abs() < 1e-9);
    }

    #[test]
    fn simetrico_iguala_e_canto_solta() {
        let mut c = Caminho::novo(1, "t");
        let r = novo_subcaminho(&mut c, Ponto::novo(0.0, 0.0), OperacaoDoComponente::Somar);
        {
            let a = c.ancora_mut(r).unwrap();
            a.entrada = Some(Ponto::novo(-10.0, 0.0));
            a.saida = Some(Ponto::novo(30.0, 0.0));
        }
        definir_ligacao(&mut c, r, Ligacao::Simetrico);
        let a = c.ancora(r).unwrap().clone();
        assert_eq!(a.saida, Some(Ponto::novo(20.0, 0.0)));
        assert_eq!(a.entrada, Some(Ponto::novo(-20.0, 0.0)));
        mover_alca(&mut c, r, Lado::Saida, Ponto::novo(0.0, 5.0), false);
        assert_eq!(c.ancora(r).unwrap().entrada, Some(Ponto::novo(0.0, -5.0)));
        // ⌥: só esta alça, e a âncora vira canto.
        mover_alca(&mut c, r, Lado::Saida, Ponto::novo(7.0, 7.0), true);
        let a = c.ancora(r).unwrap();
        assert_eq!(a.ligacao, Ligacao::Canto);
        assert_eq!(a.entrada, Some(Ponto::novo(0.0, -5.0)));
    }

    #[test]
    fn converter_entre_canto_e_suave() {
        let mut c = retangulo_em_caminho(0.0, 0.0, 100.0, 100.0);
        let s = &c.subcaminhos[0];
        let r = RefAncora {
            sub: s.id,
            ancora: s.ancoras[1].id,
        };
        // Canto → suave pelo arrasto.
        puxar_alcas(&mut c, r, Lado::Saida, Ponto::novo(100.0, 30.0));
        let a = c.ancora(r).unwrap();
        assert_eq!(a.ligacao, Ligacao::Suave);
        assert_eq!(a.entrada, Some(Ponto::novo(100.0, -30.0)));
        // Suave → canto pelo clique.
        assert!(converter_em_canto(&mut c, r));
        let a = c.ancora(r).unwrap();
        assert_eq!(
            (a.entrada, a.saida, a.ligacao),
            (None, None, Ligacao::Canto)
        );
        assert!(!converter_em_canto(&mut c, r), "já era canto");
    }

    #[test]
    fn excluir_ancora_liga_as_vizinhas_com_as_alcas_delas() {
        let (mut c, sub) = curva_simples();
        let nova = inserir_ancora(&mut c, sub, 0, 0.5).unwrap();
        assert!(excluir_ancora(&mut c, nova));
        let s = c.subcaminho(sub).unwrap();
        assert_eq!(s.ancoras.len(), 2);
        // As alças das pontas ficaram as da divisão (mais curtas): a curva
        // muda, de forma previsível.
        assert!(s.ancoras[0].saida.is_some());
    }

    #[test]
    fn delete_na_selecao_direta_abre_o_fechado_e_parte_o_aberto() {
        let mut c = retangulo_em_caminho(0.0, 0.0, 100.0, 100.0);
        let s = c.subcaminhos[0].clone();
        let r = RefAncora {
            sub: s.id,
            ancora: s.ancoras[2].id,
        };
        assert!(excluir_ancoras_partindo(&mut c, &[r]));
        assert_eq!(c.subcaminhos.len(), 1);
        let aberto = &c.subcaminhos[0];
        assert!(!aberto.fechado);
        assert_eq!(aberto.ancoras.len(), 3);
        assert_eq!(
            aberto.ancoras[0].id, s.ancoras[3].id,
            "começa depois do furo"
        );
        // Num aberto, uma do meio parte em dois.
        let meio = RefAncora {
            sub: aberto.id,
            ancora: aberto.ancoras[1].id,
        };
        assert!(excluir_ancoras_partindo(&mut c, &[meio]));
        assert_eq!(c.subcaminhos.len(), 2);
        assert!(c.subcaminhos.iter().all(|s| s.ancoras.len() == 1));
    }

    #[test]
    fn retomar_pelo_inicio_inverte_o_papel_das_alcas() {
        assert_eq!(Extremo::Inicio.alca_para_frente(), Lado::Entrada);
        let (mut c, sub) = curva_simples();
        let primeira = ancora_do_extremo(&c, sub, Extremo::Inicio).unwrap();
        assert_eq!(extremo_de(&c, primeira), Some(Extremo::Inicio));
        let nova = acrescentar(&mut c, sub, Extremo::Inicio, Ponto::novo(-100.0, 50.0)).unwrap();
        let s = c.subcaminho(sub).unwrap();
        assert_eq!(s.ancoras[0].id, nova.ancora);
        // O segmento novo vai da nova até a antiga primeira e usa a entrada
        // dela — a alça que aponta para onde se desenha.
        puxar_alcas(&mut c, primeira, Lado::Entrada, Ponto::novo(-20.0, 120.0));
        let seg = c.subcaminho(sub).unwrap().segmento(0).unwrap();
        assert_eq!(seg.p[2], Ponto::novo(-20.0, 120.0));
        assert_eq!(seg.p[3], Ponto::novo(0.0, 100.0));
    }

    #[test]
    fn fechar_ligar_e_inverter() {
        let mut c = poligono_em_caminho(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]);
        c.subcaminhos[0].fechado = false;
        let sub = c.subcaminhos[0].id;
        assert!(fechar(&mut c, sub));
        assert!(!fechar(&mut c, sub));
        let mut s = c.subcaminhos[0].clone();
        let antes = s.clone();
        inverter(&mut s);
        inverter(&mut s);
        assert_eq!(s, antes);
        // Dois abertos viram um.
        let mut d = Caminho::novo(1, "t");
        let a = novo_subcaminho(&mut d, Ponto::novo(0.0, 0.0), OperacaoDoComponente::Somar);
        acrescentar(&mut d, a.sub, Extremo::Fim, Ponto::novo(10.0, 0.0));
        let b = novo_subcaminho(&mut d, Ponto::novo(30.0, 0.0), OperacaoDoComponente::Somar);
        let b_fim = acrescentar(&mut d, b.sub, Extremo::Fim, Ponto::novo(20.0, 0.0)).unwrap();
        let a_fim = ancora_do_extremo(&d, a.sub, Extremo::Fim).unwrap();
        assert_eq!(ligar_subcaminhos(&mut d, a_fim, b_fim), Some(a.sub));
        assert_eq!(d.subcaminhos.len(), 1);
        let xs: Vec<f64> = d.subcaminhos[0].ancoras.iter().map(|a| a.ponto.x).collect();
        assert_eq!(xs, vec![0.0, 10.0, 20.0, 30.0]);
    }

    #[test]
    fn duplicar_mover_e_excluir_componentes() {
        let mut c = retangulo_em_caminho(0.0, 0.0, 10.0, 10.0);
        let sub = c.subcaminhos[0].id;
        let novos = duplicar_subcaminhos(&mut c, &[sub]);
        assert_eq!(novos.len(), 1);
        let ids: std::collections::BTreeSet<u64> = c
            .subcaminhos
            .iter()
            .flat_map(|s| s.ancoras.iter().map(|a| a.id).chain([s.id]))
            .collect();
        assert_eq!(ids.len(), 10, "ids todos diferentes");
        mover_subcaminhos(&mut c, &novos, (5.0, 5.0));
        assert_eq!(c.subcaminhos[1].ancoras[0].ponto, Ponto::novo(5.0, 5.0));
        assert!(excluir_subcaminhos(&mut c, &[sub]));
        assert_eq!(c.subcaminhos.len(), 1);
    }

    #[test]
    fn dobrar_o_segmento_leva_o_ponto_agarrado_e_deixa_as_pontas() {
        let (mut c, sub) = curva_simples();
        let antes = c.clone();
        let seg = antes.subcaminho(sub).unwrap().segmento(0).unwrap();
        for t in [0.3, 0.5, 0.7] {
            let mut c2 = antes.clone();
            let q = avaliar(&seg.p, t);
            assert!(dobrar_segmento(&mut c2, sub, 0, t, (5.0, 12.0)));
            let novo = c2.subcaminho(sub).unwrap().segmento(0).unwrap();
            let q2 = avaliar(&novo.p, t);
            assert!(q2.distancia(q.mais((5.0, 12.0))) < 1e-9, "t={t}: {q2:?}");
            assert_eq!(novo.p[0], seg.p[0]);
            assert_eq!(novo.p[3], seg.p[3]);
        }
        // Âncora suave na ponta: a alça do outro lado gira junto.
        let r = RefAncora {
            sub,
            ancora: c.subcaminho(sub).unwrap().ancoras[1].id,
        };
        acrescentar(&mut c, sub, Extremo::Fim, Ponto::novo(300.0, 100.0));
        {
            let a = c.ancora_mut(r).unwrap();
            a.saida = Some(Ponto::novo(230.0, 190.0));
            a.ligacao = Ligacao::Suave;
        }
        dobrar_segmento(&mut c, sub, 0, 0.8, (0.0, -20.0));
        assert!(c.ancora(r).unwrap().alcas_colineares());
    }

    #[test]
    fn a_curvatura_passa_lisa_pelos_pontos_e_respeita_o_canto() {
        let mut c = Caminho::novo(1, "t");
        let r0 = novo_subcaminho(&mut c, Ponto::novo(0.0, 0.0), OperacaoDoComponente::Somar);
        let r1 = acrescentar(&mut c, r0.sub, Extremo::Fim, Ponto::novo(50.0, 40.0)).unwrap();
        let r2 = acrescentar(&mut c, r0.sub, Extremo::Fim, Ponto::novo(100.0, 0.0)).unwrap();
        for r in [r0, r1, r2] {
            tornar_automatica(&mut c, r, false);
        }
        assert!(recalcular_automaticas(&mut c));
        let a = c.ancora(r1).unwrap();
        assert!(a.alcas_colineares());
        // Tangente paralela ao vizinho de trás → da frente (horizontal).
        assert!((a.saida.unwrap().y - 40.0).abs() < 1e-9);
        // As pontas do aberto não têm alça.
        assert!(c.ancora(r0).unwrap().saida.is_none());
        // A curva passa pelos três pontos (âncoras) e é lisa no do meio.
        // Mexer no vizinho refaz.
        mover_ancoras(&mut c, &[r2], (0.0, 80.0));
        assert!(recalcular_automaticas(&mut c));
        let a = c.ancora(r1).unwrap();
        assert!(a.saida.unwrap().y > 40.0);
        // Canto: sem alças.
        tornar_automatica(&mut c, r1, true);
        recalcular_automaticas(&mut c);
        assert!(c.ancora(r1).unwrap().saida.is_none());
        // A alça mexida à mão solta a âncora da regra.
        tornar_automatica(&mut c, r1, false);
        recalcular_automaticas(&mut c);
        mover_alca(&mut c, r1, Lado::Saida, Ponto::novo(90.0, 10.0), false);
        assert!(!c.ancora(r1).unwrap().automatica);
        assert!(!recalcular_automaticas(&mut c), "não volta sozinha");
    }

    #[test]
    fn a_afim_leva_a_curva_exata() {
        let (mut c, sub) = curva_simples();
        let antes = amostras(&c, sub);
        transformar(&mut c, [0.0, -2.0, 10.0, 2.0, 0.0, -5.0]);
        let depois = amostras(&c, sub);
        for (a, b) in antes.iter().zip(&depois) {
            let esperado = Ponto::novo(-2.0 * a.y + 10.0, 2.0 * a.x - 5.0);
            assert!(b.distancia(esperado) < 1e-9);
        }
    }
}
