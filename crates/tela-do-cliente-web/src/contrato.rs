//! O contrato da tela, **sem GPU e sem canvas**.
//!
//! O que a tela decide e que não precisa de `wgpu` nem de `web-sys` mora aqui:
//! a forma dos vetores que o JavaScript manda (ajustes e corte), as
//! conferências de tamanho com as mensagens de erro, o tempo e a curva do
//! cruzamento, quem fica no ar e quem sai, e o `uniform` de cada camada — o
//! encaixe na janela e as UVs do enquadramento. `tela.rs` chama daqui.
//!
//! 🔑 **Por que separado**: `tela.rs` só compila em `wasm32`, e o que mora lá
//! não roda em `cargo test`. Este módulo compila em todo alvo, e os testes de
//! baixo provam em nativo o que o cliente vê.

use revelacao_core::{Ajustes, Corte};

/// Quanto dura o cruzamento entre uma foto e a seguinte, em milissegundos.
///
/// 🔑 **Aqui não é o mesmo compromisso da galeria.** Meio segundo era o número
/// da versão em CSS, herdado de uma tela em que o operador atravessa a tira com
/// a seta e a transição vira melado. **Esta tela é a do cliente**: quem olha não
/// está navegando, está decidindo se leva a foto — e o que ele viu foi *"tá
/// muito rápido"* (dono, 2026-09-11). Um segundo dá tempo de a foto que entra
/// ser percebida como apresentação, e não como troca de slide.
///
/// ⚠️ Passar muito disto tem um custo que não aparece parado: o operador que
/// anda três fotos seguidas deixa a tela do cliente cruzando o tempo todo, e a
/// foto nunca assenta. A saída não seria encurtar isto — é `Tela::mostrar`
/// descartar a camada do meio, que é o que ele já faz.
pub(crate) const CRUZAMENTO_MS: f64 = 1000.0;

/// De quanto a foto que entra começa maior — o respiro de apresentação.
///
/// 🔑 **Só a que entra se move.** Dois movimentos cruzados dariam a impressão
/// de a foto ter sido empurrada, e o que se quer é que ela **apareça**.
///
/// Cresceu junto com o tempo: num cruzamento de um segundo, 3% é um movimento
/// que quase não se vê — e o movimento é metade do que faz a foto parecer
/// entrar em vez de piscar.
pub(crate) const PASSO_DA_ENTRADA: f32 = 1.06;

/// O `uniform` de uma camada — a mesma struct do WGSL (`shaders/compositor.wgsl`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct CamadaUniforme {
    pub(crate) escala: [f32; 2],
    pub(crate) centro: [f32; 2],
    pub(crate) uv_x: [f32; 2],
    pub(crate) uv_y: [f32; 2],
    pub(crate) uv_off: [f32; 2],
    pub(crate) alfa: f32,
    pub(crate) _reservado: f32,
    /// A linha projetiva da perspectiva guiada, `[w0, w1, w2, 0]`: a UV é
    /// `(uv_off + uv_x·s + uv_y·t) / (w0·s + w1·t + w2)`. Sem perspectiva é
    /// `[0, 0, 1, 0]`, e a divisão por 1 é exata.
    pub(crate) uv_w: [f32; 4],
}

/// A curva do cruzamento: começa devagar, termina devagar.
///
/// 🔑 **`ease-in-out`, e não o `ease-out` de antes.** O `ease-out` sai do zero
/// a toda velocidade — a foto nova "aparece de estalo" e depois demora a
/// assentar, que é exatamente a sensação de rápido demais que o dono relatou,
/// mesmo com o tempo aumentado. Esta é simétrica: os dois primeiros quadros
/// quase não mexem, o meio faz o trabalho, e o fim encosta devagar.
///
/// ⚠️ Não prende `t`: quem chama passa o [`progresso`], que já vem em 0–1.
pub(crate) fn suavizar(t: f32) -> f32 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        let d = -2.0 * t + 2.0;
        1.0 - d * d * d / 2.0
    }
}

