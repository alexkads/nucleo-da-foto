//! Os 74 ajustes, no layout que o WGSL espera.

use serde::{Deserialize, Serialize};

/// Os ajustes, no layout que o WGSL espera.
///
/// 🚨 **Os nomes e a ordem são os do shader, e ficam em inglês de propósito.**
/// `repr(C)` + `bytemuck` mandam esta struct para a GPU como bytes crus, campo a
/// campo, por **posição**. Um campo fora de lugar aqui não é erro de compilação,
/// é a foto saindo com o ajuste errado aplicado — e poder ler os dois lados um ao
/// lado do outro é a única defesa que existe.
///
/// 🚨 **E ela já falhou uma vez: o `uniform` do outro lado declarava 28 campos.**
/// Do campo 23 em diante o shader lia o do vizinho (o matiz do vermelho virava
/// redução de ruído) e do 28 em diante não lia nada — os 4 controles de Detalhe e
/// os 3 de Lente não faziam efeito nenhum. Defeito herdado do `crates/ui`, que
/// mandava a mesma struct para o mesmo shader, e **consertado em 17/ago/2026**,
/// depois que a fase 5 tirou o outro app do caminho: o `struct Params` passou a
/// declarar os 46 na mesma ordem, e quem prende isso é
/// `o_wgsl_declara_os_mesmos_campos_na_mesma_ordem`.
///
/// ✅ **E desde 17/ago/2026 os 46 campos chegam ao shader e todos têm código que
/// os use.** Chegar e ser aplicado são duas coisas, e as duas custaram um
/// conserto próprio no mesmo dia: o alinhamento do `uniform`, e depois o corpo do
/// shader, que não mencionava matiz, luminância nem lente em lugar nenhum.
///
/// 🔑 **Desde 2026-09-04 a struct viaja também como JSON por nome** (`serde`),
/// para o site guardar a revelação de uma foto. Campo ausente no JSON recebe o
/// neutro de [`Ajustes::default`] — o mesmo `if let Some` de `ajustes_da_entidade`.
///
/// ✅ **Em 2026-09-06 entraram os 7 da Tonalização e do Grão**, e eles entraram
/// **no fim**, depois de `sharpen_radius`. Uma revelação gravada antes disso não
/// tem os campos novos no JSON, e o `serde(default)` os completa com o neutro —
/// que é o mesmo que "sem tonalização e sem grão". Fosse a inserção no meio,
/// toda revelação já gravada leria o campo do vizinho a partir dali.
#[repr(C)]
#[derive(
    Copy, Clone, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable, Serialize, Deserialize,
)]
#[serde(default)]
pub struct Ajustes {
    pub exposure: f32,
    pub contrast: f32,
    pub temperature: f32,
    pub tint: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
    pub clarity: f32,
    pub vibrance: f32,
    pub saturation: f32,
    pub tone_curve_shadows: f32,
    pub tone_curve_darks: f32,
    pub tone_curve_lights: f32,
    pub tone_curve_highlights: f32,
    pub hsl_red_sat: f32,
    pub hsl_orange_sat: f32,
    pub hsl_yellow_sat: f32,
    pub hsl_green_sat: f32,
    pub hsl_aqua_sat: f32,
    pub hsl_blue_sat: f32,
    pub hsl_purple_sat: f32,
    pub hsl_magenta_sat: f32,
    pub hsl_red_hue: f32,
    pub hsl_orange_hue: f32,
    pub hsl_yellow_hue: f32,
    pub hsl_green_hue: f32,
    pub hsl_aqua_hue: f32,
    pub hsl_blue_hue: f32,
    pub hsl_purple_hue: f32,
    pub hsl_magenta_hue: f32,
    pub hsl_red_lum: f32,
    pub hsl_orange_lum: f32,
    pub hsl_yellow_lum: f32,
    pub hsl_green_lum: f32,
    pub hsl_aqua_lum: f32,
    pub hsl_blue_lum: f32,
    pub hsl_purple_lum: f32,
    pub hsl_magenta_lum: f32,
    pub lens_distortion: f32,
    pub lens_vignette_amount: f32,
    pub lens_vignette_midpoint: f32,
    pub nr_luminance: f32,
    pub nr_color: f32,
    pub sharpen_amount: f32,
    pub sharpen_radius: f32,
    pub split_shadow_hue: f32,
    pub split_shadow_sat: f32,
    pub split_highlight_hue: f32,
    pub split_highlight_sat: f32,
    pub split_balance: f32,
    pub grain_amount: f32,
    pub grain_size: f32,
    // ------------------------------------------------ Calibração de câmera
    // 🚨 **É a base da maioria dos presets de filme**, e faltava inteira até
    // 2026-09-12 — o operador do estúdio relatou que não conseguia reproduzir
    // os estilos que tem no Lightroom e no darktable, e este era o buraco
    // maior depois da curva por ponto. Ela move os **primários** antes de todo
    // o resto: um preset que gira o vermelho para o laranja e dessatura o azul
    // muda a foto inteira, não uma faixa de matiz como o HSL faz.
    //
    // ⚠️ **É uma aproximação declarada.** A calibração da Adobe é uma matriz no
    // espaço do perfil da câmera, que este motor não tem (ele recebe RGB já
    // revelado). Aqui ela é rotação de matiz e escala de saturação em torno de
    // cada primário, com queda larga — o comportamento visível é o mesmo nos
    // valores que os presets usam; o que muda é o extremo.
    pub calib_red_hue: f32,
    pub calib_red_sat: f32,
    pub calib_green_hue: f32,
    pub calib_green_sat: f32,
    pub calib_blue_hue: f32,
    pub calib_blue_sat: f32,
    pub calib_shadow_tint: f32,
    // ------------------------------------- Color Grading: os eixos que faltavam
    // O split toning clássico tem duas pontas; o "Color Grading" do LR 10 em
    // diante tem três mais um global, e os presets modernos usam os quatro. Sem
    // os tons médios, um preset cinematográfico perde justamente onde a pele
    // vive.
    pub split_midtone_hue: f32,
    pub split_midtone_sat: f32,
    pub split_global_hue: f32,
    pub split_global_sat: f32,
    /// Quanto as três faixas se misturam — o `Blending` da Adobe, 0 a 100.
    ///
    /// 🔑 Neutro **50**, e não 0: é o valor em que a Adobe abre o controle, e
    /// abrir em 0 daria três faixas de bordas duras numa foto que ninguém
    /// tocou. Ver `Ajustes::default`.
    pub split_blending: f32,
    // ------------------------------------------ Mixer de preto e branco
    // 🚨 **Zero cobertura até 2026-09-12.** Um preset B&W do Lightroom virava
    // `saturation = -1` e perdia a mistura por canal — que é exatamente o que
    // separa um P&B de retrato (pele clara, céu escuro) de um cinza chapado.
    /// Liga o mixer. Sem ele os oito abaixo não fazem nada, como no Lightroom:
    /// o mixer só existe com a foto convertida para P&B.
    pub bw_ativo: f32,
    pub bw_red: f32,
    pub bw_orange: f32,
    pub bw_yellow: f32,
    pub bw_green: f32,
    pub bw_aqua: f32,
    pub bw_blue: f32,
    pub bw_purple: f32,
    pub bw_magenta: f32,
}

