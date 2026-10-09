//! O documento: a base de que partiu e as camadas por cima dela.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::ajuste::Ajuste;
use crate::difusao::{Guarda, MapaDifuso, DIFUSAO_MAXIMA};
use crate::mesclagem::Modo;
use crate::retangulo::Retangulo;
use crate::tiles::{retangulo_do_tile, CamadaDePixels};

/// O perfil dos pixels da base e da imagem editada (C29): sRGB codificado, 8
/// bits por canal. Um formato novo (16 bits) terá outro nome.
pub const PERFIL: &str = "sRGB-8";

/// A base neutra de que o documento partiu — **nunca** os pixels, só quem ela
/// é. Os pixels vêm do bruto a cada abertura (C28).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaseRef {
    pub largura: u32,
    pub altura: u32,
    /// O SHA-256 dos pixels RGB da base, precedido das dimensões. É o que a
    /// reabertura confere: pintar sobre outra base desalinharia tudo.
    pub sha256: String,
    pub perfil: String,
}

impl BaseRef {
    pub fn da_imagem(base: &image::RgbImage) -> Self {
        Self {
            largura: base.width(),
            altura: base.height(),
            sha256: impressao_da_base(base),
            perfil: PERFIL.into(),
        }
    }
}

/// A impressão digital da base: dimensões e pixels.
pub fn impressao_da_base(base: &image::RgbImage) -> String {
    let mut hash = Sha256::new();
    hash.update(base.width().to_le_bytes());
    hash.update(base.height().to_le_bytes());
    hash.update(base.as_raw());
    hex(&hash.finalize())
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A máscara da camada, como no Photoshop: branco revela, preto esconde, os
/// cinzas deixam passar em parte.
///
/// 🔑 **É uma camada de pixels como as outras**, pintada sobre o `fundo`: o
/// valor de um pixel é a cor pintada (em cinza) sobre o fundo, na proporção do
/// alfa. Assim o pincel, a borracha (que devolve ao fundo), o Delete, o
/// preenchimento, o degradê e a lata de tinta funcionam nela sem código à
/// parte — e um tile que nunca foi pintado não existe, como na camada.
///
/// 🔑 **Densidade e difusão não mexem nos pixels** (Propriedades da máscara,
/// etapa 16): entram só na conta da composição ([`Mascara::leitor`]). Voltar a
/// difusão a 0 devolve a borda que foi pintada.
#[derive(Clone, Debug)]
pub struct Mascara {
    /// O valor de onde não se pintou: 255 revela tudo (o botão), 0 esconde
    /// tudo (⌥ + o botão).
    pub fundo: u8,
    /// Desligada (⇧ + clique na miniatura), a camada aparece inteira.
    pub ativa: bool,
    pub pixels: CamadaDePixels,
    /// A corrente entre a miniatura da camada e a da máscara (ligada ao
    /// nascer): o Mover, o ⌘T e o Deformar levam as duas juntas.
    pub vinculada: bool,
    /// `0..=1`: quanto o preto esconde. Com 0,5, o preto deixa passar metade.
    pub densidade: f32,
    /// O raio do desfoque da máscara na composição, em pixels da foto.
    pub difusao: f32,
    /// O mapa desfocado (só com difusão), refeito onde os pixels mudarem.
    mapa: Guarda,
}

impl PartialEq for Mascara {
    fn eq(&self, outra: &Self) -> bool {
        self.fundo == outra.fundo
            && self.ativa == outra.ativa
            && self.pixels == outra.pixels
            && self.vinculada == outra.vinculada
            && self.densidade == outra.densidade
            && self.difusao == outra.difusao
    }
}

impl Mascara {
    pub fn nova(fundo: u8, largura: u32, altura: u32) -> Self {
        Self::com_pixels(fundo, CamadaDePixels::nova(largura, altura))
    }

    /// Ligada, vinculada, densidade 100% e sem difusão — como nasce.
    pub fn com_pixels(fundo: u8, pixels: CamadaDePixels) -> Self {
        Self {
            fundo,
            ativa: true,
            pixels,
            vinculada: true,
            densidade: 1.0,
            difusao: 0.0,
            mapa: Guarda::default(),
        }
    }

    /// O valor (0 esconde, 255 revela) de um pixel RGBA da máscara.
    #[inline]
    pub fn valor_do_pixel(fundo: u8, p: [u8; 4]) -> u8 {
        if p[3] == 0 {
            return fundo;
        }
        let cinza = ((77 * p[0] as u32 + 150 * p[1] as u32 + 29 * p[2] as u32 + 128) >> 8) as i32;
        let (f, a) = (fundo as i32, p[3] as i32);
        let d = (cinza - f) * a;
        (f + (d + 127 * d.signum()) / 255) as u8
    }

    /// O valor pintado em `(x, y)` — sem densidade nem difusão.
    pub fn valor(&self, x: u32, y: u32) -> u8 {
        Self::valor_do_pixel(self.fundo, self.pixels.pixel(x, y))
    }

    /// O valor que entra na composição em `(x, y)`: com a difusão e a
    /// densidade.
    pub fn valor_efetivo(&self, x: u32, y: u32) -> u8 {
        let leitor = self.leitor();
        let posicao = crate::tiles::tile_de(x, y);
        let (ox, oy) = crate::tiles::origem_do_tile(posicao);
        leitor.no_tile(posicao).valor(crate::tiles::indice(
            (x as i64 - ox) as u32,
            (y as i64 - oy) as u32,
        ))
    }

    /// A máscara esconde a camada inteira.
    pub fn esconde_tudo(&self) -> bool {
        self.ativa && self.fundo == 0 && self.densidade >= 1.0 && self.pixels.vazia()
    }

    /// O mesmo valor com a densidade: `255 − densidade · (255 − v)`.
    pub fn tabela_da_densidade(&self) -> [u8; 256] {
        let d = self.densidade.clamp(0.0, 1.0);
        std::array::from_fn(|v| {
            if d >= 1.0 {
                v as u8
            } else {
                255 - ((255 - v) as f32 * d).round() as u8
            }
        })
    }

    /// Quem lê a máscara na composição — uma vez por conta, e depois tile a
    /// tile.
    pub fn leitor(&self) -> LeitorDaMascara<'_> {
        let mapa = (self.difusao > 0.0).then(|| {
            self.mapa
                .mapa(&self.pixels, self.fundo, self.difusao.min(DIFUSAO_MAXIMA))
        });
        LeitorDaMascara {
            mascara: self,
            mapa,
            tabela: self.tabela_da_densidade(),
        }
    }

    /// Quanto uma mudança nos pixels em `sujo` muda a foto: com difusão, o
    /// desfoque leva a mudança três raios além.
    pub fn alcance(&self, sujo: &Retangulo) -> Retangulo {
        if self.difusao <= 0.0 || sujo.vazio() {
            return *sujo;
        }
        let m = MapaDifuso::alcance(self.difusao.min(DIFUSAO_MAXIMA));
        let (largura, altura) = (self.pixels.largura(), self.pixels.altura());
        Retangulo::novo(
            sujo.x.saturating_sub(m),
            sujo.y.saturating_sub(m),
            sujo.largura + 2 * m,
            sujo.altura + 2 * m,
        )
        .limitado(largura, altura)
    }

    /// Inverter (⌘I na máscara, "Inverter" nas Propriedades): o que revelava
    /// passa a esconder, valor a valor (`255 − v`), sem perder a borda.
    pub fn invertida(&self) -> Mascara {
        let mut nova = self.clone();
        nova.mapa = Guarda::default();
        nova.fundo = 255 - self.fundo;
        for (posicao, tile) in self.pixels.todos() {
            let mut t = tile.as_ref().clone();
            for p in t.as_chunks_mut::<4>().0 {
                if p[3] > 0 {
                    *p = [255 - p[0], 255 - p[1], 255 - p[2], p[3]];
                }
            }
            nova.pixels.definir(*posicao, Some(std::sync::Arc::new(t)));
        }
        nova
    }
}

