# Hardened configuration

Strata's defaults already isolate the riskiest work: previews and thumbnails are
parsed inside a [bubblewrap sandbox](preview-sandbox.md) with no network, no
session bus, and resource limits. This page lists the further choices
available to users who prefer a smaller attack surface over convenience or
hardware acceleration. None of them are required.

## Installation

- If you can, authenticate GitHub CLI before running the
  [installer](../README.md#interactive-installation). The installer then also
  verifies the release archive's provenance instead of relying on its checksum
  alone. A [manual installation](../README.md#manual-installation) can run the
  same `gh attestation verify` check before extracting the archive.
- Package-managed installations are updated only by the system package manager.

## Previews and thumbnails

The preview sandbox cannot be turned off from Settings. If bubblewrap is missing
or fails, previews are unavailable rather than falling back to unsandboxed
parsing.

| Setting | Location | Hardened value |
|---|---|---|
| **Hardware-accelerated video previews** | Settings → General | Off |
| **Show thumbnails** | Settings → Appearance | Off, for untrusted folders |
| **Single-click file previews** | Settings → General | Off, for untrusted folders |

With hardware acceleration off, media previews decode in software and the
sandbox receives no GPU render nodes, NVIDIA device files, or `/sys` mount.
Hardware decoding exposes the graphics driver to file contents, which is a larger
attack surface than software decoding.

Thumbnails and single-click previews parse files as soon as they are shown or
selected. Turning them off means an untrusted file is parsed only when you
explicitly open its preview. This matters most when browsing downloads, removable
media, or network shares from unknown sources.

Leave the `STRATA_SANDBOX_*` environment variables unset, at build time and at
launch, unless your distribution requires them. They change which runtime tree
and helpers the sandbox binds, so only point them at trusted, read-only system
paths. See [Preview sandbox](preview-sandbox.md#packaging-non-fhs-runtimes).

## Custom actions

[Custom actions](custom-actions.md) are ordinary programs running with your
permissions and are not sandboxed. Only enable or import actions whose scripts
you have read, and run actions that modify files on copies first.

## Remote locations

Strata connects to a remote location only when you open it. Remote image and
video previews are copied to a private, size-limited temporary file and parsed by
the same sandbox as local files. To keep network browsing out of the way, turn
off **Show Network in sidebar** under **Items shown in sidebar** in
Settings → General. Direct URIs remain available through **Ctrl+L**.