/// Quantos campos a struct tem — e quantos `f32` o vetor posicional carrega.
///
/// ⚠️ **Eram 46 até 2026-09-06, e 53 até 2026-09-12.** A Tonalização (5) e o
/// Grão (2) entraram primeiro; depois a Calibração de câmera (7), os eixos que
/// faltavam do Color Grading (5) e o mixer de preto e branco (9). **Todos no
/// fim da lista**, e não perto do que se parece com eles: a posição de um campo
/// é o contrato com o shader, e mover `nr_luminance` para junto do grão faria
/// toda revelação já gravada ler o campo do vizinho.
pub const QUANTIDADE: usize = 74;

/// O tamanho do buffer de `uniform`, arredondado para múltiplo de 16 bytes.
///
/// 🚨 **Não é `size_of::<Ajustes>()`, e a diferença é uma regra do WGSL.** No
/// endereço `uniform` o alinhamento de uma struct é `roundUp(16, …)`, então os
/// 53 `f32` (212 bytes) ocupam 224 do ponto de vista do shader — e o `bind
/// group` recusa um buffer menor que isso. Enquanto o `struct Params` declarava
/// 28 campos (112 bytes, múltiplo de 16) ninguém precisava saber disto.
///
/// Os 12 bytes de sobra nunca são escritos nem lidos: a CPU manda os 212 do
/// `bytemuck::bytes_of`, e o shader não tem campo além do 52. O enchimento do
/// WGSL acompanha: eram dois `_enchimento` para 184 bytes, são três para 212.
pub(crate) const TAMANHO_DO_UNIFORM: wgpu::BufferAddress = {
    let bytes = std::mem::size_of::<Ajustes>() as wgpu::BufferAddress;
    bytes.next_multiple_of(16)
};