/// O valor da máscara num tile da composição.
pub enum MascaraNoTile<'a> {
    /// O tile inteiro vale o mesmo (já com a densidade).
    Constante(u8),
    Pixels {
        tile: &'a [u8],
        fundo: u8,
        tabela: [u8; 256],
    },
    Difusa {
        mapa: &'a MapaDifuso,
        origem: (i64, i64),
        tabela: [u8; 256],
    },
}

impl MascaraNoTile<'_> {
    /// O valor no pixel cujo byte R está em `j` dentro do tile.
    #[inline]
    pub fn valor(&self, j: usize) -> u8 {
        match self {
            MascaraNoTile::Constante(v) => *v,
            MascaraNoTile::Pixels {
                tile,
                fundo,
                tabela,
            } => {
                tabela[Mascara::valor_do_pixel(
                    *fundo,
                    [tile[j], tile[j + 1], tile[j + 2], tile[j + 3]],
                ) as usize]
            }
            MascaraNoTile::Difusa {
                mapa,
                origem,
                tabela,
            } => {
                let p = (j / 4) as i64;
                let lado = crate::tiles::LADO_DO_TILE as i64;
                tabela[mapa.valor(origem.0 + p % lado, origem.1 + p / lado) as usize]
            }
        }
    }
}

/// A máscara pronta para ser lida tile a tile.
pub struct LeitorDaMascara<'a> {
    mascara: &'a Mascara,
    mapa: Option<std::sync::Arc<MapaDifuso>>,
    tabela: [u8; 256],
}

