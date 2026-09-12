#!/usr/bin/env bash
# Compila a tela do cliente do balcão para o navegador e entrega ao site.
#
# Uso: scripts/construir-tela-do-cliente.sh [caminho do frontend do e-commerce]
#      (padrão: $RECORDARFOTOS_FRONTEND, senão ../recordarfotos-e-commerce/frontend)
#
# O que sai, e para onde:
#
#   frontend/public/tela-do-cliente/tela_do_cliente_web.js        o glue do wasm-bindgen (--target web)
#   frontend/public/tela-do-cliente/tela_do_cliente_web_bg.wasm   a tela em wgpu + o revelacao-core
#   frontend/public/tela-do-cliente/VERSAO                o commit deste repositório
#   frontend/src/app/tela-do-cliente/tela_do_cliente_web.d.ts   os tipos do glue
#   frontend/src/app/tela-do-cliente/versao.ts          a mesma VERSAO, importável
#
# 🔑 É o mesmo molde do `construir-biblioteca.sh` e do `construir-web.sh`, e
# pelas mesmas razões: `--target web` dispensa configurar o bundler do Next, e
# os artefatos são commitados no repositório do site porque a Vercel não tem
# Rust.
#
# ⚠️ **O `?v=` não é enfeite**: um glue novo com um `.wasm` velho no cache chama
# exportações que não existem. As duas URLs carregam a versão.
# A pilha local é atualizada no fim deste script; ver `atualizar_a_pilha_local`.
set -euo pipefail

SEM_PILHA=0
if [[ "${1:-}" == "--sem-pilha" ]]; then SEM_PILHA=1; shift; fi

cd "$(dirname "$0")/.."

# 🚨 **A pilha local é atualizada por este script, e não por um aviso.**
#
# O `versao.ts` gerado aqui é lido por `import`, e no Docker do macOS o
# observador do Turbopack **não recebe** o evento de escrita do bind mount: o
# contêiner continua servindo o `?v=` antigo, o navegador continua pedindo o
# wasm de antes, e o Service Worker serve aquela URL de cache — para sempre.
#
# Isso custou a manhã de 2026-09-12. O dono testou a mesma correção seis vezes
# contra um motor de `31d14a5`, três commits atrás, e me avisou quatro vezes que
# a rotação estava invertida — que era exatamente o que aquele binário fazia. Só
# o selo de desenvolvimento, quando ficou legível, mostrou qual motor rodava.
#
# Por isso o script derruba o cache e reinicia o `web` sozinho. Passe
# `--sem-pilha` para não mexer em contêiner nenhum.
atualizar_a_pilha_local() {
    [[ "${SEM_PILHA:-0}" == "1" ]] && return 0
    local composicao="$FRONTEND/../docker-compose.dev.yml"
    [[ -f "$composicao" ]] || return 0
    command -v docker >/dev/null || return 0
    docker compose -f "$composicao" ps --services --filter status=running 2>/dev/null |
        grep -qx web || return 0

    echo "→ pilha local no ar: limpando o cache do Turbopack e reiniciando o web"
    docker compose -f "$composicao" exec -T web \
        sh -c 'rm -rf /app/.next/dev /app/.next/cache' >/dev/null 2>&1 || true
    docker compose -f "$composicao" restart web >/dev/null 2>&1 || true
    echo "   ⚠️  recarregue as janelas abertas com Cmd+Shift+R: o módulo da"
    echo "       versão fica na memória da página, e o HMR não o troca."
}


FRONTEND="${1:-${RECORDARFOTOS_FRONTEND:-../recordarfotos-e-commerce/frontend}}"
if [[ ! -d "$FRONTEND/public" ]]; then
    echo "frontend não encontrado em $FRONTEND (passe o caminho ou RECORDARFOTOS_FRONTEND)" >&2
    exit 1
fi
PUBLICO="$FRONTEND/public/tela-do-cliente"
FONTE="$FRONTEND/src/app/tela-do-cliente"
SAIDA="target/tela-do-cliente-web"

for ferramenta in wasm-pack wasm-opt; do
    command -v "$ferramenta" >/dev/null || {
        echo "falta $ferramenta — brew install wasm-pack binaryen" >&2
        exit 1
    }
done
rustup target add wasm32-unknown-unknown >/dev/null

echo "→ testes do core da revelação (é de onde a foto sai)"
cargo test -q -p revelacao-core --lib

echo "→ wasm-pack build (perfil release-web)"
wasm-pack build crates/tela-do-cliente-web \
    --target web \
    --profile release-web \
    --out-dir "../../$SAIDA" \
    --out-name tela_do_cliente_web

echo "→ wasm-opt -Oz"
mkdir -p "$PUBLICO" "$FONTE"
wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int \
    -o "$PUBLICO/tela_do_cliente_web_bg.wasm" "$SAIDA/tela_do_cliente_web_bg.wasm"
cp "$SAIDA/tela_do_cliente_web.js" "$PUBLICO/tela_do_cliente_web.js"
cp "$SAIDA/tela_do_cliente_web.d.ts" "$FONTE/tela_do_cliente_web.d.ts"

echo "→ VERSAO"
VERSAO="$(git rev-parse --short HEAD)"
if [[ -n "$(git status --porcelain crates/tela-do-cliente-web crates/revelacao-core)" ]]; then
    VERSAO="$VERSAO-sujo"
fi
echo "$VERSAO" > "$PUBLICO/VERSAO"
cat > "$FONTE/versao.ts" <<TS
// Gerado por scripts/construir-tela-do-cliente.sh do VintageLightbox-Rust — não editar.
// É o commit da tela do cliente que está em public/tela-do-cliente/, e vai na URL dos
// dois arquivos (\`?v=\`) para o glue e o .wasm nunca descasarem no cache.
export const VERSAO = "$VERSAO";
TS

echo
ls -la "$PUBLICO"
echo
echo "tela do cliente $VERSAO entregue em $PUBLICO"

SEM_PILHA=$SEM_PILHA atualizar_a_pilha_local