impl Default for Ajustes {
    /// O neutro, conferido campo a campo contra o `crates/ui`.
    ///
    /// ⚠️ **Nem todo neutro é zero**, e é por isso que este `Default` é escrito e
    /// não derivado: `contrast` neutro é `1.0` (é um multiplicador) e
    /// `sharpen_radius` é `1.0` (raio zero seria não ter pixel de vizinhança).
    /// Derivar daria zero nos dois, e a foto abriria já alterada — sem ninguém
    /// ter tocado em nada.
    ///
    /// 🚨 **`lens_vignette_midpoint` era `50.0` aqui, e estava errado.** O 50 veio
    /// de `GpuEditParams::default` do `crates/ui` — que o app de lá **nunca
    /// chama**: o único chamador em todo o repositório é um teste que confere só
    /// os 11 campos do Básico. O que o legado de fato usa é
    /// `AppState::new`/`reset_edits`, e nos dois o meio da vinheta é **`0.0`**.
    /// Copiar a `impl Default` em vez do caminho vivo fazia o slider "Meio da
    /// vinheta" abrir em 50 aqui e em 0 lá, na mesma foto.
    ///
    /// 🔑 A pergunta que separa os dois: não é "qual é o padrão declarado", é
    /// **"qual valor a foto recebe quando ninguém mexeu em nada"**. `impl Default`
    /// responde a primeira, e ela pode ser código morto.
    ///
    /// ⚠️ **E a resposta certa, para foto de verdade, é um quarto lugar: o
    /// schema.** `014_add_hsl_lens_fields.sql` cria a coluna com
    /// `DEFAULT 50.0`, então toda foto importada volta do banco com o meio da
    /// vinheta preenchido — e é esse 50 que os dois apps mostram no slider. O que
    /// este `Default` decide é o resto: o estado antes de abrir qualquer foto, o
    /// futuro "redefinir", e o campo que vier `NULL`. Nos três o legado diz 0.0.
    fn default() -> Self {
        let mut neutro: Self = bytemuck::Zeroable::zeroed();
        neutro.contrast = 1.0;
        neutro.sharpen_radius = 1.0;
        // 🚨 **O terceiro neutro que não é zero** (2026-09-12). A mistura do
        // Color Grading abre em 50 na Adobe; em 0 as três faixas teriam borda
        // dura, e uma foto que ninguém tocou já sairia diferente.
        neutro.split_blending = 50.0;
        neutro
    }
}