impl LeitorDaMascara<'_> {
    pub fn no_tile(&self, posicao: crate::tiles::Posicao) -> MascaraNoTile<'_> {
        if let Some(mapa) = &self.mapa {
            return MascaraNoTile::Difusa {
                mapa,
                origem: crate::tiles::origem_do_tile(posicao),
                tabela: self.tabela,
            };
        }
        match self.mascara.pixels.tile(posicao) {
            Some(t) => MascaraNoTile::Pixels {
                tile: t.as_slice(),
                fundo: self.mascara.fundo,
                tabela: self.tabela,
            },
            None => MascaraNoTile::Constante(self.tabela[self.mascara.fundo as usize]),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Camada {
    pub nome: String,
    pub visivel: bool,
    /// `0..=1`.
    pub opacidade: f32,
    pub modo: Modo,
    pub pixels: CamadaDePixels,
    pub mascara: Option<Mascara>,
    /// Uma camada de ajuste: sem pixels, muda a cor do que está abaixo
    /// (`ajuste.rs`). O pincel pinta na máscara dela.
    pub ajuste: Option<Ajuste>,
    /// Máscara de corte (⌥ + clique na divisa, "Criar máscara de corte"): a
    /// camada só aparece onde a **base do conjunto** — a primeira camada não
    /// recortada abaixo dela — tem pixels. Ver [`Documento::papeis`].
    pub recortada: bool,
    /// Os cadeados do painel Camadas (etapa 16).
    pub bloqueio: Bloqueio,
}

/// Os bloqueios da camada ("Bloquear:" no painel Camadas do Photoshop).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bloqueio {
    /// Pixels transparentes (`/`): a tinta muda a cor do que existe e não
    /// pinta onde é transparente; a borracha pinta a cor de fundo.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub transparencia: bool,
    /// Pixels: nada pinta, apaga ou preenche a camada (a máscara continua
    /// aberta, como no Photoshop).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pixels: bool,
    /// Posição: o Mover, o ⌘T e o Deformar não levam a camada.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub posicao: bool,
}

impl Bloqueio {
    /// O cadeado "Bloquear tudo": os três.
    pub const TUDO: Bloqueio = Bloqueio {
        transparencia: true,
        pixels: true,
        posicao: true,
    };

    pub fn tudo(&self) -> bool {
        *self == Self::TUDO
    }

