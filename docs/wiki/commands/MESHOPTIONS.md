---
summary: Sets how solids, surfaces and regions are tessellated when they are converted to meshes.
related: MESHSMOOTH, MESHPRIMITIVEOPTIONS
---

## Usage
1. Choose an option to change it, then press Enter to apply.

## Options
- **Mesh type**: 0 optimized (flat areas become single faces), 1 mostly quads, 2 triangles ([`FACETERMESHTYPE`](FACETERMESHTYPE-System-Variable)).
- **Max distance**: how far the mesh may stand off the original surface ([`FACETERDEVSURFACE`](FACETERDEVSURFACE-System-Variable)).
- **Max angle**: the largest angle between neighbouring new faces ([`FACETERDEVNORMAL`](FACETERDEVNORMAL-System-Variable)).
- **Smoothness**: the smoothness level the converted mesh gets ([`FACETERSMOOTHLEV`](FACETERSMOOTHLEV-System-Variable)).