impl Ajustes {
    /// Os 74 nomes, na ordem do `uniform`.
    ///
    /// 🔑 É a ordem que o vetor posicional ([`Ajustes::como_vetor`]) segue, a
    /// que o `struct Params` do WGSL declara, e a que o site recebe em
    /// `nomes.json`. Três lugares, uma lista.
    pub const NOMES: [&'static str; QUANTIDADE] = [
        "exposure",
        "contrast",
        "temperature",
        "tint",
        "highlights",
        "shadows",
        "whites",
        "blacks",
        "clarity",
        "vibrance",
        "saturation",
        "tone_curve_shadows",
        "tone_curve_darks",
        "tone_curve_lights",
        "tone_curve_highlights",
        "hsl_red_sat",
        "hsl_orange_sat",
        "hsl_yellow_sat",
        "hsl_green_sat",
        "hsl_aqua_sat",
        "hsl_blue_sat",
        "hsl_purple_sat",
        "hsl_magenta_sat",
        "hsl_red_hue",
        "hsl_orange_hue",
        "hsl_yellow_hue",
        "hsl_green_hue",
        "hsl_aqua_hue",
        "hsl_blue_hue",
        "hsl_purple_hue",
        "hsl_magenta_hue",
        "hsl_red_lum",
        "hsl_orange_lum",
        "hsl_yellow_lum",
        "hsl_green_lum",
        "hsl_aqua_lum",
        "hsl_blue_lum",
        "hsl_purple_lum",
        "hsl_magenta_lum",
        "lens_distortion",
        "lens_vignette_amount",
        "lens_vignette_midpoint",
        "nr_luminance",
        "nr_color",
        "sharpen_amount",
        "sharpen_radius",
        "split_shadow_hue",
        "split_shadow_sat",
        "split_highlight_hue",
        "split_highlight_sat",
        "split_balance",
        "grain_amount",
        "grain_size",
        "calib_red_hue",
        "calib_red_sat",
        "calib_green_hue",
        "calib_green_sat",
        "calib_blue_hue",
        "calib_blue_sat",
        "calib_shadow_tint",
        "split_midtone_hue",
        "split_midtone_sat",
        "split_global_hue",
        "split_global_sat",
        "split_blending",
        "bw_ativo",
        "bw_red",
        "bw_orange",
        "bw_yellow",
        "bw_green",
        "bw_aqua",
        "bw_blue",
        "bw_purple",
        "bw_magenta",
    ];

    /// Os 46 valores, por posição — o que a GPU recebe, como `f32`.
    pub fn como_vetor(&self) -> [f32; QUANTIDADE] {
        let campos: &[f32] = bytemuck::cast_slice(bytemuck::bytes_of(self));
        campos.try_into().expect("53 campos de quatro bytes")
    }

