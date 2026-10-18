# Os comandos do nucleo-da-foto. `make` sem argumento lista.
.DEFAULT_GOAL := ajuda
.PHONY: ajuda verificar fmt clippy testar wasm doc deny cobertura semver msrv

WEB := -p conversao-web -p fotolivro-web -p raw-web -p revelacao-web -p tela-do-cliente-web
SEM_WEB := --exclude conversao-web --exclude fotolivro-web --exclude raw-web --exclude revelacao-web --exclude tela-do-cliente-web
MSRV := $(shell sed -n 's/^rust-version *= *"\(.*\)"/\1/p' Cargo.toml)
ULTIMA_TAG := $(shell git describe --tags --abbrev=0 2>/dev/null)

ajuda: ## Lista os alvos
	@grep -hE '^[a-z-]+:.*?## ' $(MAKEFILE_LIST) | awk -F':.*?## ' '{printf "  \033[36m%-12s\033[0m %s\n", $$1, $$2}'

verificar: fmt clippy testar wasm doc deny ## Tudo o que tem de passar antes de lançar

fmt: ## Formatação (rustfmt), sem mudar nada
	cargo fmt --all -- --check

clippy: ## Clippy com -D warnings, nativo
	cargo clippy --workspace --all-targets -- -D warnings

testar: ## Os testes (unidade, integração e doc-tests)
	cargo test --workspace

wasm: ## Os wrappers *-web no alvo wasm32, com clippy -D warnings
	cargo clippy --target wasm32-unknown-unknown $(WEB) -- -D warnings

doc: ## A documentação sem nenhum aviso (link quebrado, item privado citado)
	RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

deny: ## Licenças, vulnerabilidades conhecidas e fontes das dependências (cargo-deny)
	cargo deny check

cobertura: ## Cobertura por crate (cargo-llvm-cov); HTML em target/cobertura
	cargo llvm-cov --workspace $(SEM_WEB) --html --output-dir target/cobertura
	cargo llvm-cov report --summary-only

semver: ## A API pública comparada com a última tag (cargo-semver-checks)
	cargo semver-checks --workspace --baseline-rev $(ULTIMA_TAG)

msrv: ## Compila com o Rust mínimo declarado (rust-version)
	rustup toolchain install $(MSRV) --profile minimal
	cargo +$(MSRV) check --workspace --all-targets
