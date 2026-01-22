{
  description = "Handy - Speech-to-text transcription app";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = {
    self,
    nixpkgs,
  }: let
    supportedSystems = ["x86_64-linux"];
    forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
    # Read version from Cargo.toml
    cargoToml = builtins.fromTOML (builtins.readFile ./src-tauri/Cargo.toml);
    version = cargoToml.package.version;
  in {
    packages = forAllSystems (system: let
      pkgs = nixpkgs.legacyPackages.${system};
    in {
      # AppImage-based package
      handy-appimage = let
        appimage = pkgs.appimageTools.wrapType2 {
          pname = "handy-appimage-unwrapped";
          inherit version;
          src = pkgs.fetchurl {
            url = "https://github.com/cjpais/Handy/releases/download/v${version}/Handy_${version}_amd64.AppImage";
            hash = "sha256-+uS2xU1cf50b/zGKIX2fKw/4vEi6Sq7N9/8KDO4P2Mo=";
          };
          extraPkgs = p:
            with p; [
              alsa-lib
            ];
        };
      in
        pkgs.writeShellScriptBin "handy" ''
          export WEBKIT_DISABLE_DMABUF_RENDERER=1
          exec ${appimage}/bin/handy-appimage-unwrapped "$@"
        '';

      default = self.packages.${system}.handy-appimage;
    });

    # Development shell for building from source
    devShells = forAllSystems (system: let
      pkgs = nixpkgs.legacyPackages.${system};
    in {
      default = pkgs.mkShell {
        buildInputs = with pkgs; [
          # Rust
          rustc
          cargo
          rust-analyzer
          clippy
          # Frontend
          nodejs
          bun
          # Native deps
          pkg-config
          openssl
          alsa-lib
          libsoup_3
          webkitgtk_4_1
          gtk3
          glib
          # Tauri CLI
          cargo-tauri
        ];

        shellHook = ''
          echo "Handy development environment"
          bun install
          echo "Run 'bun run tauri dev' to start"
        '';
      };
    });
  };
}