    /// De um vetor posicional de 46 `f32`; `None` se o tamanho for outro.
    ///
    /// 🚨 É por aqui que o navegador manda os ajustes, e o tamanho é a única
    /// conferência possível: 52 valores seriam 52 campos certos e um lixo.
    pub fn de_vetor(valores: &[f32]) -> Option<Self> {
        if valores.len() != QUANTIDADE {
            return None;
        }
        Some(bytemuck::pod_read_unaligned(bytemuck::cast_slice(valores)))
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    /// ⚠️ O neutro **não é zero** em dois campos — e num terceiro parecia não ser.
    ///
    /// `contrast` é multiplicador e `sharpen_radius` zero seria não ter pixel de
    /// vizinhança: um `#[derive(Default)]` daria zero nos dois e a foto abriria
    /// alterada sem ninguém ter tocado em nada.
    ///
    /// 🚨 `lens_vignette_midpoint` está aqui pelo motivo oposto: ele **é** zero, e
    /// já esteve em 50 porque foi copiado de `GpuEditParams::default`, uma `impl`
    /// que o app do legado nunca chama. O que o legado usa — `AppState::new` e
    /// `reset_edits` — diz `0.0`. O teste cobra o valor do caminho vivo, e não o
    /// do padrão declarado.
    #[test]
    fn o_neutro_e_o_do_caminho_vivo_do_legado() {
        let neutro = Ajustes::default();
        assert_eq!(neutro.contrast, 1.0);
        assert_eq!(neutro.sharpen_radius, 1.0);
        assert_eq!(
            neutro.lens_vignette_midpoint, 0.0,
            "é o que `AppState::new` e `reset_edits` põem no slider; \
             o 50 de `GpuEditParams::default` é código morto"
        );
        assert_eq!(neutro.exposure, 0.0);
        assert_eq!(neutro.saturation, 0.0);
    }

    /// O layout que vai para a GPU tem os 74 campos, de quatro bytes cada.
    ///
    /// Campo a mais desloca **todos** os seguintes na leitura do shader, e o
    /// sintoma é a saturação virando nitidez.
    ///
    /// ⚠️ **O número do `uniform` é escrito à mão de propósito.** Derivá-lo aqui
    /// (`size_of().next_multiple_of(16)`) faria o teste concordar com qualquer
    /// mudança, inclusive com a errada — e é justamente o alinhamento de 16
    /// bytes do WebGL2 que já derrubou este shader uma vez. 74 × 4 = 296, e o
    /// próximo múltiplo de 16 é 304.
    #[test]
    fn o_layout_tem_os_campos_de_quatro_bytes() {
        assert_eq!(std::mem::size_of::<Ajustes>(), QUANTIDADE * 4);
        assert_eq!(TAMANHO_DO_UNIFORM, 304);
    }

    /// Os nomes do `struct Params` do WGSL, na ordem em que ele os declara.
    fn campos_do_wgsl() -> Vec<String> {
        let shader = include_str!("shaders/corpo.wgsl");
        shader
            .split("struct Params {")
            .nth(1)
            .and_then(|resto| resto.split('}').next())
            .expect("o shader tem de declarar `struct Params`")
            .lines()
            .filter_map(|linha| linha.split(':').next())
            .map(str::trim)
            // O enchimento de 16 bytes do WebGL2 começa com `_` e não é campo.
            .filter(|nome| !nome.is_empty() && !nome.starts_with("//") && !nome.starts_with('_'))
            .map(str::to_string)
            .collect()
    }

    /// O `struct Params` do WGSL, com o enchimento, ocupa os mesmos 224 bytes
    /// que `TAMANHO_DO_UNIFORM` reserva — nem mais (o Rust não escreveria o
    /// resto) nem menos (o WebGL2 recusaria o pipeline).
    #[test]
    fn o_enchimento_do_wgsl_fecha_os_192_bytes() {
        let shader = include_str!("shaders/corpo.wgsl");
        let campos = shader
            .split("struct Params {")
            .nth(1)
            .and_then(|resto| resto.split('}').next())
            .expect("o shader tem de declarar `struct Params`")
            .lines()
            .map(str::trim)
            .filter(|l| l.contains(": f32"))
            .count();
        assert_eq!(campos as u64 * 4, TAMANHO_DO_UNIFORM);
    }

    /// O `struct Params` do WGSL declara os mesmos 53 campos do `Ajustes`, na
    /// mesma ordem.
    ///
    /// 🚨 **Este teste substitui um que prendia o defeito oposto.** Até 17/ago o
    /// WGSL declarava **28** campos para os 46 que a CPU manda, e como o
    /// `uniform` chega por **posição** e não por nome, a partir do 23 o shader
    /// lia o campo do vizinho:
    ///
    /// | posição | o Rust mandava | o shader lia como | o usuário via |
    /// |--------:|----------------|-------------------|---------------|
    /// | 23 | `hsl_red_hue` | `nr_luminance` | a foto **borrava** |
    /// | 24 | `hsl_orange_hue` | `nr_luminance` de novo — declarado duas vezes | nada |
    /// | 25 | `hsl_yellow_hue` | `nr_color` | tirava ruído de cor |
    /// | 26 | `hsl_green_hue` | `sharpen_amount` | afiava |
    /// | 27 | `hsl_aqua_hue` | `sharpen_radius` | nada sozinho |
    /// | 28–45 | matiz (3), luminância (8), lente (3), Detalhe (4) | **nada** | nada |
    ///
    /// Nada disso falhava: o buffer é maior que o mínimo que o binding exige,
    /// então o wgpu aceita e ignora a sobra, e a duplicata de `nr_luminance` no
    /// WGSL o naga também aceita. Não havia erro, log nem tela quebrada — havia
    /// um controle que responde e uma foto que muda pelo motivo errado.
    ///
    /// 🔑 **A conferência é por leitura do arquivo, e não por medida na imagem**,
    /// porque campo declarado e campo aplicado são coisas diferentes: quem mede
    /// a segunda são os testes do [`crate::motor`].
    #[test]
    fn o_wgsl_declara_os_mesmos_campos_na_mesma_ordem() {
        assert_eq!(
            campos_do_wgsl(),
            Ajustes::NOMES,
            "o `struct Params` do WGSL divergiu do `Ajustes` — e o `uniform` casa por posição"
        );
    }

    /// 🔑 O vetor posicional e os nomes contam a mesma história.
    ///
    /// `NOMES[i]` tem de ser o campo que `como_vetor()[i]` carrega — senão o site
    /// escreveria "exposure" e a GPU leria contraste. Conferido nos dois que não
    /// são zero no neutro e em dois do meio da lista.
    #[test]
    fn o_vetor_segue_a_ordem_dos_nomes() {
        let posicao = |nome: &str| {
            Ajustes::NOMES
                .iter()
                .position(|n| *n == nome)
                .unwrap_or_else(|| panic!("`{nome}` não está em NOMES"))
        };
        let neutro = Ajustes::default().como_vetor();
        assert_eq!(neutro[posicao("contrast")], 1.0);
        assert_eq!(neutro[posicao("sharpen_radius")], 1.0);
        // 🚨 O terceiro neutro que não é zero, desde 2026-09-12: a mistura do
        // Color Grading abre em 50, como na Adobe. Em 0 as três faixas teriam
        // borda dura e uma foto intocada já sairia diferente.
        assert_eq!(neutro[posicao("split_blending")], 50.0);
        assert_eq!(neutro.iter().filter(|v| **v != 0.0).count(), 3);

        let com_matiz = Ajustes {
            hsl_green_hue: 33.0,
            lens_distortion: -7.0,
            split_shadow_hue: 210.0,
            grain_amount: 40.0,
            ..Default::default()
        }
        .como_vetor();
        assert_eq!(com_matiz[posicao("hsl_green_hue")], 33.0);
        assert_eq!(com_matiz[posicao("lens_distortion")], -7.0);
        assert_eq!(com_matiz[posicao("split_shadow_hue")], 210.0);
        assert_eq!(com_matiz[posicao("grain_amount")], 40.0);
    }

    /// O vetor volta a ser struct — e só com 46 valores.
    #[test]
    fn de_vetor_exige_exatamente_a_quantidade() {
        let original = Ajustes {
            exposure: 0.5,
            hsl_blue_lum: -20.0,
            ..Default::default()
        };
        assert_eq!(Ajustes::de_vetor(&original.como_vetor()), Some(original));
        assert_eq!(Ajustes::de_vetor(&[0.0; QUANTIDADE - 1]), None);
        assert_eq!(Ajustes::de_vetor(&[0.0; QUANTIDADE + 1]), None);
    }

    /// 🔑 No JSON, campo ausente é o neutro — e `contrast` ausente é `1.0`.
    ///
    /// Um `#[serde(default)]` derivado do zero deixaria a foto cinza chapada
    /// sempre que o site mandasse só a exposição.
    #[test]
    fn o_json_por_nome_completa_com_o_neutro() {
        let lido: Ajustes = serde_json::from_str(r#"{"exposure": 0.5, "hsl_red_sat": 40}"#)
            .expect("JSON parcial é válido");
        assert_eq!(lido.exposure, 0.5);
        assert_eq!(lido.hsl_red_sat, 40.0);
        assert_eq!(
            lido.contrast, 1.0,
            "ausente é o neutro, e o neutro do contraste é 1"
        );
        assert_eq!(lido.sharpen_radius, 1.0);

        let ida_e_volta: Ajustes =
            serde_json::from_str(&serde_json::to_string(&lido).expect("serializa"))
                .expect("desserializa");
        assert_eq!(ida_e_volta, lido);
    }

    /// 🔑 Uma revelação gravada **antes** da Tonalização abre sem tonalização.
    ///
    /// O JSON do site guarda só o que difere do neutro, então toda foto revelada
    /// até 2026-09-06 tem um objeto sem os sete campos novos. Ausente é neutro, e
    /// o neutro dos sete é zero — a foto abre exatamente como foi deixada, e não
    /// com um matiz que ninguém escolheu.
    #[test]
    fn a_revelacao_gravada_antes_da_tonalizacao_abre_sem_ela() {
        let antiga: Ajustes = serde_json::from_str(r#"{"exposure": 0.4, "saturation": -1.0}"#)
            .expect("o JSON de antes continua válido");
        assert_eq!(antiga.split_shadow_sat, 0.0);
        assert_eq!(antiga.split_highlight_sat, 0.0);
        assert_eq!(antiga.split_balance, 0.0);
        assert_eq!(antiga.grain_amount, 0.0);
        assert_eq!(antiga.grain_size, 0.0);
        assert_eq!(antiga.exposure, 0.4);
        assert_eq!(antiga.saturation, -1.0);
    }
}
