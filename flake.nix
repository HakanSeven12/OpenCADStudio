{
  description = "Open CAD Studio - 2D drafting and 3D modeling in Rust";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    # rust-overlay ships upstream Rust binaries rather than building them, so
    # following nixpkgs costs it no binary cache.
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, rust-overlay }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = f:
        nixpkgs.lib.genAttrs systems (system: f (import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        }));
    in
    {
      packages = forAllSystems (pkgs:
        let
          inherit (pkgs) lib stdenv;

          toolchain = pkgs.rust-bin.stable.latest.default.override {
            targets = [ "wasm32-unknown-unknown" ];
          };

          # Build with the overlay's toolchain rather than the nixpkgs default,
          # so the package and the devShell compile with the same rustc.
          rustPlatform = pkgs.makeRustPlatform {
            cargo = toolchain;
            rustc = toolchain;
          };

          # winit/wgpu dlopen these at run time, so they must also be on the
          # wrapped binary's LD_LIBRARY_PATH, not just at link time.
          runtimeLibs = with pkgs; [
            libGL
            libxkbcommon
            vulkan-loader
            wayland
            xorg.libX11
            xorg.libXcursor
            xorg.libXi
            xorg.libXrandr
          ];
        in
        rec {
          default = opencadstudio;

          opencadstudio = rustPlatform.buildRustPackage {
            pname = "opencadstudio";
            version = (lib.importTOML ./Cargo.toml).package.version;
            src = self;

            cargoLock = {
              lockFile = ./Cargo.lock;
              # One entry per distinct git source in Cargo.lock, keyed by any
              # one package from it. Two of these are transitive and easy to
              # miss by reading Cargo.toml alone: cryoglyph and dpi arrive
              # through iced, which pins its own forks. Regenerate the list
              # from the lock rather than the manifest:
              #   grep -B2 'source = "git+' Cargo.lock
              # Each hash must be refreshed whenever the matching rev moves.
              outputHashes = {
                "acadrust-0.5.5" = "sha256-rYtJaJsb+lZZIDBtPTCfWRVyA1Gfinn3UqJlfzkhhGk=";
                "cadkernel-0.1.0" = "sha256-7smcL9oo5mJwoCLYUbD4+lA9Xcm+/qpVswYxzoGjTDk=";
                "cryoglyph-0.1.0" = "sha256-5BOJNDcjhpt17/XSri3JGtX8NwC7db18bddp5wanMes=";
                "dpi-0.1.1" = "sha256-pQn1lCFSJMkjUfHoggEzMHnm5k+Chnzi5JEDjahnjUA=";
                "iced-0.15.0-dev" = "sha256-Atg+nr1VgXwW5I7LiMAMqHemz/9ZiehTc2EWuSFEGHI=";
                "iced_aw-0.14.1" = "sha256-OynSIjv8SgSoxpw1S4wH58vDdqga/YojOR5A9ZLKrd8=";
              };
            };

            nativeBuildInputs = with pkgs; [
              pkg-config
              makeWrapper
            ];

            buildInputs = with pkgs; [
              fontconfig
              freetype
            ] ++ runtimeLibs
            ++ lib.optionals stdenv.hostPlatform.isDarwin [ darwin.apple_sdk.frameworks.AppKit ];

            # build.rs asks git for a revision and falls back to "unknown"
            # when the repo or the binary is absent, which it is in the
            # sandbox. Nothing to do but let it take the fallback.
            doCheck = false;

            postInstall = lib.optionalString stdenv.hostPlatform.isLinux ''
              wrapProgram $out/bin/OpenCADStudio \
                --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath runtimeLibs}
            '';

            meta = {
              description = "Open-source 2D drafting and 3D modeling, built with Rust";
              homepage = "https://www.opencadstudio.com";
              license = lib.licenses.gpl3Only;
              mainProgram = "OpenCADStudio";
              platforms = systems;
            };
          };
        });

      devShells = forAllSystems (pkgs:
        let
          toolchain = pkgs.rust-bin.stable.latest.default.override {
            extensions = [ "rust-src" "rust-analyzer" "clippy" "rustfmt" ];
            targets = [ "wasm32-unknown-unknown" ];
          };
          runtimeLibs = with pkgs; [
            libGL
            libxkbcommon
            vulkan-loader
            wayland
            xorg.libX11
            xorg.libXcursor
            xorg.libXi
            xorg.libXrandr
          ];
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              toolchain
              pkg-config
              fontconfig
              freetype
              git
              # Web build: `trunk build` per Trunk.toml. wasm-bindgen-cli must
              # match the wasm-bindgen version Cargo.lock resolves, the same
              # constraint .github/workflows/pages.yml:83 reads out of the lock.
              trunk
              wasm-bindgen-cli
              # Rasterises assets/logo.svg for packaging, as release.yml does.
              librsvg
            ] ++ runtimeLibs;

            # A dev binary is run straight out of ./target, so it never gets
            # the wrapper the package build applies.
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;
          };
        });

      checks = forAllSystems (pkgs: {
        default = self.packages.${pkgs.stdenv.hostPlatform.system}.opencadstudio;
      });

      formatter = forAllSystems (pkgs: pkgs.nixpkgs-fmt);
    };
}
