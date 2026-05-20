# CHUM Manifest Specification

**Status:** v0.1 spec to be drafted in a subsequent session.

## Intent

A CHUM manifest is a TOML file (one per MCP server) describing how to install, configure, run, and govern that server. Manifests live under [`/manifests/`](../manifests/) in this repo for first-party packages, and on the public registry for third-party packages.

The v0.1 spec will cover:

- **Metadata** — name, version, author, description, license, source repo.
- **Install method** — `npm`, `pip`, prebuilt binary, or source build.
- **Runtime** — transport (`stdio` or `http+sse`), ports, env contract.
- **Capabilities** — tools exposed, resources accessed, secrets required.
- **Versioning** — pinning, update policy, dependency edges.
- **Signing fields** — placeholder fields in v0.1 for forward compatibility; full signing semantics arrive in v0.3.

## Current state

No manifest types or parsers exist yet in `chum-core`. This document is filled in alongside the first feature commit that introduces them.

## File naming

`<name>.toml` (unprefixed). The `/manifests/` directory acts as its own namespace.