    pub fn algum(&self) -> bool {
        self.transparencia || self.pixels || self.posicao
    }
}

impl Camada {
    /// Transparente e visível, do tamanho da base.
    pub fn nova(nome: &str, largura: u32, altura: u32) -> Self {
        Self {
            nome: nome.into(),
            visivel: true,
            opacidade: 1.0,
            modo: Modo::Normal,
            pixels: CamadaDePixels::nova(largura, altura),
            mascara: None,
            ajuste: None,
            recortada: false,
            bloqueio: Bloqueio::default(),
        }
    }

    /// Uma camada de ajuste com a máscara branca (revela tudo), como o
    /// Photoshop a cria.
    pub fn de_ajuste(nome: &str, ajuste: Ajuste, largura: u32, altura: u32) -> Self {
        let mut camada = Self::nova(nome, largura, altura);
        camada.ajuste = Some(ajuste);
        camada.mascara = Some(Mascara::nova(255, largura, altura));
        camada
    }

    /// A máscara que vale na composição — `None` sem máscara ou com ela
    /// desligada.
    pub fn mascara_ativa(&self) -> Option<&Mascara> {
        self.mascara.as_ref().filter(|m| m.ativa)
    }

    /// Os pixels onde se pinta: os da máscara ou os da camada.
    pub fn alvo(&self, na_mascara: bool) -> &CamadaDePixels {
        match (&self.mascara, na_mascara) {
            (Some(m), true) => &m.pixels,
            _ => &self.pixels,
        }
    }

    pub fn alvo_mut(&mut self, na_mascara: bool) -> &mut CamadaDePixels {
        match (&mut self.mascara, na_mascara) {
            (Some(m), true) => &mut m.pixels,
            _ => &mut self.pixels,
        }
    }

    /// O retângulo que os tiles pintados cobrem — onde a camada pode mudar a
    /// foto. Esconder, mexer na opacidade ou trocar de lugar só recompõe aqui.
    pub fn area(&self) -> Retangulo {
        let (largura, altura) = (self.pixels.largura(), self.pixels.altura());
        if self.ajuste.is_some() {
            // O ajuste muda a foto inteira — ou só onde a máscara que esconde
            // tudo foi pintada.
            return match self.mascara_ativa() {
                Some(m) if m.fundo == 0 && m.densidade >= 1.0 => {
                    m.alcance(&area_dos_tiles(&m.pixels))
                }
                _ => Retangulo::inteiro(largura, altura),
            };
        }
        self.pixels
            .existentes()
            .fold(Retangulo::default(), |area, (posicao, _)| {
                area.uniao(&retangulo_do_tile(*posicao, largura, altura))
            })
    }

    /// A camada não muda nenhum pixel da base (C30).
    pub fn sem_efeito(&self) -> bool {
        !self.visivel
            || self.opacidade <= 0.0
            || match &self.ajuste {
                Some(a) => a.neutro(),
                None => self.pixels.vazia(),
            }
            || self.mascara.as_ref().is_some_and(Mascara::esconde_tudo)
    }
}

fn area_dos_tiles(pixels: &CamadaDePixels) -> Retangulo {
    let (largura, altura) = (pixels.largura(), pixels.altura());
    pixels
        .existentes()
        .fold(Retangulo::default(), |area, (posicao, _)| {
            area.uniao(&retangulo_do_tile(*posicao, largura, altura))
        })
}

/// Como uma camada entra na composição.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Papel {
    /// Não muda nada (escondida, vazia, opacidade 0 — ou recortada por uma
    /// base sem efeito).
    Fora,
    /// Composta sobre o que está embaixo, no modo dela.
    Solta,
    /// A base de um conjunto de recorte com pelo menos uma recortada.
    Base,
    /// Composta sobre a base, com a transparência dela travada.
    Recortada,
}

/// O nome da camada que nasce com o documento.
pub const NOME_DA_PRIMEIRA: &str = "Pintura";

