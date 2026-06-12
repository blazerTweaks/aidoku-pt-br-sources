# Aidoku PT-BR Sources

Fontes em português brasileiro para o [Aidoku](https://aidoku.app/), compiladas para WebAssembly.

## Sources

| ID | Nome | Idioma | Status |
|----|------|--------|--------|
| `pt-br.mangaflix` | MangaFlix | pt-BR | Browse, detalhes, capítulos, páginas |

## Como usar

Adicione a source list no Aidoku:

```
https://raw.githubusercontent.com/blazerTweaks/aidoku-pt-br-sources/main/source.json
```

## Desenvolvimento

### Pré-requisitos

- Rust com target `wasm32-unknown-unknown`
- [aidoku-cli](https://github.com/Aidoku/aidoku-rs)

### Build

```bash
cd sources/pt-br.mangaflix
cargo build --target wasm32-unknown-unknown
aidoku package
```

O arquivo `.aix` gerado fica em `sources/pt-br.mangaflix/package.aix`.
