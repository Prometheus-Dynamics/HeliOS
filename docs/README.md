# Website

This website is built using [Docusaurus](https://docusaurus.io/), a modern static website generator.

## Installation

```bash
bun install
```

## Local Development

```bash
bun run start
```

This command starts a local development server and opens up a browser window. Most changes are reflected live without having to restart the server.

## Build

```bash
bun run build
```

This command generates static content into the `build` directory and can be served using any static contents hosting service.

## Deployment

This repo deploys the docs with GitHub Actions via
`.github/workflows/docusaurus-pages.yml`.

### GitHub Pages

1. In the GitHub repo, open `Settings -> Pages`.
2. Set the source to `GitHub Actions`.
3. Push to `main`.

The workflow publishes automatically when these inputs change:

- `docs/**`
- `.github/workflows/docusaurus-pages.yml`

Without a custom domain, the published site URL is:

```text
https://<owner>.github.io/<repo>/
```

### Custom Domain

Yes. GitHub Pages can serve the docs from a custom domain.

1. Create a repository variable named `DOCS_CUSTOM_DOMAIN`.
2. Set its value to the hostname you want to serve the docs from, for example `docs.example.com`.
3. In `Settings -> Pages`, configure the same custom domain.
4. Point that hostname's DNS at GitHub Pages.

When `DOCS_CUSTOM_DOMAIN` is set, the workflow builds the site for:

```text
https://<your-domain>/
```

and switches Docusaurus `baseUrl` to `/`.

### Manual Docusaurus Deploy

`bun run build` regenerates the API reference pages when the local generated
spec snapshots exist under `frontend/src/lib/ts-bindings/**`. In clean CI
checkouts those ignored files are absent, so the build reuses the checked-in
generated API docs pages instead of failing.

Using SSH:

```bash
USE_SSH=true bun run deploy
```

Not using SSH:

```bash
GIT_USER=<Your GitHub username> bun run deploy
```

If you are using GitHub pages for hosting, this command is a convenient way to build the website and push to the `gh-pages` branch.