/// O nome da camada criada da fotografia base ("Criar camada da fotografia
/// base").
pub const NOME_DA_FOTOGRAFIA: &str = "Fotografia";

/// Uma guia do Photoshop: uma linha que não imprime, em pixels da foto.
/// `vertical` = a linha de cima a baixo (sai da régua da esquerda), na
/// coluna `posicao`; senão, horizontal, na linha `posicao`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Guia {
    pub vertical: bool,
    pub posicao: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Documento {
    pub base: BaseRef,
    /// De baixo para cima — a de índice 0 fica logo acima da base.
    pub camadas: Vec<Camada>,
    /// As guias (não entram na imagem editada nem no `neutro`).
    pub guias: Vec<Guia>,
}

impl Documento {
    /// Um documento novo: a base e uma camada transparente por cima.
    pub fn novo(base: BaseRef) -> Self {
        let camada = Camada::nova(NOME_DA_PRIMEIRA, base.largura, base.altura);
        Self {
            base,
            camadas: vec![camada],
            guias: Vec::new(),
        }
    }

    pub fn largura(&self) -> u32 {
        self.base.largura
    }

    pub fn altura(&self) -> u32 {
        self.base.altura
    }

    /// O nome de uma camada nova: "Camada N", com o N seguinte ao maior que já
    /// existe — como no Photoshop, que não reaproveita o número de quem saiu.
    pub fn proximo_nome(&self) -> String {
        let maior = self
            .camadas
            .iter()
            .filter_map(|c| c.nome.strip_prefix("Camada ")?.trim().parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        format!("Camada {}", maior + 1)
    }

    /// Nenhuma camada muda a base (C30): a imagem editada seria a base byte a
    /// byte, e não se publica versão.
    pub fn neutro(&self) -> bool {
        self.papeis().iter().all(|p| *p == Papel::Fora)
    }

    /// A base do conjunto de recorte da camada `indice`: a primeira camada
    /// **não recortada** abaixo dela. `None` quando a camada não é recortada,
    /// quando não há camada embaixo, ou quando a base é de ajuste (a composição
    /// ainda não recorta por ela — a camada é composta solta).
    pub fn base_do_recorte(&self, indice: usize) -> Option<usize> {
        if !self.camadas.get(indice)?.recortada {
            return None;
        }
        let base = (0..indice).rev().find(|&j| !self.camadas[j].recortada)?;
        self.camadas[base].ajuste.is_none().then_some(base)
    }

    /// O papel de cada camada na composição (índices do documento).
    ///
    /// 🔑 **Regras da máscara de corte** (as do Photoshop com "Mesclar camadas
    /// recortadas como grupo", o padrão):
    /// - a base delimita o conjunto pelo **alfa dela vezes a máscara dela**;
    /// - base escondida, com opacidade 0 ou vazia esconde o conjunto inteiro;
    /// - uma recortada escondida (ou vazia) só some ela;
    /// - a base que não tem nenhuma recortada com efeito é composta solta, pela
    ///   conta de sempre (byte a byte a mesma de antes do recorte existir);
    /// - recortada sem base válida é composta solta.
    pub fn papeis(&self) -> Vec<Papel> {
        let mut papeis = vec![Papel::Fora; self.camadas.len()];
        for (i, c) in self.camadas.iter().enumerate() {
            match self.base_do_recorte(i) {
                Some(base) => {
                    if papeis[base] != Papel::Fora && !c.sem_efeito() {
                        papeis[i] = Papel::Recortada;
                        papeis[base] = Papel::Base;
                    }
                }
                None if !c.sem_efeito() => papeis[i] = Papel::Solta,
                None => {}
            }
        }
        papeis
    }

    /// A última camada do conjunto de recorte que começa na base `indice` (a
    /// própria base quando nada é recortado nela).
    pub fn fim_do_conjunto(&self, indice: usize) -> usize {
        let mut fim = indice;
        while self.camadas.get(fim + 1).is_some_and(|c| c.recortada) {
            fim += 1;
        }
        fim
    }
}
