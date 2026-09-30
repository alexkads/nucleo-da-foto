#!/usr/bin/env bash
# Compila o leitor de RAW para a importação do SITE e entrega ao frontend.
#
# 🔑 **Por que ele é separado do motor de revelação**: o decodificador de RAW
# (rawler e as tabelas de ~1.000 câmeras) pesa mais que o motor inteiro, e só
# quem importa RAW precisa dele. A importação o carrega quando a leva tem um
# `.dng`, `.nef`, `.cr2`…
#
# Uso: scripts/construir-raw.sh [--sem-pilha] [caminho do frontend do e-commerce]
#
# O que sai, e para onde:
#
#   frontend/public/raw/raw_web.js        o glue do wasm-bindgen (--target web)
#   frontend/public/raw/raw_web_bg.wasm   o leitor de RAW
#   frontend/public/raw/VERSAO            o commit e o hash do conteúdo
#   frontend/src/.../importacao/raw/raw_web.d.ts   os tipos do glue
#   frontend/src/.../importacao/raw/versao.ts      a mesma VERSAO, importável
#
# Os artefatos são commitados no repositório do site: a Vercel não tem Rust.
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
PUBLICO="$FRONTEND/public/raw"
# Os tipos e a VERSAO vão para onde a importação os importa.
FONTE="$FRONTEND/src/app/(dashboard)/dashboard/sessoes-fotograficas/importacao/raw"
SAIDA="target/raw-web"

for ferramenta in wasm-pack wasm-opt; do
    command -v "$ferramenta" >/dev/null || {
        echo "falta $ferramenta — brew install wasm-pack binaryen" >&2
        exit 1
    }
done
rustup target add wasm32-unknown-unknown >/dev/null

echo "→ wasm-pack build (perfil release-web)"
# `--profile` é o do cargo (`[profile.release-web]` no Cargo.toml do workspace);
# não se passa `--release` junto, o wasm-pack recusa os dois.
RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+simd128" \
wasm-pack build crates/raw-web \
    --target web \
    --profile release-web \
    --out-dir "../../$SAIDA" \
    --out-name raw_web

echo "→ wasm-opt -O3"
mkdir -p "$PUBLICO" "$FONTE"
wasm-opt -O3 --enable-simd --enable-bulk-memory --enable-nontrapping-float-to-int \
    -o "$PUBLICO/raw_web_bg.wasm" "$SAIDA/raw_web_bg.wasm"
cp "$SAIDA/raw_web.js" "$PUBLICO/raw_web.js"
cp "$SAIDA/raw_web.d.ts" "$FONTE/raw_web.d.ts"

echo "→ VERSAO"
VERSAO="$(git rev-parse --short HEAD)"
# 🚨 **O perfil e este script entram na conta do "sujo".** Antes só os dois
# crates contavam, e isso bastava enquanto a compilação era sempre a mesma. Não
# é mais: o `opt-level` do `[profile.release-web]` e o `+simd128` daqui mudam o
# `.wasm` **sem mudar uma linha de Rust** — e o site agora serve o motor com
# `Cache-Control: immutable`, onde uma VERSAO que não muda é um wasm velho que
# nunca mais se desfaz no navegador de quem já o baixou.
if [[ -n "$(git status --porcelain crates/raw-codec crates/raw-web vendor/rawler Cargo.toml "$0")" ]]; then
    VERSAO="$VERSAO-sujo"
fi
# 🚨 **O conteúdo entra na VERSAO, e não só o commit** (2026-09-15). Recompilar
# sobre a mesma árvore suja dava o mesmo `?v=…-sujo` para um `.wasm` diferente —
# e com `Cache-Control: immutable` e o service worker servindo cache-first, a
# URL igual devolvia o motor velho. Só "Desviar para rede" no DevTools passava.
# Com o hash, arquivo novo é URL nova, sempre — que é o que `immutable` promete.
# Um pedaço por arquivo, glue primeiro e .wasm depois: o service worker do site
# confere cada um pelo seu antes de guardar, e `versao-dos-motores.test.ts` do
# e-commerce recusa a build em que VERSAO e arquivos não batem.
VERSAO="$VERSAO-$(shasum -a 256 < "$PUBLICO/raw_web.js" | cut -c1-8)$(shasum -a 256 < "$PUBLICO/raw_web_bg.wasm" | cut -c1-8)"
echo "$VERSAO" > "$PUBLICO/VERSAO"
cat > "$FONTE/versao.ts" <<TS
// Gerado por scripts/construir-raw.sh do VintageLightbox-Rust — não editar.
// É o commit e o hash do conteúdo do leitor que está em public/raw/, e vai na URL dos dois
// arquivos (\`?v=\`) para o glue e o .wasm nunca descasarem no cache.
export const VERSAO = "$VERSAO";
TS

echo
ls -la "$PUBLICO"
echo
echo "leitor de RAW $VERSAO entregue em $PUBLICO"

SEM_PILHA=$SEM_PILHA atualizar_a_pilha_local