/// Quanto do cruzamento já passou, de 0 a 1, desde `inicio` (no relógio do
/// `requestAnimationFrame`). Relógio que volta fica em 0; depois do fim, 1.
pub(crate) fn progresso(agora: f64, inicio: f64) -> f32 {
    ((agora - inicio) / CRUZAMENTO_MS).clamp(0.0, 1.0) as f32
}

/// A foto que entra, no ponto `t` do cruzamento: o alfa sobe pela curva e o
/// zoom desce de [`PASSO_DA_ENTRADA`] até 1.
pub(crate) fn alfa_e_zoom_da_entrada(t: f32) -> (f32, f32) {
    let suave = suavizar(t);
    (suave, PASSO_DA_ENTRADA + (1.0 - PASSO_DA_ENTRADA) * suave)
}

/// A foto que sai, no ponto `t` da saída: **só esmaece**, partindo do alfa
/// `de` que ela tinha — dois movimentos cruzados pareceriam empurrão.
pub(crate) fn alfa_da_saida(de: f32, t: f32) -> f32 {
    de * (1.0 - suavizar(t))
}

/// Qual camada sai quando já há duas e chega uma terceira, dadas como
/// `(saindo, alfa)` na ordem da pilha.
///
/// 🚨 **Sai a menos visível, e não a mais antiga.** Com `remove(0)`, o
/// operador que apertasse a seta duas vezes antes do primeiro quadro
/// descartava a foto que estava **inteira** na tela (ela era a mais antiga) e
/// mantinha uma que nunca chegou a ser desenhada, em alfa zero: a tela saltava
/// de uma para a outra sem cruzamento nenhum. A que ainda tem o que mostrar é
/// a que fica. Sem nenhuma saindo, a primeira.
pub(crate) fn camada_a_descartar(camadas: impl IntoIterator<Item = (bool, f32)>) -> usize {
    camadas
        .into_iter()
        .enumerate()
        .filter(|(_, (saindo, _))| *saindo)
        .min_by(|(_, (_, a)), (_, (_, b))| a.total_cmp(b))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// A camada continua no ar depois do quadro? Quem terminou de sair vai embora
/// — e nunca a última, que é a foto no ar mesmo quando ela chegou sem
/// cruzamento nenhum.
pub(crate) fn fica_no_ar(e_a_ultima: bool, alfa: f32, saindo: bool) -> bool {
    e_a_ultima || alfa > 0.0 || !saindo
}

/// Os ajustes vindos do JavaScript, na ordem do `uniform`.
pub(crate) fn ajustes_de(v: &[f32]) -> Result<Ajustes, String> {
    Ajustes::de_vetor(v).ok_or_else(|| {
        format!(
            "esperava {} ajustes, recebi {}",
            revelacao_core::QUANTIDADE,
            v.len()
        )
    })
}

/// O enquadramento vindo do JavaScript: oito números, ou oito mais os seis da
/// perspectiva guiada — a mesma forma do `revelacao-web`.
pub(crate) fn corte_de(v: &[f32]) -> Result<Corte, String> {
    if v.len() != 8 && v.len() != 14 {
        return Err(format!(
            "esperava 8 ou 14 campos de corte, recebi {}",
            v.len()
        ));
    }
    let corte = Corte::novo(
        v[0],
        v[1],
        v[2],
        v[3],
        v[4] as i32,
        v[5],
        v[6] != 0.0,
        v[7] != 0.0,
    );
    Ok(match v.get(8..14) {
        Some(&[rx, ry, rz, foco, vertical, horizontal]) => {
            corte.com_perspectiva(revelacao_core::perspectiva::Perspectiva::nova(
                [rx, ry, rz],
                foco,
                vertical,
                horizontal,
            ))
        }
        _ => corte,
    })
}

/// `largura × altura × 4` em 64 bits: no wasm32 o `usize` tem 32, e
/// 32768 × 32768 × 4 dava a volta até 0 — um buffer vazio passaria na conta.
fn bytes_rgba(largura: u32, altura: u32) -> u64 {
    largura as u64 * altura as u64 * 4
}

/// A foto que chega do JavaScript, conferida antes de virar camada: tem
/// tamanho, os bytes batem e cabe numa textura.
pub(crate) fn conferir_foto(
    largura: u32,
    altura: u32,
    bytes: usize,
    limite: u32,
) -> Result<(), String> {
    if largura == 0 || altura == 0 {
        return Err("imagem sem tamanho".to_string());
    }
    if bytes as u64 != bytes_rgba(largura, altura) {
        return Err(format!(
            "esperava {} bytes RGBA para {largura}×{altura}, recebi {bytes}",
            bytes_rgba(largura, altura),
        ));
    }
    if largura > limite || altura > limite {
        return Err(format!(
            "{largura}×{altura} passa do limite de textura deste dispositivo ({limite} px)"
        ));
    }
    Ok(())
}

/// A razão entre estes pixels e a foto original, pelo maior lado.
///
/// 🔑 Os módulos locais do darktable medem em pixels da foto original; a tela
/// recebe a cópia de trabalho, como o editor, e precisa da mesma escala para
/// mostrar o mesmo estilo. Sem `lado_original` (ou zero), vale 1.
pub(crate) fn escala_do_original(largura: u32, altura: u32, lado_original: Option<u32>) -> f32 {
    match lado_original {
        Some(lado) if lado > 0 => (largura.max(altura) as f32 / lado as f32).min(1.0),
        _ => 1.0,
    }
}

/// Onde a camada fica na janela, e que pedaço da foto ela mostra.
///
/// 🚨 **O enquadramento é a mesma conta do arquivo** (`Corte::retangulo` sobre
/// `dimensoes_giradas`), e não uma aproximação: é o que impede o operador de
/// enquadrar uma coisa na tela e o cliente ver outra. O que muda entre os dois é
/// só quem interpola — aqui o amostrador, no arquivo a bilinear do core.
pub(crate) fn montar_uniforme(
    largura: u32,
    altura: u32,
    corte: &Corte,
    janela: (f32, f32),
    alfa: f32,
    zoom: f32,
) -> CamadaUniforme {
    // 1. Encaixe: a foto enquadrada cabe inteira na janela, sem cortar nada.
    //
    // 🚨 **O tamanho vem de `dimensoes_de_saida`, e não do retângulo.** Com
    // ângulo os dois divergem — o endireitamento reamostra para a saída pedida,
    // sem o `min` da borda —, e usar o retângulo ali deixava a foto endireitada
    // com a proporção errada na tela do cliente enquanto o arquivo saía certo.
    // É a outra metade do defeito de 2026-09-12; a primeira estava nas UVs.
    let (sw, sh) = corte.dimensoes_de_saida(largura, altura);
    let (jw, jh) = janela;
    let escala = (jw / sw.max(1) as f32).min(jh / sh.max(1) as f32) * zoom;
    let largura_na_tela = sw as f32 * escala;
    let altura_na_tela = sh as f32 * escala;

    // 2. As UVs — a conta pura, testada em `uvs_do_enquadramento` e, com
    //    perspectiva, em `a_tela_do_cliente_amostra_o_que_o_arquivo_tem`.
    let m = revelacao_core::transformacao::mapa_do_enquadramento(largura, altura, corte);

    CamadaUniforme {
        escala: [largura_na_tela / jw, altura_na_tela / jh],
        centro: [0.0, 0.0],
        uv_x: [m[0][0], m[1][0]],
        uv_y: [m[0][1], m[1][1]],
        uv_off: [m[0][2], m[1][2]],
        alfa,
        _reservado: 0.0,
        uv_w: [m[2][0], m[2][1], m[2][2], 0.0],
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    const INTEIRO: [f32; 8] = [0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0];

    fn perto(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    fn perto2(a: [f32; 2], b: [f32; 2]) -> bool {
        perto(a[0], b[0]) && perto(a[1], b[1])
    }

    // --- A curva e o tempo --------------------------------------------------

    #[test]
    fn a_curva_vai_de_zero_a_um() {
        assert_eq!(suavizar(0.0), 0.0);
        assert_eq!(suavizar(1.0), 1.0);
        assert_eq!(suavizar(0.5), 0.5);
    }

    #[test]
    fn a_curva_nunca_volta() {
        let mut anterior = suavizar(0.0);
        for i in 1..=1000 {
            let agora = suavizar(i as f32 / 1000.0);
            assert!(agora >= anterior, "voltou em t = {}", i as f32 / 1000.0);
            anterior = agora;
        }
    }

    #[test]
    fn a_curva_e_simetrica_e_continua_no_meio() {
        for i in 0..=100 {
            let t = i as f32 / 100.0;
            assert!(
                (suavizar(t) + suavizar(1.0 - t) - 1.0).abs() < 1e-5,
                "assimétrica em t = {t}"
            );
        }
        assert!((suavizar(0.499_99) - suavizar(0.5)).abs() < 1e-4);
    }

    #[test]
    fn a_curva_comeca_e_termina_devagar() {
        // `ease-in-out`: o primeiro décimo anda bem menos que um décimo, e o
        // último também.
        assert!(suavizar(0.1) < 0.01);
        assert!(1.0 - suavizar(0.9) < 0.01);
    }

    #[test]
    fn o_progresso_vai_de_zero_a_um_em_um_cruzamento() {
        assert_eq!(progresso(500.0, 500.0), 0.0);
        assert_eq!(progresso(1000.0, 500.0), 0.5);
        assert_eq!(progresso(500.0 + CRUZAMENTO_MS, 500.0), 1.0);
    }

    #[test]
    fn o_progresso_fica_preso_entre_zero_e_um() {
        assert_eq!(progresso(0.0, 500.0), 0.0, "relógio que volta");
        assert_eq!(progresso(1e9, 0.0), 1.0, "muito depois do fim");
    }

    #[test]
    fn a_foto_que_entra_aparece_e_assenta() {
        let (alfa, zoom) = alfa_e_zoom_da_entrada(0.0);
        assert_eq!(alfa, 0.0);
        assert!(perto(zoom, PASSO_DA_ENTRADA));
        let (alfa, zoom) = alfa_e_zoom_da_entrada(1.0);
        assert_eq!(alfa, 1.0);
        assert!(perto(zoom, 1.0));
        // No caminho, o zoom só desce e o alfa só sobe.
        let mut antes = alfa_e_zoom_da_entrada(0.0);
        for i in 1..=100 {
            let agora = alfa_e_zoom_da_entrada(i as f32 / 100.0);
            assert!(agora.0 >= antes.0 && agora.1 <= antes.1);
            antes = agora;
        }
    }

    #[test]
    fn a_foto_que_sai_parte_do_alfa_que_tinha() {
        // 🚨 Trocada no meio do cruzamento, ela parte de 0,3 — e não salta.
        assert!(perto(alfa_da_saida(0.3, 0.0), 0.3));
        assert_eq!(alfa_da_saida(0.3, 1.0), 0.0);
        assert!(alfa_da_saida(0.3, 0.5) < 0.3);
        assert_eq!(alfa_da_saida(1.0, 0.0), 1.0);
    }

    // --- Quem fica e quem sai -----------------------------------------------

    #[test]
    fn sai_a_camada_menos_visivel_e_nao_a_mais_antiga() {
        // A de baixo está inteira e saindo, a do meio nunca foi desenhada.
        assert_eq!(camada_a_descartar([(true, 1.0), (true, 0.0)]), 1);
        assert_eq!(camada_a_descartar([(true, 0.2), (true, 0.7)]), 0);
    }

    #[test]
    fn so_sai_quem_ja_esta_saindo() {
        assert_eq!(camada_a_descartar([(false, 0.0), (true, 0.9)]), 1);
    }

    #[test]
    fn sem_nenhuma_saindo_sai_a_primeira() {
        assert_eq!(camada_a_descartar([(false, 0.5), (false, 0.0)]), 0);
        assert_eq!(camada_a_descartar([]), 0);
    }

    #[test]
    fn no_empate_sai_a_mais_antiga() {
        assert_eq!(camada_a_descartar([(true, 0.4), (true, 0.4)]), 0);
    }

    #[test]
    fn a_ultima_camada_nunca_sai_da_tela() {
        assert!(fica_no_ar(true, 0.0, true));
    }

    #[test]
    fn quem_terminou_de_sair_vai_embora() {
        assert!(!fica_no_ar(false, 0.0, true));
        assert!(fica_no_ar(false, 0.01, true), "ainda esmaecendo");
        assert!(fica_no_ar(false, 0.0, false), "ainda não começou a entrar");
    }

    // --- Os vetores do JavaScript -------------------------------------------

    #[test]
    fn o_corte_inteiro_e_a_identidade() {
        let corte = corte_de(&INTEIRO).unwrap();
        assert_eq!(corte, Corte::inteiro());
        assert!(corte.e_inteiro());
    }

    #[test]
    fn o_corte_le_os_campos_na_ordem_de_corte_novo() {
        let corte = corte_de(&[0.1, 0.2, 0.5, 0.6, 3.0, -2.0, 0.0, 1.0]).unwrap();
        assert_eq!(corte, Corte::novo(0.1, 0.2, 0.5, 0.6, 3, -2.0, false, true));
    }

    #[test]
    fn o_corte_de_quatorze_campos_traz_a_perspectiva() {
        let mut v = INTEIRO.to_vec();
        v.extend_from_slice(&[5.0, 0.0, 0.0, 30.0, 0.0, 0.0]);
        assert!(corte_de(&v).unwrap().tem_perspectiva());
    }

    #[test]
    fn o_corte_de_outro_tamanho_e_erro_com_a_mensagem_do_site() {
        for n in [0, 7, 9, 13, 15] {
            assert_eq!(
                corte_de(&vec![0.0; n]).unwrap_err(),
                format!("esperava 8 ou 14 campos de corte, recebi {n}")
            );
        }
    }

    #[test]
    fn os_ajustes_neutros_passam() {
        let v = Ajustes::default().como_vetor();
        assert_eq!(ajustes_de(&v).unwrap().como_vetor(), v);
    }

    #[test]
    fn os_ajustes_de_outro_tamanho_sao_erro_com_a_mensagem_do_site() {
        let n = revelacao_core::QUANTIDADE;
        for tamanho in [0, n - 1, n + 1] {
            assert_eq!(
                ajustes_de(&vec![0.0; tamanho]).err(),
                Some(format!("esperava {n} ajustes, recebi {tamanho}"))
            );
        }
    }

    // --- As conferências e a escala -----------------------------------------

    #[test]
    fn a_foto_sem_tamanho_e_erro() {
        assert_eq!(
            conferir_foto(0, 3, 0, 8192).unwrap_err(),
            "imagem sem tamanho"
        );
        assert_eq!(
            conferir_foto(3, 0, 0, 8192).unwrap_err(),
            "imagem sem tamanho"
        );
    }

    #[test]
    fn a_foto_com_bytes_a_menos_e_erro() {
        assert_eq!(
            conferir_foto(3, 2, 23, 8192).unwrap_err(),
            "esperava 24 bytes RGBA para 3×2, recebi 23"
        );
    }

    #[test]
    fn a_foto_maior_que_a_textura_e_erro() {
        assert_eq!(
            conferir_foto(10, 4097, 10 * 4097 * 4, 4096).unwrap_err(),
            "10×4097 passa do limite de textura deste dispositivo (4096 px)"
        );
        assert!(conferir_foto(4096, 4096, 4096 * 4096 * 4, 4096).is_ok());
    }

    #[test]
    fn a_escala_do_original_e_a_razao_dos_lados_maiores_ate_um() {
        assert_eq!(escala_do_original(2048, 1365, None), 1.0);
        assert_eq!(escala_do_original(2048, 1365, Some(0)), 1.0);
        assert_eq!(escala_do_original(1024, 2048, Some(4096)), 0.5);
        assert_eq!(escala_do_original(4096, 2048, Some(2048)), 1.0);
    }

    // --- O uniform ----------------------------------------------------------

    #[test]
    fn o_uniforme_tem_o_leiaute_do_wgsl() {
        // vec2 ×5 + f32 ×2 = 48 bytes, e o vec4 alinhado em 16 logo depois.
        assert_eq!(std::mem::size_of::<CamadaUniforme>(), 64);
        assert_eq!(std::mem::offset_of!(CamadaUniforme, alfa), 40);
        assert_eq!(std::mem::offset_of!(CamadaUniforme, uv_w), 48);
    }

    #[test]
    fn o_corte_inteiro_mostra_a_foto_toda_sem_mexer_nas_uvs() {
        let u = montar_uniforme(3000, 2000, &Corte::inteiro(), (1500.0, 1000.0), 0.7, 1.0);
        assert_eq!(
            u,
            CamadaUniforme {
                escala: [1.0, 1.0],
                centro: [0.0, 0.0],
                uv_x: [1.0, 0.0],
                uv_y: [0.0, 1.0],
                uv_off: [0.0, 0.0],
                alfa: 0.7,
                _reservado: 0.0,
                uv_w: [0.0, 0.0, 1.0, 0.0],
            }
        );
    }

    #[test]
    fn a_foto_deitada_encaixa_pela_altura_numa_janela_mais_larga() {
        let u = montar_uniforme(3000, 2000, &Corte::inteiro(), (1920.0, 1080.0), 1.0, 1.0);
        // 2000 → 1080 é 0,54; 3000 × 0,54 = 1620 de 1920.
        assert!(perto2(u.escala, [1620.0 / 1920.0, 1.0]), "{:?}", u.escala);
    }

    #[test]
    fn a_foto_em_retrato_encaixa_pela_altura_e_sobra_dos_lados() {
        let u = montar_uniforme(2000, 3000, &Corte::inteiro(), (1920.0, 1080.0), 1.0, 1.0);
        assert!(perto2(u.escala, [720.0 / 1920.0, 1.0]), "{:?}", u.escala);
    }

    #[test]
    fn a_foto_panoramica_encaixa_pela_largura() {
        let u = montar_uniforme(4000, 1000, &Corte::inteiro(), (1000.0, 1000.0), 1.0, 1.0);
        assert!(perto2(u.escala, [1.0, 0.25]), "{:?}", u.escala);
    }

    #[test]
    fn o_zoom_da_entrada_aumenta_os_dois_lados_por_igual() {
        let parada = montar_uniforme(3000, 2000, &Corte::inteiro(), (1920.0, 1080.0), 1.0, 1.0);
        let entrando = montar_uniforme(
            3000,
            2000,
            &Corte::inteiro(),
            (1920.0, 1080.0),
            0.0,
            PASSO_DA_ENTRADA,
        );
        assert!(perto(
            entrando.escala[0],
            parada.escala[0] * PASSO_DA_ENTRADA
        ));
        assert!(perto(
            entrando.escala[1],
            parada.escala[1] * PASSO_DA_ENTRADA
        ));
        assert_eq!(
            entrando.uv_x, parada.uv_x,
            "o zoom não mexe no enquadramento"
        );
    }

    #[test]
    fn o_giro_de_um_quarto_encaixa_a_foto_virada() {
        let corte = corte_de(&[0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0]).unwrap();
        // 3000×2000 virada é 2000×3000: na janela quadrada, sobra dos lados.
        let u = montar_uniforme(3000, 2000, &corte, (1000.0, 1000.0), 1.0, 1.0);
        assert!(perto2(u.escala, [2.0 / 3.0, 1.0]), "{:?}", u.escala);
        assert_ne!(u.uv_x, [1.0, 0.0], "virada, a UV não é mais a identidade");
    }

    #[test]
    fn o_recorte_vira_uv_do_pedaco_e_encaixa_o_tamanho_do_arquivo() {
        // A metade da direita, inteira na altura: um arquivo de 1500×2000.
        let corte = corte_de(&[0.5, 0.0, 0.5, 1.0, 0.0, 0.0, 0.0, 0.0]).unwrap();
        let u = montar_uniforme(3000, 2000, &corte, (1000.0, 1000.0), 1.0, 1.0);
        assert!(perto2(u.uv_x, [0.5, 0.0]), "{:?}", u.uv_x);
        assert!(perto2(u.uv_y, [0.0, 1.0]), "{:?}", u.uv_y);
        assert!(perto2(u.uv_off, [0.5, 0.0]), "{:?}", u.uv_off);
        assert!(perto2(u.escala, [0.75, 1.0]), "{:?}", u.escala);
    }

    #[test]
    fn com_angulo_o_encaixe_usa_o_tamanho_do_arquivo() {
        let corte = corte_de(&[0.2, 0.2, 0.6, 0.6, 0.0, 10.0, 0.0, 0.0]).unwrap();
        let (sw, sh) = corte.dimensoes_de_saida(1000, 500);
        let u = montar_uniforme(1000, 500, &corte, (1000.0, 1000.0), 1.0, 1.0);
        let escala = (1000.0 / sw as f32).min(1000.0 / sh as f32);
        assert!(perto2(
            u.escala,
            [sw as f32 * escala / 1000.0, sh as f32 * escala / 1000.0]
        ));
    }

    #[test]
    fn sem_perspectiva_a_linha_projetiva_e_neutra_e_com_ela_nao() {
        let sem = montar_uniforme(3000, 2000, &Corte::inteiro(), (100.0, 100.0), 1.0, 1.0);
        assert_eq!(sem.uv_w, [0.0, 0.0, 1.0, 0.0]);

        let mut v = INTEIRO.to_vec();
        v.extend_from_slice(&[8.0, 0.0, 0.0, 30.0, 0.0, 0.0]);
        let com = montar_uniforme(3000, 2000, &corte_de(&v).unwrap(), (100.0, 100.0), 1.0, 1.0);
        assert!(perto(com.uv_w[2], 1.0), "normalizada: {:?}", com.uv_w);
        assert_eq!(com.uv_w[3], 0.0);
        assert!(
            com.uv_w[0].abs() > 1e-6 || com.uv_w[1].abs() > 1e-6,
            "{:?}",
            com.uv_w
        );
    }

    #[test]
    fn as_uvs_sao_as_da_conta_do_core() {
        let corte = corte_de(&[0.1, 0.05, 0.7, 0.8, 3.0, -4.0, 1.0, 0.0]).unwrap();
        let m = revelacao_core::transformacao::mapa_do_enquadramento(4000, 3000, &corte);
        let u = montar_uniforme(4000, 3000, &corte, (800.0, 600.0), 1.0, 1.0);
        assert_eq!(u.uv_x, [m[0][0], m[1][0]]);
        assert_eq!(u.uv_y, [m[0][1], m[1][1]]);
        assert_eq!(u.uv_off, [m[0][2], m[1][2]]);
        assert_eq!(u.uv_w, [m[2][0], m[2][1], m[2][2], 0.0]);
    }

    #[test]
    fn a_conta_dos_bytes_nao_da_a_volta_em_32_bits() {
        // 2³² bytes: no wasm32 a conta em usize virava 0 e o buffer vazio passava.
        assert!(conferir_foto(32768, 32768, 0, u32::MAX).is_err());
        assert_eq!(bytes_rgba(32768, 32768), 1 << 32);
    }
}
